import { inviteCodeFrom, rememberInvite, type MadClient } from "./client";
import { banner, copyText, el } from "./dom";

export function createInviteLanding(client: MadClient, input: string): HTMLElement {
  const root = el("main", { class: "invite-landing card stack" });
  let code: string;
  try { code = inviteCodeFrom(input, client.base); }
  catch (error) { root.append(banner("error", error instanceof Error ? error.message : "Convite inválido.")); return root; }
  root.append(el("h1", {}, "Você recebeu um convite"), el("p", { class: "small", textContent: "Conferindo o convite…" }));
  void client.previewInvite(code).then(async preview => {
    const download = el("a", { class: "btn", textContent: "Baixar o app para Windows" });
    const hint = el("p", { class: "small muted", textContent: "Depois de instalar, volte a esta página e use Abrir convite no app." });
    const openApp = el("a", { class: "btn primary", href: `madscreen://invite/${code}`, textContent: "Já tenho o app · abrir convite" });
    const web = el("a", { class: "btn ghost", href: `/watch?invite=${encodeURIComponent(code)}`, textContent: "Continuar no navegador" });
    web.addEventListener("click", () => { rememberInvite(code, client.base); });
    const copy = el("button", { class: "btn sm", type: "button", textContent: "Copiar código" });
    const codeBox = el("input", { class: "input mono", value: code, readOnly: true });
    codeBox.setAttribute("aria-label", "Código do convite");
    copy.addEventListener("click", () => { void copyText(code).then(ok => { if (ok) copy.textContent = "Copiado"; else { codeBox.focus(); codeBox.select(); } }); });
    root.replaceChildren(el("span", { class: "eyebrow", textContent: "Convite para o madscreen" }),
      el("h1", { textContent: preview.server?.name ?? "Entre com seus amigos" }),
      el("p", { textContent: `${preview.inviter.displayName} (@${preview.inviter.username}) convidou você${preview.channel ? ` para o canal ${preview.channel.name}` : ""}.` }),
      openApp, download, hint, web, el("div", { class: "row" }, codeBox, copy));
    try {
      const info = await client.request<{ downloadAvailable: boolean }>("GET", "/app/info", undefined, false);
      if (info.downloadAvailable) download.href = "/download/windows";
      else { download.setAttribute("aria-disabled", "true"); hint.textContent = "O instalador público ainda não está disponível. Peça o instalador a quem convidou ou continue no navegador."; }
    } catch {
      download.setAttribute("aria-disabled", "true");
      hint.textContent = "Não foi possível consultar o download. Você ainda pode abrir o app instalado ou continuar no navegador.";
    }
  }).catch(error => {
    root.replaceChildren(el("h1", {}, "Não foi possível abrir o convite"), banner("error", error instanceof Error ? error.message : "Convite inválido ou expirado."), el("a", { class: "btn", href: "/watch", textContent: "Abrir madscreen" }));
  });
  return root;
}
