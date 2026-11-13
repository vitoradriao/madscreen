import assert from "node:assert/strict";
import { test } from "node:test";
import { Room, RoomEvent, publication, track } from "./fake-sdk.mjs";
import { LiveKitViewer } from "../shared/livekit.ts";
import { WatchSession, videoRttMs, videoQuality } from "../shared/client.ts";
import "./receiver-latency.test.mjs";
import "./ux.test.mjs";
import "./share-history.test.mjs";

async function withSamples(run) {
  const originalPerformance = globalThis.performance;
  const originalWebSocket = globalThis.WebSocket;
  const originalWarn = console.warn;
  let now = 0;
  const requests = [], warnings = [], stats = [];
  globalThis.performance = { now: () => now };
  globalThis.WebSocket = { OPEN: 1 };
  console.warn = (...args) => warnings.push(args);
  const session = new WatchSession({session:{user:{username:"viewer"}}}, "owner", {}, {onState(){},onStats:s=>stats.push(s)});
  let inbound;
  let extra = [];
  const pc = { getStats: async () => new Map([["video", inbound], ...extra]), close() {} };
  session.pc = pc;
  session.ws = { readyState: 1, send: () => requests.push(now) };
  const sample = async (at, values = {}, transportStats = []) => {
    extra = transportStats;
    now = at;
    inbound = { id: "video", type: "inbound-rtp", kind: "video", framesDecoded: 60,
      bytesReceived: 1000 + at, packetsReceived: 100 + at / 10, packetsLost: 0, ...values };
    await session.sample(pc);
  };
  try { await run({ sample, requests, warnings, stats, session }); }
  finally { globalThis.performance = originalPerformance; globalThis.WebSocket = originalWebSocket; console.warn = originalWarn; }
}

test("RTT segue o par selecionado pelo transporte do vídeo, não pares antigos nominados", () => {
  const report=new Map([
    ["transport",{type:"transport",selectedCandidatePairId:"active"}],
    ["active",{type:"candidate-pair",nominated:true,state:"succeeded",currentRoundTripTime:0.06}],
    ["old",{type:"candidate-pair",nominated:true,state:"succeeded",currentRoundTripTime:0.2}],
  ]);
  assert.equal(videoRttMs(report,{transportId:"transport"}),60);
  assert.equal(videoRttMs(report,{}),null);
  report.delete("old");assert.equal(videoRttMs(report,{}),60);
  report.get("active").currentRoundTripTime=NaN;assert.equal(videoRttMs(report,{}),null);
  report.get("active").currentRoundTripTime=-0.1;assert.equal(videoRttMs(report,{}),null);
});

test("diagnóstico conserva RTT de 200 e 60 ms junto aos quadros, sem atribuir causalidade", async () => {
  await withSamples(async ({sample,session}) => {
    const transport=ms=>[["transport",{type:"transport",selectedCandidatePairId:"active"}],["active",{type:"candidate-pair",currentRoundTripTime:ms/1000}]];
    await sample(0,{transportId:"transport"},transport(20));
    await sample(250,{transportId:"transport",packetsLost:1},transport(200));
    await sample(500,{transportId:"transport",packetsLost:1},transport(60));
    await sample(750,{transportId:"transport",packetsLost:1,framesDecoded:90},transport(20));
    const diagnostics=session.getDiagnostics();
    assert.deepEqual(diagnostics.samples.map(s=>s.rttMs),[20,200,60,20]);
    assert.deepEqual(diagnostics.samples.map(s=>s.decodeGapMs),[0,250,500,0]);
    assert.deepEqual(diagnostics.samples.map(s=>s.framesDecoded),[60,60,60,90]);
    assert.equal(diagnostics.samples[0].presentedFrames,null);
    assert.equal(diagnostics.samples[0].presentationGapMs,null);
    const json=JSON.stringify(diagnostics);
    for(const key of ["owner","test-token","sessionId","candidate-pair","transportId"])assert(!json.includes(key));
    diagnostics.samples[0].rttMs=999;assert.equal(session.getDiagnostics().samples[0].rttMs,20);
  });
});

test("diagnóstico copiável traz QP, keyframes, codec e decoder do inbound-rtp",async()=>{
  await withSamples(async({sample,session})=>{
    const codec=[["c1",{type:"codec",mimeType:"video/H264",sdpFmtpLine:"level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"}]];
    for(let i=0;i<=10;i++)await sample(i*1000,{codecId:"c1",framesDecoded:60+i*60,qpSum:i*60*30,keyFramesDecoded:1+(i>5?1:0),pliCount:i>5?1:0,decoderImplementation:"FFmpeg"},codec);
    const quality=JSON.parse(JSON.stringify(session.getDiagnostics())).quality;
    assert.equal(quality.qpAverage,30);assert.equal(quality.codec,"H264 42e01f");assert.equal(quality.decoder,"FFmpeg");
    assert.equal(quality.keyframesPerMinute,6);assert.equal(quality.pliPerMinute,6);
    assert.deepEqual(session.getQuality(),quality);
  });
});

test("diagnóstico é limitado a 60 amostras e identifica reinício de contadores RTP",async()=>{
  await withSamples(async({sample,session})=>{
    for(let i=0;i<70;i++)await sample(i*250,{framesDecoded:60+i});
    const first=session.getDiagnostics();
    assert.equal(first.samples.length,60);assert.equal(first.samples[0].atMs,2500);
    const segment=first.samples.at(-1).segment;
    await sample(18000,{id:"replacement",framesDecoded:1,bytesReceived:1,packetsReceived:1});
    assert.equal(session.getDiagnostics().samples.at(-1).segment,segment+1);
    assert.equal(session.getDiagnostics().samples.at(-1).decodeGapMs,0);
  });
});

test("vídeo com pacotes mas sem decode pede recuperação em 500 ms, limitada a 1,5 s", async () => {
  await withSamples(async ({ sample, requests, warnings }) => {
    for (let at = 0; at <= 2250; at += 250) await sample(at);
    assert.deepEqual(requests, [500, 2000]);
    assert.equal(warnings[0][1].reason, "decode-stall");
    assert.equal(warnings[0][1].decodeGapMs, 500);
    console.info("Recuperação simulada: primeiro pedido em 500 ms; repetição em 2000 ms.");
  });
});

test("tela parada e atraso curto de decode não pedem keyframe", async () => {
  await withSamples(async ({ sample, requests }) => {
    for (let at = 0; at <= 2000; at += 250) await sample(at, {bytesReceived:1000,packetsReceived:100});
    assert.deepEqual(requests, []);
  });
  await withSamples(async ({ sample, requests }) => {
    await sample(0); await sample(250); await sample(500, {framesDecoded:90});
    await sample(750, {framesDecoded:105});
    assert.deepEqual(requests, []);
  });
});

test("perda antiga seguida de silencio nao repete pedidos sem novos pacotes", async () => {
  await withSamples(async ({sample,requests}) => {
    await sample(0);
    await sample(250,{packetsLost:1});
    await sample(500,{packetsLost:1});
    for(let at=750;at<=2500;at+=250) await sample(at,{
      packetsLost:1,bytesReceived:1500,packetsReceived:150,
    });
    assert.deepEqual(requests,[500]);
    await sample(2750,{packetsLost:1});
    assert.deepEqual(requests,[500,2750]);
  });
});

test("perda mantém recuperação rápida e troca de RTP reinicia as bases", async () => {
  await withSamples(async ({ sample, requests }) => {
    await sample(0); await sample(250, {packetsLost:1}); await sample(500, {packetsLost:1});
    assert.deepEqual(requests, [500]);
    await sample(2500, {id:"replacement",framesDecoded:1,bytesReceived:10,packetsReceived:1});
    assert.deepEqual(requests, [500]);
  });
});

test("estatísticas usam o intervalo recente e não diluem uma perda rara no histórico", async () => {
  await withSamples(async ({ sample, stats }) => {
    await sample(0, {packetsReceived:10000,jitterBufferDelay:100,jitterBufferEmittedCount:10000});
    await sample(1000, {framesDecoded:120,packetsReceived:10090,packetsLost:10,jitterBufferDelay:103,jitterBufferEmittedCount:10100});
    assert.equal(stats[0].lossPct, 10);
    assert.equal(stats[0].jitterBufferMs, 30);
    await sample(2000, {framesDecoded:180,packetsReceived:10190,packetsLost:10,jitterBufferDelay:104,jitterBufferEmittedCount:10200});
    assert.equal(stats[1].lossPct, 0);
    assert.equal(stats[1].jitterBufferMs, 10);
    await sample(3000, {id:"replacement",framesDecoded:1,bytesReceived:10,packetsReceived:1});
    assert.equal(stats.length, 2);
  });
});

const connection = { url: "wss://livekit.example.com", token: "test-token", room: "room-a", identity: "v-a", publisherIdentity: "p-a" };
const callbacks = (received = []) => ({ onTrack: x => received.push(x), onTrackRemoved() {}, onReconnecting() {}, onReconnected() {}, onDisconnected() {} });
const roomOf = () => Room.instances.at(-1);

test("diagnóstico de áudio preserva falhas e temporização sem conteúdo ou identificadores", async () => {
  await withSamples(async ({sample,session}) => {
    const audio = { id:"private-track", type:"inbound-rtp", kind:"audio", packetsReceived:100,
      packetsLost:2, concealedSamples:960, concealmentEvents:3, jitter:0.012,
      totalSamplesReceived:48000, insertedSamplesForDeceleration:120,
      removedSamplesForAcceleration:60, audioLevel:NaN, trackIdentifier:"private-name" };
    for(let i=0;i<65;i++) await sample(i*250,{type:"outbound-rtp"},[["audio",audio]]);
    const diagnostics=session.getDiagnostics();
    assert.equal(diagnostics.samples.length,0);
    assert.equal(diagnostics.audioSamples.length,60);
    const last=diagnostics.audioSamples.at(-1);
    assert.equal(last.concealedSamples,960);assert.equal(last.jitter,0.012);
    assert.equal(last.audioLevel,null);assert.equal(last.totalAudioEnergy,null);
    assert.equal(last.removedSamplesForAcceleration,60);
    assert(!JSON.stringify(diagnostics).includes("private-"));
    const segment=last.segment;
    diagnostics.audioSamples.at(-1).concealedSamples=0;
    assert.equal(session.getDiagnostics().audioSamples.at(-1).concealedSamples,960);
    await sample(17000,{},[]);
    await sample(17250,{},[["audio",audio]]);
    assert.equal(session.getDiagnostics().audioSamples.at(-1).segment,segment+1);
    const pc=session.pc;
    await session.teardown(false);session.pc=pc;
    await sample(17500,{},[["audio",audio]]);
    assert(session.getDiagnostics().audioSamples.at(-1).segment>segment+1);
  });
});

test("LiveKit coleta áudio separado do vídeo e tolera uma faixa sem estatísticas", async () => {
  const viewer=new LiveKitViewer(connection,true,callbacks()), room=roomOf();
  await viewer.connect(new AbortController().signal);
  const video=track("video"), audio=track("audio");
  audio.getRTCStatsReport=async()=>new Map([["audio",{id:"audio",type:"inbound-rtp",kind:"audio",concealedSamples:960}]]);
  room.emit(RoomEvent.TrackSubscribed,video,publication("video"),{identity:"p-a"});
  room.emit(RoomEvent.TrackSubscribed,audio,publication("audio"),{identity:"p-a"});
  assert.equal((await viewer.getStats()).get("audio").concealedSamples,960);
  video.getRTCStatsReport=async()=>{throw Error("unavailable")};
  assert.equal((await viewer.getStats()).size,1);
  room.emit(RoomEvent.TrackUnsubscribed,audio);
  assert.equal(await viewer.getStats(),null);
  viewer.close();
});

test("preferências LiveKit preservam áudio/vídeo e são limpas com a faixa", async () => {
  const viewer=new LiveKitViewer(connection,true,callbacks()), room=roomOf();
  await viewer.connect(new AbortController().signal);
  const video=track("video"), audio=track("audio");
  for(const track of [video,audio]) track.receiver=Object.create({jitterBufferTarget:null,playoutDelayHint:null});
  room.emit(RoomEvent.TrackSubscribed,video,publication("video"),{identity:"p-a"});
  room.emit(RoomEvent.TrackSubscribed,audio,publication("audio"),{identity:"p-a"});
  const copy=viewer.getLatencyPreferences();
  assert.equal(copy.video.jitterBufferTarget.reportedMs,0);
  assert.equal(copy.audio.jitterBufferTarget.reportedMs,40);
  assert.equal(audio.receiver.playoutDelayHint,0.04);
  copy.audio.jitterBufferTarget.reportedMs=999;
  assert.equal(viewer.getLatencyPreferences().audio.jitterBufferTarget.reportedMs,40);
  room.emit(RoomEvent.TrackUnsubscribed,audio);
  assert.equal(viewer.getLatencyPreferences().audio,null);
  viewer.close();
  assert.equal(viewer.getLatencyPreferences().video,null);
});

test("LiveKit descarta estatísticas de uma faixa removida durante a consulta", async () => {
  const viewer=new LiveKitViewer(connection,true,callbacks()), room=roomOf();
  await viewer.connect(new AbortController().signal);
  const audio=track("audio");let resolve;
  audio.getRTCStatsReport=()=>new Promise(r=>{resolve=r});
  room.emit(RoomEvent.TrackSubscribed,audio,publication("audio"),{identity:"p-a"});
  const pending=viewer.getStats();
  room.emit(RoomEvent.TrackUnsubscribed,audio);
  resolve(new Map([["audio",{type:"inbound-rtp",kind:"audio"}]]));
  assert.equal(await pending,null);
  viewer.close();
});

test("LiveKit assina só o publicador autorizado e mantém prévia sem áudio", async () => {
  const received = [];
  const viewer = new LiveKitViewer(connection, false, callbacks(received));
  const room = roomOf();
  const video = publication("video"), audio = publication("audio"), other = publication("video");
  room.remoteParticipants.set("p-a", { identity: "p-a", trackPublications: new Map([["v", video],["a",audio]]) });
  room.remoteParticipants.set("p-other", { identity: "p-other", trackPublications: new Map([["v",other]]) });
  await viewer.connect(new AbortController().signal);
  assert.equal(room.connection.options.autoSubscribe, false);
  assert.deepEqual(video.subscriptions, [true]);
  assert.deepEqual(audio.subscriptions, [false]);
  assert.deepEqual(other.subscriptions, [false]);
  room.emit(RoomEvent.TrackSubscribed, track("video"), video, { identity: "p-a" });
  room.emit(RoomEvent.TrackSubscribed, track("audio"), audio, { identity: "p-a" });
  assert.deepEqual(received.map(x=>x.kind), ["video"]);
  viewer.close();
  assert.equal(viewer.connected, false);
});

test("duas sessões LiveKit são independentes para splitscreen e estatísticas", async () => {
  const a = [], b = [];
  const first = new LiveKitViewer(connection, true, callbacks(a));
  const roomA = roomOf();
  const second = new LiveKitViewer({ ...connection, room: "room-b", publisherIdentity: "p-b" }, true, callbacks(b));
  const roomB = roomOf();
  await Promise.all([first.connect(new AbortController().signal), second.connect(new AbortController().signal)]);
  const videoA=track("video","a"), videoB=track("video","b");
  roomA.emit(RoomEvent.TrackSubscribed, videoA, publication("video"), { identity: "p-a" });
  roomB.emit(RoomEvent.TrackSubscribed, videoB, publication("video"), { identity: "p-b" });
  assert.equal(a[0].id,"a"); assert.equal(b[0].id,"b");
  assert.equal("jitterBufferTarget" in videoA.receiver, false);
  assert.equal(first.getLatencyPreferences().video.jitterBufferTarget.status, "unsupported");
  assert.ok(await second.getStats());
  first.close();
  assert.equal(roomA.state,"disconnected"); assert.equal(second.connected,true);
  assert.equal(await first.getStats(),null);
  second.close();
});

test("cancelamento durante connect também fecha uma conexão que conclui tarde", async () => {
  let resolve;
  Room.nextConnect = new Promise(r=>{resolve=r;});
  const viewer = new LiveKitViewer(connection,true,callbacks());
  const room = roomOf(), controller = new AbortController();
  const pending = viewer.connect(controller.signal);
  controller.abort(); resolve();
  await assert.rejects(pending, { name: "AbortError" });
  assert.equal(room.state,"disconnected"); assert.equal(viewer.connected,false);
});

test("erro de SDK não expõe token e callbacks encerrados não recebem faixas", async () => {
  Room.nextConnect = Promise.reject(new Error("secret-token-in-url"));
  const received=[];
  const viewer = new LiveKitViewer(connection,true,callbacks(received));
  const room=roomOf();
  await assert.rejects(viewer.connect(new AbortController().signal), e=>!e.message.includes("secret-token"));
  room.emit(RoomEvent.TrackSubscribed,track("video"),publication("video"),{identity:"p-a"});
  assert.equal(received.length,0);
});

globalThis.window = new EventTarget();
globalThis.localStorage = { getItem: () => null };
globalThis.MediaStream = class {
  tracks=[];
  addTrack(x) { if (!this.tracks.includes(x)) this.tracks.push(x); }
  removeTrack(x) { this.tracks=this.tracks.filter(t=>t!==x); }
  getAudioTracks() { return this.tracks.filter(t=>t.kind==="audio"); }
  getVideoTracks() { return this.tracks.filter(t=>t.kind==="video"); }
};
class Video extends EventTarget {
  srcObject=null; muted=false; volume=0.42; readyState=2; videoWidth=1920;
  play() { return Promise.resolve(); }
}

test("WatchSession usa a mesma mídia/volume/heartbeat no LiveKit sem SDP nem ICE Cloudflare", async () => {
  const requests=[];
  const video=new Video();
  const client={session:{user:{username:"friend"}},base:"https://worker.example.com",iceServers(){throw Error("Cloudflare ICE must not be used");}, async request(method,path,body){requests.push({method,path,body});return path==="/api/watch/start"?{provider:"livekit",viewerId:"owner.viewer",generation:1,audio:true,livekit:connection}:{};}};
  const states=[],audio=[];
  const session=new WatchSession(client,"owner",video,{onState:x=>states.push(x),onAudio:x=>audio.push(x)});
  session.live=true;
  try {
    await session.negotiate();
    const room=roomOf();
    room.emit(RoomEvent.TrackSubscribed,track("video"),publication("video"),{identity:"p-a"});
    room.emit(RoomEvent.TrackSubscribed,track("audio"),publication("audio"),{identity:"p-a"});
    assert.equal(video.volume,0.42);
    assert.equal(video.srcObject.getVideoTracks().length,1);
    assert.equal(video.srcObject.getAudioTracks().length,1);
    assert.ok(states.includes("playing")); assert.deepEqual(audio,[true]);
    await session.heartbeat();
    await session.setPreferredRid("h");
    await assert.rejects(session.setPreferredRid("l"));
    assert.ok(requests.some(r=>r.path==="/api/watch/heartbeat"));
    assert.ok(requests.some(r=>r.path==="/api/watch/layer"));
    assert.ok(!requests.some(r=>r.path==="/api/watch/answer"));
    const original=globalThis.fetch;
    try { globalThis.fetch=async()=>Response.json({ok:true}); session.stop(); }
    finally { globalThis.fetch=original; }
    assert.equal(video.srcObject,null); assert.equal(room.state,"disconnected");
  } finally { await session.teardown(false); }
});

test("observação de apresentação continua após o primeiro quadro e é cancelada ao sair", async()=>{
  class ObservedVideo extends Video {
    sequence=0;callbacks=new Map();
    requestVideoFrameCallback(fn){const id=++this.sequence;this.callbacks.set(id,fn);return id;}
    cancelVideoFrameCallback(id){this.callbacks.delete(id);}
    frame(at,count){const [id,fn]=this.callbacks.entries().next().value;this.callbacks.delete(id);fn(at,{presentedFrames:count});}
  }
  const video=new ObservedVideo(),states=[];
  const session=new WatchSession({session:{user:{username:"viewer"}}},"owner",video,{onState:state=>states.push(state)});
  session.stream=new MediaStream();video.srcObject=session.stream;
  session.observeFirstFrame(new AbortController().signal);
  video.frame(10,1);video.frame(30,2);
  assert.equal(session.presentedFrames,2);assert.equal(session.lastPresentedAt,30);
  assert.equal(states.filter(state=>state==="playing").length,1);
  assert.equal(video.callbacks.size,1);
  await session.teardown(false);
  assert.equal(video.callbacks.size,0);assert.equal(session.presentedFrames,null);
});


test("recuperação registra a retomada do decode e da apresentação sem botão manual", async () => {
  class ObservedVideo extends Video {
    sequence=0;callbacks=new Map();
    requestVideoFrameCallback(fn){const id=++this.sequence;this.callbacks.set(id,fn);return id;}
    cancelVideoFrameCallback(id){this.callbacks.delete(id);}
    frame(at,count){const [id,fn]=this.callbacks.entries().next().value;this.callbacks.delete(id);fn(at,{presentedFrames:count});}
  }
  await withSamples(async ({sample,requests,session}) => {
    const video=new ObservedVideo();
    session.video=video;
    session.stream=new MediaStream();video.srcObject=session.stream;
    session.observeFirstFrame(new AbortController().signal);
    video.frame(0,1);
    await sample(0);
    await sample(250,{packetsLost:1});
    await sample(500,{packetsLost:1});
    assert.deepEqual(requests,[500]);
    let events=session.getDiagnostics().recoveries;
    assert.equal(events.length,1);
    assert.equal(events[0].reason,"loss");
    assert.equal(events[0].timeToDecodeMs,null);
    assert.equal(events[0].timeToPresentationMs,null);
    video.frame(600,2); // Um frame ainda enfileirado não é recuperação do decoder.
    assert.equal(session.getDiagnostics().recoveries[0].timeToPresentationMs,null);
    await sample(750,{packetsLost:1,framesDecoded:90});
    video.frame(800,3);
    events=session.getDiagnostics().recoveries;
    assert.equal(events[0].timeToDecodeMs,250);
    assert.equal(events[0].timeToPresentationMs,300);
    events[0].timeToPresentationMs=999;
    assert.equal(session.getDiagnostics().recoveries[0].timeToPresentationMs,300);
    await session.teardown(false);
    assert.equal(video.callbacks.size,0);
  });
});

test("pedido automatico antes do primeiro decode aparece no diagnostico", async () => {
  await withSamples(async ({sample,session,requests}) => {
    session.waitingForFrameAt=0;
    await sample(0,{framesDecoded:0,bytesReceived:0,packetsReceived:0});
    await sample(1000,{framesDecoded:0,bytesReceived:0,packetsReceived:0});
    assert.deepEqual(requests,[1000]);
    await sample(1250,{framesDecoded:1});
    const event=session.getDiagnostics().recoveries[0];
    assert.equal(event.reason,"first-frame");
    assert.equal(event.timeToDecodeMs,250);
    assert.equal(event.timeToPresentationMs,null);
  });
});

test("pedido sem estatisticas RTP e associado a primeira geracao observada", async () => {
  await withSamples(async ({sample,session,requests}) => {
    session.waitingForFrameAt=0;
    await sample(1000,{type:"outbound-rtp"});
    assert.deepEqual(requests,[1000]);
    await sample(1250,{framesDecoded:1});
    const event=session.getDiagnostics().recoveries[0];
    assert.equal(event.segment,session.getDiagnostics().samples[0].segment);
    assert.equal(event.timeToDecodeMs,250);
  });
});

test("reconexao antes das primeiras estatisticas nao resolve pedido da conexao anterior", async () => {
  await withSamples(async ({sample,session}) => {
    session.waitingForFrameAt=0;
    await sample(1000,{type:"outbound-rtp"});
    const pc=session.pc;
    await session.teardown(false);
    session.pc=pc;
    await sample(1250,{framesDecoded:1});
    assert.equal(session.getDiagnostics().recoveries[0].timeToDecodeMs,null);
  });
});

test("uma nova geração RTP não resolve um pedido de recuperação da sessão anterior", async () => {
  await withSamples(async ({sample,session,requests}) => {
    await sample(0);
    await sample(250,{packetsLost:1});
    await sample(500,{packetsLost:1});
    assert.deepEqual(requests,[500]);
    await sample(750,{id:"new-video",framesDecoded:1,bytesReceived:2,packetsReceived:1,packetsLost:0});
    await sample(1000,{id:"new-video",framesDecoded:30,bytesReceived:250,packetsReceived:50,packetsLost:0});
    const observation=session.getDiagnostics().recoveries[0];
    assert.equal(observation.timeToDecodeMs,null);
    assert.equal(observation.timeToPresentationMs,null);
  });
});


test("histórico de dois minutos preserva amostras recentes e segmenta reconexões", async () => {
  await withSamples(async ({ sample, session }) => {
    for (let i = 0; i < 520; i++) await sample(i * 250, { framesDecoded: 60 + i });
    const result = session.getDiagnostics();
    assert.equal(result.samples.length, 60);
    assert.equal(result.history.length, 120);
    assert.equal(result.history[0].atMs, 10000);
    assert.equal(result.historyIntervalMs, 1000);
    const segment = result.history.at(-1).segment;
    await sample(130000, { id: "replacement", framesDecoded: 1, bytesReceived: 1, packetsReceived: 1 });
    assert.equal(session.getDiagnostics().history.at(-1).segment, segment + 1);
    result.history[0].rttMs = 999;
    assert.notEqual(session.getDiagnostics().history[0].rttMs, 999);
  });
});

test("desligar armazenamento do diagnóstico preserva recuperação automática", async () => {
  await withSamples(async ({ sample, session, requests }) => {
    session.options.diagnostics = false;
    await sample(0);
    await sample(500);
    assert.deepEqual(requests, [500]);
    const result = session.getDiagnostics();
    for (const key of ["samples", "history", "audioSamples", "audioHistory", "recoveries"]) assert.equal(result[key].length, 0);
  });
});

test("reinicio dos contadores com mesmo id isola diagnostico e pedidos pendentes", async () => {
  await withSamples(async ({sample,session,requests}) => {
    await sample(0);
    await sample(500);
    assert.deepEqual(requests,[500]);
    const segment=session.getDiagnostics().samples.at(-1).segment;
    await sample(750,{framesDecoded:1,bytesReceived:10,packetsReceived:1});
    const current=session.getDiagnostics().samples.at(-1);
    assert.equal(current.segment,segment+1);
    assert.equal(current.decodeGapMs,0);
    assert.equal(session.getDiagnostics().recoveries[0].timeToDecodeMs,null);
  });
});

test("lacuna longa preserva somente dois minutos e reset do audio com mesmo id isola segmento", async () => {
  await withSamples(async ({sample,session}) => {
    const audio=bytes=>[["audio",{id:"same-audio",type:"inbound-rtp",kind:"audio",bytesReceived:bytes,packetsReceived:bytes,totalSamplesReceived:bytes}]];
    await sample(0,{},audio(100));
    const segment=session.getDiagnostics().audioSamples.at(-1).segment;
    await sample(250,{framesDecoded:61},audio(1));
    assert.equal(session.getDiagnostics().audioSamples.at(-1).segment,segment+1);
    await sample(130000,{framesDecoded:62},audio(2));
    const report=session.getDiagnostics();
    assert.equal(report.history.length,1);
    assert.equal(report.audioHistory.length,1);
    assert.equal(report.samples.at(-1).sampleGapMs,129750);
  });
});

test("qualidade do vídeo: QP por diferença, eventos por minuto e ausentes como null", () => {
  const s = (atMs, framesDecoded, qpSum, extra = {}) => ({ atMs, segment: 1, framesDecoded, qpSum, keyFramesDecoded: 2, pliCount: 0, nackCount: 0,
    codec: "H264 42e01f", decoderImplementation: null, ...extra });
  assert.deepEqual(videoQuality([]), { windowSeconds: null, qpAverage: null, keyframesPerMinute: null, pliPerMinute: null, nackPerMinute: null, codec: null, decoder: null });
  const q = videoQuality([s(0, 0, 0), s(10_000, 600, 1000), s(30_000, 1800, 9999, { keyFramesDecoded: 4, pliCount: 1, nackCount: 10 }), s(70_000, 3600, 50_000, { keyFramesDecoded: 5, pliCount: 2, nackCount: 30 })]);
  // Janela de 60 s: começa na amostra de 10 s.
  assert.equal(q.windowSeconds, 60);
  assert.equal(q.qpAverage, (50_000 - 1000) / 3000);
  assert.deepEqual([q.keyframesPerMinute, q.pliPerMinute, q.nackPerMinute, q.codec, q.decoder], [3, 2, 30, "H264 42e01f", null]);
  // framesDecoded sem avanço, qpSum que volta ou ausente: QP indisponível, nunca 0.
  assert.equal(videoQuality([s(0, 100, 500), s(1000, 100, 500)]).qpAverage, null);
  assert.equal(videoQuality([s(0, 100, 500), s(1000, 160, 400)]).qpAverage, null);
  assert.equal(videoQuality([s(0, 100, null), s(1000, 160, null)]).qpAverage, null);
  // Segmento novo (contadores reiniciados) não mistura com o anterior.
  assert.equal(videoQuality([s(0, 900, 9000), s(1000, 10, 100, { segment: 2 })]).windowSeconds, null);
});
