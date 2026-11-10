import assert from 'node:assert/strict';
import { test } from 'node:test';
import { defaultFps, parseHistory, preferenceFor, confirmedPreferenceFor, rememberApp, sourceForApp, validateWindow } from '../../apps/madscreen-app/src/share-history.ts';
const app='a'.repeat(64), instance='b'.repeat(64);
const pref={appId:app,processName:'game.exe',height:1080,fps:60,audio:'window'};
const window={hwnd:123,title:'Título privado',process:'game.exe',app_id:app,instance_id:instance,width:1920,height:1080};
test('perfil exige sessão live confirmada da mesma instância e ignora tentativa/rollback',()=>{
  const source=sourceForApp(pref,[window]);
  const state={phase:'live',source_instance_id:instance,quality_height:720,fps_target:30};
  assert.deepEqual(confirmedPreferenceFor(source,state),{...pref,height:720,fps:30});
  for(const changed of [{phase:'starting'},{phase:'idle'},{changing:true},{source_instance_id:'c'.repeat(64)},{source_instance_id:null}])
    assert.equal(confirmedPreferenceFor(source,{...state,...changed}),null);
});
test('histórico limita cinco apps e remove títulos, HWND/PID e campos desconhecidos',()=>{
  const history=parseHistory(JSON.stringify({version:1,apps:[...Array.from({length:8},(_,i)=>({...pref,appId:String(i).repeat(64),pid:123,hwnd:123,title:'privado',thumbnail:'bytes'}))]}));
  assert.equal(history.length,5);
  assert.deepEqual(Object.keys(history[0]).sort(),Object.keys(pref).sort());
  assert.equal(JSON.stringify(history).includes('privado'),false);
  const updated=rememberApp(history,{...history[2],fps:30});
  assert.equal(updated.length,5);assert.equal(updated[0].fps,30);
  assert.equal(updated.filter(p=>p.appId===history[2].appId).length,1);
});
test('configuração inválida ou versão desconhecida não vira captura implícita',()=>{
  for(const raw of ['{','null','[]',JSON.stringify({version:2,apps:[pref]}),JSON.stringify({version:1,apps:[{...pref,fps:120}]}),JSON.stringify({version:1,apps:[{...pref,processName:'C:/game.exe'}]})]){
    assert.deepEqual(parseHistory(raw),[]);
  }
  assert.equal(preferenceFor({kind:'monitor',index:0},1080,60),null);
  assert.equal(preferenceFor({kind:'window_handle',hwnd:123},1080,60),null);
});
test('fonte ausente/ambígua/reciclada é recusada e não vira monitor',()=>{
  assert.equal(sourceForApp(pref,[]),null);
  assert.equal(sourceForApp(pref,[window,{...window,hwnd:124}]),null);
  const picked=sourceForApp(pref,[window]);
  assert.equal(picked.kind,'window_handle');assert.equal(picked.audio.follow_window,true);
  assert.equal(validateWindow(picked,[window]),true);
  assert.equal(validateWindow(picked,[{...window,instance_id:'c'.repeat(64)}]),false);
  assert.equal(validateWindow(picked,[{...window,app_id:'d'.repeat(64)}]),false);
});
test('preferências de som mantêm modo explícito sem persistir PIDs',()=>{
  const source={kind:'window_handle',hwnd:123,appId:app,instanceId:instance,processName:'game.exe',audio:{system:false,follow_window:false,apps:[1234]}};
  const stored=preferenceFor(source,720,30);
  assert.equal(stored.audio,'custom');assert.equal(JSON.stringify(stored).includes('1234'),false);
  assert.deepEqual(sourceForApp({...stored,audio:'none'},[window]).audio,{system:false,follow_window:false,apps:[]});
  assert.equal(sourceForApp({...stored,audio:'system'},[window]).audio.system,true);
});
test('30 FPS só para QuickSync/CPU sem NVENC; AMD, NVENC e engine antigo seguem em 60',()=>{
  const intel={nvenc_available:false,gpu_vendor_id:0x8086};
  assert.equal(defaultFps({quality_fps:60},intel),30);
  assert.equal(defaultFps({quality_fps:60},{nvenc_available:false,gpu_vendor_id:0x1414}),30,'adaptador básico: x264');
  assert.equal(defaultFps({quality_fps:60},{nvenc_available:false,gpu_vendor_id:0x1002}),60,'AMD dedicada faz 60 no AMF');
  assert.equal(defaultFps({quality_fps:60},{nvenc_available:true,gpu_vendor_id:0x10de}),60);
  assert.equal(defaultFps({quality_fps:60,quality_fps_chosen:true},intel),60);
  assert.equal(defaultFps({quality_fps:30},{nvenc_available:true,gpu_vendor_id:0x10de}),30);
  for(const unknown of [null,{nvenc_available:false},{nvenc_available:false,gpu_vendor_id:null},{gpu_vendor_id:0x8086}])
    assert.equal(defaultFps({quality_fps:60},unknown),60);
});
