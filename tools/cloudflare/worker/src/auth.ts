import { ApiError, integerField, isRecord, textField, type Env } from "./common";

/** Sessão assinada (HMAC-SHA256 com SESSION_SECRET). O Hub confere conta e revogacao persistente da linhagem. */
export interface SessionPayload {
  v: 1;
  j?: string;
  /** id do usuário */
  u: string;
  iat: number;
  exp: number;
}
export interface Principal {
  uid: string;
  sessionId: string;
  iat: number;
  exp: number;
}

export const SESSION_TTL = 90 * 86400;
const encoder = new TextEncoder();

export function base64urlEncode(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function base64urlDecode(value: string): Uint8Array {
  if (!/^[A-Za-z0-9_-]+$/.test(value) || value.length % 4 === 1) throw invalidToken();
  try {
    const binary = atob(value.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - value.length % 4) % 4));
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
    if (base64urlEncode(bytes) !== value) throw invalidToken();
    return bytes;
  } catch {
    throw invalidToken();
  }
}

export function constantTimeEqual(left: Uint8Array, right: Uint8Array): boolean {
  let difference = left.length ^ right.length;
  for (let index = 0; index < Math.max(left.length, right.length); index++) {
    difference |= (left[index] ?? 0) ^ (right[index] ?? 0);
  }
  return difference === 0;
}

export async function sha256(value: string): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest("SHA-256", encoder.encode(value)));
}

export function hex(bytes: Uint8Array): string {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function hmac(key: string, payload: string): Promise<Uint8Array> {
  if (!key) throw new ApiError(503, "configuracao_invalida", "Assinatura de sessões indisponível.");
  const cryptoKey = await crypto.subtle.importKey(
    "raw", encoder.encode(key), { name: "HMAC", hash: "SHA-256" }, false, ["sign"],
  );
  return new Uint8Array(await crypto.subtle.sign("HMAC", cryptoKey, encoder.encode(payload)));
}

function invalidToken(): ApiError {
  return new ApiError(401, "sessao_invalida", "Sessão inválida ou expirada. Entre novamente.");
}

export async function signSession(uid: string, secret: string, now = Math.floor(Date.now() / 1000), sessionId = crypto.randomUUID()): Promise<{ token: string; exp: number }> {
  const payload: SessionPayload = { v: 1, j: sessionId, u: uid, iat: now, exp: now + SESSION_TTL };
  const encoded = base64urlEncode(encoder.encode(JSON.stringify(payload)));
  return { token: `${encoded}.${base64urlEncode(await hmac(secret, encoded))}`, exp: payload.exp };
}

export async function verifySession(token: string, secret: string): Promise<Principal> {
  if (token.length > 1024) throw invalidToken();
  const parts = token.split(".");
  const [payload, signature] = parts;
  if (parts.length !== 2 || !payload || !signature) throw invalidToken();
  const expected = await hmac(secret, payload);
  if (!constantTimeEqual(expected, base64urlDecode(signature))) throw invalidToken();
  try {
    const value: unknown = JSON.parse(new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(base64urlDecode(payload)));
    if (!isRecord(value) || value.v !== 1) throw invalidToken();
    const principal: Principal = { sessionId: typeof value.j === "string" ? textField(value.j, 128) : `legacy:${String(value.u)}:${String(value.iat)}`, uid: textField(value.u, 64), iat: integerField(value.iat, 1), exp: integerField(value.exp, 1) };
    if (principal.exp <= Math.floor(Date.now() / 1000)) throw invalidToken();
    return principal;
  } catch {
    throw invalidToken();
  }
}

export function bearerToken(request: Request): string {
  const match = /^Bearer ([^\s]+)$/i.exec(request.headers.get("Authorization") ?? "");
  if (!match?.[1]) throw invalidToken();
  return match[1];
}

/** `Authorization: Bearer <sessão>` ou, em WebSocket, subprotocolo `ms.<sessão>`. */
export async function authenticate(request: Request, env: Env): Promise<Principal> {
  const protocol = request.headers.get("Sec-WebSocket-Protocol");
  let token: string;
  if (protocol && request.headers.get("Upgrade")?.toLowerCase() === "websocket") {
    const chosen = protocol.split(",").map((item) => item.trim()).find((item) => item.startsWith("ms."));
    if (!chosen) throw invalidToken();
    token = chosen.slice(3);
  } else token = bearerToken(request);
  const principal = await verifySession(token, env.SESSION_SECRET);
  const digest = hex(await sha256(principal.sessionId));
  await env.HUB.get(env.HUB.idFromName("hub")).assertSession(principal.uid, digest, principal.iat);
  return principal;
}

const ALPHABET = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

export function randomCode(length: number): string {
  const bytes = crypto.getRandomValues(new Uint8Array(length));
  return [...bytes].map((byte) => ALPHABET[byte % ALPHABET.length]).join("");
}

export function randomToken(bytes = 32): string {
  return base64urlEncode(crypto.getRandomValues(new Uint8Array(bytes)));
}
