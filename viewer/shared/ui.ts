// Componentes de interface compartilhados (app Tauri e viewer web): entrar, amigos e assistir.
import { ApiFailure, clearPkce, savedPkce, pendingInvite, rememberInvite, WatchSession, type FriendView, type MadClient, type Snapshot, type UserView, type WatchState, type WatchStats } from "./client";
import { ago, banner, copyText, el, initials, segmented, svg } from "./dom";
import { icon } from "./icons";
import { diagnosticGuidance } from "./diagnostic-guidance";
import { videoTheme } from "./theme";

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
  /** Abre a URL no navegador do sistema (app) ou navega (web). */
  openUrl(url: string): void;
  onLoggedIn(recoveryKey?: string): void;
  invite?: string | undefined;
  signal?: AbortSignal;
  onInviteDestination?(server: string | null, channel: string | null): void;
}

export function createLogin(client: MadClient, options: LoginOptions): HTMLElement {
  const alerts = el("div", { class: "stack tight" });
  const say = (kind: "error" | "info" | "ok", text: string): void => alerts.replaceChildren(banner(kind, text));
  let abort: AbortController | null = null;
  options.signal?.addEventListener("abort", () => abort?.abort(), { once: true });

  const discord = el("button", { class: "btn primary big block", type: "button" }, svg(icon("radio")), "Continuar com Discord");
  const google = el("button", { class: "btn big block", type: "button", hidden: true }, svg(icon("users")), "Continuar com Google");
  const hint = el("p", { class: "small faint", hidden: true });

  async function oauth(provider: "discord" | "google"): Promise<void> {
    if (options.signal?.aborted) return;
    abort?.abort();
    const controller = new AbortController();
    abort = controller;
    const { url, verifier } = await client.beginOAuth(provider, code.value);
    if (controller.signal.aborted || abort !== controller) return;
    localStorage.setItem("ms.pkce", verifier);
    options.openUrl(url);
    await resume(verifier, controller);
  }

  async function resume(verifier: string, controller = new AbortController()): Promise<void> {
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
    alerts.replaceChildren(el("div", { class: "banner info row between" }, "Conclua o login na janela do navegador…", cancel));
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
  const code = el("input", { class: "input", placeholder: "XXXX-XXXX ou link do convite", autocomplete: "off", maxLength: 512, spellcheck: false, value: options.invite ?? pendingInvite() ?? "" });
  const create = el("button", { class: "btn primary block", type: "submit", textContent: "Criar conta" });
  const codeField = field("Tenho um convite (código ou link)", code);
  const destination = el("p", { class: "small muted", role: "status" });
  let previewVersion = 0;
  async function previewDestination(): Promise<void> {
    const version = ++previewVersion;
    const value = code.value.trim();
    options.onInviteDestination?.(null, null);
    destination.textContent = value ? "Consultando destino do convite…" : "Cole o convite para conferir em qual servidor você vai entrar.";
    if (!value) return;
    try {
      const preview = await client.previewInvite(value);
      if (version !== previewVersion || options.signal?.aborted) return;
      options.onInviteDestination?.(preview.server?.name ?? null, preview.channel?.name ?? null);
      destination.textContent = preview.server
        ? `Convite de ${preview.inviter.displayName} para ${preview.server.name}${preview.channel ? ` · ${preview.channel.name}` : ""}. Você entrará após o login.`
        : `Convite de ${preview.inviter.displayName}. Você se conectará a essa pessoa após o login.`;
    } catch (error) {
      if (version === previewVersion && !options.signal?.aborted) destination.textContent = `${errorText(error)} Confira o convite ou peça um novo link a quem convidou.`;
    }
  }
  code.addEventListener("input", () => { previewVersion++; options.onInviteDestination?.(null, null); destination.textContent = "Confirme o destino ao sair deste campo."; });
  code.addEventListener("change", () => { void previewDestination(); });
  void previewDestination();
  const registerForm = el("form", { class: "stack", noValidate: true }, field("Nome de usuário", username), create);

  const loginUser = el("input", { class: "input", placeholder: "seunome", autocomplete: "username", spellcheck: false });
  const recovery = el("input", { class: "input mono", placeholder: "XXXXX-XXXXX-XXXXX-XXXXX", autocomplete: "current-password", spellcheck: false, type: "password" });
  const enter = el("button", { class: "btn primary block", type: "submit", textContent: "Entrar" });
  const loginForm = el("form", { class: "stack", noValidate: true, hidden: true }, field("Nome de usuário", loginUser), field("Chave de recuperação", recovery), enter);

  const mode = segmented<"invite" | "account">(
    [{ value: "invite", label: "Tenho um convite" }, { value: "account", label: "Já tenho conta" }],
    "invite",
    (value) => {
      registerForm.hidden = value !== "invite";
      loginForm.hidden = value !== "account";
      alerts.replaceChildren();
    },
  );
  mode.root.setAttribute("aria-label", "Entrar sem Discord");
  const fallback = el("details", { class: "auth-fallback" }, el("summary", { textContent: "Entrar sem Discord" }), el("div", { class: "stack mt" }, mode.root, registerForm, loginForm));
  const oauthGroup = el("div", { class: "stack tight auth-oauth" }, discord, google, hint);
  fallback.addEventListener("toggle", () => {
    root.classList.toggle("alternative", fallback.open);
    discord.classList.toggle("primary", !fallback.open);
    if (fallback.open) registerForm.prepend(codeField, destination);
    else root.insertBefore(el("div", { class: "auth-invite stack tight" }, codeField, destination), oauthGroup);
    for (const empty of root.querySelectorAll(".auth-invite:empty")) empty.remove();
  });

  registerForm.addEventListener("submit", (event) => {
    event.preventDefault();
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
      codeField.querySelector("label")!.textContent = "Código de convite (opcional: você é o primeiro)";
    }
  }).catch(() => { fallback.open = true; say("error", "Não foi possível consultar as opções de entrada. Confira sua conexão e tente entrar novamente."); });

  const pending = savedPkce();
  if (pending) queueMicrotask(() => void resume(pending));

  const root = el(
    "div",
    { class: "stack loose fade-in auth-flow" },
    el("div", { class: "stack tight" }, el("h2", {}, options.invite ? "Convite recebido" : "Acessar conta")),
    el("div", { class: "auth-invite stack tight" }, codeField, destination),
    oauthGroup,
    fallback,
    alerts,
  );
  return root;
}

/** Mostra a chave de recuperação uma única vez, depois do cadastro por convite. */
export function createRecoveryNotice(key: string, done: () => void): HTMLElement {
  const copy = el("button", { class: "btn", type: "button" }, svg(icon("copy", "sm")), "Copiar chave");
  copy.addEventListener("click", () => { void copyText(key).then((ok) => { copy.lastChild!.textContent = ok ? "Copiado" : "Selecione e copie"; }); });
  const ok = el("button", { class: "btn primary", type: "button", textContent: "Guardei, continuar" });
  ok.addEventListener("click", done);
  return el(
    "div",
    { class: "stack loose fade-in" },
    el("div", { class: "stack tight" }, el("span", { class: "eyebrow", textContent: "Conta criada" }), el("h2", {}, "Guarde sua chave de recuperação")),
    el("p", { class: "muted", textContent: "É a única forma de entrar de novo em outro computador com nome de usuário. Ela não será mostrada outra vez." }),
    el("div", { class: "keybox mono recovery-key", textContent: key }),
    el("div", { class: "row" }, copy, ok),
  );
}

// ---- Amigos ------------------------------------------------------------------------------------

export interface FriendsPanel {
  root: HTMLElement;
  update(snapshot: Snapshot): void;
  setConnected(connected: boolean): void;
  dispose(): void;
}

export function createFriends(client: MadClient, handlers: { onWatch(friend: FriendView): void }): FriendsPanel {
  const alerts = el("div", { class: "stack tight" });
  const say = (kind: "error" | "info" | "ok", text: string): void => alerts.replaceChildren(banner(kind, text));
  const lists = el("div", { class: "stack loose" });
  const status = el("span", { class: "small faint", textContent: "Conectando…" });
  let current: Snapshot | null = null;
  let fingerprint = "";
  let disposed = false;
  let ageTimer: ReturnType<typeof setInterval> | undefined;
  const liveTimes = new Map<HTMLElement, number>();

  function refreshAges(): void {
    if (document.hidden || root.closest("[hidden]")) return;
    for (const [label, since] of liveTimes) {
      const text = ago(since);
      if (label.textContent !== text) label.textContent = text;
    }
  }

  const usernameInput = el("input", { class: "input", placeholder: "Nome de usuário", spellcheck: false, maxLength: 21 });
  const addButton = el("button", { class: "btn", type: "submit" }, svg(icon("userPlus", "sm")), "Adicionar");
  const addForm = el("form", { class: "row nowrap" }, el("div", { class: "grow" }, usernameInput), addButton);
  addForm.addEventListener("submit", (event) => {
    event.preventDefault();
    if (!usernameInput.value.trim()) return;
    addButton.disabled = true;
    client.requestFriend(usernameInput.value)
      .then((snapshot) => { usernameInput.value = ""; update(snapshot); say("ok", "Pedido enviado."); })
      .catch((error: unknown) => say("error", errorText(error)))
      .finally(() => { addButton.disabled = false; });
  });

  const inviteButton = el("button", { class: "btn block", type: "button" }, svg(icon("link", "sm")), "Copiar link de convite");
  inviteButton.addEventListener("click", () => {
    inviteButton.disabled = true;
    client.createInvite()
      .then(async (invite) => {
        const copied = await copyText(invite.link);
        say("ok", copied ? `Link copiado (vale por 7 dias, até ${invite.maxUses} pessoas). Código: ${invite.code}` : `Envie este link: ${invite.link}`);
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
    const row = el("div", { class: "list-row" }, avatar(friend, friend.online), el("div", { class: "grow" }, name, sub));
    if (live) {
      const watch = el("button", { class: "btn primary sm", type: "button" }, svg(icon("play", "sm")), "Assistir");
      watch.addEventListener("click", () => handlers.onWatch(friend));
      row.append(watch);
    }
    const remove = el("button", { class: "btn ghost sm row-remove", type: "button", title: "Remover amigo", ariaLabel: `Remover ${friend.displayName}` }, svg(icon("x", "sm")));
    remove.addEventListener("click", () => {
      if (!confirm(`Remover ${friend.displayName} dos amigos?`)) return;
      client.removeFriend(friend.id).then(update).catch((error: unknown) => say("error", errorText(error)));
    });
    row.append(remove);
    return row;
  }

  function requestRow(user: UserView, incoming: boolean): HTMLElement {
    const row = el("div", { class: "list-row" }, avatar(user), el("div", { class: "grow" }, el("div", { class: "name", textContent: user.displayName }), el("div", { class: "sub", textContent: `@${user.username}${incoming ? " quer ser seu amigo" : " · pedido enviado"}` })));
    if (incoming) {
      const accept = el("button", { class: "btn primary sm", type: "button", textContent: "Aceitar" });
      const decline = el("button", { class: "btn sm", type: "button", textContent: "Recusar" });
      accept.addEventListener("click", () => { client.acceptFriend(user.id).then(update).catch((e: unknown) => say("error", errorText(e))); });
      decline.addEventListener("click", () => { client.declineFriend(user.id).then(update).catch((e: unknown) => say("error", errorText(e))); });
      row.append(decline, accept);
    } else {
      const cancel = el("button", { class: "btn ghost sm", type: "button", textContent: "Cancelar" });
      cancel.addEventListener("click", () => { client.removeFriend(user.id).then(update).catch((e: unknown) => say("error", errorText(e))); });
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
    const nextFingerprint = JSON.stringify([snapshot.friends, snapshot.incoming, snapshot.outgoing]);
    if (fingerprint === nextFingerprint) {
      refreshAges();
      return;
    }
    fingerprint = nextFingerprint;
    liveTimes.clear();
    const live = snapshot.friends.filter((friend) => friend.live);
    const online = snapshot.friends.filter((friend) => !friend.live && friend.online);
    const offline = snapshot.friends.filter((friend) => !friend.live && !friend.online);
    const blocks = [
      section("Pedidos recebidos", snapshot.incoming.length, snapshot.incoming.map((user) => requestRow(user, true))),
      section("Ao vivo agora", live.length, live.map(friendRow)),
      section("Online", online.length, online.map(friendRow), snapshot.friends.length ? undefined : "Você ainda não tem amigos. Adicione por nome de usuário ou envie um link de convite."),
      section("Offline", offline.length, offline.map(friendRow)),
      section("Pedidos enviados", snapshot.outgoing.length, snapshot.outgoing.map((user) => requestRow(user, false))),
    ];
    lists.replaceChildren(...blocks.filter((block): block is HTMLElement => block !== null));
    if (live.length && ageTimer === undefined) ageTimer = setInterval(refreshAges, 30_000);
    if (!live.length) {
      clearInterval(ageTimer);
      ageTimer = undefined;
    }
  }

  const root = el(
    "div",
    { class: "stack loose" },
    el("div", { class: "row between" }, el("h2", {}, "Amigos"), status),
    el("div", { class: "stack tight" }, addForm, inviteButton),
    alerts,
    lists,
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
  watch(friend: { username: string; displayName: string }): void;
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
  /** O app Tauri leva a janela inteira para tela cheia; no navegador usa a Fullscreen API. */
  toggleFullscreen?(stage: HTMLElement): Promise<void>;
  onLeave?(): void;
  /** Prévia local: nunca assina áudio nem permite liberar o som. */
  preview?: boolean;
}

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
  const playerStats = el("span", { class: "player-stats mono", hidden: true });
  const fullscreen = el("button", { class: "btn sm", type: "button", title: "Tela cheia" }, svg(icon("fullscreen", "sm")));
  const diagnostic = el("button", { class: "btn sm", type: "button", textContent: "Copiar diagnóstico", title: "Copie logo após uma travadinha para registrar os últimos segundos.", disabled: true });
  const diagnosticHelp = el("p", { class: "small muted" }, "Travou ou apresentou ruído? Copie logo após o problema e peça o diagnóstico da transmissão ao amigo. Informe a versão dos dois apps e a fonte escolhida. Não foi possível determinar a causa apenas pelo estado da conexão.");
  const stateDetail = el("p", { class: "small muted", hidden: true });
  const guidance = el("div", { class: "stack tight", hidden: true });
  const explain = el("input", { type: "checkbox", checked: true });
  const refreshGuidance = () => {
    guidance.hidden = !explain.checked;
    guidance.replaceChildren(...(explain.checked ? diagnosticGuidance(session?.getDiagnostics() ?? null, performance.now(), document.hidden).map(text => el("p", { class: "small muted" }, text)) : []));
  };
  explain.addEventListener("change", refreshGuidance);
  const refresh = el("button", { class: "btn sm", type: "button", textContent: "Atualizar observações" });
  refresh.addEventListener("click", refreshGuidance);
  const diagnostics = el("details", { class: "watch-diagnostics" },
    el("summary", { textContent: "Detalhes e ajuda" }),
    el("div", { class: "stack tight mt" }, hud, stateDetail, diagnosticHelp,
      el("label", { class: "row" }, explain, "Mostrar observações do último intervalo"),
      el("p", { class: "small muted" }, "Retrato ao abrir a ajuda ou escolher Atualizar observações; não acompanha a transmissão continuamente."), guidance, refresh, diagnostic),
  );
  const info = el("button", { class: "btn sm", type: "button", textContent: "Ajuda" });
  info.setAttribute("aria-expanded", "false");
  info.addEventListener("click", async () => {
    if (document.fullscreenElement === stage) {
      try { await document.exitFullscreen(); } catch { return; }
    }
    diagnostics.open = !diagnostics.open;
    if (diagnostics.open) diagnostics.querySelector("summary")?.focus();
  });
  diagnostics.addEventListener("toggle", () => { info.setAttribute("aria-expanded", String(diagnostics.open)); if (diagnostics.open) refreshGuidance(); });
  let diagnosticReset: ReturnType<typeof setTimeout> | undefined;
  const volume = el("input", { class: "volume", type: "range", min: "0", max: "100", step: "1", value: String(Math.round(audioPref.volume * 100)) });
  volume.setAttribute("aria-label", "Volume");
  const mute = el("button", { class: "btn sm", type: "button" }, svg(icon("volume", "sm")));
  const unlock = el("button", { class: "audio-unlock", type: "button", hidden: true }, svg(icon("volumeOff", "sm")), "Clique para ativar o som");
  const stage = el("div", { class: "stage" }, video, message, unlock, el("div", { class: "controls" }, el("div", { class: "audio-ctl" }, volume, mute), playerStats, info, fullscreen));
  stage.dataset.theme = videoTheme();
  const title = el("h2", { textContent: "Assistir" });
  const badge = el("span", { class: "badge", hidden: true });
  const playerTop = el("div", { class: "player-top" }, el("div", { class: "who" }, title, badge, audioStatus));
  stage.append(playerTop);
  const popout = el("button", { class: "btn sm", type: "button", textContent: "Mini-player", hidden: !options.popout });
  popout.setAttribute("data-main-window-only", "");
  info.setAttribute("data-main-window-only", "");
  popout.addEventListener("click", async () => {
    if (!options.popout || !session || popout.disabled) return;
    const generation = ++detachGeneration;
    popout.disabled = true;
    try {
      const handle = await options.popout(stage, displayName);
      if (generation !== detachGeneration || !session) handle.close();
      else detached = handle;
    } catch (error) {
      stateDetail.textContent = error instanceof Error ? error.message : "Não foi possível abrir o mini-player.";
      stateDetail.hidden = false;
    } finally { popout.disabled = false; }
  });
  if (options.popout) stage.querySelector(".controls")?.append(popout);
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
      mute.replaceChildren(svg(icon(soundIcon, "sm")));
    }
    mute.disabled = !audioPresent || selfView;
    volume.disabled = !audioPresent || selfView;
    const label = selfView ? "Sua própria transmissão fica sempre sem som" : !audioPresent ? "Esta transmissão não tem áudio" : silent ? "Ativar o som" : "Silenciar";
    mute.title = label;
    mute.setAttribute("aria-label", `${label}: ${displayName}`);
    volume.value = String(Math.round(audioPref.volume * 100));
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
  const leave = el("button", { class: "btn sm", type: "button", textContent: "Sair da transmissão", hidden: true });
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
  stage.addEventListener("dblclick", () => fullscreen.click());
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
    `${s.width}×${s.height} · ${s.fps.toFixed(0)} FPS · ${s.mbps.toFixed(1)} Mbps${s.rttMs !== null ? ` · RTT ${s.rttMs.toFixed(0)} ms` : ""}`;

  const panel: WatchPanel = {
    getDiagnostics: () => session?.getDiagnostics() ?? null,
    root: el("div", { class: "stack fade-in watch-panel" }, el("div", { class: "row between watch-panel-head" }, el("div", { class: "row" }, retry, leave)), selfNotice, stage, diagnostics),
    setBackgroundMuted(muted) { if (backgroundMuted !== muted) { backgroundMuted = muted; applyAudio(); } },
    watch(friend) {
      panel.stop();
      title.textContent = friend.displayName;
      displayName = friend.displayName;
      panel.root.setAttribute("aria-label", `Transmissão de ${displayName}`);
      volume.setAttribute("aria-label", `Volume: ${displayName}`);
      fullscreen.setAttribute("aria-label", `Tela cheia: ${displayName}`);
      leave.setAttribute("aria-label", `Sair da transmissão: ${displayName}`);
      diagnostic.setAttribute("aria-label", `Copiar diagnóstico: ${displayName}`);
      info.setAttribute("aria-label", `Ajuda: ${displayName}`);
      selfView = options.preview === true || friend.username.toLowerCase() === client.session?.user.username.toLowerCase();
      selfNotice.hidden = !selfView;
      audioKey = `${AUDIO_KEY}.${encodeURIComponent(client.session?.user.id ?? "anonymous")}.${encodeURIComponent(friend.username.toLowerCase())}`;
      audioPref = loadAudioPref(audioKey);
      setAudioPresent(false);
      session = new WatchSession(client, friend.username, video, {
        onState: (state, detail) => {
          show(state, detail);
        },
        onAudio: setAudioPresent,
        onAutoplayBlocked: () => { autoplayBlocked = true; applyAudio(); },
        onStats: (stats) => {
          hud.textContent = formatStats(stats);
          playerStats.textContent = hud.textContent;
          hud.title = `Último intervalo: perda ${stats.lossPct?.toFixed(2) ?? "—"}% · buffer de vídeo ${stats.jitterBufferMs?.toFixed(1) ?? "—"} ms`;
        },
      }, { audio: !selfView });
      session.start();
    },
    stop() {
      detachGeneration++;
      detached?.close(); detached = null;
      clearTimeout(diagnosticReset);
      diagnostic.textContent = "Copiar diagnóstico";
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
  watch(friend: Pick<FriendView, "username" | "displayName">): void;
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
  const controls = el("div", { class: "row wrap" });
  const add = el("button", { class: "btn sm", type: "button", textContent: "Adicionar transmissão" });
  const swap = el("button", { class: "btn sm", type: "button", textContent: "Trocar destaque", title: "Trocar destaque (D)" });
  const equal = el("button", { class: "btn sm", type: "button", textContent: "Lado a lado" });
  const audioMode = el("input", { type: "checkbox" });
  const audioLabel = el("label", { class: "row small" }, audioMode, "Som só no destaque");
  add.addEventListener("click", () => options.onAdd?.());
  const hint = el("p", { class: "small faint", textContent: "Com duas transmissões abertas, a próxima substitui a tela selecionada." });
  const root = el("div", { class: "watch-layout stack" }, controls, hint, grid);
  const update = (): void => {
    const count = users.filter(Boolean).length;
    if (count < 2) featured = null;
    grid.classList.toggle("split", count === 2);
    grid.classList.toggle("featured-first", count === 2 && featured === 0);
    grid.classList.toggle("featured-second", count === 2 && featured === 1);
    swap.hidden = equal.hidden = audioLabel.hidden = count < 2;
    equal.setAttribute("aria-pressed", String(featured === null));
    hint.hidden = count < 2;
    hint.textContent = `Selecionada: Tela ${selected + 1} · ${names[selected]}. A próxima transmissão substitui esta tela. Atalhos: M para som, F para tela cheia e D para destaque.`;
    add.hidden = count === 0 || !options.onAdd;
    for (let index = 0; index < 2; index++) {
      slots[index]!.hidden = users[index] === null;
      slots[index]!.classList.toggle("selected", index === selected);
      selectors[index]!.hidden = users[index] === null;
      selectors[index]!.textContent = count === 1 ? "Assistindo · 1 de 2" : `Tela ${index + 1}`;
      selectors[index]!.setAttribute("aria-pressed", String(index === selected));
      selectors[index]!.setAttribute("aria-label", `Selecionar Tela ${index + 1}: ${names[index]}${index === selected ? " (selecionada)" : ""}`);
      slots[index]!.setAttribute("aria-label", `Tela ${index + 1}: ${names[index]}${index === selected ? " (selecionada)" : ""}`);
      panels[index]!.setBackgroundMuted(count === 2 && onlyFeaturedAudio && index !== (featured ?? selected));
    }
  };
  const swapFeature = (): void => {
    featured = featured === null ? selected : 1 - featured;
    selected = featured;
    update();
  };
  swap.addEventListener("click", swapFeature);
  equal.addEventListener("click", () => { featured = null; update(); });
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
    const selector = el("button", { class: "btn sm", type: "button", textContent: `Tela ${index + 1}` });
    selector.addEventListener("click", () => { selected = index; update(); });
    selectors.push(selector); controls.append(selector);
    const panel = createWatchPanel(client, {
      ...(options.popout ? { popout: options.popout } : {}),
      ...(options.toggleFullscreen ? { toggleFullscreen: options.toggleFullscreen } : {}),
      ...(options.preview !== undefined ? { preview: options.preview } : {}),
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
    slot.addEventListener("pointerdown", () => { selected = index; update(); });
    slot.addEventListener("focusin", () => { selected = index; update(); });
    slots.push(slot); grid.append(slot);
  }
  controls.append(swap, equal, audioLabel, add);
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
