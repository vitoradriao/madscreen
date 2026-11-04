// Gera assets nativos a partir do mesmo SVG usado na marca da interface.
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { spawn, execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
const root = fileURLToPath(new URL('../../', import.meta.url));
const { build } = createRequire(new URL('../../viewer/package.json', import.meta.url))('esbuild');
const compiled = await build({entryPoints:[join(root,'viewer/shared/icons.ts')],write:false,bundle:true,format:'esm',platform:'node'});
const { icon } = await import(`data:text/javascript;base64,${Buffer.from(compiled.outputFiles[0].text).toString('base64')}`);
const mark = icon('logo').replace(/^<svg[^>]*>/,'').replace('</svg>','');
const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 32 32"><rect width="32" height="32" rx="6" fill="#222c32"/><g transform="translate(4 4)" fill="#fafafa">${mark}</g></svg>`;
const output=join(root,'apps/madscreen-app/src-tauri/icons');
await writeFile(join(output,'source.svg'),svg+'\n');
const profile=join(root,'bench-results',`brand-chrome-${Date.now()}`);await mkdir(profile,{recursive:true});
const chrome=spawn(process.env.CHROME_PATH??'C:/Program Files/Google/Chrome/Application/chrome.exe',['--headless=new','--no-first-run','--disable-background-networking','--remote-debugging-port=0',`--user-data-dir=${profile}`,'about:blank'],{windowsHide:true,stdio:'ignore'});
let socket;
try {
  let port;
  for(let i=0;i<60;i++){try{port=(await readFile(join(profile,'DevToolsActivePort'),'utf8')).split('\n')[0];break;}catch{await new Promise(r=>setTimeout(r,100));}}
  if(!port)throw Error('Chrome não iniciou para gerar assets');
  const targets=await(await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket=new WebSocket(targets.find(t=>t.type==='page').webSocketDebuggerUrl);
  await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
  socket.send(JSON.stringify({id:1,method:'Runtime.evaluate',params:{expression:`(async()=>{const image=new Image();image.src=${JSON.stringify('data:image/svg+xml;base64,'+Buffer.from(svg).toString('base64'))};await image.decode();const canvas=document.createElement('canvas');canvas.width=512;canvas.height=512;canvas.getContext('2d').drawImage(image,0,0);return canvas.toDataURL('image/png').split(',')[1];})()`,awaitPromise:true,returnByValue:true}}));
  const result=await new Promise((resolve,reject)=>{socket.onmessage=e=>{const r=JSON.parse(e.data);if(r.id===1)r.error||r.result.exceptionDetails?reject(Error('Falha renderizando SVG')):resolve(r.result.result.value);};});
  await writeFile(join(output,'source.png'),Buffer.from(result,'base64'));
  execFileSync('python',[join(root,'tools/ui/native-icons.py')],{windowsHide:true,stdio:'inherit'});
  console.log('SVG, PNGs, ICO e ICNS gerados a partir da marca compartilhada.');
} finally {socket?.close();chrome.kill();}
