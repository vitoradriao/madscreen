import { DurableObject } from "cloudflare:workers";
import { ApiError, errorResponse, integerField, invalidBody, json, readJson, sdpField, textField, type Env } from "./common";
import { activeProvider, selectedProvider, allocationProvider, createProvider, iceServers, SfuError, type ProviderId, type SfuProvider } from "./sfu";
import { sameDestination, type StreamDestination } from "./direct";

const MAX_VIEWERS = 8;
const ALARM_INTERVAL = 10_000;
const PUBLISHER_TIMEOUT = 25_000;
const VIEWER_TIMEOUT = 40_000;

interface Allocation {
  provider?: ProviderId;
  sessionId: string;
  mids: string[];
}
interface Publisher extends Allocation {
  uid: string;
  name: string;
  trackName: "screen";
  channelId?: string;
  destination?: StreamDestination;
  generation: number;
  lastHeartbeat: number;
  since: number;
  title: string | null;
  /** A transmissão publica também a faixa de áudio (`audio`). Ausente em estados antigos = sem áudio. */
  audio?: boolean;
  /** Um espectador pediu quadro-chave; o publicador lê no próximo heartbeat. */
  keyframe: boolean;
}
interface Viewer extends Allocation {
  viewerId: string;
  uid: string;
  name: string;
  generation: number;
  lastSeen: number;
  answered: boolean;
}
interface PendingCleanup extends Allocation {
  // Inspeciona depois de alocações incertas, inclusive após reinício do isolate.
  inspect: boolean;
  createdAt: number;
}
interface StreamState {
  generation: number;
  publisher: Publisher | null;
  viewers: Viewer[];
  pending: PendingCleanup[];
  /** Issued tokens remain relevant after watch/leave and server renewal. */
  authorizedViewerUids?: string[];
}
/** Identidade já autenticada pelo Worker (cabeçalhos x-ms-*, sempre sobrescritos lá). */
interface Caller {
  uid: string;
  name: string;
  owner: string;
  channelId: string | null;
  destination: StreamDestination | null;
  sessionDigest: string | null;
  sessionIat: number;
}

/**
 * Uma transmissão por usuário (instância nomeada pelo id do dono). O Worker já autenticou a
 * sessão e já conferiu a amizade no Hub; aqui ficam o estado do SFU, a fila de mutações por
 * sessão, a limpeza e o WebSocket de eventos (ao vivo, entrou/saiu, pedido de quadro-chave).
 */
export class StreamDO extends DurableObject<Env> {
  private state: StreamState = { generation: 0, publisher: null, viewers: [], pending: [], authorizedViewerUids: [] };
  private roomQueue: Promise<void> = Promise.resolve();
  private readonly sessionQueues = new Map<string, Promise<void>>();
  private readonly providers = new Map<ProviderId, SfuProvider>();

  private sfu(allocation?: Pick<Allocation, "provider">): SfuProvider {
    const id = allocation ? allocationProvider(allocation) : activeProvider(this.env);
    let provider = this.providers.get(id);
    if (!provider) {
      provider = createProvider(this.env, id);
      this.providers.set(id, provider);
    }
    return provider;
  }

  private requireProviderClient(body: Record<string, unknown>, provider: ProviderId): void {
    if (provider === "livekit" && (!Array.isArray(body.providers) || !body.providers.includes("livekit"))) {
      throw new ApiError(409, "cliente_desatualizado", "Esta transmissão exige um cliente com suporte ao LiveKit.");
    }
  }

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    ctx.storage.sql.exec("CREATE TABLE IF NOT EXISTS room_state (id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL)");
    ctx.storage.sql.exec("CREATE TABLE IF NOT EXISTS rate_limits (uid TEXT PRIMARY KEY, window_start INTEGER NOT NULL, count INTEGER NOT NULL)");
    const row = ctx.storage.sql.exec<{ data: string }>("SELECT data FROM room_state WHERE id = 1").toArray()[0];
    if (row) this.state = JSON.parse(row.data) as StreamState;
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair("ping", "pong"));
    void ctx.blockConcurrencyWhile(async () => { await this.scheduleAlarm(); });
  }

  private enqueue<T>(operation: () => Promise<T>): Promise<T> {
    const next = this.roomQueue.then(operation);
    this.roomQueue = next.then(() => undefined, () => undefined);
    return next;
  }

  private async inSession<T>(sessionId: string, operation: () => Promise<T>): Promise<T> {
    const previous = this.sessionQueues.get(sessionId) ?? Promise.resolve();
    const next = previous.then(operation);
    const tail = next.then(() => undefined, () => undefined);
    this.sessionQueues.set(sessionId, tail);
    try {
      return await next;
    } finally {
      if (this.sessionQueues.get(sessionId) === tail) this.sessionQueues.delete(sessionId);
    }
  }

  private async scheduleAlarm(): Promise<void> {
    const { publisher, viewers, pending } = this.state;
    if ((publisher !== null || viewers.length > 0 || pending.length > 0) && await this.ctx.storage.getAlarm() === null) {
      await this.ctx.storage.setAlarm(Date.now() + ALARM_INTERVAL);
    }
  }

  private async save(): Promise<void> {
    this.ctx.storage.sql.exec("INSERT INTO room_state (id, data) VALUES (1, ?) ON CONFLICT(id) DO UPDATE SET data = excluded.data", JSON.stringify(this.state));
    await this.scheduleAlarm();
  }

  private rateLimit(uid: string): void {
    const now = Date.now();
    const sql = this.ctx.storage.sql;
    sql.exec("DELETE FROM rate_limits WHERE window_start <= ?", now - 10_000);
    const row = sql.exec<{ count: number }>("SELECT count FROM rate_limits WHERE uid = ?", uid).toArray()[0];
    if (row && row.count >= 60) throw new ApiError(429, "limite_requisicoes", "Muitas requisições. Aguarde alguns segundos.");
    sql.exec("INSERT INTO rate_limits (uid, window_start, count) VALUES (?, ?, 1) ON CONFLICT(uid) DO UPDATE SET count = count + 1", uid, now);
  }

  // ---- eventos em tempo real ----------------------------------------------------------------

  private broadcast(message: Record<string, unknown>, tag?: string): void {
    const text = JSON.stringify(message);
    for (const socket of this.ctx.getWebSockets(...(tag ? [tag] : []))) {
      if (socket.readyState !== WebSocket.READY_STATE_OPEN) continue;
      try {
        socket.send(text);
      } catch {
        // O evento de close remove o socket.
      }
    }
  }

  private status(): Record<string, unknown> {
    return {
      live: this.state.publisher !== null,
      generation: this.state.generation,
      publisherName: this.state.publisher?.name ?? null,
      viewers: this.state.viewers.length,
      trackName: this.state.publisher?.trackName ?? null,
      audio: this.state.publisher?.audio === true,
    };
  }

  /** Reflete "ao vivo" e contagem de espectadores no Hub (lista de amigos). */
  private async syncHub(owner: string): Promise<void> {
    const publisher = this.state.publisher;
    try {
      if (this.env.SFU_FAILOVER_ENABLED === "1") {
        await this.env.HUB.get(this.env.HUB.idFromName("hub")).recordMediaUsage(owner, this.state.generation,
          publisher ? allocationProvider(publisher) : null, publisher ? this.state.viewers.length : 0);
      }
      const accepted = await this.env.HUB.get(this.env.HUB.idFromName("hub")).setLive(owner,
        publisher ? { since: publisher.since, viewers: this.state.viewers.length, title: publisher.title, ...(publisher.destination ? { destination: publisher.destination } : publisher.channelId ? { channelId: publisher.channelId } : {}) } : null);
      if (!accepted && this.state.publisher) await this.retirePublisher(owner);
    } catch {
      // O Hub é consistente depois: o próximo evento corrige.
    }
  }

  // ---- HTTP / WebSocket -----------------------------------------------------------------------

  private caller(request: Request): Caller {
    const uid = request.headers.get("x-ms-uid");
    const name = request.headers.get("x-ms-name");
    const owner = request.headers.get("x-ms-owner");
    if (!uid || !name || !owner) throw new ApiError(401, "sessao_invalida", "Sessão inválida.");
    const target = request.headers.get("x-ms-destination");
    return { uid, name, owner, channelId: request.headers.get("x-ms-channel"), destination: target ? JSON.parse(target) as StreamDestination : null, sessionDigest: request.headers.get("x-ms-session"), sessionIat: Number(request.headers.get("x-ms-session-iat") ?? 0) };
  }

  private async assertCallerSession(caller: Caller): Promise<void> {
    // The Worker strips client x-ms-* headers and supplies only the digest.
    // Missing digest retains compatibility with already-forwarded legacy calls.
    if (caller.sessionDigest) await this.env.HUB.get(this.env.HUB.idFromName("hub")).assertSession(caller.uid, caller.sessionDigest, caller.sessionIat);
  }

  private async authorizeCurrent(caller: Caller): Promise<void> {
    const publisher = this.state.publisher;
    await this.env.HUB.get(this.env.HUB.idFromName("hub")).canWatch(caller.uid, caller.owner, publisher?.destination ?? publisher?.channelId);
    if (publisher !== this.state.publisher) throw new ApiError(409, "geracao_antiga", "A transmissão mudou. Abra novamente.");
  }

  override async fetch(request: Request): Promise<Response> {
    try {
      const caller = this.caller(request);
      const url = new URL(request.url);
      if (request.headers.get("Upgrade")?.toLowerCase() === "websocket") {
        await this.assertCallerSession(caller);
        await this.authorizeCurrent(caller);
        return this.accept(request, caller);
      }
      const body = request.method === "POST" ? await readJson(request) : {};
      this.rateLimit(caller.uid);
      // Leituras e heartbeats só tocam campos simples: não esperam a fila de mutações do SFU.
      if (url.pathname === "/api/stream/status" || url.pathname === "/api/ice" || url.pathname === "/api/share/heartbeat" || url.pathname === "/api/watch/heartbeat") {
        return await this.dispatch(request, url, caller, body);
      }
      return await this.enqueue(async () => {
        return this.dispatch(request, url, caller, body);
      });
    } catch (error) {
      return errorResponse(error);
    }
  }

  private accept(request: Request, caller: Caller): Response {
    const pair = new WebSocketPair();
    this.ctx.acceptWebSocket(pair[1], [caller.uid === caller.owner ? "host" : "viewer", `uid:${caller.uid}`]);
    const participants = caller.uid === caller.owner
      ? this.state.viewers.map(({ viewerId, uid, name }) => ({ viewerId, uid, name })) : undefined;
    pair[1].send(JSON.stringify({ type: "hello", ...this.status(), participants }));
    const headers = new Headers();
    const protocol = request.headers.get("x-ms-protocol");
    if (protocol) headers.set("Sec-WebSocket-Protocol", protocol);
    return new Response(null, { status: 101, webSocket: pair[0], headers });
  }

  override async webSocketMessage(socket: WebSocket, message: string | ArrayBuffer): Promise<void> {
    if (typeof message !== "string" || message.length > 256) return;
    let value: unknown;
    try {
      value = JSON.parse(message);
    } catch {
      return;
    }
    if (typeof value !== "object" || value === null || (value as { type?: unknown }).type !== "keyframe") return;
    // Espectadores e a prévia do dono podem pedir; o publicador lê no heartbeat.
    const tags = this.ctx.getTags(socket);
    if (!tags.includes("viewer") && !tags.includes("host")) return;
    await this.enqueue(async () => {
      if (!this.state.publisher) return;
      this.state.publisher.keyframe = true;
      await this.save();
      this.broadcast({ type: "keyframe" }, "host");
    });
  }

  override webSocketClose(socket: WebSocket): void {
    try {
      socket.close(1000, "bye");
    } catch {
      // Já fechado.
    }
  }

  /** Revoga imediatamente a mídia de um usuário que saiu do servidor. */
  async revokeViewer(uid: string): Promise<void> {
    await this.enqueue(async () => {
      const viewers = this.state.viewers.filter(viewer => viewer.uid === uid);
      // Self-hosted LiveKit does not invalidate copied/refreshed JWTs on removal.
      // Retire the entire generation: the old room must never carry new private media.
      // A subsequent explicit publication obtains a new random room and tokens.
      const retired: Allocation[] = [];
      if (this.state.publisher && allocationProvider(this.state.publisher) === "livekit"
          && (this.state.authorizedViewerUids === undefined || this.state.authorizedViewerUids.includes(uid))) {
        retired.push(this.state.publisher);
        const owner = this.state.publisher.uid;
        await this.retirePublisher(owner);
      }
      for (const viewer of viewers.filter(item => this.state.viewers.includes(item))) {
        retired.push(viewer);
        await this.retireViewer(viewer.viewerId.split(".")[0] ?? "", viewer);
      }
      // Do not report confirmed revocation when the SFU did not acknowledge close.
      // The durable pending list already exists and the alarm can retry on failure.
      await this.confirmRetired(retired);
      await this.cleanPending();
      for (const socket of this.ctx.getWebSockets(`uid:${uid}`)) {
        try { socket.close(4003, "acesso revogado"); } catch { /* Já fechado. */ }
      }
    });
  }

  private async confirmRetired(retired: Allocation[] = []): Promise<void> {
    // Include durable retirements on retries after a failed first close.
    const allocations = [...new Map([...retired, ...this.state.pending].map(allocation => [allocation.sessionId, allocation])).values()];
    // DeleteRoom closes every participant. Concurrent RemoveParticipant during room teardown
    // can fail even after the room is gone; confirm the room once instead.
    const closingRooms = new Set(allocations.filter(allocation => allocationProvider(allocation) === "livekit" && allocation.sessionId.split("/")[1]?.startsWith("p-")).map(allocation => allocation.sessionId.split("/")[0]));
    const confirmations = allocations.filter(allocation => allocationProvider(allocation) !== "livekit" || !allocation.sessionId.split("/")[1]?.startsWith("v-") || !closingRooms.has(allocation.sessionId.split("/")[0]));
    const results = await Promise.allSettled(confirmations.map(async allocation => {
      try { await this.inSession(allocation.sessionId, () => this.sfu(allocation).close(allocation.sessionId, allocation.mids)); }
      catch (error) {
        if (!(error instanceof SfuError) || ![404, 410].includes(error.upstreamStatus)) throw error;
      }
    }));
    if (results.some(result => result.status === "rejected")) {
      throw new ApiError(502, "revogacao_pendente", "O acesso foi removido, mas o encerramento da mídia ainda não foi confirmado.");
    }
  }

  private async dispatch(request: Request, url: URL, caller: Caller, body: Record<string, unknown>): Promise<Response> {
    await this.assertCallerSession(caller);
    if (url.pathname.startsWith("/api/stream/") || url.pathname.startsWith("/api/watch/") && url.pathname !== "/api/watch/leave") await this.authorizeCurrent(caller);
    const route = `${request.method} ${url.pathname}`;
    switch (route) {
      case "POST /api/share/start":
        this.requireOwner(caller);
        return this.startPublisher(caller, body);
      case "POST /api/share/heartbeat": {
        this.requireOwner(caller);
        const publisher = this.currentPublisher(body);
        await this.env.HUB.get(this.env.HUB.idFromName("hub")).resolvePublished(caller.uid, publisher.destination ?? publisher.channelId);
        publisher.lastHeartbeat = Date.now();
        const keyframe = publisher.keyframe;
        publisher.keyframe = false;
        await this.save();
        return json({ ok: true, viewers: this.state.viewers.length, generation: this.state.generation, keyframe });
      }
      case "POST /api/share/stop":
        this.requireOwner(caller);
        this.currentPublisher(body);
        await this.retirePublisher(caller.owner);
        await this.confirmRetired();
        void this.cleanPending();
        return json({ ok: true });
      case "POST /api/watch/start":
        return this.startViewer(caller, body);
      case "POST /api/watch/answer": {
        const viewer = this.currentViewer(caller, body);
        const answer = sdpField(body.answer);
        if (viewer.answered) throw new ApiError(409, "resposta_ja_enviada", "A resposta desta conexão já foi enviada.");
        try {
          await this.inSession(viewer.sessionId, () => this.sfu(viewer).renegotiate(viewer.sessionId, answer));
        } catch (error) {
          await this.retireViewer(caller.owner, viewer);
          void this.cleanPending();
          throw error;
        }
        viewer.answered = true;
        viewer.lastSeen = Date.now();
        await this.save();
        return json({ ok: true });
      }
      case "POST /api/watch/heartbeat": {
        const viewer = this.currentViewer(caller, body);
        await this.env.HUB.get(this.env.HUB.idFromName("hub")).canWatch(caller.uid, caller.owner, this.state.publisher?.destination ?? this.state.publisher?.channelId);
        viewer.lastSeen = Date.now();
        await this.save();
        return json({ ok: true, live: this.state.publisher !== null, generation: this.state.generation });
      }
      case "POST /api/watch/layer": {
        const viewer = this.currentViewer(caller, body);
        if (body.preferredRid !== "h") throw new ApiError(409, "camada_indisponivel", "Esta transmissão publica uma única camada de vídeo.");
        await this.inSession(viewer.sessionId, () => this.sfu(viewer).selectLayer(viewer.sessionId, "high"));
        return json({ ok: true, preferredRid: "h", layers: ["h"] });
      }
      case "POST /api/watch/leave": {
        const viewer = this.currentViewer(caller, body);
        await this.retireViewer(caller.owner, viewer);
        void this.cleanPending();
        return json({ ok: true });
      }
      case "GET /api/stream/status":
        return json(this.status());
      case "GET /api/ice":
        return json(await iceServers(this.env));
      default:
        throw new ApiError(404, "rota_nao_encontrada", "Rota não encontrada.");
    }
  }

  private requireOwner(caller: Caller): void {
    if (caller.uid !== caller.owner) throw new ApiError(403, "sem_permissao", "Só o dono transmite nesta sala.");
  }

  /** Chamado ao apagar a conta. */
  async forceStop(owner: string): Promise<void> {
    await this.enqueue(async () => {
      await this.retirePublisher(owner);
      await this.confirmRetired();
      await this.cleanPending();
    });
  }

  async forceStopDirect(owner: string, sessionId: string): Promise<void> {
    await this.enqueue(async () => {
      if (this.state.publisher?.destination?.kind === "direct" && this.state.publisher.destination.sessionId === sessionId) await this.retirePublisher(owner);
      await this.confirmRetired();
      await this.cleanPending();
    });
  }

  private currentPublisher(body: Record<string, unknown>): Publisher {
    const generation = integerField(body.generation);
    const publisher = this.state.publisher;
    if (generation !== this.state.generation || !publisher) {
      throw new ApiError(409, "geracao_antiga", "A publicação já foi substituída ou encerrada.");
    }
    return publisher;
  }

  private currentViewer(caller: Caller, body: Record<string, unknown>): Viewer {
    const viewerId = textField(body.viewerId);
    const viewer = this.state.viewers.find((item) => item.viewerId === viewerId);
    if (!viewer) throw new ApiError(404, "viewer_nao_encontrado", "Conexão de visualização encerrada.");
    if (viewer.uid !== caller.uid) throw new ApiError(403, "sem_permissao", "Esta conexão pertence a outra pessoa.");
    return viewer;
  }

  private addPending(allocation: Allocation, inspect = false): void {
    if (this.state.pending.some((item) => item.sessionId === allocation.sessionId)) return;
    this.state.pending.push({ provider: allocationProvider(allocation), sessionId: allocation.sessionId, mids: [...allocation.mids], inspect: inspect || allocationProvider(allocation) === "livekit", createdAt: Date.now() });
  }

  private async retirePublisher(owner: string): Promise<void> {
    if (!this.state.publisher) return;
    this.addPending(this.state.publisher);
    for (const viewer of this.state.viewers) this.addPending(viewer);
    this.state.publisher = null;
    this.state.viewers = [];
    this.state.generation++;
    await this.save();
    this.broadcast({ type: "ended", generation: this.state.generation });
    for (const socket of this.ctx.getWebSockets()) {
      try { socket.close(1000, "transmissao encerrada"); } catch { /* Already closed. */ }
    }
    await this.syncHub(owner);
  }

  private async retireViewer(owner: string, viewer: Viewer): Promise<void> {
    this.addPending(viewer);
    this.state.viewers = this.state.viewers.filter((item) => item.viewerId !== viewer.viewerId);
    await this.save();
    this.broadcast({ type: "viewer_left", viewerId: viewer.viewerId, uid: viewer.uid, name: viewer.name, viewers: this.state.viewers.length }, "host");
    await this.syncHub(owner);
  }

  private cleaning: Promise<void> | null = null;

  /**
   * Fecha no SFU o que ficou pendente. Melhor esforço e fora da fila da sala: uma sessão lenta
   * não pode travar status, heartbeat nem novas transmissões; o alarme repete até expirar.
   */
  private cleanPending(): Promise<void> {
    if (this.cleaning) return this.cleaning;
    const run = async (): Promise<void> => {
      await Promise.all([...this.state.pending].map(async (pending) => {
        try {
          await this.inSession(pending.sessionId, async () => {
            if (pending.inspect && allocationProvider(pending) === "cloudflare") {
              const tracks = await this.sfu(pending).inspect(pending.sessionId);
              const inactive = new Set(tracks.filter((track) => track.status === "inactive").map((track) => track.mid));
              pending.mids = [...new Set([...pending.mids, ...tracks.map((track) => track.mid)])].filter((mid) => !inactive.has(mid));
            }
            await this.sfu(pending).close(pending.sessionId, pending.mids);
          });
          // CF: tracks/new incerto pode concluir depois do timeout. LiveKit self-hosted:
          // remover participante não revoga JWT; repete fechamento além do TTL de 60 s.
          const cleanupWindow = allocationProvider(pending) === "livekit" ? 70_000 : 60_000;
          if (!pending.inspect || Date.now() - pending.createdAt > cleanupWindow) {
            this.state.pending = this.state.pending.filter((item) => item.sessionId !== pending.sessionId);
          } else {
            pending.mids = [];
          }
        } catch (error) {
          // 404/410 = a sessão já não existe; passados 10 min a alocação expirou no SFU de qualquer forma.
          const gone = error instanceof SfuError && (error.upstreamStatus === 404 || error.upstreamStatus === 410);
          const expiredLiveKitToken = allocationProvider(pending) !== "livekit" || Date.now() - pending.createdAt > 70_000;
          if ((gone && expiredLiveKitToken) || Date.now() - pending.createdAt > 600_000) {
            this.state.pending = this.state.pending.filter((item) => item.sessionId !== pending.sessionId);
          } else {
            pending.inspect = true;
          }
        }
      }));
      await this.save();
    };
    this.cleaning = run().finally(() => { this.cleaning = null; });
    return this.cleaning;
  }

  private async startPublisher(caller: Caller, body: Record<string, unknown>): Promise<Response> {
    const sfu = this.sfu({ provider: await selectedProvider(this.env) });
    const provider = sfu.id;
    this.requireProviderClient(body, provider);
    if (body.expectedProvider !== undefined && body.expectedProvider !== provider) {
      throw new ApiError(409, "provedor_alterado", "O provedor mudou. Inicie a transmissão novamente.");
    }
    const offer = provider === "cloudflare" ? sdpField(body.offer) : "";
    const mid = provider === "cloudflare" ? textField(body.mid, 32) : "screen";
    const audioMid = body.audioMid === undefined || body.audioMid === null ? undefined : textField(body.audioMid, 32);
    if (audioMid === mid) invalidBody();
    if (body.trackName !== undefined && body.trackName !== "screen") invalidBody();
    const title = body.title === undefined || body.title === null ? null : textField(body.title, 80);
    const target = await this.env.HUB.get(this.env.HUB.idFromName("hub")).resolveShare(caller.uid, caller.destination ?? caller.channelId ?? undefined, true);
    const current = this.state.publisher;
    const currentTarget = current?.destination ?? (current?.channelId ? { kind: "channel" as const, channelId: current.channelId } : null);
    if (current && !sameDestination(currentTarget, target.destination)) throw new ApiError(409, "encerre_transmissao", "Pare a transmissão atual antes de mudar o público.");
    if (target.destination.kind === "direct" && provider !== "livekit") throw new ApiError(409, "sfu_incompativel", "Esta sessão privada exige LiveKit.");
    await this.retirePublisher(caller.owner);
    // A substituição só é anunciada depois do fechamento forçado do publicador antigo, mas uma
    // limpeza que falha não pode impedir uma nova transmissão: o alarme repete até expirar.
    void this.cleanPending();
    const startedAt = Date.now();
    let sessionId: string;
    try { sessionId = await sfu.newSession({ role: "publisher" }); }
    catch (error) {
      await this.recordOutcome(provider, false, startedAt, error);
      throw error;
    }
    this.addPending({ provider, sessionId, mids: audioMid ? [mid, audioMid] : [mid] }, true);
    await this.save();
    try {
      const result = await this.inSession(sessionId, () => sfu.publish(sessionId, offer, mid, audioMid));
      await this.recordOutcome(provider, true, startedAt);
      await this.env.HUB.get(this.env.HUB.idFromName("hub")).resolveShare(caller.uid, target.destination, true);
      await this.assertCallerSession(caller);
      this.state.generation++;
      this.state.authorizedViewerUids = [];
      this.state.publisher = {
        provider, sessionId, mids: result.tracks.map((track) => track.mid), uid: caller.uid, name: caller.name,
        trackName: "screen", destination: target.destination, ...(target.destination.kind === "channel" ? { channelId: target.destination.channelId } : {}), generation: this.state.generation, lastHeartbeat: Date.now(),
        since: Math.floor(Date.now() / 1000), title, keyframe: false, audio: audioMid !== undefined,
      };
      this.state.pending = this.state.pending.filter((item) => item.sessionId !== sessionId);
      await this.save();
      this.broadcast({ type: "live", generation: this.state.generation, publisherName: caller.name });
      await this.syncHub(caller.owner);
      if (this.state.publisher?.sessionId !== sessionId) throw new ApiError(403, "sem_acesso_servidor", "O canal não está mais disponível para transmitir.");
      return json({ provider, layers: ["h"], ...(result.signaling === "sdp" ? { answer: result.sdp } : { livekit: result }), sessionId, generation: this.state.generation, trackName: "screen" });
    } catch (error) {
      if (error instanceof SfuError) await this.recordOutcome(provider, false, startedAt, error);
      void this.cleanPending();
      throw error;
    }
  }

  private async recordOutcome(provider: ProviderId, ok: boolean, startedAt: number, error?: unknown): Promise<void> {
    if (this.env.SFU_FAILOVER_ENABLED !== "1") return;
    try {
      await this.env.HUB.get(this.env.HUB.idFromName("hub")).recordSfuOutcome(provider, ok,
        Date.now() - startedAt, error instanceof SfuError ? error.upstreamStatus : 0);
    } catch { /* Telemetria não pode invalidar uma alocação já concluída. */ }
  }

  private async startViewer(caller: Caller, body: Record<string, unknown>): Promise<Response> {
    const publisher = this.state.publisher;
    if (!publisher) throw new ApiError(409, "sem_publicador", "Este amigo não está transmitindo agora.");
    await this.env.HUB.get(this.env.HUB.idFromName("hub")).canWatch(caller.uid, caller.owner, publisher.destination ?? publisher.channelId);
    const target = publisher.destination ?? (publisher.channelId ? { kind: "channel" as const, channelId: publisher.channelId } : undefined);
    if (body.destination !== undefined && (!target || !sameDestination(body.destination, target))
        || target?.kind === "direct" && caller.uid !== publisher.uid && body.destination === undefined) {
      throw new ApiError(409, "destino_alterado", "O destino da transmissão mudou. Abra Amigos e escolha Assistir novamente.");
    }
    const sfu = this.sfu(publisher);
    const provider = sfu.id;
    this.requireProviderClient(body, provider);
    if (body.preferredRid !== undefined && body.preferredRid !== null && body.preferredRid !== "h") {
      throw new ApiError(409, "camada_indisponivel", "Esta transmissão publica uma única camada de vídeo.");
    }
    if (this.state.viewers.length >= MAX_VIEWERS) throw new ApiError(429, "limite_viewers", "A transmissão atingiu o limite de espectadores.");
    // Clientes antigos e a prévia do próprio dono recebem somente vídeo, mesmo pedindo áudio.
    const withAudio = body.audio === true && publisher.audio === true && caller.uid !== publisher.uid;
    // Assinaturas antigas precisam parar de encaminhar antes de admitir novas (melhor esforço).
    void this.cleanPending();
    if (provider === "livekit" && this.state.authorizedViewerUids !== undefined && !this.state.authorizedViewerUids.includes(caller.uid)) {
      if (this.state.authorizedViewerUids.length >= 512) throw new ApiError(429, "limite_autorizacoes", "Esta transmissão atingiu o limite de autorizações. Inicie uma nova transmissão.");
      this.state.authorizedViewerUids.push(caller.uid);
    }
    const sessionId = await sfu.newSession({ role: "viewer", publisherId: publisher.sessionId });
    this.addPending({ provider, sessionId, mids: [] }, true);
    await this.save();
    try {
      const result = await this.inSession(sessionId, () => sfu.subscribe(sessionId, publisher.sessionId, withAudio));
      await this.env.HUB.get(this.env.HUB.idFromName("hub")).canWatch(caller.uid, caller.owner, publisher.destination ?? publisher.channelId);
      await this.assertCallerSession(caller);
      const viewerId = `${caller.owner}.${crypto.randomUUID()}`;
      this.state.viewers.push({
        provider, viewerId, sessionId, mids: result.tracks.map((track) => track.mid), uid: caller.uid, name: caller.name,
        generation: this.state.generation, lastSeen: Date.now(), answered: result.signaling !== "sdp",
      });
      this.state.pending = this.state.pending.filter((item) => item.sessionId !== sessionId);
      publisher.keyframe = true;
      this.broadcast({ type: "keyframe" }, "host");
      await this.save();
      this.broadcast({ type: "viewer_joined", viewerId, uid: caller.uid, name: caller.name, viewers: this.state.viewers.length }, "host");
      await this.syncHub(caller.owner);
      return json({ provider, viewerId, layers: ["h"], ...(result.signaling === "sdp" ? { offer: result.sdp } : { livekit: result }), generation: this.state.generation, trackName: "screen", publisherName: publisher.name, audio: withAudio });
    } catch (error) {
      void this.cleanPending();
      throw error;
    }
  }

  override async alarm(): Promise<void> {
    await this.enqueue(async () => {
      try {
        const now = Date.now();
        const publisher = this.state.publisher;
        const owner = publisher?.uid ?? this.state.viewers[0]?.viewerId.split(".")[0] ?? "";
        if (publisher && now - publisher.lastHeartbeat > PUBLISHER_TIMEOUT) await this.retirePublisher(owner);
        for (const viewer of [...this.state.viewers]) {
          if (now - viewer.lastSeen > VIEWER_TIMEOUT) await this.retireViewer(owner, viewer);
        }
        // Renova o "ao vivo" no Hub (abaixo) e limpa o que ficou pendente depois da fila.
        // Renova o "ao vivo" no Hub; sem renovação ele expira sozinho (transmissão órfã).
        if (this.state.publisher) await this.syncHub(this.state.publisher.uid);
        this.ctx.storage.sql.exec("DELETE FROM rate_limits WHERE window_start <= ?", now - 10_000);
      } finally {
        if (this.state.publisher || this.state.viewers.length || this.state.pending.length) {
          await this.ctx.storage.setAlarm(Date.now() + ALARM_INTERVAL);
        }
      }
    });
    // Fora da fila: uma sessão lenta no SFU não trava as requisições da sala.
    await this.cleanPending();
    if (this.state.pending.length && await this.ctx.storage.getAlarm() === null) {
      await this.ctx.storage.setAlarm(Date.now() + ALARM_INTERVAL);
    }
  }
}
