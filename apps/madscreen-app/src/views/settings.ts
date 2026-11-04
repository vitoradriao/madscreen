import { icon } from "@shared/icons";
import { errorText, getSettings } from "../api";
import { banner, el, segmented, svg, type View } from "../dom";
import { currentTheme, setTheme, type Theme } from "@shared/theme";
import { avatar } from "@shared/ui";
import { client } from "../session";
import { createSfuAdminPanel } from "@shared/sfu-admin";
import { invoke } from "@tauri-apps/api/core";
import type { UpdateController, UpdateStatus } from "../updates";

export function createSettingsView(onLogout: () => Promise<void>, updates: UpdateController): View {
  let disposed = false;
  const account = el("div", { class: "stack tight" });
  const themeStatus = el("div", { class: "alerts", role: "status" });
  const theme = segmented<Theme>([
    { value: "dark", label: "Grafite" },
    { value: "light", label: "Papel" },
    { value: "contrast", label: "Alto contraste" },
  ], currentTheme(), value => {
    try { setTheme(value); themeStatus.replaceChildren(); }
    catch { theme.set(currentTheme()); themeStatus.replaceChildren(banner("error", "Não foi possível salvar o tema. Tente novamente.")); }
  });
  theme.root.setAttribute("aria-label", "Tema da interface");
  const joinSound = el("input", { type: "checkbox", checked: true });
  try { joinSound.checked = localStorage.getItem("ms.join-sound") !== "off"; } catch { /* preferência opcional */ }
  const soundStatus = el("div", { class: "alerts" });
  joinSound.addEventListener("change", () => {
    try {
      localStorage.setItem("ms.join-sound", joinSound.checked ? "on" : "off");
      soundStatus.replaceChildren();
    } catch {
      joinSound.checked = !joinSound.checked;
      soundStatus.replaceChildren(banner("error", "Não foi possível salvar o aviso sonoro. Tente novamente."));
    }
  });
  const accountAlerts = el("div", { class: "alerts" });
  const sfuAdmin = createSfuAdminPanel(client);
  const compatibilityStatus = el("div", { class: "alerts" });
  const softwareDecode = el("input", { type: "checkbox", disabled: true });
  const compatibility = el("label", { class: "row" }, softwareDecode, "Modo de compatibilidade para vídeo travando");
  const retryCompatibility = el("button", { class: "btn sm", type: "button", textContent: "Consultar modo de vídeo novamente", hidden: true });
  async function loadCompatibility() {
    retryCompatibility.disabled = true;
    try {
      const settings = await getSettings();
      if (!disposed) { softwareDecode.checked = settings.software_video_decode === true; softwareDecode.disabled = false; retryCompatibility.hidden = true; compatibilityStatus.replaceChildren(); }
    } catch {
      if (!disposed) { retryCompatibility.hidden = false; compatibilityStatus.replaceChildren(banner("error", "Não foi possível consultar o modo de vídeo. Tente novamente.")); }
    } finally { if (!disposed) retryCompatibility.disabled = false; }
  }
  retryCompatibility.addEventListener("click", () => { void loadCompatibility(); });
  void loadCompatibility();
  softwareDecode.addEventListener("change", async () => {
    const enabled = softwareDecode.checked;
    softwareDecode.disabled = true;
    try {
      await invoke<void>("set_software_video_decode", { enabled });
      if (!disposed) compatibilityStatus.replaceChildren(banner("info", "Preferência salva. Pare as transmissões quando for conveniente, saia pelo menu da bandeja e abra o aplicativo novamente para aplicar."));
    } catch {
      if (!disposed) { softwareDecode.checked = !enabled; compatibilityStatus.replaceChildren(banner("error", "Não foi possível salvar. Tente novamente.")); }
    } finally { if (!disposed) softwareDecode.disabled = false; }
  });

  function renderAccount() {
    const user = client.session?.user;
    const out = el("button", { class: "btn", type: "button" }, svg(icon("logout", "sm")), "Sair da conta");
    out.addEventListener("click", async () => {
      out.disabled = true;
      accountAlerts.replaceChildren();
      try {
        await onLogout();
      } catch (error) {
        accountAlerts.replaceChildren(banner("error", `Não foi possível sair: ${errorText(error)}`));
      } finally {
        out.disabled = false;
      }
    });
    account.replaceChildren(
      ...(user ? [el("div", { class: "identity" }, avatar(user, true), el("div", { class: "who" }, el("b", { textContent: user.displayName }), el("span", { textContent: `@${user.username}` })))] : []),
      el("div", { class: "row mt" }, out),
    );
  }

  const updateStatus = el("div", { class: "alerts" });
  const install = el("button", { class: "btn primary", type: "button", textContent: "Instalar e reiniciar", hidden: true });
  const check = el("button", { class: "btn", type: "button" }, svg(icon("refresh", "sm")), "Verificar atualizações");
  check.addEventListener("click", () => { void updates.check(true); });
  install.addEventListener("click", () => { void updates.install(); });
  const renderUpdate = (status: UpdateStatus): void => {
    const busy = status.kind === "checking" || status.kind === "downloading" || status.kind === "installing";
    check.disabled = busy;
    install.disabled = busy;
    install.hidden = !(status.kind === "available" || (status.kind === "error" && status.stage === "install"));
    install.textContent = status.kind === "error" ? "Tentar instalar de novo" : "Instalar e reiniciar";
    const message = (() => {
      switch (status.kind) {
        case "idle": return null;
        case "checking": return banner("info", "Procurando atualizações…");
        case "current": return banner("ok", "Você já está na versão mais recente.");
        case "available": return banner("info", `Versão ${status.version} disponível. A instalação encerra a transmissão e reinicia o aplicativo.`);
        case "downloading": return banner("info", status.percent === null
          ? `Baixando a versão ${status.version}: ${status.megabytes.toFixed(1)} MB`
          : `Baixando a versão ${status.version}: ${status.percent}%`);
        case "installing": return banner("info", "Instalando. O aplicativo será reiniciado…");
        case "error": return banner("error", status.stage === "check"
          ? `Não foi possível verificar: ${status.message}`
          : `Não foi possível instalar: ${status.message}. Tente novamente.`);
      }
    })();
    updateStatus.replaceChildren(...(message ? [message] : []));
  };
  const unsubscribeUpdates = updates.subscribe(status => { if (!disposed) renderUpdate(status); });

  const root = el(
    "section",
    { class: "view" },
    el("header", { class: "view-head" }, el("div", { class: "grow" }, el("h1", {}, "Conta"), el("p", { class: "muted" }, "Sua identidade no madscreen e atualizações do app."))),
    el("div", { class: "card stack" }, el("h3", {}, "Conta"), account, accountAlerts),
    el("div", { class: "card stack" }, el("h3", {}, "Aparência"), el("p", { class: "small muted" }, "Escolha o tema da interface. O vídeo mantém suas cores."), theme.root, themeStatus),
    el("div", { class: "card stack" }, el("h3", {}, "Avisos"),
      el("label", { class: "row" }, joinSound, "Tocar um som quando alguém entrar na minha transmissão"),
      el("p", { class: "small muted" }, "Um aviso curto por pessoa, após sua primeira interação com o app. Sua prévia e conexões adicionais da mesma pessoa não tocam o aviso."), soundStatus),
    sfuAdmin.root,
    el("div", { class: "card stack" }, el("h3", {}, "Reprodução de vídeo"), compatibility,
      el("p", { class: "small muted" }, "Se a imagem congela enquanto a transmissão continua chegando, este modo usa a CPU para decodificar o vídeo. Pode aumentar o uso de CPU. Não muda a captura nem o codificador da sua transmissão. Requer fechar e abrir o app; as lives atuais continuam até você sair."), compatibilityStatus, retryCompatibility),
    el(
      "div",
      { class: "card stack" },
      el("h3", {}, "Atualizações"),
      el("p", { class: "muted small" }, "O app procura versões novas ao abrir e a cada hora, sem interromper o uso. A instalação só começa ao escolher Instalar e reiniciar."),
      el("div", { class: "row" }, check, install),
      updateStatus,
    ),
  );
  const sections = el("div", { class: "settings-sections" });
  for (const section of Array.from(root.children).slice(1)) sections.append(section);
  // Administração continua aparecendo somente quando autorizada pelo serviço.
  sections.append(sfuAdmin.root);
  const navigation = el("nav", { class: "settings-nav snav", ariaLabel: "Seções da conta" });
  const entries = new Map<Element, HTMLButtonElement>();
  for (const section of Array.from(sections.children)) {
    const title = section.querySelector("h3, h2")?.textContent;
    if (!title) continue;
    const pick = el("button", { class: "btn ghost", type: "button", textContent: title, hidden: (section as HTMLElement).hidden });
    pick.addEventListener("click", () => {
      section.scrollIntoView({ block: "start", behavior: "instant" });
      for (const button of entries.values()) button.setAttribute("aria-current", String(button === pick));
      const heading = section.querySelector<HTMLElement>("h3, h2");
      if (heading) { heading.tabIndex = -1; heading.focus({ preventScroll: true }); }
    });
    entries.set(section, pick);
    navigation.append(pick);
  }
  entries.values().next().value?.setAttribute("aria-current", "true");
  const adminVisibility = new MutationObserver(() => { const pick = entries.get(sfuAdmin.root); if (pick) pick.hidden = sfuAdmin.root.hidden; });
  adminVisibility.observe(sfuAdmin.root, { attributes: true, attributeFilter: ["hidden"] });
  for (const input of [joinSound, softwareDecode]) { input.classList.add("settings-switch"); input.setAttribute("role", "switch"); }
  root.classList.add("settings-view");
  root.append(el("div", { class: "settings-layout" }, navigation, sections));
  return { root, show: () => { renderAccount(); theme.set(currentTheme()); sfuAdmin.show(); }, hide: sfuAdmin.hide, dispose: () => { disposed = true; adminVisibility.disconnect(); sfuAdmin.dispose(); unsubscribeUpdates(); } };
}
