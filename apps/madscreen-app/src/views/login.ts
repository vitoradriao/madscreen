import { clearPkce } from "@shared/client";
import { createLogin, createRecoveryNotice } from "@shared/ui";
import { el } from "../dom";
import { client, openExternal } from "../session";

/** Tela Entrar: Discord/Google abrem o navegador; o app recebe a sessão por polling seguro. */
export function createLoginScreen(onDone: () => void, signal?: AbortSignal): HTMLElement {
  const holder = el("div", { class: "auth-card card" });
  const defaultTitle = "Compartilhe a tela e o som do jogo com seus amigos";
  const defaultLead = "Mostre uma janela ou o monitor inteiro, com o som do aplicativo, em servidores só seus. Quem assiste pode acompanhar duas lives ao mesmo tempo.";
  const title = el("h1", { class: "display" }, defaultTitle);
  const lead = el("p", { class: "lead", textContent: defaultLead });
  const afterLogin = (recoveryKey?: string): void => {
    clearPkce();
    if (recoveryKey) {
      title.textContent = "Conta criada";
      lead.textContent = "Falta só guardar a sua chave de recuperação. Depois disso, você já entra no servidor.";
      holder.replaceChildren(createRecoveryNotice(recoveryKey, onDone));
    } else onDone();
  };
  holder.append(createLogin(client, { openUrl: (url) => void openExternal(url), onLoggedIn: afterLogin,
    onInviteDestination: (server, channel, inviter) => {
      title.textContent = server ? (inviter ? `${inviter} convidou você para ${server}` : `Convite para ${server}`) : defaultTitle;
      lead.textContent = server ? `${channel ? `Você entra direto em # ${channel}. ` : ""}Escolha como acessar ao lado e a live dos seus amigos aparece logo depois.` : defaultLead;
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
