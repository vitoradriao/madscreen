import { ApiFailure, type MadClient } from "./client";
import { banner, el } from "./dom";

interface SfuStatus {
  mode: "auto" | "cloudflare" | "livekit";
  provider: "cloudflare" | "livekit";
  health: Record<"cloudflare" | "livekit", { status: string; latencyMs: number | null; errorRate: number | null }>;
  usage: { bytes: { cloudflare: number; livekit: number }; quotaBytes: number; threshold: number; resetsAt: number; quotaEnforced?: boolean };
  transitions: { at: number; from: "cloudflare" | "livekit"; to: "cloudflare" | "livekit"; reason: string }[];
}

const providerName = (id: string): string => id === "cloudflare" ? "Cloudflare" : "LiveKit de reserva";
const healthName: Record<string, string> = { healthy: "Saudável", degraded: "Degradado", down: "Indisponível", unknown: "Ainda não verificado", unconfigured: "Não configurado" };

/** Só aparece para contas autorizadas pelo Worker; consulta não interrompe sessões existentes. */
export function createSfuAdminPanel(client: Pick<MadClient, "request">) {
  let disposed = false, active = false, loading = false, sequence = 0;
  let lastTransition = "";
  let controller: AbortController | null = null;
  let timer: ReturnType<typeof setInterval> | undefined;
  const summary = el("div", { class: "stack tight" });
  const alerts = el("div", { class: "alerts" });
  const transitionNotice = el("div", { class: "small muted", role: "status" });
  const mode = el("select", { class: "input" },
    el("option", { value: "auto", textContent: "Automático" }),
    el("option", { value: "cloudflare", textContent: "Forçar Cloudflare" }),
    el("option", { value: "livekit", textContent: "Forçar LiveKit de reserva" }));
  mode.setAttribute("aria-label", "Provedor para novas transmissões");
  const refresh = el("button", { class: "btn sm", type: "button", textContent: "Verificar serviços" });
  const root = el("div", { class: "card stack", hidden: true },
    el("h3", {}, "Serviço de transmissão"), summary, transitionNotice,
    el("label", { class: "field" }, el("span", { class: "label", textContent: "Provedor para novas transmissões" }), mode),
    el("p", { class: "small muted", textContent: "A troca vale para novas transmissões. As que já estão abertas continuam conectadas. Forçar Cloudflare ignora o limite de uso estimado." }),
    el("div", { class: "row" }, refresh), alerts);

  async function load(change?: { mode?: string; refresh?: boolean }): Promise<void> {
    if (disposed || !active || loading) return;
    const current = ++sequence;
    controller = new AbortController();
    loading = mode.disabled = refresh.disabled = true;
    try {
      const status = await client.request<SfuStatus>(change ? "POST" : "GET", "/api/admin/sfu", change, true, controller.signal);
      if (disposed || !active || sequence !== current) return;
      root.hidden = false;
      mode.value = status.mode;
      const used = status.usage.bytes.cloudflare;
      const budget = status.usage.quotaEnforced === true ? `Troca em ${(status.usage.threshold * 100).toFixed(0)}%.`
        : status.usage.quotaEnforced === false ? "Troca por consumo desativada: estimativa sem calibração confirmada."
          : "Não foi possível determinar se a troca por consumo está ativa.";
      summary.replaceChildren(
        el("div", { class: "version" }, el("div", { class: "grow" }, el("span", { class: "small muted" }, "Novas transmissões"), el("b", {}, providerName(status.provider)))),
        el("p", { class: "small muted" }, `Uso estimado da Cloudflare: ${(used / 1e9).toLocaleString("pt-BR", { minimumFractionDigits: 2, maximumFractionDigits: 2 })} GB de ${(status.usage.quotaBytes / 1e9).toLocaleString("pt-BR", { maximumFractionDigits: 0 })} GB. ${budget} Reinício do ciclo: ${new Date(status.usage.resetsAt).toLocaleDateString("pt-BR", { timeZone: "UTC" })} (UTC).`),
        (() => {
          const progress = el("progress", { class: "usage-meter", max: Math.max(1, status.usage.quotaBytes), value: Math.max(0, used) });
          progress.setAttribute("aria-label", "Uso estimado da Cloudflare neste ciclo");
          return progress;
        })(),
        el("p", { class: "small muted" }, "As verificações abaixo medem o serviço de controle. Não foi possível determinar a saúde do vídeo e do áudio por essas sondas."),
        ...(["cloudflare", "livekit"] as const).map(id => {
          const health = status.health[id];
          return el("div", { class: "probe" }, el("div", { class: "grow" }, el("b", {}, providerName(id)), el("span", {}, `Controle${health.latencyMs === null ? "" : ` · resposta ${health.latencyMs} ms`}${health.errorRate === null ? "" : ` · falhas recentes ${(health.errorRate * 100).toFixed(0)}%`}`)),
            el("span", { class: `state${health.status === "healthy" ? "" : health.status === "unknown" ? " unknown" : " warn"}` }, healthName[health.status] ?? "Ainda não verificado"));
        }));
      const transition = status.transitions.at(-1);
      const key = transition ? `${transition.at}/${transition.from}/${transition.to}/${transition.reason}` : "";
      if (transition && key !== lastTransition) {
        lastTransition = key;
        const reason = transition.reason === "usage-threshold" ? "limite de uso estimado"
          : transition.reason === "cloudflare-down" ? "falha na Cloudflare"
            : transition.reason === "cloudflare-recovered" ? "recuperação da Cloudflare" : "escolha manual";
        transitionNotice.textContent = `Troca para ${providerName(transition.to)} em ${new Date(transition.at).toLocaleString("pt-BR")}: ${reason}.`;
      }
      alerts.replaceChildren();
      timer ??= setInterval(() => { if (!document.hidden && root.isConnected) void load(); }, 30_000);
    } catch (error) {
      if (disposed || !active || sequence !== current) return;
      if (error instanceof ApiFailure && (error.status === 403 || error.status === 404 || error.code === "sfu_desabilitado")) {
        root.hidden = true;
        clearInterval(timer); timer = undefined;
      } else {
        root.hidden = false;
        alerts.replaceChildren(banner("error", "Não foi possível consultar o serviço de transmissão. Tente novamente."));
      }
    } finally {
      if (sequence === current) loading = mode.disabled = refresh.disabled = false;
    }
  }
  mode.addEventListener("change", () => { void load({ mode: mode.value }); });
  refresh.addEventListener("click", () => { void load({ refresh: true }); });
  const hide = (): void => {
    active = false; sequence++; controller?.abort(); controller = null;
    loading = mode.disabled = refresh.disabled = false;
    clearInterval(timer); timer = undefined;
  };
  return { root, show() { active = true; void load(); }, hide, dispose() { disposed = true; hide(); } };
}
