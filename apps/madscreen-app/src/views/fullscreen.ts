import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";

let wasMaximized = false;

/**
 * Janela do Tauri junto com o elemento: o WebView2 sozinho deixa a moldura do app visível.
 * Janela sem moldura e maximizada mantém a área útil (sem a barra de tarefas) mesmo em tela cheia,
 * deixando faixas pretas; por isso desmaximiza antes e maximiza de volta ao sair.
 */
async function setWindowFullscreen(on: boolean) {
  try {
    const win = getCurrentWindow();
    if (on) {
      // `||`: um segundo pedido durante a transição não pode esquecer que estava maximizada.
      wasMaximized = wasMaximized || (await win.isMaximized());
      if (wasMaximized) await win.unmaximize();
      await win.setFullscreen(true);
    } else {
      await win.setFullscreen(false);
      if (wasMaximized) await win.maximize();
      wasMaximized = false;
    }
  } catch {
    /* fora do Tauri (vite dev) ou sem permissão: o fullscreen do elemento basta */
  }
}

let listening = false;

/** Alterna a tela cheia do palco de vídeo; Esc sai e devolve a janela ao normal. */
export async function toggleFullscreen(stage: HTMLElement): Promise<void> {
  if (stage.ownerDocument !== document) {
    const doc = stage.ownerDocument;
    const on = !doc.fullscreenElement;
    await invoke("control_miniplayer", { action: "fullscreen", value: on, token: stage.ownerDocument.defaultView?.location.hash.slice(1) ?? "" });
    if (on) await stage.requestFullscreen().catch(() => undefined);
    else await doc.exitFullscreen().catch(() => undefined);
    return;
  }
  if (!listening) {
    listening = true;
    document.addEventListener("fullscreenchange", () => {
      if (!document.fullscreenElement) void setWindowFullscreen(false);
    });
  }
  if (document.fullscreenElement) {
    await document.exitFullscreen();
    return;
  }
  await setWindowFullscreen(true);
  try {
    await stage.requestFullscreen();
  } catch {
    await setWindowFullscreen(false);
  }
}
