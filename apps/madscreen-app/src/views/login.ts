import { clearPkce } from "@shared/client";
import { createLogin, createRecoveryNotice } from "@shared/ui";
import { el } from "../dom";
import { client, openExternal } from "../session";

/** Tela Entrar: Discord/Google abrem o navegador; o app recebe a sessão por polling seguro. */
export function createLoginScreen(onDone: () => void, signal?: AbortSignal): HTMLElement {
  const holder = el("div", { class: "auth-card card" });
  const title = el("h1", { class: "display" }, "Entrar no madscreen");
  const lead = el("p", { class: "lead", textContent: "Acesse seus servidores e transmissões." });
  const afterLogin = (recoveryKey?: string): void => {
    clearPkce();
    if (recoveryKey) holder.replaceChildren(createRecoveryNotice(recoveryKey, onDone));
    else onDone();
  };
  holder.append(createLogin(client, { openUrl: (url) => void openExternal(url), onLoggedIn: afterLogin,
    onInviteDestination: (server, channel) => {
      title.textContent = server ?? "Entrar no madscreen";
      lead.textContent = server ? `Você recebeu um convite para este servidor.${channel ? ` Canal de destino: # ${channel}.` : ""} Entre para continuar.` : "Acesse seus servidores e transmissões.";
    }, ...(signal ? { signal } : {}) }));
  return el(
    "div",
    { class: "login-screen" },
    el("div", { class: "login-brand" }, el("span", { class: "wordmark" }, "m."), "madscreen"),
    el(
      "main",
      { class: "login-body" },
      el(
        "div",
        { class: "fade-in" },
        title, lead,
      ),
      holder,
    ),
  );
}
