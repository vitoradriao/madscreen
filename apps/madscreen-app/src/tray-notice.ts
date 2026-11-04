import { banner, el } from "./dom";

/** Mantém a janela aberta até a primeira explicação ser confirmada. */
export function showTrayNotice(hide: () => Promise<void>): void {
  if (document.querySelector("#tray-notice")) return;
  const previous = document.activeElement;
  const status = el("div", { class: "alerts" });
  const cancel = el("button", { class: "btn", type: "button", textContent: "Continuar no app" });
  const confirm = el("button", { class: "btn primary", type: "button", textContent: "Entendi, ir para a bandeja" });
  const dialog = el("dialog", { id: "tray-notice", class: "server-dialog" },
    el("div", { class: "stack" }, el("h2", { id: "tray-notice-title" }, "O madscreen continua na bandeja"),
      el("p", {}, "Fechar a janela mantém sua transmissão e as lives abertas. Para voltar, clique no ícone do madscreen perto do relógio do Windows."),
      el("div", { class: "taskbar", ariaLabel: "Ícone do madscreen perto do relógio" }, el("span", { class: "mine" }, "m."), el("span", {}, "Bandeja do Windows")),
      el("p", {}, "No menu desse ícone, use Parar compartilhamento para encerrar sua live ou Sair para fechar o aplicativo."),
      el("div", { class: "row" }, cancel, confirm), status));
  dialog.setAttribute("aria-labelledby", "tray-notice-title");
  // O evento close é assíncrono; encerrar aqui deixa o diálogo removido assim que fechar.
  const finish = (): void => { dialog.remove(); if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); };
  dialog.addEventListener("close", finish);
  cancel.addEventListener("click", () => { dialog.close(); finish(); });
  confirm.addEventListener("click", async () => {
    confirm.disabled = true;
    try { await hide(); dialog.close(); finish(); }
    catch { status.replaceChildren(banner("error", "Não foi possível minimizar para a bandeja. Tente novamente.")); }
    finally { confirm.disabled = false; }
  });
  document.body.append(dialog); dialog.showModal(); cancel.focus();
}
