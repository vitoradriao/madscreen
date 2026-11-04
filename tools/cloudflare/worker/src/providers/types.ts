import { ApiError } from "../common";

export class SfuError extends ApiError {
  constructor(readonly upstreamStatus = 0, readonly detail = "") {
    super(502, "sfu_erro", "O serviço de mídia não concluiu a operação.");
  }
}
export interface IceServer { urls: string | string[]; username?: string; credential?: string }

export type ProviderId = "cloudflare" | "livekit";
export interface SfuTrack { mid: string; status?: string }
/** Estatísticas do plano de controle; bitrate/perda/RTT vêm dos SDKs WebRTC. */
export interface SfuStats { trackCount: number; inactiveTrackCount: number }
export function trackStats(tracks: SfuTrack[]): SfuStats {
  return { trackCount: tracks.length, inactiveTrackCount: tracks.filter(track => track.status === "inactive").length };
}
export interface SessionContext { role: "publisher" | "viewer"; publisherId?: string }
export interface SdpResult { signaling: "sdp"; sdp: string; tracks: SfuTrack[] }
export interface LiveKitConnection {
  signaling: "livekit";
  url: string;
  token: string;
  room: string;
  identity: string;
  publisherIdentity?: string;
  tracks: SfuTrack[];
}
export type TrackResult = SdpResult | LiveKitConnection;
export type VideoLayer = "high" | "medium" | "low";

/** SFU controla recursos e autorização; SDKs negociam a mídia no modo LiveKit. */
export interface SfuProvider {
  readonly id: ProviderId;
  /** Checa o plano de controle autenticado; não confirma conectividade de mídia. */
  healthCheck(): Promise<void>;
  newSession(context: SessionContext): Promise<string>;
  publish(sessionId: string, offer: string, mid: string, audioMid?: string): Promise<TrackResult>;
  subscribe(sessionId: string, publisherId: string, audio?: boolean): Promise<TrackResult>;
  renegotiate(sessionId: string, answer: string): Promise<void>;
  inspect(sessionId: string): Promise<SfuTrack[]>;
  stats(sessionId: string): Promise<SfuStats>;
  close(sessionId: string, mids: string[]): Promise<void>;
  iceServers(): Promise<{ iceServers: IceServer[] }>;
  /** O encoder atual só publica uma camada; não simular suporte a simulcast. */
  selectLayer(sessionId: string, layer: VideoLayer): Promise<void>;
}

/** Estados anteriores à abstração pertencem à Cloudflare, mesmo após trocar o secret. */
export function allocationProvider(allocation: { provider?: ProviderId }): ProviderId {
  return allocation.provider ?? "cloudflare";
}
