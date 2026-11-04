import { createWatchLayout } from "../shared/ui";
import type { MadClient } from "../shared/client";

export async function checkMiniPlayerLayout(client: MadClient, sendTracks: (username: string) => void,
  check: (ok: unknown, label: string) => void, flush: () => Promise<void>) {
  const iframe=document.createElement("iframe");document.body.append(iframe);
  let closes=0,opens=0;
  const layout=createWatchLayout(client,{popout:async stage=>{
    opens++;
    const marker=document.createElement("div");stage.replaceWith(marker);
    iframe.contentDocument!.body.append(stage);
    let closed=false;
    return {close(){if(closed)return;closed=true;closes++;marker.replaceWith(stage);}};
  }});
  document.body.append(layout.root);
  try {
    layout.watch({username:"bob",displayName:"Bob"});await flush();sendTracks("bob");await flush();
    layout.watch({username:"carol",displayName:"Carol"});await flush();sendTracks("carol");await flush();
    const videos=[...layout.root.querySelectorAll("video")];
    const other=videos[1].srcObject,first=videos[0].srcObject;
    const buttons=[...layout.root.querySelectorAll<HTMLButtonElement>("button")].filter(b=>b.textContent==="Mini-player");
    check(buttons.length===2,"Duas lives recebem controle de mini-player por tela");
    buttons[0].click();await flush();
    check(opens===1&&layout.count===2&&videos[0].ownerDocument===iframe.contentDocument&&videos[0].srcObject===first
      && videos[1].ownerDocument===document&&videos[1].srcObject===other,"Destacar uma live não altera mídia ou sessão da outra");
    layout.stop();await flush();
    check(closes===1&&videos[0].ownerDocument===document&&videos.every(v=>v.srcObject===null),"Stop/logout devolve palco e encerra mídias das duas telas sem janela órfã");
  } finally {layout.dispose();layout.root.remove();iframe.remove();}

  let complete: ((handle:{close():void})=>void) | null=null;
  let lateCloses=0;
  const late=createWatchLayout(client,{popout:()=>new Promise(resolve=>{complete=resolve;})});
  document.body.append(late.root);
  try {
    late.watch({username:"bob",displayName:"Bob"});await flush();sendTracks("bob");await flush();
    [...late.root.querySelectorAll<HTMLButtonElement>("button")].find(b=>b.textContent==="Mini-player")!.click();
    await flush();late.stop();
    const resolve=complete as ((handle:{close():void})=>void)|null;
    resolve?.({close(){lateCloses++;}});
    await flush();
    check(lateCloses===1&&late.count===0,"Abertura tardia após Stop é fechada em vez de reativar sessão");
  } finally {late.dispose();late.root.remove();}
}
