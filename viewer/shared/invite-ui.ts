import { inviteCodeFrom, rememberInvite, type MadClient } from "./client";
import { banner, copyText, el } from "./dom";

export function createInviteLanding(client: MadClient, input: string): HTMLElement {
  const root = el("main", { class: "invite-landing card stack" });
  let code: string;
  try { code = inviteCodeFrom(input, client.base); }
  catch (error) { root.append(banner("error", error instanceof Error ? error.message : "Convite inválido.")); return root; }
  root.append(el("h1", {}, "Você recebeu um convite"), el("p", { class: "small", role: "status", textContent: "Conferindo o convite…" }));
  void client.previewInvite(code).then(async preview => {
    const download = el("a", { class: "btn", textContent: "Baixar o madscreen para Windows" });
    const hint = el("p", { class: "small muted", textContent: "Para transmitir a sua tela, instale o app. Depois de instalar, abra este link de novo para entrar direto no servidor." });
    const openApp = el("a", { class: "btn", href: `madscreen://invite/${code}`, textContent: "Já tenho o app: abrir convite" });
    const web = el("a", { class: "btn ghost", href: `/watch?invite=${encodeURIComponent(code)}`, textContent: "Assistir pelo navegador, sem instalar" });
    web.addEventListener("click", () => { rememberInvite(code, client.base); });
    const copy = el("button", { class: "btn sm", type: "button", textContent: "Copiar código" });
    const codeBox = el("input", { class: "input mono", value: code, readOnly: true });
    codeBox.setAttribute("aria-label", "Código do convite");
    copy.addEventListener("click", () => { void copyText(code).then(ok => { if (ok) copy.textContent = "Código copiado"; else { codeBox.focus(); codeBox.select(); } }); });
    const destination = preview.server ? `${preview.server.name}${preview.channel ? ` · # ${preview.channel.name}` : ""}` : null;
    root.replaceChildren(
      el("h1", { textContent: destination ? `${preview.inviter.displayName} convidou você para ${preview.server!.name}` : `${preview.inviter.displayName} convidou você para o madscreen` }),
      el("p", { class: "muted", textContent: `${destination ? `Destino: ${destination}. ` : ""}No madscreen vocês compartilham a tela e o som do jogo e assistem às lives uns dos outros.` }),
      download, openApp, web, hint, el("div", { class: "row" }, codeBox, copy));
    try {
      const info = await client.request<{ downloadAvailable: boolean }>("GET", "/app/info", undefined, false);
      if (info.downloadAvailable) { download.href = "/download/windows"; download.classList.add("primary"); }
      else {
        download.setAttribute("aria-disabled", "true");
        web.className = "btn primary";
        root.insertBefore(web, download);
        hint.textContent = "O instalador ainda não está disponível por aqui. Assista pelo navegador ou peça o instalador a quem convidou.";
      }
    } catch {
      download.setAttribute("aria-disabled", "true");
      hint.textContent = "Não foi possível consultar o download. Você ainda pode abrir o app instalado ou assistir pelo navegador.";
    }
  }).catch(error => {
    root.replaceChildren(el("h1", {}, "Não foi possível abrir o convite"), banner("error", error instanceof Error ? error.message : "Convite inválido ou expirado."), el("a", { class: "btn", href: "/watch", textContent: "Abrir madscreen" }));
  });
  return root;
}
