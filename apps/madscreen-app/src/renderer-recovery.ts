import { invoke } from "@tauri-apps/api/core";

export function showRendererRecoveryNotice(): HTMLElement {
  document.querySelector("[data-renderer-recovery]")?.remove();
  const notice = document.createElement("div");
  notice.dataset.rendererRecovery = "true";
  notice.setAttribute("role", "alert");
  notice.style.cssText = "position:fixed;bottom:20px;left:20px;z-index:20000;max-width:min(480px,calc(100vw - 40px));padding:16px;border:1px solid var(--border,#555);border-radius:8px;background:var(--surface-2,#20252b);color:var(--text,#fff);box-shadow:0 8px 24px #0006";
  const text = document.createElement("p");
  text.textContent = "A interface foi recuperada após uma falha. Abra novamente as lives que estava assistindo.";
  const close = document.createElement("button");
  close.className = "btn sm"; close.textContent = "Fechar";
  close.setAttribute("aria-label", "Fechar aviso de recuperação");
  close.addEventListener("click", () => notice.remove());
  notice.append(text, close); document.body.append(notice);
  return notice;
}

export async function checkRendererRecovery(take = () => invoke<boolean>("take_renderer_recovery")): Promise<HTMLElement | null> {
  try { return await take() ? showRendererRecoveryNotice() : null; }
  catch { return null; } // Browser fixtures/older engines do not expose the command.
}
