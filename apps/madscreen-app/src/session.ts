import { MadClient } from "@shared/client";

/**
 * Worker de sinalização (contas, amigos, presença e SFU), definido na build por `VITE_MADSCREEN_URL`
 * (ver `.env.example`). Sem a variável o endereço é `madscreen.invalid` (TLD reservado): a conexão
 * falha de forma explícita em vez de apontar para o servidor de outra pessoa.
 */
export const WORKER_URL: string =
  (import.meta.env.VITE_MADSCREEN_URL as string | undefined)?.trim() || "https://madscreen.invalid";

export const client = new MadClient(WORKER_URL);

/**
 * Abre a URL no navegador do sistema (login Discord/Google). Retorna `false` quando o sistema
 * recusou a abertura, para a tela oferecer copiar o link.
 */
export async function openExternal(url: string): Promise<boolean> {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
    return true;
  } catch (error) {
    console.warn("Falha ao abrir o navegador do sistema", error);
    return window.open(url, "_blank", "noopener") !== null;
  }
}
