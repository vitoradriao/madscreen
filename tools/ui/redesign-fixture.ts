// Local-only visual inventory. Production components, fictional data, no remote services.
import { client } from '../../apps/madscreen-app/src/session';
import { createLoginScreen } from '../../apps/madscreen-app/src/views/login';
import { createSettingsView } from '../../apps/madscreen-app/src/views/settings';
import { createShareView } from '../../apps/madscreen-app/src/views/share';
import { createWatchView } from '../../apps/madscreen-app/src/views/watch';
import { createServers } from '../../viewer/shared/servers-ui';
import { createRecoveryNotice } from '../../viewer/shared/ui';
import { showTrayNotice } from '../../apps/madscreen-app/src/tray-notice';
import { ApiFailure } from '../../viewer/shared/client';
import { Room, RoomEvent, publication, track } from '../../viewer/tests/fake-sdk.mjs';
import { openMiniPlayerWithHost } from '../../apps/madscreen-app/src/mini-player';
import { createUpdateController } from '../../apps/madscreen-app/src/updates';
import { createUpdateNotice } from '../../apps/madscreen-app/src/views/update-notice';
import { initializeTheme } from '../../viewer/shared/theme';

initializeTheme();

const user=(id:string,name:string,role='member')=>({id,username:id,displayName:name,avatar:null,online:true,role,live:{since:Date.now()-300000,title:'Janela de demonstração',viewers:2,generation:1}});
const ana=user('ana','Ana','owner'), bruno=user('bruno','Bruno'), carol={...user('carol','Carol'),online:false,live:null};
const snapshot:any={servers:[{id:'demo',name:'Amigos',role:'owner',members:[ana,bruno,carol],channels:[{id:'general',name:'Geral',live:[bruno,user('dani','Dani')]},{id:'games',name:'Jogos',live:[]}]}]};
client.setSession({token:'fictional-local-only',exp:9999999999,user:ana} as any);
client.providers=async()=>({discord:true,google:false,firstUser:false});
client.previewInvite=async()=>({inviter:{displayName:'Ana'},server:{name:'Amigos'},channel:{name:'Geral'}} as any);
client.beginOAuth=async()=>({url:'https://example.invalid/login',verifier:'demo'} as any);
client.awaitOAuth=async()=>new Promise(()=>{});
client.createInvite=async()=>({code:'DEMONSTRACAO',link:'https://example.invalid/i/DEMONSTRACAO',expiresAt:Math.floor(Date.now()/1000)+7*86400,maxUses:5});
let admin=false,update=false;
client.request=async(_method,path,body:any)=>{
  if(path==='/api/admin/sfu'){
    if(!admin)throw new ApiFailure(403,'forbidden','Sem permissão');
    return {mode:'livekit',provider:'livekit',health:{cloudflare:{status:'unknown',latencyMs:null,errorRate:null},livekit:{status:'healthy',latencyMs:32,errorRate:0}},usage:{bytes:{cloudflare:0,livekit:0},quotaBytes:1000000000000,threshold:.9,resetsAt:Date.UTC(2026,11,1)},transitions:[]} as any;
  }
  if(path==='/api/watch/start')return {provider:'livekit',viewerId:body.owner+'.demo',generation:1,audio:body.audio,livekit:{url:'wss://example.invalid',token:body.owner,room:body.owner,identity:'ana',publisherIdentity:'p-'+body.owner}} as any;
  return {ok:true} as any;
};
client.iceServers=async()=>({iceServers:[]});
const windows=['Jogo de demonstração','Navegador de demonstração','Editor de demonstração'].map((title,i)=>({hwnd:100+i,title,process:['game.exe','browser.exe','editor.exe'][i],instance_id:'demo-'+i,app_id:'demo-'+i,width:1920,height:1080,icon_png_base64:null,thumbnail_png_base64:null}));
(window as any).__TAURI_INTERNALS__={transformCallback:()=>1,unregisterCallback:()=>{},invoke:async(command:string,args:any)=>{
  if(command==='get_settings')return {quality_height:1080,quality_fps:60,software_video_decode:false};
  if(command==='list_sources'||command==='list_monitors')return {windows,monitors:[{index:0,name:'Monitor principal',width:2560,height:1440,primary:true,thumbnail_png_base64:null}],encoder:null};
  if(command==='get_foreground_suggestion')return {hwnd:100,instance_id:'demo-0'};
  if(command==='list_audio_apps')return [{pid:10,display_name:'Jogo de demonstração',process_name:'game.exe',active:true,peak:.6},{pid:20,display_name:'Música de demonstração',process_name:'music.exe',active:true,peak:.4}];
  if(command==='output_options')return [720,1080,1440,2160,0].map(choice=>({choice,label:choice?choice+'p':'Nativa',available:choice===0||choice<=args.height,width:choice?Math.round(choice*16/9):args.width,height:choice||args.height,reduced:false,ultrawide:false,note:null}));
  return null;
}};
// Vídeo de canvas e faixa de áudio silenciosa, sem capturar tela nem dispositivos.
const canvas=document.createElement('canvas');canvas.width=1920;canvas.height=1080;
const ctx=canvas.getContext('2d')!;ctx.fillStyle='#15302c';ctx.fillRect(0,0,1920,1080);ctx.fillStyle='#89e2c0';ctx.font='52px sans-serif';ctx.fillText('Conteúdo fictício · demonstração do player',170,500);
const video=canvas.captureStream(1).getVideoTracks()[0];
const silentAudio = new AudioContext().createMediaStreamDestination().stream.getAudioTracks()[0]!;
function demoTracks(): void {
  for (const room of Room.instances) {
    if (room._demo) continue;
    room._demo = true;
    for (const streamTrack of [video, silentAudio]) {
      const remote = track(streamTrack.kind);
      remote.mediaStreamTrack = streamTrack.clone();
      room.emit(RoomEvent.TrackSubscribed, remote, publication(streamTrack.kind), { identity: 'p-' + room.connection.token });
    }
  }
}
class Socket {static OPEN=1;readyState=1;onopen:any;onmessage:any;onclose:any;constructor(){queueMicrotask(()=>{this.onopen?.();this.onmessage?.({data:JSON.stringify({type:'hello',live:true,generation:1})});});}send(){}close(){this.readyState=3;}}
(window as any).WebSocket=Socket;
let directory:any,share:any,watch:any,settings:any,updates:any,notice:any;
function mount(node:HTMLElement){document.body.replaceChildren(node);}
function shell(){
  directory?.dispose();share?.dispose?.();watch?.dispose?.();settings?.dispose?.();
  updates?.dispose();notice?.dispose();
  share=createShareView();watch=createWatchView(()=>page('watch'),()=>page('servers'));
  updates=createUpdateController({check:async()=>update?({version:'0.1.13-demo',currentVersion:'0.1.12',close:async()=>{},download:async()=>{},install:async()=>{}} as any):null,prepareInstall:async()=>{}});
  notice=createUpdateNotice(updates);
  settings=createSettingsView(async()=>{},updates);
  directory=createServers(client,{onSettings:()=>page('settings'),onOpenDirectory:()=>page('servers'),onOpenWatch:()=>page('watch'),onOpenShare:()=>page('share'),onShare:target=>{page('share');share.startInChannel(target);},onWatch:friend=>{page('watch');watch.watch(friend);},watchLabel:name=>watch.panel.actionLabel(name)});
  const root=document.createElement('div');root.className='shell directory-mode unified-shell';const main=document.createElement('main');main.className='content directory-mode';main.append(directory.root);root.append(main,watch.compact,notice.root);mount(root);
  directory.update(structuredClone(snapshot));directory.setConnected(true);share.updateDirectory(structuredClone(snapshot));directory.pageHost.append(share.root,watch.root,settings.root);page('servers');
}
function page(id:string){directory.setPage(id);for(const [name,view] of [['share',share],['watch',watch],['settings',settings]] as const){view.root.hidden=name!==id;if(name===id)view.show?.();else view.hide?.();}}
const live:any={phase:'live',session_id:1,source:'Jogo de demonstração',encoder:'NVENC',width:1920,height:1080,source_width:1920,source_height:1080,quality_height:1080,fps:60,fps_target:60,bitrate_bps:8000000,bitrate_target_bps:8000000,publishing:true,publishing_connected:true,audio_active:1,privacy_supported:true,privacy_video:'active',privacy_pending:false,message:null,message_is_error:false,diagnostic:null};
Object.assign(live,{channel_id:'general',server_id:'demo'});
(window as any).demoMini=()=>openMiniPlayerWithHost(document.querySelector('.stage') as HTMLElement,'Bruno · madscreen',{invoke:async()=>undefined as any,listen:async()=>()=>{}});
(window as any).demo={shell,page,snapshot,live,shareState:(state:any)=>{const next={...live,...state};share.update(next);directory.setSharing(next.phase!=='idle');notice.setLive(next.phase!=='idle');},directory:(state:any)=>directory.update(state),disconnect:()=>directory.setConnected(false),error:()=>directory.showError(new Error('Não foi possível carregar os servidores. Verifique sua conexão.')),login:()=>mount(createLoginScreen(()=>{})),recovery:()=>{const root=createLoginScreen(()=>{});root.querySelector('.auth-card')!.replaceChildren(createRecoveryNotice('CHAVE-FICTICIA-SEM-VALIDADE',()=>{}));mount(root);},tray:()=>showTrayNotice(async()=>{}),admin:(value:boolean)=>{admin=value;},update:(value:boolean)=>{update=value;},notice:()=>{update=true;return updates.check(true);},tracks: demoTracks,buttons:()=>[...document.querySelectorAll('button,summary')].filter((el:any)=>el.getClientRects().length).map(el=>({text:el.textContent,aria:el.getAttribute('aria-label'),key:(el as HTMLElement).dataset.focuskey}))};
shell();
