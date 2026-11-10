import { errorText, getForegroundSuggestion, listAudioApps, listSources, type AudioApp, type AudioChoice, type MonitorSource, type PickedSource, type SourceList, type WindowSource } from "../api";
import { icon as lineIcon } from "@shared/icons";
import { banner, el, segmented, svg } from "../dom";
import "./picker.css";
import { sourceForApp, type AppPreference } from "../share-history";

type Tab = "windows" | "monitors";

const dataUrl = (base64: string) => `data:image/png;base64,${base64}`;

function thumb(base64: string | null, placeholder: string): HTMLElement {
  const frame = el("div", { class: "pk-thumb" });
  if (base64) {
    frame.append(el("img", { src: dataUrl(base64), alt: "", draggable: false }));
  } else {
    frame.append(el("span", { class: "pk-nothumb", textContent: placeholder }));
  }
  return frame;
}

function card(content: HTMLElement[], label: string, onPick: () => void): HTMLButtonElement {
  const button = el("button", { class: "pk-card", type: "button", title: label }, ...content);
  button.setAttribute("aria-label", label);
  button.addEventListener("click", onPick);
  return button;
}

function audioLabel(mode:AppPreference["audio"]):string {
  return mode==="window"?"som da janela":mode==="system"?"som do PC":mode==="custom"?"revisar som":"sem som";
}
function windowCard(source: WindowSource, onPick: (choice: PickedSource) => void, preference?:AppPreference): HTMLButtonElement {
  const icon = source.icon_png_base64
    ? el("img", { class: "pk-icon", src: dataUrl(source.icon_png_base64), alt: "", draggable: false })
    : el("span", { class: "pk-icon pk-icon-empty" }, svg(lineIcon("logo")));
  const meta = el(
    "div",
    { class: "pk-meta" },
    el("div", { class: "pk-title", textContent: source.title }),
    el("div", { class: "pk-sub", textContent: `${source.process} · ${source.width}×${source.height}` }),
    preference?el("div",{class:"pk-sub",textContent:`Salvo: ${preference.height||"Nativa"} · ${preference.fps} FPS · ${audioLabel(preference.audio)}`}):null,
  );
  const button=card([thumb(source.thumbnail_png_base64, "Sem prévia"), el("div", { class: "pk-info" }, icon, meta)], source.title, () =>
    onPick({ kind: "window_handle", hwnd: source.hwnd, width: source.width, height: source.height, name: source.title,
      ...(source.app_id&&source.instance_id?{appId:source.app_id,instanceId:source.instance_id,processName:source.process,selectionId:source.selection_id??undefined}:{}) }),
  );
  if(source.selection_id!=null&&(!source.app_id||!source.instance_id)){
    button.disabled=true;
    button.title="Não foi possível verificar esta janela. Use o seletor do Windows.";
    button.append(el("span",{class:"pk-sub",textContent:"Identidade indisponível · use o seletor do Windows"}));
  }
  return button;
}

function monitorCard(source: MonitorSource, onPick: (choice: PickedSource) => void): HTMLButtonElement {
  const name = `Tela ${source.index + 1}${source.primary ? " (principal)" : ""}`;
  const meta = el(
    "div",
    { class: "pk-meta" },
    el("div", { class: "pk-title", textContent: name }),
    el("div", { class: "pk-sub", textContent: `${source.width}×${source.height}` }),
  );
  return card([thumb(source.thumbnail_png_base64, "Sem prévia"), el("div", { class: "pk-info" }, el("span", { class: "pk-icon pk-icon-empty" }, svg(lineIcon("monitor"))), meta)], name, () =>
    onPick({ kind: "monitor", index: source.index, width: source.width, height: source.height, name: `Tela ${source.index + 1}` }),
  );
}

const AUDIO_KEY = "ms.audio.pick";
const AUDIO_POLL_MS = 3000;

interface AudioPrefs {
  system: boolean;
  windowAudio: boolean;
  apps: string[];
}

function loadAudioPrefs(): AudioPrefs {
  try {
    const value = JSON.parse(localStorage.getItem(AUDIO_KEY) ?? "null") as Partial<AudioPrefs> | null;
    if (value && typeof value === "object") {
      return {
        system: value.system === true,
        windowAudio: value.windowAudio !== false,
        apps: Array.isArray(value.apps) ? value.apps.filter((name): name is string => typeof name === "string") : [],
      };
    }
  } catch {
    // Preferência corrompida: volta ao padrão.
  }
  return { system: false, windowAudio: true, apps: [] };
}

function saveAudioPrefs(prefs: AudioPrefs): void {
  try { localStorage.setItem(AUDIO_KEY, JSON.stringify(prefs)); } catch { /* armazenamento indisponível */ }
}

function checkRow(label: string, hint: string, checked: boolean, onChange: (checked: boolean) => void, badge?: string): { root: HTMLLabelElement; input: HTMLInputElement } {
  const input = el("input", { type: "checkbox", checked });
  input.addEventListener("change", () => onChange(input.checked));
  const text = el("span", { class: "pk-check-text" }, el("span", { class: "pk-check-label", textContent: label }), hint ? el("span", { class: "pk-check-hint", textContent: hint }) : null);
  const root = el("label", { class: "pk-check" }, input, text, badge ? el("span", { class: "badge", textContent: badge }) : null);
  return { root, input };
}

function isDisabled(element: HTMLElement): boolean {
  return "disabled" in element && Boolean((element as HTMLButtonElement).disabled);
}

function captureOpener(): HTMLElement | null {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement)) return null;
  if (active === document.body || active === document.documentElement) return null;
  return active;
}

/** Foca o controle que abriu o seletor quando ele ainda está na página e habilitado. */
function restoreFocus(opener: HTMLElement | null): void {
  if (!opener) return;
  let observer: MutationObserver | undefined;
  const stop = (): void => {
    observer?.disconnect();
    observer = undefined;
  };
  const attempt = (): boolean => {
    if (!opener.isConnected) {
      stop();
      return true;
    }
    if (isDisabled(opener)) return false;
    opener.focus();
    stop();
    return true;
  };
  // Depois do evento atual e da retomada de quem desabilitou o botão.
  setTimeout(() => {
    if (attempt() || !document.body) return;
    observer = new MutationObserver(() => { attempt(); });
    observer.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["disabled"] });
  }, 0);
}

function focusableIn(root: ParentNode): HTMLElement[] {
  const selector = [
    "a[href]",
    "button:not([disabled])",
    "input:not([disabled]):not([type='hidden'])",
    "select:not([disabled])",
    "textarea:not([disabled])",
    "[tabindex]:not([tabindex='-1'])",
  ].join(", ");
  return [...root.querySelectorAll<HTMLElement>(selector)].filter((node) => {
    if (node.tabIndex < 0 || isDisabled(node)) return false;
    if (node.closest("[hidden], [inert], [aria-hidden='true']")) return false;
    const style = getComputedStyle(node);
    if (style.display === "none" || style.visibility === "hidden") return false;
    return node.getClientRects().length > 0;
  });
}

/**
 * Seletor próprio de janela ou tela. Resolve com a escolha, `{ kind: "picker" }` para usar o
 * seletor do Windows (também quando a listagem falha) ou `null` se o usuário cancelar.
 */
export function pickSource(signal?: AbortSignal, options: {
  quickStart?: boolean; liveChange?: boolean; recent?:AppPreference[]; onRecent?(preference:AppPreference):void; clearRecent?():void; removeRecent?(appId:string):void;
} = {}): Promise<PickedSource | null> {
  if (signal?.aborted) return Promise.resolve(null);
  return new Promise((resolve) => {
    const opener = captureOpener();
    let tab: Tab = "windows";
    let list: SourceList | null = null;
    let suggestion: {hwnd:number;instance_id:string}|null = null;
    let loading = false;
    let token = 0;
    let finished = false;
    const silenced: HTMLElement[] = [];
    const prefs = loadAudioPrefs();
    let audioApps: AudioApp[] | null = null;
    let audioError = false;
    let audioPrefilled = false;
    let audioTouched = false;
    const chosenApps = new Set<number>();
    let audioPoll: ReturnType<typeof setInterval> | undefined;

    const tabs = segmented<Tab>(
      [
        { value: "windows", label: "Aplicativos" },
        { value: "monitors", label: "Telas" },
      ],
      tab,
      (value) => {
        tab = value;
        render();
      },
    );
    const refresh = el("button", { class: "btn", type: "button" }, svg(lineIcon("refresh", "sm")), "Atualizar");
    const close = el("button", { class: "pk-close", type: "button", title: "Cancelar" }, svg(lineIcon("x", "sm")));
    close.setAttribute("aria-label", "Cancelar");
    const fallback = el("button", { class: "btn", type: "button", textContent: "Usar o seletor do Windows" });
    const body = el("div", { class: "pk-body" });
    const audioPanel = el("section", { class: "pk-audio" });
    audioPanel.setAttribute("aria-label", "Som da transmissão");
    const instructions = el("p", { class: "muted small", id: "pk-instructions", textContent: options.liveChange
      ? "Escolha o som antes da fonte. Ao escolher a fonte, imagem e som serão trocados na live."
      : options.quickStart
      ? "Escolha o som antes da fonte. Clique na fonte para transmitir com a qualidade salva."
      : "Escolha o som antes da fonte. Ao escolher a fonte, você volta para revisar e confirmar." });
    if (options.recent?.length) instructions.append(" O som salvo na fonte vale até você alterar o painel.");
    const scopeHint = el("p", { class: "pk-scope muted small" });

    const dialog = el(
      "div",
      { class: "pk-dialog", role: "dialog" },
      el("header", { class: "pk-head" }, el("h2", {}, "Escolha o que compartilhar"), close),
      el("div", { class: "pk-bar" }, tabs.root, refresh),
      el("div", { class: "pk-content" }, audioPanel, scopeHint, body),
      el("footer", { class: "pk-foot" }, instructions, fallback),
    );
    dialog.setAttribute("aria-modal", "true");
    dialog.setAttribute("aria-label", "Escolha o que compartilhar");
    dialog.setAttribute("aria-describedby", instructions.id);
    const overlay = el("div", { class: "pk-overlay" }, dialog);

    /** O som que acompanha a escolha: o app da janela, ou os apps/PC marcados para uma tela. */
    function audioFor(choice: PickedSource): AudioChoice | undefined {
      if (choice.kind === "window_handle") return prefs.windowAudio ? { system: false, apps: [], follow_window: true } : undefined;
      if (choice.kind !== "monitor") return undefined;
      if (prefs.system) return { system: true, apps: [], follow_window: false };
      const apps = (audioApps ?? []).filter((app) => chosenApps.has(app.pid)).map((app) => app.pid);
      return apps.length > 0 ? { system: false, apps, follow_window: false } : undefined;
    }
    function pick(choice: PickedSource) {
      const preference = choice.kind === "window_handle" && choice.appId
        ? options.recent?.find(item => item.appId === choice.appId) : undefined;
      if(preference?.audio==="custom"&&!audioTouched){reviewCustomAudio();return;}
      if (preference) options.onRecent?.(preference);
      const audio = preference&&!audioTouched?{
        system:preference.audio==="system",follow_window:preference.audio==="window",apps:[],
      }:audioFor(choice);
      finish(audio ? { ...choice, audio } : choice);
    }
    function reviewCustomAudio(){
      body.prepend(banner("info","As outras fontes de som salvas não são reabertas automaticamente. Revise o controle de som abaixo e escolha a janela novamente."));
      audioPanel.querySelector<HTMLInputElement>('input')?.focus();
    }
    function pickRecent(preference:AppPreference) {
      if(!list)return;
      const source=sourceForApp(preference,list.windows);
      if(!source)return;
      pick(source);
    }

    function persistApps() {
      prefs.apps = (audioApps ?? []).filter((app) => chosenApps.has(app.pid)).map((app) => app.process_name);
      saveAudioPrefs(prefs);
    }

    function renderAudio() {
      if (tab === "windows") {
        const row = checkRow("Compartilhar também o som do aplicativo", "Só o áudio da janela escolhida, não o resto do PC.", prefs.windowAudio, (value) => {
          audioTouched = true;
          prefs.windowAudio = value;
          saveAudioPrefs(prefs);
        });
        audioPanel.replaceChildren(el("h3", { class: "pk-audio-title" }, "Som"), row.root);
        return;
      }
      const system = checkRow("Som do PC todo", "Tudo que toca no PC, menos o próprio madscreen.", prefs.system, (value) => {
        const focused = document.activeElement === system.input;
        prefs.system = value;
        saveAudioPrefs(prefs);
        renderAudio();
        if (focused) audioPanel.querySelector<HTMLInputElement>("input")?.focus();
      });
      const list = el("div", { class: "pk-audio-list" });
      if (audioError) {
        list.append(el("p", { class: "muted small", textContent: "Não foi possível listar os aplicativos com som." }));
      } else if (audioApps === null) {
        list.append(el("p", { class: "muted small", textContent: "Procurando aplicativos com som…" }));
      } else if (audioApps.length === 0) {
        list.append(el("p", { class: "muted small", textContent: "Nenhum aplicativo está tocando som agora. Abra o aplicativo e ele aparece aqui." }));
      } else {
        for (const app of audioApps) {
          const row = checkRow(app.display_name, app.process_name, chosenApps.has(app.pid) && !prefs.system, (value) => {
            if (value) chosenApps.add(app.pid); else chosenApps.delete(app.pid);
            persistApps();
          }, app.active ? "tocando" : undefined);
          row.input.disabled = prefs.system;
          list.append(row.root);
        }
      }
      audioPanel.replaceChildren(
        el("h3", { class: "pk-audio-title" }, "Som: escolha os aplicativos que quer compartilhar"),
        system.root,
        list,
      );
    }

    async function loadAudio() {
      try {
        const apps = await listAudioApps();
        if (finished) return;
        audioError = false;
        audioApps = apps;
        if (!audioPrefilled) {
          audioPrefilled = true;
          for (const app of apps) if (prefs.apps.includes(app.process_name)) chosenApps.add(app.pid);
        }
      } catch {
        if (finished) return;
        audioError = true;
      }
      // Não redesenha a lista enquanto o usuário usa um controle dela (perderia o foco).
      if (tab === "monitors" && !audioPanel.contains(document.activeElement)) renderAudio();
    }
    function syncAudioPoll() {
      if (tab === "monitors" && audioPoll === undefined) {
        audioPoll = setInterval(() => void loadAudio(), AUDIO_POLL_MS);
        void loadAudio();
      } else if (tab !== "monitors" && audioPoll !== undefined) {
        clearInterval(audioPoll);
        audioPoll = undefined;
      }
    }

    function finish(result: PickedSource | null) {
      if (finished) return;
      finished = true;
      if (audioPoll !== undefined) clearInterval(audioPoll);
      token++;
      signal?.removeEventListener("abort", cancel);
      document.removeEventListener("keydown", onKey, true);
      for (const node of silenced) node.inert = false;
      silenced.length = 0;
      overlay.remove();
      resolve(result);
      restoreFocus(opener);
    }
    function cancel() { finish(null); }
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        finish(null);
        return;
      }
      if (event.key !== "Tab") return;
      const items = focusableIn(dialog);
      if (items.length === 0) {
        event.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      const inside = active instanceof Node && dialog.contains(active);
      if (event.shiftKey ? !inside || active === first : !inside || active === last) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      }
    }

    function render() {
      tabs.set(tab);
      scopeHint.textContent = tab === "windows"
        ? "Compartilhe uma janela. As outras ficam fora da transmissão."
        : "Tudo o que aparecer nesta tela poderá ser visto, inclusive notificações.";
      renderAudio();
      syncAudioPoll();
      refresh.disabled = loading;
      if (loading && !list) {
        body.replaceChildren(el("div", { class: "pk-empty" }, "Procurando janelas e telas…"));
        return;
      }
      if (!list) return;
      const items = tab === "windows" ? list.windows : list.monitors;
      if (items.length === 0 && !(tab==="windows"&&options.recent?.length)) {
        body.replaceChildren(
          el("div", { class: "pk-empty" }, tab === "windows" ? "Nenhuma janela aberta para compartilhar." : "Nenhuma tela encontrada."),
        );
        return;
      }
      const grid = el("div", { class: "pk-grid" });
      if (tab === "windows") for (const w of list.windows) grid.append(windowCard(w, pick,options.recent?.find(p=>p.appId===w.app_id)));
      else for (const m of list.monitors) grid.append(monitorCard(m, pick));
      body.replaceChildren(grid);
      if(tab==="windows"&&suggestion){
        const candidate=list.windows.find(w=>w.hwnd===suggestion!.hwnd&&w.instance_id===suggestion!.instance_id);
        if(candidate){
          const suggested=el("section",{class:"pk-suggestion stack tight"},
            el("h3",{},"Aplicativo usado recentemente"),
            windowCard(candidate,pick,options.recent?.find(p=>p.appId===candidate.app_id)));
          suggested.setAttribute("aria-label","Aplicativo sugerido");
          body.prepend(suggested);
        }
      }
      if(tab==="windows"&&options.recent?.length){
        const recent=el("div",{class:"stack tight"},el("h3",{}, "Aplicativos recentes"));
        for(const preference of options.recent){
          const source=sourceForApp(preference,list.windows);
          const button=el("button",{class:"btn sm",type:"button",disabled:!source,
            textContent:`${preference.processName} · ${preference.height||"Nativa"} · ${preference.fps} FPS · ${preference.audio==="window"?"som da janela":preference.audio==="system"?"som do PC":preference.audio==="custom"?"revisar som":"sem som"}${source?"":" · escolha a janela abaixo"}`});
          button.addEventListener("click",()=>pickRecent(preference));
          const remove=el("button",{class:"btn ghost sm",type:"button",textContent:"Remover"});
          remove.setAttribute("aria-label",`Remover ${preference.processName} dos recentes`);
          remove.addEventListener("click",()=>{options.removeRecent?.(preference.appId);options.recent=options.recent?.filter(p=>p.appId!==preference.appId);render();});
          recent.append(el("div",{class:"row"},button,remove));
        }
        const clear=el("button",{class:"btn ghost sm",type:"button",textContent:"Limpar aplicativos recentes"});
        clear.addEventListener("click",()=>{options.clearRecent?.();options.recent=[];render();});
        recent.append(clear);body.prepend(recent);
      }
    }

    async function load() {
      const mine = ++token;
      loading = true;
      render();
      try {
        const [result,foreground] = await Promise.all([listSources(),getForegroundSuggestion().catch(()=>null)]);
        if (mine !== token) return;
        list = result;
        suggestion = foreground;
      } catch (error) {
        if (mine !== token) return;
        list = null;
        body.replaceChildren(
          banner("error", `Não foi possível listar as janelas: ${errorText(error)}`),
          el("p", { class: "muted" }, "Você ainda pode usar o seletor do Windows."),
        );
        loading = false;
        refresh.disabled = false;
        return;
      }
      loading = false;
      render();
    }

    close.addEventListener("click", () => finish(null));
    fallback.addEventListener("click", () => finish({ kind: "picker" }));
    refresh.addEventListener("click", () => { void load(); if (tab === "monitors") void loadAudio(); });
    overlay.addEventListener("mousedown", (event) => {
      if (event.target === overlay) finish(null);
    });
    document.addEventListener("keydown", onKey, true);
    signal?.addEventListener("abort", cancel, { once: true });
    document.body.append(overlay);
    for (const child of document.body.children) {
      if (!(child instanceof HTMLElement) || child === overlay || child.inert) continue;
      child.inert = true;
      silenced.push(child);
    }
    (tabs.root.querySelector("button") ?? focusableIn(dialog)[0])?.focus();
    void load();
  });
}
