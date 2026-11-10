import type { PickedSource, WindowSource } from "./api";

export interface AppPreference {
  appId:string;
  processName:string;
  height:number;
  fps:number;
  audio:"window"|"system"|"none"|"custom";
}
const hash = (value:unknown): value is string => typeof value==="string"&&/^[a-f0-9]{64}$/.test(value);
export function parseHistory(raw:string|null):AppPreference[] {
  try {
    if (!raw || raw.length>20000) return [];
    const value=JSON.parse(raw);
    if (value?.version!==1||!Array.isArray(value.apps)) return [];
    const seen=new Set<string>(),result:AppPreference[]=[];
    for(const row of value.apps){
      if(!row||!hash(row.appId)||seen.has(row.appId)||typeof row.processName!=="string"
        || !/^[^\\/:\r\n]{1,128}\.exe$/i.test(row.processName)
        || ![0,720,1080,1440,2160].includes(row.height)||![30,60].includes(row.fps)
        || !["window","system","none","custom"].includes(row.audio))continue;
      seen.add(row.appId);
      // Não carregar campos adicionais de uma versão antiga/corrompida.
      result.push({appId:row.appId,processName:row.processName,height:row.height,fps:row.fps,audio:row.audio});
      if(result.length===5)break;
    }
    return result;
  }catch{return [];}
}
export function preferenceFor(source:PickedSource,height:number,fps:number):AppPreference|null {
  if(source.kind!=="window_handle"||!hash(source.appId)||!hash(source.instanceId))return null;
  const audio=source.audio;
  const preference={appId:source.appId,processName:source.processName??"",height,fps,
    audio:audio?.system?"system":audio?.follow_window?"window":audio?.apps.length?"custom":"none"};
  return parseHistory(JSON.stringify({version:1,apps:[preference]}))[0]??null;
}
export function rememberApp(history:AppPreference[],preference:AppPreference):AppPreference[] {
  return parseHistory(JSON.stringify({version:1,apps:[preference,...history.filter(p=>p.appId!==preference.appId)]}));
}
export function sourceForApp(preference:AppPreference,windows:WindowSource[]):PickedSource|null {
  const matches=windows.filter(window=>window.app_id===preference.appId&&hash(window.instance_id));
  if(matches.length!==1)return null;
  const window=matches[0];
  return {kind:"window_handle",hwnd:window.hwnd,width:window.width,height:window.height,
    name:window.title,appId:window.app_id!,instanceId:window.instance_id!,processName:window.process,selectionId:window.selection_id??undefined,
    audio:{system:preference.audio==="system",follow_window:preference.audio==="window",apps:[]}};
}
export function validateWindow(source:PickedSource,windows:WindowSource[]):boolean {
  if(source.kind!=="window_handle"||!source.instanceId)return true; // compatibilidade de engines anteriores
  return windows.some(window=>window.hwnd===source.hwnd&&window.instance_id===source.instanceId&&window.app_id===source.appId
    && (source.selectionId===undefined||window.selection_id===source.selectionId));
}
const AMD_VENDOR_ID=0x1002;
/**
 * FPS inicial da tela de compartilhamento. Sem NVENC e fora da AMD o encoder previsto é
 * QuickSync ou x264 (notebook/PC fraco, readback por quadro): 30 FPS corta esse custo pela metade
 * e dá o dobro de bits por quadro para texto. AMD segue em 60: placas dedicadas fazem 1080p60 no
 * AMF (APU fraca fica para o ajuste por carga). Vendor ou NVENC desconhecidos (engine antigo)
 * mantêm 60. Escolha explícita do usuário e um 30 salvo sempre valem.
 */
export function defaultFps(settings:{quality_fps:number;quality_fps_chosen?:boolean},gpu:{nvenc_available?:boolean|null;gpu_vendor_id?:number|null}|null):30|60 {
  if(settings.quality_fps===30)return 30;
  if(settings.quality_fps_chosen)return 60;
  const vendor=gpu?.gpu_vendor_id;
  return gpu?.nvenc_available===false&&typeof vendor==="number"&&vendor!==AMD_VENDOR_ID?30:60;
}
export function historyKey(userId:string):string { return "ms.share.apps."+encodeURIComponent(userId); }

/** Só uma sessão confirmada pode atualizar o perfil; nunca uma tentativa ou rollback de outra fonte. */
export function confirmedPreferenceFor(source:PickedSource,state:{phase:string;changing?:boolean;source_instance_id?:string|null;quality_height?:number;fps_target:number}):AppPreference|null {
  if(state.phase!=="live"||state.changing||!source.instanceId||state.source_instance_id!==source.instanceId)return null;
  return preferenceFor(source,state.quality_height??0,state.fps_target);
}
