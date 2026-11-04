import { createShareView } from "../../apps/madscreen-app/src/views/share";
import { client } from "../../apps/madscreen-app/src/session";

export async function checkShareChanges(check: (condition: unknown, name: string) => void, flush: () => Promise<void>) {
  const originalInternals = (window as any).__TAURI_INTERNALS__, originalSession = client.session;
  client.session = { token: "local-change-fixture", exp: 9999999999, user: { id: "host", username: "host", displayName: "Host", avatar: null } };
  const changes: any[] = []; let stops = 0, starts = 0;
  let finish: () => void = () => {}, reject: (error: Error) => void = () => {};
  (window as any).__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
    if (command === "get_settings") return { quality_height: 1080, quality_fps: 60 };
    if (command === "list_monitors" || command === "list_sources") return {
      windows: [{ hwnd: 789, title: "Outra janela", process: "fixture.exe", width: 1920, height: 1080, icon_png_base64: null, thumbnail_png_base64: null }],
      monitors: [{ index: 1, name: "Monitor de teste", width: 2560, height: 1440, primary: false, thumbnail_png_base64: null }], encoder: null,
    };
    if (command === "list_audio_apps") return [];
    if (command === "output_options") return [720, 1080, 1440, 2160, 0].map(choice => ({
      choice, label: choice === 0 ? "Nativa" : `${choice}p`, available: choice === 0 || choice <= args.height,
      width: choice === 0 ? args.width : 0, height: choice === 0 ? args.height : choice, reduced: false, ultrawide: false, note: null,
    }));
    if (command === "start_share_cloudflare") { starts++; return; }
    if (command === "stop_share") { stops++; return; }
    if (command === "change_share") { changes.push(args); return new Promise<void>((resolve, fail) => { finish = resolve; reject = fail; }); }
    throw Error(`Comando inesperado: ${command}`);
  } };
  const view = createShareView(); document.body.append(view.root);
  const button = (label: string) => [...view.root.querySelectorAll<HTMLButtonElement>("button")].find(item => item.textContent === label)!;
  const radio = (label: string) => [...view.root.querySelectorAll<HTMLButtonElement>('.res-option')].find(item => item.querySelector('.res-title')?.textContent === label)!;
  const state = (patch: any = {}) => ({ phase: "live", changing: false, session_id: 1, quality_height: 1080, source: "Janela original", source_width: 1920, source_height: 1080, width: 1920, height: 1080, fps: 60, fps_target: 60, bitrate_bps: 1000000, bitrate_target_bps: 14000000, publishing: false, publishing_connected: null, audio_active: 1, message: null, message_is_error: false, encoder: "fixture", level: 42, diagnostic: null, ...patch });
  try {
    view.updateDirectory({ servers: [{ id: "server", name: "Teste", channels: [{ id: "channel", name: "Geral" }] }] } as any);
    await flush(); view.update(state()); await flush();
    view.root.querySelector<HTMLDetailsElement>(".share-quality")!.open = true;
    check(!radio("720p").disabled && !button("Trocar janela ou tela").disabled, "Qualidade e troca de fonte ficam disponíveis durante a live");
    radio("720p").click(); button("30 FPS").click(); await flush();
    check(view.root.querySelector('.share-source')?.textContent?.includes("Janela original · 1920×1080") && view.root.textContent?.includes("Em transmissão: 1920×1080 · 60 FPS · Escolha pendente: 720p · 30 FPS"), "Resumo distingue qualidade em transmissão da escolha ainda não aplicada");
    (window as any).shareUiHtml = view.root.outerHTML;
    button("Aplicar qualidade").click(); button("Aplicar qualidade").click(); await flush();
    check(changes.length === 1 && changes[0].height === 720 && changes[0].fps === 30 && changes[0].source === null && starts === 0 && stops === 0, "Aplicar combina resolução/FPS uma vez e usa a sessão atual sem Parar manual");
    view.update(state({ phase: "stopping", changing: true }));
    view.update(state({ phase: "idle", changing: true })); await flush();
    check(view.root.textContent?.includes("Aplicando alterações") && !button("Parar").disabled && radio("720p").disabled, "Troca mantém estado de aplicação entre processos e permite cancelar com Parar");
    view.update(state({ session_id: 2, quality_height: 720, width: 1280, height: 720, fps_target: 30, fps: 30 })); finish?.(); await flush();
    check(radio("720p").getAttribute("aria-checked") === "true" && button("Aplicar qualidade").disabled, "Pipeline confirmado restaura controles com a qualidade aplicada");
    button("Trocar janela ou tela").click(); await flush();
    document.querySelector<HTMLButtonElement>('.pk-close')!.click(); await flush();
    check(changes.length === 1 && stops === 0 && view.root.textContent?.includes("Janela original"), "Cancelar o seletor mantém a transmissão e a fonte anteriores");
    button("Trocar janela ou tela").click(); await flush();
    [...document.querySelectorAll<HTMLButtonElement>('.pk-dialog button')].find(item => item.textContent === "Telas")!.click(); await flush();
    document.querySelector<HTMLButtonElement>('.pk-card')!.click(); await flush();
    check(changes.length === 2 && changes[1].source.kind === "monitor" && changes[1].source.index === 1 && !('channelId' in changes[1]), "Trocar para tela inteira envia só o novo alvo e preserva o canal no supervisor");
    reject?.(new Error("Fonte de teste recusada")); await flush();
    check(view.root.textContent?.includes("Fonte de teste recusada") && view.root.textContent?.includes("Janela original"), "Falha ao trocar restaura a seleção confirmada e apresenta o erro");
    button("Trocar janela ou tela").click(); await flush();
    document.querySelector<HTMLButtonElement>('.pk-card')!.click(); await flush();
    check(changes.length === 3 && changes[2].source.kind === "window_handle" && changes[2].source.hwnd === 789, "Trocar aplicativo envia a janela exata do seletor");
    button("Parar").click(); view.update(state({ phase: "stopping" })); finish?.(); await flush();
    check(stops === 1 && starts === 0 && changes.length === 3, "Parar durante alteração não dispara uma nova transmissão pelo frontend");
  } finally {
    finish?.(); view.dispose?.(); view.root.remove();
    (window as any).__TAURI_INTERNALS__ = originalInternals; client.session = originalSession;
  }
}
