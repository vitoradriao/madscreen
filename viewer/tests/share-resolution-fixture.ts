import { createShareView } from "../../apps/madscreen-app/src/views/share";
import { client } from "../../apps/madscreen-app/src/session";

export async function checkShareResolution(check: (condition: unknown, name: string) => void, flush: () => Promise<void>) {
  const originalInternals = (window as any).__TAURI_INTERNALS__;
  const originalSession = client.session;
  client.session = { token: "local-share-fixture", exp: 9999999999, user: { id: "host", username: "host", displayName: "Host", avatar: null } };
  try {
    for (const scenario of [
      { sourceHeight: 1056, selected: 1080, expected: 0, label: "Janela 1936x1056 preserva Nativa ao pedir 1080p, sem queda para 720p" },
      { sourceHeight: 1080, selected: 1080, expected: 1080, label: "Fonte 1080p conserva a escolha 1080p" },
      { sourceHeight: 1056, selected: 720, expected: 720, label: "720p escolhido explicitamente continua sendo respeitado" },
    ]) {
      const starts: any[] = [];
      const source = { hwnd: 123, title: "Chrome de teste", process: "chrome.exe", width: 1936, height: scenario.sourceHeight, icon_png_base64: null, thumbnail_png_base64: null };
      (window as any).__TAURI_INTERNALS__ = { invoke: async (command: string, args: any) => {
        if (command === "get_settings") return { quality_height: scenario.selected, quality_fps: 60 };
        if (command === "list_monitors") return { windows: [], monitors: [], encoder: null };
        if (command === "list_sources") return { windows: [source], monitors: [], encoder: null };
        if (command === "list_audio_apps") return [];
        if (command === "output_options") return [720, 1080, 1440, 2160, 0].map(choice => ({
          choice, label: choice === 0 ? "Nativa" : `${choice}p`, available: choice === 0 || choice <= args.height,
          width: choice === 0 ? args.width : 0, height: choice === 0 ? args.height : choice,
          reduced: false, ultrawide: false, note: null,
        }));
        if (command === "start_share_cloudflare") { starts.push(args); return; }
        throw Error(`Comando inesperado no fixture: ${command}`);
      } };
      const view = createShareView(); document.body.append(view.root);
      try {
        view.updateDirectory({ servers: [{ id: "server", name: "Teste", channels: [{ id: "channel", name: "Geral" }] }] } as any);
    { const audience=view.root.querySelector<HTMLSelectElement>('select[aria-label="Canal da transmissão"]')!; audience.value="channel"; audience.dispatchEvent(new Event("change")); }
        await flush();
        [...view.root.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent === "Escolher janela ou tela")!.click();
        await flush();
        document.querySelector<HTMLButtonElement>(".pk-card")!.click();
        await flush();
        check(starts.length === 0, "Escolher fonte permite conferir qualidade antes de iniciar");
        [...view.root.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent === "Compartilhar")!.click();
        await flush();
        const selected = view.root.querySelector<HTMLButtonElement>('.res-option[aria-checked="true"]');
        check(starts.length === 1 && starts[0].height === scenario.expected && (scenario.expected !== 0 || selected?.textContent?.includes("Nativa")), scenario.label);
      } finally { view.dispose?.(); view.root.remove(); }
    }
  } finally {
    (window as any).__TAURI_INTERNALS__ = originalInternals;
    client.session = originalSession;
  }
}
