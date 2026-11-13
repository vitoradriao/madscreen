import { inviteCodeFrom, rememberInvite, type InvitePreview, type MadClient } from "./client";
import { banner, copyText, el } from "./dom";

export function createInviteLanding(client: MadClient, input: string): HTMLElement {
  const root = el("main", { class: "invite-landing card stack" });
  let code: string;
  try { code = inviteCodeFrom(input, client.base); }
  catch (error) { root.append(banner("error", error instanceof Error ? error.message : "Convite inválido.")); return root; }
  root.append(el("h1", {}, "Você recebeu um convite"), el("p", { class: "small", role: "status", textContent: "Conferindo o convite…" }));
  void client.previewInvite(code).then(async preview => {
    const download = el("a", { class: "btn", textContent: "Baixar o madscreen para Windows" });
    const hint = el("p", { class: "small muted", textContent: "Para transmitir a sua tela, instale o app. Depois de instalar, abra este link novamente para confirmar o convite." });
    const openApp = el("a", { class: "btn", href: `madscreen://invite/${code}`, textContent: "Já tenho o app: abrir convite" });
    const web = el("a", { class: "btn ghost", href: `/watch?invite=${encodeURIComponent(code)}`, textContent: "Assistir pelo navegador, sem instalar" });
    web.addEventListener("click", () => { rememberInvite(code, client.base); });
    const copy = el("button", { class: "btn sm", type: "button", textContent: "Copiar código" });
    const codeBox = el("input", { class: "input mono", value: code, readOnly: true });
    codeBox.setAttribute("aria-label", "Código do convite");
    copy.addEventListener("click", () => { void copyText(code).then(ok => { if (ok) copy.textContent = "Código copiado"; else { codeBox.focus(); codeBox.select(); } }); });
    const destination = preview.server ? `${preview.server.name}${preview.channel ? ` · # ${preview.channel.name}` : ""}` : null;
    root.replaceChildren(
      el("h1", { textContent: destination ? `Entrar em ${preview.server!.name}` : `Adicionar ${preview.inviter.displayName} e entrar no madscreen` }),
      el("p", { class: "muted", textContent: `${destination ? `Destino: ${destination}. Este convite concede apenas participação no servidor.` : "Convite pessoal: cria somente a amizade, sem entrar em servidores."} Válido até ${new Date(preview.expiresAt * 1000).toLocaleDateString("pt-BR")}; ${preview.maxUses - preview.uses} uso(s) disponível(is).` }),
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

export function confirmInvite(preview: InvitePreview): Promise<boolean> {
  return new Promise(resolve => {
    const dialog = el("dialog", { class: "server-dialog" });
    const title = el("h2", { id: `invite-${crypto.randomUUID()}`, textContent: preview.kind === "personal" ? `Adicionar ${preview.inviter.displayName}?` : `Entrar em ${preview.server?.name ?? "servidor"}?` });
    dialog.setAttribute("aria-labelledby", title.id);
    const accept = el("button", { class: "btn primary", type: "button", textContent: preview.kind === "personal" ? "Aceitar amizade" : "Entrar no servidor" });
    const cancel = el("button", { class: "btn", type: "button", textContent: "Agora não" });
    let accepted = false;
    accept.addEventListener("click", () => { accepted = true; dialog.close(); });
    cancel.addEventListener("click", () => dialog.close());
    dialog.addEventListener("close", () => { dialog.remove(); resolve(accepted); }, { once: true });
    dialog.append(title, el("p", { textContent: preview.kind === "personal" ? "Este convite cria somente a amizade. Você escolhe com quem compartilhar." : "Este convite dá acesso somente a este servidor. Nenhuma amizade será criada." }), el("div", { class: "row" }, cancel, accept));
    document.body.append(dialog); dialog.showModal(); cancel.focus();
  });
}

export function createInviteManager(client: MadClient, serverId?: string): HTMLElement {
  const details = el("details", { class: "stack tight" });
  const list = el("div", { class: "stack tight" });
  const refresh = async (): Promise<void> => {
    try {
      const invites = await client.listInvites(serverId);
      list.replaceChildren(...invites.slice(0, 30).map(invite => {
        const active = !invite.revoked && invite.expires * 1000 > Date.now() && invite.uses < invite.max_uses;
        const copy = el("button", { class: "btn sm", type: "button", textContent: "Copiar", disabled: !active });
        copy.addEventListener("click", () => { void copyText(`${client.base}/i/${invite.code}`).then(ok => { copy.textContent = ok ? "Copiado" : "Copiar"; }); });
        const revoke = el("button", { class: "btn sm", type: "button", textContent: "Revogar", disabled: !active });
        revoke.addEventListener("click", () => { revoke.disabled = true; void client.revokeInvite(invite.code).then(refresh).catch(error => list.append(banner("error", String(error)))); });
        return el("div", { class: "row" }, el("span", { class: "grow small", textContent: `${invite.code} · ${invite.uses}/${invite.max_uses} usos · até ${new Date(invite.expires * 1000).toLocaleDateString("pt-BR")}${invite.revoked ? " · revogado" : ""}` }), copy, revoke);
      }));
      if (!invites.length) list.append(el("p", { class: "small muted", textContent: "Nenhum convite criado." }));
    } catch (error) { list.replaceChildren(banner("error", error instanceof Error ? error.message : String(error))); }
  };
  details.append(el("summary", { class: "btn ghost", textContent: serverId ? "Gerenciar convites do servidor" : "Meus convites pessoais" }), list);
  details.addEventListener("toggle", () => { if (details.open) void refresh(); });
  return details;
}
