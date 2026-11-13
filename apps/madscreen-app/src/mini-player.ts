import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { checkRendererRecovery } from "./renderer-recovery";

void checkRendererRecovery();

export interface DetachedPlayer { close(): void }
/** Uma janela por transmissão do layout (até duas lives ao mesmo tempo). */
export const MAX_MINI_PLAYERS = 2;
const active = new Set<object>();
/** O mesmo palco nunca vai para duas janelas. */
const detachedStages = new WeakSet<HTMLElement>();
export interface MiniPlayerHost {
  invoke<T>(command: string, args: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (event: { payload: string }) => void): Promise<UnlistenFn>;
}

/** Move o mesmo palco/elemento; não cria sessão, MediaStream ou receptor. */
export async function openMiniPlayer(stage: HTMLElement, title: string): Promise<DetachedPlayer> {
  return openMiniPlayerWithHost(stage, title, { invoke, listen });
}

/** Host explícito permite ensaiar o mesmo controle nativo em laboratório isolado. */
export async function openMiniPlayerWithHost(stage: HTMLElement, title: string, host: MiniPlayerHost): Promise<DetachedPlayer> {
  if (detachedStages.has(stage)) throw Error("Esta transmissão já está no mini-player.");
  if (active.size >= MAX_MINI_PLAYERS) throw Error("Já existem dois mini-players abertos.");
  let allowed = true;
  const reservation = { close: () => { allowed = false; } };
  active.add(reservation);
  detachedStages.add(stage);
  const token = crypto.randomUUID().replaceAll("-", "");
  const home = stage.ownerDocument;
  try {
    if (home.fullscreenElement) await home.exitFullscreen();
    await host.invoke("prepare_miniplayer", { token }); }
  catch (error) { active.delete(reservation); detachedStages.delete(stage); throw error; }
  if (!allowed || !stage.isConnected) {
    active.delete(reservation); detachedStages.delete(stage);
    void host.invoke("control_miniplayer", { action: "close", value: null, token }).catch(() => {});
    throw Error("A reprodução foi encerrada.");
  }
  const popup = window.open(`about:blank#${token}`, `madscreen-mini-${token}`, "width=640,height=360");
  if (!popup) {
    active.delete(reservation); detachedStages.delete(stage);
    void host.invoke("control_miniplayer", { action: "close", value: null, token }).catch(() => {});
    throw Error("Não foi possível abrir o mini-player."); }
  const placeholder = home.createElement("div");
  placeholder.className = "placeholder popout-placeholder";
  placeholder.textContent = "Esta transmissão está no mini-player.";
  const back = home.createElement("button");
  back.className = "btn sm"; back.textContent = "Voltar à janela principal";
  placeholder.append(back);
  const playingVideos = [...stage.querySelectorAll("video")].filter(video => !video.paused);
  stage.replaceWith(placeholder);
  const savedStyle = stage.getAttribute("style");
  let closed = false;
  let presentedDocument: Document | null = null;
  const subscriptions: UnlistenFn[] = [];
  const control = (action: string, value: boolean | null = null) => host.invoke<void>("control_miniplayer", { action, value, token });
  const restore = (activate = false) => {
    if (closed) return;
    closed = true;
    for (const off of subscriptions) off();
    clearInterval(recovery);
    placeholder.replaceWith(stage);
    stage.classList.remove("mini-hidden");
    if (savedStyle === null) stage.removeAttribute("style"); else stage.setAttribute("style", savedStyle);
    for (const video of playingVideos) void video.play().catch(() => {});
    active.delete(handle); detachedStages.delete(stage);
    void control(activate ? "return" : "close").catch(() => {});
  };
  const recovery = setInterval(() => {
    try {
      if (popup.closed || (presentedDocument && (popup.document !== presentedDocument
        || stage.ownerDocument !== presentedDocument || !stage.isConnected))) restore();
    } catch { restore(); }
  }, 500);
  const handle = { close: () => restore() };
  active.delete(reservation);
  active.add(handle);
  back.addEventListener("click", () => restore(true));
  try {
    for (const event of ["miniplayer-close-requested", "miniplayer-closed"]) {
      const off = await host.listen(event, message => { if (message.payload === token) restore(); });
      if (closed) off(); else subscriptions.push(off);
    }
    if (closed) return handle;
    const doc = popup.document;
    const childWindow = doc.defaultView;
    const onPageHide = () => restore();
    childWindow?.addEventListener("pagehide", onPageHide);
    subscriptions.push(() => childWindow?.removeEventListener("pagehide", onPageHide));
    doc.documentElement.dataset.theme = "dark";
    doc.title = title;
    doc.addEventListener("fullscreenchange", () => {
      if (!doc.fullscreenElement) void control("fullscreen", false).catch(() => {});
    });
    for (const node of home.querySelectorAll('link[rel="stylesheet"],style')) doc.head.append(node.cloneNode(true));
    const style = doc.createElement("style");
    style.textContent = "html,body{margin:0;width:100%;height:100%;overflow:hidden;background:var(--video-bg);color:var(--text)}body{display:block}.mini-toolbar{position:absolute;z-index:5;top:0;left:0;right:0;display:flex;gap:var(--sp-1);align-items:center;padding:var(--sp-2);background:linear-gradient(var(--scrim),transparent);opacity:0}body:hover .mini-toolbar,body:focus-within .mini-toolbar{opacity:1}.mini-toolbar button{font-size:var(--fs-xs);padding:var(--sp-1) var(--sp-2);background:var(--overlay)}.mini-toolbar span{flex:1;min-width:0;overflow:hidden;white-space:nowrap;text-overflow:ellipsis}[data-main-window-only]{display:none!important}.stage{position:absolute;inset:0;min-height:0;height:100%!important;aspect-ratio:auto!important;border:0;border-radius:0}.stage .player-top{top:var(--sp-7)}.stage video{width:100%;height:100%;object-fit:contain}.mini-hidden .controls{opacity:0;pointer-events:none}.mini-hidden:focus-within .controls,.mini-hidden:hover .controls{opacity:1;pointer-events:auto}";
    style.textContent += ".stage .player-top{top:0}body:hover .stage .player-top,body:focus-within .stage .player-top{opacity:0}";
    doc.head.append(style);
    const toolbar = doc.createElement("div"); toolbar.className = "mini-toolbar";
    const caption = doc.createElement("span"); caption.textContent = title;
    const top = doc.createElement("button"); top.className = "btn sm"; top.textContent = "Fixado"; top.setAttribute("aria-label", "Sempre no topo"); top.setAttribute("aria-pressed", "true");
    top.addEventListener("click", () => {
      const next = top.getAttribute("aria-pressed") !== "true";
      top.disabled = true;
      void control("top", next).then(() => { top.setAttribute("aria-pressed", String(next)); top.textContent = next ? "Fixado" : "Fixar no topo"; })
        .catch(() => { top.title = "Não foi possível alterar Sempre no topo."; })
        .finally(() => { top.disabled = false; });
    });
    const hide = doc.createElement("button"); hide.className = "btn sm"; hide.textContent = "Controles"; hide.setAttribute("aria-pressed", "true");
    hide.addEventListener("click", () => {
      const hidden = stage.classList.toggle("mini-hidden");
      hide.setAttribute("aria-pressed", String(!hidden));
    });
    const returnButton = doc.createElement("button"); returnButton.className = "btn sm"; returnButton.textContent = "Voltar ao app"; returnButton.addEventListener("click", () => restore(true));
    toolbar.append(caption, top, hide, returnButton);
    doc.body.replaceChildren(toolbar, stage);
    presentedDocument = doc;
    doc.addEventListener("keydown", event => {
      if (event.repeat || event.ctrlKey || event.altKey || event.metaKey) return;
      const target = event.target as HTMLElement | null;
      if (target && /^(INPUT|SELECT|TEXTAREA)$/.test(target.tagName)) return;
      if (event.key.toLowerCase() === "m") { event.preventDefault(); stage.querySelector<HTMLButtonElement>(".audio-ctl button")?.click(); }
      if (event.key.toLowerCase() === "f") stage.querySelector<HTMLButtonElement>(".player-fullscreen")?.click();
    });
    stage.style.cssText = "width:100%;flex:1;min-height:0";
    // Remover/adotar o <video> o pausa e o autoplay não retoma: reinicia a reprodução no novo documento.
    for (const video of playingVideos) void video.play().catch(() => {});
    await control("show");
    for (const video of playingVideos) void video.play().catch(() => {});
    returnButton.focus();
    return handle;
  } catch (error) { restore(); throw error; }
}
