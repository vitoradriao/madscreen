import { readCss } from '../tools/ui/read-css.mjs';
// Validação opt-in contra o SFU real; requer Worker local e sessões de teste privadas.
import { build } from 'esbuild';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root=fileURLToPath(new URL('../',import.meta.url));
const output=path.resolve(process.env.MADSCREEN_MEDIA_OUTPUT??path.join(root,'bench-results/livekit-validation'));
await mkdir(output,{recursive:true});
const sessions=JSON.parse(await readFile(path.join(output,'sessions.json'),'utf8'));
const signalingUrl=new URL(sessions.base);
if(signalingUrl.protocol!=='http:'||!['127.0.0.1','localhost'].includes(signalingUrl.hostname)||signalingUrl.username||signalingUrl.password||signalingUrl.search||signalingUrl.hash||signalingUrl.pathname!=='/')throw Error('O teste exige sinalização local isolada.');
const testProvider=await(await fetch(`${sessions.base}/api/sfu`,{headers:{Authorization:`Bearer ${sessions.host.token}`},signal:AbortSignal.timeout(10000)})).json();
if(testProvider.provider!=='livekit')throw Error('Este harness exige o ambiente local LiveKit; não muda a seleção de produção.');
const directoryResponse=await fetch(`${sessions.base}/api/servers`,{headers:{Authorization:`Bearer ${sessions.host.token}`},signal:AbortSignal.timeout(10000)});
if(!directoryResponse.ok)throw Error('O Worker local recusou a sessão de teste.');
const directory=await directoryResponse.json();
const channel=directory.servers.find(s=>s.members.some(m=>m.username===sessions.second.user.username))?.channels[0]?.id;
if(!channel)throw Error('As contas de teste precisam compartilhar um canal.');
const children=[];
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const wave=Buffer.alloc(44+48000*4);wave.write('RIFF');wave.writeUInt32LE(wave.length-8,4);wave.write('WAVEfmt ',8);wave.writeUInt32LE(16,16);wave.writeUInt16LE(1,20);wave.writeUInt16LE(2,22);wave.writeUInt32LE(48000,24);wave.writeUInt32LE(192000,28);wave.writeUInt16LE(4,32);wave.writeUInt16LE(16,34);wave.write('data',36);wave.writeUInt32LE(wave.length-44,40);
for(let i=0;i<48000;i++){wave.writeInt16LE(Math.round(2500*Math.sin(2*Math.PI*600*i/48000)),44+i*4);wave.writeInt16LE(Math.round(2500*Math.sin(2*Math.PI*900*i/48000)),46+i*4);}
const wavePath=path.join(output,'stereo.wav');await writeFile(wavePath,wave);
const audio=spawn('python',['-c','import os,winsound,time; winsound.PlaySound(os.environ["MADSCREEN_TEST_WAV"],winsound.SND_FILENAME|winsound.SND_ASYNC|winsound.SND_LOOP); time.sleep(150)'],{env:{...process.env,MADSCREEN_TEST_WAV:wavePath},windowsHide:true,stdio:'ignore'});children.push(audio);
async function publish(key){
 const child=spawn(path.join(root,'target/release/madscreen-encode-test.exe'),['--source','synthetic','--encoder','nvenc','--size','1920x1080','--fps','60','--duration','120','--warmup','2','--stats-interval','1','--diagnostics-output',path.join(output,`${key}.publisher.jsonl`),'--transport','cloudflare','--cf-channel',channel,'--audio-pid',String(audio.pid)],{cwd:root,env:{...process.env,MADSCREEN_CF_URL:sessions.base,MADSCREEN_CF_TOKEN:sessions[key].token},windowsHide:true,stdio:['ignore','pipe','pipe']});
 children.push(child);let stdout='',stderr='';child.stdout.on('data',b=>stdout+=b);child.stderr.on('data',b=>stderr+=b);child.on('close',async code=>{await writeFile(path.join(output,`${key}.stdout.log`),stdout);await writeFile(path.join(output,`${key}.stderr.log`),stderr);});
 for(let i=0;i<80;i++){if(stdout.includes('Publicando pelo provedor'))return child;if(child.exitCode!==null)throw Error(`Publicador ${key} encerrou (${child.exitCode}); consulte o log local.`);await wait(250);}
 throw Error(`Publicador ${key} não confirmou conexão em 20 s.`);
}
const fixture=`import {MadClient,WatchSession} from ${JSON.stringify(path.join(root,'viewer/shared/client.ts'))};
import {createWatchLayout} from ${JSON.stringify(path.join(root,'viewer/shared/ui.ts'))};
const pcs=[];const Native=window.RTCPeerConnection;window.RTCPeerConnection=class extends Native{constructor(...args){super(...args);pcs.push(this);}};
window.qa={errors:[]};window.addEventListener('unhandledrejection',()=>window.qa.errors.push('unhandledrejection'));
const s=await(await fetch('/session')).json();const client=new MadClient(s.base,s.viewer);const layout=createWatchLayout(client);document.body.append(layout.root);window.layout=layout;
window.add=(username)=>layout.watch({username,displayName:username});
window.own=()=>{const v=document.createElement('video');v.muted=true;v.autoplay=true;v.style.width='300px';document.body.append(v);window.ownVideo=v;window.ownSession=new WatchSession(new MadClient(s.base,s.host),'sfu_host',v,{onState:s=>window.ownState=s});window.ownSession.start();};
window.late=()=>{const v=document.createElement('video');v.muted=true;v.autoplay=true;v.style.width='300px';document.body.append(v);window.lateVideo=v;window.lateSession=new WatchSession(client,'sfu_host',v,{onState:s=>window.lateState=s});window.lateSession.start();};
window.snapshot=async()=>({count:layout.count,videos:Array.from(layout.root.querySelectorAll('video')).map(v=>({width:v.videoWidth,height:v.videoHeight,time:v.currentTime,paused:v.paused,muted:v.muted,audio:v.srcObject?.getAudioTracks().map(t=>t.getSettings())??[],quality:{frames:v.getVideoPlaybackQuality().totalVideoFrames,dropped:v.getVideoPlaybackQuality().droppedVideoFrames}})),late:window.lateVideo?{state:window.lateState,width:window.lateVideo.videoWidth,time:window.lateVideo.currentTime}:null,stats:await Promise.all(pcs.filter(p=>p.connectionState==='connected').map(async p=>Array.from((await p.getStats()).values()).filter(s=>s.type==='inbound-rtp'||s.type==='codec'||s.type==='candidate-pair'&&s.state==='succeeded').map(s=>Object.fromEntries(Object.entries(s).filter(([k])=>!['localCandidateId','remoteCandidateId','trackIdentifier','transportId'].includes(k)))))),errors:window.qa.errors});
window.stereo=async()=>{const stream=document.querySelector('video').srcObject;const ctx=new AudioContext();await ctx.resume();const source=ctx.createMediaStreamSource(new MediaStream(stream.getAudioTracks()));const split=ctx.createChannelSplitter(2);source.connect(split);const nodes=[0,1].map(i=>{const n=ctx.createAnalyser();n.fftSize=8192;split.connect(n,i);return n;});await new Promise(r=>setTimeout(r,500));const result=nodes.map(n=>{const b=new Float32Array(n.frequencyBinCount);n.getFloatFrequencyData(b);let index=1;for(let i=2;i<b.length;i++)if(b[i]>b[index])index=i;return {peakHz:index*ctx.sampleRate/n.fftSize,peakDb:b[index],at600:b[Math.round(600*n.fftSize/ctx.sampleRate)],at900:b[Math.round(900*n.fftSize/ctx.sampleRate)]};});await ctx.close();return result;};
window.add('sfu_host');window.ready=true;`;
const bundle=await build({stdin:{contents:fixture,resolveDir:path.join(root,'viewer'),sourcefile:'media-fixture.ts'},bundle:true,write:false,format:'esm',platform:'browser',target:'es2022'});
const css=await readCss(path.join(root,'viewer/shared/tokens.css'));
const server=createServer((req,res)=>{res.setHeader('Content-Type',req.url==='/qa.js'?'text/javascript':req.url==='/session'?'application/json':'text/html');res.end(req.url==='/qa.js'?bundle.outputFiles[0].text:req.url==='/session'?JSON.stringify({base:sessions.base,viewer:sessions.viewer,host:sessions.host}):`<!doctype html><meta charset="utf-8"><style>${css}</style><body style="padding:24px"><script type="module" src="/qa.js"></script></body>`);});
let socket,browser;
try{
 console.log('Conectando publicador 1 ao Worker/LiveKit...');await publish('host');
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 const profile=path.join(output,`chrome-${Date.now()}`);await mkdir(profile,{recursive:true});
 browser=spawn(process.env.CHROME_PATH??'C:/Program Files/Google/Chrome/Application/chrome.exe',['--headless=new','--no-first-run','--disable-background-networking','--autoplay-policy=no-user-gesture-required','--window-size=1440,1000','--remote-debugging-port=0',`--user-data-dir=${profile}`,'about:blank'],{stdio:'ignore',windowsHide:true});
 let port;for(let i=0;i<100;i++){try{port=(await readFile(path.join(profile,'DevToolsActivePort'),'utf8')).split('\n')[0];break;}catch{await wait(100);}}if(!port)throw Error('Chrome não iniciou.');
 const tabs=await(await fetch(`http://127.0.0.1:${port}/json/list`)).json();socket=new WebSocket(tabs.find(t=>t.type==='page').webSocketDebuggerUrl);await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
 let sequence=0;const pending=new Map();socket.onmessage=e=>{const v=JSON.parse(e.data);if(v.id){const p=pending.get(v.id);if(p){pending.delete(v.id);v.error?p.reject(v.error):p.resolve(v.result);}}};
 const send=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}));});
 const evaluate=async expression=>{const r=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(r.exceptionDetails)throw Error('Falha no script de teste do navegador.');return r.result.value;};
 await send('Page.navigate',{url:`http://127.0.0.1:${server.address().port}`});
 await wait(12000);const first=await evaluate('window.snapshot()');console.log('Primeiro viewer:',JSON.stringify(first.videos));
 console.log('Conectando publicador 2 e duas telas...');await publish('second');await evaluate("window.add('sfu_host2')");await wait(12000);
 await evaluate('window.late()');
 const samples=[];for(let i=0;i<6;i++){await wait(2000);samples.push(await evaluate('window.snapshot()'));}
 const stereo=await evaluate('window.stereo()');
 await evaluate('window.own()');await wait(4000);
 const own=await evaluate('({state:window.ownState,width:window.ownVideo.videoWidth,audio:window.ownVideo.srcObject?.getAudioTracks().length})');
 const headers={Authorization:`Bearer ${sessions.host.token}`,'Content-Type':'application/json'};
 const status=await(await fetch(`${sessions.base}/api/stream/status?owner=sfu_host`,{headers})).json();
 const beforeStopDiagnostics=await evaluate('window.layout.getDiagnostics()');
 await writeFile(path.join(output,'viewer-before-stop.json'),JSON.stringify(beforeStopDiagnostics,null,2));
 const screenshot=await send('Page.captureScreenshot',{format:'png'});await writeFile(path.join(output,'real-splitscreen.png'),Buffer.from(screenshot.data,'base64'));
 await fetch(`${sessions.base}/api/share/stop`,{method:'POST',headers,body:JSON.stringify({generation:status.generation})});await wait(1500);
 const stopped=await evaluate('window.snapshot()');
 const diagnostics=await evaluate('window.layout.getDiagnostics()');
 const result={at:new Date().toISOString(),provider:'livekit',kind:'real-sfu-functional-smoke',first,samples,stereo,own,stopped,diagnostics};
 await writeFile(path.join(output,'viewer-diagnostics.json'),JSON.stringify(diagnostics[1],null,2));await writeFile(path.join(output,'real-media.json'),JSON.stringify(result,null,2));
 const last=samples.at(-1);const ok=last.count===2&&last.videos.every(v=>v.width===1920&&v.height===1080&&v.time>5)&&last.late.width===1920&&last.late.time>5&&last.errors.length===0&&Math.abs(stereo[0].peakHz-600)<15&&Math.abs(stereo[1].peakHz-900)<15&&own.width===1920&&own.audio===0&&stopped.videos[0].audio.length===0&&stopped.videos[1].time>last.videos[1].time;
 console.log(JSON.stringify({ok,videos:last.videos,late:last.late,stereo,own,stopPreservesOther:stopped.videos[1].time>last.videos[1].time,errors:last.errors},null,2));
 await evaluate('window.ownSession.stop();window.lateSession.stop();window.layout.dispose()');await wait(1000);process.exitCode=ok?0:1;
}finally{
 socket?.close();browser?.kill();server.closeAllConnections();server.close();
 for(const key of ['host','second']){
  try{
   const headers={Authorization:`Bearer ${sessions[key].token}`,'Content-Type':'application/json'};
   const status=await(await fetch(`${sessions.base}/api/stream/status?owner=${sessions[key].user.username}`,{headers,signal:AbortSignal.timeout(5000)})).json();
   if(status.live)await fetch(`${sessions.base}/api/share/stop`,{method:'POST',headers,body:JSON.stringify({generation:status.generation}),signal:AbortSignal.timeout(5000)});
  }catch{/* O lease do Worker também limpa quando o teste é interrompido. */}
 }
 for(const child of children)child.kill();
}
