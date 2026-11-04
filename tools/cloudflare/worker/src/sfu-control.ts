import { ApiError, type Env } from "./common";
import { createProvider, type ProviderId, SfuError } from "./sfu";

const INTERVAL = 30_000;
const LEASE_MS = 40_000;
const IDS: ProviderId[] = ["cloudflare", "livekit"];
type Mode = "auto" | ProviderId;
type HealthStatus = "unknown" | "healthy" | "degraded" | "down" | "unconfigured";
interface Health {
  status: HealthStatus;
  checkedAt: number;
  latencyMs: number | null;
  failures: number;
  successes: number;
  samples: boolean[];
  lastError: { at: number; status: number } | null;
}
interface Lease { generation: number; provider: ProviderId | null; viewers: number; at: number; expires: number }
interface Transition { at: number; from: ProviderId; to: ProviderId; reason: string }
interface ControlState {
  mode: Mode;
  selected: ProviderId;
  changedAt: number;
  checkedAt: number;
  health: Record<ProviderId, Health>;
  usage: Record<string, Record<ProviderId, number>>;
  leases: Record<string, Lease>;
  transitions: Transition[];
}

export function failoverEnabled(env: Env): boolean { return env.SFU_FAILOVER_ENABLED === "1"; }

function numberOption(value: string | undefined, fallback: number, min: number, max: number): number {
  if (value === undefined) return fallback;
  const parsed = Number(value);
  if (!value.trim() || !Number.isFinite(parsed) || parsed < min || parsed > max) {
    throw new ApiError(503, "sfu_configuracao", "A política de mídia não está configurada.");
  }
  return parsed;
}

/** Ciclo em UTC, dias 1–28: não há ambiguidade em meses curtos. */
export function usagePeriod(at: number, day: number): { key: string; end: number } {
  const date = new Date(at);
  const month = date.getUTCMonth() - (date.getUTCDate() < day ? 1 : 0);
  return { key: new Date(Date.UTC(date.getUTCFullYear(), month, day)).toISOString().slice(0, 10), end: Date.UTC(date.getUTCFullYear(), month + 1, day) };
}

function emptyHealth(): Health {
  return { status: "unknown", checkedAt: 0, latencyMs: null, failures: 0, successes: 0, samples: [], lastError: null };
}

/** Apenas plano de controle e estimativas; não recebe pacotes nem contadores enviados por viewers. */
export class SfuControl {
  private readonly state: ControlState;
  private checking: Promise<void> | null = null;
  private readonly quota: number;
  private readonly threshold: number;
  private readonly bitrate: number;
  private readonly cycleDay: number;

  constructor(private readonly sql: SqlStorage, private readonly env: Env,
    private readonly probe: (id: ProviderId) => Promise<void> = id => createProvider(env, id).healthCheck()) {
    this.quota = numberOption(env.SFU_MONTHLY_QUOTA_BYTES, 1_000_000_000_000, 1, Number.MAX_SAFE_INTEGER);
    this.threshold = numberOption(env.SFU_FAILOVER_THRESHOLD, 0.9, 0.01, 1);
    this.bitrate = numberOption(env.SFU_USAGE_BITRATE_BPS, 30_128_000, 1000, 1_000_000_000);
    this.cycleDay = numberOption(env.SFU_CYCLE_DAY, 1, 1, 28);
    if (!Number.isInteger(this.cycleDay)) throw new ApiError(503, "sfu_configuracao", "O dia do ciclo precisa ser inteiro.");
    sql.exec("CREATE TABLE IF NOT EXISTS sfu_control (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL)");
    const row = sql.exec<{ data: string }>("SELECT data FROM sfu_control WHERE id=1").toArray()[0];
    this.state = row ? JSON.parse(row.data) as ControlState : {
      mode: "auto", selected: "cloudflare", changedAt: 0, checkedAt: 0,
      health: { cloudflare: emptyHealth(), livekit: emptyHealth() }, usage: {}, leases: {}, transitions: [],
    };
  }

  private save(): void {
    this.sql.exec("INSERT INTO sfu_control(id,data) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data", JSON.stringify(this.state));
  }

  private configured(id: ProviderId): boolean {
    return id === "cloudflare" ? Boolean(this.env.CF_REALTIME_APP_ID && this.env.CF_REALTIME_APP_SECRET)
      : Boolean(this.env.LIVEKIT_URL && this.env.LIVEKIT_API_KEY && this.env.LIVEKIT_API_SECRET);
  }

  private healthy(id: ProviderId, at: number): boolean {
    const health = this.state.health[id];
    return this.configured(id) && health.status === "healthy" && at >= health.checkedAt && at - health.checkedAt <= INTERVAL * 3;
  }

  private quotaReached(at: number): boolean {
    return this.env.SFU_USAGE_CALIBRATED === "1"
      && (this.state.usage[usagePeriod(at, this.cycleDay).key]?.cloudflare ?? 0) >= this.quota * this.threshold;
  }

  private flushUsage(at: number): void {
    for (const lease of Object.values(this.state.leases)) {
      const end = Math.min(at, lease.expires);
      if (lease.provider && lease.viewers > 0) {
        for (let start = lease.at; start < end;) {
          const period = usagePeriod(start, this.cycleDay);
          const until = Math.min(end, period.end);
          const usage = this.state.usage[period.key] ??= { cloudflare: 0, livekit: 0 };
          usage[lease.provider] += (until - start) * this.bitrate * lease.viewers / 8000;
          start = until;
        }
      }
      lease.at = Math.max(lease.at, end);
    }
    const keys = Object.keys(this.state.usage).sort();
    for (const key of keys.slice(0, -13)) delete this.state.usage[key];
    // Mantém tombstones de gerações por um dia para rejeitar renovações atrasadas.
    for (const [owner, lease] of Object.entries(this.state.leases)) if (at - lease.expires > 86400_000) delete this.state.leases[owner];
  }

  noteUsage(owner: string, generation: number, provider: ProviderId | null, viewers: number, at = Date.now()): void {
    if (!Number.isSafeInteger(generation) || generation < 0 || !Number.isInteger(viewers) || viewers < 0 || viewers > 8 ||
      (provider !== null && !IDS.includes(provider))) throw new ApiError(400, "corpo_invalido", "Estimativa de mídia inválida.");
    this.flushUsage(at);
    const previous = this.state.leases[owner];
    if (!previous || generation >= previous.generation) {
      this.state.leases[owner] = { generation, provider, viewers, at, expires: at + LEASE_MS };
    }
    this.decide(at);
    this.save();
  }

  /** Não persiste mensagens de upstream, URLs, nomes, SDP ou credenciais. */
  outcome(id: ProviderId, ok: boolean, latencyMs: number, status = 0, urgent = false, at = Date.now()): void {
    const health = this.state.health[id];
    health.checkedAt = at;
    health.latencyMs = Math.max(0, Math.round(latencyMs));
    health.samples.push(ok);
    health.samples = health.samples.slice(-20);
    if (ok) {
      health.successes++;
      health.failures = 0;
      health.status = health.status === "down" || health.status === "degraded" ? health.successes >= 3 ? "healthy" : "degraded" : "healthy";
    } else {
      health.failures = urgent ? Math.max(2, health.failures + 1) : health.failures + 1;
      health.successes = 0;
      health.status = health.failures >= 2 ? "down" : "degraded";
      health.lastError = { at, status };
    }
    this.flushUsage(at);
    this.decide(at);
    this.save();
  }

  private decide(at: number): void {
    const state = this.state;
    let next = state.selected;
    let reason = "manual";
    if (state.mode !== "auto") next = state.mode;
    else {
      const quotaReached = this.quotaReached(at);
      if ((quotaReached || state.health.cloudflare.status === "down") && this.healthy("livekit", at)) {
        next = "livekit";
        reason = quotaReached ? "usage-threshold" : "cloudflare-down";
      } else if (state.selected === "livekit" && !quotaReached && this.healthy("cloudflare", at) &&
        ((state.health.cloudflare.successes >= 3 && at - state.changedAt >= 60_000) || state.health.livekit.status === "down")) {
        next = "cloudflare";
        reason = "cloudflare-recovered";
      }
    }
    if (next !== state.selected) {
      state.transitions.push({ at, from: state.selected, to: next, reason });
      state.transitions = state.transitions.slice(-20);
      state.selected = next;
      state.changedAt = at;
    }
  }

  /** Coalesce checagens simultâneas; cada adaptador limita o timeout remoto. */
  async check(): Promise<void> {
    if (this.checking) return this.checking;
    this.checking = (async () => {
      await Promise.all(IDS.map(async id => {
        if (!this.configured(id)) {
          this.state.health[id].status = "unconfigured";
          this.state.health[id].checkedAt = Date.now();
          return;
        }
        const started = Date.now();
        try { await this.probe(id); this.outcome(id, true, Date.now() - started); }
        catch (error) { this.outcome(id, false, Date.now() - started, error instanceof SfuError ? error.upstreamStatus : 0); }
      }));
      this.state.checkedAt = Date.now();
      this.flushUsage(Date.now());
      this.decide(Date.now());
      this.save();
    })().finally(() => { this.checking = null; });
    return this.checking;
  }

  async select(): Promise<ProviderId> {
    if (Date.now() - this.state.checkedAt >= INTERVAL || this.state.checkedAt === 0) await this.check();
    this.flushUsage(Date.now());
    this.decide(Date.now());
    this.save();
    const id = this.state.selected;
    const quotaReached = this.quotaReached(Date.now());
    if (!this.configured(id) || (this.state.mode === "auto" && (this.state.health[id].status === "down" || (id === "cloudflare" && quotaReached)))) {
      throw new ApiError(503, "sfu_indisponivel", "O serviço de mídia está indisponível. Tente novamente em instantes.");
    }
    return id;
  }

  setMode(value: unknown): void {
    if (value !== "auto" && value !== "cloudflare" && value !== "livekit") throw new ApiError(400, "corpo_invalido", "Modo de mídia inválido.");
    if (value !== "auto" && !this.configured(value)) throw new ApiError(503, "sfu_configuracao", "O provedor escolhido não está configurado.");
    this.state.mode = value;
    this.flushUsage(Date.now());
    this.decide(Date.now());
    this.save();
  }

  status(): Record<string, unknown> {
    const at = Date.now();
    this.flushUsage(at);
    this.decide(at);
    this.save();
    const period = usagePeriod(at, this.cycleDay);
    return { enabled: true, mode: this.state.mode, provider: this.state.selected,
      health: Object.fromEntries(IDS.map(id => { const value = this.state.health[id];
        const fresh = value.checkedAt > 0 && at >= value.checkedAt && at - value.checkedAt <= INTERVAL * 3;
        return [id, {
        scope: "control", mediaStatus: "unknown",
        status: !this.configured(id) ? "unconfigured" : fresh ? value.status : "unknown",
        checkedAt: value.checkedAt, latencyMs: fresh ? value.latencyMs : null,
        errorRate: fresh && value.samples.length ? value.samples.filter(ok => !ok).length / value.samples.length : null,
        lastError: value.lastError,
      }]; })),
      usage: { estimated: true, quotaEnforced: this.env.SFU_USAGE_CALIBRATED === "1", period: period.key, resetsAt: period.end, bitrateBps: this.bitrate,
        bytes: this.state.usage[period.key] ?? { cloudflare: 0, livekit: 0 }, quotaBytes: this.quota, threshold: this.threshold },
      transitions: this.state.transitions.map(value => ({ ...value })),
    };
  }
}
