import type { HubDO } from "./hub";
import type { StreamDO } from "./stream";

export interface Env {
  CF_REALTIME_APP_ID: string;
  CF_REALTIME_APP_SECRET: string;
  /** Secrets do Worker; ausente mantém Cloudflare. Nunca aceitar seleção do cliente. */
  SFU_PROVIDER?: string;
  SFU_FAILOVER_ENABLED?: string;
  SFU_ADMIN_USER_IDS?: string;
  SFU_MONTHLY_QUOTA_BYTES?: string;
  SFU_FAILOVER_THRESHOLD?: string;
  SFU_USAGE_BITRATE_BPS?: string;
  /** Só habilitar após calibração documentada da estimativa. */
  SFU_USAGE_CALIBRATED?: string;
  SFU_CYCLE_DAY?: string;
  LIVEKIT_URL?: string;
  LIVEKIT_API_KEY?: string;
  LIVEKIT_API_SECRET?: string;
  /** HMAC das sessões. Criado uma vez como secret_text; nunca exibido. */
  SESSION_SECRET: string;
  HUB: DurableObjectNamespace<HubDO>;
  STREAM: DurableObjectNamespace<StreamDO>;
  DISCORD_CLIENT_ID?: string;
  DISCORD_CLIENT_SECRET?: string;
  GOOGLE_CLIENT_ID?: string;
  GOOGLE_CLIENT_SECRET?: string;
  TURN_KEY_ID?: string;
  TURN_KEY_API_TOKEN?: string;
  /** URL HTTPS pública do instalador Windows; nunca uma credencial. */
  APP_DOWNLOAD_URL?: string;
}

type ApiStatus = 400 | 401 | 403 | 404 | 409 | 410 | 429 | 502 | 503;

/**
 * Erro de API com status e código estáveis. A mensagem leva o prefixo `@api:` porque erros
 * lançados dentro de um Durable Object chegam ao Worker só com `message`; `asApiError` recompõe.
 */
export class ApiError extends Error {
  readonly userMessage: string;
  constructor(
    readonly status: ApiStatus,
    readonly code: string,
    message: string,
  ) {
    super(`@api:${status}:${code}:${message}`);
    this.userMessage = message;
  }
}

export function asApiError(error: unknown): ApiError | null {
  if (error instanceof ApiError) return error;
  if (error instanceof Error) {
    const match = /@api:(\d{3}):([a-z_]+):([\s\S]*)$/.exec(error.message);
    if (match) return new ApiError(Number(match[1]) as ApiStatus, match[2] ?? "erro", match[3] ?? "");
  }
  return null;
}

export function json(value: unknown, status = 200): Response {
  return Response.json(value, { status, headers: { "Cache-Control": "no-store" } });
}

export function errorResponse(error: unknown): Response {
  const api = asApiError(error);
  if (api) {
    // Só diagnóstico do SFU (status HTTP e código/descrição curtos), nunca SDP nem credenciais.
    const sfu = api.code === "sfu_erro" ? { upstream: (api as { upstreamStatus?: number }).upstreamStatus, detail: (api as { detail?: string }).detail } : {};
    return json({ error: api.code, message: api.userMessage, ...sfu }, api.status);
  }
  // Never reflect an exception: upstream failures may contain credentials or SDP.
  return json({ error: "erro_interno", message: "Não foi possível concluir a operação." }, 502);
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function invalidBody(): never {
  throw new ApiError(400, "corpo_invalido", "Corpo da requisição inválido.");
}

export function textField(value: unknown, max = 128): string {
  if (typeof value !== "string" || !value.trim() || value.length > max) invalidBody();
  return value;
}

export function integerField(value: unknown, min = 0, max = Number.MAX_SAFE_INTEGER): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < min || value > max) invalidBody();
  return value;
}

export function sdpField(value: unknown): string {
  const sdp = textField(value, 65_536);
  if (new TextEncoder().encode(sdp).byteLength > 65_536) invalidBody();
  return sdp;
}

export async function readJson(request: Request): Promise<Record<string, unknown>> {
  if (request.headers.get("Content-Type")?.split(";")[0]?.trim().toLowerCase() !== "application/json") invalidBody();
  // SDP may use JSON escapes; limit the stream as well as the decoded SDP.
  const reader = request.body?.getReader();
  if (!reader) invalidBody();
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      length += value.byteLength;
      if (length > 524_288) {
        await reader.cancel();
        invalidBody();
      }
      chunks.push(value);
    }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.byteLength;
    }
    const value: unknown = JSON.parse(new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(bytes));
    if (!isRecord(value)) invalidBody();
    return value;
  } catch (error) {
    if (error instanceof ApiError) throw error;
    invalidBody();
  } finally {
    reader.releaseLock();
  }
}
