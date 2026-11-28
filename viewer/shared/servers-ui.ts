import { clearInvite, inviteCodeFrom, type FriendView, type InviteDestination, type MadClient, type ServerView, type Snapshot } from "./client";
import { ago, banner, copyText, el, initials, svg, tint } from "./dom";
import { avatar, createFriends } from "./ui";
import { createInviteManager, confirmInvite } from "./invite-ui";
import { icon, type IconName } from "./icons";
import { videoTheme } from "./theme";

interface ServerOptions {
  onWatch(stream: FriendView): void;
  watchLabel?(username: string): string;
  onShare?(destination: InviteDestination): void;
  onDirectShare?(friend: FriendView): void;
  onSnapshot?(snapshot: Snapshot): void;
  onSettings?(): void;
  onOpenDirectory?(): void;
  onOpenWatch?(): void;
  onOpenShare?(): void;
  /** Controles rápidos da própria transmissão no painel lateral (só no app). */
  onShareControl?(action: "pause" | "mute" | "stop"): void;
}

/** Estado da própria transmissão que o painel lateral mostra. */
export interface ShareDockStatus { paused: boolean; muted: boolean; audio: boolean; privacy: boolean }

export function createServers(client: MadClient, options: ServerOptions) {
  const storageKey = `ms.servers.${client.session?.user.id ?? "anonymous"}`;
  let serverId = "";
  let channelId = "";
  let friendsHome = true;
  try { const saved = JSON.parse(localStorage.getItem(storageKey) ?? "{}"); serverId = saved.serverId ?? ""; channelId = saved.channelId ?? ""; } catch { /* Preferência corrompida. */ }
  let snapshot: Snapshot | null = null;
  let disposed = false;
  let busy = false;
  let operationFocusKey: string | undefined;
  let joinValue = "";
  // Em janela estreita a lista de membros sobrepõe o canal: fica fechada (abre pelo botão) e volta a coluna ao alargar.
  const narrowMembers = typeof matchMedia === "function" ? matchMedia("(max-width: 960px)") : null;
  let membersExpanded = !narrowMembers?.matches;
  const onNarrowMembers = (event: MediaQueryListEvent): void => { if (disposed) return; membersExpanded = !event.matches; applyPage(); };
  narrowMembers?.addEventListener("change", onNarrowMembers);
  let watchMembersExpanded = false;
  let joinPreview = "";
  let joinTarget = "";
  let joinVersion = 0;
  let joinTimer = 0;
  const dialogs = new Set<HTMLDialogElement>();
  const closeMenus = (event: Event): void => {
    if (dialogs.size) return;
    for (const menu of root.querySelectorAll<HTMLDetailsElement>("details[data-action-menu][open]")) {
      if (event instanceof KeyboardEvent ? event.key === "Escape" : event.target instanceof Node && !menu.contains(event.target)) {
        menu.open = false;
        if (event instanceof KeyboardEvent) { menu.querySelector("summary")?.focus(); event.preventDefault(); }
      }
    }
  };
  const alerts = el("div", { class: "stack tight" });
  const operationStatus = el("div", { class: "small", role: "status", hidden: true });
  const connection = banner("info", "Conectando ao servidor…");
  const navigation = el("aside", { class: "server-navigation" });
  navigation.setAttribute("aria-label", "Servidores e canais");
  const content = el("div", { class: "server-content stack" });
  const pageHost = el("div", { class: "server-page", hidden: true });
  let activePage = "servers";
  let sharing = false;
  let shareStatus: ShareDockStatus = { paused: false, muted: false, audio: false, privacy: false };
  const serverRail = el("aside", { class: "directory-rail" });
  const topbar = el("header", { class: "channel-topbar" });
  const root = el("section", { class: "stack server-view" }, connection, alerts, operationStatus, el("div", { class: "server-workspace" }, serverRail, navigation, topbar, content, pageHost));
  const friends = createFriends(client, { onWatch: options.onWatch, ...(options.onDirectShare ? { onShare: options.onDirectShare } : {}), onSnapshot: next => { snapshot = next; options.onSnapshot?.(next); render(); } });
  let menuFrame = 0;
  const menuObserver = new ResizeObserver(queueMenuLayout);
  function queueMenuLayout(): void {
    if (disposed || menuFrame || !root.querySelector("details[data-action-menu][open]")) return;
    menuFrame = requestAnimationFrame(() => {
      menuFrame = 0;
      for (const menu of root.querySelectorAll<HTMLDetailsElement>("details[data-action-menu][open]")) {
        const trigger = menu.querySelector("summary")?.getBoundingClientRect();
        const panel = menu.querySelector<HTMLElement>(".context-actions");
        if (!trigger || !panel || !menu.isConnected) continue;
        const gap = parseFloat(getComputedStyle(root).getPropertyValue("--sp-2")) || 8;
        const box = panel.getBoundingClientRect();
        const width = document.documentElement.clientWidth;
        const height = document.documentElement.clientHeight;
        const beside = menu.classList.contains("server-add-menu");
        const preferredLeft = beside ? trigger.right + gap : trigger.left;
        const left = preferredLeft + box.width <= width - gap ? preferredLeft
          : beside ? trigger.left - gap - box.width : trigger.right - box.width;
        const below = trigger.bottom + gap;
        const top = beside ? trigger.top : below + box.height <= height - gap ? below : trigger.top - gap - box.height;
        panel.style.left = `${Math.max(gap, Math.min(left, width - box.width - gap))}px`;
        panel.style.top = `${Math.max(gap, Math.min(top, height - box.height - gap))}px`;
      }
    });
  }
  const onMenuToggle = (event: Event): void => {
    const menu = event.target;
    if (!(menu instanceof HTMLDetailsElement) || !menu.matches("[data-action-menu]") || !menu.open) return;
    for (const other of root.querySelectorAll<HTMLDetailsElement>("details[data-action-menu][open]")) if (other !== menu) other.open = false;
    queueMenuLayout();
  };
  document.addEventListener("pointerdown", closeMenus);
  root.addEventListener("keydown", closeMenus);
  root.addEventListener("toggle", onMenuToggle, true);
  window.addEventListener("resize", queueMenuLayout);
  window.addEventListener("scroll", queueMenuLayout, true);
  const liveTimes = new Map<HTMLElement, number>();
  const watchButtons = new Map<HTMLButtonElement, FriendView>();
  function refreshWatchActions(): void {
    for (const [watch, stream] of watchButtons) {
      const label = options.watchLabel?.(stream.username) ?? "Assistir";
      watch.textContent = label;
      watch.title = `${label}: ${stream.displayName}`;
      watch.setAttribute("aria-label", watch.title);
    }
  }
  const ageTimer = setInterval(() => {
    if (document.hidden || root.closest("[hidden]")) return;
    for (const [label, since] of liveTimes) label.textContent = ago(since);
  }, 30_000);

  function remember(): void {
    try { localStorage.setItem(storageKey, JSON.stringify({ serverId, channelId })); } catch { /* Sem armazenamento. */ }
  }

  function ask(title: string, initial?: string, detail?: string, label = "Nome"): Promise<string | null> {
    return new Promise(resolve => {
      const openerKey = document.activeElement instanceof HTMLElement ? document.activeElement.dataset.focuskey : undefined;
      // O menu que abriu o diálogo some enquanto ele está aberto e volta depois, com o foco na mesma ação.
      const openerMenu = document.activeElement instanceof HTMLElement ? document.activeElement.closest<HTMLDetailsElement>("details[data-action-menu]")?.dataset.actionMenu : undefined;
      for (const menu of root.querySelectorAll<HTMLDetailsElement>("details[data-action-menu][open]")) menu.open = false;
      const dialog = el("dialog", { class: "server-dialog" });
      const input = initial === undefined ? null : el("input", { class: "input", value: initial, required: true, maxLength: 40 });
      const captionId = `server-dialog-${crypto.randomUUID()}`;
      const caption = el("h2", { id: captionId, textContent: title });
      dialog.setAttribute("aria-labelledby", captionId);
      const description = detail ? el("p", { id: `${captionId}-detail`, class: "small muted", textContent: detail }) : null;
      if (description) dialog.setAttribute("aria-describedby", description.id);
      let inputField: HTMLElement | null = null;
      if (input) {
        input.id = `${captionId}-input`;
        const caption = el("label", { textContent: label });
        caption.htmlFor = input.id;
        inputField = el("div", { class: "field" }, caption, input);
      }
      const validation = el("p", { id: `${captionId}-validation`, class: "small", role: "status" });
      if (input) {
        input.setAttribute("aria-describedby", validation.id);
        input.addEventListener("input", () => { input.setCustomValidity(""); input.removeAttribute("aria-invalid"); validation.textContent = ""; });
      }
      const cancel = el("button", { class: "btn", type: "button", textContent: "Cancelar" });
      const verb = title.replace(/\?$/, "");
      const save = el("button", { class: "btn primary", type: "submit", textContent: verb.startsWith("Renomear") ? "Salvar nome" : verb });
      let result: string | null = null;
      const form = el("form", { class: "stack" }, caption, description, inputField, input ? validation : null, el("div", { class: "row" }, cancel, save));
      form.addEventListener("submit", event => {
        event.preventDefault();
        if (input && !input.value.trim()) {
          validation.textContent = "Digite um nome que não contenha apenas espaços.";
          input.setCustomValidity(validation.textContent);
          input.setAttribute("aria-invalid", "true");
          input.reportValidity();
          input.focus();
          return;
        }
        result = input?.value.trim() ?? "yes"; dialog.close();
      });
      cancel.addEventListener("click", () => dialog.close());
      dialog.addEventListener("close", () => {
        dialogs.delete(dialog);
        dialog.remove();
        if (!disposed && openerMenu) {
          const menu = [...root.querySelectorAll<HTMLDetailsElement>("details[data-action-menu]")].find(node => node.dataset.actionMenu === openerMenu);
          if (menu) menu.open = true;
        }
        if (!disposed && openerKey) restoreFocus(openerKey);
        resolve(result);
      }, { once: true });
      dialog.append(form);
      dialogs.add(dialog);
      document.body.append(dialog);
      dialog.showModal();
      (input ?? cancel).focus();
    });
  }

  const button = (text: string, label: string, key: string, action: () => void) => {
    const node = el("button", { class: "btn sm", type: "button", textContent: text, title: label, disabled: busy });
    node.setAttribute("aria-label", label);
    node.dataset.focuskey = key;
    node.addEventListener("click", action);
    return node;
  };
  const menuButton = (text: string, label: string, key: string, action: () => void, leadingIcon?: IconName) => {
    const node = button(text, label, key, action);
    node.className = "menu-item";
    if (leadingIcon) node.prepend(svg(icon(leadingIcon, "sm")));
    return node;
  };

  async function run(operation: () => Promise<Snapshot | void>): Promise<void> {
    if (busy || disposed) return;
    const openerKey = document.activeElement instanceof HTMLElement ? document.activeElement.dataset.focuskey : undefined;
    operationFocusKey = openerKey;
    busy = true;
    alerts.replaceChildren();
    operationStatus.textContent = "Concluindo operação…";
    operationStatus.hidden = false;
    render();
    try {
      const next = await operation();
      if (!disposed && next) {
        snapshot = next;
        if (next.destination) { friendsHome = false; serverId = next.destination.serverId; channelId = next.destination.channelId; remember(); }
        options.onSnapshot?.(next);
      }
    } catch (error) {
      if (!disposed) alerts.replaceChildren(banner("error", error instanceof Error ? error.message : "Não foi possível concluir a operação."));
    } finally {
      busy = false;
      operationFocusKey = undefined;
      if (!disposed) {
        operationStatus.textContent = "";
        operationStatus.hidden = true;
        render();
        if (!dialogs.size && openerKey) restoreFocus(openerKey);
      }
    }
  }

  /** Convite do servidor; com `live`, o link abre direto a live dessa pessoa no navegador. */
  async function invite(server: ServerView, target = channelId, live?: FriendView): Promise<void> {
    const openerKey = operationFocusKey;
    const invite = await client.createInvite(server.id, target);
    if (disposed) return;
    const link = live ? `${invite.link}?u=${encodeURIComponent(live.username)}` : invite.link;
    const titleId = `invite-dialog-${crypto.randomUUID()}`;
    const input = el("input", { class: "input", id: `${titleId}-link`, value: link, readOnly: true });
    const linkLabel = el("label", { textContent: live ? "Link para assistir" : "Link do convite" });
    linkLabel.htmlFor = input.id;
    const days = Math.max(1, Math.round((invite.expiresAt * 1000 - Date.now()) / 86_400_000));
    const where = invite.destination ? `${invite.destination.serverName} · # ${invite.destination.channelName}` : server.name;
    const validity = `Vale por ${days} ${days === 1 ? "dia" : "dias"} e para até ${invite.maxUses} ${invite.maxUses === 1 ? "pessoa" : "pessoas"}.`;
    const terms = el("p", { class: "small muted", textContent: live
      ? `A pessoa abre o link no navegador, entra com Discord ou com um nome e cai direto na sua live, sem instalar nada. ${validity} Quem entrar passa a fazer parte de ${server.name}.`
      : `Leva para ${where}. ${validity} Quem ainda não tem o madscreen pode baixar o app ou assistir pelo navegador. Qualquer membro do servidor pode convidar.` });
    const copyStatus = el("p", { class: "small", role: "status" });
    const copy = button("Copiar link", live ? "Copiar link para assistir" : "Copiar link de convite", "copy-invite", () => {
      void copyText(link).then(copied => {
        if (!dialog.open) return;
        copy.textContent = copied ? "Link copiado" : "Copiar link";
        copyStatus.textContent = copied ? "Cole o link no WhatsApp, no Discord ou onde seus amigos estiverem." : "Não foi possível copiar. O link foi selecionado; use Ctrl+C para copiar.";
        if (!copied) { input.focus(); input.select(); }
      });
    });
    copy.disabled = false;
    const dialog = el("dialog", { class: "server-dialog" });
    dialog.setAttribute("aria-labelledby", titleId);
    const close = button("Fechar", "Fechar convite", "close-invite", () => dialog.close());
    close.disabled = false;
    copy.classList.add("primary");
    dialog.append(el("div", { class: "stack" },
      el("h2", { id: titleId, textContent: live ? "Convidar para assistir à sua live" : `Convidar para ${server.name}` }),
      terms, el("div", { class: "field" }, linkLabel, el("div", { class: "invite-link" }, input, copy)), copyStatus, el("div", { class: "row" }, close)));
    dialog.addEventListener("close", () => {
      dialogs.delete(dialog);
      dialog.remove();
      if (!disposed && openerKey) restoreFocus(openerKey);
    }, { once: true });
    dialogs.add(dialog);
    document.body.append(dialog);
    dialog.showModal();
    copy.focus();
  }

  function createServer(): void {
    void ask("Criar servidor", "", "Você será o dono e poderá convidar amigos, criar canais e renomear o servidor.", "Nome do servidor")
      .then(name => { if (name) void run(() => client.createServer(name)); });
  }

  /** Campo de convite com prévia do destino, igual ao da entrada. Os campos visíveis compartilham o valor. */
  function joinWidget(key: string): HTMLElement {
    const input = el("input", { class: "input", placeholder: "Código ou link do convite", maxLength: 512, disabled: busy, value: joinValue, spellcheck: false, autocomplete: "off" });
    input.setAttribute("aria-label", "Código ou link do convite");
    input.dataset.focuskey = key;
    const status = el("p", { class: "small muted join-status", role: "status", textContent: joinPreview });
    const submit = el("button", { class: "btn primary sm join-submit", type: "submit", textContent: joinTarget ? `Entrar em ${joinTarget}` : "Entrar", disabled: busy || !joinValue.trim() });
    const form = el("form", { class: "stack tight join-form" }, input, status, submit);
    const paint = (): void => {
      for (const node of root.querySelectorAll<HTMLElement>(".join-status")) node.textContent = joinPreview;
      for (const node of root.querySelectorAll<HTMLButtonElement>(".join-submit")) {
        node.textContent = joinTarget ? `Entrar em ${joinTarget}` : "Entrar";
        node.disabled = busy || !joinValue.trim();
      }
      for (const node of root.querySelectorAll<HTMLInputElement>(".join-form input")) if (node !== document.activeElement) node.value = joinValue;
    };
    input.addEventListener("input", () => {
      joinValue = input.value;
      joinTarget = "";
      const version = ++joinVersion;
      clearTimeout(joinTimer);
      joinPreview = joinValue.trim() ? "Conferindo o convite…" : "";
      paint();
      if (!joinValue.trim()) return;
      joinTimer = window.setTimeout(() => {
        client.previewInvite(joinValue).then(preview => {
          if (version !== joinVersion || disposed) return;
          joinTarget = preview.server?.name ?? "";
          joinPreview = preview.server
            ? `Convite de ${preview.inviter.displayName} para ${preview.server.name}${preview.channel ? ` · # ${preview.channel.name}` : ""}.`
            : `Convite de ${preview.inviter.displayName}.`;
          paint();
        }).catch((error: unknown) => {
          if (version !== joinVersion || disposed) return;
          joinPreview = `${error instanceof Error ? error.message : "Convite inválido."} Confira o convite ou peça um novo link a quem convidou.`;
          paint();
        });
      }, 400);
    });
    form.addEventListener("submit", event => {
      event.preventDefault();
      const value = input.value;
      joinValue = value;
      if (!value.trim()) { input.focus(); return; }
      void run(async () => {
        const session = client.session;
        if (!await confirmInvite(await client.previewInvite(value))) return;
        if (disposed || client.session !== session) return;
        const next = await client.redeemInvite(inviteCodeFrom(value, client.base));
        joinValue = ""; joinPreview = ""; joinTarget = ""; joinVersion++;
        clearInvite();
        return next;
      });
    });
    return form;
  }

  /** Painel fixo da própria transmissão: entrar ao vivo, convidar para assistir e os controles rápidos. */
  function shareDock(target: InviteDestination | null): HTMLElement | null {
    if (!options.onShare && !options.onOpenShare) return null;
    const me = snapshot?.me?.id;
    const mine = sharing && me ? (snapshot?.servers ?? []).flatMap(item => item.channels.flatMap(room =>
      room.live.filter(stream => stream.id === me).map(stream => ({ server: item, channel: room, stream }))))[0] : undefined;
    const dock = el("section", { class: `share-dock${sharing ? " on" : ""}`, ariaLabel: "Sua transmissão" });
    const head = (text: string) => el("div", { class: "dock-head" }, el("span", { class: "dock-dot", ariaHidden: "true" }), el("strong", { textContent: text, title: text }));
    if (!sharing) {
      const share = button("Compartilhar tela", target ? `Compartilhar tela em ${target.channelName}` : "Escolher o que compartilhar", "share-here",
        () => target && options.onShare ? options.onShare(target) : options.onOpenShare?.());
      share.classList.add("live", "block");
      share.prepend(svg(icon("screenShare", "sm")));
      dock.append(head("Sua tela está fora do ar"), el("p", { class: "small muted" }, "Escolha uma janela ou a tela inteira, com o som do jogo."), share);
      return dock;
    }
    const header = head(mine ? `Ao vivo em # ${mine.channel.name}` : "Transmissão em andamento");
    if (mine?.stream.live) {
      const age = el("span", { class: "dock-age", textContent: ago(mine.stream.live.since) });
      liveTimes.set(age, mine.stream.live.since);
      header.append(age);
    }
    const viewers = mine?.stream.live?.viewers ?? 0;
    dock.append(header, el("p", { class: "small muted" }, shareStatus.paused ? "Vídeo pausado: quem assiste vê a tela parada."
      : mine ? `${mine.stream.live?.title || "Tela compartilhada"} · ${viewers} assistindo` : "Abra os ajustes para ver o destino e a qualidade."));
    if (mine) {
      const watchInvite = button("Convidar para assistir", "Criar link para assistir à sua live pelo navegador", "share-invite",
        () => { void run(() => invite(mine.server, mine.channel.id, mine.stream)); });
      watchInvite.classList.add("block");
      watchInvite.prepend(svg(icon("link", "sm")));
      dock.append(watchInvite);
    }
    const controls = el("div", { class: "dock-controls" });
    const control = (label: string, title: string, key: string, glyph: IconName, action: () => void, pressed?: boolean) => {
      const node = button("", title, key, action);
      node.className = "dock-control";
      node.append(svg(icon(glyph, "sm")), el("span", {}, label));
      if (pressed !== undefined) node.setAttribute("aria-pressed", String(pressed));
      return node;
    };
    if (options.onShareControl && shareStatus.privacy) {
      controls.append(control(shareStatus.paused ? "Voltar" : "Pausar", shareStatus.paused ? "Retomar o vídeo" : "Pausar o vídeo", "share-pause",
        shareStatus.paused ? "play" : "pause", () => options.onShareControl?.("pause"), shareStatus.paused));
      if (shareStatus.audio) controls.append(control(shareStatus.muted ? "Sem som" : "Som", shareStatus.muted ? "Reativar o som compartilhado" : "Silenciar o som compartilhado",
        "share-mute", shareStatus.muted ? "volumeOff" : "volume", () => options.onShareControl?.("mute"), shareStatus.muted));
    }
    if (options.onOpenShare) controls.append(control("Ajustes", "Abrir fonte, qualidade e diagnóstico da transmissão", "open-share", "settings", options.onOpenShare));
    if (options.onShareControl) {
      const stop = control("Parar", "Parar a transmissão", "share-stop", "stop", () => options.onShareControl?.("stop"));
      stop.classList.add("stop");
      controls.append(stop);
    }
    dock.append(controls);
    return dock;
  }

  function render(): void {
    if (disposed) return;
    menuObserver.disconnect();
    const active = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusKey = active && root.contains(active) ? active.dataset.focuskey : undefined;
    const selection = active instanceof HTMLInputElement ? [active.selectionStart, active.selectionEnd] as const : null;
    const openMenus = new Set([...root.querySelectorAll<HTMLDetailsElement>("details[data-action-menu][open]")].map(menu => menu.dataset.actionMenu));
    const actionMenu = (label: string, key: string) => {
      const summary = el("summary", { class: "btn ghost sm", textContent: label });
      summary.dataset.focuskey = `${key}-menu`;
      const items = el("div", { class: "context-actions menu" });
      const menu = el("details", { class: "action-menu", open: openMenus.has(key) }, summary, items);
      menu.dataset.actionMenu = key;
      menuObserver.observe(items);
      return { menu, items };
    };
    const servers = snapshot?.servers ?? [];
    const server = servers.find(item => item.id === serverId) ?? servers[0];
    const channel = server?.channels.find(item => item.id === channelId) ?? server?.channels[0];
    if (snapshot?.servers) { serverId = server?.id ?? ""; channelId = channel?.id ?? ""; remember(); }
    const rail = el("nav", { class: "server-rail" });
    const home = button("Amigos", "Abrir Amigos", "friends-home", () => { friendsHome = true; render(); options.onOpenDirectory?.(); });
    home.prepend(svg(icon("users")));
    rail.append(home);
    rail.setAttribute("aria-label", "Escolher servidor");
    for (const item of servers) {
      const pick = button("", item.name, `server-${item.id}`, () => { friendsHome = false; serverId = item.id; channelId = ""; render(); options.onOpenDirectory?.(); });
      // "Galera do Tiro" vira GT: iniciais das duas primeiras palavras, ignorando as curtas como "de" e "do".
      const words = item.name.trim().split(/\s+/).filter(word => word.length > 2);
      pick.append((words.length > 1 ? [...words[0]!][0]! + [...words[1]!][0]! : initials(item.name)).toUpperCase());
      pick.classList.add("server-pick");
      pick.style.setProperty("--tint", tint(item.id));
      const liveCount = item.channels.reduce((total, channel) => total + channel.live.length, 0);
      if (liveCount && (friendsHome || item.id !== serverId)) pick.append(el("span", { class: "server-live-dot", ariaLabel: `${liveCount} transmissões em outro servidor` }));
      rail.append(pick);
    }
    const addServer = actionMenu("+", "add-server");
    addServer.menu.classList.add("server-add-menu");
    const addSummary = addServer.menu.querySelector("summary")!;
    addSummary.setAttribute("aria-label", "Adicionar servidor");
    addSummary.replaceChildren(svg(icon("plus")));
    addSummary.title = "Adicionar servidor";
    addServer.items.append(menuButton("Criar servidor", "Criar servidor", "server-create", createServer, "plus"));
    rail.append(addServer.menu);
    serverRail.replaceChildren(el("div", { class: "directory-brand", ariaLabel: "madscreen" }, "m", el("span", { class: "brand-dot", ariaHidden: "true" }, ".")), rail);
    const channelList = el("nav", { class: "stack tight" });
    channelList.setAttribute("aria-label", "Canais do servidor");
    for (const item of server?.channels ?? []) {
      const pick = button("", `Abrir canal ${item.name}`, `channel-${item.id}`, () => { channelId = item.id; remember(); render(); options.onOpenDirectory?.(); });
      pick.append(el("span", { class: "channel-symbol", ariaHidden: "true" }, "#"), el("span", { class: "grow" }, item.name));
      const spectators = item.live.reduce((total, stream) => total + (stream.live?.viewers ?? 0), 0);
      if (item.live.length) pick.setAttribute("aria-label", `Abrir canal ${item.name}, ${spectators} espectadores`);
      if (item.live.length) pick.append(el("span", { class: "channel-live-count count", ariaLabel: `${item.live.length} transmissões no canal` }, String(item.live.length)));
      pick.classList.add("channel-pick");
      pick.setAttribute("aria-current", String(item.id === channelId));
      channelList.append(pick);
      // Cada live aparece sob o seu canal: um clique assiste, sem passar pela grade.
      for (const stream of item.live) {
        const watch = button("", `Assistir à transmissão de ${stream.displayName} em ${item.name}`, `side-live-${stream.id}`, () => {
          channelId = item.id; remember(); render(); options.onWatch(stream);
        });
        watch.className = "live-pick";
        watch.append(avatar(stream), el("b", { textContent: stream.displayName }), el("span", { class: "live-title", textContent: stream.live?.title || "Tela compartilhada" }), svg(icon("radio", "sm")));
        channelList.append(watch);
      }
    }
    addServer.items.append(el("hr", { class: "menu-sep" }), el("div", { class: "server-join stack tight" },
      el("span", { class: "menu-label" }, svg(icon("link", "sm")), "Entrar com convite"), joinWidget("join-code")));
    const sideActions = el("div", { class: "channel-side-actions stack tight" });
    const user = client.session?.user;
    const profile = el("div", { class: "channel-profile" }, user ? avatar(user, true) : null,
      el("div", { class: "grow" }, el("strong", {}, user?.displayName ?? "Conta"), el("p", { class: "small muted" }, "Online")));
    if (options.onSettings) { const settings = button("", "Abrir configurações da conta", "account-settings", options.onSettings); settings.append(svg(icon("settings"))); profile.append(settings); }
    navigation.replaceChildren(el("h2", { class: "server-name", textContent: server?.name ?? "Seus servidores" }));
    content.replaceChildren();
    liveTimes.clear();
    watchButtons.clear();
    let memberToggle: HTMLButtonElement | null = null;
    if (snapshot && !snapshot.servers) {
      content.append(banner("info", "O servidor de sinalização precisa ser atualizado para oferecer servidores e canais."));
    } else if (!snapshot) {
      const skeleton = el("div", { class: "channel-main stack", role: "status", ariaLabel: "Carregando servidores" },
        el("p", { class: "muted" }, "Carregando servidores…"), el("div", { class: "server-streams" }, el("div", { class: "skel skel-tile" }), el("div", { class: "skel skel-tile" })));
      content.append(skeleton);
    } else if (!server || !channel) {
      const create = button("Criar um servidor", "Criar um servidor", "empty-create", createServer);
      create.classList.add("ghost");
      content.append(el("div", { class: "empty-main empty-start" }, svg(icon("users")),
        el("h2", {}, "Você ainda não está em nenhum servidor"),
        el("p", { class: "small", textContent: "Cole o convite que um amigo mandou para entrar no servidor dele, ou crie o seu e chame a turma." }),
        joinWidget("empty-join"), create));
    } else {
      const target: InviteDestination = { serverId: server.id, serverName: server.name, channelId: channel.id, channelName: channel.name };
      const makeShare = (key: string) => {
        const share = button("Compartilhar tela", `Compartilhar tela em ${channel.name}`, key, () => sharing ? options.onOpenShare?.() : options.onShare?.(target));
        share.classList.add("live", "channel-share");
        share.prepend(svg(icon("screenShare", "sm")));
        return share;
      };
      if (server.role !== "member") {
        const channelMenu = actionMenu("Opções do canal", `channel-${channel.id}`);
        sideActions.append(channelMenu.menu);
        channelMenu.items.append(menuButton("Novo canal", "Criar canal", "channel-create", () => {
          void ask("Criar canal", "", undefined, "Nome do canal").then(name => { if (name) void run(() => client.createChannel(server.id, name)); });
        }, "plus"));
        channelMenu.items.append(menuButton("Renomear", "Renomear canal", "channel-rename", () => {
          void ask("Renomear canal", channel.name, undefined, "Nome do canal").then(name => { if (name) void run(() => client.renameChannel(server.id, channel.id, name)); });
        }));
        if (server.channels.length > 1) {
          const remove = menuButton("Apagar canal", "Apagar canal e encerrar suas transmissões", "channel-remove", () => {
            void ask("Apagar canal?", undefined, `O canal ${channel.name} será removido e suas transmissões serão encerradas.`).then(ok => { if (ok) void run(() => client.removeChannel(server.id, channel.id)); });
          });
          remove.classList.add("destructive");
          channelMenu.items.append(el("hr", { class: "menu-sep" }), remove);
        }
      }
      const streams = el("div", { class: "server-streams" });
      for (const stream of channel.live) {
        const watch = button("Assistir", `Assistir à transmissão de ${stream.displayName}`, `watch-${stream.id}`, () => options.onWatch(stream));
        watchButtons.set(watch, stream);
        const age = el("span", { textContent: stream.live ? ago(stream.live.since) : "" });
        if (stream.live) liveTimes.set(age, stream.live.since);
        const tile = el("article", { class: "stream-card tile" },
          el("div", { class: "noframe stream-cover", ariaHidden: "true" }, avatar(stream)),
          el("span", { class: "tl badge live", textContent: "Ao vivo" }),
          el("span", { class: "tr pill", ariaLabel: `${stream.live?.viewers ?? 0} espectadores` }, svg(icon("users", "sm")), String(stream.live?.viewers ?? 0)),
          el("div", { class: "info" }, avatar(stream, true),
            el("div", { class: "who" }, el("b", { textContent: stream.displayName }), el("span", { textContent: stream.live?.title || "Tela compartilhada" }), age), watch));
        tile.dataset.theme = videoTheme();
        tile.style.setProperty("--tint", tint(stream.id));
        streams.append(tile);
      }
      if (!channel.live.length && server.members.length <= 1 && server.role !== "member") {
        // Conta nova: o servidor só tem você. Sem amigos não há quem assista, então convidar vem antes.
        const inviteNow = button("Convidar amigos", "Criar convite deste canal", "empty-invite", () => { void run(() => invite(server)); });
        inviteNow.classList.add("primary");
        inviteNow.prepend(svg(icon("userPlus", "sm")));
        const shareAlone = options.onShare ? makeShare("empty-share") : null;
        shareAlone?.classList.remove("live");
        const rename = server.role === "owner" ? button("Trocar o nome", "Renomear servidor", "empty-rename", () => {
          void ask("Renomear servidor", server.name, undefined, "Nome do servidor").then(name => { if (name) void run(() => client.renameServer(server.id, name)); });
        }) : null;
        rename?.classList.add("ghost");
        streams.append(el("div", { class: "empty-main empty-start" }, svg(icon("userPlus")),
          el("h2", {}, "Chame seus amigos para cá"),
          el("p", { class: "small", textContent: `${server.name} é o seu servidor. Mande o convite para quem vai assistir às suas lives e transmitir junto com você.` }),
          el("div", { class: "row empty-actions" }, inviteNow, shareAlone, rename)));
      } else if (!channel.live.length) {
        const inviteMore = button("Convidar pessoas", "Convidar pessoas para o servidor", "empty-invite", () => { void run(() => invite(server)); });
        inviteMore.prepend(svg(icon("userPlus", "sm")));
        streams.append(el("div", { class: "empty-main" }, svg(icon("radio")), el("h2", {}, options.onShare ? "Seja o primeiro a entrar ao vivo" : `Ninguém ao vivo em # ${channel.name}`),
          el("p", { class: "small", textContent: options.onShare ? "Compartilhe a tela do jogo ou chame alguém para o servidor." : "Quando alguém compartilhar a tela neste canal, a live aparece aqui." }),
          el("div", { class: "row empty-actions" }, options.onShare ? makeShare("empty-share") : null, inviteMore)));
      }
      // Lives dos outros canais e servidores, para não precisar caçar quem está no ar.
      const elsewhere = servers.flatMap(item => item.channels.flatMap(room => room === channel ? [] : room.live.map(stream => ({ item, room, stream }))));
      const also = elsewhere.length ? el("section", { class: "also-live", ariaLabel: "Também ao vivo" }, el("h3", {}, "Também ao vivo")) : null;
      for (const { item, room, stream } of elsewhere) {
        const go = button("", `Assistir à transmissão de ${stream.displayName} em ${item.name}, # ${room.name}`, `also-${room.id}-${stream.id}`, () => {
          friendsHome = false; serverId = item.id; channelId = room.id; remember(); render(); options.onWatch(stream);
        });
        go.className = "also-row";
        go.append(avatar(stream), el("span", { class: "grow" }, el("b", { textContent: stream.displayName }), el("small", { textContent: stream.live?.title || "Tela compartilhada" })),
          el("span", { class: "also-where", textContent: `${item.id === server.id ? "" : `${item.name} · `}# ${room.name}` }), el("span", { class: "btn sm", textContent: "Assistir" }));
        also?.append(go);
      }
      const members = el("aside", { class: "server-members", hidden: !membersExpanded, ariaLabel: "Membros do servidor" });
      const toggleMembers = () => {
        if (activePage === "watch") watchMembersExpanded = !watchMembersExpanded;
        else membersExpanded = !membersExpanded;
        applyPage();
      };
      memberToggle = button("Membros", "Mostrar ou ocultar membros", "toggle-members", toggleMembers); memberToggle.prepend(svg(icon("users", "sm"))); memberToggle.setAttribute("aria-expanded", String(membersExpanded));
      memberToggle.classList.add("ghost");
      const closeMembers = button("", "Ocultar membros", "close-members", toggleMembers); closeMembers.append(svg(icon("x", "sm")));
      members.append(el("div", { class: "row" }, el("span", { class: "eyebrow grow" }, `Membros · ${server.members.length}`), closeMembers));
      const roster = el("div", { class: "stack tight mt" });
      const liveMembers = new Map(server.channels.flatMap(item => item.live.map(stream => [stream.id, stream] as const)));
      const group = (person: typeof server.members[number]) => liveMembers.has(person.id) ? "Transmitindo" : person.online ? "Online" : "Offline";
      const order = ["Transmitindo", "Online", "Offline"];
      let lastGroup = "";
      for (const person of [...server.members].sort((a, b) => order.indexOf(group(a)) - order.indexOf(group(b)))) {
        const heading = group(person);
        if (heading !== lastGroup) {
          roster.append(el("span", { class: "eyebrow roster-group" }, `${heading} — ${server.members.filter(member => group(member) === heading).length}`));
          lastGroup = heading;
        }
        const live = liveMembers.get(person.id);
        const portrait = avatar(person, person.online);
        if (live) {
          const presence = portrait.querySelector<HTMLElement>(".presence");
          presence?.classList.add("live");
          presence?.replaceChildren(svg(icon("radio", "sm")));
          presence?.setAttribute("title", "Ao vivo");
        }
        const name = el("span", { class: "name" }, el("span", { class: "member-name", textContent: person.displayName, title: person.displayName }));
        if (person.role !== "member") name.append(el("span", { class: "role" }, person.role === "owner" ? "Dono" : "Admin"));
        const row = el("div", { class: `member member-row${person.online ? "" : " offline"}` }, portrait,
          el("div", { class: "text member-identity" }, name, el("span", { class: "activity", textContent: live ? `Ao vivo · ${live.live?.title || "Tela compartilhada"}` : person.online ? "Online" : "Offline" })));
        const canKick = person.id !== snapshot?.me?.id && person.role !== "owner" && (server.role === "owner" || server.role === "admin" && person.role === "member");
        // Remover fica no ⋯: um botão fixo na linha cortava o nome no painel de 248 px.
        const permissions = server.role === "owner" && person.role !== "owner" || canKick ? actionMenu("⋯", `member-${person.id}`) : null;
        permissions?.menu.querySelector("summary")!.setAttribute("aria-label", `Opções de ${person.displayName}`);
        if (permissions && server.role === "owner" && person.role !== "owner") {
          permissions.items.append(menuButton(person.role === "admin" ? "Remover admin" : "Tornar admin", `Alterar permissão de ${person.displayName}`, `role-${person.id}`, () => { void run(() => client.setMemberRole(server.id, person.id, person.role === "admin" ? "member" : "admin")); }, "key"));
          permissions.items.append(menuButton("Transferir propriedade", `Transferir propriedade para ${person.displayName}`, `transfer-${person.id}`, () => { void ask("Transferir propriedade?", undefined, `${person.displayName} será o novo dono. Você ficará como administrador.`).then(ok => { if (ok) void run(() => client.transferServer(server.id, person.id)); }); }));
        }
        if (permissions && canKick) {
          if (permissions.items.childElementCount) permissions.items.append(el("hr", { class: "menu-sep" }));
          const kick = menuButton("Remover do servidor", `Remover ${person.displayName} do servidor`, `kick-${person.id}`, () => { void ask("Remover membro?", undefined, `${person.displayName} perderá acesso aos canais e às transmissões deste servidor.`).then(ok => { if (ok) void run(() => client.removeMember(server.id, person.id)); }); });
          kick.classList.add("destructive");
          permissions.items.append(kick);
        }
        if (permissions) row.append(permissions.menu);
        roster.append(row);
      }
      members.append(roster);
      const heading = channel.live.length ? el("div", { class: "view-head channel-heading" }, el("h1", {}, "Ao vivo agora"),
        el("p", { class: "muted" }, "Clique numa live para assistir. Dá para ver duas ao mesmo tempo.")) : null;
      const body = el("div", { class: `channel-body${membersExpanded ? "" : " members-collapsed"}` },
        el("div", { class: "channel-main stack" }, heading, streams, also), members);
      content.append(body);
      if (options.onShare && channel.live.length) {
        const callout = button("", `Compartilhar tela em ${channel.name}`, "share-callout", () => options.onShare?.(target));
        callout.className = "callout";
        callout.append(svg(icon("screenShare", "sm")), el("span", { class: "grow" }, el("b", {}, "Sua tela está fora do ar."), ` Entre ao vivo aqui em # ${channel.name}.`), el("span", { class: "btn sm" }, "Compartilhar tela"));
        body.querySelector(".channel-main")?.append(callout);
      }
      const serverMenu = actionMenu(server.name, `server-${server.id}`);
      if (server.role !== "member") serverMenu.items.append(createInviteManager(client, server.id));
      serverMenu.menu.classList.add("server-title-menu");
      const serverSummary = serverMenu.menu.querySelector("summary")!;
      serverSummary.setAttribute("aria-label", "Opções do servidor");
      serverSummary.title = server.name;
      serverSummary.replaceChildren(el("span", { class: "server-title", textContent: server.name }), svg(icon("chevronDown", "sm")));
      navigation.querySelector('.server-name')?.replaceWith(serverMenu.menu);
      const leaveServer = menuButton("Sair do servidor", "Sair do servidor", "server-leave", () => {
        void ask("Sair do servidor?", undefined, "Suas transmissões neste servidor serão encerradas. Você precisará de um novo convite para voltar.").then(ok => { if (ok) void run(() => client.leaveServer(server.id)); });
      }, "logout");
      if (server.role === "owner") {
        serverMenu.items.append(menuButton("Renomear servidor", "Renomear servidor", "server-rename", () => { void ask("Renomear servidor", server.name, undefined, "Nome do servidor").then(name => { if (name) void run(() => client.renameServer(server.id, name)); }); }));
      }
      if (server.role === "owner") serverMenu.items.append(el("hr", { class: "menu-sep" }));
      if (server.role !== "owner") serverMenu.items.append(leaveServer);
      if (server.role === "owner") {
        const remove = menuButton("Apagar servidor", "Apagar servidor", "server-remove", () => { void ask("Apagar servidor?", undefined, "Todos os canais, convites e transmissões deste servidor serão encerrados.").then(ok => { if (ok) void run(() => client.removeServer(server.id)); }); });
        remove.classList.add("destructive");
        serverMenu.items.append(remove);
      }
    }
    // Barra lateral (servidor, canais com as lives, sua transmissão, conta) e cabeçalho do canal.
    const workspace = root.querySelector<HTMLElement>(".server-workspace")!;
    workspace.querySelector(":scope > .server-members")?.remove();
    const members = content.querySelector<HTMLElement>(".server-members");
    if (members) workspace.append(members);
    const serverTitle = navigation.querySelector(".server-title-menu, .server-name");
    const channelMenu = sideActions.querySelector(".action-menu");
    if (channelMenu) {
      const summary = channelMenu.querySelector("summary")!;
      summary.textContent = "";
      summary.setAttribute("aria-label", "Opções do canal");
      summary.title = "Opções do canal";
      summary.append(svg(icon("settings", "sm")));
    }
    // Qualquer membro convida; listar e revogar convites continua no menu do servidor, para dono e admins.
    const inviteButton = server && channel ? button("Convidar pessoas", `Convidar pessoas para ${server.name}`, "invite", () => { void run(() => invite(server)); }) : null;
    inviteButton?.classList.add("side-invite");
    inviteButton?.prepend(svg(icon("userPlus", "sm")));
    const channels = el("div", { class: "side-channels" }, el("div", { class: "side-label" }, el("span", {}, "Canais"), channelMenu), channelList);
    const target = server && channel ? { serverId: server.id, serverName: server.name, channelId: channel.id, channelName: channel.name } : null;
    const dock = shareDock(target);
    const back = button(channel && !friendsHome ? `# ${channel.name}` : "Voltar", "Voltar para o canal", "back-channel", () => options.onOpenDirectory?.());
    back.classList.add("back-channel", "ghost");
    back.prepend(svg(icon("arrowLeft", "sm")));
    const openWatch = options.onOpenWatch ? button("Player", "Abrir as transmissões que você está assistindo", "open-watch", options.onOpenWatch) : null;
    openWatch?.classList.add("ghost");
    openWatch?.prepend(svg(icon("play", "sm")));
    const liveHere = channel?.live.length ?? 0;
    const memberCount = server?.members.length ?? 0;
    const title = channel ? el("div", { class: "topbar-title" },
      el("h1", {}, el("span", { class: "channel-hash", ariaHidden: "true" }, "#"), channel.name),
      el("p", {}, `${liveHere ? `${liveHere} ao vivo` : "Ninguém ao vivo"} · ${memberCount} ${memberCount === 1 ? "membro" : "membros"}`)) : null;
    topbar.replaceChildren(back, ...(title ? [title] : []), el("div", { class: "topbar-actions" }, openWatch, friendsHome ? null : memberToggle));
    navigation.replaceChildren(el("div", { class: "side-head" }, ...(serverTitle ? [serverTitle] : []), ...(inviteButton ? [inviteButton] : [])), channels, ...(dock ? [dock] : []), profile);
    content.classList.toggle("friends-content", friendsHome);
    if (friendsHome) {
      workspace.querySelector(":scope > .server-members")?.remove();
      navigation.replaceChildren(el("div", { class: "side-head" }, el("h1", { textContent: "Amigos" })), el("div", { class: "side-channels" }), ...(dock ? [dock] : []), profile);
      content.replaceChildren(friends.root);
      if (snapshot) friends.update(snapshot);
    }
    applyPage();
    queueMenuLayout();
    refreshWatchActions();
    for (const region of [serverRail, navigation, topbar, content]) region.setAttribute("aria-busy", String(busy));
    let focusRestored = false;
    if (focusKey && !busy) for (const node of root.querySelectorAll<HTMLElement>("[data-focuskey]")) if (node.dataset.focuskey === focusKey) {
      node.focus({ preventScroll: true });
      focusRestored = true;
      if (node instanceof HTMLInputElement && selection) node.setSelectionRange(...selection);
      break;
    }
    if (focusKey && !focusRestored && !busy) restoreFocus(focusKey);
  }

  function restoreFocus(key: string): void {
    const candidates = [...root.querySelectorAll<HTMLElement>("[data-focuskey]")];
    const available = (node: HTMLElement) => !node.closest("[hidden]") && !node.matches(":disabled") && node.getClientRects().length > 0;
    const target = candidates.find(node => node.dataset.focuskey === key && available(node))
      ?? candidates.find(node => node.matches('.channel-pick[aria-current="true"]') && available(node))
      ?? candidates.find(node => node.matches("summary") && available(node));
    target?.focus({ preventScroll: true });
  }

  function applyPage(): void {
    content.hidden = activePage !== "servers";
    pageHost.hidden = activePage === "servers";
    pageHost.classList.toggle("watching", activePage === "watch");
    root.dataset.page = activePage;
    const workspace = root.querySelector<HTMLElement>(".server-workspace")!;
    const members = workspace.querySelector<HTMLElement>(":scope > .server-members");
    // Sem servidor não há membros: a coluna não fica reservada vazia.
    const showMembers = !!members && (activePage === "watch" ? watchMembersExpanded : membersExpanded && (activePage === "servers" || activePage === "share"));
    workspace.classList.toggle("members-collapsed", !showMembers);
    // Na página do canal o cabeçalho só existe com canal; nas outras páginas ele oferece a volta.
    topbar.hidden = activePage === "servers" && (friendsHome || !topbar.querySelector(".topbar-title"));
    if (members) members.hidden = !showMembers;
    root.querySelector('[data-focuskey="toggle-members"]')?.setAttribute("aria-expanded", String(showMembers));
    root.querySelectorAll<HTMLElement>('[data-focuskey="share-callout"]').forEach(node => { node.hidden = sharing; });
    // Em Conta o destaque fica só no avatar: um destaque por região.
    for (const node of serverRail.querySelectorAll<HTMLElement>('[data-focuskey="friends-home"], .server-pick')) {
      node.setAttribute("aria-pressed", String(activePage !== "settings" && (node.dataset.focuskey === "friends-home" ? friendsHome : !friendsHome && node.dataset.focuskey === `server-${serverId}`)));
    }
    for (const [key, page] of [["open-watch", "watch"], ["open-share", "share"], ["account-settings", "settings"]]) {
      root.querySelector(`[data-focuskey="${key}"]`)?.setAttribute("aria-current", String(activePage === page));
    }
  }

  render();
  return {
    pageHost,
    setPage(page: string) { if (page === "watch" && activePage !== "watch") watchMembersExpanded = false; activePage = page; applyPage(); },
    setSharing(active: boolean, status?: ShareDockStatus) {
      const next = status ?? shareStatus;
      if (sharing === active && JSON.stringify(next) === JSON.stringify(shareStatus)) return;
      sharing = active; shareStatus = next; render();
    },
    root,
    show: refreshWatchActions,
    update(next: Snapshot) { if (disposed) return; snapshot = next; if (next.destination) { friendsHome = false; serverId = next.destination.serverId; channelId = next.destination.channelId; } render(); },
    select(target: InviteDestination) { friendsHome = false; serverId = target.serverId; channelId = target.channelId; remember(); render(); },
    setConnected(connected: boolean) { friends.setConnected(connected); connection.hidden = connected; connection.textContent = "Sem conexão. Tentando reconectar…"; },
    showError(error: unknown) {
      const retry = button("Tentar novamente", "Carregar servidores novamente", "retry-directory", () => { void run(() => client.servers()); });
      alerts.replaceChildren(banner("error", error instanceof Error ? error.message : "Não foi possível entrar no servidor."), retry);
    },
    dispose() { friends.dispose(); disposed = true; narrowMembers?.removeEventListener("change", onNarrowMembers); clearTimeout(joinTimer); menuObserver.disconnect(); cancelAnimationFrame(menuFrame); window.removeEventListener("resize", queueMenuLayout); window.removeEventListener("scroll", queueMenuLayout, true); root.removeEventListener("toggle", onMenuToggle, true); document.removeEventListener("pointerdown", closeMenus); root.removeEventListener("keydown", closeMenus); clearInterval(ageTimer); liveTimes.clear(); watchButtons.clear(); for (const dialog of dialogs) dialog.close(); dialogs.clear(); },
  };
}
