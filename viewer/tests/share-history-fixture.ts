import { createShareView } from "../../apps/madscreen-app/src/views/share";
import { client } from "../../apps/madscreen-app/src/session";
import { historyKey } from "../../apps/madscreen-app/src/share-history";
import { validateWindow } from "../../apps/madscreen-app/src/share-history";
import { pickSource } from "../../apps/madscreen-app/src/views/picker";

export async function checkShareHistory(check:(ok:unknown,label:string)=>void,flush:()=>Promise<void>) {
  const original=(window as any).__TAURI_INTERNALS__,session=client.session;
  const appId="a".repeat(64),instanceId="b".repeat(64);
  const userId="history-fixture";
  const key=historyKey(userId);
  const preference={appId,processName:"game.exe",height:720,fps:30,audio:"window"};
  let windows=[{hwnd:456,title:"Fonte privada do laboratório",process:"game.exe",width:1920,height:1080,
    app_id:appId,instance_id:instanceId,selection_id:1,icon_png_base64:null,thumbnail_png_base64:null}];
  const starts:any[]=[];
  let outputBlocked=false;
  let liveState:any=null, failChange=false;
  let listDenied=false,listGate:Promise<void>|null=null;
  client.session={token:"history",exp:9999999999,user:{id:userId,username:"host",displayName:"Host",avatar:null}};
  localStorage.setItem(key,JSON.stringify({version:1,apps:[preference]}));
  (window as any).__TAURI_INTERNALS__={invoke:async(command:string,args:any)=>{
    if(command==="get_settings")return {quality_height:1080,quality_fps:60};
    if(command==="get_foreground_suggestion")return {hwnd:456,instance_id:instanceId};
    if(command==="list_sources"||command==="list_monitors"){
      if(command==="list_sources") {if(listDenied)throw Error("Permissão de janela indisponível");if(listGate)await listGate;}
      return {windows,monitors:[],encoder:null};
    }
    if(command==="output_options")return [720,1080,2160,0].map(choice=>({choice,label:choice?choice+"p":"Nativa",available:choice!==2160&&!outputBlocked||choice===0,width:1280,height:choice||1080,reduced:false,ultrawide:false,note:null}));
    if(command==="start_share_cloudflare"){starts.push(args);return;}
    if(command==="get_state")return liveState;
    if(command==="change_share"){
      if(failChange)throw Error("Alteração recusada no teste");
      liveState={...liveState,session_id:liveState.session_id+1,quality_height:args.height,fps_target:args.fps};return;
    }
    throw Error("Comando inesperado no histórico: "+command);
  }};
  const target={serverId:"server",serverName:"Teste",channelId:"channel",channelName:"Geral"};
  const directory={servers:[{id:"server",name:"Teste",channels:[{id:"channel",name:"Geral"}]}]} as any;
  let view=createShareView();document.body.append(view.root);view.updateDirectory(directory);
  const button=(text:string)=>[...view.root.querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent===text)!;
  const recent=()=>[...document.querySelectorAll<HTMLButtonElement>(".pk-body button")].find(b=>b.textContent?.startsWith("game.exe ·"))!;
  try {
    button("Escolher janela ou tela").click();await flush();
    check(starts.length===0&&!!recent()&&!recent().disabled,"Abrir app/seletor e carregar recentes não inicia captura");
    check(document.querySelector('[aria-label="Aplicativo sugerido"] .pk-title')?.textContent===windows[0].title,
      "Sugestão confirma HWND e instância na listagem elegível sem iniciar captura");
    recent().click();await flush();
    check(starts.length===0&&view.root.textContent?.includes("Fonte privada do laboratório")
      && view.root.querySelector('[aria-label="Taxa de quadros"] [aria-checked="true"]')?.textContent==="30 FPS","Recente restaura fonte/som/qualidade e aguarda consentimento explícito");
    const prior=windows;windows=[{...windows[0],instance_id:"c".repeat(64)}];
    button("Compartilhar").click();await flush();
    check(starts.length===0&&view.root.textContent?.includes("fechada ou substituída"),"Janela reciclada entre seleção e início é recusada sem fallback para monitor");
    windows=prior;button("Compartilhar").click();await flush();
    check(starts.length===1&&starts[0].source.kind==="verified_window"&&starts[0].source.instance_id===instanceId
      &&starts[0].height===720&&starts[0].fps===30&&starts[0].audio.follow_window&&starts[0].selectionId===1,"Início confirmado envia identidade esperada, ticket, qualidade salva e som da janela");
    liveState={phase:"live",session_id:1,quality_height:720,source_instance_id:instanceId,source:"Fonte privada do laboratório",encoder:"fixture",width:1280,height:720,
      source_width:1920,source_height:1080,level:32,profile_level_id:"640020",fps:30,bitrate_bps:1000000,fps_target:30,bitrate_target_bps:1000000,
      publishing:false,publishing_connected:null,audio_active:1,message:null,message_is_error:false,diagnostic:null};view.update(liveState);
    const stored=localStorage.getItem(key)!;
    check(!stored.includes("Fonte privada")&&!stored.includes("456")&&!stored.includes(instanceId)&&stored.includes(appId),"Histórico gravado após início confirmado exclui título, alvo efêmero e identidade da instância");
    [...view.root.querySelectorAll<HTMLButtonElement>('[aria-label="Taxa de quadros"] button')].find(b=>b.textContent==="60 FPS")!.click();await flush();
    button("Aplicar qualidade").click();await flush();
    check(JSON.parse(localStorage.getItem(key)!).apps[0].fps===60,"Qualidade aplicada durante live só atualiza perfil após confirmação do supervisor da mesma fonte");
    failChange=true;
    [...view.root.querySelectorAll<HTMLButtonElement>('[aria-label="Taxa de quadros"] button')].find(b=>b.textContent==="30 FPS")!.click();await flush();
    button("Aplicar qualidade").click();await flush();
    check(JSON.parse(localStorage.getItem(key)!).apps[0].fps===60,"Falha/rollback de alteração conserva perfil confirmado anterior");
    failChange=false;
    view.dispose?.();view.root.remove();
    view=createShareView();document.body.append(view.root);view.updateDirectory(directory);
    windows=[prior[0],{...prior[0],hwnd:457}];button("Escolher janela ou tela").click();await flush();
    check(recent().disabled,"Duas janelas do mesmo app exigem escolha explícita");
    check(!!document.querySelector('[aria-label="Aplicativo sugerido"]'),"Sugestão identifica inequivocamente uma janela mesmo com dois apps iguais");
    document.querySelector<HTMLButtonElement>('[aria-label="Remover game.exe dos recentes"]')!.click();await flush();
    check(!recent()&&JSON.parse(localStorage.getItem(key)!).apps.length===0,"Remover recente elimina registro local sem capturar");
    document.querySelector<HTMLButtonElement>(".pk-close")!.click();await flush();
    localStorage.setItem(key,JSON.stringify({version:1,apps:[preference]}));windows=prior;
    view.startInChannel(target);await flush();recent().click();await flush();
    check(starts.length===2&&starts[1].height===720&&starts[1].fps===30,"Caminho rápido por canal inicia após clique explícito no recente com preferências revalidadas");
    outputBlocked=true;
    view.startInChannel(target);await flush();recent().click();await flush();
    check(starts.length===2&&view.root.textContent?.includes("Resolução ajustada"),
      "Preferência incompatível interrompe início rápido e apresenta alternativa antes de nova confirmação");
    outputBlocked=false;
    windows=[];button("Trocar fonte ou som").click();await flush();
    check(recent().disabled&&!document.querySelector('[aria-label="Aplicativo sugerido"]'),
      "Fonte fechada remove sugestão, desabilita recente e permite limpar histórico sem fallback");
    document.querySelector<HTMLButtonElement>(".pk-close")!.click();await flush();
    button("Trocar fonte ou som").click();await flush();
    view.dispose?.();await flush();
    check(!document.querySelector(".pk-overlay")&&starts.length===2,"Logout/descartar view cancela recente e não inicia captura");
    windows=prior;
    const interactionCounts:number[]=[];
    for(const useRecent of [false,true]){
      view.root.remove();
      localStorage.setItem(key,JSON.stringify({version:1,apps:useRecent?[preference]:[]}));
      view=createShareView();document.body.append(view.root);view.updateDirectory(directory);await flush();
      let clicks=0;
      const count=()=>{clicks++;};document.addEventListener("click",count,true);
      try{
        button("Escolher janela ou tela").click();await flush();
        if(useRecent){recent().click();await flush();}
        else{
          document.querySelector<HTMLButtonElement>(".pk-grid .pk-card")!.click();await flush();
          [...view.root.querySelectorAll<HTMLButtonElement>('[aria-label="Resolução"] button')].find(b=>b.textContent?.startsWith("720p"))!.click();await flush();
          [...view.root.querySelectorAll<HTMLButtonElement>('[aria-label="Taxa de quadros"] button')].find(b=>b.textContent==="30 FPS")!.click();await flush();
        }
        button("Compartilhar").click();await flush();interactionCounts.push(clicks);
      }finally{document.removeEventListener("click",count,true);view.dispose?.();}
    }
    check(interactionCounts[0]===5&&interactionCounts[1]===3&&starts.length===4,
      "Jornada automatizada 720p/30: fluxo manual usa cinco cliques e recente três, ambos com confirmação explícita");
    view.root.remove();
    const secondApp="d".repeat(64),secondInstance="e".repeat(64);
    windows=[prior[0],{...prior[0],hwnd:789,app_id:secondApp,instance_id:secondInstance,selection_id:2,process:"second.exe",title:"Segundo aplicativo"}];
    const profiles=[{...preference,audio:"none" as const},{appId:secondApp,processName:"second.exe",height:1080,fps:60,audio:"window" as const}];
    let restored:any=null;
    for(const index of [0,1]){
      const pending=pickSource(undefined,{recent:profiles,onRecent:p=>{restored=p;}});await flush();
      check(starts.length===4,"Abrir perfis de executáveis diferentes não inicia captura");
      const cards=document.querySelectorAll<HTMLButtonElement>(".pk-grid .pk-card");
      check(cards[index].textContent?.includes(index===0?"sem som":"som da janela"),"Cartão da fonte informa modo de áudio salvo antes do clique");
      cards[index].click();const source=await pending;
      check(restored.appId===profiles[index].appId&&restored.height===profiles[index].height&&restored.fps===profiles[index].fps
        && source?.audio?.follow_window===(index===1)&&source?.audio?.system===false&&source.selectionId===index+1,
        "Escolha manual restaura perfil do app exato, conserva ticket e não amplia áudio para PC");
      if(index===0)check(!validateWindow(source!,[{...windows[0],selection_id:99}]),
        "Mesmo HWND/PID/título/fingerprint com ticket de outra vida é recusado");
    }
    restored=null;
    const layoutPicker=pickSource();await flush();
    (window as any).pickerSuggestionHtml=document.querySelector(".pk-overlay")!.outerHTML;
    document.querySelector<HTMLButtonElement>('.pk-close')!.click();await layoutPicker;
    const suggested=pickSource(undefined,{recent:profiles,onRecent:p=>{restored=p;}});await flush();
    document.querySelector<HTMLButtonElement>('[aria-label="Aplicativo sugerido"] .pk-card')!.click();
    const chosen=await suggested;
    check(restored.appId===appId&&restored.height===720&&chosen?.audio?.follow_window===false,
      "Sugestão usa o mesmo perfil e consentimento da seleção manual");
    const custom=[{...preference,audio:"custom" as const}];
    const customPending=pickSource(undefined,{recent:custom});await flush();
    document.querySelector<HTMLButtonElement>(".pk-grid .pk-card")!.click();await flush();
    check(!!document.querySelector(".pk-overlay")&&document.querySelector(".pk-body")?.textContent?.includes("não são reabertas automaticamente"),
      "Áudio customizado exige revisão, sem reusar PID nem abrir outra fonte silenciosamente");
    document.querySelector<HTMLInputElement>('.pk-audio input')!.click();
    document.querySelector<HTMLButtonElement>(".pk-grid .pk-card")!.click();
    const reviewed=await customPending;
    check(reviewed?.audio?.system!==true&&!(reviewed?.audio?.apps.length),"Revisão customizada só usa som explicitamente escolhido no painel");
    view=createShareView();document.body.append(view.root);view.updateDirectory(directory);
    button("Escolher janela ou tela").click();await flush();recent().click();await flush();
    listDenied=true;button("Compartilhar").click();await flush();
    check(starts.length===4&&view.root.textContent?.includes("Permissão de janela indisponível"),"Permissão perdida após seleção falha sem captura ou fallback");
    listDenied=false;
    let release!:()=>void;listGate=new Promise<void>(resolve=>{release=resolve;});
    button("Compartilhar").click();await flush();view.updateDirectory({servers:[]} as any);release();await flush();listGate=null;
    check(starts.length===4,"Canal removido durante revalidação impede publicação tardia");
    view.updateDirectory(directory);
    listGate=new Promise<void>(resolve=>{release=resolve;});button("Compartilhar").click();await flush();
    const ownSession=client.session;client.session=null;release();await flush();listGate=null;
    check(starts.length===4,"Logout durante revalidação impede publicação tardia sem nova autorização");client.session=ownSession;
    listGate=new Promise<void>(resolve=>{release=resolve;});button("Compartilhar").click();await flush();
    client.session={...ownSession!,token:"outra-sessao"};release();await flush();listGate=null;
    check(starts.length===4,"Logout seguido de outra sessão não herda consentimento do token anterior");client.session=ownSession;
  }finally{view.dispose?.();view.root.remove();(window as any).__TAURI_INTERNALS__=original;client.session=session;localStorage.removeItem(key);localStorage.removeItem("ms.share.channel."+userId);}
}
