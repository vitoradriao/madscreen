import { readCss } from '../../tools/ui/read-css.mjs';
import { build } from '../node_modules/esbuild/lib/main.js';
import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { readFile,writeFile,mkdir } from 'node:fs/promises';
import path from 'node:path';
const root=process.cwd(),out=path.join(root,'bench-results/issue19');await mkdir(out,{recursive:true});
const js=(await build({entryPoints:['viewer/tests/member-list-fixture.ts'],bundle:true,write:false,format:'esm',platform:'browser',target:'es2022'})).outputFiles[0].text;
const base=await readCss(path.join(root,'viewer/shared/tokens.css'));
const app=await readFile('apps/madscreen-app/src/styles.css','utf8'),web=await readFile('viewer/web/web.css','utf8'),redesign=await readFile('viewer/shared/redesign.css','utf8');
const server=createServer((req,res)=>{res.setHeader('Content-Type',req.url==='/qa.js'?'text/javascript':'text/html; charset=utf-8');res.end(req.url==='/qa.js'?js:'<!doctype html><meta charset="utf-8"><style>'+base+'\n'+(req.url==='/app'?app:web)+'\n'+redesign+'</style><body><script type="module" src="/qa.js"></script>');});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const profile=path.join(out,'chrome-'+Date.now());await mkdir(profile);
const child=spawn('C:/Program Files/Google/Chrome/Application/chrome.exe',['--headless=new','--no-first-run','--disable-background-networking','--window-size=1280,800','--remote-debugging-port=0','--user-data-dir='+profile,'about:blank'],{stdio:'ignore',windowsHide:true});
let socket;
try{
let port;for(let i=0;i<100;i++){try{port=(await readFile(path.join(profile,'DevToolsActivePort'),'utf8')).split('\n')[0];break;}catch{await new Promise(r=>setTimeout(r,100));}}
if(!port)throw Error('Chrome nao iniciou');
const tabs=await(await fetch('http://127.0.0.1:'+port+'/json/list')).json();socket=new WebSocket(tabs.find(t=>t.type==='page').webSocketDebuggerUrl);await new Promise((r,j)=>{socket.onopen=r;socket.onerror=j;});
let id=0;const pending=new Map();socket.onmessage=e=>{const v=JSON.parse(e.data);if(v.id){const p=pending.get(v.id);pending.delete(v.id);v.error?p.reject(v.error):p.resolve(v.result);}};
const send=(method,params={})=>new Promise((resolve,reject)=>{const n=++id;pending.set(n,{resolve,reject});socket.send(JSON.stringify({id:n,method,params}));});
const evaluate=async expression=>{const v=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(v.exceptionDetails)throw Error(v.exceptionDetails.exception?.description||v.exceptionDetails.text);return v.result.value;};
await send('Network.enable');await send('Network.setBlockedURLs',{urls:['https://*','wss://*']});
await send('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false});
const results=[];
for(const surface of ['app','web']){
await send('Page.navigate',{url:'http://127.0.0.1:'+server.address().port+'/'+surface});
for(let i=0;i<100&&!await evaluate('!!window.memberReady');i++)await new Promise(r=>setTimeout(r,50));
for(const theme of ['dark','light','contrast']){
await evaluate('document.documentElement.dataset.theme='+JSON.stringify(theme));
await evaluate('document.fonts.ready.then(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))))');
results.push({surface,theme,rows:await evaluate('checkMembers()')});
const shot=await send('Page.captureScreenshot',{format:'png'});await writeFile(path.join(out,surface+'-'+theme+'.png'),Buffer.from(shot.data,'base64'));
}}
await writeFile(path.join(out,'result.json'),JSON.stringify({ok:true,results,limit:'Chrome com componente e estilos reais; WebView2 nativo nao exercitado; sem midia.'},null,2));console.log(JSON.stringify({ok:true,captures:6,out}));
}finally{socket?.close();child.kill();server.close();}
