import {
  changeShare, errorText, getPublisherDiagnostics, getSettings, getState, listMonitors, listSources, outputOptions, setSharePrivacy, startShareCloudflare, stopShare,
  type EncoderLimits, type OutputOption, type Phase, type PickedSource, type Settings, type ShareState,
} from "../api";
import { connectStreamEvents, type StreamEvent, type InviteDestination, type FriendView, type Snapshot } from "@shared/client";
import { createWatchPanel, type WatchPanel } from "@shared/ui";
import { banner, el, segmented, type View } from "../dom";
import { client, WORKER_URL } from "../session";
import { pickSource } from "./picker";
import { toggleFullscreen } from "./fullscreen";
import { createJoinNotice, createJoinSound } from "../join-sound";
import { defaultFps,historyKey,parseHistory,confirmedPreferenceFor,rememberApp,validateWindow } from "../share-history";

const PHASE_LABEL: Record<Phase, string> = {
  idle: "Parado",
  starting: "Iniciando…",
  picking: "Escolha o que compartilhar",
  live: "Ao vivo",
  stopping: "Parando…",
};

type ShareTarget = InviteDestination | { sessionId: string; peerId: string; peerName: string };
const targetKey = (target: ShareTarget | null): string => target ? "sessionId" in target ? target.sessionId : target.channelId : "";
const targetLabel = (target: ShareTarget): string => "sessionId" in target ? `Com ${target.peerName} · Somente ${target.peerName} poderá assistir` : `${target.serverName} / ${target.channelName}`;

const HEIGHT_CHOICES = [720, 1080, 1440, 2160, 0];
/** Megabits por segundo com vírgula decimal, como o restante do app em pt-BR. */
const mbps = (bps: number): string => (bps / 1_000_000).toLocaleString("pt-BR", { minimumFractionDigits: 1, maximumFractionDigits: 1 });

function fallbackOptions(): OutputOption[] {
  return HEIGHT_CHOICES.map((choice) => ({
    choice, label: choice === 0 ? "Nativa" : `${choice}p`, available: true,
    width: 0, height: 0, reduced: false, ultrawide: false, note: null,
  }));
}

function optionTitle(option: OutputOption): string {
  return option.label.replace(/\s+\d+\s*[x×]\s*\d+\s*$/i, "");
}

export interface ShareView extends View {
  update(state: ShareState): void;
  cancelPending(): void;
  updateDirectory(snapshot: Snapshot): void;
  startInChannel(target: InviteDestination): void;
  startWithFriend(friend: FriendView): void;
}

export function createShareView(): ShareView {
  let height = 1080;
  let fps = 60;
  let phase: Phase = "idle";
  let changing = false;
  let lastSession: number | undefined;
  let confirmedHeight = 1080;
  let confirmedFps = 60;
  let confirmedReference: typeof reference = null;
  let preparing = false;
  let disposed = false;
  let pendingSource: AbortController | null = null;
  let directStartVersion = 0;
  let heightEdited = false;
  let fpsEdited = false;
  let savedSettings: Settings | null = null;
  let gpuInfo: { nvenc_available?: boolean | null; gpu_vendor_id?: number | null } | null = null;
  let reference: { name: string; width: number; height: number } | null = null;
  let selectedSource: PickedSource | null = null;
  let retryStart = false;
  let confirmedSound: string | null = null;
  let activeDestination: ShareTarget | null = null;
  let latestState: ShareState | null = null;
  let limits: EncoderLimits | null = null;
  let options = fallbackOptions();
  let optionsToken = 0;
  let pendingOptions = Promise.resolve();
  let adjustment: string | null = null;
  let channel: ShareTarget | null = null;
  let channels: InviteDestination[] = [];
  let preferredChannel = "";
  const channelKey = `ms.share.channel.${client.session?.user.id ?? "anonymous"}`;
  const appHistoryKey=historyKey(client.session?.user.id??"anonymous");
  const readHistory=()=>{try{return parseHistory(localStorage.getItem(appHistoryKey));}catch{return [];}};
  let profileChangePending=false;
  function saveCurrentApp(state=latestState){
    if(!selectedSource||!state||profileChangePending)return;
    const preference=confirmedPreferenceFor(selectedSource,state);
    if(!preference)return;
    try{localStorage.setItem(appHistoryKey,JSON.stringify({version:1,apps:rememberApp(readHistory(),preference)}));}catch{/* Sem armazenamento. */}
  }
  async function revalidate(source:PickedSource,signal:AbortSignal){
    if(source.kind!=="window_handle"||!source.instanceId)return;
    const list=await listSources();
    if(signal.aborted)throw Error("Escolha cancelada.");
    if(!validateWindow(source,list.windows))throw Error("A janela foi fechada ou substituída. Escolha a fonte novamente.");
  }
  try { preferredChannel = localStorage.getItem(channelKey) ?? ""; } catch { /* Sem armazenamento. */ }
  const channelSelect = el("select", { class: "input" });
  channelSelect.setAttribute("aria-label", "Canal da transmissão");
  channelSelect.addEventListener("change", () => {
    channel = channels.find(item => item.channelId === channelSelect.value) ?? null;
    preferredChannel = targetKey(channel);
    try { localStorage.setItem(channelKey, preferredChannel); } catch { /* Sem armazenamento. */ }
    syncControls();
  });

  const resolution = el("div", { class: "res-grid", role: "radiogroup" });
  resolution.setAttribute("aria-label", "Resolução");
  const sourceSummary = el("p", { class: "muted small", hidden: true });
  const resolutionAlerts = el("div", { class: "alerts res-alerts" });
  const resolutionButtons = new Map<number, {
    button: HTMLButtonElement;
    title: HTMLElement;
    dimensions: HTMLElement;
    reduced: HTMLElement;
    note: HTMLElement;
  }>();

  function selectResolution(choice: number) {
    height = choice;
    heightEdited = true;
    adjustment = null;
    renderResolution();
  }

  for (const choice of HEIGHT_CHOICES) {
    const title = el("span", { class: "res-title" });
    const dimensions = el("span", { class: "res-dimensions" });
    const reduced = el("span", { class: "badge res-reduced", textContent: "reduzida" });
    const note = el("span", { class: "res-note" });
    const optionButton = el("button", { class: "res-option", type: "button" }, title, dimensions, reduced, note);
    optionButton.setAttribute("role", "radio");
    optionButton.addEventListener("click", () => selectResolution(choice));
    optionButton.addEventListener("keydown", (event) => {
      if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
      const enabled = [...resolutionButtons.entries()].filter(([, item]) => !item.button.disabled);
      if (!enabled.length) return;
      event.preventDefault();
      const index = enabled.findIndex(([value]) => value === choice);
      const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? enabled.length - 1
        : (index + (event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1) + enabled.length) % enabled.length;
      const [nextChoice, next] = enabled[nextIndex];
      selectResolution(nextChoice);
      next.button.focus();
    });
    resolutionButtons.set(choice, { button: optionButton, title, dimensions, reduced, note });
    resolution.append(optionButton);
  }

  const rate = segmented(
    [
      { value: 30, label: "30 FPS" },
      { value: 60, label: "60 FPS" },
    ],
    fps,
    (value) => {
      fps = value;
      fpsEdited = true;
      void recalculateOptions();
    },
  );
  rate.root.setAttribute("aria-label", "Taxa de quadros");

  function renderResolution() {
    const selected = options.find((option) => option.choice === height);
    const tabChoice = selected?.available ? height : options.find((option) => option.available)?.choice;
    for (const option of options) {
      const item = resolutionButtons.get(option.choice);
      if (!item) continue;
      item.title.textContent = optionTitle(option);
      item.dimensions.textContent = option.width > 0 && option.height > 0 ? `${option.width}x${option.height}` : "";
      item.dimensions.hidden = !item.dimensions.textContent;
      item.reduced.hidden = !option.reduced;
      item.note.textContent = !option.available ? option.note : "";
      item.note.hidden = !item.note.textContent;
      item.button.title = option.note ?? "";
      item.button.setAttribute("aria-checked", String(option.choice === height));
      item.button.disabled = !canEditQuality() || !option.available;
      item.button.tabIndex = option.choice === tabChoice ? 0 : -1;
    }
    sourceSummary.hidden = !reference;
    sourceSummary.textContent = reference
      ? `Opções para: ${reference.name} · ${reference.width}x${reference.height}` : "";
    resolutionAlerts.replaceChildren();
    if (adjustment) resolutionAlerts.append(banner("info", adjustment));
    if (selected?.note) resolutionAlerts.append(banner("info", selected.note));
    syncChangeControls();
    syncSummary();
  }

  function recalculateOptions(): Promise<void> {
    if (disposed) return Promise.resolve();
    const mine = ++optionsToken;
    const source = reference;
    pendingOptions = (async () => {
      let next = fallbackOptions();
      if (source) {
        try {
          next = await outputOptions(source.width, source.height, fps, limits);
        } catch {
          // Sem planejamento disponível, o engine ainda valida a escolha ao iniciar.
        }
      }
      if (disposed || mine !== optionsToken) return;
      options = next;
      adjustment = null;
      if (!options.find((option) => option.choice === height)?.available) {
        const lower = options.filter((option) => option.available && option.choice > 0 && option.choice < height)
          .sort((a, b) => b.choice - a.choice)[0];
        // Uma janela quase 1080p (ex.: 1056p sem a barra de tarefas) deve manter
        // seus pixels, em vez de cair para o proximo preset menor, como 720p.
        const replacement = options.find((option) => option.available && option.choice === 0) ?? lower
          ?? options.find((option) => option.available);
        if (replacement) {
          height = replacement.choice;
          if (source) adjustment = `Resolução ajustada para ${optionTitle(replacement)} porque a fonte tem apenas ${source.height}p (${source.width}x${source.height}).`;
        }
      }
      const focused = document.activeElement;
      renderResolution();
      if (focused instanceof HTMLButtonElement && resolution.contains(focused) && focused.disabled) {
        resolutionButtons.get(height)?.button.focus();
      }
    })();
    return pendingOptions;
  }

  function clearSourceReference() {
    if (!reference) return;
    reference = null;
    // Invalida também um cálculo pendente da fonte anterior.
    void recalculateOptions();
  }

  const viewers = el("p", { class: "muted small", hidden: true });
  viewers.setAttribute("role", "status");
  const joinSound = createJoinSound();
  let joinNotice = createJoinNotice(client.session?.user.id ?? "", () => joinSound.play());
  let events: { close(): void } | null = null;
  let eventsOwner = "";
  const watching = new Map<string, { uid: string; name: string }>();

  function renderViewers(count: number | null) {
    viewers.hidden = count === null;
    if (count === null) return;
    const people = new Map<string, { name: string; connections: number }>();
    for (const { uid, name } of watching.values()) {
      const person = people.get(uid);
      if (person) person.connections++;
      else people.set(uid, { name, connections: 1 });
    }
    const names = [...people.values()].map(({ name, connections }) => connections > 1 ? `${name} (${connections} conexões)` : name);
    viewers.textContent = count === 0
      ? "Ninguém assistindo ainda. Os membros do servidor veem sua live no canal."
      : `${people.size ? `${people.size} ${people.size === 1 ? "pessoa assistindo" : "pessoas assistindo"} · ` : ""}${count} ${count === 1 ? "conexão" : "conexões"}${names.length ? `: ${names.join(", ")}` : ""}`;
  }

  function trackViewers(live: boolean) {
    const owner = client.session?.user.username ?? "";
    if (!live || !owner) {
      events?.close();
      events = null;
      eventsOwner = "";
      watching.clear();
      renderViewers(null);
      return;
    }
    if (events && eventsOwner === owner) return;
    events?.close();
    eventsOwner = owner;
    joinNotice = createJoinNotice(client.session?.user.id ?? "", () => joinSound.play());
    renderViewers(0);
    events = connectStreamEvents(client, owner, (event: StreamEvent) => {
      if (disposed) return;
      joinNotice(event);
      if (event.participants) {
        watching.clear();
        for (const person of event.participants) watching.set(person.viewerId, { uid: person.uid, name: person.name });
      }
      if (event.type === "viewer_joined" && event.viewerId && event.uid && event.name) watching.set(event.viewerId, { uid: event.uid, name: event.name });
      if (event.type === "viewer_left" && event.viewerId) watching.delete(event.viewerId);
      if (typeof event.viewers === "number") {
        if (event.viewers === 0) watching.clear();
        renderViewers(event.viewers);
      }
    });
  }

  const button = el("button", { class: "btn primary big", type: "button", textContent: "Compartilhar" });
  const chooseSource = el("button", { class: "btn sm", type: "button", textContent: "Escolher janela ou tela" });
  const sourceName = el("p", { class: "share-source", textContent: "Nenhuma fonte selecionada" });
  const soundSummary = el("p", { class: "small muted" });
  const qualitySummary = el("p", { class: "small" });
  const destination = el("p", { class: "small muted", hidden: true });
  const qualityDetails = el("details", { class: "share-quality", hidden: true },
    el("summary", { textContent: "Alterar qualidade" }),
    el("div", { class: "stack mt" },
      el("div", { class: "res-field" }, el("span", { class: "label" }, "Resolução"), resolution, sourceSummary, resolutionAlerts),
      el("div", { class: "stack tight" }, el("span", { class: "label" }, "Taxa de quadros"), rate.root),
      el("p", { class: "small muted" }, "Nativa mantém o tamanho original. Resoluções menores reduzem os detalhes e os dados enviados. FPS é a quantidade de quadros por segundo."),
    ),
  );
  const hint = el("p", { class: "muted small" }, "Nada é transmitido antes da confirmação.");
  const alerts = el("div", { class: "alerts" });
  const applyQuality = el("button", { class: "btn primary block", type: "button", textContent: "Aplicar qualidade" });
  const changeSource = el("button", { class: "btn", type: "button", textContent: "Trocar janela ou tela" });
  const changeControls = el("div", { class: "share-apply", hidden: true }, applyQuality);
  const actions = el("div", { class: "share-actions stack tight" });
  const pauseVideo = el("button", { class: "btn", type: "button", textContent: "Pausar vídeo" });
  const muteAudio = el("button", { class: "btn", type: "button", textContent: "Silenciar áudio compartilhado" });
  const previewButton = el("button", { class: "btn", type: "button", textContent: "Mostrar prévia (sem som)", hidden: true });
  const privacyStatus = el("p", { class: "small", role: "status" });
  const privacyControls = el("div", { class: "stack tight", hidden: true },
    el("div", { class: "row center share-live-actions" }, previewButton, pauseVideo, muteAudio, changeSource), privacyStatus);
  let privacyRequestPending = false;
  async function requestPrivacy(video: boolean) {
    if (privacyRequestPending || !latestState?.privacy_supported || latestState.privacy_pending) return;
    privacyRequestPending = true;
    pauseVideo.disabled = muteAudio.disabled = true;
    try {
      await setSharePrivacy(video ? latestState.privacy_video !== "paused" : null,
        video ? null : !latestState.audio_muted);
    } catch (error) {
      alerts.replaceChildren(banner("error", errorText(error)));
    } finally {
      privacyRequestPending = false;
      syncPrivacy();
    }
  }
  pauseVideo.addEventListener("click", () => { void requestPrivacy(true); });
  muteAudio.addEventListener("click", () => { void requestPrivacy(false); });
  function syncPrivacy() {
    const state = latestState;
    privacyControls.hidden = phase !== "live";
    const pending = privacyRequestPending || state?.privacy_pending === true;
    const enabled = state?.privacy_supported === true && !pending && !changing && !preparing;
    pauseVideo.disabled = !enabled;
    muteAudio.disabled = !enabled || !state?.audio_active;
    pauseVideo.textContent = state?.privacy_video === "paused" ? "Retomar vídeo"
      : state?.privacy_video === "pausing" ? "Pausando…" : state?.privacy_video === "resuming" ? "Retomando…" : "Pausar vídeo";
    muteAudio.textContent = state?.audio_muted ? "Reativar áudio compartilhado" : "Silenciar áudio compartilhado";
    privacyStatus.textContent = !state?.privacy_supported ? "Atualize o engine para usar pausa segura."
      : pending ? "Aguardando confirmação do engine. Para encerrar imediatamente, use Parar."
        : state?.privacy_video === "paused" ? `Transmissão pausada. ${state.audio_active && !state.audio_muted ? "O áudio compartilhado continua ativo." : "Sem áudio compartilhado."}`
          : state?.audio_muted ? "Vídeo ativo; áudio compartilhado silenciado." : "";
  }
  function canEditQuality() { return ((phase === "idle" && !!selectedSource) || phase === "live") && !changing && !preparing
    && !latestState?.privacy_pending && (!latestState?.privacy_video || latestState.privacy_video === "active") && !latestState?.audio_muted; }
  function soundChoice(source: PickedSource): string {
    const audio = source.audio;
    if (source.kind === "picker") return "Som a confirmar após a escolha no Windows";
    if (!audio) return "Sem som";
    if (audio.system) return "Som do PC todo, exceto o madscreen";
    if (audio.follow_window) return "Som do aplicativo da janela";
    return `Som de ${audio.apps.length} ${audio.apps.length === 1 ? "aplicativo escolhido" : "aplicativos escolhidos"}`;
  }
  function syncSummary() {
    const active = phase !== "idle" || changing;
    const state = latestState;
    actions.classList.toggle("active", active);
    const current = options.find(option => option.choice === height);
    sourceName.textContent = active ? state?.source ?? confirmedReference?.name ?? "Preparando fonte…"
      : selectedSource?.name ?? (selectedSource?.kind === "picker" ? "Seletor do Windows" : "Nenhuma fonte selecionada");
    const size = active ? confirmedReference : reference;
    if (size) sourceName.textContent += ` · ${size.width}×${size.height}`;
    qualityDetails.hidden = !active && !selectedSource;
    qualitySummary.textContent = !active && !selectedSource
      ? "Escolha uma janela ou tela para conferir a qualidade."
      : active && state && state.width > 0
      ? `Em transmissão: ${state.width}×${state.height} · ${confirmedFps} FPS${height !== confirmedHeight || fps !== confirmedFps ? ` · Escolha pendente: ${current ? optionTitle(current) : height} · ${fps} FPS` : ""}`
      : `Qualidade: ${current ? optionTitle(current) : height} · ${fps} FPS`;
    soundSummary.textContent = active && state?.phase === "live"
      ? state.audio_active == null || state.audio_active === 0 ? "Sem áudio ativo. Confira a fonte e as opções de som ao trocar janela ou tela."
        : confirmedSound ?? `Som de ${state.audio_active} ${state.audio_active === 1 ? "fonte" : "fontes"}`
      : selectedSource ? soundChoice(selectedSource) : "Escolha a fonte e o som no seletor. Sem microfone.";
    destination.hidden = !active && !channel;
    destination.textContent = activeDestination ? `Transmitindo: ${targetLabel(activeDestination)}` : channel ? targetLabel(channel) : "Escolha com quem compartilhar";
    chooseSource.hidden = active;
    chooseSource.classList.toggle("primary", !selectedSource);
    chooseSource.textContent = selectedSource ? "Trocar fonte ou som" : "Escolher janela ou tela";
    chooseSource.disabled = preparing || !channel;
    button.textContent = active ? phase === "starting" && !changing ? "Cancelar" : "Parar"
      : selectedSource ? retryStart ? "Tentar novamente" : "Compartilhar" : "Compartilhar tela";
  }
  function syncChangeControls() {
    const qualityChanged = height !== confirmedHeight || fps !== confirmedFps;
    const hideApply = !changing && (phase !== "live" || !qualityChanged);
    if (hideApply && changeControls.contains(document.activeElement)) qualityDetails.querySelector("summary")?.focus();
    changeControls.hidden = hideApply;
    applyQuality.disabled = !canEditQuality() || !qualityChanged;
    changeSource.disabled = !canEditQuality();
    applyQuality.textContent = changing ? "Aplicando…" : "Aplicar qualidade";
  }
  function restoreConfirmedSelection() {
    height = confirmedHeight;
    fps = confirmedFps;
    reference = confirmedReference;
    rate.set(fps);
    return recalculateOptions();
  }
  async function applyChange(pickNewSource: boolean) {
    if (disposed || phase !== "live" || changing || preparing) return;
    const operation = new AbortController();
    pendingSource = operation;
    preparing = true;
    alerts.replaceChildren();
    syncControls();
    try {
      const source = pickNewSource ? await pickSource(operation.signal, { liveChange: true }) : undefined;
      if (disposed || operation.signal.aborted || (pickNewSource && !source)) return;
      if (source?.width && source.height) {
        reference = { name: source.name ?? "Fonte selecionada", width: source.width, height: source.height };
        await recalculateOptions();
      }
      await pendingOptions;
      if (disposed || operation.signal.aborted || phase !== "live") return;
      if(source)await revalidate(source,operation.signal);
      if(disposed||operation.signal.aborted||phase!=="live")return;
      profileChangePending=true;
      await changeShare(height, fps, source ?? undefined, fpsEdited);
      if (source && !disposed && !operation.signal.aborted) confirmedSound = soundChoice(source);
      if(!disposed&&!operation.signal.aborted&&(source?.instanceId||selectedSource?.instanceId)){
        // A alteração pode emitir eventos antes de resolver. Ler o supervisor evita
        // persistir qualidade de uma sessão antiga ou de uma troca cancelada.
        const current=await getState();
        const candidate=source??selectedSource;
        if(!disposed&&!operation.signal.aborted&&candidate&&confirmedPreferenceFor(candidate,current)){
          selectedSource=candidate;
          profileChangePending=false;
          saveCurrentApp(current);
        }
      }
    } catch (error) {
      if (disposed || operation.signal.aborted) return;
      await restoreConfirmedSelection();
      if (!disposed) alerts.replaceChildren(banner("error", errorText(error)));
    } finally {
      profileChangePending=false;
      if (pendingSource === operation) pendingSource = null;
      preparing = false;
      if (!disposed) syncControls();
    }
  }
  applyQuality.addEventListener("click", () => { void applyChange(false); });
  changeSource.addEventListener("click", () => { void applyChange(true); });
    previewButton.setAttribute("aria-expanded", "false");
  const previewHolder = el("div", { class: "share-preview", hidden: true });
  const previewStage = el("div", { class: "share-stage", hidden: true }, previewHolder);
  const previewNote = el("p", { class: "share-preview-note muted small", hidden: true },
    "A prévia sem som fica desligada para poupar o jogo. Abra quando quiser conferir a imagem.");
  let preview: WatchPanel | null = null;
  let previewOwner = "";
  let previewEnabled = false;
  let publishingConnected = false;

  function closePreview() {
    previewEnabled = false;
    previewOwner = "";
    preview?.stop();
    previewHolder.hidden = true;
    previewStage.hidden = true;
    previewNote.hidden = !publishingConnected || (!!latestState?.privacy_video && latestState.privacy_video !== "active");
    previewButton.textContent = "Mostrar prévia (sem som)";
    previewButton.setAttribute("aria-expanded", "false");
  }

  function syncPreview() {
    const user = client.session?.user;
    previewButton.hidden = !publishingConnected;
    if (latestState?.privacy_video && latestState.privacy_video !== "active") {
      closePreview();
      previewButton.hidden = true;
      return;
    }
    if (!publishingConnected || !user) {
      closePreview();
      return;
    }
    if (!previewEnabled || previewOwner === user.username) {
      previewNote.hidden = !previewHolder.hidden;
      return;
    }
    preview ??= createWatchPanel(client, { preview: true, toggleFullscreen, onLeave: closePreview });
    previewHolder.replaceChildren(preview.root);
    previewHolder.hidden = false;
    previewStage.hidden = false;
    previewNote.hidden = true;
    previewButton.textContent = "Ocultar prévia";
    previewButton.setAttribute("aria-expanded", "true");
    previewOwner = user.username;
    preview.watch(user);
  }

  previewButton.addEventListener("click", () => {
    if (previewEnabled) closePreview();
    else {
      previewEnabled = true;
      syncPreview();
    }
  });

  const badge = el("span", { class: "badge", textContent: PHASE_LABEL.idle });
  const fpsValue = el("div", { class: "stat-value", textContent: "—" });
  const bitrateValue = el("div", { class: "stat-value", textContent: "—" });
  const fpsSub = el("div", { class: "stat-sub", textContent: "FPS" });
  const bitrateSub = el("div", { class: "stat-sub", textContent: "Mbps" });
  const details = el("p", { class: "muted small" });
  const copyPublisher = el("button", { class: "btn ghost sm", type: "button", textContent: "Copiar diagnóstico da transmissão" });
  copyPublisher.addEventListener("click", async () => {
    copyPublisher.disabled = true;
    try {
      const report = await getPublisherDiagnostics();
      if (disposed) return;
      if (!report.samples.length) { alerts.replaceChildren(banner("info", "Ainda não há amostras. Inicie uma transmissão e aguarde alguns segundos.")); return; }
      await navigator.clipboard.writeText(JSON.stringify({ ...report, capturedAt: new Date().toISOString() }, null, 2));
      if (!disposed) alerts.replaceChildren(banner("ok", "Diagnóstico copiado. O relatório contém apenas contadores e tempos, sem imagem ou som."));
    } catch {
      if (!disposed) alerts.replaceChildren(banner("error", "Não foi possível copiar o diagnóstico da transmissão."));
    } finally { if (!disposed) copyPublisher.disabled = false; }
  });
  const encoderStatus = el("p", { class: "muted small", textContent: "Codificador: seleção automática" });
  encoderStatus.setAttribute("aria-live", "polite");

  async function prepareSource(quickStart = false) {
    if (disposed || preparing || phase !== "idle" || changing) return;
    const operation = new AbortController();
    pendingSource = operation;
    preparing = true;
    alerts.replaceChildren();
    syncControls();
    try {
      const selectedChannel = channel;
      const consentToken=client.session?.token;
      if (!selectedChannel) throw new Error("Escolha um amigo online ou um canal antes de transmitir.");
      await initialization;
      if (disposed || operation.signal.aborted) return;
      const source = await pickSource(operation.signal, { quickStart, recent:readHistory(),
        onRecent:preference=>{height=preference.height;fps=preference.fps;rate.set(fps);},
        removeRecent:appId=>{try{localStorage.setItem(appHistoryKey,JSON.stringify({version:1,apps:readHistory().filter(p=>p.appId!==appId)}));}catch{/* Sem armazenamento. */}},
        clearRecent:()=>{try{localStorage.removeItem(appHistoryKey);}catch{/* Sem armazenamento. */}} });
      if (!source || disposed || operation.signal.aborted) return;
      selectedSource = source;
      retryStart = false;
      reference = source.width && source.height
        ? { name: source.name ?? "Fonte selecionada", width: source.width, height: source.height } : null;
      await recalculateOptions();
      if (disposed || operation.signal.aborted) return;
      if (quickStart && !adjustment) {
        if (!client.session) throw new Error("Entre na sua conta para transmitir.");
        confirmedSound = soundChoice(source);
        activeDestination = selectedChannel;
        await revalidate(source,operation.signal);
        if(disposed||operation.signal.aborted||targetKey(channel)!==targetKey(selectedChannel)||!client.session||client.session.token!==consentToken)return;
        await startShareCloudflare(height, fps, source, WORKER_URL, client.token, source.audio, "sessionId" in selectedChannel ? undefined : selectedChannel.channelId, fpsEdited, "sessionId" in selectedChannel ? selectedChannel.sessionId : undefined);
      }
    } catch (error) {
      if (!disposed && !operation.signal.aborted) {
        retryStart = quickStart && !!selectedSource;
        alerts.replaceChildren(banner("error", errorText(error)));
      }
    } finally {
      if (pendingSource === operation) { pendingSource = null; preparing = false; }
      if (!disposed) syncControls();
    }
  }
  chooseSource.addEventListener("click", () => { void prepareSource(); });
  button.addEventListener("click", async () => {
    if (disposed) return;
    if (phase === "idle" && !changing && !selectedSource) { await prepareSource(); return; }
    if (phase === "idle" && !changing && preparing) return;
    alerts.replaceChildren();
    retryStart = false;
    const operation = new AbortController();
    try {
      if (phase !== "idle" || changing) {
        pendingSource?.abort();
        await stopShare();
      } else {
        const source = selectedSource, target = channel;
        const consentToken=client.session?.token;
        if (!source || !target || !client.session) throw new Error("Escolha uma fonte e um canal para transmitir.");
        pendingSource = operation;
        preparing = true;
        syncControls();
        await pendingOptions;
        if (disposed || operation.signal.aborted) return;
        await revalidate(source,operation.signal);
        if(disposed||operation.signal.aborted||targetKey(channel)!==targetKey(target)||!client.session||client.session.token!==consentToken)return;
        confirmedSound = soundChoice(source);
        activeDestination = target;
        await startShareCloudflare(height, fps, source, WORKER_URL, client.token, source.audio, "sessionId" in target ? undefined : target.channelId, fpsEdited, "sessionId" in target ? target.sessionId : undefined);
      }
    } catch (error) {
      if (!disposed && !operation.signal.aborted) {
        retryStart = phase === "idle" && !!selectedSource;
        alerts.replaceChildren(banner("error", errorText(error)));
      }
    } finally {
      if (pendingSource === operation) { pendingSource = null; preparing = false; }
      if (!disposed) syncControls();
    }
  });

  const step = (number: string, title: string, ...children: HTMLElement[]) => el("div", { class: `step share-step share-step-${number}` },
    el("span", { class: "num", ariaHidden: "true" }, number), el("div", { class: "body" }, el("h2", {}, title), ...children));
  const channelStep = step("1", "Canal", channelSelect, destination);
  const sourceStep = step("2", "Imagem e som",
    el("div", { class: "share-source-row" }, el("div", { class: "stack tight grow" }, sourceName, soundSummary), chooseSource),
    privacyControls);
  const qualityStep = step("3", "Qualidade", qualitySummary, qualityDetails);
  const root = el(
    "section",
    { class: "view share-view" },
    el("header", { class: "view-head" }, el("div", { class: "grow" }, el("h1", {}, "Minha transmissão"), el("p", { class: "muted" }, "Escolha o que seus amigos vão ver e ouvir.")), badge),
    el("div", { class: "share-layout" },
      el("section", { class: "share-side card steps", ariaLabel: "Configuração da transmissão" },
        channelStep, sourceStep, qualityStep, actions),
      el("div", { class: "share-main" },
        previewStage,
        previewNote,
        viewers,
        el("div", { class: "share-support" }, el("details", { class: "card stack share-diagnostics" },
          el("summary", { textContent: "Detalhes e diagnóstico" }),
          el("div", { class: "stats mt" }, el("div", { class: "stat" }, fpsValue, fpsSub), el("div", { class: "stat" }, bitrateValue, bitrateSub)),
          encoderStatus, details,
          el("p", { class: "small muted" }, "Travou ou apresentou ruído? Copie logo após o problema, junto com o diagnóstico de quem assiste. Informe a versão dos dois apps e a fonte escolhida."),
        ), copyPublisher),
      ),
    ),
  );

  actions.append(alerts, hint, changeControls, button);
  renderResolution();
  const initialization = Promise.all([
    getSettings().then((settings) => {
      if (disposed) return;
      if (!heightEdited) height = HEIGHT_CHOICES.includes(settings.quality_height) ? settings.quality_height : 1080;
      savedSettings = settings;
      applyDefaultFps();
      return recalculateOptions();
    }).catch(() => undefined),
    listMonitors().then((sources) => {
      if (disposed) return;
      limits = sources.encoder ?? null;
      gpuInfo = { nvenc_available: sources.nvenc_available, gpu_vendor_id: sources.gpu_vendor_id };
      applyDefaultFps();
      // Sem uma escolha no seletor, nenhuma tela ou janela está selecionada.
      return recalculateOptions();
    }).catch(() => undefined),
  ]).then(async () => { await pendingOptions; });

  /** As duas consultas chegam em qualquer ordem; só o seletor do usuário encerra o padrão. */
  function applyDefaultFps() {
    if (fpsEdited || !savedSettings) return;
    fps = defaultFps(savedSettings, gpuInfo);
    rate.set(fps);
  }

  function syncControls() {
    syncPrivacy();
    channelSelect.disabled = phase !== "idle" || preparing || !channels.length;
    for (const option of options) {
      const item = resolutionButtons.get(option.choice);
      if (item) item.button.disabled = !canEditQuality() || !option.available;
    }
    rate.setDisabled(!canEditQuality());
    button.disabled = !changing && (phase === "stopping" || (phase === "idle" && (preparing || !selectedSource || !channel)));
    syncChangeControls();
    syncSummary();
    channelStep.classList.toggle("done", !!channel);
    sourceStep.classList.toggle("done", !!selectedSource || phase === "live");
    qualityStep.classList.toggle("done", !!selectedSource || phase === "live");
    qualityStep.classList.toggle("locked", !selectedSource && phase === "idle");
  }

  function update(state: ShareState) {
    if (disposed) return;
    const previousPhase = phase;
    phase = state.phase;
    latestState = state;
    syncPrivacy();
    changing = state.changing === true;
    if ((phase === "stopping" || phase === "idle") && !changing && previousPhase !== "idle") pendingSource?.abort();
    if (phase === "idle" && !changing && previousPhase !== "idle") {
      retryStart = state.message_is_error === true && !!selectedSource;
      if (!retryStart) { selectedSource = null; clearSourceReference(); }
      confirmedSound = null; confirmedReference = null; activeDestination = null; lastSession = undefined;
    }
    if (phase === "live" && !changing && (previousPhase !== "live" || lastSession !== state.session_id)) {
      lastSession = state.session_id;
      const requested = state.quality_height ?? (HEIGHT_CHOICES.includes(state.height) ? state.height : 0);
      confirmedHeight = requested > state.source_height && state.source_height > 0 ? 0 : requested;
      confirmedFps = state.fps_target === 30 ? 30 : 60;
      height = confirmedHeight;
      fps = confirmedFps;
      saveCurrentApp();
      confirmedReference = state.source_width > 0 && state.source_height > 0
        ? { name: state.source ?? "Fonte atual", width: state.source_width, height: state.source_height } : reference;
      void restoreConfirmedSelection();
    }
    const busy = phase !== "idle" || changing;
    syncControls();
    const connected = phase === "live" && state.publishing && state.publishing_connected === true;
    publishingConnected = connected;
    syncPreview();
    badge.textContent = changing ? "Aplicando alterações…" : phase !== "live" ? PHASE_LABEL[phase] : !state.publishing ? "Captura local"
      : connected ? "Ao vivo" : state.publishing_connected === false ? "Reconectando…" : "Verificando conexão…";
    badge.dataset.phase = connected ? "live" : phase === "live" ? "starting" : phase;
    button.classList.toggle("danger", busy);
    button.classList.toggle("primary", !busy);
    hint.hidden = busy && phase !== "live" && !changing;
    hint.textContent = phase === "live" || changing
      ? "Alterar a qualidade ou a fonte pode causar uma breve reconexão para quem assiste."
      : "Nada é transmitido antes da confirmação.";
    trackViewers(connected);

    const live = phase === "live";
    encoderStatus.textContent = live
      ? `Codificador ativo: ${state.encoder ?? "não informado pelo engine"}`
      : phase === "starting" || phase === "picking" ? "Codificador: selecionando automaticamente…"
        : phase === "stopping" && state.encoder ? `Codificador: ${state.encoder} · encerrando…`
          : "Codificador: seleção automática";
    fpsValue.textContent = live ? state.fps.toFixed(0) : "—";
    bitrateValue.textContent = live ? mbps(state.bitrate_bps) : "—";
    fpsSub.textContent = live ? `FPS do encoder (alvo ${state.fps_target.toFixed(0)})` : "FPS";
    bitrateSub.textContent = live ? `Mbps do encoder (alvo ${mbps(state.bitrate_target_bps)})` : "Mbps";
    const output = `${state.width}x${state.height}`;
    const dimensions = state.source_width > 0 && state.source_height > 0
      && (state.source_width !== state.width || state.source_height !== state.height)
      ? `fonte ${state.source_width}x${state.source_height} → saída ${output}` : output;
    const codec = state.level != null ? ` · H.264 nível ${(state.level / 10).toFixed(1)}` : "";
    const sound = state.audio_active == null ? ""
      : state.audio_active === 0 ? " · sem som"
        : ` · som de ${state.audio_active} ${state.audio_active === 1 ? "fonte" : "fontes"}`;
    details.textContent = live
      ? `${state.source ?? "Fonte"} · ${dimensions}${codec}${sound} · ${
          !state.publishing ? "captura e encode locais, nada é enviado" : connected
            ? "conectado à transmissão" : state.publishing_connected === false
              ? "conexão interrompida; tentando reconectar automaticamente. Você pode parar e iniciar novamente."
              : "aguardando confirmação da conexão de mídia"
        }`
      : "";
    syncSummary();
    if (state.message) {
      const note = banner(state.message_is_error ? "error" : "info", state.message);
      if (state.diagnostic && state.message_is_error) {
        const text = state.diagnostic;
        const copy = el("button", { class: "btn ghost", type: "button", textContent: "Copiar diagnóstico" });
        copy.addEventListener("click", () => {
          void navigator.clipboard.writeText(text).then(
            () => { copy.textContent = "Diagnóstico copiado"; },
            () => { copy.textContent = "Não foi possível copiar"; },
          );
        });
        alerts.replaceChildren(note, copy);
      } else alerts.replaceChildren(note);
    }
  }

  return {
    root,
    update,
    cancelPending: () => { directStartVersion++; pendingSource?.abort(); if (!pendingSource) { preparing = false; syncControls(); } },
    updateDirectory(snapshot) {
      channels = (snapshot.servers ?? []).flatMap(server => server.channels.map(item => ({ serverId: server.id, serverName: server.name, channelId: item.id, channelName: item.name })));
      const previous = targetKey(channel) || preferredChannel;
      if (channel && "sessionId" in channel) {
        const session = snapshot.directSessions?.find(item => item.id === targetKey(channel));
        if (!session || session.reconnecting) {
          pendingSource?.abort();
          if (!session && phase !== "idle") void stopShare().catch(error => alerts.replaceChildren(banner("error", errorText(error))));
          if (!session) channel = null;
        }
      } else channel = channels.find(item => item.channelId === previous) ?? null;
      if (preparing && previous && !(channel && "sessionId" in channel) && !channels.some(item => item.channelId === previous)) pendingSource?.abort();
      channelSelect.replaceChildren(...channels.map(item => el("option", { value: item.channelId, textContent: `${item.serverName} / ${item.channelName}` })));
      if (channel && "sessionId" in channel) channelSelect.prepend(el("option", { value: channel.sessionId, textContent: targetLabel(channel) }));
      channelSelect.prepend(el("option", { value: "", textContent: "Escolha um canal ou abra Amigos" }));
      channelSelect.value = targetKey(channel);

      syncControls();
    },
    startWithFriend(friend) {
      if (phase !== "idle" || preparing) { alerts.replaceChildren(banner("info", "Pare a transmissão atual antes de mudar o público.")); return; }
      if (!friend.online) { alerts.replaceChildren(banner("info", "Seu amigo está offline.")); return; }
      const version = ++directStartVersion;
      const consentToken = client.session?.token;
      preparing = true; syncControls();
      const current = () => !disposed && version === directStartVersion && client.session?.token === consentToken;
      void (async () => {
        const health = await client.request<{ capabilities?: string[] }>("GET", "/health", undefined, false);
        if (!health.capabilities?.includes("direct-sessions")) throw new Error("O Hub precisa ser atualizado para compartilhar com amigos.");
        if (!current()) return;
        const result = await client.startDirect(friend.id);
        if (!current()) return;
        channel = { sessionId: result.sessionId, peerId: friend.id, peerName: friend.displayName };
        channelSelect.prepend(el("option", { value: result.sessionId, textContent: targetLabel(channel) }));
        channelSelect.value = result.sessionId;
        preparing = false; syncControls();
        await prepareSource(false);
      })().catch(error => { if (current()) alerts.replaceChildren(banner("error", errorText(error))); }).finally(() => { if (version === directStartVersion && !pendingSource) { preparing = false; if (!disposed) syncControls(); } });
    },
    startInChannel(target) {
      if (phase !== "idle" || preparing) { alerts.replaceChildren(banner("info", "Pare o compartilhamento atual antes de iniciar em outro canal.")); return; }
      channel = target;
      preferredChannel = target.channelId;
      channelSelect.value = target.channelId;
      try { localStorage.setItem(channelKey, preferredChannel); } catch { /* Sem armazenamento. */ }
      void prepareSource(true);
    },
    hide: closePreview,
    dispose() {
      disposed = true;
      directStartVersion++;
      joinSound.dispose();
      pendingSource?.abort();
      optionsToken++;
      events?.close();
      events = null;
      watching.clear();
      preview?.dispose();
    },
  };
}
