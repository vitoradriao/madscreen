// Componentes de interface compartilhados (app Tauri e viewer web): entrar, amigos e assistir.
import { ApiFailure, clearPkce, savedPkce, pendingInvite, rememberInvite, WatchSession, type FriendView, type MadClient, type Snapshot, type UserView, type WatchState, type WatchStats, type VideoQuality } from "./client";
import { ago, banner, copyText, el, initials, segmented, svg } from "./dom";
import { icon } from "./icons";
import { diagnosticGuidance } from "./diagnostic-guidance";
import { videoTheme } from "./theme";
import { confirmInvite, createInviteManager } from "./invite-ui";

const errorText = (error: unknown): string => (error instanceof Error ? error.message : String(error));

export function avatar(user: UserView, online?: boolean): HTMLElement {
  const root = el("div", { class: "avatar" });
  if (user.avatar) {
    const image = el("img", { src: user.avatar, alt: "", referrerPolicy: "no-referrer", loading: "lazy", draggable: false });
    image.addEventListener("error", () => image.replaceWith(initials(user.displayName)), { once: true });
    root.append(image);
  } else {
    root.append(initials(user.displayName));
  }
  if (online !== undefined) root.append(el("span", { class: `presence${online ? " on" : ""}`, title: online ? "Online" : "Offline" }));
  return root;
}

let fieldId = 0;

function nextFieldId(): string {
  let id: string;
  do id = `ms-field-${++fieldId}`;
  while (document.getElementById(id));
  return id;
}

function field(label: string, input: HTMLElement): HTMLElement {
  if (!input.id) input.id = nextFieldId();
  const caption = el("label", { textContent: label });
  caption.htmlFor = input.id;
  return el("div", { class: "field" }, caption, input);
}

// ---- Entrar -----------------------------------------------------------------------------------

export interface LoginOptions {
  /** Abre a URL no navegador do sistema (app) ou navega (web). `false` indica que não abriu. */
  openUrl(url: string): void | boolean | Promise<boolean | void>;
  onLoggedIn(recoveryKey?: string): void;
  invite?: string | undefined;
  signal?: AbortSignal;
  onInviteDestination?(server: string | null, channel: string | null, inviter?: string | null): void;
}

function describe(input: HTMLElement, hint: HTMLElement): void {
  if (!hint.id) hint.id = nextFieldId();
  input.setAttribute("aria-describedby", hint.id);
}

export function createLogin(client: MadClient, options: LoginOptions): HTMLElement {
  const alerts = el("div", { class: "stack tight" });
  const say = (kind: "error" | "info" | "ok", text: string): void => alerts.replaceChildren(banner(kind, text));
  let abort: AbortController | null = null;
  options.signal?.addEventListener("abort", () => abort?.abort(), { once: true });

  const discord = el("button", { class: "btn primary big block", type: "button", textContent: "Continuar com Discord" });
  const google = el("button", { class: "btn big block", type: "button", hidden: true, textContent: "Continuar com Google" });
  const why = el("p", { class: "small muted auth-why", textContent: "O Discord só confirma quem você é. A transmissão é do madscreen, sem usar o compartilhamento de tela do Discord." });
  const hint = el("p", { class: "small faint", hidden: true });

  async function oauth(provider: "discord" | "google"): Promise<void> {
    if (options.signal?.aborted) return;
    abort?.abort();
    const controller = new AbortController();
    abort = controller;
    const { url, verifier } = await client.beginOAuth(provider, code.value);
    if (controller.signal.aborted || abort !== controller) return;
    localStorage.setItem("ms.pkce", verifier);
    await resume(verifier, controller, url);
  }

  async function resume(verifier: string, controller = new AbortController(), url?: string): Promise<void> {
    if (options.signal?.aborted) return;
    if (abort !== controller) abort?.abort();
    abort = controller;
    const cancel = el("button", { class: "btn ghost sm", type: "button", textContent: "Cancelar" });
    cancel.addEventListener("click", () => {
      if (abort !== controller) return;
      controller.abort();
      clearPkce();
      alerts.replaceChildren();
    });
    const message = el("span", { class: "grow", textContent: "Autorize o acesso na janela do navegador que abriu…" });
    const actions = el("div", { class: "row oauth-wait-actions" });
    if (url) {
      const open = el("button", { class: "btn ghost sm", type: "button" }, svg(icon("link", "sm")), "Abrir no navegador");
      const copy = el("button", { class: "btn ghost sm", type: "button" }, svg(icon("copy", "sm")), "Copiar link");
      let copyTimer = 0;
      const launch = async (): Promise<void> => {
        let opened: boolean | void = undefined;
        try { opened = await options.openUrl(url); } catch { opened = false; }
        if (abort !== controller) return;
        message.textContent = opened === false
          ? "Não foi possível abrir o navegador. Copie o link e abra no navegador para autorizar."
          : "Autorize o acesso na janela do navegador que abriu…";
      };
      open.addEventListener("click", () => { void launch(); });
      copy.addEventListener("click", () => {
        void copyText(url).then((ok) => {
          copy.replaceChildren(svg(icon(ok ? "check" : "copy", "sm")), ok ? "Copiado" : "Não foi possível copiar");
          window.clearTimeout(copyTimer);
          copyTimer = window.setTimeout(() => copy.replaceChildren(svg(icon("copy", "sm")), "Copiar link"), 2000);
        });
      });
      actions.append(open, copy);
      void launch();
    }
    actions.append(cancel);
    alerts.replaceChildren(el("div", { class: "banner info oauth-wait", role: "status" }, message, actions));
    try {
      await client.awaitOAuth(verifier, controller.signal);
      if (controller.signal.aborted || options.signal?.aborted || abort !== controller) return;
      clearPkce();
      options.onLoggedIn();
    } catch (error) {
      if (!controller.signal.aborted && abort === controller) say("error", errorText(error));
    }
  }

  for (const [button, provider] of [[discord, "discord"], [google, "google"]] as const) {
    button.addEventListener("click", () => { void oauth(provider).catch((error: unknown) => say("error", errorText(error))); });
  }

  const username = el("input", { class: "input", placeholder: "seunome", autocomplete: "username", maxLength: 20, spellcheck: false });
  const usernameRule = el("p", { class: "small muted", textContent: "De 3 a 20 letras minúsculas, números, ponto ou sublinhado. É o nome que seus amigos vão ver." });
  describe(username, usernameRule);
  const code = el("input", { class: "input", placeholder: "XXXX-XXXX ou link do convite", autocomplete: "off", maxLength: 512, spellcheck: false, value: options.invite ?? pendingInvite() ?? "" });
  const create = el("button", { class: "btn primary block", type: "submit", textContent: "Criar conta" });
  const codeField = field("Convite de um amigo (opcional)", code);
  const idleDestination = "Cole um convite pessoal para adicionar um amigo ou um convite de servidor para participar dele.";
  const destination = el("p", { class: "small muted", role: "status" });
  describe(code, destination);
  const inviteBlock = el("div", { class: "auth-invite stack tight" }, codeField, destination);
  let previewVersion = 0;
  let previewTimer = 0;
  async function previewDestination(version = ++previewVersion): Promise<void> {
    const value = code.value.trim();
    options.onInviteDestination?.(null, null, null);
    destination.textContent = value ? "Conferindo o convite…" : idleDestination;
    if (!value) return;
    try {
      const preview = await client.previewInvite(value);
      if (version !== previewVersion || options.signal?.aborted) return;
      options.onInviteDestination?.(preview.server?.name ?? null, preview.channel?.name ?? null, preview.inviter.displayName);
      destination.textContent = preview.server
        ? `Convite de ${preview.inviter.displayName} para ${preview.server.name}${preview.channel ? ` · # ${preview.channel.name}` : ""}. Você entra no servidor logo depois de acessar.`
        : `Convite de ${preview.inviter.displayName}. Vocês ficam conectados logo depois de você acessar.`;
    } catch (error) {
      if (version === previewVersion && !options.signal?.aborted) destination.textContent = `${errorText(error)} Confira o convite ou peça um novo link a quem convidou.`;
    }
  }
  // A prévia sai enquanto a pessoa cola ou digita, sem esperar o campo perder o foco.
  code.addEventListener("input", () => {
    const version = ++previewVersion;
    clearTimeout(previewTimer);
    options.onInviteDestination?.(null, null, null);
    destination.textContent = code.value.trim() ? "Conferindo o convite…" : idleDestination;
    previewTimer = window.setTimeout(() => { void previewDestination(version); }, 400);
  });
  options.signal?.addEventListener("abort", () => clearTimeout(previewTimer), { once: true });
  void previewDestination();
  const registerNote = el("p", { class: "small muted", textContent: "Sem Discord, a conta é criada com o convite de um amigo. Você recebe uma chave para entrar de novo depois." });
  const registerForm = el("form", { class: "stack", noValidate: true }, registerNote, field("Nome de usuário", username), usernameRule, create);

  const loginUser = el("input", { class: "input", placeholder: "seunome", autocomplete: "username", spellcheck: false });
  const recovery = el("input", { class: "input mono", placeholder: "XXXXX-XXXXX-XXXXX-XXXXX", autocomplete: "current-password", spellcheck: false, type: "password" });
  const lostKey = el("p", { class: "small muted", textContent: "Perdeu a chave? Ela não pode ser recuperada. Se você ainda está conectado em outro computador, gere uma nova em Conta. Se não, peça um convite novo a um amigo e crie outra conta." });
  describe(recovery, lostKey);
  const enter = el("button", { class: "btn primary block", type: "submit", textContent: "Entrar" });
  const loginForm = el("form", { class: "stack", noValidate: true, hidden: true }, field("Nome de usuário", loginUser), field("Chave de recuperação", recovery), lostKey, enter);

  const mode = segmented<"invite" | "account">(
    [{ value: "invite", label: "Criar conta com convite" }, { value: "account", label: "Entrar com chave" }],
    "invite",
    (value) => {
      registerForm.hidden = value !== "invite";
      loginForm.hidden = value !== "account";
      alerts.replaceChildren();
    },
  );
  mode.root.setAttribute("aria-label", "Como entrar sem Discord");
  const fallback = el("details", { class: "auth-fallback" }, el("summary", { textContent: "Entrar sem Discord" }), el("div", { class: "stack mt" }, mode.root, registerForm, loginForm));
  const oauthGroup = el("div", { class: "stack tight auth-oauth" }, discord, google, why, hint);
  // A ordem do DOM acompanha a ordem visual: o Tab percorre a tela de cima para baixo.
  let firstUser = false;
  const arrange = (): void => {
    // Sem Discord o convite é obrigatório (exceto para a primeira conta); com Discord ele é opcional.
    codeField.querySelector("label")!.textContent = firstUser ? "Código de convite (opcional: você é o primeiro)"
      : fallback.open ? "Convite de um amigo" : "Convite de um amigo (opcional)";
    root.classList.toggle("alternative", fallback.open);
    discord.classList.toggle("primary", !fallback.open);
    why.hidden = fallback.open;
    if (fallback.open) {
      registerForm.insertBefore(inviteBlock, registerNote.nextSibling);
      root.replaceChildren(heading, fallback, oauthGroup, alerts);
    } else {
      root.replaceChildren(heading, inviteBlock, oauthGroup, fallback, alerts);
    }
  };
  fallback.addEventListener("toggle", arrange);

  registerForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (code.value.trim()) {
      try { if (!await confirmInvite(await client.previewInvite(code.value))) return; }
      catch (error) { say("error", errorText(error)); return; }
    }
    if (options.signal?.aborted) return;
    abort?.abort();
    clearPkce();
    create.disabled = true;
    const operation = new AbortController();
    abort = operation;
    client.register(username.value, code.value, undefined, operation.signal)
      .then((result) => { if (!options.signal?.aborted) options.onLoggedIn(result.recoveryKey); })
      .catch((error: unknown) => say("error", errorText(error)))
      .finally(() => { create.disabled = false; });
  });
  loginForm.addEventListener("submit", (event) => {
    event.preventDefault();
    abort?.abort();
    clearPkce();
    enter.disabled = true;
    try { rememberInvite(code.value, client.base); } catch (error) { say("error", errorText(error)); enter.disabled = false; return; }
    const operation = new AbortController();
    abort = operation;
    client.login(loginUser.value, recovery.value, operation.signal)
      .then(() => { if (!options.signal?.aborted) options.onLoggedIn(); })
      .catch((error: unknown) => say("error", errorText(error)))
      .finally(() => { enter.disabled = false; });
  });

  client.providers().then((providers) => {
    discord.disabled = !providers.discord;
    google.hidden = !providers.google;
    google.disabled = !providers.google;
    fallback.open = !providers.discord;
    if (!providers.discord) {
      hint.hidden = false;
      hint.textContent = !providers.discord && !providers.google
        ? "O login com Discord e Google ainda não foi configurado neste servidor. Use um convite de amigo abaixo."
        : `O login com ${providers.discord ? "Google" : "Discord"} ainda não foi configurado.`;
    }
    if (providers.firstUser) {
      firstUser = true;
      codeField.querySelector("label")!.textContent = "Código de convite (opcional: você é o primeiro)";
      registerNote.textContent = "Você é a primeira pessoa deste servidor do madscreen, então o convite não é necessário. Você recebe uma chave para entrar de novo depois.";
    }
  }).catch(() => { fallback.open = true; say("error", "Não foi possível consultar as opções de entrada. Confira sua conexão e tente entrar novamente."); });

  const pending = savedPkce();
  if (pending) queueMicrotask(() => void resume(pending));

  const heading = el("div", { class: "stack tight" }, el("h2", {}, options.invite ? "Convite recebido" : "Entrar"));
  const root = el("div", { class: "stack loose fade-in auth-flow" });
  arrange();
  return root;
}

/** Mostra a chave de recuperação uma única vez, depois do cadastro por convite ou da troca em Conta. */
export function createRecoveryNotice(key: string, done: () => void, options: { title?: string; doneLabel?: string } = {}): HTMLElement {
  const copy = el("button", { class: "btn", type: "button" }, svg(icon("copy", "sm")), "Copiar chave");
  copy.addEventListener("click", () => { void copyText(key).then((ok) => { copy.lastChild!.textContent = ok ? "Chave copiada" : "Selecione a chave e copie"; }); });
  const ok = el("button", { class: "btn primary", type: "button", textContent: options.doneLabel ?? "Continuar", disabled: true });
  ok.addEventListener("click", done);
  const saved = el("input", { type: "checkbox" });
  saved.addEventListener("change", () => { ok.disabled = !saved.checked; });
  const heading = el("h2", { tabIndex: -1, textContent: options.title ?? "Guarde sua chave de recuperação" });
  const box = el("div", { class: "keybox mono recovery-key", textContent: key });
  box.setAttribute("aria-label", `Chave de recuperação: ${key}`);
  const root = el(
    "div",
    { class: "stack loose fade-in recovery-notice" },
    heading,
    el("p", { class: "muted", textContent: "Ela funciona como a sua senha: sem ela, você não entra de novo se sair da conta ou trocar de computador. A chave não aparece outra vez, mas você pode gerar uma nova em Conta enquanto estiver conectado." }),
    box,
    el("div", { class: "row" }, copy),
    el("label", { class: "row check" }, saved, "Guardei a chave em um lugar seguro"),
    el("div", { class: "row" }, ok),
  );
  // Quem usa leitor de tela precisa saber que a chave apareceu: o foco vai para o título.
  setTimeout(() => { if (root.isConnected) heading.focus(); }, 0);
  return root;
}

// ---- Amigos ------------------------------------------------------------------------------------

export interface FriendsPanel {
  root: HTMLElement;
  update(snapshot: Snapshot): void;
  setConnected(connected: boolean): void;
  dispose(): void;
}

export function createFriends(client: MadClient, handlers: { onWatch(friend: FriendView): void; onShare?(friend: FriendView): void; onSnapshot?(snapshot: Snapshot): void }): FriendsPanel {
  const alerts = el("div", { class: "stack tight" });
  const say = (kind: "error" | "info" | "ok", text: string): void => alerts.replaceChildren(banner(kind, text));
  const lists = el("div", { class: "stack loose" });
  const status = el("span", { class: "small faint friends-status", role: "status", textContent: "Conectando…" });
  let current: Snapshot | null = null;
  let fingerprint = "";
  let disposed = false;
  let ageTimer: ReturnType<typeof setInterval> | undefined;
  const liveTimes = new Map<HTMLElement, number>();
  let selectedId = "";
  let filter = "all";
  const search = el("input", { class: "input", type: "search", placeholder: "Buscar amigo", ariaLabel: "Buscar amigo" });
  search.addEventListener("input", () => { fingerprint = ""; if (current) update(current); });
  const filters = segmented<"all" | "online" | "pending">([{ value: "all", label: "Todos" }, { value: "online", label: "Online" }, { value: "pending", label: "Pendentes" }], "all", value => { filter = value; fingerprint = ""; if (current) update(current); });
  const profile = el("section", { class: "card stack", ariaLabel: "Amigo selecionado" });
  const apply = (next: Snapshot): void => { update(next); handlers.onSnapshot?.(next); };

  function refreshAges(): void {
    if (document.hidden || root.closest("[hidden]")) return;
    for (const [label, since] of liveTimes) {
      const text = ago(since);
      if (label.textContent !== text) label.textContent = text;
    }
  }

  const usernameInput = el("input", { class: "input", id: nextFieldId(), placeholder: "Nome de usuário", spellcheck: false, maxLength: 21, required: true });
  const addButton = el("button", { class: "btn", type: "submit" }, svg(icon("userPlus", "sm")), "Adicionar");
  const addForm = el("form", { class: "friends-add" },
    el("label", { htmlFor: usernameInput.id, class: "friends-label", textContent: "Adicionar amigo" }),
    el("p", { class: "small muted", textContent: "Envie um pedido pelo nome de usuário." }),
    el("div", { class: "friends-add-controls" }, usernameInput, addButton));
  addForm.addEventListener("submit", (event) => {
    event.preventDefault();
    if (!usernameInput.value.trim()) return;
    addButton.disabled = true;
    client.requestFriend(usernameInput.value)
      .then((snapshot) => { usernameInput.value = ""; apply(snapshot); say("ok", "Pedido enviado."); })
      .catch((error: unknown) => say("error", errorText(error)))
      .finally(() => { addButton.disabled = false; });
  });

  const inviteButton = el("button", { class: "btn primary", type: "button" }, svg(icon("link", "sm")), "Copiar link de convite");
  inviteButton.addEventListener("click", () => {
    inviteButton.disabled = true;
    client.createInvite()
      .then(async (invite) => {
        const copied = await copyText(invite.link);
        say("ok", copied ? `Convite pessoal copiado: um destinatário, sete dias. Código: ${invite.code}` : `Envie este convite pessoal: ${invite.link}`);
      })
      .catch((error: unknown) => say("error", errorText(error)))
      .finally(() => { inviteButton.disabled = false; });
  });

  function friendRow(friend: FriendView): HTMLElement {
    const live = friend.live;
    const name = el("div", { class: "row nowrap tight-row" }, el("span", { class: "name", textContent: friend.displayName }), live ? el("span", { class: "badge live", textContent: "Ao vivo" }) : null);
    const age = el("span", { textContent: live ? ago(live.since) : "" });
    if (live) liveTimes.set(age, live.since);
    const sub = live
      ? el("div", { class: "sub" }, `@${friend.username} · `, age, ` · ${live.viewers} assistindo`)
      : el("div", { class: "sub", textContent: `@${friend.username} · ${friend.online ? "online" : "offline"}` });
    if (live?.destination?.kind === "direct") sub.prepend(`${friend.displayName} está compartilhando com você · `);
    const row = el("div", { class: "list-row" }, avatar(friend, friend.online), el("div", { class: "grow" }, name, sub));
    const choose = el("button", { class: "btn ghost sm", type: "button", textContent: "Abrir", ariaLabel: `Abrir perfil de ${friend.displayName}` });
    choose.addEventListener("click", () => { selectedId = friend.id; fingerprint = ""; if (current) update(current); });
    row.append(choose);
    if (live) {
      const watch = el("button", { class: "btn primary sm", type: "button" }, svg(icon("play", "sm")), "Assistir");
      watch.addEventListener("click", () => handlers.onWatch(friend));
      row.append(watch);
      const session = current?.directSessions?.find(item => item.peerId === friend.id);
      if (live.destination?.kind === "direct" && session && !session.declined) {
        const decline = el("button", { class: "btn sm", textContent: "Agora não", type: "button" });
        decline.addEventListener("click", () => { void client.declineDirect(session.id).then(() => client.friends()).then(apply).catch(error => say("error", errorText(error))); });
        row.append(decline);
      }
    }
    const remove = el("button", { class: "btn ghost sm row-remove", type: "button", title: "Remover amigo", ariaLabel: `Remover ${friend.displayName}` }, svg(icon("x", "sm")));
    remove.addEventListener("click", () => {
      if (!confirm(`Remover ${friend.displayName} dos amigos?`)) return;
      client.removeFriend(friend.id).then(apply).catch((error: unknown) => say("error", errorText(error)));
    });
    row.append(remove);
    return row;
  }

  function requestRow(user: UserView, incoming: boolean): HTMLElement {
    const row = el("div", { class: "list-row" }, avatar(user), el("div", { class: "grow" }, el("div", { class: "name", textContent: user.displayName }), el("div", { class: "sub", textContent: `@${user.username}${incoming ? " quer ser seu amigo" : " · pedido enviado"}` })));
    if (incoming) {
      const accept = el("button", { class: "btn primary sm", type: "button", textContent: "Aceitar" });
      const decline = el("button", { class: "btn sm", type: "button", textContent: "Recusar" });
      accept.addEventListener("click", () => { client.acceptFriend(user.id).then(apply).catch((e: unknown) => say("error", errorText(e))); });
      decline.addEventListener("click", () => { client.declineFriend(user.id).then(apply).catch((e: unknown) => say("error", errorText(e))); });
      row.append(decline, accept);
    } else {
      const cancel = el("button", { class: "btn ghost sm", type: "button", textContent: "Cancelar" });
      cancel.addEventListener("click", () => { client.removeFriend(user.id).then(apply).catch((e: unknown) => say("error", errorText(e))); });
      row.append(cancel);
    }
    return row;
  }

  function section(title: string, count: number | null, rows: HTMLElement[], empty?: string): HTMLElement | null {
    if (!rows.length && !empty) return null;
    return el(
      "section",
      {},
      el("div", { class: "section-title" }, el("span", { class: "eyebrow", textContent: title }), count === null ? null : el("span", { class: "small faint", textContent: String(count) })),
      rows.length ? el("div", { class: "list" }, ...rows) : el("div", { class: "list" }, el("div", { class: "list-empty", textContent: empty })),
    );
  }

  function update(snapshot: Snapshot): void {
    if (disposed) return;
    current = snapshot;
    const nextFingerprint = JSON.stringify([snapshot.friends, snapshot.incoming, snapshot.outgoing, snapshot.directSessions, selectedId, filter, search.value]);
    if (fingerprint === nextFingerprint) {
      refreshAges();
      return;
    }
    fingerprint = nextFingerprint;
    liveTimes.clear();
    const matches = snapshot.friends.filter(friend => `${friend.displayName} ${friend.username}`.toLowerCase().includes(search.value.toLowerCase()));
    const live = matches.filter(friend => friend.live && filter !== "pending");
    const online = matches.filter(friend => !friend.live && friend.online && filter !== "pending");
    const offline = matches.filter(friend => !friend.live && !friend.online && filter === "all");
    const blocks = [
      filter !== "online" ? section("Pedidos recebidos", snapshot.incoming.length, snapshot.incoming.map((user) => requestRow(user, true))) : null,
      section("Ao vivo agora", live.length, live.map(friendRow)),
      section("Online", online.length, online.map(friendRow), snapshot.friends.length ? undefined : "Você ainda não tem amigos. Adicione por nome de usuário ou envie um link de convite."),
      section("Offline", offline.length, offline.map(friendRow)),
      filter !== "online" ? section("Pedidos enviados", snapshot.outgoing.length, snapshot.outgoing.map((user) => requestRow(user, false))) : null,
    ];
    const visibleBlocks = blocks.filter((block): block is HTMLElement => block !== null);
    if (!snapshot.friends.length && !snapshot.incoming.length && !snapshot.outgoing.length) {
      lists.replaceChildren(el("div", { class: "friends-empty" }, svg(icon("users")),
        el("h2", { textContent: "Nenhum amigo adicionado" }),
        el("p", { class: "muted", textContent: "Adicione pelo nome de usuário ou envie um link de convite." })));
    } else if (!visibleBlocks.length) {
      lists.replaceChildren(el("div", { class: "friends-empty" }, svg(icon("users")),
        el("h2", { textContent: search.value ? "Nenhum amigo encontrado" : filter === "pending" ? "Nenhum pedido pendente" : "Nenhum amigo online" }),
        el("p", { class: "muted", textContent: search.value ? "Tente buscar por outro nome." : "Escolha Todos para ver sua lista de amigos." })));
    } else lists.replaceChildren(...visibleBlocks);
    const selected = snapshot.friends.find(friend => friend.id === (snapshot.inviter ?? selectedId)) ?? snapshot.friends[0];
    profile.replaceChildren();
    profile.hidden = !selected;
    if (selected) {
      selectedId = selected.id;
      const session = snapshot.directSessions?.find(item => item.peerId === selected.id);
      const share = handlers.onShare ? el("button", { class: "btn primary", type: "button", textContent: `Compartilhar com ${selected.displayName}`, disabled: !selected.online || !!session?.reconnecting }) : null;
      share?.addEventListener("click", () => handlers.onShare?.(selected));
      profile.append(avatar(selected, selected.online), el("h2", { textContent: selected.displayName }), el("p", { textContent: session?.reconnecting ? "Reconectando… A sessão encerra após 30 segundos sem retorno." : selected.online ? "Online" : "Offline · aguarde seu amigo voltar para compartilhar." }), el("p", { class: "small muted", textContent: "Compartilhamento privado. Visível somente para vocês." }));
      if (share) profile.append(share);
      if (session) {
        const end = el("button", { class: "btn", type: "button", textContent: "Encerrar sessão privada" });
        end.addEventListener("click", () => { void client.endDirect(session.id).then(() => client.friends()).then(apply).catch(error => say("error", errorText(error))); });
        profile.append(end);
      }
    }
    if (live.length && ageTimer === undefined) ageTimer = setInterval(refreshAges, 30_000);
    if (!live.length) {
      clearInterval(ageTimer);
      ageTimer = undefined;
    }
  }

  const inviteManager = createInviteManager(client);
  inviteManager.querySelector("summary")?.prepend(svg(icon("chevronDown", "sm")));
  const root = el(
    "div",
    { class: "friends-panel" },
    el("div", { class: "friends-panel-status" }, status),
    el("div", { class: "friends-actions" }, addForm,
      el("section", { class: "friends-invite" },
        el("h2", { class: "friends-label", textContent: "Convite pessoal" }),
        el("p", { class: "small muted", textContent: "Envie um link para adicionar alguém aos amigos." }),
        inviteButton, el("div", { class: "friends-invite-manager" }, inviteManager))),
    el("div", { class: "friends-toolbar" }, filters.root, el("div", { class: "friends-search" }, svg(icon("search", "sm")), search)),
    alerts,
    el("div", { class: "friends-space" }, lists, profile),
  );
  return {
    root,
    update,
    setConnected(connected) {
      if (disposed) return;
      status.textContent = connected ? "Tempo real ativo" : "Reconectando…";
      if (connected && current) update(current);
    },
    dispose() {
      disposed = true;
      clearInterval(ageTimer);
      liveTimes.clear();
    },
  };
}

// ---- Assistir ----------------------------------------------------------------------------------

export interface WatchPanel {
  getDiagnostics(): ReturnType<WatchSession["getDiagnostics"]> | null;
  root: HTMLElement;
  setBackgroundMuted(muted: boolean): void;
  watch(friend: Pick<FriendView, "username" | "displayName"> & Partial<Pick<FriendView, "live">>): void;
  stop(): void;
  dispose(): void;
}

const STATE_TEXT: Record<WatchState, string> = {
  connecting: "Conectando à transmissão…",
  waiting: "Aguardando o amigo iniciar a transmissão",
  playing: "",
  ended: "A transmissão terminou",
  error: "Não foi possível assistir",
  stopped: "Selecione um amigo ao vivo para assistir",
};

export interface WatchOptions {
  popout?(stage: HTMLElement, title: string): Promise<{ close(): void }>;
  keyboardActive?(): boolean;
  /** Ajuda compartilhada pelo layout, sem redimensionar os vídeos. */
  helpHost?: HTMLElement;
  /** Avisa o layout que esta ajuda passou a ocupar o `helpHost`. */
  onHelpShown?(): void;
  /** O app Tauri leva a janela inteira para tela cheia; no navegador usa a Fullscreen API. */
  toggleFullscreen?(stage: HTMLElement): Promise<void>;
  onLeave?(): void;
  /** Prévia local: nunca assina áudio nem permite liberar o som. */
  preview?: boolean;
  /** Espera após "ended" antes de sair sozinho; uma nova geração dentro dela reconecta. */
  endedLeaveMs?: number;
}

const ENDED_LEAVE_MS = 10_000;

const AUDIO_KEY = "ms.audio";

interface AudioPref {
  volume: number;
  muted: boolean;
  lastVolume?: number;
}

function loadAudioPref(key: string): AudioPref {
  try {
    // Migra a preferência global antiga ao salvar a primeira escolha por transmissão.
    const value = JSON.parse(localStorage.getItem(key) ?? localStorage.getItem(AUDIO_KEY) ?? "null") as Partial<AudioPref> | null;
    if (value && typeof value.volume === "number" && Number.isFinite(value.volume) && typeof value.muted === "boolean") {
      return { volume: Math.min(1, Math.max(0, value.volume)), muted: value.muted,
        lastVolume: typeof value.lastVolume === "number" && Number.isFinite(value.lastVolume) && value.lastVolume > 0 && value.lastVolume <= 1 ? value.lastVolume : value.volume > 0 ? Math.min(1, value.volume) : 0.8 };
    }
  } catch {
    // Preferência corrompida: usa o padrão.
  }
  return { volume: 0.8, muted: false };
}

/** O navegador só libera som automático depois de uma interação do usuário com a página. */
/** Linha de qualidade dos detalhes técnicos; null aparece como "indisponível", nunca 0. */
export function formatVideoQuality(q: VideoQuality | undefined): string {
  if (!q || q.windowSeconds === null) return "Qualidade: aguardando amostras.";
  const n = (value: number | null, digits = 1) => value === null ? "indisponível" : value.toFixed(digits).replace(".", ",");
  return `Qualidade (últimos ${Math.round(q.windowSeconds)} s): QP médio ${q.qpAverage === null ? "indisponível (o decodificador por hardware não informa; veja o diagnóstico de quem transmite)" : n(q.qpAverage)} · keyframes ${n(q.keyframesPerMinute)}/min · PLI ${n(q.pliPerMinute)}/min · NACK ${n(q.nackPerMinute)}/min · ${q.codec ?? "codec indisponível"} · decoder ${q.decoder ?? "indisponível"}`;
}

function soundAllowed(): boolean {
  return navigator.userActivation?.hasBeenActive ?? true;
}

export function createWatchPanel(client: MadClient, options: WatchOptions = {}): WatchPanel {
  let detached: { close(): void } | null = null;
  let detachGeneration = 0;
  let session: WatchSession | null = null;
  let audioPresent = false;
  let audioKey = AUDIO_KEY;
  let audioPref = loadAudioPref(audioKey);
  let selfView = options.preview === true;
  let autoplayBlocked = false;
  let playAttempt = 0;
  let backgroundMuted = false;
  let hasPlayed = false;
  let displayName = "transmissão";
  const audioStatus = el("span", { class: "small faint", textContent: "Sem áudio" });
  const video = el("video", { autoplay: true, muted: true, playsInline: true });
  video.setAttribute("aria-label", "Tela compartilhada");
  video.disablePictureInPicture = true;
  const message = el("div", { class: "placeholder" }, svg(icon("monitor")), el("span", { textContent: STATE_TEXT.stopped }));
  const hud = el("div", { class: "hud", hidden: true });
  const qualityLine = el("p", { class: "small mono" });
  const playerStats = el("span", { class: "player-stats mono", hidden: true });
  const fullscreen = el("button", { class: "btn player-fullscreen", type: "button" });
  const diagnostic = el("button", { class: "btn sm", type: "button", textContent: "Copiar diagnóstico", title: "Copie logo após uma travadinha para registrar os últimos segundos.", disabled: true });
  const diagnosticHelp = el("p", { class: "small muted" }, "Para pedir ajuda, informe a versão do seu app e peça também o diagnóstico de quem transmite. O relatório contém contadores e tempos, sem imagem ou som.");
  const stateDetail = el("p", { class: "small muted", hidden: true });
  const guidance = el("div", { class: "stack tight", hidden: true });
  const explain = el("input", { type: "checkbox" });
  const refreshGuidance = () => {
    guidance.hidden = !explain.checked;
    guidance.replaceChildren(...(explain.checked ? diagnosticGuidance(session?.getDiagnostics() ?? null, performance.now(), document.hidden).map(text => el("p", { class: "small muted" }, text)) : []));
  };
  explain.addEventListener("change", refreshGuidance);
  const refresh = el("button", { class: "btn sm", type: "button", textContent: "Atualizar observações" });
  refresh.addEventListener("click", refreshGuidance);
  const closeHelp = el("button", { class: "btn sm", type: "button", textContent: "Fechar ajuda" });
  const helpTitle = el("summary", { textContent: "Detalhes e ajuda" });
  const diagnostics = el("details", { class: "watch-diagnostics" },
    helpTitle,
    el("div", { class: "watch-help-actions row" }, diagnostic, closeHelp),
    el("div", { class: "stack tight mt" }, stateDetail,
      el("p", { class: "small" }, "Use o volume para ajustar o som, M para silenciar e F para tela cheia. Se houver travadas ou ruído, copie o diagnóstico e envie a quem transmite."),
      diagnosticHelp,
      el("details", { class: "watch-technical" }, el("summary", {}, "Detalhes técnicos"), hud, qualityLine,
      el("label", { class: "row" }, explain, "Mostrar observações do último intervalo"),
      el("p", { class: "small muted" }, "Retrato ao abrir a ajuda ou escolher Atualizar observações; não acompanha a transmissão continuamente."), guidance, refresh)),
  );
  const info = el("button", { class: "btn", type: "button", title: "Ajuda" }, svg(icon("help")), el("span", { class: "control-label" }, "Ajuda"));
  info.setAttribute("aria-expanded", "false");
  const syncHelp = (): void => {
    info.setAttribute("aria-expanded", String(diagnostics.open));
    if (diagnostics.open) {
      const host = document.fullscreenElement === stage ? stage : options.helpHost;
      if (host) {
        const previous = diagnostics.parentElement;
        for (const other of host.querySelectorAll<HTMLDetailsElement>(".watch-diagnostics[open]")) {
          if (other !== diagnostics) other.open = false;
        }
        host.append(diagnostics);
        if (previous === options.helpHost && previous !== host && !previous.querySelector(".watch-diagnostics[open]")) previous.hidden = true;
        if (host === options.helpHost) { host.hidden = false; options.onHelpShown?.(); }
      }
      refreshGuidance();
    } else {
      const host = diagnostics.parentElement;
      if (diagnostics.contains(document.activeElement)) info.focus();
      if (host && host !== panel.root) panel.root.append(diagnostics);
      if (host === options.helpHost && !host.querySelector(".watch-diagnostics[open]")) host.hidden = true;
    }
  };
  info.addEventListener("click", () => {
    diagnostics.open = !diagnostics.open;
    syncHelp();
    if (diagnostics.open) closeHelp.focus();
  });
  const dismissHelp = (): void => { diagnostics.open = false; syncHelp(); info.focus(); };
  closeHelp.addEventListener("click", dismissHelp);
  diagnostics.addEventListener("keydown", event => {
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); dismissHelp(); }
  });
  diagnostics.addEventListener("toggle", syncHelp);
  let diagnosticReset: ReturnType<typeof setTimeout> | undefined;
  const volume = el("input", { class: "volume", type: "range", min: "0", max: "100", step: "1", value: String(Math.round(audioPref.volume * 100)) });
  volume.setAttribute("aria-label", "Volume");
  const level = el("span", { class: "volume-value", ariaHidden: "true" });
  const mute = el("button", { class: "btn", type: "button" });
  const unlock = el("button", { class: "audio-unlock", type: "button", hidden: true }, svg(icon("volumeOff", "sm")), "Clique para ativar o som");
  const popout = el("button", { class: "btn", type: "button", title: "Mini-player", ariaLabel: "Mini-player", hidden: !options.popout }, svg(icon("miniPlayer")), el("span", { class: "control-label" }, "Mini-player"));
  const stage = el("div", { class: "stage" }, video, message, unlock, el("div", { class: "controls" },
    el("div", { class: "audio-ctl" }, mute, volume, level), playerStats, info, popout, fullscreen));
  const syncFullscreen = (): void => {
    const active = stage.ownerDocument.fullscreenElement === stage;
    const label = active ? "Sair da tela cheia" : "Tela cheia";
    fullscreen.title = `${label} (F)`;
    fullscreen.ariaLabel = `${label}: ${displayName}`;
    fullscreen.replaceChildren(svg(icon(active ? "exitFullscreen" : "fullscreen")), el("span", { class: "control-label" }, label));
  };
  syncFullscreen();
  stage.addEventListener("fullscreenchange", () => { syncFullscreen(); syncHelp(); });
  const actionError = el("div", { class: "watch-action-error", role: "alert", hidden: true });
  stage.append(actionError);
  stage.dataset.theme = videoTheme();
  const title = el("h2", { textContent: "Assistir" });
  const badge = el("span", { class: "badge", hidden: true });
  const playerTop = el("div", { class: "player-top" }, el("div", { class: "who" }, title, badge, audioStatus));
  stage.append(playerTop);
  popout.setAttribute("data-main-window-only", "");
  info.setAttribute("data-main-window-only", "");
  popout.addEventListener("click", async () => {
    if (!options.popout || !session || popout.disabled) return;
    const generation = ++detachGeneration;
    popout.disabled = true;
    actionError.hidden = true;
    try {
      const handle = await options.popout(stage, displayName);
      if (generation !== detachGeneration || !session) handle.close();
      else detached = handle;
    } catch (error) {
      stateDetail.textContent = error instanceof Error ? error.message : "Não foi possível abrir o mini-player.";
      stateDetail.hidden = false;
      actionError.textContent = "Não foi possível abrir o mini-player. Tente novamente pelo botão Mini-player.";
      actionError.hidden = false;
    } finally { popout.disabled = false; }
  });
  const selfNotice = banner("info", "Sua própria transmissão fica sempre sem som para evitar eco.");
  selfNotice.hidden = true;

  const saveAudioPref = (): void => {
    if (selfView) return;
    try { localStorage.setItem(audioKey, JSON.stringify(audioPref)); } catch { /* armazenamento indisponível */ }
  };
  /** Aplica volume e mudo ao <video>; sem interação prévia o navegador exige manter mudo. */
  const applyAudio = (): void => {
    const allowed = soundAllowed();
    video.volume = audioPref.volume;
    video.muted = selfView || backgroundMuted || !audioPresent || audioPref.muted || !allowed || autoplayBlocked;
    unlock.hidden = selfView || backgroundMuted || !audioPresent || audioPref.muted || audioPref.volume === 0 || (allowed && !autoplayBlocked);
    audioStatus.textContent = selfView ? "Prévia sem som" : !audioPresent ? "Sem faixa de áudio"
      : backgroundMuted ? "Som só no destaque" : audioPref.volume === 0 ? "Volume zero"
        : audioPref.muted ? "Silenciado" : !allowed || autoplayBlocked ? "Ative o som com um clique" : "Som ativo";
    const silent = audioPref.muted || audioPref.volume === 0;
    const soundIcon = silent || !audioPresent || selfView ? "volumeOff" : "volume";
    // Não remova o alvo do pointerdown antes de o navegador entregar o click.
    if (mute.dataset.soundIcon !== soundIcon) {
      mute.dataset.soundIcon = soundIcon;
      mute.replaceChildren(svg(icon(soundIcon)));
    }
    mute.disabled = !audioPresent || selfView;
    volume.disabled = !audioPresent || selfView;
    const label = selfView ? "Sua própria transmissão fica sempre sem som" : !audioPresent ? "Esta transmissão não tem áudio" : silent ? "Ativar o som" : "Silenciar";
    mute.title = label;
    mute.setAttribute("aria-label", `${label}: ${displayName}`);
    volume.value = String(Math.round(audioPref.volume * 100));
    const percent = `${volume.value}%`;
    volume.style.setProperty("--volume-level", percent);
    volume.setAttribute("aria-valuetext", percent);
    level.textContent = volume.disabled ? "—" : percent;
    const attempt = ++playAttempt;
    if (!video.muted && video.srcObject) {
      // Desmutar sem gesto pode pausar o vídeo: volta a mudo em vez de travar a imagem.
      void video.play().catch(() => {
        if (attempt !== playAttempt) return;
        autoplayBlocked = true;
        applyAudio();
        void video.play().catch(() => undefined);
      });
    }
  };
  const setAudioPresent = (present: boolean): void => {
    audioPresent = present;
    applyAudio();
  };
  mute.addEventListener("click", () => {
    if (selfView || !audioPresent) return;
    autoplayBlocked = false;
    audioPref.muted = !(audioPref.muted || audioPref.volume === 0);
    if (!audioPref.muted && audioPref.volume === 0) audioPref.volume = audioPref.lastVolume ?? 0.8;
    saveAudioPref();
    applyAudio();
  });
  volume.addEventListener("input", () => {
    if (selfView || !audioPresent) return;
    autoplayBlocked = false;
    audioPref.volume = Number(volume.value) / 100;
    if (audioPref.volume > 0) audioPref.lastVolume = audioPref.volume;
    audioPref.muted = audioPref.volume === 0;
    saveAudioPref();
    applyAudio();
  });
  // A primeira interação libera o som que o navegador manteve mudo.
  const onInteract = (): void => {
    if (!audioPresent || selfView) return;
    autoplayBlocked = false;
    applyAudio();
  };
  document.addEventListener("pointerdown", onInteract, true);
  document.addEventListener("keydown", onInteract, true);
  unlock.addEventListener("click", onInteract);
  const leave = el("button", { class: "btn sm", type: "button", textContent: "Sair", title: "Sair desta transmissão", hidden: true });
  playerTop.append(leave);
  const retry = el("button", { class: "btn primary sm", type: "button", textContent: "Tentar novamente", hidden: true });
  diagnostic.addEventListener("click", async () => {
    if (!session) return;
    const activeSession = session;
    const copied = await copyText(JSON.stringify(session.getDiagnostics(), null, 2));
    if (session !== activeSession) return;
    diagnostic.textContent = copied ? "Copiado" : "Não foi possível copiar";
    clearTimeout(diagnosticReset);
    diagnosticReset = setTimeout(() => { diagnostic.textContent = "Copiar diagnóstico"; }, 2000);
  });

  fullscreen.addEventListener("click", () => {
    if (options.toggleFullscreen) void options.toggleFullscreen(stage);
    else if (document.fullscreenElement) void document.exitFullscreen();
    else void stage.requestFullscreen().catch(() => undefined);
  });
  stage.addEventListener("dblclick", event => {
    if (event.target === stage || event.target === video || message.contains(event.target as Node)) fullscreen.click();
  });
  // Tecla F alterna a tela cheia (fora de campos de texto), como nos outros viewers.
  const onKey = (event: KeyboardEvent): void => {
    if (options.keyboardActive && !options.keyboardActive()) return;
    const key = event.key.toLowerCase();
    if ((key !== "f" && key !== "m") || event.repeat || event.ctrlKey || event.metaKey || event.altKey || !stage.isConnected || stage.closest("[hidden]")) return;
    const target = event.target;
    if (target instanceof HTMLElement && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName))) return;
    if (target instanceof HTMLElement && target.closest(".watch-panel") && !stage.parentElement?.contains(target)) return;
    if (key === "m") {
      if (audioPresent) mute.click();
    } else if (video.videoWidth > 0) fullscreen.click();
  };
  document.addEventListener("keydown", onKey);
  leave.addEventListener("click", () => { panel.stop(); options.onLeave?.(); });
  retry.addEventListener("click", () => session?.reconnect());

  // Fim da live sai sozinho, mas espera a nova geração de uma troca de qualidade/fonte.
  let endedTimer: ReturnType<typeof setTimeout> | undefined;
  const cancelEndedLeave = (): void => { clearTimeout(endedTimer); endedTimer = undefined; };
  const scheduleEndedLeave = (): void => {
    cancelEndedLeave();
    const current = session;
    endedTimer = setTimeout(() => {
      endedTimer = undefined;
      if (session !== current || session === null) return;
      panel.stop();
      options.onLeave?.();
    }, options.endedLeaveMs ?? ENDED_LEAVE_MS);
  };

  const show = (state: WatchState, detail?: string): void => {
    guidance.replaceChildren();
    diagnostic.disabled = session === null;
    const playing = state === "playing";
    if (state === "ended" || state === "error" || state === "stopped") {
      detachGeneration++;
      detached?.close(); detached = null;
    }
    message.hidden = playing;
    const reconnecting = state === "connecting" && hasPlayed;
    message.lastElementChild!.textContent = state === "error"
      ? "Não foi possível assistir. Tente novamente ou copie o diagnóstico em Detalhes e ajuda."
      : reconnecting ? "Reconectando à transmissão… Aguarde a retomada."
        : detail ?? STATE_TEXT[state];
    stateDetail.textContent = detail ?? "";
    stateDetail.hidden = !detail;
    if (playing) hasPlayed = true;
    panel.root.dataset.state = state;
    hud.hidden = !playing;
    playerStats.hidden = !playing;
    badge.hidden = state === "stopped";
    badge.className = playing ? "badge live" : "badge";
    badge.textContent = playing ? "Ao vivo" : state === "connecting" ? reconnecting ? "Reconectando" : "Conectando" : state === "waiting" ? "Aguardando" : state === "error" ? "Falha na conexão" : "Encerrada";
    leave.hidden = state === "stopped";
    retry.hidden = state !== "error";
  };
  const formatStats = (s: WatchStats): string =>
    `${s.width}×${s.height} · ${s.fps.toFixed(0)} FPS · ${s.mbps.toLocaleString("pt-BR", { minimumFractionDigits: 1, maximumFractionDigits: 1 })} Mbps${s.rttMs !== null ? ` · RTT ${s.rttMs.toFixed(0)} ms` : ""}`;

  const panel: WatchPanel = {
    getDiagnostics: () => session?.getDiagnostics() ?? null,
    root: el("div", { class: "stack fade-in watch-panel" }, el("div", { class: "row between watch-panel-head" }, retry), selfNotice, stage, diagnostics),
    setBackgroundMuted(muted) { if (backgroundMuted !== muted) { backgroundMuted = muted; applyAudio(); } },
    watch(friend) {
      panel.stop();
      title.textContent = friend.displayName;
      displayName = friend.displayName;
      panel.root.setAttribute("aria-label", `Transmissão de ${displayName}`);
      volume.setAttribute("aria-label", `Volume: ${displayName}`);
      syncFullscreen();
      leave.setAttribute("aria-label", `Sair da transmissão: ${displayName}`);
      diagnostic.setAttribute("aria-label", `Copiar diagnóstico: ${displayName}`);
      info.setAttribute("aria-label", `Ajuda: ${displayName}`);
      helpTitle.textContent = `Detalhes e ajuda · ${displayName}`;
      closeHelp.setAttribute("aria-label", `Fechar ajuda: ${displayName}`);
      selfView = options.preview === true || friend.username.toLowerCase() === client.session?.user.username.toLowerCase();
      selfNotice.hidden = !selfView;
      audioKey = `${AUDIO_KEY}.${encodeURIComponent(client.session?.user.id ?? "anonymous")}.${encodeURIComponent(friend.username.toLowerCase())}`;
      audioPref = loadAudioPref(audioKey);
      setAudioPresent(false);
      session = new WatchSession(client, friend.username, video, {
        onState: (state, detail) => {
          // "waiting" (hello sem live após reconectar o socket) não cancela a saída pendente.
          if (state === "ended") scheduleEndedLeave();
          else if (state !== "waiting") cancelEndedLeave();
          show(state, detail);
        },
        onAudio: setAudioPresent,
        onAutoplayBlocked: () => { autoplayBlocked = true; applyAudio(); },
        onStats: (stats) => {
          hud.textContent = formatStats(stats);
          playerStats.textContent = hud.textContent;
          if (diagnostics.open) qualityLine.textContent = formatVideoQuality(session?.getQuality());
          hud.title = `Último intervalo: perda ${stats.lossPct?.toFixed(2) ?? "—"}% · buffer de vídeo ${stats.jitterBufferMs?.toFixed(1) ?? "—"} ms`;
        },
      }, { audio: !selfView, ...(friend.live?.destination ? { destination: friend.live.destination } : {}) });
      session.start();
    },
    stop() {
      diagnostics.open = false;
      syncHelp();
      actionError.hidden = true;
      cancelEndedLeave();
      detachGeneration++;
      detached?.close(); detached = null;
      clearTimeout(diagnosticReset);
      diagnostic.textContent = "Copiar diagnóstico";
      qualityLine.textContent = "";
      session?.stop();
      session = null;
      audioPresent = false;
      autoplayBlocked = false;
      hasPlayed = false;
      hud.textContent = "";
      playerStats.textContent = "";
      guidance.replaceChildren();
      hud.removeAttribute("title");
      selfNotice.hidden = true;
      applyAudio();
      title.textContent = "Assistir";
      show("stopped");
    },
    dispose() {
      panel.stop();
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("pointerdown", onInteract, true);
      document.removeEventListener("keydown", onInteract, true);
    },
  };
  show("stopped");
  applyAudio();
  return panel;
}

export { ApiFailure };

export interface WatchLayout {
  getDiagnostics(): Array<ReturnType<WatchSession["getDiagnostics"]> | null>;
  root: HTMLElement;
  readonly count: number;
  actionLabel(username: string): string;
  watch(friend: Pick<FriendView, "username" | "displayName"> & Partial<Pick<FriendView, "live">>): void;
  stop(): void;
  dispose(): void;
}

/** Duas conexões independentes; áudio, estatísticas e ciclo de vida pertencem a cada painel. */
export function createWatchLayout(client: MadClient, options: WatchOptions & { onAdd?(): void } = {}): WatchLayout {
  let selected = 0;
  let featured: number | null = null;
  let onlyFeaturedAudio = false;
  const users: (string | null)[] = [null, null];
  const names: string[] = ["", ""];
  const grid = el("div", { class: "watch-grid" });
  const slots: HTMLElement[] = [];
  const selectors: HTMLButtonElement[] = [];
  const panels: WatchPanel[] = [];
  const controls = el("div", { class: "watch-toolbar" });
  const selection = el("div", { class: "row wrap watch-selection" });
  const helpHost = el("aside", { class: "watch-help-host", hidden: true, ariaLabel: "Ajuda da transmissão" });
  const add = el("button", { class: "btn sm", type: "button" }, svg(icon("plus", "sm")), "Adicionar transmissão");
  const swap = el("button", { class: "btn sm", type: "button", textContent: "Trocar destaque", title: "Trocar destaque (D)" });
  const equal = el("button", { class: "btn sm", type: "button", textContent: "Lado a lado" });
  const audioMode = el("input", { type: "checkbox" });
  const audioLabel = el("label", { class: "row small" }, audioMode, "Som só no destaque");
  add.addEventListener("click", () => options.onAdd?.());
  const hint = el("p", { class: "small faint", textContent: "Com duas transmissões abertas, a próxima substitui a tela selecionada." });
  const root = el("div", { class: "watch-layout stack" }, controls, hint, grid);
  // Fração do espaço da Tela 1; o destaque é só a tela maior. Arrastar o divisor ajusta livremente.
  let ratio = 0.5;
  const FEATURED_RATIO = 0.7, MIN_RATIO = 0.2, MAX_RATIO = 0.8;
  const setRatio = (next: number): void => {
    ratio = Math.round(Math.min(MAX_RATIO, Math.max(MIN_RATIO, next)) * 1000) / 1000;
    if (Math.abs(ratio - 0.5) < 0.02) ratio = 0.5;
    featured = ratio > 0.5 ? 0 : ratio < 0.5 ? 1 : null;
  };
  const divider = el("div", { class: "watch-divider", tabIndex: 0, hidden: true });
  divider.setAttribute("role", "separator");
  divider.setAttribute("aria-label", "Redimensionar transmissões (arraste, setas ou duplo clique para igualar)");
  divider.title = "Arraste para redimensionar · duplo clique para igualar";
  const update = (): void => {
    const count = users.filter(Boolean).length;
    if (count < 2) { featured = null; ratio = 0.5; }
    grid.style.setProperty("--split-a", `${ratio}fr`);
    grid.style.setProperty("--split-b", `${1 - ratio}fr`);
    divider.hidden = count < 2;
    divider.setAttribute("aria-valuenow", String(Math.round(ratio * 100)));
    divider.setAttribute("aria-valuemin", String(MIN_RATIO * 100));
    divider.setAttribute("aria-valuemax", String(MAX_RATIO * 100));
    grid.classList.toggle("split", count === 2);
    grid.classList.toggle("featured-first", count === 2 && featured === 0);
    grid.classList.toggle("featured-second", count === 2 && featured === 1);
    swap.hidden = equal.hidden = audioLabel.hidden = count < 2;
    equal.setAttribute("aria-pressed", String(featured === null));
    hint.hidden = count < 2;
    hint.textContent = "A próxima transmissão substitui a tela marcada. Atalhos: D destaque, M som, F tela cheia.";
    add.hidden = count === 0 || !options.onAdd;
    for (let index = 0; index < 2; index++) {
      slots[index]!.hidden = users[index] === null;
      slots[index]!.classList.toggle("selected", index === selected);
      selectors[index]!.hidden = users[index] === null;
      selectors[index]!.textContent = count === 1 ? `Assistindo · ${names[index]}` : `Tela ${index + 1} · ${names[index]}`;
      // Com uma live não há o que escolher: o botão só identifica quem está na tela.
      if (count === 2) selectors[index]!.setAttribute("aria-pressed", String(index === selected));
      else selectors[index]!.removeAttribute("aria-pressed");
      selectors[index]!.setAttribute("aria-label", `Selecionar Tela ${index + 1}: ${names[index]}${index === selected ? " (selecionada)" : ""}`);
      slots[index]!.setAttribute("aria-label", `Tela ${index + 1}: ${names[index]}${index === selected ? " (selecionada)" : ""}`);
      panels[index]!.setBackgroundMuted(count === 2 && onlyFeaturedAudio && index !== (featured ?? selected));
    }
  };
  /** Sem destaque, a selecionada vira a maior; com destaque, as telas trocam de tamanho (mantém o ajuste arrastado). */
  const swapFeature = (): void => {
    setRatio(featured === null ? (selected === 0 ? FEATURED_RATIO : 1 - FEATURED_RATIO) : 1 - ratio);
    selected = featured ?? selected;
    update();
  };
  swap.addEventListener("click", swapFeature);
  equal.addEventListener("click", () => { setRatio(0.5); update(); });
  // Divisor: horizontal lado a lado; vertical quando a janela estreita empilha as telas.
  const stacked = (): boolean => slots[1]!.getBoundingClientRect().top >= slots[0]!.getBoundingClientRect().bottom - 1;
  divider.addEventListener("pointerdown", event => {
    if (event.button !== 0) return;
    event.preventDefault();
    divider.setPointerCapture(event.pointerId);
    divider.classList.add("dragging");
    const vertical = stacked();
    const box = grid.getBoundingClientRect();
    const size = (vertical ? box.height : box.width) - (vertical ? divider.offsetHeight : divider.offsetWidth);
    const move = (moveEvent: PointerEvent): void => {
      if (size <= 0) return;
      const offset = vertical ? moveEvent.clientY - box.top : moveEvent.clientX - box.left;
      setRatio((offset - (vertical ? divider.offsetHeight : divider.offsetWidth) / 2) / size);
      update();
    };
    const end = (): void => {
      divider.classList.remove("dragging");
      divider.removeEventListener("pointermove", move);
      divider.removeEventListener("pointerup", end);
      divider.removeEventListener("pointercancel", end);
    };
    divider.addEventListener("pointermove", move);
    divider.addEventListener("pointerup", end);
    divider.addEventListener("pointercancel", end);
  });
  divider.addEventListener("dblclick", () => { setRatio(0.5); update(); });
  divider.addEventListener("keydown", event => {
    const step = { ArrowLeft: -0.05, ArrowUp: -0.05, ArrowRight: 0.05, ArrowDown: 0.05 }[event.key];
    if (step === undefined && event.key !== "Home" && event.key !== "End") return;
    event.preventDefault();
    setRatio(event.key === "Home" ? MIN_RATIO : event.key === "End" ? MAX_RATIO : ratio + step!);
    update();
  });
  audioMode.addEventListener("change", () => { onlyFeaturedAudio = audioMode.checked; update(); });
  const onLayoutKey = (event: KeyboardEvent): void => {
    if (event.key.toLowerCase() !== "d" || event.repeat || event.ctrlKey || event.metaKey || event.altKey ||
      !root.isConnected || root.closest("[hidden]") || document.fullscreenElement ||
      users.filter(Boolean).length !== 2 || (options.keyboardActive && !options.keyboardActive())) return;
    const target = event.target;
    if (target instanceof HTMLElement && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName))) return;
    if (target instanceof HTMLElement && target.closest(".watch-layout") && !root.contains(target)) return;
    event.preventDefault();
    swapFeature();
  };
  document.addEventListener("keydown", onLayoutKey);
  for (let index = 0; index < 2; index++) {
    const selector = el("button", { class: "btn sm watch-selector", type: "button", textContent: `Tela ${index + 1}` });
    selector.addEventListener("click", () => { selected = index; update(); });
    selectors.push(selector); selection.append(selector);
    const panel = createWatchPanel(client, {
      helpHost,
      // A ajuda abre do lado oposto ao da pessoa, para que o vídeo e os controles dela continuem à vista.
      onHelpShown: () => { helpHost.dataset.side = index === 1 && users.every(Boolean) ? "start" : "end"; },
      ...(options.popout ? { popout: options.popout } : {}),
      ...(options.toggleFullscreen ? { toggleFullscreen: options.toggleFullscreen } : {}),
      ...(options.preview !== undefined ? { preview: options.preview } : {}),
      ...(options.endedLeaveMs !== undefined ? { endedLeaveMs: options.endedLeaveMs } : {}),
      keyboardActive: () => (document.fullscreenElement ? slots[index]!.contains(document.fullscreenElement) : selected === index) && (options.keyboardActive?.() ?? true),
      onLeave: () => {
        users[index] = null;
        selected = users[1 - index] ? 1 - index : 0;
        update();
        if (!users.some(Boolean)) options.onLeave?.();
      },
    });
    panels.push(panel);
    const slot = el("div", { class: "watch-slot", role: "group" }, panel.root);
    slot.setAttribute("aria-label", `Tela ${index + 1}`);
    // Clicar no vídeo da tela menor (fora dos controles) troca os tamanhos: ela vira a maior.
    let swapOnClick = false;
    slot.addEventListener("pointerdown", event => {
      const target = event.target instanceof Element ? event.target : null;
      swapOnClick = event.button === 0 && featured !== null && featured !== index && users.every(Boolean) &&
        !target?.closest("button, input, select, textarea, label, a, [role=slider], [role=button], summary, .banner, .watch-diagnostics");
      selected = index; update();
    });
    slot.addEventListener("click", () => {
      if (!swapOnClick) return;
      swapOnClick = false;
      if (featured !== null && featured !== index) { setRatio(1 - ratio); selected = index; update(); }
    });
    slot.addEventListener("focusin", () => { selected = index; update(); });
    slots.push(slot); grid.append(slot);
    if (index === 0) grid.append(divider);
  }
  // A ajuda fica sobre as telas, sem cobrir a barra de troca e destaque, e vem depois delas no foco.
  grid.append(helpHost);
  controls.append(selection, el("div", { class: "row wrap watch-layout-actions" }, swap, equal, audioLabel, add));
  update();
  return {
    getDiagnostics: () => panels.map(panel => panel.getDiagnostics()),
    root,
    get count() { return users.filter(Boolean).length; },
    actionLabel(username) {
      if (users.includes(username.toLowerCase())) return "Voltar à transmissão";
      const count = users.filter(Boolean).length;
      return count === 0 ? "Assistir" : count === 1 ? "Assistir ao lado" : "Substituir tela selecionada";
    },
    watch(friend) {
      const username = friend.username.toLowerCase();
      const existing = users.indexOf(username);
      if (existing >= 0) { selected = existing; update(); return; }
      const empty = users.indexOf(null);
      if (empty >= 0) selected = empty;
      users[selected] = username;
      names[selected] = friend.displayName;
      panels[selected]!.watch(friend);
      update();
    },
    stop() { users.fill(null); panels.forEach(panel => panel.stop()); selected = 0; update(); },
    dispose() { document.removeEventListener("keydown", onLayoutKey); users.fill(null); panels.forEach(panel => panel.dispose()); update(); },
  };
}
