import { authenticate, hex, base64urlEncode, sha256, signSession } from "./auth";
import { ApiError, errorResponse, invalidBody, json, readJson, textField, asApiError, type Env } from "./common";
import { authorizeUrl, configured, fetchProfile, providerId } from "./oauth";
import { selectedProvider, iceServers } from "./sfu";
import { webAssets } from "./web.generated";

export { HubDO } from "./hub";
export { StreamDO } from "./stream";

const WEB_PATHS = new Set(["/", "/watch", "/login", "/cf.html"]);
const OWNER_ID = /^u_[0-9a-f]{16}$/;

function hub(env: Env) {
  return env.HUB.get(env.HUB.idFromName("hub"));
}

function clientIp(request: Request): string {
  return request.headers.get("CF-Connecting-IP") ?? "local";
}

function allowedOrigin(request: Request): string | null {
  const origin = request.headers.get("Origin");
  if (origin === null) return null;
  if (origin === new URL(request.url).origin) return origin;
  // WebView do app (Tauri v2: tauri.localhost no Windows, tauri://localhost nos demais) e desenvolvimento local.
  if (origin === "http://tauri.localhost" || origin === "https://tauri.localhost" || origin === "tauri://localhost") return origin;
  try {
    const url = new URL(origin);
    if (url.origin === origin && url.protocol === "http:" && (url.hostname === "127.0.0.1" || url.hostname === "localhost")) return origin;
  } catch {
    // Origem malformada ou opaca é recusada.
  }
  throw new ApiError(403, "origem_negada", "Origem não autorizada.");
}

function secure(response: Response, origin: string | null): Response {
  if (response.status === 101) return response;
  const result = new Response(response.body, response);
  result.headers.set("Vary", "Origin");
  result.headers.set("X-Content-Type-Options", "nosniff");
  result.headers.set("Referrer-Policy", "no-referrer");
  if (!result.headers.has("X-Frame-Options")) result.headers.set("X-Frame-Options", "DENY");
  if (origin) {
    result.headers.set("Access-Control-Allow-Origin", origin);
    result.headers.set("Access-Control-Allow-Methods", "GET, POST, PATCH, DELETE, OPTIONS");
    result.headers.set("Access-Control-Allow-Headers", "Authorization, Content-Type");
    result.headers.set("Access-Control-Max-Age", "600");
  }
  return result;
}

function bytesFromBase64(value: string): Uint8Array {
  return Uint8Array.from(atob(value), (char) => char.charCodeAt(0));
}

function staticAsset(url: URL, env: Env): Response | null {
  const path = url.pathname;
  let mediaOrigins = "";
  if (env.LIVEKIT_URL) {
    try {
      const upstream = new URL(env.LIVEKIT_URL);
      if (upstream.protocol === "wss:" && !upstream.username && !upstream.password && !upstream.search && !upstream.hash && upstream.pathname === "/") {
        mediaOrigins = `${upstream.origin} ${upstream.origin.replace(/^wss:/, "https:")}`;
      }
    } catch { /* Configuração reserva inválida não impede o viewer Cloudflare. */ }
  }
  const csp = webAssets.csp.replace("WSORIGIN", `wss://${url.host}`).replace("SFUORIGINS", mediaOrigins);
  const page = { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store", "Content-Security-Policy": csp };
  if (WEB_PATHS.has(path) || path.startsWith("/i/")) return new Response(webAssets.html, { headers: page });
  if (path === "/web.js") return new Response(webAssets.js, { headers: { "Content-Type": "text/javascript; charset=utf-8", "Cache-Control": "no-store" } });
  if (path === "/web.css") return new Response(webAssets.css, { headers: { "Content-Type": "text/css; charset=utf-8", "Cache-Control": "no-store" } });
  if (path === "/fonts/inter.woff2") {
    return new Response(bytesFromBase64(webAssets.fontBase64), { headers: { "Content-Type": "font/woff2", "Cache-Control": "public, max-age=31536000, immutable" } });
  }
  return null;
}

function notice(title: string, message: string, status = 200): Response {
  const body = `<!doctype html><html lang="pt-BR"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${title}</title>
<link rel="stylesheet" href="/web.css"><style>
body{margin:0;min-height:100vh;display:grid;place-items:center;background:var(--bg);color:var(--text);font:var(--fw-regular) var(--fs-lg)/1.5 var(--font)}
main{max-width:420px;padding:var(--sp-6);border:1px solid var(--border);border-radius:var(--r-lg);background:var(--surface-1)}
h1{font-size:var(--fs-2xl);font-weight:var(--fw-medium);margin:0 0 var(--sp-2)}p{color:var(--text-2);margin:0 0 var(--sp-5)}
a{display:inline-block;background:var(--accent);color:var(--accent-fg);padding:var(--sp-2) var(--sp-4);border-radius:var(--r-md);text-decoration:none;font-weight:var(--fw-medium)}</style>
<main><h1>${title}</h1><p>${message}</p><a href="/watch" id="go">Abrir no navegador</a></main>
<script>if(localStorage.getItem("ms.pkce"))location.replace("/watch")</script></html>`;
  return new Response(body, { status, headers: { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store", "Content-Security-Policy": "default-src 'none'; style-src 'self' 'unsafe-inline'; font-src 'self'; script-src 'unsafe-inline'; base-uri 'none'; form-action 'none'" } });
}

async function sessionFor(env: Env, uid: string): Promise<{ token: string; exp: number; user: unknown }> {
  const user = await hub(env).me(uid);
  const { token, exp } = await signSession(uid, env.SESSION_SECRET);
  return { token, exp, user };
}

async function challengeOf(verifier: unknown): Promise<string> {
  const value = textField(verifier, 128);
  if (value.length < 32 || !/^[A-Za-z0-9_-]+$/.test(value)) invalidBody();
  return base64urlEncode(await sha256(value));
}

async function authRoutes(request: Request, env: Env, url: URL): Promise<Response | null> {
  const method = request.method;
  const path = url.pathname;
  if (method === "GET" && path === "/auth/providers") return json({ ...configured(env), invite: true, firstUser: await hub(env).isEmpty() });
  const start = /^\/auth\/(discord|google)\/start$/.exec(path);
  if (method === "GET" && start?.[1]) {
    const id = providerId(start[1]);
    const challenge = url.searchParams.get("challenge") ?? "";
    if (!/^[A-Za-z0-9_-]{43}$/.test(challenge)) throw new ApiError(400, "corpo_invalido", "Pedido de login inválido.");
    if (!configured(env)[id]) {
      return notice("Login indisponível", `O login com ${id === "discord" ? "Discord" : "Google"} ainda não foi configurado. Entre com usuário e convite.`, 503);
    }
    const state = await hub(env).oauthBegin(id, challenge, url.searchParams.get("invite"));
    return Response.redirect(authorizeUrl(env, id, url.origin, state), 302);
  }
  const callback = /^\/auth\/(discord|google)\/callback$/.exec(path);
  if (method === "GET" && callback?.[1]) {
    const id = providerId(callback[1]);
    const state = url.searchParams.get("state") ?? "";
    const code = url.searchParams.get("code");
    if (url.searchParams.get("error") || !code || state.length > 64) {
      return notice("Login cancelado", "O login não foi concluído. Volte ao madscreen e tente novamente.", 400);
    }
    try {
      const profile = await fetchProfile(env, id, url.origin, code);
      await hub(env).oauthFinish(state, id, profile);
    } catch (error) {
      const api = asApiError(error);
      return notice("Não foi possível entrar", api?.userMessage ?? "Tente novamente em instantes.", api?.status ?? 502);
    }
    return notice("Tudo certo", "Login concluído. Pode voltar ao madscreen: ele já entra sozinho.");
  }
  if (method === "POST" && path === "/auth/exchange") {
    const body = await readJson(request);
    const uid = await hub(env).exchange(await challengeOf(body.verifier));
    if (!uid) return json({ pending: true }, 202);
    return json(await sessionFor(env, uid));
  }
  if (method === "POST" && path === "/auth/register") {
    const body = await readJson(request);
    const result = await hub(env).register(clientIp(request), body.username, body.code, body.displayName);
    return json({ ...await sessionFor(env, result.uid), recoveryKey: result.recoveryKey });
  }
  if (method === "POST" && path === "/auth/login") {
    const body = await readJson(request);
    const result = await hub(env).login(clientIp(request), body.username, body.recoveryKey);
    return json(await sessionFor(env, result.uid));
  }
  if (method === "GET" && path === "/api/invites/preview") {
    return json(await hub(env).previewInvite(clientIp(request), url.searchParams.get("code")));
  }
  return null;
}

function forward(request: Request, stub: { fetch(request: Request): Promise<Response> }, trusted: Record<string, string>): Promise<Response> {
  const headers = new Headers(request.headers);
  for (const name of [...headers.keys()]) if (name.toLowerCase().startsWith("x-ms-")) headers.delete(name);
  for (const [name, value] of Object.entries(trusted)) headers.set(name, value);
  const protocol = request.headers.get("Sec-WebSocket-Protocol");
  if (protocol) {
    const chosen = protocol.split(",").map((item) => item.trim()).find((item) => item.startsWith("ms."));
    if (chosen) headers.set("x-ms-protocol", chosen);
  }
  // Nunca encaminha o token adiante: o Hub/DO confiam só nos cabeçalhos x-ms-*.
  headers.delete("Authorization");
  headers.delete("Sec-WebSocket-Protocol");
  return stub.fetch(new Request(request, { headers }));
}

async function apiRoutes(request: Request, env: Env, url: URL): Promise<Response> {
  const method = request.method;
  const path = url.pathname;
  const principal = await authenticate(request, env);
  const uid = principal.uid;
  const mediaSession = { "x-ms-session": hex(await sha256(principal.sessionId)) };

  if (method === "POST" && path === "/api/logout") {
    const owners = await hub(env).revokeSession(uid, hex(await sha256(principal.sessionId)), principal.exp);
    await Promise.all(owners.map(owner => {
      const stream = env.STREAM.get(env.STREAM.idFromName(owner));
      return owner === uid ? stream.forceStop(owner) : stream.revokeViewer(uid);
    }));
    return json({ ok: true });
  }

  if (path === "/api/admin/sfu") {
    if (method === "GET") return json(await hub(env).sfuAdmin(uid));
    if (method === "POST") return json(await hub(env).sfuAdmin(uid, await readJson(request)));
  }

  if (method === "GET" && path === "/api/ws") {
    return forward(request, hub(env), { "x-ms-uid": uid });
  }
  if (path === "/api/me") {
    if (method === "GET") {
      const user = await hub(env).me(uid);
      const refresh = Math.floor(Date.now() / 1000) - principal.iat > 30 * 86400
        ? await signSession(uid, env.SESSION_SECRET, Math.floor(Date.now() / 1000), principal.sessionId) : null;
      return json({ user, ...(refresh ? { token: refresh.token, exp: refresh.exp } : {}) });
    }
    if (method === "PATCH") {
      const body = await readJson(request);
      return json({ user: await hub(env).updateMe(uid, body.displayName) });
    }
    if (method === "DELETE") {
      const owners = await hub(env).activeMediaOwners(uid);
      await hub(env).deleteMe(uid);
      await Promise.all(owners.map(owner => owner === uid
        ? env.STREAM.get(env.STREAM.idFromName(owner)).forceStop(uid)
        : env.STREAM.get(env.STREAM.idFromName(owner)).revokeViewer(uid)));
      return json({ ok: true });
    }
  }
  const changeDirectory = async (action: string, target: string, value?: unknown, memberId?: string): Promise<Response> => {
    const result = await hub(env).changeDirectory(uid, action, target, value, memberId);
    const cleanup = result.cleanup;
    if (cleanup) {
      await Promise.all([
        ...cleanup.stopOwners.map(owner => env.STREAM.get(env.STREAM.idFromName(owner)).forceStop(owner)),
        ...cleanup.revokeOwners.map(owner => env.STREAM.get(env.STREAM.idFromName(owner)).revokeViewer(action === "kick" ? textField(memberId, 64) : uid)),
      ]);
    }
    return json(result.snapshot);
  };
  if (path === "/api/servers") {
    if (method === "GET") return json(await hub(env).directorySnapshot(uid));
    if (method === "POST") return changeDirectory("create", "", (await readJson(request)).name);
  }
  const serverRoute = /^\/api\/servers\/([A-Za-z0-9_]{1,64})$/.exec(path);
  if (serverRoute?.[1]) {
    const serverId = serverRoute[1];
    if (method === "GET") {
      const snapshot = await hub(env).directorySnapshot(uid, serverId);
      return json({ server: snapshot.servers.find(server => server.id === serverId) });
    }
    if (method === "PATCH") return changeDirectory("rename", serverId, (await readJson(request)).name);
    if (method === "DELETE") return changeDirectory("remove", serverId);
  }
  const channelRoute = /^\/api\/servers\/([A-Za-z0-9_]{1,64})\/channels(?:\/([A-Za-z0-9_]{1,64}))?$/.exec(path);
  if (channelRoute?.[1]) {
    const serverId = channelRoute[1];
    const channelId = channelRoute[2];
    if (!channelId && method === "POST") return changeDirectory("channel_create", serverId, (await readJson(request)).name);
    const snapshot = await hub(env).directorySnapshot(uid, serverId);
    const channels = snapshot.servers.find(server => server.id === serverId)?.channels ?? [];
    if (!channelId && method === "GET") return json({ channels });
    if (channelId && !channels.some(channel => channel.id === channelId)) throw new ApiError(404, "canal_nao_encontrado", "Canal não encontrado neste servidor.");
    if (channelId && method === "PATCH") return changeDirectory("channel_rename", channelId, (await readJson(request)).name);
    if (channelId && method === "DELETE") return changeDirectory("channel_remove", channelId);
  }
  const memberRoute = /^\/api\/servers\/([A-Za-z0-9_]{1,64})\/members\/(me|u_[0-9a-f]{16})$/.exec(path);
  if (memberRoute?.[1] && memberRoute[2]) {
    if (method === "DELETE" && memberRoute[2] !== "me") return changeDirectory("kick", memberRoute[1], undefined, memberRoute[2]);
    if (method === "DELETE" && memberRoute[2] === "me") return changeDirectory("leave", memberRoute[1]);
    if (method === "PATCH" && memberRoute[2] !== "me") return changeDirectory("role", memberRoute[1], (await readJson(request)).role, memberRoute[2]);
  }
  const serverInvite = /^\/api\/servers\/([A-Za-z0-9_]{1,64})\/invites$/.exec(path);
  if (method === "POST" && serverInvite?.[1]) {
    const body = await readJson(request);
    const invite = await hub(env).createInvite(uid, serverInvite[1], body.channelId);
    return json({ ...invite, link: `${url.origin}/i/${invite.code}` });
  }
  if (method === "GET" && path === "/api/friends") return json(await hub(env).friends(uid));
  if (method === "POST" && path === "/api/friends/request") return json(await hub(env).requestFriend(uid, (await readJson(request)).username));
  if (method === "POST" && path === "/api/friends/accept") return json(await hub(env).answerRequest(uid, (await readJson(request)).id, true));
  if (method === "POST" && path === "/api/friends/decline") return json(await hub(env).answerRequest(uid, (await readJson(request)).id, false));
  const removal = /^\/api\/friends\/(u_[0-9a-f]{16})$/.exec(path);
  if (method === "DELETE" && removal?.[1]) return json(await hub(env).removeFriend(uid, removal[1]));
  if (method === "POST" && path === "/api/invites") {
    const body = await readJson(request);
    const invite = await hub(env).createInvite(uid, body.serverId, body.channelId);
    return json({ ...invite, link: `${url.origin}/i/${invite.code}` });
  }
  if (method === "POST" && path === "/api/invites/redeem") {
    return json(await hub(env).redeemInvite(uid, clientIp(request), (await readJson(request)).code));
  }
  if (method === "GET" && path === "/api/ice") return json(await iceServers(env));
  if (method === "GET" && path === "/api/sfu") return json({ provider: await selectedProvider(env), layers: ["h"] });

  const stream = (ownerId: string) => env.STREAM.get(env.STREAM.idFromName(ownerId));

  // Publicador: sempre a própria transmissão.
  if (method === "POST" && /^\/api\/share\/(start|heartbeat|stop)$/.test(path)) {
    const target = path === "/api/share/start" ? await hub(env).resolveShare(uid, (await readJson(request.clone())).channelId) : null;
    return forward(request, stream(uid), { ...mediaSession, "x-ms-uid": uid, "x-ms-name": target?.displayName ?? uid, "x-ms-owner": uid, ...(target ? { "x-ms-channel": target.channelId } : {}) });
  }

  // Espectador: só amigos (o Hub confere); depois do start o viewerId já carrega o dono.
  if (method === "POST" && path === "/api/watch/start") {
    const body = await readJson(request.clone());
    const allowed = await hub(env).canWatch(uid, textField(body.owner, 64));
    return forward(request, stream(allowed.ownerId), { ...mediaSession, "x-ms-uid": uid, "x-ms-name": allowed.viewerName, "x-ms-owner": allowed.ownerId });
  }
  if (method === "POST" && /^\/api\/watch\/(answer|heartbeat|leave|layer)$/.test(path)) {
    const body = await readJson(request.clone());
    const viewerId = textField(body.viewerId, 96);
    const owner = viewerId.split(".")[0] ?? "";
    if (!OWNER_ID.test(owner)) invalidBody();
    if (path !== "/api/watch/leave") {
      try { await hub(env).canWatch(uid, owner); }
      catch (error) {
        await forward(new Request(new URL("/api/watch/leave", url), { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ viewerId }) }), stream(owner), { ...mediaSession, "x-ms-uid": uid, "x-ms-name": uid, "x-ms-owner": owner }).catch(() => undefined);
        throw error;
      }
    }
    return forward(request, stream(owner), { ...mediaSession, "x-ms-uid": uid, "x-ms-name": uid, "x-ms-owner": owner });
  }
  if (method === "GET" && (path === "/api/stream/status" || path === "/api/stream/ws")) {
    const allowed = await hub(env).canWatch(uid, textField(url.searchParams.get("owner"), 64));
    return forward(request, stream(allowed.ownerId), { ...mediaSession, "x-ms-uid": uid, "x-ms-name": allowed.viewerName, "x-ms-owner": allowed.ownerId });
  }
  throw new ApiError(404, "rota_nao_encontrada", "Rota não encontrada.");
}

async function route(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  if (request.method === "OPTIONS") return new Response(null, { status: 204 });
  if (request.method === "GET") {
    if (url.pathname === "/health") return json({ ok: true, version: 3 });
    let download: URL | null = null;
    try { const configuredUrl = new URL(env.APP_DOWNLOAD_URL ?? ""); if (configuredUrl.protocol === "https:" && !configuredUrl.username && !configuredUrl.password) download = configuredUrl; } catch { /* Download ainda não configurado. */ }
    if (url.pathname === "/app/info") return json({ downloadAvailable: download !== null });
    if (url.pathname === "/download/windows") return download ? Response.redirect(download.href, 302)
      : notice("Instalador indisponível", "O download público ainda não foi configurado. Peça o instalador a quem convidou ou use o navegador.", 503);
    const asset = staticAsset(url, env);
    if (asset) return asset;
  }
  if (url.pathname.startsWith("/auth/") || url.pathname === "/api/invites/preview") {
    const response = await authRoutes(request, env, url);
    if (response) return response;
  } else if (url.pathname.startsWith("/api/")) {
    return apiRoutes(request, env, url);
  }
  throw new ApiError(404, "rota_nao_encontrada", "Rota não encontrada.");
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    let origin: string | null = null;
    try {
      origin = allowedOrigin(request);
      return secure(await route(request, env), origin);
    } catch (error) {
      return secure(errorResponse(error), origin);
    }
  },
} satisfies ExportedHandler<Env>;
