import { DurableObject } from "cloudflare:workers";
import { randomCode } from "./auth";
import { ApiError, invalidBody, textField, type Env } from "./common";
import { safeAvatar } from "./oauth";
import { ServerDirectory, type InviteDestination, type DirectoryChange } from "./servers";
import { SfuControl, failoverEnabled } from "./sfu-control";
import { activeProvider, type ProviderId } from "./sfu";

/**
 * Diretório global (instância única "hub"): usuários, identidades OAuth, amizades, convites,
 * presença (WebSockets hibernáveis) e a lista "ao vivo agora".
 *
 * Persistência em SQLite do próprio Durable Object, e não em D1: o Hub precisa de leitura
 * consistente logo após escrita (aceitar amigo, convite de uso limitado), de transação
 * implícita por requisição e do mesmo ponto de serialização que já guarda os WebSockets de
 * presença. D1 exigiria um binding e migrações extras, sem ganho para um grupo de amigos
 * (centenas de usuários, não milhões). A mídia nunca passa por aqui.
 */

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
  servers: ReturnType<ServerDirectory["list"]>;
  destination?: InviteDestination;
}
export interface OAuthProfile {
  sub: string;
  username: string;
  displayName: string;
  avatar: string | null;
}

interface UserRow {
  [column: string]: SqlStorageValue;
  id: string;
  username: string;
  display: string;
  avatar: string | null;
  recovery_hash: string | null;
}

const USERNAME = /^[a-z0-9_.]{3,20}$/;
const RESERVED = new Set(["admin", "madscreen", "root", "system", "suporte", "support"]);
const INVITE_TTL = 7 * 86400;
const INVITE_USES = 5;
const MAX_ACTIVE_INVITES = 10;
/** Segundos sem renovação até uma transmissão deixar de aparecer como ao vivo. */
const LIVE_TTL = 90;

function now(): number {
  return Math.floor(Date.now() / 1000);
}

export function normalizeUsername(value: unknown): string {
  if (typeof value !== "string") invalidBody();
  const name = value.trim().replace(/^@/, "").toLowerCase();
  if (!USERNAME.test(name) || RESERVED.has(name)) {
    throw new ApiError(400, "usuario_invalido", "Use de 3 a 20 letras minúsculas, números, ponto ou sublinhado.");
  }
  return name;
}

function sanitizeUsername(value: string): string {
  const base = value.toLowerCase().normalize("NFD").replace(/[\u0300-\u036f]/g, "").replace(/[^a-z0-9_.]+/g, "_").replace(/^_+|_+$/g, "").slice(0, 16);
  return base.length >= 3 ? base : `${base}${randomCode(5).toLowerCase()}`.slice(0, 20);
}

async function hashSecret(value: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value.trim().toUpperCase().replace(/[^A-Z0-9]/g, "")));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function cleanCode(value: unknown): string {
  if (typeof value !== "string") invalidBody();
  const code = value.trim().toUpperCase().replace(/[^A-Z0-9]/g, "");
  if (code.length !== 8) throw new ApiError(400, "convite_invalido", "Código de convite inválido.");
  return `${code.slice(0, 4)}-${code.slice(4)}`;
}

export class HubDO extends DurableObject<Env> {
  private readonly sql: SqlStorage;
  private readonly directory: ServerDirectory;
  private readonly lastSnapshots = new WeakMap<WebSocket, string>();
  private sfuControl: SfuControl | null = null;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    this.sql.exec(`
      CREATE TABLE IF NOT EXISTS users (
        id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE COLLATE NOCASE, display TEXT NOT NULL,
        avatar TEXT, recovery_hash TEXT, created_at INTEGER NOT NULL, last_seen INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS identities (
        provider TEXT NOT NULL, sub TEXT NOT NULL, user_id TEXT NOT NULL, PRIMARY KEY (provider, sub));
      CREATE TABLE IF NOT EXISTS friends (
        a TEXT NOT NULL, b TEXT NOT NULL, status TEXT NOT NULL, requester TEXT NOT NULL, created_at INTEGER NOT NULL,
        PRIMARY KEY (a, b));
      CREATE TABLE IF NOT EXISTS invites (
        code TEXT PRIMARY KEY, user_id TEXT NOT NULL, expires INTEGER NOT NULL, max_uses INTEGER NOT NULL, uses INTEGER NOT NULL DEFAULT 0);
      CREATE TABLE IF NOT EXISTS oauth_state (
        state TEXT PRIMARY KEY, provider TEXT NOT NULL, challenge TEXT NOT NULL, invite TEXT, expires INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS handoff (
        challenge TEXT PRIMARY KEY, user_id TEXT NOT NULL, expires INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS revoked_sessions (
        digest TEXT PRIMARY KEY, expires INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS rate (
        key TEXT PRIMARY KEY, window_start INTEGER NOT NULL, count INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS live (
        user_id TEXT PRIMARY KEY, title TEXT, since INTEGER NOT NULL, viewers INTEGER NOT NULL);
    `);
    try {
      this.sql.exec("ALTER TABLE live ADD COLUMN updated INTEGER NOT NULL DEFAULT 0");
    } catch {
      // Coluna já existe.
    }
    this.directory = new ServerDirectory(ctx.storage);
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair("ping", "pong"));
    if (failoverEnabled(env)) void ctx.blockConcurrencyWhile(async () => {
      if (await ctx.storage.getAlarm() === null) await ctx.storage.setAlarm(Date.now() + 30_000);
    });
  }

  private mediaControl(): SfuControl {
    if (!failoverEnabled(this.env)) throw new ApiError(503, "sfu_desabilitado", "A política automática de mídia não está habilitada.");
    return this.sfuControl ??= new SfuControl(this.sql, this.env);
  }

  async selectSfu(): Promise<ProviderId> {
    return failoverEnabled(this.env) ? this.mediaControl().select() : activeProvider(this.env);
  }

  recordMediaUsage(owner: string, generation: number, provider: ProviderId | null, viewers: number): void {
    if (failoverEnabled(this.env)) this.mediaControl().noteUsage(owner, generation, provider, viewers);
  }

  recordSfuOutcome(provider: ProviderId, ok: boolean, latencyMs: number, status: number): void {
    if (failoverEnabled(this.env)) this.mediaControl().outcome(provider, ok, latencyMs, status, !ok);
  }

  async sfuAdmin(uid: string, change?: { mode?: unknown; refresh?: unknown }): Promise<Record<string, unknown>> {
    this.require(uid);
    if (!(this.env.SFU_ADMIN_USER_IDS ?? "").split(",").map(value => value.trim()).includes(uid)) {
      throw new ApiError(403, "sem_permissao", "Só o administrador pode consultar a política de mídia.");
    }
    const control = this.mediaControl();
    if (change) {
      if (change.mode === undefined && change.refresh !== true) invalidBody();
      if (change.mode !== undefined) control.setMode(change.mode);
      if (change.refresh === true) await control.check();
    }
    return control.status();
  }

  override async alarm(): Promise<void> {
    if (!failoverEnabled(this.env)) return;
    try { await this.mediaControl().check(); }
    finally { await this.ctx.storage.setAlarm(Date.now() + 30_000); }
  }

  // ---- utilidades ---------------------------------------------------------------------------

  assertSession(uid: string, digest: string): void {
    this.require(uid);
    if (this.sql.exec("SELECT digest FROM revoked_sessions WHERE digest = ? AND expires > ?", digest, Math.floor(Date.now() / 1000)).toArray().length) {
      throw new ApiError(401, "sessao_revogada", "Esta sessão foi encerrada. Entre novamente.");
    }
  }

  activeMediaOwners(uid: string): string[] {
    this.require(uid);
    return [...new Set([uid, ...this.sql.exec<{ user_id: string }>(
      "SELECT DISTINCT live.user_id FROM live JOIN channels ON channels.id = live.channel_id JOIN members ON members.server_id = channels.server_id WHERE members.user_id = ?", uid
    ).toArray().map(row => row.user_id)])];
  }

  revokeSession(uid: string, digest: string, exp: number): string[] {
    const owners = this.activeMediaOwners(uid);
    this.sql.exec("DELETE FROM revoked_sessions WHERE expires <= ?", Math.floor(Date.now() / 1000));
    this.sql.exec("INSERT OR REPLACE INTO revoked_sessions (digest, expires) VALUES (?, ?)", digest, Math.max(exp, Math.floor(Date.now() / 1000) + 90 * 86400));
    for (const socket of this.ctx.getWebSockets(`u:${uid}`)) {
      try { socket.close(4001, "sessao encerrada"); } catch { /* Already closed. */ }
    }
    return owners;
  }

  private find(id: string): UserRow | undefined {
    return this.sql.exec<UserRow>("SELECT id, username, display, avatar, recovery_hash FROM users WHERE id = ?", id).toArray()[0];
  }

  private require(id: string): UserRow {
    const user = this.find(id);
    if (!user) throw new ApiError(401, "sessao_invalida", "Conta não encontrada. Entre novamente.");
    return user;
  }

  private findByName(username: string): UserRow | undefined {
    return this.sql.exec<UserRow>("SELECT id, username, display, avatar, recovery_hash FROM users WHERE username = ?", username).toArray()[0];
  }

  private view(user: UserRow): UserView {
    return { id: user.id, username: user.username, displayName: user.display, avatar: safeAvatar(user.avatar) };
  }

  private pair(left: string, right: string): [string, string] {
    return left < right ? [left, right] : [right, left];
  }

  private friendshipStatus(left: string, right: string): { status: string; requester: string } | undefined {
    const [a, b] = this.pair(left, right);
    return this.sql.exec<{ status: string; requester: string }>("SELECT status, requester FROM friends WHERE a = ? AND b = ?", a, b).toArray()[0];
  }

  private online(uid: string): boolean {
    return this.ctx.getWebSockets(`u:${uid}`).some((socket) => socket.readyState === WebSocket.READY_STATE_OPEN);
  }

  private friendIds(uid: string): string[] {
    return this.sql.exec<{ other: string }>(
      "SELECT CASE WHEN a = ?1 THEN b ELSE a END AS other FROM friends WHERE status = 'accepted' AND (a = ?1 OR b = ?1)", uid,
    ).toArray().map((row) => row.other);
  }

  private snapshot(uid: string): Snapshot {
    const me = this.require(uid);
    const rows = this.sql.exec<{ other: string; status: string; requester: string }>(
      "SELECT CASE WHEN a = ?1 THEN b ELSE a END AS other, status, requester FROM friends WHERE a = ?1 OR b = ?1", uid,
    ).toArray();
    const friends: FriendView[] = [];
    const incoming: UserView[] = [];
    const outgoing: UserView[] = [];
    for (const row of rows) {
      const user = this.find(row.other);
      if (!user) continue;
      if (row.status === "accepted") {
        const live = this.sql.exec<{ title: string | null; since: number; viewers: number }>(
          "SELECT l.title,l.since,l.viewers FROM live l JOIN channels c ON c.id = l.channel_id JOIN members m ON m.server_id = c.server_id WHERE l.user_id = ? AND l.updated >= ? AND m.user_id = ?", user.id, now() - LIVE_TTL, uid,
        ).toArray()[0];
        friends.push({ ...this.view(user), online: this.online(user.id), live: live ? { since: live.since, viewers: live.viewers, title: live.title } : null });
      } else if (row.requester === uid) {
        outgoing.push(this.view(user));
      } else {
        incoming.push(this.view(user));
      }
    }
    friends.sort((x, y) => Number(y.live !== null) - Number(x.live !== null) || Number(y.online) - Number(x.online) || x.displayName.localeCompare(y.displayName));
    return { me: this.view(me), friends, incoming, outgoing, servers: this.directory.list(uid, id => this.online(id)) };
  }

  private push(uid: string): void {
    const sockets = this.ctx.getWebSockets(`u:${uid}`).filter((socket) => socket.readyState === WebSocket.READY_STATE_OPEN);
    if (!sockets.length) return;
    const message = JSON.stringify({ type: "snapshot", ...this.snapshot(uid) });
    for (const socket of sockets) {
      if (this.lastSnapshots.get(socket) === message) continue;
      try {
        socket.send(message);
        this.lastSnapshots.set(socket, message);
      } catch {
        // Conexão já encerrada: o evento de close limpa.
      }
    }
  }

  private pushWithFriends(uid: string): void {
    this.push(uid);
    for (const peer of new Set([...this.friendIds(uid), ...this.directory.peerIds(uid)])) this.push(peer);
  }

  private async rateLimit(key: string, limit: number, windowSeconds: number): Promise<void> {
    const at = now();
    this.sql.exec("DELETE FROM rate WHERE window_start <= ?", at - 3600);
    const row = this.sql.exec<{ window_start: number; count: number }>("SELECT window_start, count FROM rate WHERE key = ?", key).toArray()[0];
    if (row && at - row.window_start < windowSeconds) {
      if (row.count >= limit) throw new ApiError(429, "limite_requisicoes", "Muitas tentativas. Aguarde um pouco.");
      this.sql.exec("UPDATE rate SET count = count + 1 WHERE key = ?", key);
    } else {
      this.sql.exec("INSERT INTO rate (key, window_start, count) VALUES (?, ?, 1) ON CONFLICT(key) DO UPDATE SET window_start = excluded.window_start, count = 1", key, at);
    }
  }

  private createUser(username: string, display: string, avatar: string | null, recoveryHash: string | null): UserRow {
    const id = `u_${[...crypto.getRandomValues(new Uint8Array(8))].map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
    this.sql.exec("INSERT INTO users (id, username, display, avatar, recovery_hash, created_at, last_seen) VALUES (?, ?, ?, ?, ?, ?, ?)",
      id, username, display.slice(0, 40) || username, avatar, recoveryHash, now(), now());
    return { id, username, display, avatar, recovery_hash: recoveryHash };
  }

  private befriend(left: string, right: string): void {
    const [a, b] = this.pair(left, right);
    this.sql.exec(
      "INSERT INTO friends (a, b, status, requester, created_at) VALUES (?, ?, 'accepted', ?, ?) ON CONFLICT(a, b) DO UPDATE SET status = 'accepted'",
      a, b, left, now(),
    );
  }

  private readInvite(code: string) {
    const row = this.sql.exec<{ user_id: string; expires: number; max_uses: number; uses: number; channel_id: string | null }>(
      "SELECT user_id,expires,max_uses,uses,channel_id FROM invites WHERE code = ?", code,
    ).toArray()[0];
    if (!row || !this.find(row.user_id) || !row.channel_id) throw new ApiError(404, "convite_invalido", "Este convite não é válido.");
    if (row.expires <= now()) throw new ApiError(410, "convite_expirado", "Este convite expirou. Peça um novo convite.");
    const destination = this.directory.destination(row.channel_id);
    return { ...row, destination };
  }

  private takeInvite(code: string) {
    const row = this.readInvite(code);
    if (row.uses >= row.max_uses) throw new ApiError(410, "convite_esgotado", "Este convite atingiu o limite de usos. Peça um novo convite.");
    this.sql.exec("UPDATE invites SET uses = uses + 1 WHERE code = ?", code);
    return row;
  }

  /** Resgate idempotente: reabrir um convite válido não gasta uso de quem já é membro. */
  private acceptInvite(uid: string, code: string): { inviter: string; destination: InviteDestination } {
    return this.ctx.storage.transactionSync(() => {
      const row = this.readInvite(code);
      if (!this.directory.isMember(uid, row.destination.serverId)) {
        this.takeInvite(code);
        this.directory.join(uid, row.destination);
      }
      if (uid !== row.user_id) this.befriend(uid, row.user_id);
      return { inviter: row.user_id, destination: row.destination };
    });
  }

  // ---- entrada ------------------------------------------------------------------------------

  async isEmpty(): Promise<boolean> {
    return (this.sql.exec<{ n: number }>("SELECT COUNT(*) AS n FROM users").toArray()[0]?.n ?? 0) === 0;
  }

  /** Cadastro por nome de usuário + convite de amigo. O primeiro usuário do sistema entra sem convite. */
  async register(ip: string, usernameInput: unknown, codeInput: unknown, displayInput?: unknown): Promise<{ uid: string; recoveryKey: string }> {
    await this.rateLimit(`reg:${ip}`, 10, 600);
    const username = normalizeUsername(usernameInput);
    if (this.findByName(username)) throw new ApiError(409, "usuario_em_uso", "Este nome de usuário já existe.");
    const recoveryKey = [randomCode(5), randomCode(5), randomCode(5), randomCode(5)].join("-");
    const display = typeof displayInput === "string" && displayInput.trim() ? displayInput.trim().slice(0, 40) : username;
    const recoveryHash = await hashSecret(recoveryKey);
    const user = this.ctx.storage.transactionSync(() => {
      // O hash cede a execução: confira o nome novamente antes de consumir o convite.
      if (this.findByName(username)) throw new ApiError(409, "usuario_em_uso", "Este nome de usuário já existe.");
      const count = this.sql.exec<{ n: number }>("SELECT COUNT(*) AS n FROM users").toArray()[0]?.n ?? 0;
      let invitation: ReturnType<HubDO["takeInvite"]> | null = null;
      if (count > 0 || (typeof codeInput === "string" && codeInput.trim())) invitation = this.takeInvite(cleanCode(codeInput));
      const created = this.createUser(username, display, null, recoveryHash);
      if (invitation) {
        this.directory.join(created.id, invitation.destination);
        this.befriend(created.id, invitation.user_id);
      } else this.directory.defaultChannel(created.id);
      return created;
    });
    this.pushWithFriends(user.id);
    return { uid: user.id, recoveryKey };
  }

  /** Volta à conta com nome de usuário + chave de recuperação (mostrada uma vez no cadastro). */
  async login(ip: string, usernameInput: unknown, recoveryInput: unknown): Promise<{ uid: string }> {
    await this.rateLimit(`login:${ip}`, 10, 600);
    if (typeof recoveryInput !== "string") invalidBody();
    const username = normalizeUsername(usernameInput);
    const user = this.findByName(username);
    const hash = await hashSecret(recoveryInput);
    // Compara sempre algo para não revelar se o usuário existe pelo tempo de resposta.
    const expected = user?.recovery_hash ?? "0".repeat(64);
    let diff = 0;
    for (let index = 0; index < 64; index++) diff |= expected.charCodeAt(index) ^ hash.charCodeAt(index);
    if (!user || !user.recovery_hash || diff !== 0) throw new ApiError(401, "credenciais_invalidas", "Usuário ou chave de recuperação incorretos.");
    return { uid: user.id };
  }

  async oauthBegin(provider: string, challenge: string, invite: string | null): Promise<string> {
    this.sql.exec("DELETE FROM oauth_state WHERE expires <= ?", now());
    this.sql.exec("DELETE FROM handoff WHERE expires <= ?", now());
    const state = randomCode(32);
    this.sql.exec("INSERT INTO oauth_state (state, provider, challenge, invite, expires) VALUES (?, ?, ?, ?, ?)",
      state, provider, challenge, invite ? cleanCode(invite) : null, now() + 600);
    return state;
  }

  /** Conclui o OAuth: cria ou encontra o usuário e deixa uma credencial de uso único para o app buscar. */
  async oauthFinish(state: string, provider: string, profile: OAuthProfile): Promise<void> {
    const row = this.sql.exec<{ provider: string; challenge: string; invite: string | null; expires: number }>(
      "SELECT provider, challenge, invite, expires FROM oauth_state WHERE state = ?", state,
    ).toArray()[0];
    this.sql.exec("DELETE FROM oauth_state WHERE state = ?", state);
    if (!row || row.provider !== provider || row.expires <= now()) throw new ApiError(400, "estado_invalido", "O login expirou. Tente entrar novamente.");
    let identity = this.sql.exec<{ user_id: string }>("SELECT user_id FROM identities WHERE provider = ? AND sub = ?", provider, profile.sub).toArray()[0];
    let uid = identity?.user_id;
    if (!uid || !this.find(uid)) {
      let username = sanitizeUsername(profile.username);
      while (this.findByName(username) || RESERVED.has(username)) username = `${sanitizeUsername(profile.username).slice(0, 15)}${randomCode(4).toLowerCase()}`.slice(0, 20);
      const user = this.createUser(username, profile.displayName || profile.username, profile.avatar, null);
      uid = user.id;
      this.sql.exec("INSERT OR REPLACE INTO identities (provider, sub, user_id) VALUES (?, ?, ?)", provider, profile.sub, uid);
      identity = { user_id: uid };
    } else {
      this.sql.exec("UPDATE users SET avatar = COALESCE(?, avatar), last_seen = ? WHERE id = ?", profile.avatar, now(), uid);
    }
    if (row.invite) {
      try {
        this.acceptInvite(uid, row.invite);
      } catch {
        // Convite vencido no meio do login não deve impedir a entrada.
      }
    }
    if (!this.directory.peerIds(uid).length) this.directory.defaultChannel(uid);
    this.sql.exec("INSERT OR REPLACE INTO handoff (challenge, user_id, expires) VALUES (?, ?, ?)", row.challenge, uid, now() + 300);
    this.pushWithFriends(uid);
  }

  /** O app/navegador troca o verificador PKCE pelo usuário (uso único). `null` = ainda não concluído. */
  async exchange(challenge: string): Promise<string | null> {
    const row = this.sql.exec<{ user_id: string; expires: number }>("SELECT user_id, expires FROM handoff WHERE challenge = ?", challenge).toArray()[0];
    if (!row) return null;
    this.sql.exec("DELETE FROM handoff WHERE challenge = ?", challenge);
    return row.expires > now() ? row.user_id : null;
  }

  // ---- conta --------------------------------------------------------------------------------

  async me(uid: string): Promise<UserView> {
    return this.view(this.require(uid));
  }

  /** Conta criada com nome de usuário entra com chave de recuperação; conta de Discord/Google não. */
  async hasRecoveryKey(uid: string): Promise<boolean> {
    return this.require(uid).recovery_hash !== null;
  }

  /** Troca a chave de recuperação de quem está conectado. A chave antiga deixa de valer na hora. */
  async rotateRecoveryKey(uid: string): Promise<string> {
    const user = this.require(uid);
    if (!user.recovery_hash) throw new ApiError(409, "sem_chave", "Esta conta entra pelo Discord ou Google e não usa chave de recuperação.");
    await this.rateLimit(`recovery:${uid}`, 5, 3600);
    const recoveryKey = [randomCode(5), randomCode(5), randomCode(5), randomCode(5)].join("-");
    this.sql.exec("UPDATE users SET recovery_hash = ? WHERE id = ?", await hashSecret(recoveryKey), uid);
    return recoveryKey;
  }

  async updateMe(uid: string, displayInput: unknown): Promise<UserView> {
    this.require(uid);
    if (typeof displayInput !== "string" || !displayInput.trim() || displayInput.length > 40) invalidBody();
    this.sql.exec("UPDATE users SET display = ? WHERE id = ?", displayInput.trim(), uid);
    this.pushWithFriends(uid);
    return this.view(this.require(uid));
  }

  async deleteMe(uid: string): Promise<void> {
    this.require(uid);
    const friends = [...new Set([...this.friendIds(uid), ...this.directory.peerIds(uid)])];
    const memberships = this.sql.exec<{ server_id: string }>("SELECT server_id FROM members WHERE user_id = ?", uid).toArray();
    for (const membership of memberships) this.directory.leave(uid, membership.server_id);
    for (const table of ["identities", "invites", "live", "handoff"]) this.sql.exec(`DELETE FROM ${table} WHERE user_id = ?`, uid);
    this.sql.exec("DELETE FROM friends WHERE a = ? OR b = ?", uid, uid);
    this.sql.exec("DELETE FROM users WHERE id = ?", uid);
    for (const socket of this.ctx.getWebSockets(`u:${uid}`)) {
      try {
        socket.close(4001, "conta removida");
      } catch {
        // Já fechado.
      }
    }
    for (const friend of friends) this.push(friend);
  }

  // ---- amigos -------------------------------------------------------------------------------

  async friends(uid: string): Promise<Snapshot> {
    return this.snapshot(uid);
  }

  async requestFriend(uid: string, usernameInput: unknown): Promise<Snapshot> {
    this.require(uid);
    const target = this.findByName(normalizeUsername(usernameInput));
    if (!target) throw new ApiError(404, "usuario_nao_encontrado", "Nenhum usuário com esse nome.");
    if (target.id === uid) throw new ApiError(400, "usuario_invalido", "Você não pode adicionar a si mesmo.");
    const existing = this.friendshipStatus(uid, target.id);
    if (existing?.status === "accepted") throw new ApiError(409, "ja_amigos", "Vocês já são amigos.");
    if (existing && existing.requester === uid) throw new ApiError(409, "pedido_pendente", "Pedido já enviado.");
    if (existing) {
      this.befriend(uid, target.id);
    } else {
      const [a, b] = this.pair(uid, target.id);
      this.sql.exec("INSERT INTO friends (a, b, status, requester, created_at) VALUES (?, ?, 'pending', ?, ?)", a, b, uid, now());
    }
    this.push(uid);
    this.push(target.id);
    return this.snapshot(uid);
  }

  async answerRequest(uid: string, otherId: unknown, accept: boolean): Promise<Snapshot> {
    this.require(uid);
    if (typeof otherId !== "string") invalidBody();
    const existing = this.friendshipStatus(uid, otherId);
    if (!existing || existing.status !== "pending" || existing.requester === uid) {
      throw new ApiError(404, "pedido_nao_encontrado", "Pedido de amizade não encontrado.");
    }
    if (accept) {
      this.befriend(uid, otherId);
    } else {
      const [a, b] = this.pair(uid, otherId);
      this.sql.exec("DELETE FROM friends WHERE a = ? AND b = ?", a, b);
    }
    this.push(uid);
    this.push(otherId);
    return this.snapshot(uid);
  }

  /** Desfaz amizade ou cancela pedido enviado. */
  async removeFriend(uid: string, otherId: string): Promise<Snapshot> {
    this.require(uid);
    const [a, b] = this.pair(uid, otherId);
    this.sql.exec("DELETE FROM friends WHERE a = ? AND b = ?", a, b);
    this.push(uid);
    this.push(otherId);
    return this.snapshot(uid);
  }

  // ---- convites -----------------------------------------------------------------------------

  async createInvite(uid: string, serverInput?: unknown, channelInput?: unknown) {
    await this.rateLimit(`invite:${uid}`, 20, 600);
    this.require(uid);
    const destination = this.directory.inviteTarget(uid,
      serverInput === undefined ? undefined : textField(serverInput, 64),
      channelInput === undefined ? undefined : textField(channelInput, 64));
    this.sql.exec("DELETE FROM invites WHERE expires <= ?", now());
    const active = this.sql.exec<{ n: number }>("SELECT COUNT(*) AS n FROM invites WHERE user_id = ? AND uses < max_uses", uid).toArray()[0]?.n ?? 0;
    if (active >= MAX_ACTIVE_INVITES) throw new ApiError(429, "limite_convites", "Você já tem convites demais ativos. Aguarde algum expirar.");
    let code: string;
    do { const raw = randomCode(8); code = `${raw.slice(0, 4)}-${raw.slice(4)}`; }
    while (this.sql.exec("SELECT code FROM invites WHERE code = ?", code).toArray().length);
    const expiresAt = now() + INVITE_TTL;
    this.sql.exec("INSERT INTO invites (code,user_id,expires,max_uses,uses,server_id,channel_id) VALUES (?,?,?,?,0,?,?)", code, uid, expiresAt, INVITE_USES, destination.serverId, destination.channelId);
    return { code, expiresAt, maxUses: INVITE_USES, destination };
  }

  async previewInvite(ip: string, codeInput: unknown) {
    await this.rateLimit(`prev:${ip}`, 30, 600);
    const row = this.readInvite(cleanCode(codeInput));
    if (row.uses >= row.max_uses) throw new ApiError(410, "convite_esgotado", "Este convite atingiu o limite de usos. Peça um novo convite.");
    return { inviter: this.view(this.require(row.user_id)), expiresAt: row.expires,
      server: { id: row.destination.serverId, name: row.destination.serverName },
      channel: { id: row.destination.channelId, name: row.destination.channelName } };
  }

  async redeemInvite(uid: string, ip: string, codeInput: unknown): Promise<Snapshot> {
    await this.rateLimit(`redeem:${ip}`, 10, 600);
    this.require(uid);
    const joined = this.acceptInvite(uid, cleanCode(codeInput));
    this.pushWithFriends(uid);
    return { ...this.snapshot(uid), destination: joined.destination };
  }

  // ---- servidores e canais ------------------------------------------------------------------

  async directorySnapshot(uid: string, serverId?: string): Promise<Snapshot> {
    this.require(uid);
    if (serverId) this.directory.role(uid, serverId);
    return this.snapshot(uid);
  }

  async changeDirectory(uid: string, action: string, target: string, value?: unknown, memberId?: string) {
    await this.rateLimit(`directory:${uid}`, 30, 60);
    this.require(uid);
    let cleanup: DirectoryChange | null = null;
    let destination: InviteDestination | undefined;
    let notify: string[] = [];
    switch (action) {
      case "create": destination = this.directory.create(uid, value); break;
      case "rename": this.directory.rename(uid, target, value); notify = this.directory.memberIds(target); break;
      case "remove": cleanup = this.directory.remove(uid, target); break;
      case "channel_create": this.directory.createChannel(uid, target, value); notify = this.directory.memberIds(target); break;
      case "channel_rename": notify = this.directory.memberIds(this.directory.renameChannel(uid, target, value)); break;
      case "channel_remove": cleanup = this.directory.removeChannel(uid, target); break;
      case "leave": cleanup = this.directory.leave(uid, target); break;
      case "kick": cleanup = this.directory.removeMember(uid, target, textField(memberId, 64)); break;
      case "role": this.directory.setRole(uid, target, textField(memberId, 64), value); notify = this.directory.memberIds(target); break;
      default: invalidBody();
    }
    for (const peer of new Set([uid, ...notify, ...(cleanup?.notify ?? [])])) this.push(peer);
    return { snapshot: { ...this.snapshot(uid), ...(destination ? { destination } : {}) }, cleanup };
  }

  async resolveShare(uid: string, channelInput?: unknown): Promise<InviteDestination & { displayName: string }> {
    const user = this.require(uid);
    const destination = channelInput === undefined || channelInput === null ? this.directory.defaultChannel(uid)
      : this.directory.destination(textField(channelInput, 64), uid);
    return { ...destination, displayName: user.display };
  }

  // ---- ao vivo ------------------------------------------------------------------------------

  /** Chamado pelo StreamDO. `null` encerra. */
  async setLive(uid: string, info: { since: number; viewers: number; title: string | null; channelId?: string } | null): Promise<boolean> {
    if (!this.find(uid)) return false;
    const notify = this.directory.peerIds(uid);
    if (info) {
      let destination: InviteDestination;
      try {
        const previous = this.sql.exec<{ channel_id: string | null }>("SELECT channel_id FROM live WHERE user_id = ?", uid).toArray()[0];
        destination = info.channelId ? this.directory.destination(info.channelId, uid)
          : previous?.channel_id ? this.directory.destination(previous.channel_id, uid) : this.directory.defaultChannel(uid);
      } catch { return false; }
      this.sql.exec("INSERT INTO live (user_id,title,since,viewers,updated,channel_id) VALUES (?,?,?,?,?,?) ON CONFLICT(user_id) DO UPDATE SET title=excluded.title,since=excluded.since,viewers=excluded.viewers,updated=excluded.updated,channel_id=excluded.channel_id",
        uid, info.title, info.since, info.viewers, now(), destination.channelId);
    } else this.sql.exec("DELETE FROM live WHERE user_id = ?", uid);
    for (const peer of new Set([...this.friendIds(uid), ...notify, uid])) this.push(peer);
    return true;
  }

  /** Acesso à live exige participação no servidor do canal atual. */
  async canWatch(viewerId: string, ownerRef: string, channelId?: string): Promise<{ ownerId: string; ownerName: string; viewerName: string }> {
    const viewer = this.require(viewerId);
    const owner = /^u_[0-9a-f]{16}$/.test(ownerRef) ? this.find(ownerRef) : this.findByName(normalizeUsername(ownerRef));
    if (!owner) throw new ApiError(404, "usuario_nao_encontrado", "Nenhum usuário com esse nome.");
    if (owner.id !== viewer.id) {
      // O StreamDO passa o canal da geração atual para evitar uma troca de canal entre
      // a autorização HTTP e a assinatura. Uma linha antiga não amplia permissões.
      const live = this.sql.exec<{ channel_id: string | null }>("SELECT channel_id FROM live WHERE user_id = ?", owner.id).toArray()[0];
      const currentChannel = channelId ?? live?.channel_id;
      if (currentChannel) this.directory.destination(currentChannel, viewerId);
      else if (!this.sql.exec("SELECT mine.server_id FROM members mine JOIN members other ON other.server_id = mine.server_id WHERE mine.user_id = ? AND other.user_id = ? LIMIT 1", viewerId, owner.id).toArray().length) {
        throw new ApiError(403, "sem_acesso_servidor", "Você precisa participar do servidor para assistir.");
      }
    }
    return { ownerId: owner.id, ownerName: owner.display, viewerName: viewer.display };
  }

  // ---- WebSocket de presença ------------------------------------------------------------------

  override async fetch(request: Request): Promise<Response> {
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") throw new ApiError(400, "corpo_invalido", "Esperado WebSocket.");
    const uid = request.headers.get("x-ms-uid") ?? "";
    this.require(uid);
    const pair = new WebSocketPair();
    const first = !this.online(uid);
    this.ctx.acceptWebSocket(pair[1], [`u:${uid}`]);
    this.sql.exec("UPDATE users SET last_seen = ? WHERE id = ?", now(), uid);
    const message = JSON.stringify({ type: "snapshot", ...this.snapshot(uid) });
    pair[1].send(message);
    this.lastSnapshots.set(pair[1], message);
    if (first) for (const peer of new Set([...this.friendIds(uid), ...this.directory.peerIds(uid)])) this.push(peer);
    const headers = new Headers();
    const protocol = request.headers.get("x-ms-protocol");
    if (protocol) headers.set("Sec-WebSocket-Protocol", protocol);
    return new Response(null, { status: 101, webSocket: pair[0], headers });
  }

  override webSocketMessage(): void {
    // O cliente só envia "ping" (resposta automática); mensagens extras são ignoradas.
  }

  override webSocketClose(socket: WebSocket): void {
    this.afterClose(socket);
  }

  override webSocketError(socket: WebSocket): void {
    this.afterClose(socket);
  }

  private afterClose(socket: WebSocket): void {
    const tag = this.ctx.getTags(socket).find((item) => item.startsWith("u:"));
    try {
      socket.close(1000, "bye");
    } catch {
      // Já fechado.
    }
    if (!tag) return;
    const uid = tag.slice(2);
    if (this.online(uid)) return;
    this.sql.exec("UPDATE users SET last_seen = ? WHERE id = ?", now(), uid);
    for (const peer of new Set([...this.friendIds(uid), ...this.directory.peerIds(uid)])) this.push(peer);
  }
}
