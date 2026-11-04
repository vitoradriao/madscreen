import { openMiniPlayerWithHost, type MiniPlayerHost } from "../../apps/madscreen-app/src/mini-player";
import { checkRendererRecovery } from "../../apps/madscreen-app/src/renderer-recovery";

export async function checkMiniPlayer(check: (ok: unknown, label: string) => void, flush: () => Promise<void>) {
  const open = window.open;
  const iframe = document.createElement("iframe");
  document.body.append(iframe);
  const stage = document.createElement("div");
  stage.className = "stage";
  stage.setAttribute("style", "height:240px");
  const video = document.createElement("video");
  const stream = document.createElement("canvas").captureStream(1);
  video.srcObject = stream; video.muted = true; video.volume = 0.37;
  stage.append(video); document.body.append(stage);
  let popupClosed = false;
  let listeners: Array<{ event: string; handler: (event: {payload:string}) => void }> = [];
  let calls: Array<{ command: string; args: Record<string,unknown> }> = [];
  let token = "";
  let failShow = false;
  const host: MiniPlayerHost = {
    invoke: async <T>(command: string, args: Record<string, unknown>) => {
      calls.push({command,args});
      if (command === "prepare_miniplayer") { token = String(args.token); popupClosed = false; }
      if (args.action === "show" && failShow) throw Error("falha de janela simulada");
      if (args.action === "close" || args.action === "return") popupClosed = true;
      return undefined as T;
    },
    listen: async (event, handler) => {
      const entry = {event,handler}; listeners.push(entry);
      return () => { listeners = listeners.filter(value => value !== entry); };
    },
  };
  window.open = (() => ({get document(){return iframe.contentDocument!;},get closed(){return popupClosed;}})) as typeof window.open;
  try {
    check(await checkRendererRecovery(async()=>false)===null, "Inicialização normal não inventa falha de renderer");
    const recovery=await checkRendererRecovery(async()=>true);
    check(recovery?.getAttribute("role")==="alert" && recovery.textContent?.includes("Abra novamente")
      && !recovery.textContent?.includes("continuou"), "Recuperação de renderer anuncia interrupção e pede reabrir lives");
    recovery?.querySelector<HTMLButtonElement>("button")?.click();
    check(!document.querySelector("[data-renderer-recovery]"), "Aviso de recuperação pode ser dispensado sem controlar mídia");
    check(await checkRendererRecovery(async()=>{throw Error("engine antigo");})===null, "Ausência do comando de recuperação não quebra cliente antigo");
    let handle = await openMiniPlayerWithHost(stage,"Fonte própria",host);
    check(video.ownerDocument === iframe.contentDocument && video.srcObject === stream
      && video.muted && video.volume === 0.37, "Mini-player adota o mesmo vídeo/MediaStream e preserva áudio");
    check(document.body.textContent?.includes("Esta transmissão está no mini-player.")
      && listeners.length === 2, "Principal mantém retorno acessível e registra ciclo de vida");
    let duplicateRejected = false;
    try { await openMiniPlayerWithHost(stage,"Outra",host); } catch { duplicateRejected = true; }
    check(duplicateRejected, "Segundo mini-player é recusado sem criar assinatura adicional");
    const top=iframe.contentDocument!.querySelector<HTMLButtonElement>('[aria-label="Sempre no topo"]')!;
    top.click(); await flush();
    check(top.getAttribute("aria-pressed")==="false" && calls.at(-1)?.args.action==="top", "Sempre no topo só muda estado após confirmação nativa");
    const hide=[...iframe.contentDocument!.querySelectorAll("button")].find(b=>b.textContent==="Controles")!;
    hide.click();
    check(stage.classList.contains("mini-hidden"), "Controle permite ocultar barra sem remover os controles do DOM");
    listeners[0].handler({payload:"token-antigo"});
    check(video.ownerDocument===iframe.contentDocument, "Evento de janela antiga não fecha mini-player atual");
    listeners[0].handler({payload:token}); await flush();
    check(video.ownerDocument===document && video.srcObject===stream && !stage.classList.contains("mini-hidden")
      && stage.getAttribute("style")==="height:240px" && listeners.length===0, "Fechar nativo retorna palco e limpa estado/assinaturas de eventos");
    const count=calls.length;handle.close();
    check(calls.length===count, "Limpeza repetida é idempotente");
    handle=await openMiniPlayerWithHost(stage,"Palco removido",host);
    iframe.contentDocument!.body.replaceChildren();
    check(!popupClosed && !stage.isConnected, "Perda do palco ocorre com janela ainda aberta e sem evento nativo de fechamento");
    await new Promise(resolve=>setTimeout(resolve,650));
    check(stage.isConnected && stage.ownerDocument===document && video.srcObject===stream
      && video.muted && video.volume===0.37 && stage.getAttribute("style")==="height:240px"
      && listeners.length===0, "Watchdog recupera o mesmo palco quando o documento vivo perde o conteúdo");
    handle.close();
    handle=await openMiniPlayerWithHost(stage,"Documento substituído",host);
    const previousDocument=iframe.contentDocument;
    const loaded=new Promise(resolve=>iframe.addEventListener("load",resolve,{once:true}));
    iframe.src="about:blank?miniplayer-replacement";
    await loaded;
    check(iframe.contentDocument!==previousDocument, "Regressão substitui o documento real da janela relacionada");
    await new Promise(resolve=>setTimeout(resolve,650));
    check(stage.isConnected && stage.ownerDocument===document && video.srcObject===stream
      && stage.querySelector("video")===video && video.muted && video.volume===0.37
      && listeners.length===0, "Troca de documento recupera vídeo e sessão sem deixar mini-player reservado");
    handle.close();
    failShow=true;
    let failed=false;try { await openMiniPlayerWithHost(stage,"Falha",host); } catch { failed=true; }
    check(failed && stage.ownerDocument===document && listeners.length===0, "Falha de exibição restaura palco e permite nova tentativa");
    failShow=false;handle=await openMiniPlayerWithHost(stage,"Novamente",host);
    handle.close();
    check(stage.ownerDocument===document && calls.at(-1)?.args.action==="close", "Fim/logout fecha janela e conserva o elemento para limpeza da sessão");
  } finally {
    for (const entry of [...listeners]) entry.handler({payload:token});
    window.open=open;stage.remove();iframe.remove();
    for (const track of stream.getTracks()) track.stop();
  }
}
