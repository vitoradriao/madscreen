import type { Update } from "@tauri-apps/plugin-updater";
import { invoke } from "@tauri-apps/api/core";
import { errorText } from "./api";

/** Intervalo entre consultas automáticas ao manifesto de releases. */
export const UPDATE_CHECK_INTERVAL_MS = 60 * 60 * 1000;
/** Espera após abrir o app antes da primeira consulta, para não competir com o login e o hub. */
export const UPDATE_FIRST_CHECK_DELAY_MS = 10_000;

export type UpdateResource = Pick<Update, "version" | "currentVersion" | "close" | "download" | "install">;

export interface UpdateBackend {
  check(): Promise<UpdateResource | null>;
  prepareInstall(): Promise<void>;
}

export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "checking"; manual: boolean }
  | { kind: "current"; manual: boolean }
  | { kind: "available"; version: string; currentVersion: string }
  | { kind: "downloading"; version: string; percent: number | null; megabytes: number }
  | { kind: "installing"; version: string }
  | { kind: "error"; stage: "check" | "install"; message: string; version?: string; manual: boolean };

export interface UpdateController {
  readonly status: UpdateStatus;
  /** Consulta o manifesto. Não baixa nem instala; não afeta a transmissão. */
  check(manual?: boolean): Promise<void>;
  /** Baixa (validado pelo plugin), prepara o encerramento do sidecar e instala reiniciando o app. */
  install(): Promise<void>;
  subscribe(listener: (status: UpdateStatus) => void): () => void;
  /** Agenda a primeira consulta e as seguintes a cada hora. */
  start(): void;
  dispose(): void;
}

const tauriBackend: UpdateBackend = {
  check: async () => (await import("@tauri-apps/plugin-updater")).check({ timeout: 30_000 }),
  prepareInstall: () => invoke<void>("prepare_update_install"),
};

interface Timers {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}
const browserTimers: Timers = {
  setTimeout: (fn, ms) => window.setTimeout(fn, ms),
  clearTimeout: handle => window.clearTimeout(handle as number),
};

export function createUpdateController(backend: UpdateBackend = tauriBackend, timers: Timers = browserTimers): UpdateController {
  let status: UpdateStatus = { kind: "idle" };
  let resource: UpdateResource | null = null;
  let downloaded: UpdateResource | null = null;
  let checking: Promise<void> | null = null;
  let timer: unknown = null;
  let disposed = false;
  const listeners = new Set<(status: UpdateStatus) => void>();

  const set = (next: UpdateStatus): void => {
    if (disposed) return;
    status = next;
    for (const listener of listeners) listener(next);
  };
  const busy = (): boolean => status.kind === "downloading" || status.kind === "installing";
  const release = (): void => {
    const old = resource;
    resource = null;
    downloaded = null;
    if (old) void old.close().catch(() => undefined);
  };
  const schedule = (ms: number): void => {
    if (disposed) return;
    if (timer !== null) timers.clearTimeout(timer);
    timer = timers.setTimeout(() => {
      timer = null;
      void check(false).finally(() => schedule(UPDATE_CHECK_INTERVAL_MS));
    }, ms);
  };

  async function check(manual = false): Promise<void> {
    if (disposed || busy()) return;
    if (checking) return checking;
    // Uma consulta automática não apaga um aviso já visível enquanto roda.
    if (manual || status.kind !== "available") set({ kind: "checking", manual });
    checking = (async () => {
      try {
        const found = await backend.check();
        if (disposed) { await found?.close(); return; }
        if (!found) {
          release();
          set({ kind: "current", manual });
          return;
        }
        if (resource && resource.version === found.version) {
          // Mesma versão já conhecida: mantém o recurso (e um download feito) e descarta o novo.
          void found.close().catch(() => undefined);
        } else {
          release();
          resource = found;
        }
        set({ kind: "available", version: found.version, currentVersion: found.currentVersion });
      } catch (error) {
        // Falha automática com aviso já visível: mantém o aviso; a próxima consulta tenta de novo.
        if (!manual && resource) set({ kind: "available", version: resource.version, currentVersion: resource.currentVersion });
        else set({ kind: "error", stage: "check", message: errorText(error), manual });
      } finally {
        checking = null;
      }
    })();
    return checking;
  }

  async function install(): Promise<void> {
    const update = resource;
    if (disposed || !update || busy()) return;
    let received = 0;
    let total = 0;
    set({ kind: "downloading", version: update.version, percent: null, megabytes: 0 });
    try {
      if (downloaded !== update) {
        await update.download(event => {
          if (event.event === "Started") total = event.data.contentLength ?? 0;
          if (event.event === "Progress") received += event.data.chunkLength;
          const percent = total > 0 ? Math.min(100, Math.floor(received * 100 / total)) : null;
          if (status.kind === "downloading" && status.percent === percent && percent !== null) return;
          set({ kind: "downloading", version: update.version, percent, megabytes: received / 1_048_576 });
        }, { timeout: 120_000 });
        if (disposed) return;
        downloaded = update;
      }
      // Download já validado pelo plugin (assinatura e versão). Falha de preparação impede a instalação.
      await backend.prepareInstall();
      if (disposed) return;
      set({ kind: "installing", version: update.version });
      await update.install({ restartAfterInstall: true });
    } catch (error) {
      set({ kind: "error", stage: "install", message: errorText(error), version: update.version, manual: true });
    }
  }

  return {
    get status() { return status; },
    check,
    install,
    subscribe(listener) {
      listeners.add(listener);
      listener(status);
      return () => listeners.delete(listener);
    },
    start() {
      if (timer === null && !disposed) schedule(UPDATE_FIRST_CHECK_DELAY_MS);
    },
    dispose() {
      disposed = true;
      if (timer !== null) timers.clearTimeout(timer);
      timer = null;
      listeners.clear();
      release();
    },
  };
}
