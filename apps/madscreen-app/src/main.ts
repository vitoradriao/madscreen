import "@shared/tokens.css";
import "./styles.css";
import "@shared/redesign.css";
import { initializeTheme } from "@shared/theme";
import { ApiFailure, clearInvite, pendingInvite, rememberInvite, type FriendView, type Snapshot } from "@shared/client";
import { getState, onShareState, stopShare, type ShareState } from "./api";
import { banner, el, type View } from "./dom";
import { client } from "./session";
import { createServers } from "@shared/servers-ui";
import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { showTrayNotice } from "./tray-notice";
import { createLoginScreen } from "./views/login";
import { createSettingsView } from "./views/settings";
import { createUpdateNotice } from "./views/update-notice";
import { createUpdateController } from "./updates";
import { createShareView } from "./views/share";
import { createWatchView } from "./views/watch";

const app = document.querySelector<HTMLDivElement>("#app")!;
initializeTheme();
let hub: { close(): void } | null = null;
let unlistenShare: (() => void) | null = null;
let disposeShell = (): void => {};
let renderVersion = 0;
let loginAbort: AbortController | null = null;
let processInvite: (() => Promise<void>) | null = null;
// Um único controlador para o app inteiro: consulta ao abrir e a cada hora, mesmo entre logins.
const updates = createUpdateController();

function showLogin(): void {
  renderVersion++;
  disposeShell();
  hub?.close();
  hub = null;
  unlistenShare?.();
  unlistenShare = null;
  processInvite = null;
  loginAbort?.abort();
  loginAbort = new AbortController();
  app.replaceChildren(createLoginScreen(() => void showShell(), loginAbort.signal));
}

async function showShell(): Promise<void> {
  if (!client.session) return showLogin();
  const version = ++renderVersion;
  loginAbort?.abort();
  processInvite = null;
  try {
    await client.me();
  } catch (error) {
    if (version !== renderVersion) return;
    if (error instanceof ApiFailure && error.status === 401) {
      client.setSession(null);
      return showLogin();
    }
    // Sem rede agora: segue com a sessão salva; o hub reconecta sozinho.
  }
  if (version !== renderVersion) return;
  disposeShell();
  if (!client.session?.user) return showLogin();

  const share = createShareView();
  const watch = createWatchView(() => select("watch"), () => select("servers"));
  const servers = createServers(client, {
    onSettings: () => select("settings"),
    onOpenDirectory: () => select("servers"),
    onOpenWatch: () => select("watch"),
    onOpenShare: () => select("share"),
    watchLabel: username => watch.panel.actionLabel(username),
    onWatch: (friend: FriendView) => { select("watch"); watch.watch(friend); },
    onShare: target => { select("share"); share.startInChannel(target); },
    onSnapshot: next => { receivedSnapshot = true; updateDirectory(next); },
  });
  let signingOut: Promise<void> | null = null;
  const signOut = (): Promise<void> => {
    if (!signingOut) {
      share.cancelPending();
      signingOut = (async () => {
        await stopShare();
        disposeShell();
        await client.logout();
        showLogin();
      })().finally(() => { signingOut = null; });
    }
    return signingOut;
  };
  const updateNotice = createUpdateNotice(updates);
  const settings = createSettingsView(signOut, updates);

  const tabs: { id: string; view: View }[] = [
    { id: "servers", view: servers },
    { id: "share", view: share },
    { id: "watch", view: watch },
    { id: "settings", view: settings },
  ];

  const content = el("main", { class: "content directory-mode" }, servers.root);
  let active = "";

  function select(id: string): void {
    if (signingOut || id === active) return;
    tabs.find((t) => t.id === active)?.view.hide?.();
    active = id;
    servers.setPage(id);
    for (const tab of tabs) {
      tab.view.root.hidden = tab.id !== "servers" && tab.id !== id;
    }
    tabs.find((t) => t.id === id)?.view.show?.();
  }

  for (const tab of tabs) {
    if (tab.id === "servers") continue;
    tab.view.root.hidden = true;
    servers.pageHost.append(tab.view.root);
  }
  app.replaceChildren(el("div", { class: "shell directory-mode unified-shell" }, content, watch.compact, updateNotice.root));
  select("servers");

  hub?.close();
  let disposed = false;
  let receivedSnapshot = false;
  const updateDirectory = (next: Snapshot): void => {
    if (disposed) return;
    servers.update(next);
    share.updateDirectory(next);

  };
  const inviteStatus = el("div", { class: "alerts" });
  content.prepend(inviteStatus);
  let joining: Promise<void> | null = null;
  let joinAgain = false;
  processInvite = () => {
    if (joining) { joinAgain = true; return joining; }
    const code = pendingInvite();
    if (!code || disposed) return Promise.resolve();
    inviteStatus.replaceChildren(banner("info", "Entrando no servidor do convite…"));
    joining = client.completeInvite().then(next => {
      if (disposed || !next || (pendingInvite() && pendingInvite() !== code)) return;
      receivedSnapshot = true;
      updateDirectory(next);
      if (next.destination) servers.select(next.destination);
      select("servers");
      inviteStatus.replaceChildren(banner("ok", `Você entrou em ${next.destination?.serverName ?? "seu servidor"}.`));
    }).catch(error => {
      if (disposed) return;
      const retry = el("button", { class: "btn sm", type: "button", textContent: "Tentar novamente" });
      retry.addEventListener("click", () => { void processInvite?.(); });
      const dismiss = el("button", { class: "btn ghost sm", type: "button", textContent: "Descartar convite" });
      dismiss.addEventListener("click", () => { clearInvite(); inviteStatus.replaceChildren(); });
      inviteStatus.replaceChildren(banner("error", error instanceof Error ? error.message : "Não foi possível entrar no servidor."), el("div", { class: "row" }, retry, dismiss));
      select("servers");
    }).finally(() => {
      joining = null;
      if (joinAgain && !disposed) { joinAgain = false; void processInvite?.(); }
    });
    return joining;
  };
  void client.servers().then(next => { if (!receivedSnapshot) updateDirectory(next); }).catch(error => { if (!disposed) servers.showError(error); });
  void processInvite();
  hub = client.connectHub({
    onSnapshot(next: Snapshot) { receivedSnapshot = true; updateDirectory(next); },
    onState: connected => { if (!disposed) servers.setConnected(connected); },
    onAuthLost: () => {
      if (disposed) return;
      void signOut().catch(() => {
        if (disposed) return;
        content.prepend(banner("error", "Sua sessão expirou. Não foi possível parar o compartilhamento. Pare a transmissão e tente sair novamente."));
        select("share");
      });
    },
  });

  // Estado do engine (captura/encode local) alimenta a aba Compartilhar.
  const apply = (state: ShareState): void => {
    if (disposed) return;
    share.update(state);
    servers.setSharing(state.phase !== "idle");
    updateNotice.setLive(state.phase !== "idle");
  };
  void getState().then(apply).catch(() => undefined);
  void onShareState(apply).then((unlisten) => {
    if (disposed) unlisten();
    else unlistenShare = unlisten;
  }).catch(() => undefined);
  const onVisibility = (): void => {
    if (!document.hidden) void getState().then(apply).catch(() => undefined);
  };
  document.addEventListener("visibilitychange", onVisibility);
  disposeShell = () => {
    if (disposed) return;
    disposed = true;
    hub?.close();
    hub = null;
    unlistenShare?.();
    unlistenShare = null;
    document.removeEventListener("visibilitychange", onVisibility);
    for (const tab of tabs) tab.view.dispose?.();
    updateNotice.dispose();
  };
}

function handleInviteUrls(urls: string[]): boolean {
  for (const url of urls) {
    if (!url.startsWith("madscreen:")) continue;
    try {
      rememberInvite(url, client.base);
      if (client.session) { if (processInvite) void processInvite(); else void showShell(); }
      else showLogin();
    } catch (error) {
      clearInvite();
      const message = error instanceof Error ? error.message : "Convite inválido.";
      if (client.session) void showShell().then(() => app.prepend(banner("error", message)));
      else { showLogin(); app.prepend(banner("error", message)); }
    }
    return true;
  }
  return false;
}

async function boot(): Promise<void> {
  if (isTauri()) {
    await listen("tray-close-requested", () => showTrayNotice(() => invoke<void>("hide_to_tray")));
    // Só consulta o manifesto; nada é baixado ou instalado sem o clique.
    updates.start();
    try {
      await onOpenUrl(urls => { handleInviteUrls(urls); });
      const urls = await getCurrent();
      if (urls && handleInviteUrls(urls)) return;
    } catch { /* Em dev, o protocolo pode ainda não estar registrado pelo instalador. */ }
  }
  if (client.session) await showShell();
  else showLogin();
}
void boot();
