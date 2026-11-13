import { ApiError, textField, isRecord } from "./common";

export type StreamDestination = { kind: "channel"; channelId: string } | { kind: "direct"; sessionId: string };
export function sameDestination(value: unknown, target: StreamDestination): boolean {
  return isRecord(value) && value.kind === target.kind && (target.kind === "direct" ? value.sessionId === target.sessionId : value.channelId === target.channelId);
}
export interface DirectSession {
  [column: string]: SqlStorageValue;
  id: string; a: string; b: string; active: number; absent_a: number | null; absent_b: number | null;
  declined_a: number; declined_b: number; cleanup: number;
}
const GRACE = 30_000;

/** Consent and audience live in the Hub; room credentials are never session identifiers. */
export class DirectDirectory {
  constructor(private readonly sql: SqlStorage, private readonly online: (uid: string) => boolean) {
    sql.exec(`CREATE TABLE IF NOT EXISTS direct_sessions (
      id TEXT PRIMARY KEY, a TEXT NOT NULL, b TEXT NOT NULL, active INTEGER NOT NULL,
      absent_a INTEGER, absent_b INTEGER, declined_a INTEGER NOT NULL DEFAULT 0,
      declined_b INTEGER NOT NULL DEFAULT 0, cleanup INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
      CREATE INDEX IF NOT EXISTS direct_by_pair ON direct_sessions(a,b,active);`);
  }

  private friends(a: string, b: string): boolean {
    return this.sql.exec("SELECT a FROM friends WHERE a = ? AND b = ? AND status = 'accepted'", a, b).toArray().length > 0;
  }
  expired(row: DirectSession): boolean {
    return [row.absent_a, row.absent_b].some(at => at !== null && Date.now() - at >= GRACE);
  }
  require(uid: string, input: unknown): DirectSession {
    const id = textField(input, 64);
    const row = this.sql.exec<DirectSession>("SELECT * FROM direct_sessions WHERE id = ?", id).toArray()[0];
    if (!row || !row.active || (row.a !== uid && row.b !== uid) || !this.friends(row.a, row.b) || this.expired(row)) {
      throw new ApiError(403, "sessao_privada_encerrada", "Esta sessão privada não está disponível. Inicie uma nova sessão com seu amigo.");
    }
    return row;
  }
  start(uid: string, peer: string): DirectSession {
    const [a, b] = uid < peer ? [uid, peer] : [peer, uid];
    if (uid === peer || !this.friends(a, b)) throw new ApiError(403, "amizade_obrigatoria", "Adicione esta pessoa aos amigos primeiro.");
    if (!this.online(uid) || !this.online(peer)) throw new ApiError(409, "amigo_offline", "Seu amigo está offline. Aguarde ele voltar para compartilhar.");
    const existing = this.sql.exec<DirectSession>("SELECT * FROM direct_sessions WHERE a = ? AND b = ? AND active = 1", a, b).toArray()[0];
    if (existing && !this.expired(existing)) return existing;
    if (existing) this.end(existing.id);
    const recent = this.sql.exec<{ created_at: number }>("SELECT created_at FROM direct_sessions WHERE a = ? AND b = ? ORDER BY created_at DESC LIMIT 1", a, b).toArray()[0];
    if (recent && Date.now() - recent.created_at < 30_000) throw new ApiError(429, "aviso_recente", "Aguarde 30 segundos antes de iniciar outra sessão com esta pessoa.");
    const id = `d_${crypto.randomUUID().replace(/-/g, "")}`;
    this.sql.exec("INSERT INTO direct_sessions (id,a,b,active,created_at) VALUES (?,?,?,1,?)", id, a, b, Date.now());
    return this.require(uid, id);
  }
  end(id: string): void { this.sql.exec("UPDATE direct_sessions SET active = 0, cleanup = 1 WHERE id = ?", id); }
  endFor(uid: string, peer?: string): void {
    for (const row of this.list(uid)) if (!peer || row.a === peer || row.b === peer) this.end(row.id);
  }
  decline(uid: string, id: string): void {
    const row = this.require(uid, id);
    this.sql.exec(`UPDATE direct_sessions SET ${uid === row.a ? "declined_a" : "declined_b"} = 1 WHERE id = ?`, id);
  }
  list(uid: string): DirectSession[] { return this.sql.exec<DirectSession>("SELECT * FROM direct_sessions WHERE active = 1 AND (a = ? OR b = ?)", uid, uid).toArray(); }
  presence(uid: string): void {
    for (const row of this.list(uid)) {
      if (this.expired(row)) { this.end(row.id); continue; }
      const column = uid === row.a ? "absent_a" : "absent_b";
      this.sql.exec(`UPDATE direct_sessions SET ${column} = ? WHERE id = ?`, this.online(uid) ? null : row[column] ?? Date.now(), row.id);
    }
  }
  expire(): void {
    for (const row of this.sql.exec<DirectSession>("SELECT * FROM direct_sessions WHERE active = 1").toArray()) {
      if (this.expired(row) || !this.friends(row.a, row.b)) this.end(row.id);
    }
  }
  pending(): DirectSession[] { return this.sql.exec<DirectSession>("SELECT * FROM direct_sessions WHERE cleanup = 1").toArray(); }
  cleaned(id: string): void { this.sql.exec("UPDATE direct_sessions SET cleanup = 0 WHERE id = ?", id); }
  needsAlarm(): boolean { return this.sql.exec("SELECT id FROM direct_sessions WHERE active = 1 OR cleanup = 1 LIMIT 1").toArray().length > 0; }
}
