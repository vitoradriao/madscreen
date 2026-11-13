import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Phase = "idle" | "starting" | "picking" | "live" | "stopping";

export interface ShareState {
  source_instance_id?: string | null;
  privacy_supported?: boolean;
  privacy_video?: "active" | "pausing" | "paused" | "resuming" | "failed" | "stopped" | null;
  privacy_pending?: boolean;
  audio_muted?: boolean;
  phase: Phase;
  changing?: boolean;
  session_id?: number;
  quality_height?: number;
  source: string | null;
  encoder: string | null;
  width: number;
  height: number;
  source_width: number;
  source_height: number;
  level: number | null;
  profile_level_id: string | null;
  fps: number;
  bitrate_bps: number;
  fps_target: number;
  bitrate_target_bps: number;
  publishing: boolean;
  publishing_connected: boolean | null;
  /** Fontes de áudio ativas; `null` quando a transmissão não leva áudio. */
  audio_active: number | null;
  message: string | null;
  message_is_error: boolean;
  /** Encoder, SDP de oferta/resposta e erro completo, para copiar quando a publicação falha. */
  diagnostic: string | null;
}

export type SourceChoice = { kind: "picker" } | { kind: "monitor"; index: number } | { kind: "window_handle"; hwnd: number }
  | { kind:"verified_window"; hwnd:number; instance_id:string };
/** Som que acompanha a tela: o aplicativo da janela, apps escolhidos ou o PC todo (sem o madscreen). */
export interface AudioChoice {
  system: boolean;
  apps: number[];
  follow_window: boolean;
}

export interface AudioApp {
  pid: number;
  process_name: string;
  display_name: string;
  active: boolean;
  peak: number;
}

export type PickedSource = Exclude<SourceChoice,{kind:"verified_window"}> & {
  width?: number; height?: number; name?: string; audio?: AudioChoice;
  appId?:string; instanceId?:string; processName?:string; selectionId?:number;
};

/** Metadados do seletor ficam na UI; o engine recebe apenas a identificação da fonte. */
function sourceChoice(source?: PickedSource): SourceChoice | null {
  if (!source) return null;
  switch (source.kind) {
    case "monitor": return { kind: source.kind, index: source.index };
    case "window_handle": return source.instanceId
      ? {kind:"verified_window",hwnd:source.hwnd,instance_id:source.instanceId}
      : { kind: source.kind, hwnd: source.hwnd };
    case "picker": return { kind: source.kind };
  }
}

export interface EncoderLimits {
  max_width: number;
  max_height: number;
  min_width: number;
  min_height: number;
}

export interface OutputOption {
  choice: number;
  label: string;
  available: boolean;
  width: number;
  height: number;
  reduced: boolean;
  ultrawide: boolean;
  note: string | null;
}

export interface WindowSource {
  selection_id?:number|null;
  app_id?:string|null;
  instance_id?:string|null;
  hwnd: number;
  title: string;
  process: string;
  width: number;
  height: number;
  icon_png_base64: string | null;
  thumbnail_png_base64: string | null;
}

export interface MonitorSource {
  index: number;
  name: string;
  width: number;
  height: number;
  primary: boolean;
  thumbnail_png_base64: string | null;
}

export interface SourceList {
  windows: WindowSource[];
  monitors: MonitorSource[];
  encoder?: EncoderLimits | null;
  /** `false`: sem NVENC, caminho de compatibilidade com readback; ausente em engine anterior. */
  nvenc_available?: boolean | null;
  /** PCI vendor da GPU padrão (0x8086 Intel, 0x1002 AMD); ausente em engine anterior. */
  gpu_vendor_id?: number | null;
}

export interface Settings {
  quality_height: number;
  quality_fps: number;
  /** O FPS salvo veio do seletor, não do padrão. */
  quality_fps_chosen?: boolean;
  software_video_decode?: boolean;
}

export const getPublisherDiagnostics = () => invoke<{ schema: number; app_version: string; samples: unknown[] }>("get_publisher_diagnostics");
export const getState = () => invoke<ShareState>("get_state");
export const getSettings = () => invoke<Settings>("get_settings");
export const saveSettings = (settings: Settings) => invoke<void>("save_settings", { settings });
/** Compartilha pelo SFU da Cloudflare; sem `url`/`token` o engine usa MADSCREEN_CF_URL/TOKEN. */
export const startShareCloudflare = (
  height: number,
  fps: number,
  source?: PickedSource,
  url?: string,
  token?: string,
  audio?: AudioChoice,
  channelId?: string,
  fpsChosen?: boolean,
  directSessionId?: string,
) =>
  invoke<void>("start_share_cloudflare", {
    height,
    fps,
    source: sourceChoice(source),
    selectionId: source?.selectionId ?? null,
    url: url ?? null,
    token: token ?? null,
    audio: audio ?? null,
    channelId: channelId ?? null,
    directSessionId: directSessionId ?? null,
    fpsChosen: fpsChosen ?? null,
  });
export const listAudioApps = () => invoke<AudioApp[]>("list_audio_apps");
/** Mantém destino/canal do compartilhamento atual; fonte omitida altera só qualidade. */
export const changeShare = (height: number, fps: number, source?: PickedSource, fpsChosen?: boolean) =>
  invoke<void>("change_share", { height, fps, source: source ? sourceChoice(source) : null, audio: source?.audio ?? null, selectionId:source?.selectionId??null, fpsChosen: fpsChosen ?? null });
export const listSources = () => invoke<SourceList>("list_sources");
export const getForegroundSuggestion = () => invoke<{hwnd:number;instance_id:string}|null>("get_foreground_suggestion");
export const listMonitors = () => invoke<SourceList>("list_monitors");
export const outputOptions = (width: number, height: number, fps: number, limits?: EncoderLimits | null) =>
  invoke<OutputOption[]>("output_options", { width, height, fps, limits: limits ?? null });

export const stopShare = () => invoke<void>("stop_share");
export const setSharePrivacy = (pause: boolean | null, muted: boolean | null) =>
  invoke<void>("set_share_privacy", { pause, muted });
export function onShareState(handler: (state: ShareState) => void): Promise<UnlistenFn> {
  return listen<ShareState>("share-state", (event) => handler(event.payload));
}

export function errorText(error: unknown): string {
  return typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
}
