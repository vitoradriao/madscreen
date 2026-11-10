import { copyReceiverLatency, requestReceiverLatency, type ReceiverLatency } from "./receiver-latency";
import type { LiveKitViewer, LiveKitWatchConnection } from "./livekit";
// Cliente do madscreen-signal compartilhado pelo app Tauri e pelo viewer web. Sem dependência de DOM
// além de fetch, WebSocket, WebRTC e localStorage.

export interface UserView {
  id: string;
  username: string;
  displayName: string;
  avatar: string | null;
}
export interface LiveInfo {
  since: number;
  viewers: number;
  title: string | null;
}
export interface FriendView extends UserView {
  online: boolean;
  live: LiveInfo | null;
}
export interface Snapshot {
  me: UserView;
  friends: FriendView[];
  incoming: UserView[];
  outgoing: UserView[];
  servers?: ServerView[];
  destination?: InviteDestination;
}
export interface InviteDestination { serverId: string; serverName: string; channelId: string; channelName: string }
export type MemberRole = "owner" | "admin" | "member";
export interface ServerChannel { id: string; name: string; live: FriendView[] }
export interface ServerView {
  id: string; name: string; ownerId: string; role: MemberRole;
  channels: ServerChannel[];
  members: (UserView & { role: MemberRole; online: boolean })[];
}

/** Aceita código, link HTTPS do Worker ou o protocolo registrado pelo instalador. */
export function inviteCodeFrom(input: string, base?: string): string {
  let value = input.trim();
  if (value.includes(":")) {
    let url: URL;
    try { url = new URL(value); } catch { throw new ApiFailure(400, "convite_invalido", "Código ou link de convite inválido."); }
    if (url.protocol === "madscreen:" && url.hostname === "invite") value = url.pathname.slice(1);
    else if (url.protocol === "https:" && (!base || url.origin === new URL(base).origin) && /^\/i\/[A-Za-z0-9-]+$/.test(url.pathname)) value = url.pathname.slice(3);
    else throw new ApiFailure(400, "convite_invalido", "Use um convite do madscreen.");
  }
  value = value.toUpperCase();
  if (!/^[A-Z0-9]{4}-?[A-Z0-9]{4}$/.test(value)) throw new ApiFailure(400, "convite_invalido", "Use o código XXXX-XXXX ou o link do convite.");
  const raw = value.replace("-", "");
  return `${raw.slice(0, 4)}-${raw.slice(4)}`;
}

export function pendingInvite(): string | null {
  try { return localStorage.getItem("ms.invite"); } catch { return null; }
}
export function rememberInvite(value: string, base?: string): string | null {
  const code = value.trim() ? inviteCodeFrom(value, base) : null;
  try { if (code) localStorage.setItem("ms.invite", code); else localStorage.removeItem("ms.invite"); } catch { /* Sem armazenamento: a chamada corrente ainda pode usar o código. */ }
  return code;
}
export function clearInvite(): void { try { localStorage.removeItem("ms.invite"); } catch { /* Sem armazenamento. */ } }
export interface Providers {
  discord: boolean;
  google: boolean;
  invite: boolean;
  /** Ainda não há usuários: o primeiro cadastro dispensa convite. */
  firstUser: boolean;
}
export interface Session {
  token: string;
  exp: number;
  user: UserView;
}

export class ApiFailure extends Error {
  constructor(readonly status: number, readonly code: string, message: string) {
    super(message);
  }
}

const SESSION_KEY = "ms.session";
const PKCE_KEY = "ms.pkce";

function b64url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function loadSession(): Session | null {
  try {
    const value = JSON.parse(localStorage.getItem(SESSION_KEY) ?? "null") as Session | null;
    if (value && typeof value.token === "string" && value.exp > Date.now() / 1000) return value;
  } catch {
    // Valor corrompido: trata como sem sessão.
  }
  return null;
}

export function saveSession(session: Session | null): void {
  if (session) localStorage.setItem(SESSION_KEY, JSON.stringify(session));
  else localStorage.removeItem(SESSION_KEY);
}

export function savedPkce(): string | null {
  return localStorage.getItem(PKCE_KEY);
}
export function clearPkce(): void {
  localStorage.removeItem(PKCE_KEY);
}

export interface HubHandlers {
  onSnapshot(snapshot: Snapshot): void;
  onState?(online: boolean): void;
  onAuthLost?(): void;
}

export class MadClient {
  session: Session | null;

  constructor(readonly base: string, session: Session | null = loadSession()) {
    this.base = base.replace(/\/+$/, "");
    this.session = session;
  }

  setSession(session: Session | null): void {
    this.session = session;
    saveSession(session);
  }

  get token(): string {
    if (!this.session) throw new ApiFailure(401, "sessao_invalida", "Entre para continuar.");
    return this.session.token;
  }

  async request<T>(method: string, path: string, body?: unknown, auth = true, signal?: AbortSignal): Promise<T> {
    signal?.throwIfAborted();
    const session = this.session;
    const headers: Record<string, string> = {};
    if (auth) headers.Authorization = `Bearer ${this.token}`;
    if (body !== undefined) headers["Content-Type"] = "application/json";
    let response: Response;
    try {
      response = await fetch(this.base + path, {
        method, headers, cache: "no-store", credentials: "omit", referrerPolicy: "no-referrer",
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
        signal: signal ? AbortSignal.any([signal, AbortSignal.timeout(20_000)]) : AbortSignal.timeout(20_000),
      });
    } catch {
      signal?.throwIfAborted();
      throw new ApiFailure(0, "rede", "Sem conexão com o servidor do madscreen.");
    }
    signal?.throwIfAborted();
    if (response.status === 202) return (await response.json()) as T;
    if (!response.ok) {
      let code = "erro";
      let message = "Não foi possível concluir a operação.";
      try {
        const data = (await response.json()) as { error?: string; message?: string };
        code = data.error ?? code;
        message = data.message ?? message;
      } catch {
        // Corpo sem JSON.
      }
      if (response.status === 401 && auth && code === "sessao_invalida" && this.session === session) this.setSession(null);
      throw new ApiFailure(response.status, code, message);
    }
    return (await response.json()) as T;
  }

  // ---- entrada ----
  providers = () => this.request<Providers>("GET", "/auth/providers", undefined, false);

  private async adopt(result: Session & { recoveryKey?: string }, signal?: AbortSignal): Promise<Session & { recoveryKey?: string }> {
    signal?.throwIfAborted();
    this.setSession({ token: result.token, exp: result.exp, user: result.user });
    return result;
  }

  async register(username: string, code: string, displayName?: string, signal?: AbortSignal) {
    code = rememberInvite(code, this.base) ?? "";
    return this.adopt(await this.request<Session & { recoveryKey: string }>("POST", "/auth/register", { username, code, ...(displayName ? { displayName } : {}) }, false, signal), signal);
  }

  async login(username: string, recoveryKey: string, signal?: AbortSignal) {
    return this.adopt(await this.request<Session>("POST", "/auth/login", { username, recoveryKey }, false, signal), signal);
  }

  /** Prepara o login pelo navegador. O verificador fica só com quem chamou (PKCE). */
  async beginOAuth(provider: "discord" | "google", invite?: string): Promise<{ url: string; verifier: string }> {
    const code = rememberInvite(invite ?? pendingInvite() ?? "", this.base);
    const verifier = b64url(crypto.getRandomValues(new Uint8Array(32)));
    const challenge = b64url(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier))));
    const query = new URLSearchParams({ challenge, ...(code ? { invite: code } : {}) });
    return { url: `${this.base}/auth/${provider}/start?${query.toString()}`, verifier };
  }

  /** Consulta até o navegador concluir o login (ou `signal` abortar / passar 10 min). */
  async awaitOAuth(verifier: string, signal: AbortSignal): Promise<Session> {
    const deadline = Date.now() + 10 * 60_000;
    while (!signal.aborted && Date.now() < deadline) {
      const result = await this.request<Session & { pending?: boolean }>("POST", "/auth/exchange", { verifier }, false, signal);
      signal.throwIfAborted();
      if (!result.pending && result.token) {
        this.setSession({ token: result.token, exp: result.exp, user: result.user });
        return result;
      }
      await wait(1500, signal);
    }
    signal.throwIfAborted();
    throw new ApiFailure(0, "login_expirou", "O acesso pelo navegador não foi concluído. Tente de novo.");
  }

  async logout(): Promise<void> {
    if (this.token) await this.request("POST", "/api/logout", {});
    this.setSession(null);
  }

  // ---- conta e amigos ----
  async me(): Promise<UserView> {
    const session = this.session;
    const result = await this.request<{ user: UserView; token?: string; exp?: number }>("GET", "/api/me");
    if (result.token && result.exp && this.session === session && session) this.setSession({ token: result.token, exp: result.exp, user: result.user });
    return result.user;
  }
  /** `null` quando o servidor ainda não informa o tipo de acesso da conta. */
  async hasRecoveryKey(): Promise<boolean | null> {
    const result = await this.request<{ account?: { recoveryKey?: boolean } }>("GET", "/api/me");
    return typeof result.account?.recoveryKey === "boolean" ? result.account.recoveryKey : null;
  }
  rotateRecoveryKey = () => this.request<{ recoveryKey: string }>("POST", "/api/me/recovery-key", {});
  friends = () => this.request<Snapshot>("GET", "/api/friends");
  requestFriend = (username: string) => this.request<Snapshot>("POST", "/api/friends/request", { username });
  acceptFriend = (id: string) => this.request<Snapshot>("POST", "/api/friends/accept", { id });
  declineFriend = (id: string) => this.request<Snapshot>("POST", "/api/friends/decline", { id });
  removeFriend = (id: string) => this.request<Snapshot>("DELETE", `/api/friends/${encodeURIComponent(id)}`);
  createInvite = (serverId?: string, channelId?: string) => this.request<{ code: string; link: string; expiresAt: number; maxUses: number; destination?: InviteDestination }>("POST", "/api/invites", { serverId, channelId });
  redeemInvite = (code: string) => this.request<Snapshot>("POST", "/api/invites/redeem", { code });
  previewInvite = (code: string) =>
    this.request<{ inviter: UserView; expiresAt: number; server?: { id: string; name: string }; channel?: { id: string; name: string } }>("GET", `/api/invites/preview?code=${encodeURIComponent(inviteCodeFrom(code, this.base))}`, undefined, false);
  servers = () => this.request<Snapshot>("GET", "/api/servers");
  createServer = (name: string) => this.request<Snapshot>("POST", "/api/servers", { name });
  renameServer = (serverId: string, name: string) => this.request<Snapshot>("PATCH", `/api/servers/${encodeURIComponent(serverId)}`, { name });
  removeServer = (serverId: string) => this.request<Snapshot>("DELETE", `/api/servers/${encodeURIComponent(serverId)}`);
  createChannel = (serverId: string, name: string) => this.request<Snapshot>("POST", `/api/servers/${encodeURIComponent(serverId)}/channels`, { name });
  renameChannel = (serverId: string, channelId: string, name: string) => this.request<Snapshot>("PATCH", `/api/servers/${encodeURIComponent(serverId)}/channels/${encodeURIComponent(channelId)}`, { name });
  removeChannel = (serverId: string, channelId: string) => this.request<Snapshot>("DELETE", `/api/servers/${encodeURIComponent(serverId)}/channels/${encodeURIComponent(channelId)}`);
  leaveServer = (serverId: string) => this.request<Snapshot>("DELETE", `/api/servers/${encodeURIComponent(serverId)}/members/me`);
  setMemberRole = (serverId: string, userId: string, role: "admin" | "member") => this.request<Snapshot>("PATCH", `/api/servers/${encodeURIComponent(serverId)}/members/${encodeURIComponent(userId)}`, { role });
  async completeInvite(): Promise<Snapshot | null> {
    const code = pendingInvite();
    if (!code) return null;
    const result = await this.redeemInvite(code);
    if (pendingInvite() === code) clearInvite();
    return result;
  }
  iceServers = (signal?: AbortSignal) => this.request<{ iceServers: RTCIceServer[] }>("GET", "/api/ice", undefined, true, signal);

  // ---- tempo real ----
  wsUrl(path: string): string {
    return this.base.replace(/^http/, "ws") + path;
  }

  /** Presença e lista de amigos em tempo real, com reconexão automática. */
  connectHub(handlers: HubHandlers): { close(): void } {
    let socket: WebSocket | null = null;
    let closed = false;
    let failures = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let ping: ReturnType<typeof setInterval> | undefined;

    const open = (): void => {
      if (closed || !this.session) return;
      const ws = new WebSocket(this.wsUrl("/api/ws"), [`ms.${this.token}`]);
      socket = ws;
      ws.onopen = () => {
        failures = 0;
        handlers.onState?.(true);
        ping = setInterval(() => { if (ws.readyState === WebSocket.OPEN) ws.send("ping"); }, 25_000);
      };
      ws.onmessage = (event: MessageEvent) => {
        if (typeof event.data !== "string" || event.data === "pong") return;
        try {
          const message = JSON.parse(event.data) as { type: string } & Partial<Snapshot>;
          if (message.type === "snapshot") handlers.onSnapshot(message as Snapshot);
        } catch {
          // Mensagem desconhecida.
        }
      };
      ws.onclose = () => {
        clearInterval(ping);
        handlers.onState?.(false);
        if (closed) return;
        failures++;
        if (failures >= 3) {
          // Distingue rede fora do ar de sessão inválida.
          void this.me().catch((error: unknown) => {
            if (error instanceof ApiFailure && error.status === 401) handlers.onAuthLost?.();
          });
        }
        timer = setTimeout(open, Math.min(15_000, 1000 * 2 ** Math.min(failures, 4)));
      };
    };
    open();
    return {
      close: () => {
        closed = true;
        clearTimeout(timer);
        clearInterval(ping);
        socket?.close();
      },
    };
  }
}

// ---- assistir ----------------------------------------------------------------------------------

export type WatchState = "connecting" | "waiting" | "playing" | "ended" | "error" | "stopped";
export interface WatchStats {
  width: number;
  height: number;
  fps: number;
  mbps: number;
  rttMs: number | null;
  jitterBufferMs: number | null;
  lossPct: number | null;
}

/** RTT do transporte usado pelo vídeo; não confundir pares antigos com o selecionado. */
export function videoRttMs(report: RTCStatsReport, inbound: RTCInboundRtpStreamStats): number | null {
  const transport = inbound.transportId ? report.get(inbound.transportId) as RTCTransportStats | undefined : undefined;
  const selected = transport?.selectedCandidatePairId;
  const validRtt = (pair: RTCIceCandidatePairStats | undefined): number | null =>
    pair?.type === "candidate-pair" && typeof pair.currentRoundTripTime === "number" && Number.isFinite(pair.currentRoundTripTime) && pair.currentRoundTripTime >= 0 ? pair.currentRoundTripTime * 1000 : null;
  if (selected) return validRtt(report.get(selected) as RTCIceCandidatePairStats | undefined);
  const pairs: RTCIceCandidatePairStats[] = [];
  report.forEach(item => {
    const pair = item as RTCIceCandidatePairStats;
    if (pair.type === "candidate-pair" && pair.nominated && (!pair.state || pair.state === "succeeded")) pairs.push(pair);
  });
  return pairs.length === 1 ? validRtt(pairs[0]) : null;
}

interface AudioDiagnosticSample {
  atMs: number;
  segment: number;
  provider: "cloudflare" | "livekit";
  packetsReceived: number | null;
  packetsLost: number | null;
  bytesReceived: number | null;
  jitter: number | null;
  jitterBufferDelay: number | null;
  jitterBufferEmittedCount: number | null;
  totalSamplesReceived: number | null;
  concealedSamples: number | null;
  silentConcealedSamples: number | null;
  concealmentEvents: number | null;
  insertedSamplesForDeceleration: number | null;
  removedSamplesForAcceleration: number | null;
  totalAudioEnergy: number | null;
  totalSamplesDuration: number | null;
  audioLevel: number | null;
}

interface VideoDiagnosticSample {
  atMs: number;
  segment: number;
  provider: "cloudflare" | "livekit";
  rttMs: number | null;
  framesDecoded: number;
  statsCollectionMs?: number;
  sampleGapMs?: number | null;
  framesReceived?: number | null;
  framesDropped?: number | null;
  freezeCount?: number | null;
  totalFreezesDurationSeconds?: number | null;
  maxPresentationGapMs?: number | null;
  presentedFrames: number | null;
  presentationGapMs: number | null;
  decodeGapMs: number | null;
  bytesReceived: number;
  packetsReceived: number;
  packetsLost: number;
  nackCount: number;
  pliCount: number;
  jitterMs: number | null;
  jitterBufferDelaySeconds: number | null;
  jitterBufferEmittedCount: number | null;
  totalDecodeTimeSeconds: number | null;
  documentHidden: boolean;
}
/** Pedido automático observado até a retomada do decode e da apresentação (sem inferir causalidade). */
interface VideoRecoveryObservation {
  segment: number;
  reason: "first-frame" | "loss" | "decode-stall";
  requestedAtMs: number;
  presentedFramesAtRequest: number | null;
  decodeResumedAtMs: number | null;
  presentedAtMs: number | null;
}
/** Pede Opus estéreo no SDP: acrescenta `stereo=1;sprop-stereo=1` ao fmtp de cada payload Opus. */
export function withOpusStereo(sdp: string): string {
  const payloads = new Set<string>();
  for (const match of sdp.matchAll(/^a=rtpmap:(\d+) opus\/48000\/2\s*$/gim)) payloads.add(match[1] ?? "");
  if (payloads.size === 0) return sdp;
  const extra = "stereo=1;sprop-stereo=1";
  const lines = sdp.split(/\r?\n/);
  const seen = new Set<string>();
  const out: string[] = [];
  for (const line of lines) {
    const fmtp = /^a=fmtp:(\d+) (.*)$/.exec(line);
    if (fmtp && payloads.has(fmtp[1] ?? "")) {
      seen.add(fmtp[1] ?? "");
      const kept = (fmtp[2] ?? "").split(";").map((part) => part.trim())
        .filter((part) => part && !/^(stereo|sprop-stereo)=/.test(part));
      out.push(`a=fmtp:${fmtp[1]} ${[...kept, extra].join(";")}`);
      continue;
    }
    out.push(line);
    const rtpmap = /^a=rtpmap:(\d+) opus\/48000\/2\s*$/i.exec(line);
    // Sem fmtp original: cria a linha logo após o rtpmap (o fmtp existente, se vier depois, é reescrito acima).
    if (rtpmap && !lines.some((other) => other.startsWith(`a=fmtp:${rtpmap[1]} `))) out.push(`a=fmtp:${rtpmap[1]} ${extra}`);
  }
  return out.join("\r\n");
}

export interface WatchHandlers {
  onState(state: WatchState, detail?: string): void;
  onStats?(stats: WatchStats): void;
  /** A transmissão traz (ou deixou de trazer) a faixa de áudio. */
  onAudio?(present: boolean): void;
  onAutoplayBlocked?(): void;
}

export interface WatchSessionOptions {
  /** Desliga somente armazenamento de diagnóstico; não altera a recuperação. */
  diagnostics?: boolean;
  audio?: boolean;
  preferredRid?: "h" | "m" | "l" | null;
}

function wait(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const done = (): void => { clearTimeout(timer); signal?.removeEventListener("abort", done); resolve(); };
    const timer = setTimeout(done, ms);
    signal?.addEventListener("abort", done, { once: true });
    if (signal?.aborted) done();
  });
}

/**
 * Assiste à transmissão de um amigo: WebSocket do StreamDO avisa quando entra/sai do ar, o SFU oferece
 * a faixa e o navegador só responde. Solicita jitterBufferTarget 0 e playoutDelayHint 0 no vídeo.
 */
export class WatchSession {
  private pc: RTCPeerConnection | null = null;
  private livekit: LiveKitViewer | null = null;
  private videoLatency: ReceiverLatency | null = null;
  private audioLatency: ReceiverLatency | null = null;
  private ws: WebSocket | null = null;
  private viewerId: string | null = null;
  private generation: number | null = null;
  private live = false;
  private stopped = false;
  private subscribing = false;
  private subscribeAgain = false;
  private pingTimer: ReturnType<typeof setInterval> | undefined;
  private wsRetryTimer: ReturnType<typeof setTimeout> | undefined;
  private mediaTimers: ReturnType<typeof setInterval>[] = [];
  private mediaTimeouts = new Set<ReturnType<typeof setTimeout>>();
  private mediaAbort: AbortController | null = null;
  private stream: MediaStream | null = null;
  private audioPresent = false;
  private framePresented = false;
  private frameCleanup: (() => void) | null = null;
  private firstFrameTimer: ReturnType<typeof setTimeout> | undefined;
  private previous: { bytes: number; frames: number; received: number; lost: number; jitterDelay: number; jitterCount: number; at: number } | null = null;
  private inboundId: string | null = null;
  private recoveryPrevious: { bytes: number; frames: number; received: number; lost: number; at: number } | null = null;
  private waitingForFrameAt: number | null = null;
  private lastDecodeAt: number | null = null;
  private decodeStalledAt: number | null = null;
  private lossStalledAt: number | null = null;
  private sampling: RTCPeerConnection | LiveKitViewer | null = null;
  private lastKeyframeRequestAt = -Infinity;
  private readonly diagnosticSamples: VideoDiagnosticSample[] = [];
  private readonly diagnosticHistory: VideoDiagnosticSample[] = [];
  private readonly audioDiagnosticHistory: AudioDiagnosticSample[] = [];
  private maxPresentationGapMs: number | null = null;
  private readonly audioDiagnosticSamples: AudioDiagnosticSample[] = [];
  private audioInboundId: string | null = null;
  private audioDiagnosticSegment = 0;
  private readonly recoveryObservations: VideoRecoveryObservation[] = [];
  private diagnosticSegment = 0;
  private presentedFrames: number | null = null;
  private lastPresentedAt: number | null = null;
  private wsFailures = 0;
  private readonly abort = new AbortController();
  private readonly onOnline = (): void => {
    if (this.ws?.readyState !== WebSocket.OPEN || !this.mediaConnected()) this.reconnect();
  };

  constructor(
    private readonly client: MadClient,
    readonly owner: string,
    private readonly video: HTMLVideoElement,
    private readonly handlers: WatchHandlers,
    private readonly options: WatchSessionOptions = {},
  ) {}

  private mediaConnected(): boolean {
    return this.pc?.connectionState === "connected" || this.livekit?.connected === true;
  }

  private get audioEnabled(): boolean {
    return this.options.audio !== false && this.owner.toLowerCase() !== this.client.session?.user.username.toLowerCase();
  }

  start(): void {
    if (this.stopped || this.ws) return;
    window.addEventListener("online", this.onOnline);
    this.handlers.onState("connecting");
    this.openSocket();
  }

  /** Ambos os publicadores atuais são pré-codificados com uma única camada espacial. */
  async setPreferredRid(rid: "h" | "m" | "l"): Promise<void> {
    if (rid !== "h") throw new ApiFailure(409, "camada_indisponivel", "Esta transmissão publica uma única camada de vídeo.");
    const viewerId = this.viewerId;
    const signal = this.mediaAbort?.signal;
    if (viewerId && signal) await this.client.request("POST", "/api/watch/layer", { viewerId, preferredRid: rid }, true, signal);
  }

  /** Retoma o canal de eventos ou tenta novamente a mídia, sem descartar vídeo saudável. */
  reconnect(): void {
    if (this.stopped) return;
    this.wsFailures = 0;
    clearTimeout(this.wsRetryTimer);
    if (this.ws?.readyState !== WebSocket.OPEN) {
      if (!this.mediaConnected()) this.handlers.onState("connecting", "Reconectando à transmissão…");
      const previous = this.ws;
      this.ws = null;
      previous?.close();
      this.openSocket();
    } else if (this.live) {
      void this.subscribe();
    }
  }

  /** Recuperação automática interna: limita pedidos de quadro-chave a um a cada 1,5 s. */
  requestKeyframe(reason?: "first-frame" | "loss" | "decode-stall"): void {
    const now = performance.now();
    if (this.ws?.readyState === WebSocket.OPEN && now - this.lastKeyframeRequestAt >= 1500) {
      this.ws.send(JSON.stringify({ type: "keyframe" }));
      this.lastKeyframeRequestAt = now;
      if (reason && this.options.diagnostics !== false) {
        this.recoveryObservations.push({
          segment: this.diagnosticSegment + (this.inboundId === null ? 1 : 0), reason, requestedAtMs: Math.round(now),
          presentedFramesAtRequest: this.presentedFrames, decodeResumedAtMs: null, presentedAtMs: null,
        });
        if (this.recoveryObservations.length > 20) this.recoveryObservations.shift();
        console.warn("madscreen: recuperação de vídeo", { reason, decodeGapMs: this.lastDecodeAt === null ? null : Math.round(now - this.lastDecodeAt) });
      }
    }
  }

  /** Últimas 60 amostras locais, sem tokens, SDP, IPs, nomes ou conteúdo de mídia. */
  getDiagnostics() {
    return {
      receiverPreferences: this.livekit?.getLatencyPreferences() ?? { video: copyReceiverLatency(this.videoLatency), audio: copyReceiverLatency(this.audioLatency) },
      schema: 1, capturedAt: new Date().toISOString(),
      samples: this.diagnosticSamples.map(sample => ({ ...sample })),
      history: this.diagnosticHistory.map(sample => ({ ...sample })),
      historyIntervalMs: 1000,
      audioHistory: this.audioDiagnosticHistory.map(sample => ({ ...sample })),
      audioSamples: this.audioDiagnosticSamples.map(sample => ({ ...sample })),
      recoveries: this.recoveryObservations.map(item => ({
        ...item,
        timeToDecodeMs: item.decodeResumedAtMs === null ? null : Math.max(0, item.decodeResumedAtMs - item.requestedAtMs),
        timeToPresentationMs: item.presentedAtMs === null ? null : Math.max(0, item.presentedAtMs - item.requestedAtMs),
      })),
    };
  }

  stop(): void {
    if (this.stopped) return;
    this.stopped = true;
    this.subscribeAgain = false;
    window.removeEventListener("online", this.onOnline);
    this.abort.abort();
    clearInterval(this.pingTimer);
    clearTimeout(this.wsRetryTimer);
    this.ws?.close();
    void this.teardown(true);
    this.handlers.onState("stopped");
  }

  private openSocket(): void {
    if (this.stopped) return;
    if (!this.client.session) {
      this.handlers.onState("error", "Entre novamente para assistir à transmissão.");
      return;
    }
    const url = this.client.wsUrl(`/api/stream/ws?owner=${encodeURIComponent(this.owner)}`);
    const ws = new WebSocket(url, [`ms.${this.client.token}`]);
    this.ws = ws;
    ws.onopen = () => { if (!this.stopped && this.ws === ws) this.wsFailures = 0; };
    ws.onmessage = (event: MessageEvent) => {
      if (this.stopped || this.ws !== ws) return;
      if (typeof event.data !== "string" || event.data === "pong") return;
      let message: { type?: string; live?: boolean; generation?: number };
      try {
        message = JSON.parse(event.data) as typeof message;
      } catch {
        return;
      }
      const generation = typeof message.generation === "number" ? message.generation : this.generation;
      if (message.type === "hello" || message.type === "live") {
        const changed = generation !== this.generation;
        const wasLive = this.live;
        this.live = message.type === "live" || message.live === true;
        this.generation = generation;
        if (this.live) {
          if (changed || !wasLive || (!this.pc && !this.livekit && !this.subscribing)
            || this.pc?.connectionState === "failed" || this.pc?.connectionState === "closed") void this.subscribe();
        } else {
          this.subscribeAgain = false;
          void this.teardown(true);
          this.handlers.onState("waiting");
        }
      } else if (message.type === "ended") {
        this.live = false;
        this.subscribeAgain = false;
        this.generation = generation;
        void this.teardown(true);
        this.handlers.onState("ended");
      }
    };
    ws.onclose = () => {
      if (this.stopped || this.ws !== ws) return;
      clearInterval(this.pingTimer);
      this.wsFailures++;
      if (this.wsFailures >= 3 && !this.mediaConnected()) {
        this.handlers.onState("error", "Sem conexão com a transmissão. Tentaremos novamente quando a conexão voltar.");
      }
      this.wsRetryTimer = setTimeout(() => this.openSocket(), Math.min(8000, 1000 * this.wsFailures));
    };
    clearInterval(this.pingTimer);
    this.pingTimer = setInterval(() => { if (ws.readyState === WebSocket.OPEN) ws.send("ping"); }, 25_000);
  }

  private async teardown(leave: boolean): Promise<void> {
    this.mediaAbort?.abort();
    this.mediaAbort = null;
    for (const timer of this.mediaTimers.splice(0)) clearInterval(timer);
    for (const timer of this.mediaTimeouts) clearTimeout(timer);
    this.mediaTimeouts.clear();
    this.firstFrameTimer = undefined;
    this.frameCleanup?.();
    this.frameCleanup = null;
    this.framePresented = false;
    this.presentedFrames = null;
    this.lastPresentedAt = null;
    this.maxPresentationGapMs = null;
    const pc = this.pc;
    const livekit = this.livekit;
    this.livekit = null;
    livekit?.close();
    const viewerId = this.viewerId;
    this.pc = null;
    this.videoLatency = null;
    this.audioLatency = null;
    this.viewerId = null;
    this.previous = null;
    this.inboundId = null;
    this.audioInboundId = null;
    this.audioDiagnosticSegment++;
    // Invalida tambem pedidos first-frame feitos antes de existir um id RTP.
    this.diagnosticSegment++;
    this.recoveryPrevious = null;
    this.waitingForFrameAt = null;
    this.lastDecodeAt = null;
    this.decodeStalledAt = null;
    this.lossStalledAt = null;
    if (pc) {
      pc.ontrack = null;
      pc.onconnectionstatechange = null;
      pc.close();
    }
    if (this.stream && this.video.srcObject === this.stream) this.video.srcObject = null;
    this.stream = null;
    this.setAudioPresent(false);
    if (leave && viewerId) {
      void this.leaveViewer(viewerId);
    }
  }

  private setAudioPresent(present: boolean): void {
    if (this.audioPresent === present) return;
    this.audioPresent = present;
    this.handlers.onAudio?.(present);
  }

  private async leaveViewer(viewerId: string): Promise<void> {
    await fetch(`${this.client.base}/api/watch/leave`, {
      method: "POST", keepalive: true, credentials: "omit", referrerPolicy: "no-referrer",
      headers: { Authorization: `Bearer ${this.client.session?.token ?? ""}`, "Content-Type": "application/json" },
      body: JSON.stringify({ viewerId }),
      signal: AbortSignal.timeout(5000),
    }).catch(() => undefined);
  }

  private observeFirstFrame(signal: AbortSignal): void {
    this.frameCleanup?.();
    const video = this.video;
    const current = (): boolean => !signal.aborted && !this.stopped && video.srcObject === this.stream;
    let callback: number | null = null;
    const cleanup = (): void => {
      if (callback !== null) video.cancelVideoFrameCallback?.(callback);
      video.removeEventListener("loadeddata", ready);
      video.removeEventListener("playing", ready);
    };
    const presented = (): void => {
      if (!current() || this.framePresented) return;
      this.framePresented = true;
      if (this.firstFrameTimer !== undefined) {
        clearTimeout(this.firstFrameTimer);
        this.mediaTimeouts.delete(this.firstFrameTimer);
        this.firstFrameTimer = undefined;
      }
      this.handlers.onState("playing");
    };
    const ready = (): void => {
      if (video.readyState >= 2 && video.videoWidth > 0) presented();
    };
    this.frameCleanup = cleanup;
    this.handlers.onState("connecting", "Recebendo vídeo…");
    if (typeof video.requestVideoFrameCallback === "function") {
      const onFrame: VideoFrameRequestCallback = (at, metadata) => {
        callback = null;
        if (!current()) return;
        if (this.lastPresentedAt !== null) this.maxPresentationGapMs = Math.max(this.maxPresentationGapMs ?? 0, at - this.lastPresentedAt);
        this.presentedFrames = metadata.presentedFrames;
        this.lastPresentedAt = at;
        for (const item of this.recoveryObservations) {
          // Um callback não comprova que o IDR causou a recuperação: registra apenas correlação.
          if (item.segment === this.diagnosticSegment && item.decodeResumedAtMs !== null && item.presentedAtMs === null
            && at >= item.decodeResumedAtMs
            && (item.presentedFramesAtRequest === null || metadata.presentedFrames > item.presentedFramesAtRequest)) {
            item.presentedAtMs = Math.round(at);
          }
        }
        presented();
        if (current()) callback = video.requestVideoFrameCallback(onFrame);
      };
      callback = video.requestVideoFrameCallback(onFrame);
    } else {
      video.addEventListener("loadeddata", ready);
      video.addEventListener("playing", ready);
      ready();
    }
    void video.play().catch(async () => {
      if (!current()) return;
      // A política de autoplay pode bloquear só o som. Preserva a imagem e oferece liberação na UI.
      video.muted = true;
      this.handlers.onAutoplayBlocked?.();
      try { await video.play(); return; } catch { /* Mostra o erro se nem o vídeo mudo iniciou. */ }
      if (current()) this.handlers.onState("error", "O navegador não iniciou o vídeo. Use Tentar novamente para retomar.");
    });
  }

  private async subscribe(): Promise<void> {
    if (this.stopped || !this.live) return;
    if (this.subscribing) {
      this.subscribeAgain = true;
      this.mediaAbort?.abort();
      return;
    }
    this.subscribing = true;
    this.subscribeAgain = false;
    try {
      for (let attempt = 0; !this.stopped && this.live; attempt++) {
        try {
          await this.teardown(true);
          if (this.stopped || !this.live || this.subscribeAgain) return;
          this.handlers.onState("connecting");
          await this.negotiate();
          return;
        } catch (error) {
          await this.teardown(true);
          if (this.stopped || !this.live || this.subscribeAgain) return;
          if (error instanceof ApiFailure && error.code === "sem_publicador") {
            this.live = false;
            this.handlers.onState("waiting");
            return;
          }
          if (error instanceof ApiFailure && [401, 403].includes(error.status)) {
            this.handlers.onState("error", error.message);
            return;
          }
          if (error instanceof ApiFailure && ["camada_indisponivel", "cliente_desatualizado"].includes(error.code)) {
            this.handlers.onState("error", error.message);
            return;
          }
          if (attempt >= 4) {
            this.handlers.onState("error", error instanceof Error ? error.message : "Falha na conexão de vídeo.");
            return;
          }
          await wait(Math.min(4000, 800 * (attempt + 1)), this.abort.signal);
        }
      }
    } finally {
      this.subscribing = false;
      if (this.subscribeAgain && !this.stopped && this.live) void this.subscribe();
    }
  }

  private async negotiate(): Promise<void> {
    if (!this.audioEnabled) this.video.muted = true;
    const controller = new AbortController();
    this.mediaAbort = controller;
    const signal = AbortSignal.any([this.abort.signal, controller.signal]);
    const active = (): void => {
      signal.throwIfAborted();
      if (this.stopped || !this.live || this.mediaAbort !== controller) throw new ApiFailure(0, "cancelado", "Conexão substituída.");
    };
    const started = await this.client.request<{
      viewerId: string; generation: number; audio?: boolean;
    } & ({ provider?: "cloudflare"; offer: string } | { provider: "livekit"; livekit: LiveKitWatchConnection })>(
      "POST", "/api/watch/start", { owner: this.owner, audio: this.audioEnabled, preferredRid: this.options.preferredRid ?? "h", providers: ["cloudflare", "livekit"] }, true, signal);
    if (signal.aborted || this.mediaAbort !== controller) void this.leaveViewer(started.viewerId);
    active();
    this.viewerId = started.viewerId;
    this.generation = started.generation;
    if (started.provider === "livekit") {
      const { LiveKitViewer } = await import("./livekit");
      active();
      const viewer = new LiveKitViewer(started.livekit, this.audioEnabled && started.audio === true, {
        onTrack: (track) => {
          if (signal.aborted || this.livekit !== viewer) return;
          this.stream ??= new MediaStream();
          this.stream.addTrack(track);
          if (this.video.srcObject !== this.stream) this.video.srcObject = this.stream;
          if (track.kind === "audio") this.setAudioPresent(true);
          else { this.waitingForFrameAt ??= performance.now(); this.observeFirstFrame(signal); }
        },
        onTrackRemoved: (track) => {
          if (signal.aborted || this.livekit !== viewer) return;
          this.stream?.removeTrack(track);
          if (track.kind === "audio") this.setAudioPresent((this.stream?.getAudioTracks().length ?? 0) > 0);
        },
        onReconnecting: () => {
          if (!signal.aborted) { this.framePresented = false; this.handlers.onState("connecting", "Reconectando à transmissão…"); }
        },
        onReconnected: () => {
          if (!signal.aborted && this.stream?.getVideoTracks().length) { this.observeFirstFrame(signal); this.requestKeyframe(); }
        },
        onDisconnected: () => { if (!signal.aborted && this.live && !this.stopped) void this.subscribe(); },
      });
      this.livekit = viewer;
      await viewer.connect(signal);
      active();
      this.startMediaTimers(viewer, signal);
      return;
    }
    const ice = await this.client.iceServers(signal).catch(() => {
      active();
      return { iceServers: [{ urls: "stun:stun.cloudflare.com:3478" }] };
    });
    active();
    const pc = new RTCPeerConnection({
      iceServers: ice.iceServers, bundlePolicy: "max-bundle",
      iceTransportPolicy: localStorage.getItem("ms.forceRelay") === "1" ? "relay" : "all",
    });
    this.pc = pc;
    pc.ontrack = (event) => {
      if (signal.aborted || this.pc !== pc || this.stopped) return;
      if (event.track.kind === "audio" && !this.audioEnabled) {
        event.track.stop();
        this.video.muted = true;
        return;
      }
      const receiver = event.receiver;
      // O vídeo e o áudio tocam no mesmo MediaStream (o elemento <video> reproduz os dois).
      this.stream ??= new MediaStream();
      this.stream.addTrack(event.track);
      if (this.video.srcObject !== this.stream) this.video.srcObject = this.stream;
      if (event.track.kind === "audio") {
        // Pouco buffer para o som acompanhar a imagem, mas o bastante para não picotar com jitter.
        this.audioLatency = requestReceiverLatency(receiver, 40);
        this.setAudioPresent(true);
        return;
      }
      this.waitingForFrameAt ??= performance.now();
      // Preferência de atraso adicional zero; o buffer efetivo depende do receiver.
      this.videoLatency = requestReceiverLatency(receiver, 0, 0);
      this.observeFirstFrame(signal);
    };
    pc.onconnectionstatechange = () => {
      if (signal.aborted || this.pc !== pc || this.stopped) return;
      if (pc.connectionState === "connected") this.waitingForFrameAt ??= performance.now();
      if (pc.connectionState === "failed" || pc.connectionState === "disconnected") {
        const timer = setTimeout(() => {
          this.mediaTimeouts.delete(timer);
          if (this.pc === pc && !this.stopped && pc.connectionState !== "connected" && this.live) void this.subscribe();
        }, pc.connectionState === "failed" ? 0 : 4000);
        this.mediaTimeouts.add(timer);
      }
    };
    await pc.setRemoteDescription({ type: "offer", sdp: started.offer });
    active();
    const answer = await pc.createAnswer();
    active();
    // Sem `stereo=1` na resposta o navegador decodifica o Opus em mono.
    await pc.setLocalDescription({ type: "answer", sdp: withOpusStereo(answer.sdp ?? "") });
    active();
    await this.gathered(pc, signal);
    active();
    const sdp = pc.localDescription?.sdp;
    if (!sdp) throw new ApiFailure(0, "sdp", "Falha ao preparar a resposta de vídeo.");
    await this.client.request("POST", "/api/watch/answer", { viewerId: started.viewerId, answer: sdp }, true, signal);
    active();
    this.startMediaTimers(pc, signal);
  }

  private startMediaTimers(source: RTCPeerConnection | LiveKitViewer, signal: AbortSignal): void {
    if (!this.framePresented) {
      const timer = setTimeout(() => {
        this.mediaTimeouts.delete(timer);
        this.firstFrameTimer = undefined;
        if (!signal.aborted && !this.framePresented) this.handlers.onState("error", "A conexão foi aberta, mas o vídeo não chegou. Use Tentar novamente para reconectar.");
      }, 15_000);
      this.firstFrameTimer = timer;
      this.mediaTimeouts.add(timer);
    }
    this.mediaTimers.push(
      setInterval(() => void this.heartbeat(), 8000),
      setInterval(() => void this.sample(source), 250),
    );
  }

  private gathered(pc: RTCPeerConnection, signal: AbortSignal): Promise<void> {
    if (pc.iceGatheringState === "complete") return Promise.resolve();
    return new Promise((resolve) => {
      const done = (): void => { pc.removeEventListener("icegatheringstatechange", check); signal.removeEventListener("abort", done); clearTimeout(timer); resolve(); };
      const check = (): void => { if (pc.iceGatheringState === "complete") done(); };
      // Candidatos srflx/relay chegam rápido; não espera mais que 2,5 s.
      const timer = setTimeout(done, 2500);
      pc.addEventListener("icegatheringstatechange", check);
      signal.addEventListener("abort", done, { once: true });
      if (signal.aborted) done();
    });
  }

  private async heartbeat(): Promise<void> {
    const viewerId = this.viewerId;
    const controller = this.mediaAbort;
    if (!viewerId || !controller || this.stopped) return;
    try {
      await this.client.request("POST", "/api/watch/heartbeat", { viewerId }, true, controller.signal);
    } catch (error) {
      if (this.stopped || controller !== this.mediaAbort || viewerId !== this.viewerId) return;
      if (error instanceof ApiFailure && (error.status === 404 || error.status === 409) && this.live) void this.subscribe();
    }
  }

  private async sample(pc: RTCPeerConnection | LiveKitViewer): Promise<void> {
    if ((this.pc !== pc && this.livekit !== pc) || this.stopped || this.sampling === pc) return;
    this.sampling = pc;
    const collectionStartedAt = performance.now();
    let report: RTCStatsReport | null;
    try {
      report = await pc.getStats().catch(() => null);
    } finally {
      if (this.sampling === pc) this.sampling = null;
    }
    if (!report || (this.pc !== pc && this.livekit !== pc) || this.stopped) return;
    let inbound: RTCInboundRtpStreamStats | null = null;
    let audioInbound: RTCInboundRtpStreamStats | null = null;
    report.forEach((stat) => {
      const value = stat as RTCInboundRtpStreamStats;
      if (value.type === "inbound-rtp" && value.kind === "video") inbound = value;
      if (value.type === "inbound-rtp" && value.kind === "audio") audioInbound = value;
    });
    const stat = inbound as RTCInboundRtpStreamStats | null;
    const now = performance.now();
    const audio = audioInbound as RTCInboundRtpStreamStats | null;
    if (audio && this.options.diagnostics !== false) {
      const previousCounters = this.audioDiagnosticSamples.at(-1);
      const counterReset = previousCounters && ["bytesReceived", "packetsReceived", "totalSamplesReceived"].some(key => {
        const current = audio[key as keyof RTCInboundRtpStreamStats];
        const previous = previousCounters[key as keyof AudioDiagnosticSample];
        return typeof current === "number" && typeof previous === "number" && current < previous;
      });
      if (this.audioInboundId !== audio.id || counterReset) {
        this.audioInboundId = audio.id;
        this.audioDiagnosticSegment++;
      }
      const numeric = (key: keyof RTCInboundRtpStreamStats): number | null => {
        const value = audio[key];
        return typeof value === "number" && Number.isFinite(value) ? value : null;
      };
      this.audioDiagnosticSamples.push({
        atMs: Math.round(now), segment: this.audioDiagnosticSegment, provider: this.livekit ? "livekit" : "cloudflare",
        packetsReceived: numeric("packetsReceived"), packetsLost: numeric("packetsLost"), bytesReceived: numeric("bytesReceived"),
        jitter: numeric("jitter"), jitterBufferDelay: numeric("jitterBufferDelay"), jitterBufferEmittedCount: numeric("jitterBufferEmittedCount"),
        totalSamplesReceived: numeric("totalSamplesReceived"), concealedSamples: numeric("concealedSamples"),
        silentConcealedSamples: numeric("silentConcealedSamples"), concealmentEvents: numeric("concealmentEvents"),
        insertedSamplesForDeceleration: numeric("insertedSamplesForDeceleration"), removedSamplesForAcceleration: numeric("removedSamplesForAcceleration"),
        totalAudioEnergy: numeric("totalAudioEnergy"), totalSamplesDuration: numeric("totalSamplesDuration"), audioLevel: numeric("audioLevel"),
      });
      if (this.audioDiagnosticSamples.length > 60) this.audioDiagnosticSamples.shift();
      const audioSample = this.audioDiagnosticSamples.at(-1)!;
      const previousAudio = this.audioDiagnosticHistory.at(-1);
      if (!previousAudio || previousAudio.segment !== audioSample.segment || now - previousAudio.atMs >= 1000) {
        this.audioDiagnosticHistory.push({ ...audioSample });
        if (this.audioDiagnosticHistory.length > 120) this.audioDiagnosticHistory.shift();
        while (this.audioDiagnosticHistory[0] && now - this.audioDiagnosticHistory[0].atMs > 120_000) this.audioDiagnosticHistory.shift();
      }
    } else {
      // O mesmo id pode reaparecer depois de uma nova assinatura com contadores zerados.
      this.audioInboundId = null;
    }
    const frames = stat?.framesDecoded ?? 0;
    if (!stat) {
      if (this.waitingForFrameAt !== null && now - this.waitingForFrameAt >= 1000) this.requestKeyframe("first-frame");
      return;
    }
    const bytes = stat.bytesReceived ?? 0;
    const lost = stat.packetsLost ?? 0;
    const received = stat.packetsReceived ?? 0;
    const previousCounters = this.recoveryPrevious;
    const counterReset = previousCounters && (
      (typeof stat.framesDecoded === "number" && frames < previousCounters.frames)
      || (typeof stat.bytesReceived === "number" && bytes < previousCounters.bytes)
      || (typeof stat.packetsReceived === "number" && received < previousCounters.received));
    if (this.inboundId !== stat.id || counterReset) {
      this.diagnosticSegment++;
      this.inboundId = stat.id;
      this.previous = null;
      this.recoveryPrevious = null;
      this.lastDecodeAt = now;
      this.decodeStalledAt = null;
      this.lossStalledAt = null;
    }
    if (frames > 0) this.waitingForFrameAt = null;
    else if (this.waitingForFrameAt !== null && now - this.waitingForFrameAt >= 1000) this.requestKeyframe("first-frame");
    const recovery = this.recoveryPrevious;
    if (frames > 0 && (!recovery || frames > recovery.frames)) {
      for (const item of this.recoveryObservations) {
        if (item.segment === this.diagnosticSegment && item.decodeResumedAtMs === null) item.decodeResumedAtMs = Math.round(now);
      }
    }
    if (recovery) {
      if (frames > recovery.frames) {
        this.lastDecodeAt = now;
        this.decodeStalledAt = null;
        this.lossStalledAt = null;
      }
      // Uma perda na mesma amostra de um decode pode travar os quadros seguintes.
      if (lost > recovery.lost) this.lossStalledAt ??= now;
      const stalled = frames === recovery.frames;
      const receiving = bytes > recovery.bytes && received > recovery.received;
      if (stalled && receiving) this.decodeStalledAt ??= recovery.at;
      // Conserva a janela entre rajadas; sem novos pacotes, silêncio não dispara recuperação.
      // Sem perda reportada também pode faltar uma referência. Não espera 1 s para recuperar
      // vídeo com pacotes chegando; silêncio de uma tela parada continua sem gerar pedidos.
      if (stalled && receiving && this.lossStalledAt !== null && now - (this.lastDecodeAt ?? this.lossStalledAt) >= 300) this.requestKeyframe("loss");
      else if (stalled && receiving && this.decodeStalledAt !== null && now - this.decodeStalledAt >= 500) this.requestKeyframe("decode-stall");
    } else {
      this.lastDecodeAt = now;
    }
    this.recoveryPrevious = { bytes, frames, received, lost, at: now };
    const rttMs = videoRttMs(report, stat);
    if (this.options.diagnostics !== false) {
      const previousSample = this.diagnosticSamples.at(-1);
      this.diagnosticSamples.push({
        atMs: Math.round(now), segment: this.diagnosticSegment, provider: this.livekit ? "livekit" : "cloudflare",
        statsCollectionMs: Math.max(0, now - collectionStartedAt),
        sampleGapMs: previousSample?.segment === this.diagnosticSegment ? Math.max(0, now - previousSample.atMs) : null,
        framesReceived: typeof stat.framesReceived === "number" && Number.isFinite(stat.framesReceived) ? stat.framesReceived : null,
        framesDropped: typeof stat.framesDropped === "number" && Number.isFinite(stat.framesDropped) ? stat.framesDropped : null,
        freezeCount: typeof stat.freezeCount === "number" && Number.isFinite(stat.freezeCount) ? stat.freezeCount : null,
        totalFreezesDurationSeconds: typeof stat.totalFreezesDuration === "number" && Number.isFinite(stat.totalFreezesDuration) ? stat.totalFreezesDuration : null,
        maxPresentationGapMs: this.maxPresentationGapMs,
        rttMs, framesDecoded: frames, presentedFrames: this.presentedFrames,
        presentationGapMs: this.lastPresentedAt === null ? null : Math.max(0, Math.round(now - this.lastPresentedAt)),
        decodeGapMs: this.lastDecodeAt === null ? null : Math.max(0, Math.round(now - this.lastDecodeAt)),
        bytesReceived: bytes, packetsReceived: received, packetsLost: lost, nackCount: stat.nackCount ?? 0, pliCount: stat.pliCount ?? 0,
        jitterMs: stat.jitter === undefined ? null : stat.jitter * 1000,
        jitterBufferDelaySeconds: stat.jitterBufferDelay ?? null, jitterBufferEmittedCount: stat.jitterBufferEmittedCount ?? null,
        totalDecodeTimeSeconds: stat.totalDecodeTime ?? null, documentHidden: typeof document !== "undefined" && document.hidden,
      });
      if (this.diagnosticSamples.length > 60) this.diagnosticSamples.shift();
      const recent = this.diagnosticSamples.at(-1)!;
      const previousHistory = this.diagnosticHistory.at(-1);
      if (!previousHistory || previousHistory.segment !== recent.segment || now - previousHistory.atMs >= 1000) {
        this.diagnosticHistory.push({ ...recent });
        if (this.diagnosticHistory.length > 120) this.diagnosticHistory.shift();
        while (this.diagnosticHistory[0] && now - this.diagnosticHistory[0].atMs > 120_000) this.diagnosticHistory.shift();
        this.maxPresentationGapMs = null;
      }
    }
    // A detecção usa 250 ms; o HUD mantém deltas e callbacks de aproximadamente 1 s.
    if (this.previous && now - this.previous.at < 1000) return;
    const jitterDelay = stat.jitterBufferDelay ?? 0;
    const jitterCount = stat.jitterBufferEmittedCount ?? 0;
    if (this.previous) {
      const seconds = (now - this.previous.at) / 1000;
      const fps = (frames - this.previous.frames) / seconds;
      const emitted = jitterCount - this.previous.jitterCount;
      const delay = jitterDelay - this.previous.jitterDelay;
      const lostDelta = Math.max(0, lost - this.previous.lost);
      const receivedDelta = Math.max(0, received - this.previous.received);
      this.handlers.onStats?.({
        width: stat.frameWidth ?? 0,
        height: stat.frameHeight ?? 0,
        fps,
        mbps: ((bytes - this.previous.bytes) * 8) / seconds / 1e6,
        rttMs,
        jitterBufferMs: emitted > 0 && delay >= 0 ? delay / emitted * 1000 : null,
        lossPct: receivedDelta + lostDelta > 0 ? lostDelta / (receivedDelta + lostDelta) * 100 : null,
      });
    }
    this.previous = { bytes, frames, received, lost, jitterDelay, jitterCount, at: now };
  }
}

// ---- eventos da própria transmissão (anfitrião) ----------------------------------------------

export interface StreamEvent {
  type: "hello" | "live" | "ended" | "viewer_joined" | "viewer_left" | "keyframe";
  viewers?: number;
  name?: string;
  live?: boolean;
  viewerId?: string;
  uid?: string;
  participants?: { viewerId: string; uid: string; name: string }[];
}

/** Quem está assistindo agora: o anfitrião recebe entrou/saiu e pedidos de quadro-chave. */
export function connectStreamEvents(client: MadClient, owner: string, onEvent: (event: StreamEvent) => void): { close(): void } {
  let closed = false;
  let socket: WebSocket | null = null;
  let failures = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let ping: ReturnType<typeof setInterval> | undefined;
  const open = (): void => {
    if (closed || !client.session) return;
    const ws = new WebSocket(client.wsUrl(`/api/stream/ws?owner=${encodeURIComponent(owner)}`), [`ms.${client.token}`]);
    socket = ws;
    ws.onopen = () => {
      if (closed || socket !== ws) return;
      failures = 0;
      clearInterval(ping);
      ping = setInterval(() => { if (ws.readyState === WebSocket.OPEN) ws.send("ping"); }, 25_000);
    };
    ws.onmessage = (event: MessageEvent) => {
      if (closed || socket !== ws || typeof event.data !== "string" || event.data === "pong") return;
      try {
        onEvent(JSON.parse(event.data) as StreamEvent);
      } catch {
        // Mensagem desconhecida.
      }
    };
    ws.onclose = () => {
      if (closed || socket !== ws) return;
      clearInterval(ping);
      failures++;
      timer = setTimeout(open, Math.min(10_000, 1000 * failures));
    };
  };
  open();
  return { close: () => { closed = true; clearTimeout(timer); clearInterval(ping); socket?.close(); } };
}
