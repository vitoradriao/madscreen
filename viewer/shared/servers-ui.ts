import { clearInvite, inviteCodeFrom, type FriendView, type InviteDestination, type MadClient, type ServerView, type Snapshot } from "./client";
import { ago, banner, copyText, el, initials, svg } from "./dom";
import { avatar } from "./ui";
import { icon, type IconName } from "./icons";
import { videoTheme } from "./theme";

interface ServerOptions {
  onWatch(stream: FriendView): void;
  watchLabel?(username: string): string;
  onShare?(destination: InviteDestination): void;
  onSnapshot?(snapshot: Snapshot): void;
  onSettings?(): void;
  onOpenDirectory?(): void;
  onOpenWatch?(): void;
  onOpenShare?(): void;
}

export function createServers(client: MadClient, options: ServerOptions) {
  const storageKey = `ms.servers.${client.session?.user.id ?? "anonymous"}`;
  let serverId = "";
  let channelId = "";
  try { const saved = JSON.parse(localStorage.getItem(storageKey) ?? "{}"); serverId = saved.serverId ?? ""; channelId = saved.channelId ?? ""; } catch { /* Preferência corrompida. */ }
  let snapshot: Snapshot | null = null;
  let disposed = false;
  let busy = false;
  let operationFocusKey: string | undefined;
  let joinValue = "";
  let membersExpanded = true;
  let watchMembersExpanded = false;
  let joinExpanded = false;
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
  const serverRail = el("aside", { class: "directory-rail" });
  const root = el("section", { class: "stack server-view" }, connection, alerts, operationStatus, el("div", { class: "server-workspace" }, serverRail, navigation, content, pageHost));
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

  function ask(title: string, initial?: string, detail?: string): Promise<string | null> {
    return new Promise(resolve => {
      const openerKey = document.activeElement instanceof HTMLElement ? document.activeElement.dataset.focuskey : undefined;
      const dialog = el("dialog", { class: "server-dialog" });
      const input = initial === undefined ? null : el("input", { class: "input", value: initial, required: true, maxLength: 40 });
      const captionId = `server-dialog-${crypto.randomUUID()}`;
      const caption = el("h2", { id: captionId, textContent: title });
      dialog.setAttribute("aria-labelledby", captionId);
      const description = detail ? el("p", { id: `${captionId}-detail`, class: "small", textContent: detail }) : null;
      if (description) dialog.setAttribute("aria-describedby", description.id);
      if (input) input.setAttribute("aria-label", "Nome");
      const validation = el("p", { id: `${captionId}-validation`, class: "small", role: "status" });
      if (input) {
        input.setAttribute("aria-describedby", validation.id);
        input.addEventListener("input", () => { input.setCustomValidity(""); input.removeAttribute("aria-invalid"); validation.textContent = ""; });
      }
      const cancel = el("button", { class: "btn", type: "button", textContent: "Cancelar" });
      const verb = title.replace(/\?$/, "");
      const save = el("button", { class: "btn primary", type: "submit", textContent: verb.startsWith("Renomear") ? "Salvar nome" : verb });
      let result: string | null = null;
      const form = el("form", { class: "stack" }, caption, description, input, input ? validation : null, el("div", { class: "row" }, cancel, save));
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
        if (next.destination) { serverId = next.destination.serverId; channelId = next.destination.channelId; remember(); }
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

  async function invite(server: ServerView): Promise<void> {
    const openerKey = operationFocusKey;
    const invite = await client.createInvite(server.id, channelId);
    if (disposed) return;
    const input = el("input", { class: "input", value: invite.link, readOnly: true });
    input.setAttribute("aria-label", "Link de convite do servidor");
    const copyStatus = el("p", { class: "small", role: "status" });
    const copy = button("Copiar convite", "Copiar link de convite", "copy-invite", () => {
      void copyText(invite.link).then(copied => {
        if (!dialog.open) return;
        copyStatus.textContent = copied ? "Convite copiado." : "Não foi possível copiar. O link foi selecionado; use Ctrl+C para copiar.";
        if (!copied) { input.focus(); input.select(); }
      });
    });
    copy.disabled = false;
    const dialog = el("dialog", { class: "server-dialog" });
    const titleId = `invite-dialog-${crypto.randomUUID()}`;
    dialog.setAttribute("aria-labelledby", titleId);
    const close = button("Fechar", "Fechar convite", "close-invite", () => dialog.close());
    close.disabled = false;
    copy.classList.add("primary");
    dialog.append(el("div", { class: "stack" },
      el("h2", { id: titleId, textContent: "Convidar amigos" }),
      input, copyStatus, el("div", { class: "row" }, copy, close)));
    const dismissTimer = setTimeout(() => { if (dialog.open) dialog.close(); }, 8_000);
    dialog.addEventListener("close", () => {
      clearTimeout(dismissTimer);
      dialogs.delete(dialog);
      dialog.remove();
      if (!disposed && openerKey) restoreFocus(openerKey);
    }, { once: true });
    dialogs.add(dialog);
    document.body.append(dialog);
    dialog.showModal();
    copy.focus();
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
    rail.setAttribute("aria-label", "Escolher servidor");
    for (const item of servers) {
      const pick = button("", item.name, `server-${item.id}`, () => { serverId = item.id; channelId = ""; render(); options.onOpenDirectory?.(); });
      pick.append(initials(item.name));
      pick.classList.add("server-pick");
      const liveCount = item.channels.reduce((total, channel) => total + channel.live.length, 0);
      if (liveCount && item.id !== serverId) pick.append(el("span", { class: "server-live-dot", ariaLabel: `${liveCount} transmissões em outro servidor` }));
      pick.setAttribute("aria-pressed", String(item.id === serverId));
      rail.append(pick);
    }
    const addServer = actionMenu("+", "add-server");
    addServer.menu.classList.add("server-add-menu");
    const addSummary = addServer.menu.querySelector("summary")!;
    addSummary.setAttribute("aria-label", "Adicionar servidor");
    addServer.items.append(menuButton("Criar servidor", "Criar servidor", "server-create", () => {
      void ask("Criar servidor", "").then(name => { if (name) void run(() => client.createServer(name)); });
    }, "plus"));
    rail.append(addServer.menu);
    serverRail.replaceChildren(el("div", { class: "directory-brand", ariaLabel: "madscreen" }, svg(icon("logo"))), rail);
    const joinInput = el("input", { class: "input", placeholder: "Código ou link do convite", maxLength: 512, disabled: busy, value: joinValue });
    joinInput.addEventListener("input", () => { joinValue = joinInput.value; });
    joinInput.setAttribute("aria-label", "Entrar em um servidor por convite");
    joinInput.dataset.focuskey = "join-code";
    const join = el("button", { class: "btn primary sm", type: "submit", textContent: "Entrar", disabled: busy });
    const joinForm = el("form", { class: "stack tight" }, joinInput, join);
    joinForm.addEventListener("submit", event => {
      event.preventDefault();
      const value = joinInput.value;
      void run(async () => { const next = await client.redeemInvite(inviteCodeFrom(value, client.base)); joinValue = ""; clearInvite(); return next; });
    });
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
    }
    const joinSection = el("details", { class: "server-join", open: !servers.length || joinExpanded },
      el("summary", { class: "menu-item" }, svg(icon("link", "sm")), "Entrar por convite", svg(icon("chevronDown", "sm"))), joinForm);
    joinSection.addEventListener("toggle", () => { if (joinSection.isConnected) joinExpanded = joinSection.open; });
    addServer.items.append(joinSection);
    const sideActions = el("div", { class: "channel-side-actions stack tight" });
    const user = client.session?.user;
    const profile = el("div", { class: "channel-profile" }, user ? avatar(user, true) : null,
      el("div", { class: "grow" }, el("strong", {}, user?.displayName ?? "Conta"), el("p", { class: "small muted" }, "Conectado")));
    if (options.onSettings) { const settings = button("", "Abrir configurações da conta", "account-settings", options.onSettings); settings.append(svg(icon("settings"))); profile.append(settings); }
    if (options.onOpenWatch) sideActions.append(button("Assistir", "Abrir transmissões em reprodução", "open-watch", options.onOpenWatch));
    if (options.onOpenShare) sideActions.append(button("Minha transmissão", "Abrir controles e diagnóstico da transmissão", "open-share", options.onOpenShare));
    navigation.replaceChildren(el("h2", { class: "server-name", textContent: server?.name ?? "Seus servidores" }), el("span", { class: "eyebrow" }, "Seus canais"), channelList, el("span", { class: "eyebrow" }, "Servidor"), sideActions, profile);
    content.replaceChildren();
    liveTimes.clear();
    watchButtons.clear();
    if (snapshot && !snapshot.servers) {
      content.append(banner("info", "O servidor de sinalização precisa ser atualizado para oferecer servidores e canais."));
    } else if (!snapshot) {
      const skeleton = el("div", { class: "channel-main stack", role: "status", ariaLabel: "Carregando servidores" },
        el("p", { class: "muted" }, "Carregando servidores…"), el("div", { class: "server-streams" }, el("div", { class: "skel skel-tile" }), el("div", { class: "skel skel-tile" })));
      content.append(skeleton);
    } else if (!server || !channel) {
      content.append(el("div", { class: "empty-main" }, svg(icon("users")), el("h2", {}, "Nenhum servidor"), el("p", { class: "small", textContent: "Use o botão + para criar um servidor ou entrar por convite." })));
    } else {
      const target: InviteDestination = { serverId: server.id, serverName: server.name, channelId: channel.id, channelName: channel.name };
      const actions = el("div", { class: "row channel-actions" });
      const makeShare = (key: string) => {
        const share = button("Compartilhar tela", `Compartilhar tela em ${channel.name}`, key, () => sharing ? options.onOpenShare?.() : options.onShare?.(target));
        share.classList.add("primary", "channel-share");
        share.prepend(svg(icon("screenShare")));
        return share;
      };
      if (options.onShare) {
        actions.append(makeShare("share-here"));
      }
      if (server.role !== "member") {
        actions.prepend(button("Convidar", "Criar convite deste canal", "invite", () => { void run(() => invite(server)); }));
        const sideInvite = button("Convidar amigos", "Criar convite deste canal", "side-invite", () => { void run(() => invite(server)); }); sideInvite.prepend(svg(icon("userPlus", "sm"))); sideActions.prepend(sideInvite);
        const channelMenu = actionMenu("Opções do canal", `channel-${channel.id}`);
        sideActions.append(channelMenu.menu);
        channelMenu.items.append(menuButton("Novo canal", "Criar canal", "channel-create", () => {
          void ask("Criar canal", "").then(name => { if (name) void run(() => client.createChannel(server.id, name)); });
        }, "plus"));
        channelMenu.items.append(menuButton("Renomear", "Renomear canal", "channel-rename", () => {
          void ask("Renomear canal", channel.name).then(name => { if (name) void run(() => client.renameChannel(server.id, channel.id, name)); });
        }));
        if (server.channels.length > 1) {
          const remove = menuButton("Apagar canal", "Apagar canal e encerrar suas transmissões", "channel-remove", () => {
            void ask("Apagar canal?", undefined, `O canal ${channel.name} será removido e suas transmissões serão encerradas.`).then(ok => { if (ok) void run(() => client.removeChannel(server.id, channel.id)); });
          });
          remove.classList.add("destructive");
          channelMenu.items.append(el("hr", { class: "menu-sep" }), remove);
        }
      }
      content.append(el("header", { class: "channel-header" }, el("h1", {}, el("span", { class: "channel-hash", ariaHidden: "true" }, "#"), channel.name), actions));
      const streams = el("div", { class: "server-streams" });
      for (const stream of channel.live) {
        const watch = button("Assistir", `Assistir à transmissão de ${stream.displayName}`, `watch-${stream.id}`, () => options.onWatch(stream));
        watchButtons.set(watch, stream);
        const age = el("span", { textContent: stream.live ? ago(stream.live.since) : "" });
        if (stream.live) liveTimes.set(age, stream.live.since);
        const tile = el("article", { class: "stream-card tile" },
          el("div", { class: "noframe stream-cover", ariaHidden: "true" }, svg(icon("monitor"))),
          el("span", { class: "tl badge live", textContent: "Ao vivo" }),
          el("span", { class: "tr pill", ariaLabel: `${stream.live?.viewers ?? 0} espectadores` }, svg(icon("users", "sm")), String(stream.live?.viewers ?? 0)),
          el("div", { class: "info" }, avatar(stream, true),
            el("div", { class: "who" }, el("b", { textContent: stream.displayName }), el("span", { textContent: stream.live?.title || "Tela compartilhada" }), age), watch));
        tile.dataset.theme = videoTheme();
        streams.append(tile);
      }
      if (!channel.live.length) streams.append(el("div", { class: "empty-main" }, svg(icon("monitor")), el("h2", {}, `Nenhuma transmissão em #${channel.name}`), el("p", { class: "small", textContent: "Selecione uma janela ou tela para compartilhar." }), options.onShare ? makeShare("empty-share") : null));
      const members = el("aside", { class: "server-members", hidden: !membersExpanded, ariaLabel: "Membros do servidor" });
      const toggleMembers = () => {
        if (activePage === "watch") watchMembersExpanded = !watchMembersExpanded;
        else membersExpanded = !membersExpanded;
        applyPage();
      };
      const memberToggle = button("", "Mostrar ou ocultar membros", "toggle-members", toggleMembers); memberToggle.append(svg(icon("users"))); memberToggle.setAttribute("aria-expanded", String(membersExpanded));
      actions.insertBefore(memberToggle, actions.querySelector('.channel-share'));
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
        if (server.role === "owner" && person.role !== "owner") {
          const permissions = actionMenu("⋯", `member-${person.id}`);
          permissions.menu.querySelector("summary")!.setAttribute("aria-label", `Opções de ${person.displayName}`);
          permissions.items.append(menuButton(person.role === "admin" ? "Remover admin" : "Tornar admin", `Alterar permissão de ${person.displayName}`, `role-${person.id}`, () => { void run(() => client.setMemberRole(server.id, person.id, person.role === "admin" ? "member" : "admin")); }, "key"));
          row.append(permissions.menu);
        }
        roster.append(row);
      }
      members.append(roster);
      const body = el("div", { class: `channel-body${membersExpanded ? "" : " members-collapsed"}` },
        el("div", { class: "channel-main stack" }, el("span", { class: "eyebrow" }, `${server.name} / ${channel.name}`), el("h2", { class: "channel-section-title" }, "Transmissões"), streams), members);
      content.append(body);
      if (options.onShare && channel.live.length) {
        const callout = button("", `Compartilhar tela em ${channel.name}`, "share-callout", () => options.onShare?.(target));
        callout.className = "callout";
        callout.append(svg(icon("screenShare")), "Sua tela não está no ar · ", el("b", {}, "Compartilhar tela"));
        body.querySelector(".channel-main")?.append(callout);
      }
      const serverMenu = actionMenu(server.name, `server-${server.id}`);
      serverMenu.menu.classList.add("server-title-menu");
      const serverSummary = serverMenu.menu.querySelector("summary")!;
      serverSummary.setAttribute("aria-label", "Opções do servidor");
      serverSummary.title = server.name;
      serverSummary.replaceChildren(el("span", { class: "server-title", textContent: server.name }), svg(icon("chevronDown", "sm")));
      navigation.querySelector('.server-name')?.replaceWith(serverMenu.menu);
      const leaveServer = menuButton("Sair do servidor", "Sair do servidor", "server-leave", () => {
        void ask("Sair do servidor?", undefined, server.role === "owner" ? "Outro membro assumirá como dono. Se não houver outros membros, o servidor será apagado." : "Você precisará de um novo convite para voltar.").then(ok => { if (ok) void run(() => client.leaveServer(server.id)); });
      }, "logout");
      if (server.role === "owner") {
        serverMenu.items.append(menuButton("Renomear servidor", "Renomear servidor", "server-rename", () => { void ask("Renomear servidor", server.name).then(name => { if (name) void run(() => client.renameServer(server.id, name)); }); }));
      }
      if (server.role === "owner") serverMenu.items.append(el("hr", { class: "menu-sep" }));
      serverMenu.items.append(leaveServer);
      if (server.role === "owner") {
        const remove = menuButton("Apagar servidor", "Apagar servidor", "server-remove", () => { void ask("Apagar servidor?", undefined, "Todos os canais, convites e transmissões deste servidor serão encerrados.").then(ok => { if (ok) void run(() => client.removeServer(server.id)); }); });
        remove.classList.add("destructive");
        serverMenu.items.append(remove);
      }
    }
    // Os mesmos controles mudam de lugar; handlers e regras de permissão permanecem.
    const workspace = root.querySelector<HTMLElement>(".server-workspace")!;
    workspace.querySelector(":scope > .server-members")?.remove();
    const members = content.querySelector<HTMLElement>(".server-members");
    if (members) workspace.append(members);
    const header = content.querySelector(".channel-header");
    const actions = header?.querySelector(".channel-actions");
    const inviteAction = actions?.querySelector('[data-focuskey="invite"]');
    inviteAction?.classList.add("ghost");
    inviteAction?.prepend(svg(icon("userPlus", "sm")));
    actions?.querySelector('[data-focuskey="toggle-members"]')?.classList.add("ghost");
    const serverTitle = navigation.querySelector(".server-title-menu, .server-name");
    const channelMenu = sideActions.querySelector(".action-menu");
    if (channelMenu) {
      const summary = channelMenu.querySelector("summary")!;
      summary.textContent = "";
      summary.setAttribute("aria-label", "Opções do canal");
      summary.append(svg(icon("settings", "sm")));
    }
    const shortcuts = el("div", { class: "directory-shortcuts" });
    for (const key of ["open-watch", "open-share"]) {
      const node = sideActions.querySelector(`[data-focuskey="${key}"]`);
      if (key === "open-watch") node?.prepend(svg(icon("play", "sm")));
      if (node) shortcuts.append(node);
    }
    const back = button("Voltar para o canal", "Voltar para o canal", "back-channel", () => options.onOpenDirectory?.());
    back.classList.add("back-channel", "ghost");
    navigation.replaceChildren(...(serverTitle ? [serverTitle] : []), channelList, ...(channelMenu ? [channelMenu] : []), back, shortcuts, ...(actions ? [actions] : []));
    navigation.classList.add("topbar");
    header?.remove();
    const main = content.querySelector(".channel-main");
    const sectionTitle = main?.querySelector(".channel-section-title");
    if (sectionTitle && channel) {
      sectionTitle.replaceWith(el("div", { class: "view-head channel-heading" }, el("h1", {}, "Ao vivo agora"), el("p", { class: "muted" }, `${channel.live.length} transmissões em # ${channel.name}`)));
      main?.querySelector(":scope > .eyebrow")?.remove();
    }
    const accountButton = button("", "Abrir configurações da conta", "account-settings", () => options.onSettings?.());
    accountButton.className = "rail-account";
    if (user) accountButton.append(avatar(user, true));
    serverRail.append(accountButton);
    serverRail.querySelector(".directory-brand")?.replaceChildren("m.");
    applyPage();
    queueMenuLayout();
    refreshWatchActions();
    for (const region of [serverRail, navigation, content]) region.setAttribute("aria-busy", String(busy));
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
    const showMembers = activePage === "watch" ? watchMembersExpanded : membersExpanded && (activePage === "servers" || activePage === "share");
    workspace.classList.toggle("members-collapsed", !showMembers);
    const members = workspace.querySelector<HTMLElement>(":scope > .server-members");
    if (members) members.hidden = !showMembers;
    root.querySelector('[data-focuskey="toggle-members"]')?.setAttribute("aria-expanded", String(showMembers));
    root.querySelectorAll<HTMLElement>('[data-focuskey="share-here"]').forEach(node => {
      node.textContent = sharing ? "Minha transmissão" : "Compartilhar tela";
      if (!sharing) node.prepend(svg(icon("screenShare")));
    });
    root.querySelectorAll<HTMLElement>('[data-focuskey="share-callout"]').forEach(node => { node.hidden = sharing; });
    const account = serverRail.querySelector('[data-focuskey="account-settings"]');
    account?.setAttribute("aria-current", String(activePage === "settings"));
    for (const [key, page] of [["open-watch", "watch"], ["open-share", "share"], ["account-settings", "settings"]]) {
      navigation.querySelector(`[data-focuskey="${key}"]`)?.setAttribute("aria-current", String(activePage === page));
    }
  }

  render();
  return {
    pageHost,
    setPage(page: string) { if (page === "watch" && activePage !== "watch") watchMembersExpanded = false; activePage = page; applyPage(); },
    setSharing(active: boolean) { if (sharing !== active) { sharing = active; applyPage(); } },
    root,
    show: refreshWatchActions,
    update(next: Snapshot) { if (disposed) return; snapshot = next; if (next.destination) { serverId = next.destination.serverId; channelId = next.destination.channelId; } render(); },
    select(target: InviteDestination) { serverId = target.serverId; channelId = target.channelId; remember(); render(); },
    setConnected(connected: boolean) { connection.hidden = connected; connection.textContent = "Sem conexão. Tentando reconectar…"; },
    showError(error: unknown) {
      const retry = button("Tentar novamente", "Carregar servidores novamente", "retry-directory", () => { void run(() => client.servers()); });
      alerts.replaceChildren(banner("error", error instanceof Error ? error.message : "Não foi possível entrar no servidor."), retry);
    },
    dispose() { disposed = true; menuObserver.disconnect(); cancelAnimationFrame(menuFrame); window.removeEventListener("resize", queueMenuLayout); window.removeEventListener("scroll", queueMenuLayout, true); root.removeEventListener("toggle", onMenuToggle, true); document.removeEventListener("pointerdown", closeMenus); root.removeEventListener("keydown", closeMenus); clearInterval(ageTimer); liveTimes.clear(); watchButtons.clear(); for (const dialog of dialogs) dialog.close(); dialogs.clear(); },
  };
}
