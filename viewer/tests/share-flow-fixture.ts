import { createShareView } from "../../apps/madscreen-app/src/views/share";
import { client } from "../../apps/madscreen-app/src/session";

export async function checkShareFlow(check: (condition: unknown, name: string) => void, flush: () => Promise<void>) {
  const internals = (window as any).__TAURI_INTERNALS__, session = client.session;
  const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  let copiedDiagnostic = '';
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (text: string) => { copiedDiagnostic = text; } } });
  client.session = { token: "flow-fixture", exp: 9999999999, user: { id: "host", username: "host", displayName: "Host", avatar: null } };
  const starts: any[] = [];
  const privacy: any[] = [];
  const destination = { serverId: "server", serverName: "Teste", channelId: "channel", channelName: "Geral" };
  (window as any).__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
    if (command === "get_settings") return { quality_height: 1080, quality_fps: 60 };
    if (command === "list_sources" || command === "list_monitors") return {
      windows: [{ hwnd: 456, title: "Fonte do fluxo", process: "fixture.exe", width: 1920, height: 1080, icon_png_base64: null, thumbnail_png_base64: null }],
      monitors: [{ index: 0, name: "Tela de teste", width: 1920, height: 1080, primary: true, thumbnail_png_base64: null }], encoder: null,
    };
    if (command === "list_audio_apps") return [{ pid: 12, display_name: "Som do teste", process_name: "fixture.exe", active: true, peak: 1 }];
    if (command === "output_options") return [720, 1080, 1440, 2160, 0].map(choice => ({ choice, label: choice === 0 ? "Nativa" : `${choice}p`, available: choice === 0 || choice <= args.height, width: choice === 0 ? args.width : Math.round(choice * 16 / 9), height: choice === 0 ? args.height : choice, reduced: false, ultrawide: false, note: null }));
    if (command === "start_share_cloudflare") { starts.push(args); return; }
    if (command === "set_share_privacy") { privacy.push(args); return; }
    if (command === "get_publisher_diagnostics") return { schema: 1, app_version: "fixture", samples: [{ at_ms: 100, fixture: true }] };
    throw Error(`Comando inesperado: ${command}`);
  } };
  const view = createShareView(); document.body.append(view.root);
  const button = (text: string) => [...view.root.querySelectorAll<HTMLButtonElement>("button")].find(item => item.textContent === text)!;
  const pickButton = (text: string) => [...document.querySelectorAll<HTMLButtonElement>('.pk-dialog button')].find(item => item.textContent === text)!;
  try {
    view.updateDirectory({ servers: [{ id: "server", name: "Teste", channels: [{ id: "channel", name: "Geral" }] }] } as any);
    await flush();
    check(view.root.querySelectorAll('.share-step').length === 3 && button("Compartilhar tela").disabled && !view.root.querySelector<HTMLDetailsElement>('.share-diagnostics')!.open, "Três passos visíveis; compartilhar exige fonte e diagnóstico começa recolhido");
    button("Escolher janela ou tela").click();await flush();
    (window as any).pickerUiHtml=document.querySelector('.pk-overlay')!.outerHTML;
    document.querySelector<HTMLButtonElement>('.pk-close')!.click();await flush();
    check(starts.length === 0 && !document.querySelector('.pk-overlay') && button("Compartilhar tela").disabled, "Cancelar primeira escolha mantém Compartilhar desabilitado e não transmite");
    button("Escolher janela ou tela").click();await flush();
    pickButton("Telas").click();await flush();
    const system = [...document.querySelectorAll<HTMLLabelElement>('.pk-check')].find(item => item.textContent?.includes("Som do PC todo"))!.querySelector('input')!;
    system.checked = true; system.dispatchEvent(new Event('change'));await flush();
    document.querySelector<HTMLButtonElement>('.pk-card')!.click();await flush();
    check(starts.length === 0 && view.root.textContent?.includes("Som do PC todo, exceto o madscreen") && !view.root.querySelector<HTMLDetailsElement>('.share-quality')!.hidden, "Fonte e som selecionados aparecem antes da confirmação, sem iniciar captura");
    (window as any).shareReviewHtml = view.root.outerHTML;
    check(view.root.querySelector<HTMLDetailsElement>('.share-quality')!.open && view.root.querySelector('[role="radiogroup"][aria-label="Taxa de quadros"]')?.textContent?.includes('60 FPS'), "Resolução e FPS ficam expostos após selecionar a fonte");
    const diagnosticButton=button('Copiar diagnóstico da transmissão');
    check(!diagnosticButton.closest('details'), 'Copiar diagnóstico permanece no cabeçalho fora das seções recolhidas');
    diagnosticButton.click();await flush();
    check(JSON.parse(copiedDiagnostic).schema===1,'Copiar diagnóstico da transmissão usa o relatório real do contrato IPC');
    button("Trocar fonte ou som").click();await flush();
    document.querySelector<HTMLButtonElement>('.pk-close')!.click();await flush();
    check(view.root.textContent?.includes("Tela 1") && view.root.textContent?.includes("Som do PC todo"), "Cancelar troca antes de iniciar preserva fonte e escolha de som");
    button("Compartilhar").click();await flush();
    check(starts.length === 1 && starts[0].source.kind === "monitor" && starts[0].audio.system === true && starts[0].channelId === "channel", "Confirmação transmite fonte, som e canal escolhidos usando contrato existente");
    const liveState: any = { phase:"live", session_id:1, width:1280, height:720, source_width:1920, source_height:1080,
      quality_height:720, fps:60, fps_target:60, bitrate_bps:1000000, bitrate_target_bps:1000000, audio_active:1,
      publishing:false, privacy_supported:true, privacy_video:"active", privacy_pending:false };
    view.update(liveState);
    button("Pausar vídeo").click(); await flush();
    check(privacy.length===1 && privacy[0].pause===true && privacy[0].muted===null
      && !view.root.textContent?.includes("Transmissão pausada."), "Pausa envia comando explícito sem anunciar confirmação antecipada");
    view.update({...liveState,privacy_video:"pausing",privacy_pending:true});
    check(button("Pausando…").disabled && button("Silenciar áudio compartilhado").disabled
      && button("Trocar janela ou tela").disabled && !button("Parar").disabled, "Pausa pendente bloqueia alterações e mantém Parar");
    view.update({...liveState,privacy_video:"paused"});
    check(view.root.textContent?.includes("Transmissão pausada. O áudio compartilhado continua ativo.")
      && !button("Retomar vídeo").disabled, "Confirmação apresenta pausa e informa áudio ainda ativo");
    button("Silenciar áudio compartilhado").click(); await flush();
    check(privacy[1].pause===null && privacy[1].muted===true, "Mudo do áudio compartilhado é independente da pausa de vídeo");
    view.update({...liveState,privacy_video:"paused",audio_muted:true});
    button("Retomar vídeo").click(); await flush();
    check(privacy[2].pause===false && privacy[2].muted===null, "Retomada exige ação explícita");
    view.update({...liveState,privacy_supported:false});
    check(button("Pausar vídeo").disabled, "Engine antigo não oferece pausa sem suporte confirmado");
    view.dispose?.();view.root.remove();
    const quick = createShareView();document.body.append(quick.root);
    try {
      quick.updateDirectory({ servers: [{ id: "server", name: "Teste", channels: [{ id: "channel", name: "Geral" }] }] } as any);
      quick.startInChannel(destination);await flush();
      check(document.querySelector('.pk-foot')?.textContent?.includes("qualidade salva"), "Início pelo canal explica que escolher fonte inicia com qualidade salva");
      document.querySelector<HTMLButtonElement>('.pk-card')!.click();await flush();
      check(starts.length === 2 && starts[1].source.kind === "window_handle" && starts[1].height === 1080 && starts[1].fps === 60, "Ação do canal e escolha da fonte mantêm caminho rápido sem nova confirmação");
    } finally { quick.dispose?.();quick.root.remove(); }
    const canceled = createShareView();document.body.append(canceled.root);
    try {
      canceled.updateDirectory({ servers: [{ id: "server", name: "Teste", channels: [{ id: "channel", name: "Geral" }] }] } as any);
      canceled.startInChannel(destination);await flush();
      canceled.cancelPending();await flush();
      check(!document.querySelector('.pk-overlay') && starts.length === 2, "Logout ou updater aborta a escolha rápida sem transmissão tardia");
    } finally { canceled.dispose?.();canceled.root.remove(); }
  } finally {
    view.dispose?.();view.root.remove();
    (window as any).__TAURI_INTERNALS__ = internals; client.session = session;
    localStorage.removeItem('ms.audio.pick');
    if (clipboardDescriptor) Object.defineProperty(navigator, 'clipboard', clipboardDescriptor);
    else delete (navigator as any).clipboard;
  }
}
