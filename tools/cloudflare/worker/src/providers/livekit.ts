import { ApiError, isRecord, type Env } from "../common";
import { SfuError, trackStats, type SfuStats, type LiveKitConnection, type SessionContext, type SfuProvider, type SfuTrack, type VideoLayer } from "./types";

function base64url(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** HS256 via Web Crypto. Nenhuma chave administrativa sai do Worker. */
async function token(env: Env, claims: Record<string, unknown>): Promise<string> {
  const encode = (value: unknown): string => base64url(new TextEncoder().encode(JSON.stringify(value)));
  const now = Math.floor(Date.now() / 1000);
  const data = `${encode({ alg: "HS256", typ: "JWT" })}.${encode({ iss: env.LIVEKIT_API_KEY, nbf: now - 5, exp: now + 60, ...claims })}`;
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(env.LIVEKIT_API_SECRET), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return `${data}.${base64url(new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data))))}`;
}

/** IDs internos aleatórios, sem token nem nome de usuário. */
function session(value: string): { room: string; identity: string } {
  const [room, identity, extra] = value.split("/");
  if (!room || !/^ms-[0-9a-f-]{36}$/.test(room) || !identity || !/^[pv]-[0-9a-f-]{36}$/.test(identity) || extra !== undefined) throw new SfuError(0, "sessao-invalida");
  return { room, identity };
}

export class LiveKitProvider implements SfuProvider {
  readonly id = "livekit" as const;
  private readonly url: URL;
  constructor(private readonly env: Env) {
    try {
      this.url = new URL(env.LIVEKIT_URL ?? "");
      if (this.url.protocol !== "wss:" || this.url.username || this.url.password || this.url.search || this.url.hash || this.url.pathname !== "/" || !env.LIVEKIT_API_KEY || !env.LIVEKIT_API_SECRET) throw new Error();
    } catch {
      throw new ApiError(503, "sfu_configuracao", "O LiveKit não está configurado no servidor.");
    }
  }

  private async rpc(method: string, room: string, body: Record<string, unknown>, timeoutMs = 6000): Promise<Record<string, unknown>> {
    const auth = await token(this.env, { video: { room, roomAdmin: true, roomCreate: true } });
    const url = new URL(`/twirp/livekit.RoomService/${method}`, this.url);
    url.protocol = "https:";
    try {
      const response = await fetch(url, {
        method: "POST", redirect: "manual", signal: AbortSignal.timeout(timeoutMs),
        headers: { Authorization: `Bearer ${auth}`, "Content-Type": "application/json" }, body: JSON.stringify(body),
      });
      if (!response.ok) throw new SfuError(response.status, `livekit-${method}`);
      const result: unknown = await response.json();
      if (!isRecord(result)) throw new SfuError(0, "livekit-resposta-invalida");
      return result;
    } catch (error) {
      if (error instanceof SfuError) throw error;
      throw new SfuError(0, `livekit-${method}`);
    }
  }

  async newSession(context: SessionContext): Promise<string> {
    if (context.role === "viewer") {
      if (!context.publisherId) throw new SfuError(0, "publicador-ausente");
      return `${session(context.publisherId).room}/v-${crypto.randomUUID()}`;
    }
    // Criada no primeiro join. Persistir o ID antes de efeitos remotos permite limpeza
    // também quando uma emissão de token não chega ao cliente.
    return `ms-${crypto.randomUUID()}/p-${crypto.randomUUID()}`;
  }

  async healthCheck(): Promise<void> {
    const id = await this.newSession({ role: "publisher" });
    const { room } = session(id);
    // Sala própria, sem join/token de cliente: não interfere em transmissões.
    await this.rpc("CreateRoom", room, { name: room, empty_timeout: 10, max_participants: 1 }, 3000);
    await this.rpc("DeleteRoom", room, { room }, 3000);
  }

  private async connection(sessionId: string, publisher: boolean, audio: boolean): Promise<LiveKitConnection> {
    const { room, identity } = session(sessionId);
    const video = {
      room, roomJoin: true, canPublish: publisher, canSubscribe: !publisher, canPublishData: false,
      ...(publisher ? { canPublishSources: audio ? ["screen_share", "screen_share_audio"] : ["screen_share"] } : {}),
    };
    return {
      signaling: "livekit", url: this.url.toString(), room, identity,
      token: await token(this.env, { sub: identity, video }),
      tracks: [{ mid: "screen" }, ...(audio ? [{ mid: "audio" }] : [])],
    };
  }

  async publish(sessionId: string, _offer: string, _mid: string, audioMid?: string): Promise<LiveKitConnection> {
    const { room } = session(sessionId);
    // Confirma disponibilidade/credenciais antes de anunciar "ao vivo" e limita a sala.
    await this.rpc("CreateRoom", room, { name: room, empty_timeout: 60, departure_timeout: 20, max_participants: 9 });
    return this.connection(sessionId, true, audioMid !== undefined);
  }

  async subscribe(sessionId: string, publisherId: string, audio = false): Promise<LiveKitConnection> {
    if (session(sessionId).room !== session(publisherId).room) throw new SfuError(0, "sala-invalida");
    return { ...await this.connection(sessionId, false, audio), publisherIdentity: session(publisherId).identity };
  }

  async renegotiate(_sessionId: string, _answer: string): Promise<void> {
    throw new ApiError(409, "sinalizacao_sdk", "Esta conexão usa a negociação do LiveKit.");
  }

  async inspect(sessionId: string): Promise<SfuTrack[]> {
    const { room, identity } = session(sessionId);
    const result = await this.rpc("GetParticipant", room, { room, identity });
    if (result.tracks === undefined) return [];
    if (!Array.isArray(result.tracks)) throw new SfuError(0, "livekit-faixas-invalidas");
    return result.tracks.map((track: unknown) => {
      if (!isRecord(track) || typeof track.sid !== "string") throw new SfuError(0, "livekit-faixa-invalida");
      return { mid: track.sid };
    });
  }

  async close(sessionId: string, _mids: string[]): Promise<void> {
    const { room, identity } = session(sessionId);
    if (identity.startsWith("p-")) await this.rpc("DeleteRoom", room, { room });
    else await this.rpc("RemoveParticipant", room, { room, identity });
  }

  async stats(sessionId: string): Promise<SfuStats> { return trackStats(await this.inspect(sessionId)); }

  async iceServers(): Promise<{ iceServers: [] }> { return { iceServers: [] }; }

  async selectLayer(_sessionId: string, layer: VideoLayer): Promise<void> {
    if (layer !== "high") throw new ApiError(409, "camada_indisponivel", "Esta transmissão publica uma única camada de vídeo.");
  }
}
