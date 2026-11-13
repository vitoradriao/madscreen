import { confirmInvite } from "../shared/invite-ui";
import { ApiFailure, MadClient, clearPkce, clearInvite, pendingInvite, rememberInvite, type FriendView, type Snapshot } from "../shared/client";
import { banner, el, svg } from "../shared/dom";
import { icon } from "../shared/icons";
import { createLogin, createRecoveryNotice, createWatchLayout, avatar } from "../shared/ui";
import { createServers } from "../shared/servers-ui";
import { createInviteLanding } from "../shared/invite-ui";

const client = new MadClient(location.origin);
const app = document.getElementById("app")!;
const params = new URLSearchParams(location.search);
const inviteMatch = /^\/i\/([^/]+)$/.exec(location.pathname);
let hub: { close(): void } | null = null;
let disposeWorkspace = (): void => {};
let renderVersion = 0;
let loginAbort: AbortController | null = null;
let startupInviteError = "";

function topbar(right?: HTMLElement): HTMLElement {
  return el("header", { class: "topbar" }, el("div", { class: "brand" }, svg(icon("logo")), "madscreen"), right ?? null);
}
function mount(...nodes: HTMLElement[]): void {
  disposeWorkspace(); hub?.close(); hub = null; app.replaceChildren(...nodes);
}
function showLogin(): void {
  renderVersion++;
  loginAbort?.abort(); loginAbort = new AbortController();
  document.title = "madscreen · entrar";
  const holder = el("div", { class: "auth-card card" });
  const afterLogin = (recoveryKey?: string): void => {
    clearPkce();
    if (recoveryKey) holder.replaceChildren(createRecoveryNotice(recoveryKey, () => void showWorkspace()));
    else void showWorkspace();
  };
  if (startupInviteError) holder.append(banner("error", startupInviteError));
  holder.append(createLogin(client, { openUrl: url => { location.href = url; }, onLoggedIn: afterLogin, ...(pendingInvite() ? { invite: pendingInvite() ?? undefined } : {}), signal: loginAbort.signal }));
  mount(topbar(), el("main", { class: "hero" }, el("div", { class: "fade-in" },
    el("h1", { class: "display", textContent: "Entrar no madscreen" }),
    el("p", { class: "lead", textContent: "Acesse seus servidores e transmissões." })), holder));
}

async function showWorkspace(): Promise<void> {
  if (!client.session) return showLogin();
  const version = ++renderVersion;
  loginAbort?.abort();
  try { await client.me(); }
  catch (error) { if (version !== renderVersion) return; if (error instanceof ApiFailure && error.status === 401) return showLogin(); }
  if (version !== renderVersion || !client.session) return;
  const user = client.session.user;
  document.title = "madscreen";
  const main = el("main", { class: "web-directory stack" });
  const alerts = el("div", { class: "stack tight" });
  if (startupInviteError) alerts.append(banner("error", startupInviteError));
  let disposed = false;
  let receivedSnapshot = false;
  let opened = false;
  let wanted = params.get("u");
  const watchPanel = createWatchLayout(client, { onLeave: () => showDirectory(), onAdd: () => chooseAnother() });
  const resume = el("button", { class: "btn sm", type: "button", textContent: "Voltar às transmissões" });
  resume.addEventListener("click", () => main.replaceChildren(watching));
  const back = el("button", { class: "btn sm", type: "button", textContent: "Parar de assistir e voltar aos servidores" });
  const watching = el("div", { class: "stack" }, back, watchPanel.root);
  const servers = createServers(client, { watchLabel: username => watchPanel.actionLabel(username), onWatch: stream => open(stream), onSnapshot: next => { receivedSnapshot = true; update(next); } });
  function showDirectory(): void { watchPanel.stop(); servers.show(); opened = false; wanted = null; history.replaceState(null, "", "/watch"); main.replaceChildren(alerts, servers.root); }
  function chooseAnother(): void { wanted = null; servers.show(); main.replaceChildren(alerts, resume, servers.root); }
  function open(stream: Pick<FriendView, "username" | "displayName">): void {
    opened = true; wanted = null; main.replaceChildren(watching); watchPanel.watch(stream);
    history.replaceState(null, "", `/watch?u=${encodeURIComponent(stream.username)}`);
  }
  back.addEventListener("click", showDirectory);
  const logout = el("button", { class: "btn ghost sm", type: "button", textContent: "Sair" });
  logout.addEventListener("click", () => { watchPanel.stop(); void client.logout().then(showLogin); });
  const chip = el("div", { class: "user-chip" }, avatar(user), el("span", { class: "small", textContent: user.displayName }), logout);
  main.append(alerts, servers.root);
  mount(topbar(chip), main);
  disposeWorkspace = () => { disposed = true; servers.dispose(); watchPanel.dispose(); };
  const update = (next: Snapshot): void => {
    if (disposed) return;
    servers.update(next);
    if (wanted && !opened) {
      const username = wanted.toLowerCase();
      const stream = (next.servers ?? []).flatMap(server => server.channels.flatMap(channel => channel.live)).find(item => item.username === username);
      if (stream) open(stream);
    }
  };
  hub = client.connectHub({ onSnapshot: next => { receivedSnapshot = true; update(next); },
    onState: connected => { if (!disposed) servers.setConnected(connected); },
    onAuthLost: () => { if (!disposed) void client.logout().then(showLogin); } });
  void client.servers().then(next => { if (!receivedSnapshot) update(next); }).catch(error => { if (!disposed) servers.showError(error); });
  const joinPending = async (): Promise<void> => {
    if (disposed || !pendingInvite()) return;
    alerts.replaceChildren(banner("info", "Conferindo o convite…"));
    try {
      const next = await client.completeInvite(confirmInvite);
      if (disposed || !next) return;
      receivedSnapshot = true;
      wanted = null;
      update(next); if (next.destination) servers.select(next.destination);
      history.replaceState(null, "", "/watch");
      alerts.replaceChildren(banner("ok", next.inviteStatus === "already_redeemed" ? "Este convite já foi usado por você. Nenhuma relação foi recriada; peça outro link se necessário." : next.destination ? `Você entrou em ${next.destination.serverName}.` : "Convite pessoal aceito. Abra Amigos para compartilhar."));
    } catch (error) {
      if (disposed) return;
      const retry = el("button", { class: "btn sm", type: "button", textContent: "Tentar novamente" });
      retry.addEventListener("click", () => { void joinPending(); });
      const dismiss = el("button", { class: "btn ghost sm", type: "button", textContent: "Descartar convite" });
      dismiss.addEventListener("click", () => { clearInvite(); alerts.replaceChildren(); });
      alerts.replaceChildren(banner("error", error instanceof Error ? error.message : "Não foi possível entrar no servidor."), el("div", { class: "row" }, retry, dismiss));
    }
  };
  void joinPending();
}

if (inviteMatch?.[1]) mount(topbar(), createInviteLanding(client, inviteMatch[1]));
else {
  const invite = params.get("invite");
  if (invite) { try { rememberInvite(invite, client.base); } catch (error) { clearInvite(); startupInviteError = error instanceof Error ? error.message : "Convite inválido."; } }
  if (client.session) void showWorkspace(); else showLogin();
}
