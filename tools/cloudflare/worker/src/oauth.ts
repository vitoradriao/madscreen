import { ApiError, isRecord, type Env } from "./common";
import type { OAuthProfile } from "./hub";

export type ProviderId = "discord" | "google";

interface Provider {
  authorize: string;
  token: string;
  scope: string;
  credentials(env: Env): { id: string; secret: string } | null;
  profile(accessToken: string): Promise<OAuthProfile>;
}

function text(value: unknown, max = 100): string {
  return typeof value === "string" ? value.slice(0, max) : "";
}

async function getJson(url: string, accessToken: string): Promise<Record<string, unknown>> {
  const response = await fetch(url, {
    headers: { Authorization: `Bearer ${accessToken}`, "User-Agent": "DiscordBot (https://github.com/vitoradriao/madscreen, 1)" },
    redirect: "manual", signal: AbortSignal.timeout(10_000),
  });
  if (!response.ok) {
    await response.body?.cancel();
    throw new ApiError(502, "provedor_erro", "O provedor de login não respondeu. Tente novamente.");
  }
  const value: unknown = await response.json();
  if (!isRecord(value)) throw new ApiError(502, "provedor_erro", "Resposta inesperada do provedor de login.");
  return value;
}

/** Avatar do Discord: o enviado pelo usuario ou, sem ele, o padrao do Discord (indice pelo id, como o proprio Discord calcula). */
export function discordAvatar(id: string, hash: string): string {
  if (/^[a-z0-9_]+$/i.test(hash)) return `https://cdn.discordapp.com/avatars/${id}/${hash}.png?size=128`;
  return `https://cdn.discordapp.com/embed/avatars/${Number((BigInt(id) >> 22n) % 6n)}.png`;
}

/** Aceita so imagens de hosts permitidos pela CSP; qualquer outra coisa gravada antes vira null e o app mostra as iniciais. */
export function safeAvatar(url: string | null): string | null {
  if (!url) return null;
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "https:") return null;
    if (parsed.hostname === "cdn.discordapp.com") {
      if (parsed.pathname.startsWith("/avatars/")) parsed.search = "?size=128";
      return parsed.toString();
    }
    return parsed.hostname === "lh3.googleusercontent.com" ? parsed.toString() : null;
  } catch {
    return null;
  }
}

const PROVIDERS: Record<ProviderId, Provider> = {
  discord: {
    authorize: "https://discord.com/oauth2/authorize",
    token: "https://discord.com/api/oauth2/token",
    scope: "identify",
    credentials: (env) => env.DISCORD_CLIENT_ID && env.DISCORD_CLIENT_SECRET ? { id: env.DISCORD_CLIENT_ID, secret: env.DISCORD_CLIENT_SECRET } : null,
    async profile(accessToken) {
      const user = await getJson("https://discord.com/api/users/@me", accessToken);
      const sub = text(user.id, 32);
      if (!/^\d{5,32}$/.test(sub)) throw new ApiError(502, "provedor_erro", "Resposta inesperada do provedor de login.");
      const avatar = text(user.avatar, 64);
      return {
        sub,
        username: text(user.username, 64) || `discord${sub.slice(-4)}`,
        displayName: text(user.global_name, 40) || text(user.username, 40),
        avatar: discordAvatar(sub, avatar),
      };
    },
  },
  google: {
    authorize: "https://accounts.google.com/o/oauth2/v2/auth",
    token: "https://oauth2.googleapis.com/token",
    scope: "openid profile",
    credentials: (env) => env.GOOGLE_CLIENT_ID && env.GOOGLE_CLIENT_SECRET ? { id: env.GOOGLE_CLIENT_ID, secret: env.GOOGLE_CLIENT_SECRET } : null,
    async profile(accessToken) {
      const user = await getJson("https://openidconnect.googleapis.com/v1/userinfo", accessToken);
      const sub = text(user.sub, 64);
      if (!/^\d{5,64}$/.test(sub)) throw new ApiError(502, "provedor_erro", "Resposta inesperada do provedor de login.");
      const name = text(user.name, 40) || text(user.given_name, 40) || `google${sub.slice(-4)}`;
      const picture = text(user.picture, 300);
      return {
        sub, username: name, displayName: name,
        avatar: picture.startsWith("https://") ? picture : null,
      };
    },
  },
};

export function providerId(value: string): ProviderId {
  if (value === "discord" || value === "google") return value;
  throw new ApiError(404, "rota_nao_encontrada", "Rota não encontrada.");
}

export function configured(env: Env): Record<ProviderId, boolean> {
  return { discord: PROVIDERS.discord.credentials(env) !== null, google: PROVIDERS.google.credentials(env) !== null };
}

export function redirectUri(origin: string, id: ProviderId): string {
  return `${origin}/auth/${id}/callback`;
}

export function authorizeUrl(env: Env, id: ProviderId, origin: string, state: string): string {
  const provider = PROVIDERS[id];
  const credentials = provider.credentials(env);
  if (!credentials) throw new ApiError(503, "provedor_nao_configurado", "Este login ainda não foi configurado.");
  const url = new URL(provider.authorize);
  url.searchParams.set("client_id", credentials.id);
  url.searchParams.set("response_type", "code");
  url.searchParams.set("redirect_uri", redirectUri(origin, id));
  url.searchParams.set("scope", provider.scope);
  url.searchParams.set("state", state);
  if (id === "discord") url.searchParams.set("prompt", "none");
  return url.toString();
}

export async function fetchProfile(env: Env, id: ProviderId, origin: string, code: string): Promise<OAuthProfile> {
  const provider = PROVIDERS[id];
  const credentials = provider.credentials(env);
  if (!credentials) throw new ApiError(503, "provedor_nao_configurado", "Este login ainda não foi configurado.");
  const response = await fetch(provider.token, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded", Accept: "application/json" },
    body: new URLSearchParams({
      client_id: credentials.id, client_secret: credentials.secret, grant_type: "authorization_code",
      code, redirect_uri: redirectUri(origin, id),
    }),
    redirect: "manual", signal: AbortSignal.timeout(10_000),
  });
  if (!response.ok) {
    await response.body?.cancel();
    throw new ApiError(400, "codigo_invalido", "O login foi recusado pelo provedor. Tente novamente.");
  }
  const token: unknown = await response.json();
  if (!isRecord(token) || typeof token.access_token !== "string") throw new ApiError(502, "provedor_erro", "Resposta inesperada do provedor de login.");
  return provider.profile(token.access_token);
}
