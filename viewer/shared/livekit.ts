// Adaptador carregado apenas quando o Worker encaminha a sessão para LiveKit.
import { ConnectionState, Room, RoomEvent, Track, VideoQuality, type RemoteParticipant, type RemoteTrack, type RemoteTrackPublication } from "livekit-client";

import { copyReceiverLatency, requestReceiverLatency, type ReceiverLatency } from "./receiver-latency";

export interface LiveKitWatchConnection {
  url: string;
  token: string;
  room: string;
  identity: string;
  publisherIdentity: string;
}
export interface LiveKitWatchHandlers {
  onTrack(track: MediaStreamTrack): void;
  onTrackRemoved(track: MediaStreamTrack): void;
  onReconnecting(): void;
  onReconnected(): void;
  onDisconnected(): void;
}

export class LiveKitViewer {
  private readonly room = new Room({ adaptiveStream: false, dynacast: false });
  private closed = false;
  private video: RemoteTrack | null = null;
  private audioTrack: RemoteTrack | null = null;
  private videoLatency: ReceiverLatency | null = null;
  private audioLatency: ReceiverLatency | null = null;
  private abortCleanup: (() => void) | null = null;

  constructor(private readonly connection: LiveKitWatchConnection, private readonly audio: boolean, private readonly handlers: LiveKitWatchHandlers) {}

  get connected(): boolean { return !this.closed && this.room.state === ConnectionState.Connected; }

  private allowed(publication: RemoteTrackPublication, participant: RemoteParticipant): boolean {
    return participant.identity === this.connection.publisherIdentity && (
      publication.source === Track.Source.ScreenShare || publication.trackName === "screen" ||
      (this.audio && (publication.source === Track.Source.ScreenShareAudio || publication.trackName === "audio"))
    );
  }

  async connect(signal: AbortSignal): Promise<void> {
    signal.throwIfAborted();
    if (!this.connection.publisherIdentity) throw new Error("A sessão LiveKit não identifica o publicador.");
    const subscribe = (publication: RemoteTrackPublication, participant: RemoteParticipant): void => {
      if (this.closed) return;
      publication.setSubscribed(this.allowed(publication, participant));
      if (publication.kind === Track.Kind.Video) publication.setVideoQuality(VideoQuality.HIGH);
    };
    this.room.on(RoomEvent.TrackPublished, subscribe);
    this.room.on(RoomEvent.TrackSubscribed, (track, publication, participant) => {
      if (this.closed || signal.aborted || !this.allowed(publication, participant) || (track.kind === Track.Kind.Audio && !this.audio)) {
        publication.setSubscribed(false);
        return;
      }
      const latency = requestReceiverLatency(track.receiver, track.kind === Track.Kind.Audio ? 40 : 0, track.kind === Track.Kind.Audio ? 40 : 0);
      if (track.kind === Track.Kind.Video) {
        this.video = track;
        this.videoLatency = latency;
      } else {
        this.audioTrack = track;
        this.audioLatency = latency;
      }
      this.handlers.onTrack(track.mediaStreamTrack);
    });
    this.room.on(RoomEvent.TrackUnsubscribed, (track) => {
      if (this.closed) return;
      if (this.video === track) { this.video = null; this.videoLatency = null; }
      if (this.audioTrack === track) { this.audioTrack = null; this.audioLatency = null; }
      this.handlers.onTrackRemoved(track.mediaStreamTrack);
    });
    this.room.on(RoomEvent.Reconnecting, () => { if (!this.closed) this.handlers.onReconnecting(); });
    this.room.on(RoomEvent.Reconnected, () => { if (!this.closed) this.handlers.onReconnected(); });
    this.room.on(RoomEvent.Disconnected, () => { if (!this.closed) this.handlers.onDisconnected(); });
    const abort = (): void => this.close();
    signal.addEventListener("abort", abort, { once: true });
    this.abortCleanup = () => signal.removeEventListener("abort", abort);
    try {
      await this.room.connect(this.connection.url, this.connection.token, { autoSubscribe: false });
      signal.throwIfAborted();
      if (this.closed) throw new Error("Conexão substituída.");
      for (const participant of this.room.remoteParticipants.values()) {
        for (const publication of participant.trackPublications.values()) subscribe(publication, participant);
      }
    } catch {
      this.close();
      // Um connect tardio pode concluir depois do primeiro disconnect (abort durante join).
      await this.room.disconnect().catch(() => undefined);
      // Erros do SDK podem trazer detalhes da sinalização. Não expor URL com token nem SDP.
      signal.throwIfAborted();
      throw new Error("Não foi possível conectar ao servidor reserva de mídia.");
    }
  }

  getLatencyPreferences() {
    return { video: copyReceiverLatency(this.videoLatency), audio: copyReceiverLatency(this.audioLatency) };
  }

  async getStats(): Promise<RTCStatsReport | null> {
    const tracks = [this.video, this.audioTrack].filter((track): track is RemoteTrack => track !== null);
    const reports = await Promise.allSettled(tracks.map(track => track.getRTCStatsReport()));
    if (this.closed) return null;
    const stats = new Map<string, RTCStats>();
    let available = false;
    for (let index = 0; index < reports.length; index++) {
      const result = reports[index];
      // Não incorpora uma faixa substituída enquanto getStats estava pendente.
      if (result?.status !== "fulfilled" || !result.value
        || (tracks[index] !== this.video && tracks[index] !== this.audioTrack)) continue;
      available = true;
      result.value.forEach((stat, id) => stats.set(id, stat));
    }
    return available ? stats as RTCStatsReport : null;
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.abortCleanup?.();
    this.abortCleanup = null;
    this.video = null;
    this.audioTrack = null;
    this.videoLatency = null;
    this.audioLatency = null;
    this.room.removeAllListeners();
    void this.room.disconnect().catch(() => undefined);
  }
}
