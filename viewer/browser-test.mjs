import { readCss } from '../tools/ui/read-css.mjs';
import { build } from "esbuild";
import { createServer } from "node:http";
import { launchChrome } from "./tests/chrome-launch.mjs";
import { readFile,writeFile,mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root=fileURLToPath(new URL("../",import.meta.url));
const output=path.join(root,"bench-results/livekit-browser");await mkdir(output,{recursive:true});
const bundle=await build({entryPoints:[fileURLToPath(new URL("tests/browser-fixture.ts",import.meta.url))],bundle:true,write:false,format:"esm",platform:"browser",target:"es2022",loader:{".css":"empty"},alias:{"livekit-client":fileURLToPath(new URL("tests/fake-sdk.mjs",import.meta.url)),"@shared":fileURLToPath(new URL("shared",import.meta.url))},define:{"import.meta.env.VITE_MADSCREEN_URL":JSON.stringify("http://127.0.0.1")}});
let css=await readCss(new URL("shared/tokens.css",import.meta.url));
const server=createServer((req,res)=>{res.setHeader("Content-Type",req.url==="/qa.js"?"text/javascript":"text/html");res.end(req.url==="/qa.js"?bundle.outputFiles[0].text:`<!doctype html><meta charset="utf-8"><style>${css}</style><body style="padding:24px"><script type="module" src="/qa.js"></script></body>`);});
await new Promise(resolve=>server.listen(0,"127.0.0.1",resolve));
let socket,chrome;
try {
  chrome=await launchChrome({profileRoot:output,args:["--headless=new","--no-first-run","--disable-background-networking","--autoplay-policy=no-user-gesture-required","--window-size=1440,1000","--no-sandbox","--disable-gpu","--enable-logging=stderr"]});
  const port=chrome.port;
  const tabs=await(await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  socket=new WebSocket(tabs.find(tab=>tab.type==="page"&&tab.url==="about:blank").webSocketDebuggerUrl);
  await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
  let sequence=0;const pending=new Map(),errors=[];
  socket.onmessage=event=>{const value=JSON.parse(event.data);if(value.method==="Runtime.exceptionThrown")errors.push(value.params.exceptionDetails.text);if(value.id){const item=pending.get(value.id);if(item){pending.delete(value.id);value.error?item.reject(value.error):item.resolve(value.result);}}};
  const send=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}));});
  await send("Network.enable");
  await send("Network.setBlockedURLs",{urls:["https://cdn.openai.com/*"]});
  await send("DOM.enable");await send("CSS.enable");
  await send("Runtime.enable");await send("Emulation.setFocusEmulationEnabled",{enabled:true});
  await send('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'reduce'}]});
  await send("Page.navigate",{url:`http://127.0.0.1:${server.address().port}`});
  const response=await send("Runtime.evaluate",{expression:'new Promise(resolve=>{const timer=setInterval(()=>{if(window.qa){clearInterval(timer);resolve(window.qa);}},50);setTimeout(()=>{clearInterval(timer);resolve({ok:false,error:"timeout"});},60000);})',awaitPromise:true,returnByValue:true});
  const result={...response.result.value,browserErrors:errors};
  const splitScreenshot=await send("Page.captureScreenshot",{format:"png"});
  await writeFile(path.join(output,"splitscreen.png"),Buffer.from(splitScreenshot.data,"base64"));
  if(result.ok) {
    const appCss=await readFile(new URL("../apps/madscreen-app/src/styles.css",import.meta.url),"utf8")+await readFile(new URL("shared/redesign.css",import.meta.url),"utf8");
    const pickerCss=await readFile(new URL("../apps/madscreen-app/src/views/picker.css",import.meta.url),"utf8");
    const evaluate=async expression=>{
      const value=await send("Runtime.evaluate",{expression,awaitPromise:true,returnByValue:true});
      if(value.exceptionDetails)throw Error(value.exceptionDetails.text);
      return value.result.value;
    };
    result.renderedFonts=[];
    await evaluate(`(() => {
      const style=document.createElement('style');style.id='picker-suggestion-qa';style.textContent=${JSON.stringify(appCss)}+${JSON.stringify(pickerCss)};document.head.append(style);
      const template=document.createElement('template');template.innerHTML=window.pickerSuggestionHtml;document.body.append(template.content);
      const image=new Image();image.src='data:image/svg+xml,'+encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="1936" height="1048"><rect width="1936" height="1048" fill="black"/></svg>');document.querySelector('.pk-suggestion .pk-thumb').replaceChildren(image);
    })()`);
    result.pickerSuggestion=[];
    for(const [width,height] of [[1235,770],[860,660],[540,600]]) {
      await send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});
      const bounds=await evaluate(`new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>{
        const box=s=>document.querySelector(s).getBoundingClientRect();
        const card=box('.pk-suggestion .pk-card'),thumb=box('.pk-suggestion .pk-thumb'),grid=box('.pk-grid .pk-card'),body=box('.pk-body'),dialog=box('.pk-dialog');
        resolve({viewport:[innerWidth,innerHeight],cardWidth:card.width,cardHeight:card.height,thumbWidth:thumb.width,thumbHeight:thumb.height,gridVisible:body.bottom-grid.top,inside:card.left>=body.left&&card.right<=body.right&&dialog.right<=innerWidth});
      })))`);
      result.pickerSuggestion.push(bounds);
      if(bounds.cardHeight>124||bounds.cardWidth>481||bounds.thumbWidth>129||bounds.thumbHeight>73||bounds.gridVisible<80||!bounds.inside)throw Error('Sugestão ocupa o seletor ou esconde a grade: '+JSON.stringify(bounds));
    }
    const pickerShot=await send('Page.captureScreenshot',{format:'png'});
    await writeFile(path.join(output,'picker-suggestion-compact.png'),Buffer.from(pickerShot.data,'base64'));
    result.checks.push('Sugestão compacta não amplia Chrome nem oculta outras janelas em 1235/860/540 px');
    await evaluate(`document.querySelector('.pk-overlay').remove();document.getElementById('picker-suggestion-qa').remove()`);
    await send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
    const verifyRenderedFonts=async view=>{
      await evaluate('document.fonts.ready');
      const {root:documentRoot}=await send('DOM.getDocument');
      for(const selector of ['h1','h2','h3','.btn','label','p','summary','input','strong','.channel-live-count']) {
        const {nodeId}=await send('DOM.querySelector',{nodeId:documentRoot.nodeId,selector});
        if(!nodeId)continue;
        const {fonts}=await send('CSS.getPlatformFontsForNode',{nodeId});
        if(!fonts.length)continue;
        result.renderedFonts.push({view,selector,fonts});
        if(fonts.some(font=>font.glyphCount>0&&(!font.isCustomFont||!font.familyName.startsWith('Inter'))))throw Error('Fonte de reserva renderizada em '+view+' '+selector+': '+JSON.stringify(fonts));
      }
    };
    result.fonts=await evaluate(`Promise.race([Promise.all([400,500,600,700].map(async weight=>({weight,loaded:(await document.fonts.load(weight+' 16px "Inter"')).some(face=>face.status==='loaded')}))),new Promise((_,reject)=>setTimeout(()=>reject(Error('Timeout carregando Inter local')),15000))])`);
    if(result.fonts.some(font=>!font.loaded))throw Error('Inter não carregou em todos os pesos');
    result.checks.push('Inter local carregada nos pesos 400/500/600/700 com CDN bloqueado');
    const overlayBackgrounds=await evaluate(`(() => [...document.querySelectorAll('.stage .controls > .btn')].map(button=>getComputedStyle(button).backgroundColor))()`);
    if(!overlayBackgrounds.length||overlayBackgrounds.some(color=>color==='transparent'||color==='rgba(0, 0, 0, 0)'))throw Error('Controles sem fundo proprio perdem contraste sobre video claro');
    result.checks.push('Ajuda e tela cheia mantêm fundo opaco sobre vídeo claro');
    await evaluate(`(() => {
      const style=document.createElement('style');style.textContent=${JSON.stringify(appCss)}+${JSON.stringify(pickerCss)};document.head.append(style);
      document.body.style.padding='0';
      const layout=document.querySelector('.watch-layout');
      const shell=document.createElement('div');shell.className='shell watching';
      const sidebar=document.createElement('aside');sidebar.className='sidebar';sidebar.innerHTML=window.shellSidebarHtml;
      const content=document.createElement('main');content.className='content watching';
      const view=document.createElement('section');view.className='view view-wide watch-view';
      view.append(layout);content.append(view);shell.append(sidebar,content);document.body.append(shell);
      window.layoutFixture={layout,shell,view};
    })()`);
    const inspect=async label=>{
      const checks=await evaluate(`new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>{
        const stages=(document.fullscreenElement ? [document.fullscreenElement] : [...document.querySelectorAll('.stage')]).filter(stage=>stage.getBoundingClientRect().width>0);
        resolve(stages.map(stage=>{
          const video=stage.querySelector('video'),s=stage.getBoundingClientRect(),v=video.getBoundingClientRect(),css=getComputedStyle(video);
          const width=stage.clientWidth,height=stage.clientHeight;
          const scale=Math.min(width/video.videoWidth,height/video.videoHeight);
          return {label:${JSON.stringify(label)},source:[video.videoWidth,video.videoHeight],stage:[width,height],
            inside:Math.abs(v.width-width)<1&&Math.abs(v.height-height)<1&&Math.abs(v.left-s.left-stage.clientLeft)<1&&Math.abs(v.top-s.top-stage.clientTop)<1,
            visible:s.top>=0&&s.bottom<=innerHeight+1&&s.left>=0&&s.right<=innerWidth+1,
            fit:css.objectFit,position:css.objectPosition,background:css.backgroundColor,
            bars:[width-video.videoWidth*scale,height-video.videoHeight*scale]};
        }));
      })))`);
      if(!checks.length||checks.some(c=>!c.inside||!c.visible||c.fit!=='contain'||c.position!=='50% 50%'||c.background!=='rgb(0, 0, 0)'||c.source[0]!==1920||c.stage[0]<=0||c.stage[1]<=0)) {
        throw Error('Proporção/corte: '+JSON.stringify(checks));
      }
      return checks;
    };
    const geometry=[];
    await send('Emulation.setDeviceMetricsOverride',{width:1280,height:720,deviceScaleFactor:1,mobile:false});
    const areaComparison=await evaluate(`new Promise(resolve=>requestAnimationFrame(()=>{
      const {shell}=window.layoutFixture;const content=shell.querySelector('main');
      const area=()=>[...shell.querySelectorAll('.stage')].reduce((sum,node)=>{const r=node.getBoundingClientRect();return sum+r.width*r.height;},0);
      const after=area();shell.style.gridTemplateColumns='240px minmax(0,1fr)';content.style.padding='32px 48px';
      const before=area();shell.style.gridTemplateColumns='';content.style.padding='';resolve({before,after,gainPct:100*(after-before)/before});
    }))`);
    if(areaComparison.gainPct<=0)throw Error('Layout não ampliou área de vídeo: '+JSON.stringify(areaComparison));
    result.areaComparison={...areaComparison,kind:'same-dom-container-comparison',viewport:[1280,720]};
    result.checks.push('Layout de assistir amplia área dos palcos na mesma janela e com as mesmas mídias');
    for(const [width,height] of [[3440,1440],[1920,1080],[1280,720],[800,900]]) {
      await send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});
      geometry.push(...await inspect(`Duas telas ${width}x${height}`));
    }
    await evaluate(`[...document.querySelectorAll('button')].find(button=>button.textContent==='Trocar destaque').click()`);
    for(const [width,height] of [[1920,1080],[800,900]]) {
      await send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});
      geometry.push(...await inspect(`Destaque 70/30 ${width}x${height}`));
      const ratio=await evaluate(`(() => {const slots=[...document.querySelectorAll('.watch-slot')].map(slot=>slot.getBoundingClientRect());return innerWidth>900?slots[1].width/slots[0].width:slots[1].height/slots[0].height;})()`);
      if(Math.abs(ratio-7/3)>0.05)throw Error('Proporção do destaque: '+ratio);
    }
    await send('Emulation.setDeviceMetricsOverride',{width:1920,height:1080,deviceScaleFactor:1,mobile:false});
    await inspect('Destaque antes da tela cheia');
    await evaluate(`(() => {
      window.fullscreenCheck={streams:[...document.querySelectorAll('video')].map(video=>video.srcObject),classes:document.querySelector('.watch-grid').className};
      [...document.querySelectorAll('button')].find(button=>button.getAttribute('aria-label')?.startsWith('Selecionar Tela 1:')).click();
    })()`);
    // Permissão limitada à origem de loopback no perfil descartável do Chrome headless.
    await send('Browser.setPermission',{permission:{name:'fullscreen',allowWithoutGesture:true},setting:'granted',origin:`http://127.0.0.1:${server.address().port}`});
    const splitFullscreen=await send('Runtime.evaluate',{expression:`document.querySelectorAll('.stage')[1].requestFullscreen()`,awaitPromise:true,userGesture:true});
    if(splitFullscreen.exceptionDetails)throw Error('Não entrou em tela cheia no splitscreen');
    geometry.push(...await inspect('Tela cheia de uma das duas transmissões'));
    const shortcuts=await evaluate(`(() => {
      const videos=[...document.querySelectorAll('video')];const before=videos.map(video=>video.muted);
      document.dispatchEvent(new KeyboardEvent('keydown',{key:'m'}));
      document.dispatchEvent(new KeyboardEvent('keydown',{key:'d'}));
      return videos[0].muted===before[0]&&videos[1].muted!==before[1]&&document.querySelector('.watch-grid').className===window.fullscreenCheck.classes;
    })()`);
    if(!shortcuts)throw Error('Atalhos afetaram a outra tela ou trocaram destaque durante tela cheia');
    await evaluate(`document.querySelectorAll('.stage')[1].querySelector('button[aria-label^="Ajuda"]').click()`);
    const helpAccessible=await evaluate(`new Promise(resolve=>setTimeout(()=>resolve(!!document.fullscreenElement?.querySelector('.watch-diagnostics[open]')),100))`);
    if(!helpAccessible)throw Error('Ajuda ficou oculta fora do elemento em tela cheia');
    await evaluate(`document.fullscreenElement.querySelector('button[aria-label^="Fechar ajuda"]').click()`);
    if(!await evaluate('!!document.fullscreenElement'))throw Error('Fechar ajuda saiu da tela cheia');
    await evaluate(`(async () => {
      const button = document.fullscreenElement.querySelector('.player-fullscreen');
      if (!button.ariaLabel.startsWith('Sair da tela cheia:')) throw Error('Botão de saída sem rótulo');
      const exited = new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(Error('O botão não saiu da tela cheia')), 2000);
        document.addEventListener('fullscreenchange', () => { clearTimeout(timer); resolve(); }, {once:true});
      });
      button.click();
      await exited;
      if (document.fullscreenElement || !button.ariaLabel.startsWith('Tela cheia:')) throw Error('Botão não voltou ao estado inicial');
    })()`);
    const restored=await evaluate(`(() => {
      const videos=[...document.querySelectorAll('video')];
      return document.querySelector('.watch-grid').className===window.fullscreenCheck.classes&&videos.every((video,index)=>video.srcObject===window.fullscreenCheck.streams[index]);
    })()`);
    if(!restored)throw Error('Tela cheia alterou o layout ou reconectou a mídia');
    result.checks.push('Ajuda abre e fecha dentro da tela cheia sem interromper a mídia');
    const featuredScreenshot=await send('Page.captureScreenshot',{format:'png'});
    await writeFile(path.join(output,'featured-splitscreen.png'),Buffer.from(featuredScreenshot.data,'base64'));
    result.checks.push('Destaque 70/30 horizontal e vertical preserva proporção e cabe na janela');
    result.checks.push('Tela cheia preserva duas mídias, limita atalhos à tela visível e restaura destaque');
    await evaluate(`[...document.querySelectorAll('.watch-slot')[1].querySelectorAll('button')].find(button=>button.getAttribute('aria-label')?.startsWith('Sair da transmissão:')).click()`);
    await send('Emulation.setDeviceMetricsOverride',{width:3440,height:1440,deviceScaleFactor:1,mobile:false});
    geometry.push(...await inspect('16:9 em 21:9'));
    if(geometry.at(-1).bars[0]<=0)throw Error('Esperadas barras pretas laterais em 21:9');
    const screenshot=await send('Page.captureScreenshot',{format:'png'});
    await writeFile(path.join(output,'aspect-ratio-ultrawide.png'),Buffer.from(screenshot.data,'base64'));
    // A navegação unificada usa flex: margens auto não podem encolher o player.
    await evaluate(`(() => {
      const {shell,view}=window.layoutFixture;
      view.parentElement.className='server-page watching';
      shell.style.gridTemplateColumns='292px minmax(0, 1fr)';
    })()`);
    result.watchWidth=[];
    for(const [width,height] of [[1920,1080],[1280,720],[800,900]]) {
      await send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});
      for(const open of [false,true,false]) {
        await evaluate(`document.querySelector('.watch-diagnostics').open=${open}`);
        geometry.push(...await inspect(`Navegação unificada ${width}x${height}, detalhes ${open}`));
        const bounds=await evaluate(`(() => {
          const {view}=window.layoutFixture,parent=view.parentElement;
          const css=getComputedStyle(parent);
          return {available:parent.clientWidth-parseFloat(css.paddingLeft)-parseFloat(css.paddingRight),width:view.getBoundingClientRect().width};
        })()`);
        result.watchWidth.push({viewport:[width,height],open,...bounds});
        if(Math.abs(bounds.available-bounds.width)>1)throw Error('Player encolheu com detalhes '+open+': '+JSON.stringify(bounds));
      }
    }
    result.checks.push('Player ocupa toda a largura na navegação unificada com detalhes fechados, abertos e fechados novamente');
    await send('Emulation.setDeviceMetricsOverride',{width:1920,height:1080,deviceScaleFactor:1,mobile:false});
    const watchScreenshot=await send('Page.captureScreenshot',{format:'png'});
    await writeFile(path.join(output,'watch-details-closed.png'),Buffer.from(watchScreenshot.data,'base64'));
    await send('Emulation.setDeviceMetricsOverride',{width:3440,height:1440,deviceScaleFactor:1,mobile:false});
    // O Chrome recusa uma reentrada automática logo após a saída, mesmo com a permissão do perfil.
    await new Promise(resolve => setTimeout(resolve, 5500));
    const fullscreen=await send('Runtime.evaluate',{expression:`document.querySelector('.stage').requestFullscreen()`,awaitPromise:true,userGesture:true});
    if(fullscreen.exceptionDetails)throw Error('Não entrou em tela cheia');
    geometry.push(...await inspect('Tela cheia 21:9'));
    await evaluate('document.exitFullscreen()');
    await evaluate(`(() => {
      const {layout,shell}=window.layoutFixture;
      const compact=document.createElement('aside');compact.className='watch-compact';
      compact.append(layout);document.body.append(compact);shell.remove();
    })()`);
    geometry.push(...await inspect('Miniatura'));
    const controlsFit=await evaluate(`new Promise(resolve=>{
      const stage=document.querySelector('.stage');stage.querySelector('.volume').focus();
      setTimeout(()=>{const bounds=stage.getBoundingClientRect();resolve([...stage.querySelector('.controls').children].filter(child=>child.getClientRects().length).every(child=>{
        const rect=child.getBoundingClientRect();return rect.left>=bounds.left&&rect.right<=bounds.right&&rect.top>=bounds.top&&rect.bottom<=bounds.bottom;
      }));},250);
    })`);
    if(!controlsFit)throw Error('Controles cortados na miniatura');
    result.checks.push('Som, ajuda e tela cheia cabem na miniatura com volume expandido');
    result.checks.push('Vídeo 1080p sem corte em 21:9, 16:9, janela pequena, duas telas, miniatura e tela cheia');
    result.geometry=geometry;
    await send('Emulation.setDeviceMetricsOverride',{width:1020,height:800,deviceScaleFactor:1,mobile:false});
    const liveControlsFit=await evaluate(`(() => {
      if(!window.shareUiHtml) return false;
      document.body.innerHTML='<div class="shell"><aside class="sidebar">'+window.shellSidebarHtml+'</aside><main class="content">'+window.shareUiHtml+'</main></div>';
      return [...document.querySelectorAll('button')].filter(button=>['Aplicar qualidade','Trocar janela ou tela'].includes(button.textContent)).every(button=>{
        const rect=button.getBoundingClientRect();return rect.width>0&&rect.left>=0&&rect.right<=innerWidth;
      });
    })()`);
    if(!liveControlsFit)throw Error('Controles de alteracao cortados na janela do app');
    const shareScreenshot=await send('Page.captureScreenshot',{format:'png'});
    await writeFile(path.join(output,'share-live-controls.png'),Buffer.from(shareScreenshot.data,'base64'));
    await verifyRenderedFonts('share');
    result.checks.push('Controles para aplicar qualidade e trocar fonte cabem na janela de 1020 px');
    result.directoryPositions=[];
    for(const [width,height] of [[1920,1080],[1348,928],[1280,720],[800,900],[510,400]]) {
      await send('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});
      await evaluate(`(() => {document.body.innerHTML='<div class="shell directory-mode"><aside class="sidebar">'+window.shellSidebarHtml+'</aside><main class="content directory-mode">'+window.directoryUiHtml+'</main></div>';})()`);
      const fit=await evaluate(`new Promise(resolve=>requestAnimationFrame(()=>{const main=document.querySelector('main');resolve(document.documentElement.scrollWidth<=innerWidth&&main.scrollWidth<=main.clientWidth);}))`);
      if(!fit)throw Error('Overflow do diretório em '+width);
      const actionsVisible=await evaluate(`(() => ['account-settings','invite','toggle-members','share-here'].every(key=>{const button=document.querySelector('[data-focuskey="'+key+'"]');const r=button?.getBoundingClientRect();return r&&r.width>0&&r.height>0&&r.left>=0&&r.right<=innerWidth;}))()`);
      if(!actionsVisible)throw Error('Ação essencial oculta ou cortada em '+width);
      if(width>=1280) {
        const positions=await evaluate(`(() => {const box=s=>document.querySelector(s).getBoundingClientRect();const rail=box('.directory-rail'),header=box('.server-navigation'),share=box('[data-focuskey="share-here"]'),members=box('.server-members'),channels=box('.server-navigation > nav');return {width:innerWidth,railLeft:rail.left,railRight:rail.right,headerLeft:header.left,headerTop:header.top,headerHeight:header.height,channelsTop:channels.top,shareRightGap:header.right-share.right,shareInHeader:share.top>=header.top&&share.bottom<=header.bottom,membersLeft:members.left,membersRight:members.right};})()`);
        if(positions.railLeft!==0||positions.railRight!==64||Math.abs(positions.railRight-positions.headerLeft)>1||positions.headerTop!==0||positions.headerHeight!==64||!positions.shareInHeader||positions.shareRightGap>33||Math.abs(positions.membersRight-width)>1)throw Error('Composição diverge da referência: '+JSON.stringify(positions));
        await verifyRenderedFonts("servers");
        const counterAligned=await evaluate(`(() => {const badge=document.querySelector('.channel-live-count');if(!badge)return false;const box=badge.getBoundingClientRect();return box.width>=20&&box.height>=20&&badge.textContent==='2';})()`);
        if(!counterAligned)throw Error('Contador de lives ilegível em '+width);
        result.directoryPositions.push(positions);
      }
      const picture=await send('Page.captureScreenshot',{format:'png'});
      await writeFile(path.join(output,`directory-${width}.png`),Buffer.from(picture.data,'base64'));
    }
    result.checks.push('Diretório com lives, membros e nome longo sem overflow em 1920/1280/800/510 px');
    await send('Emulation.setDeviceMetricsOverride',{width:1348,height:928,deviceScaleFactor:1,mobile:false});
    await evaluate(`document.body.innerHTML='<div class="shell directory-mode"><main class="content directory-mode">'+window.emptyDirectoryUiHtml+'</main></div>'`);
    const emptyPicture=await send('Page.captureScreenshot',{format:'png'});
    await writeFile(path.join(output,'directory-empty-1348.png'),Buffer.from(emptyPicture.data,'base64'));
    result.checks.push('Rail/canais/cabeçalho/membros e CTA no canto superior direito conferidos por posição');
    for(const scale of [1,1.25,1.5,2]) {
      await send('Emulation.setDeviceMetricsOverride',{width:Math.round(1020/scale),height:Math.round(800/scale),deviceScaleFactor:scale,mobile:false});
      await evaluate(`(() => {document.body.innerHTML='<div class="shell"><aside class="sidebar">'+window.shellSidebarHtml+'</aside><main class="content">'+window.shareReviewHtml+'</main></div>';document.querySelector('.share-quality').open=true;})()`);
      const fit=await evaluate(`new Promise(resolve=>requestAnimationFrame(()=>{const main=document.querySelector('main');resolve(document.documentElement.scrollWidth<=innerWidth&&main.scrollWidth<=main.clientWidth&&[...main.querySelectorAll('button')].filter(b=>b.getClientRects().length).every(b=>{const r=b.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth;}));}))`);
      if(!fit)throw Error('Overflow na aproximação de DPI '+scale);
      const picture=await send('Page.captureScreenshot',{format:'png'});
      await writeFile(path.join(output,`share-review-scale-${scale}.png`),Buffer.from(picture.data,'base64'));
    }
    result.checks.push('Compartilhamento sem overflow horizontal em viewports equivalentes a 100/125/150/200% (Chrome, não DPI nativo)');
    for(const scale of [1,1.25,1.5,2]) {
      await send('Emulation.setDeviceMetricsOverride',{width:Math.round(1020/scale),height:Math.round(800/scale),deviceScaleFactor:scale,mobile:false});
      for(const kind of ['login','tray']) {
        await evaluate(`(() => {document.body.innerHTML=window.${kind}UiHtml;const dialog=document.querySelector('dialog');if(dialog){dialog.removeAttribute('open');dialog.showModal();}})()`);
        await verifyRenderedFonts(kind);
        const fit=await evaluate(`new Promise(resolve=>requestAnimationFrame(()=>resolve(document.documentElement.scrollWidth<=innerWidth&&[...document.querySelectorAll('button,input,summary')].filter(node=>node.getClientRects().length).every(node=>{const r=node.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth;}))))`);
        if(!fit)throw Error('Overflow de '+kind+' na aproximação de DPI '+scale);
        const picture=await send('Page.captureScreenshot',{format:'png'});
        await writeFile(path.join(output,`${kind}-scale-${scale}.png`),Buffer.from(picture.data,'base64'));
      }
    }
    result.checks.push('Entrada e explicação da bandeja sem overflow em viewports equivalentes a 100/125/150/200%');
    for(const width of [1280,510]) {
      await send('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:false});
      for(const kind of ['settings','picker']) {
        await evaluate(`(() => {if (${JSON.stringify(kind)}==='settings') {
          document.body.innerHTML='<div class="shell directory-mode unified-shell"><main class="content directory-mode">'+window.directoryUiHtml+'</main></div>';
          document.querySelector('.server-content').hidden=true;
          const page=document.querySelector('.server-page');page.hidden=false;page.innerHTML=window.settingsUiHtml;
        } else {document.body.innerHTML=window.pickerUiHtml;}})()`);
        await verifyRenderedFonts(kind);
        const fit=await evaluate(`new Promise(resolve=>requestAnimationFrame(()=>{const main=document.querySelector('.content');resolve(document.documentElement.scrollWidth<=innerWidth&&(!main||main.scrollWidth<=main.clientWidth));}))`);
        if(!fit)throw Error('Overflow de '+kind+' em '+width);
        const picture=await send('Page.captureScreenshot',{format:'png'});
        await writeFile(path.join(output,`${kind}-${width}.png`),Buffer.from(picture.data,'base64'));
      }
    }
    result.checks.push('Conta e seletor de fonte revisados em 1280 e 510 px, sem overflow horizontal');
    await evaluate('window.disposeMenuDirectory = window.mountMenuDirectory()');
    const viewport = (width, height) => send('Emulation.setDeviceMetricsOverride', {width,height,deviceScaleFactor:1,mobile:false});
    const menuBounds = selector => evaluate(`new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => {
      const menu = document.querySelector(${JSON.stringify(selector)});
      const trigger = menu.querySelector(':scope > summary').getBoundingClientRect();
      const panel = menu.querySelector(':scope > .context-actions');
      const {left,right,top,bottom} = panel.getBoundingClientRect();
      const width = document.documentElement.clientWidth, height = document.documentElement.clientHeight;
      const flat = [...panel.querySelectorAll(':scope > button')].every(button => {
        const style = getComputedStyle(button);
        return style.borderTopWidth === '0px' && style.textAlign === 'start';
      });
      resolve({left,right,top,bottom,width,height,triggerLeft:trigger.left,triggerBottom:trigger.bottom,open:menu.open,flat});
    })))`);
    const menuSelectors = ['.server-title-menu', '[data-action-menu="channel-general"]', '[data-action-menu="member-bruno"]', '.server-add-menu'];
    result.contextMenuPositions = [];
    for (const [width,height] of [[1280,800],[860,560],[510,400]]) {
      await viewport(width,height);
      const tabVisible = await evaluate(`(() => {
        const tab = document.querySelector('.channel-pick[aria-current="true"]');
        const button = tab.getBoundingClientRect(), nav = tab.parentElement.getBoundingClientRect();
        return Math.min(nav.right, button.right) - Math.max(nav.left, button.left) >= 48;
      })()`);
      if (!tabVisible) throw Error('Aba ativa encoberta pelo cabeçalho em '+width);
      // Abaixo de 960 px a lista de membros começa fechada (painel sobreposto); abrir pelo botão como a pessoa faria.
      await evaluate(`new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => { const members = document.querySelector('.server-members'); if (members?.hidden) document.querySelector('[data-focuskey="toggle-members"]').click(); resolve(); })))`);
      if (await evaluate(`document.querySelector('.server-members').getBoundingClientRect().width < 200`)) throw Error('Lista de membros espremida em '+width);
      for (const selector of menuSelectors) {
        await evaluate(`document.querySelector(${JSON.stringify(selector + ' > summary')}).click()`);
        const bounds = await menuBounds(selector);
        if (!bounds.open || !bounds.flat || bounds.left < 7 || bounds.right > width - 7 || bounds.top < 7 || bounds.bottom > height - 7) {
          throw Error('Menu fora da janela ou com estilo legado: '+JSON.stringify({selector,...bounds}));
        }
        if (selector === '.server-title-menu' && Math.abs(bounds.left - bounds.triggerLeft) > 1) throw Error('Menu do servidor desalinhado');
        result.contextMenuPositions.push({selector,...bounds});
        if (width === 510 && selector === menuSelectors[1]) {
          const picture = await send('Page.captureScreenshot',{format:'png'});
          await writeFile(path.join(output,'menu-canal-510.png'),Buffer.from(picture.data,'base64'));
        }
        if (width === 860 && selector === '.server-title-menu') {
          const picture = await send('Page.captureScreenshot',{format:'png'});
          await writeFile(path.join(output,'menu-servidor-860.png'),Buffer.from(picture.data,'base64'));
          await viewport(510,400);
          const resized = await menuBounds(selector);
          if (!resized.open || resized.right > 503 || resized.bottom > 393) throw Error('Menu aberto não acompanhou o redimensionamento');
          await viewport(width,height);
        }
      }
    }
    await evaluate('window.disposeMenuDirectory()');
    result.checks.push('Menus de servidor, canal, membro e adicionar ficam inteiros, sem contorno nas ações, inclusive após redimensionar');
  }
  await writeFile(path.join(output,"result.json"),JSON.stringify(result,null,2));
  process.stdout.write(JSON.stringify(result,null,2)+"\n");process.exitCode=result.ok&&errors.length===0?0:1;
} finally {socket?.close();chrome?.close();server.closeAllConnections();server.close();}
