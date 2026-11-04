import { ApiError, textField } from "./common";
import { safeAvatar } from "./oauth";

export type MemberRole = "owner" | "admin" | "member";
export interface InviteDestination { serverId: string; serverName: string; channelId: string; channelName: string }
export interface DirectoryChange { stopOwners: string[]; revokeOwners: string[]; notify: string[] }

const MAX_SERVERS = 32;
const MAX_CHANNELS = 32;
const MAX_MEMBERS = 128;
const id = (prefix: string): string => `${prefix}_${crypto.randomUUID().replace(/-/g, "")}`;
const nameOf = (value: unknown): string => {
  const name = textField(value, 40).trim();
  if (/[\u0000-\u001f\u007f]/.test(name)) throw new ApiError(400, "nome_invalido", "Use um nome sem quebras de linha.");
  return name;
};

/** Diretório SQLite; nenhuma chamada de rede ou espera dentro das transações. */
export class ServerDirectory {
  private readonly sql: SqlStorage;
  constructor(private readonly storage: DurableObjectStorage) {
    this.sql = storage.sql;
    storage.transactionSync(() => {
      this.sql.exec(`
        CREATE TABLE IF NOT EXISTS schema_meta (key TEXT PRIMARY KEY);
        CREATE TABLE IF NOT EXISTS servers (id TEXT PRIMARY KEY, name TEXT NOT NULL, owner_id TEXT NOT NULL, created_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS channels (id TEXT PRIMARY KEY, server_id TEXT NOT NULL, name TEXT NOT NULL COLLATE NOCASE, created_at INTEGER NOT NULL, UNIQUE(server_id, name));
        CREATE TABLE IF NOT EXISTS members (server_id TEXT NOT NULL, user_id TEXT NOT NULL, role TEXT NOT NULL CHECK(role IN ('owner','admin','member')), joined_at INTEGER NOT NULL, PRIMARY KEY(server_id,user_id));
        CREATE INDEX IF NOT EXISTS members_by_user ON members(user_id,server_id);
        CREATE INDEX IF NOT EXISTS channels_by_server ON channels(server_id);
      `);
      this.column("users", "default_server_id", "TEXT");
      this.column("invites", "server_id", "TEXT");
      this.column("invites", "channel_id", "TEXT");
      this.column("live", "channel_id", "TEXT");
      if (!this.sql.exec("SELECT key FROM schema_meta WHERE key = 'servers-v1'").toArray().length) {
        // Um grupo por dono preserva os círculos de amizade, sem juntar todas as contas do hub.
        const users = this.sql.exec<{ id: string }>("SELECT id FROM users ORDER BY created_at,id").toArray();
        for (const user of users) {
          const destination = this.defaultChannel(user.id, true);
          this.sql.exec("UPDATE invites SET server_id = ?, channel_id = ? WHERE user_id = ?", destination.serverId, destination.channelId, user.id);
          this.sql.exec("UPDATE live SET channel_id = ? WHERE user_id = ?", destination.channelId, user.id);
        }
        this.sql.exec("INSERT INTO schema_meta (key) VALUES ('servers-v1')");
      }
    });
  }

  private column(table: string, column: string, type: string): void {
    if (!this.sql.exec<{ name: string }>(`PRAGMA table_info(${table})`).toArray().some(row => row.name === column)) {
      // Identificadores vêm somente das constantes acima, nunca de uma requisição.
      this.sql.exec(`ALTER TABLE ${table} ADD COLUMN ${column} ${type}`);
    }
  }

  memberIds(serverId: string): string[] {
    return this.sql.exec<{ user_id: string }>("SELECT user_id FROM members WHERE server_id = ?", serverId).toArray().map(row => row.user_id);
  }

  peerIds(uid: string): string[] {
    return this.sql.exec<{ user_id: string }>("SELECT DISTINCT other.user_id FROM members mine JOIN members other ON other.server_id = mine.server_id WHERE mine.user_id = ?", uid).toArray().map(row => row.user_id);
  }

  role(uid: string, serverId: string): MemberRole {
    const member = this.sql.exec<{ role: MemberRole }>("SELECT role FROM members WHERE server_id = ? AND user_id = ?", serverId, uid).toArray()[0];
    if (!member) throw new ApiError(403, "sem_acesso_servidor", "Você não faz parte deste servidor.");
    return member.role;
  }

  private manage(uid: string, serverId: string, ownerOnly = false): void {
    const role = this.role(uid, serverId);
    if (role !== "owner" && (ownerOnly || role !== "admin")) throw new ApiError(403, "sem_permissao", "Você não tem permissão para gerenciar este servidor.");
  }

  private capacity(uid: string): void {
    const count = this.sql.exec<{ n: number }>("SELECT COUNT(*) AS n FROM members WHERE user_id = ?", uid).toArray()[0]?.n ?? 0;
    if (count >= MAX_SERVERS) throw new ApiError(429, "limite_servidores", "Você atingiu o limite de 32 servidores.");
  }

  private insertServer(uid: string, name: string): InviteDestination {
    const serverId = id("s");
    const channelId = id("c");
    const at = Math.floor(Date.now() / 1000);
    this.sql.exec("INSERT INTO servers VALUES (?, ?, ?, ?)", serverId, name, uid, at);
    this.sql.exec("INSERT INTO members VALUES (?, ?, 'owner', ?)", serverId, uid, at);
    this.sql.exec("INSERT INTO channels VALUES (?, ?, 'Geral', ?)", channelId, serverId, at);
    return { serverId, serverName: name, channelId, channelName: "Geral" };
  }

  defaultChannel(uid: string, migrateFriends = false): InviteDestination {
    const user = this.sql.exec<{ display: string; default_server_id: string | null }>("SELECT display,default_server_id FROM users WHERE id = ?", uid).toArray()[0];
    if (!user) throw new ApiError(401, "sessao_invalida", "Conta não encontrada. Entre novamente.");
    if (user.default_server_id) {
      const channel = this.sql.exec<{ id: string }>("SELECT id FROM channels WHERE server_id = ? ORDER BY created_at,id LIMIT 1", user.default_server_id).toArray()[0];
      if (channel) {
        try { return this.destination(channel.id, uid); } catch { /* O dono pode ter saído do grupo. */ }
      }
    }
    if (!migrateFriends) this.capacity(uid);
    const destination = this.insertServer(uid, `${user.display.slice(0, 30)} e amigos`);
    this.sql.exec("UPDATE users SET default_server_id = ? WHERE id = ?", destination.serverId, uid);
    if (migrateFriends) {
      const friends = this.sql.exec<{ other: string }>("SELECT CASE WHEN a = ?1 THEN b ELSE a END AS other FROM friends WHERE status = 'accepted' AND (a = ?1 OR b = ?1)", uid).toArray();
      for (const friend of friends) this.sql.exec("INSERT OR IGNORE INTO members VALUES (?, ?, 'member', ?)", destination.serverId, friend.other, Math.floor(Date.now() / 1000));
    }
    return destination;
  }

  destination(channelId: string, uid?: string): InviteDestination {
    const row = this.sql.exec<{ serverId: string; serverName: string; channelId: string; channelName: string }>(
      "SELECT s.id AS serverId,s.name AS serverName,c.id AS channelId,c.name AS channelName FROM channels c JOIN servers s ON s.id = c.server_id WHERE c.id = ?", channelId,
    ).toArray()[0];
    if (!row) throw new ApiError(404, "canal_nao_encontrado", "Este canal não está mais disponível.");
    if (uid) this.role(uid, row.serverId);
    return { serverId: row.serverId, serverName: row.serverName, channelId: row.channelId, channelName: row.channelName };
  }

  inviteTarget(uid: string, serverId?: string, channelId?: string): InviteDestination {
    if (!serverId) return this.defaultChannel(uid);
    this.manage(uid, serverId);
    const first = channelId ?? this.sql.exec<{ id: string }>("SELECT id FROM channels WHERE server_id = ? ORDER BY created_at,id LIMIT 1", serverId).toArray()[0]?.id;
    if (!first) throw new ApiError(404, "canal_nao_encontrado", "Este servidor não tem canais disponíveis.");
    const destination = this.destination(first, uid);
    if (destination.serverId !== serverId) throw new ApiError(400, "canal_invalido", "Escolha um canal deste servidor.");
    return destination;
  }

  create(uid: string, name: unknown): InviteDestination {
    return this.storage.transactionSync(() => { this.capacity(uid); return this.insertServer(uid, nameOf(name)); });
  }

  rename(uid: string, serverId: string, name: unknown): void {
    this.manage(uid, serverId, true);
    this.sql.exec("UPDATE servers SET name = ? WHERE id = ?", nameOf(name), serverId);
  }

  createChannel(uid: string, serverId: string, input: unknown): string {
    this.manage(uid, serverId);
    const name = nameOf(input);
    const count = this.sql.exec<{ n: number }>("SELECT COUNT(*) AS n FROM channels WHERE server_id = ?", serverId).toArray()[0]?.n ?? 0;
    if (count >= MAX_CHANNELS) throw new ApiError(429, "limite_canais", "O servidor atingiu o limite de 32 canais.");
    this.uniqueChannel(serverId, name);
    const channelId = id("c");
    this.sql.exec("INSERT INTO channels VALUES (?, ?, ?, ?)", channelId, serverId, name, Math.floor(Date.now() / 1000));
    return channelId;
  }

  private uniqueChannel(serverId: string, name: string, except = ""): void {
    if (this.sql.exec("SELECT id FROM channels WHERE server_id = ? AND name = ? COLLATE NOCASE AND id != ?", serverId, name, except).toArray().length) {
      throw new ApiError(409, "canal_existente", "Já existe um canal com esse nome.");
    }
  }

  renameChannel(uid: string, channelId: string, input: unknown): string {
    const destination = this.destination(channelId, uid);
    this.manage(uid, destination.serverId);
    const name = nameOf(input);
    this.uniqueChannel(destination.serverId, name, channelId);
    this.sql.exec("UPDATE channels SET name = ? WHERE id = ?", name, channelId);
    return destination.serverId;
  }

  removeChannel(uid: string, channelId: string): DirectoryChange {
    const destination = this.destination(channelId, uid);
    this.manage(uid, destination.serverId);
    const count = this.sql.exec<{ n: number }>("SELECT COUNT(*) AS n FROM channels WHERE server_id = ?", destination.serverId).toArray()[0]?.n ?? 0;
    if (count <= 1) throw new ApiError(409, "ultimo_canal", "Mantenha pelo menos um canal no servidor.");
    const stopOwners = this.sql.exec<{ user_id: string }>("SELECT user_id FROM live WHERE channel_id = ?", channelId).toArray().map(row => row.user_id);
    this.storage.transactionSync(() => {
      this.sql.exec("DELETE FROM invites WHERE channel_id = ?", channelId);
      this.sql.exec("DELETE FROM live WHERE channel_id = ?", channelId);
      this.sql.exec("DELETE FROM channels WHERE id = ?", channelId);
    });
    return { stopOwners, revokeOwners: [], notify: this.memberIds(destination.serverId) };
  }

  join(uid: string, destination: InviteDestination): void {
    if (this.sql.exec("SELECT user_id FROM members WHERE server_id = ? AND user_id = ?", destination.serverId, uid).toArray().length) return;
    this.capacity(uid);
    const count = this.memberIds(destination.serverId).length;
    if (count >= MAX_MEMBERS) throw new ApiError(429, "limite_membros", "Este servidor atingiu o limite de membros.");
    this.sql.exec("INSERT INTO members VALUES (?, ?, 'member', ?)", destination.serverId, uid, Math.floor(Date.now() / 1000));
  }

  isMember(uid: string, serverId: string): boolean {
    return this.sql.exec("SELECT user_id FROM members WHERE user_id = ? AND server_id = ?", uid, serverId).toArray().length > 0;
  }

  setRole(uid: string, serverId: string, memberId: string, role: unknown): void {
    this.manage(uid, serverId, true);
    if (role !== "admin" && role !== "member") throw new ApiError(400, "papel_invalido", "Escolha administrador ou membro.");
    if (this.role(memberId, serverId) === "owner") throw new ApiError(409, "dono_servidor", "O papel do dono não pode ser alterado por esta operação.");
    this.sql.exec("UPDATE members SET role = ? WHERE server_id = ? AND user_id = ?", role, serverId, memberId);
  }

  removeMember(actor: string, serverId: string, member: string): DirectoryChange {
    this.manage(actor, serverId);
    const targetRole = this.role(member, serverId);
    if (actor === member || targetRole === "owner" || (targetRole === "admin" && this.role(actor, serverId) !== "owner")) {
      throw new ApiError(403, "sem_permissao", "Você não pode remover este membro.");
    }
    return this.leave(member, serverId);
  }

  leave(uid: string, serverId: string): DirectoryChange {
    const role = this.role(uid, serverId);
    const notify = this.memberIds(serverId);
    const owners = this.sql.exec<{ user_id: string }>("SELECT user_id FROM live WHERE channel_id IN (SELECT id FROM channels WHERE server_id = ?)", serverId).toArray().map(row => row.user_id);
    this.storage.transactionSync(() => {
      if (role === "owner") {
        const next = this.sql.exec<{ user_id: string }>("SELECT user_id FROM members WHERE server_id = ? AND user_id != ? ORDER BY CASE role WHEN 'admin' THEN 0 ELSE 1 END,joined_at,user_id LIMIT 1", serverId, uid).toArray()[0];
        if (next) {
          this.sql.exec("UPDATE servers SET owner_id = ? WHERE id = ?", next.user_id, serverId);
          this.sql.exec("UPDATE members SET role = 'owner' WHERE server_id = ? AND user_id = ?", serverId, next.user_id);
        } else this.deleteRows(serverId);
      }
      this.sql.exec("DELETE FROM members WHERE server_id = ? AND user_id = ?", serverId, uid);
      this.sql.exec("UPDATE users SET default_server_id = NULL WHERE id = ? AND default_server_id = ?", uid, serverId);
      this.sql.exec("DELETE FROM live WHERE user_id = ? AND channel_id IN (SELECT id FROM channels WHERE server_id = ?)", uid, serverId);
      this.sql.exec("DELETE FROM invites WHERE server_id = ? AND user_id = ?", serverId, uid);
    });
    return { stopOwners: owners.includes(uid) ? [uid] : [], revokeOwners: owners.filter(owner => owner !== uid), notify };
  }

  remove(uid: string, serverId: string): DirectoryChange {
    this.manage(uid, serverId, true);
    const notify = this.memberIds(serverId);
    const stopOwners = this.sql.exec<{ user_id: string }>("SELECT user_id FROM live WHERE channel_id IN (SELECT id FROM channels WHERE server_id = ?)", serverId).toArray().map(row => row.user_id);
    this.storage.transactionSync(() => this.deleteRows(serverId));
    return { stopOwners, revokeOwners: [], notify };
  }

  private deleteRows(serverId: string): void {
    this.sql.exec("DELETE FROM live WHERE channel_id IN (SELECT id FROM channels WHERE server_id = ?)", serverId);
    this.sql.exec("DELETE FROM invites WHERE server_id = ?", serverId);
    this.sql.exec("DELETE FROM members WHERE server_id = ?", serverId);
    this.sql.exec("DELETE FROM channels WHERE server_id = ?", serverId);
    this.sql.exec("DELETE FROM servers WHERE id = ?", serverId);
    this.sql.exec("UPDATE users SET default_server_id = NULL WHERE default_server_id = ?", serverId);
  }

  list(uid: string, online: (id: string) => boolean) {
    const servers = this.sql.exec<{ id: string; name: string; owner_id: string; role: MemberRole }>("SELECT s.id,s.name,s.owner_id,m.role FROM servers s JOIN members m ON m.server_id = s.id WHERE m.user_id = ? ORDER BY s.created_at,s.id", uid).toArray();
    return servers.map(server => ({
      id: server.id, name: server.name, ownerId: server.owner_id, role: server.role,
      members: this.sql.exec<{ id: string; username: string; display: string; avatar: string | null; role: MemberRole }>("SELECT u.id,u.username,u.display,u.avatar,m.role FROM members m JOIN users u ON u.id = m.user_id WHERE m.server_id = ? ORDER BY u.display,u.id", server.id).toArray().map(user => ({ id: user.id, username: user.username, displayName: user.display, avatar: safeAvatar(user.avatar), role: user.role, online: online(user.id) })),
      channels: this.sql.exec<{ id: string; name: string }>("SELECT id,name FROM channels WHERE server_id = ? ORDER BY created_at,id", server.id).toArray().map(channel => ({
        id: channel.id, name: channel.name,
        live: this.sql.exec<{ id: string; username: string; display: string; avatar: string | null; title: string | null; since: number; viewers: number }>("SELECT u.id,u.username,u.display,u.avatar,l.title,l.since,l.viewers FROM live l JOIN users u ON u.id = l.user_id WHERE l.channel_id = ? AND l.updated >= ? ORDER BY l.since,u.id", channel.id, Math.floor(Date.now() / 1000) - 90).toArray().map(user => ({ id: user.id, username: user.username, displayName: user.display, avatar: safeAvatar(user.avatar), online: online(user.id), live: { title: user.title, since: user.since, viewers: user.viewers } })),
      })),
    }));
  }
}
