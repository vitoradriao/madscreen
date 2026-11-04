import { icon } from "@shared/icons";
import { el, svg } from "../dom";
import type { UpdateController, UpdateStatus } from "../updates";

export interface UpdateNotice {
  root: HTMLElement;
  /** Informa se há transmissão local em andamento, para avisar antes de reiniciar. */
  setLive(live: boolean): void;
  dispose(): void;
}

/**
 * Aviso flutuante de versão nova, no canto inferior direito, visível em qualquer aba.
 * "Depois" esconde até a próxima consulta automática (a cada hora) achar a versão de novo.
 * Nada é baixado ou instalado sem o clique em Atualizar e reiniciar.
 */
export function createUpdateNotice(updates: UpdateController): UpdateNotice {
  const root = el("aside", { class: "update-notice", hidden: true });
  root.setAttribute("aria-live", "polite");
  root.setAttribute("aria-label", "Atualização do madscreen");
  let live = false;
  let dismissed: UpdateStatus | null = null;
  let current: UpdateStatus = updates.status;

  const button = (label: string, kind: string, onClick: () => void): HTMLButtonElement => {
    const node = el("button", { class: `btn sm ${kind}`, type: "button", textContent: label });
    node.addEventListener("click", onClick);
    return node;
  };
  const later = (): HTMLButtonElement => button("Depois", "ghost", () => { dismissed = current; render(); });
  const head = (iconName: "download" | "refresh", title: string, sub: string, closable: boolean): HTMLElement => {
    const close = closable ? button("", "ghost icon-only", () => { dismissed = current; render(); }) : null;
    if (close) { close.append(svg(icon("x", "sm"))); close.setAttribute("aria-label", "Fechar aviso"); }
    return el("div", { class: "update-head" }, svg(icon(iconName)), el("div", { class: "grow" }, el("strong", { textContent: title }), el("span", { class: "muted small", textContent: sub })), close);
  };

  function render(): void {
    const status = current;
    root.classList.remove("error");
    if (status.kind === "available") {
      if (dismissed === status) { root.hidden = true; return; }
      const install = (): void => { void updates.install(); };
      root.replaceChildren(
        live
          ? head("download", "Atualizar agora encerra sua live", `A versão ${status.version} reinicia o app. Quem assiste perde a transmissão até você voltar.`, true)
          : head("download", `madscreen ${status.version} disponível`, `Você usa a ${status.currentVersion}. O app fecha e abre de novo para instalar.`, true),
        el("div", { class: "update-actions" }, later(), button(live ? "Atualizar mesmo assim" : "Atualizar e reiniciar", live ? "danger" : "primary", install)),
      );
      root.hidden = false;
      return;
    }
    if (status.kind === "downloading") {
      const label = status.percent === null ? `${status.megabytes.toFixed(1)} MB` : `${status.percent}%`;
      const bar = el("div", { class: "update-bar" }, el("span", {}));
      (bar.firstElementChild as HTMLElement).style.width = `${status.percent ?? 0}%`;
      root.replaceChildren(head("download", `Baixando a ${status.version}… ${label}`, "Você pode continuar usando o app.", false), bar);
      root.hidden = false;
      return;
    }
    if (status.kind === "installing") {
      root.replaceChildren(head("refresh", `Instalando a ${status.version}`, "O madscreen vai fechar e abrir de novo.", false));
      root.hidden = false;
      return;
    }
    if (status.kind === "error" && status.stage === "install") {
      root.classList.add("error");
      root.replaceChildren(
        head("download", "Não foi possível atualizar", `${status.message}. Nada foi instalado.`, true),
        el("div", { class: "update-actions" }, later(), button("Tentar de novo", "", () => { void updates.install(); })),
      );
      root.hidden = dismissed === status;
      return;
    }
    // Ocioso, consultando, já atualizado ou falha de consulta: sem aviso flutuante (a aba Conta mostra).
    root.hidden = true;
  }

  const unsubscribe = updates.subscribe(status => { current = status; render(); });
  return {
    root,
    setLive(next) {
      if (next === live) return;
      live = next;
      render();
    },
    dispose() { unsubscribe(); root.remove(); },
  };
}
