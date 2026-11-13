import { MadClient } from "../shared/client";
import { createServers } from "../shared/servers-ui";
import { confirmInvite } from "../shared/invite-ui";

export async function checkFriends(check: (value: unknown, label: string) => void, flush: () => Promise<void>) {
  const user = {id:"ana",username:"ana",displayName:"Ana",avatar:null};
  const friend = {id:"bruno",username:"bruno",displayName:"Bruno",avatar:null,online:true,live:null};
  const client = new MadClient(location.origin, {token:"fixture",exp:9999999999,user});
  let shared = "", watched = "";
  const directory = createServers(client,{onWatch:person=>{watched=person.id;},onDirectShare:person=>{shared=person.id;}});
  document.body.append(directory.root);
  const snapshot = {me:user,friends:[friend],incoming:[],outgoing:[],servers:[],directSessions:[]};
  directory.update(snapshot);
  try {
    check(directory.root.querySelector('.server-navigation h1')?.textContent === "Amigos" && !directory.root.textContent?.includes("Você ainda não está em nenhum servidor"), "Amigos é o início com zero servidores, sem assistente obrigatório");
    const buttons = () => [...directory.root.querySelectorAll<HTMLButtonElement>("button")];
    buttons().find(button=>button.textContent=== "Compartilhar com Bruno")!.click();
    check(shared === "bruno", "Compartilhar usa o amigo selecionado, sem servidor oculto");
    directory.update({...snapshot,friends:[{...friend,online:false}]});
    check(buttons().find(button=>button.textContent=== "Compartilhar com Bruno")?.disabled, "Destinatário offline desabilita início privado");
    let accepted = false;
    const answer = confirmInvite({kind:"personal",inviter:user,expiresAt:9999999999,maxUses:1,uses:0}).then(value=>{accepted=value;});
    await flush(); check(!accepted && !!document.querySelector('dialog[open]'), "Convite pessoal aguarda confirmação explícita");
    document.querySelector<HTMLButtonElement>('dialog[open] .primary')!.click(); await answer;
    check(accepted, "Aceitar amizade confirma somente o convite apresentado");
    check(!watched, "Abrir Amigos e aceitar convite não inicia reprodução");
  } finally {directory.dispose();directory.root.remove();}
}
