import { isRecord, sdpField, type Env } from "../common";
import { SfuError, trackStats, type SfuTrack, type SdpResult, type SfuProvider, type VideoLayer, type IceServer, type SfuStats } from "./types";

const BASE_URL = "https://rtc.live.cloudflare.com/v1";

function hasError(value: Record<string, unknown>): boolean {
  return (value.errorCode !== undefined && value.errorCode !== null && value.errorCode !== "") ||
    (value.errorDescription !== undefined && value.errorDescription !== null && value.errorDescription !== "");
}

async function requestJson(url: string, secret: string, method: string, body?: unknown): Promise<Record<string, unknown>> {
  if (!secret) throw new SfuError(0, "sem-secret");
  try {
    const init: RequestInit = {
      method,
      headers: { Authorization: `Bearer ${secret}`, "Content-Type": "application/json" },
      redirect: "manual",
      signal: AbortSignal.timeout(6_000),
    };
    if (body !== undefined) init.body = JSON.stringify(body);
    const response = await fetch(url, init);
    if (!response.ok) {
      // Só código/descrição de erro do SFU (sem SDP nem credenciais), truncado.
      let detail = "";
      try {
        const upstream: unknown = await response.json();
        if (isRecord(upstream)) detail = `${String(upstream.errorCode ?? "")} ${String(upstream.errorDescription ?? "")}`.trim().slice(0, 200);
      } catch { /* sem corpo JSON */ }
      throw new SfuError(response.status, detail);
    }
    const result: unknown = await response.json();
    if (!isRecord(result)) throw new SfuError(response.status);
    if (hasError(result)) throw new SfuError(response.status, `${String(result.errorCode ?? "")} ${String(result.errorDescription ?? "")}`.trim().slice(0, 200));
    if (result.tracks !== undefined) {
      if (!Array.isArray(result.tracks)) throw new SfuError(0, "sfu-L54");
      for (const track of result.tracks) {
        if (!isRecord(track)) throw new SfuError(0, "faixa-invalida");
        if (hasError(track)) throw new SfuError(response.status, `faixa ${String(track.errorCode ?? "")} ${String(track.errorDescription ?? "")}`.trim().slice(0, 200));
      }
    }
    return result;
  } catch (error) {
    if (error instanceof SfuError) throw error;
    throw new SfuError(0, "sfu-L62");
  }
}

function tracksFrom(value: unknown): SfuTrack[] {
  if (!Array.isArray(value)) throw new SfuError(0, "sfu-L67");
  return value.map((track: unknown) => {
    if (!isRecord(track) || typeof track.mid !== "string" || !track.mid) throw new SfuError(0, "sfu-L69");
    const result: SfuTrack = { mid: track.mid };
    if (typeof track.status === "string") result.status = track.status;
    return result;
  });
}

function trackResult(result: Record<string, unknown>, type: "offer" | "answer", expected = 1): SdpResult {
  const description = result.sessionDescription;
  if (!isRecord(description) || description.type !== type) throw new SfuError(0, "sfu-L78");
  if (type === "offer" && result.requiresImmediateRenegotiation !== true) throw new SfuError(0, "sfu-L79");
  const tracks = tracksFrom(result.tracks);
  if (tracks.length !== expected) throw new SfuError(0, "sfu-L81");
  try {
    return { signaling: "sdp", sdp: sdpField(description.sdp), tracks };
  } catch {
    throw new SfuError(0, "sfu-L85");
  }
}

export class CloudflareProvider implements SfuProvider {
  readonly id = "cloudflare" as const;
  constructor(private readonly env: Env) {}

  async healthCheck(): Promise<void> {
    // Sem tracks/mídia: a sessão vazia expira no SFU; a API não expõe delete de sessão.
    await this.newSession();
  }

  iceServers(): Promise<{ iceServers: IceServer[] }> { return iceServers(this.env); }

  async selectLayer(_sessionId: string, layer: VideoLayer): Promise<void> {
    if (layer !== "high") throw new SfuError(0, "camada-indisponivel");
  }

  private request(path: string, method: string, body?: unknown): Promise<Record<string, unknown>> {
    if (!this.env.CF_REALTIME_APP_ID) throw new SfuError(0, "sfu-L93");
    return requestJson(`${BASE_URL}/apps/${encodeURIComponent(this.env.CF_REALTIME_APP_ID)}${path}`,
      this.env.CF_REALTIME_APP_SECRET, method, body);
  }

  private sessionPath(sessionId: string): string {
    return `/sessions/${encodeURIComponent(sessionId)}`;
  }

  async newSession(): Promise<string> {
    const result = await this.request("/sessions/new", "POST");
    if (typeof result.sessionId !== "string" || !result.sessionId) throw new SfuError(0, "sfu-L104");
    return result.sessionId;
  }

  /** Publica o vídeo e, se houver `audioMid`, a faixa de áudio na mesma sessão e negociação. */
  async publish(sessionId: string, offer: string, mid: string, audioMid?: string): Promise<SdpResult> {
    const tracks: Record<string, string>[] = [{ location: "local", mid, trackName: "screen" }];
    if (audioMid) tracks.push({ location: "local", mid: audioMid, trackName: "audio" });
    return trackResult(await this.request(`${this.sessionPath(sessionId)}/tracks/new`, "POST", {
      sessionDescription: { type: "offer", sdp: offer },
      tracks,
    }), "answer", tracks.length);
  }

  async subscribe(sessionId: string, publisherId: string, audio = false): Promise<SdpResult> {
    const tracks: Record<string, string>[] = [{ location: "remote", sessionId: publisherId, trackName: "screen" }];
    if (audio) tracks.push({ location: "remote", sessionId: publisherId, trackName: "audio" });
    return trackResult(await this.request(`${this.sessionPath(sessionId)}/tracks/new`, "POST", { tracks }), "offer", tracks.length);
  }

  async renegotiate(sessionId: string, answer: string): Promise<void> {
    await this.request(`${this.sessionPath(sessionId)}/renegotiate`, "PUT", {
      sessionDescription: { type: "answer", sdp: answer },
    });
  }

  async inspect(sessionId: string): Promise<SfuTrack[]> {
    return tracksFrom((await this.request(this.sessionPath(sessionId), "GET")).tracks);
  }

  async stats(sessionId: string): Promise<SfuStats> { return trackStats(await this.inspect(sessionId)); }

  async close(sessionId: string, mids: string[]): Promise<void> {
    if (!mids.length) return;
    const result = await this.request(`${this.sessionPath(sessionId)}/tracks/close`, "PUT", {
      tracks: mids.map((mid) => ({ mid })), force: true,
    });
    const tracks = tracksFrom(result.tracks);
    if (mids.some((mid) => !tracks.some((track) => track.mid === mid))) throw new SfuError(0, "sfu-L137");
  }
}

export async function iceServers(env: Env): Promise<{ iceServers: IceServer[] }> {
  if (!env.TURN_KEY_ID || !env.TURN_KEY_API_TOKEN) {
    return { iceServers: [{ urls: "stun:stun.cloudflare.com:3478" }] };
  }
  const result = await requestJson(
    `${BASE_URL}/turn/keys/${encodeURIComponent(env.TURN_KEY_ID)}/credentials/generate-ice-servers`,
    env.TURN_KEY_API_TOKEN, "POST", { ttl: 86400 },
  );
  const servers: unknown[] = Array.isArray(result.iceServers) ? result.iceServers : [result.iceServers];
  const parsed = servers.map((server): IceServer => {
    if (!isRecord(server)) throw new SfuError(0, "sfu-L151");
    const urls = server.urls;
    if (!(typeof urls === "string" && urls.length > 0) &&
      !(Array.isArray(urls) && urls.length > 0 && urls.every((url: unknown) => typeof url === "string" && url.length > 0))) throw new SfuError(0, "sfu-L154");
    // Navegadores bloqueiam a porta 53; sem trickle ICE ela também atrasa a coleta.
    const filtered = (typeof urls === "string" ? [urls] : urls as string[])
      .filter((url) => !/:53(?:[/?#]|$)/.test(url));
    const value: IceServer = { urls: filtered };
    if (server.username !== undefined) {
      if (typeof server.username !== "string") throw new SfuError(0, "sfu-L157");
      value.username = server.username;
    }
    if (server.credential !== undefined) {
      if (typeof server.credential !== "string") throw new SfuError(0, "sfu-L161");
      value.credential = server.credential;
    }
    return value;
  }).filter((server) => Array.isArray(server.urls) && server.urls.length > 0);
  if (!parsed.length) throw new SfuError(0, "sfu-L166");
  return { iceServers: parsed };
}
