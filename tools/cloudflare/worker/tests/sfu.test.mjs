import assert from "node:assert/strict";
import { test } from "node:test";
import { activeProvider, allocationProvider, createProvider, SfuError } from "../src/sfu.ts";
import { LiveKitProvider } from "../src/providers/livekit.ts";
import { StreamDO } from "../src/stream.ts";
import worker from "../src/index.ts";
import { signSession } from "../src/auth.ts";
import { SfuControl } from "../src/sfu-control.ts";
import { ApiError } from "../src/common.ts";

function localLiveKit() {
  const provider = new LiveKitProvider(livekitEnv);
  provider.rpc = async () => ({});
  return provider;
}

const livekitEnv = { LIVEKIT_URL: "wss://livekit.example.com", LIVEKIT_API_KEY: "test-key", LIVEKIT_API_SECRET: "test-secret" };
globalThis.WebSocketRequestResponsePair = class {};

function memoryContext(saved) {
  const store = { state: saved ? structuredClone(saved) : null, alarm: null };
  return {
    store,
    storage: {
      sql: { exec(query, ...args) {
        if (query.startsWith("INSERT INTO room_state")) store.state = JSON.parse(args[0]);
        if (query.startsWith("INSERT INTO sfu_control")) store.control = JSON.parse(args[0]);
        if (query.startsWith("SELECT data FROM sfu_control")) return { toArray: () => store.control ? [{data:JSON.stringify(store.control)}] : [] };
        return { toArray: () => query.startsWith("SELECT data FROM room_state") && store.state ? [{ data: JSON.stringify(store.state) }] : [] };
      } },
      getAlarm: async () => store.alarm,
      setAlarm: async (at) => { store.alarm = at; },
    },
    getWebSockets: () => [], setWebSocketAutoResponse() {},
    blockConcurrencyWhile: (fn) => fn(),
  };
}

function fakeProvider(id) {
  let count = 0;
  const calls = [];
  return {
    id, calls,
    async newSession(context) { calls.push(["new", context]); return `${id}-${++count}`; },
    async publish(sessionId, offer, mid, audioMid) {
      calls.push(["publish", sessionId, offer, mid, audioMid]);
      return { signaling: "sdp", sdp: "answer", tracks: [{ mid }, ...(audioMid ? [{ mid: audioMid }] : [])] };
    },
    async subscribe(sessionId, publisherId, audio) {
      calls.push(["subscribe", sessionId, publisherId, audio]);
      return { signaling: "sdp", sdp: "offer", tracks: [{ mid: "v" }] };
    },
    async renegotiate(...args) { calls.push(["renegotiate", ...args]); },
    async inspect(id) { calls.push(["inspect", id]); return [{ mid: "v" }]; },
    async close(...args) { calls.push(["close", ...args]); },
    async iceServers() { return { iceServers: [] }; },
    async selectLayer() {},
  };
}

function room(env = {}, saved) {
  const ctx = memoryContext(saved);
  const hub = { resolveShare: async () => ({ channelId: "channel" }), canWatch: async () => ({}), setLive: async () => true };
  const stream = new StreamDO(ctx, { HUB: { idFromName: x => x, get: () => hub }, ...env });
  return { stream, ctx, hub };
}

async function request(stream, path, body, uid = "owner", owner = "owner", sessionDigest) {
  const response = await stream.fetch(new Request(`https://worker.example.com/api/${path}`, {
    method: "POST", headers: { "Content-Type": "application/json", "x-ms-uid": uid, "x-ms-name": uid, "x-ms-owner": owner, ...(sessionDigest ? {"x-ms-session":sessionDigest} : {}) }, body: JSON.stringify(body),
  }));
  return { status: response.status, body: await response.json() };
}

test("configuração padrão e migração de estado antigo não dependem do provedor ativo", () => {
  assert.equal(activeProvider({}), "cloudflare");
  assert.equal(activeProvider({ SFU_PROVIDER: "livekit" }), "livekit");
  assert.equal(allocationProvider({}), "cloudflare");
  assert.throws(() => activeProvider({ SFU_PROVIDER: "typo" }));
  assert.throws(() => createProvider({ SFU_PROVIDER: "livekit" }));
});

test("failover automático conserva live antiga e roteia novo publicador para a reserva", async () => {
  const env={...livekitEnv,CF_REALTIME_APP_ID:"test-app",CF_REALTIME_APP_SECRET:"test-secret",SFU_FAILOVER_ENABLED:"1"};
  const ctx=memoryContext();
  const control=new SfuControl(ctx.storage.sql,env,async()=>{});
  const hub={resolveShare:async()=>({channelId:"channel"}),canWatch:async()=>({}),setLive:async()=>true,
    selectSfu:()=>control.select(),recordMediaUsage:(...args)=>control.noteUsage(...args),
    recordSfuOutcome:(...args)=>control.outcome(...args.slice(0,4),!args[1])};
  env.HUB={idFromName:x=>x,get:()=>hub};
  const first=room(env),cf=fakeProvider("cloudflare");first.stream.providers.set("cloudflare",cf);
  const published=await request(first.stream,"share/start",{offer:"offer",mid:"0",expectedProvider:"cloudflare"});
  assert.equal(published.status,200);
  const oldId=first.ctx.store.state.publisher.sessionId;
  control.outcome("cloudflare",false,10,503,true);
  assert.equal(await control.select(),"livekit");
  const watching=await request(first.stream,"watch/start",{},"friend");
  assert.equal(watching.status,200);assert.equal(watching.body.provider,"cloudflare");
  assert.equal(first.ctx.store.state.publisher.sessionId,oldId);
  const mismatched=await request(first.stream,"share/start",{offer:"offer",mid:"0",providers:["livekit"],expectedProvider:"cloudflare"});
  assert.equal(mismatched.status,409);assert.equal(first.ctx.store.state.publisher.sessionId,oldId);
  const second=room(env);second.stream.providers.set("livekit",localLiveKit());
  const reserve=await request(second.stream,"share/start",{providers:["livekit"],expectedProvider:"livekit"},"second","second");
  assert.equal(reserve.status,200);assert.equal(reserve.body.provider,"livekit");
  assert.equal(reserve.body.livekit.signaling,"livekit");
  assert.equal(first.ctx.store.state.publisher.sessionId,oldId);
  assert.equal(cf.calls.filter(call=>call[0]==="close").length,0);
});

test("sondas de saúde usam criação autenticada; LiveKit limpa apenas sua sala temporária", async () => {
  const provider=localLiveKit(),calls=[];
  provider.rpc=async(...args)=>{calls.push(args);return {};};
  await provider.healthCheck();
  assert.deepEqual(calls.map(call=>call[0]),["CreateRoom","DeleteRoom"]);
  assert.equal(calls[0][1],calls[1][1]);
  assert.equal(calls[0][2].empty_timeout,10);
  assert.equal(calls[0][3],3000);
  assert.equal(calls[1][3],3000);
  const originalFetch=globalThis.fetch;const requests=[];
  try {
    globalThis.fetch=async(url,init)=>{requests.push([url,init]);return Response.json({sessionId:"health-probe"});};
    await createProvider({CF_REALTIME_APP_ID:"app",CF_REALTIME_APP_SECRET:"test-secret"},"cloudflare").healthCheck();
    assert.equal(requests.length,1);assert.match(requests[0][0],/\/sessions\/new$/);
    assert.equal(requests[0][1].headers.Authorization,"Bearer test-secret");
    assert.equal(requests[0][1].body,undefined);
  } finally {globalThis.fetch=originalFetch;}
});

test("timeout ou falha de limpeza da sonda não viram sucesso de controle", async () => {
  const provider = localLiveKit();
  const calls = [];
  provider.rpc = async (method) => {
    calls.push(method);
    if (method === "DeleteRoom") throw new SfuError(504, "private-detail");
    return {};
  };
  await assert.rejects(provider.healthCheck(), error => error instanceof SfuError && error.upstreamStatus === 504);
  assert.deepEqual(calls, ["CreateRoom", "DeleteRoom"]);
  const original = globalThis.fetch;
  try {
    globalThis.fetch = async (_url, init) => {
      assert.ok(init.signal instanceof AbortSignal);
      throw new DOMException("private-timeout", "TimeoutError");
    };
    await assert.rejects(new LiveKitProvider(livekitEnv).healthCheck(), error => error instanceof SfuError && !error.message.includes("private-timeout"));
  } finally { globalThis.fetch = original; }
});

test("StreamDO mantém contratos Cloudflare, áudio, answer e limpeza com provedor falso", async () => {
  const { stream, ctx } = room();
  const cf = fakeProvider("cloudflare");
  stream.providers.set("cloudflare", cf);
  const pub = await request(stream, "share/start", { offer: "offer", mid: "0", audioMid: "1" });
  assert.equal(pub.status, 200);
  assert.equal(pub.body.answer, "answer");
  assert.equal(pub.body.provider, "cloudflare");
  assert.equal(ctx.store.state.publisher.provider, "cloudflare");
  const viewer = await request(stream, "watch/start", { audio: true }, "friend");
  assert.equal(viewer.status, 200);
  assert.equal(viewer.body.offer, "offer");
  assert.equal(cf.calls.find(c => c[0] === "subscribe")[3], true);
  assert.equal((await request(stream, "watch/answer", { viewerId: viewer.body.viewerId, answer: "answer" }, "friend")).status, 200);
  await request(stream, "share/stop", { generation: pub.body.generation });
  await stream.cleanPending();
  assert.equal(cf.calls.filter(c => c[0] === "close").length, 2);
});

test("reinício e troca do secret preservam roteamento de assinatura/limpeza de sessões antigas", async () => {
  const first = room();
  first.stream.providers.set("cloudflare", fakeProvider("cloudflare"));
  const pub = await request(first.stream, "share/start", { offer: "offer", mid: "0" });
  delete first.ctx.store.state.publisher.provider; // JSON do deploy antigo.
  const { stream } = room({ SFU_PROVIDER: "livekit" }, first.ctx.store.state);
  const cf = fakeProvider("cloudflare");
  const lk = fakeProvider("livekit");
  stream.providers.set("cloudflare", cf);
  stream.providers.set("livekit", lk);
  assert.equal((await request(stream, "watch/start", {}, "friend")).body.provider, "cloudflare");
  await request(stream, "share/stop", { generation: pub.body.generation });
  await stream.cleanPending();
  assert.ok(cf.calls.some(c => c[0] === "close"));
  assert.equal(lk.calls.length, 0);
});

test("cliente sem LiveKit é recusado antes de substituir a transmissão atual", async () => {
  const first = room();
  first.stream.providers.set("cloudflare", fakeProvider("cloudflare"));
  await request(first.stream, "share/start", { offer: "offer", mid: "0" });
  const { stream, ctx } = room({ SFU_PROVIDER: "livekit", ...livekitEnv }, first.ctx.store.state);
  const response = await request(stream, "share/start", { offer: "offer", mid: "0" });
  assert.equal(response.status, 409);
  assert.equal(response.body.error, "cliente_desatualizado");
  assert.deepEqual(ctx.store.state.publisher, first.ctx.store.state.publisher);
});

test("StreamDO devolve conexão LiveKit sem persistir credenciais e preserva a prévia sem áudio", async () => {
  const { stream, ctx } = room({ SFU_PROVIDER: "livekit", ...livekitEnv });
  stream.providers.set("livekit", localLiveKit());
  const started = await request(stream, "share/start", { providers: ["livekit"], audioMid: "audio" });
  assert.equal(started.status, 200);
  assert.equal(started.body.provider, "livekit");
  assert.equal(started.body.answer, undefined);
  assert.equal(started.body.livekit.signaling, "livekit");
  const own = await request(stream, "watch/start", { providers: ["livekit"], audio: true });
  const friend = await request(stream, "watch/start", { providers: ["livekit"], audio: true }, "friend");
  assert.equal(own.body.audio, false);
  assert.equal(friend.body.audio, true);
  assert.equal(friend.body.livekit.room, started.body.livekit.room);
  assert.equal(ctx.store.state.viewers[0].answered, true);
  assert.ok(!JSON.stringify(ctx.store.state).includes(started.body.livekit.token));
  assert.ok(!JSON.stringify(ctx.store.state).includes(livekitEnv.LIVEKIT_API_SECRET));
  assert.equal((await request(stream, "watch/start", {}, "old-client")).status, 409);
});

test("limpeza LiveKit repete remoção além do TTL mesmo se a sessão ainda não existe", async () => {
  const { stream, ctx } = room({ SFU_PROVIDER: "livekit" });
  const lk = fakeProvider("livekit");
  lk.close = async () => { throw new SfuError(404); };
  stream.providers.set("livekit", lk);
  stream.addPending({ provider: "livekit", sessionId: "livekit-session", mids: [] });
  await stream.cleanPending();
  assert.equal(ctx.store.state.pending.length, 1);
  assert.ok(!lk.calls.some(c => c[0] === "inspect"));
  stream.state.pending[0].createdAt -= 71_000;
  await stream.cleanPending();
  assert.equal(ctx.store.state.pending.length, 0);
});

async function claims(jwt) {
  const [header, body, signature] = jwt.split(".");
  assert.equal(JSON.parse(Buffer.from(header, "base64url")).alg, "HS256");
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(livekitEnv.LIVEKIT_API_SECRET), { name: "HMAC", hash: "SHA-256" }, false, ["verify"]);
  assert.ok(await crypto.subtle.verify("HMAC", key, Buffer.from(signature, "base64url"), new TextEncoder().encode(`${header}.${body}`)));
  return JSON.parse(Buffer.from(body, "base64url"));
}

test("LiveKit emite tokens assinados de sala única com permissões distintas", async () => {
  const lk = localLiveKit();
  const publisher = await lk.newSession({ role: "publisher" });
  const publication = await lk.publish(publisher, "", "screen", "audio");
  const p = await claims(publication.token);
  assert.equal(p.iss, "test-key");
  assert.equal(p.sub, publication.identity);
  assert.equal(p.video.room, publication.room);
  assert.equal(p.video.canPublish, true);
  assert.equal(p.video.canSubscribe, false);
  assert.deepEqual(p.video.canPublishSources, ["screen_share", "screen_share_audio"]);
  assert.ok(p.exp - p.nbf <= 65);
  const viewer = await lk.newSession({ role: "viewer", publisherId: publisher });
  const subscription = await lk.subscribe(viewer, publisher, true);
  const v = await claims(subscription.token);
  assert.equal(v.video.room, p.video.room);
  assert.equal(subscription.publisherIdentity, publication.identity);
  assert.notEqual(v.sub, p.sub);
  assert.equal(v.video.canPublish, false);
  assert.equal(v.video.canSubscribe, true);
  assert.equal(v.video.canPublishData, false);
  const other = await lk.newSession({ role: "publisher" });
  await assert.rejects(lk.subscribe(viewer, other));
  await assert.rejects(lk.selectLayer(viewer, "low"));
});

test("LiveKit fecha viewer/sala com RPC autenticado e não reflete erros privados", async () => {
  const original = globalThis.fetch;
  const calls = [];
  try {
    globalThis.fetch = async (url, init) => {
      const auth = await claims(init.headers.Authorization.slice(7));
      calls.push({ url: url.toString(), body: JSON.parse(init.body), auth });
      return Response.json({});
    };
    const lk = new LiveKitProvider(livekitEnv);
    const pub = await lk.newSession({ role: "publisher" });
    const viewer = await lk.newSession({ role: "viewer", publisherId: pub });
    await lk.close(viewer, []);
    await lk.close(pub, []);
    assert.match(calls[0].url, /\/RemoveParticipant$/);
    assert.match(calls[1].url, /^https:.*\/DeleteRoom$/);
    assert.equal(calls[0].auth.video.roomAdmin, true);
    globalThis.fetch = async () => { throw new Error("private-token"); };
    await assert.rejects(lk.close(pub, []), error => error instanceof SfuError && !error.message.includes("private-token"));
  } finally { globalThis.fetch = original; }
});

test("Cloudflare adapter preserva rotas e payloads do protocolo", async () => {
  const original = globalThis.fetch;
  const calls = [];
  const responses = [
    { sessionId: "publisher" },
    { sessionDescription: { type: "answer", sdp: "answer" }, tracks: [{ mid: "0" }, { mid: "1" }] },
    { sessionDescription: { type: "offer", sdp: "offer" }, tracks: [{ mid: "v" }], requiresImmediateRenegotiation: true },
  ];
  try {
    globalThis.fetch = async (url, init) => { calls.push({ url, body: JSON.parse(init.body ?? "null") }); return Response.json(responses.shift()); };
    const cf = createProvider({ CF_REALTIME_APP_ID: "app", CF_REALTIME_APP_SECRET: "secret" });
    assert.equal(await cf.newSession({ role: "publisher" }), "publisher");
    assert.equal((await cf.publish("publisher", "offer", "0", "1")).sdp, "answer");
    assert.equal((await cf.subscribe("viewer", "publisher")).sdp, "offer");
    assert.match(calls[0].url, /\/apps\/app\/sessions\/new$/);
    assert.deepEqual(calls[1].body.tracks, [{ location: "local", mid: "0", trackName: "screen" }, { location: "local", mid: "1", trackName: "audio" }]);
    assert.deepEqual(calls[2].body.tracks, [{ location: "remote", sessionId: "publisher", trackName: "screen" }]);
  } finally { globalThis.fetch = original; }
});

test("camada inferior é recusada e camada h usa o contrato do provedor", async () => {
  const { stream } = room();
  const provider = fakeProvider("cloudflare");
  stream.providers.set("cloudflare", provider);
  await request(stream, "share/start", { offer: "offer", mid: "0" });
  assert.equal((await request(stream, "watch/start", { preferredRid: "l" }, "friend")).status, 409);
  const started = await request(stream, "watch/start", { preferredRid: "h" }, "friend");
  const body = { viewerId: started.body.viewerId, preferredRid: "h" };
  assert.equal((await request(stream, "watch/layer", body, "friend")).status, 200);
  assert.equal((await request(stream, "watch/layer", { ...body, preferredRid: "l" }, "friend")).status, 409);
  assert.equal((await request(stream, "watch/layer", body, "stranger")).status, 403);
});

test("mudança do provedor entre consulta e start não substitui a sessão atual", async () => {
  const { stream, ctx } = room();
  stream.providers.set("cloudflare", fakeProvider("cloudflare"));
  await request(stream, "share/start", { offer: "offer", mid: "0" });
  const before = structuredClone(ctx.store.state.publisher);
  const response = await request(stream, "share/start", { expectedProvider: "livekit", providers: ["livekit"] });
  assert.equal(response.status, 409);
  assert.equal(response.body.error, "provedor_alterado");
  assert.deepEqual(ctx.store.state.publisher, before);
});

test("configuração pública de SFU exige autenticação e mantém Cloudflare por padrão", async () => {
  const env={SESSION_SECRET:"local-signature",...livekitEnv,HUB:{idFromName:x=>x,get:()=>({assertSession:async()=>{}})}};
  const request=new Request("https://worker.example.com/api/sfu");
  assert.equal((await worker.fetch(request,env)).status,401);
  const session=await signSession("u_aaaaaaaaaaaaaaaa",env.SESSION_SECRET);
  const authed=new Request(request,{headers:{Authorization:`Bearer ${session.token}`}});
  const result=await worker.fetch(authed,env);
  assert.deepEqual(await result.json(),{provider:"cloudflare",layers:["h"]});
  const backup=await worker.fetch(authed,{...env,SFU_PROVIDER:"livekit"});
  assert.equal((await backup.json()).provider,"livekit");
});

test("CSP do viewer libera somente a origem LiveKit configurada e não embute credenciais", async () => {
  const result=await worker.fetch(new Request("https://worker.example.com/watch"),livekitEnv);
  const policy=result.headers.get("Content-Security-Policy");
  assert.match(policy,/connect-src[^;]*wss:\/\/livekit.example.com https:\/\/livekit.example.com/);
  assert.ok(!policy.includes("test-secret"));
  assert.ok(!(await result.text()).includes("test-secret"));
});


test("revogacao LiveKit apos reinicio aposenta sala inteira para impedir token renovado de voltar a midia", async () => {
  const first=room({SFU_PROVIDER:"livekit",...livekitEnv});
  first.stream.providers.set("livekit",localLiveKit());
  const publication=await request(first.stream,"share/start",{providers:["livekit"]});
  await request(first.stream,"watch/start",{providers:["livekit"],audio:true},"friend");
  const copiedRoom=publication.body.livekit.room;
  const resumed=room({SFU_PROVIDER:"livekit",...livekitEnv},first.ctx.store.state);
  const provider=localLiveKit(),calls=[];
  provider.rpc=async(method,room)=>{calls.push([method,room]);return {}};
  resumed.stream.providers.set("livekit",provider);
  await resumed.stream.revokeViewer("friend");
  assert.equal(resumed.ctx.store.state.publisher,null);
  assert.equal(resumed.ctx.store.state.viewers.length,0);
  assert.ok(calls.some(([method,room])=>method==="DeleteRoom"&&room===copiedRoom));
  assert.equal((await request(resumed.stream,"share/heartbeat",{generation:publication.body.generation})).status,409);
  const next=await request(resumed.stream,"share/start",{providers:["livekit"]});
  assert.equal(next.status,200);
  assert.notEqual(next.body.livekit.room,copiedRoom);
});


test("falha de fechamento nao confirma revogacao e a mesma retirada pode ser repetida apos reinicio", async () => {
  const initial=room({SFU_PROVIDER:"livekit",...livekitEnv});initial.stream.providers.set("livekit",localLiveKit());
  await request(initial.stream,"share/start",{providers:["livekit"]});await request(initial.stream,"watch/start",{providers:["livekit"]},"friend");
  const failed=localLiveKit();failed.rpc=async()=>{throw new SfuError(503,"unavailable")};initial.stream.providers.set("livekit",failed);
  await assert.rejects(initial.stream.revokeViewer("friend"),error=>error.code==="revogacao_pendente");
  assert.equal(initial.ctx.store.state.publisher,null);assert.ok(initial.ctx.store.state.pending.length>0);
  const restarted=room({SFU_PROVIDER:"livekit",...livekitEnv},initial.ctx.store.state);restarted.stream.providers.set("livekit",failed);
  await assert.rejects(restarted.stream.revokeViewer("friend"),error=>error.code==="revogacao_pendente");
  const healthy=localLiveKit(),calls=[];healthy.rpc=async(method)=>{calls.push(method);return {}};restarted.stream.providers.set("livekit",healthy);
  await restarted.stream.revokeViewer("friend");assert.ok(calls.includes("DeleteRoom"));
});


test("logout apos watch/leave ainda revoga tokens renovados e gera nova sala sem resetar contas", async () => {
  const first=room({SFU_PROVIDER:"livekit",...livekitEnv});first.stream.providers.set("livekit",localLiveKit());
  const pub=await request(first.stream,"share/start",{providers:["livekit"]});
  const viewer=await request(first.stream,"watch/start",{providers:["livekit"]},"friend");
  assert.equal((await request(first.stream,"watch/leave",{viewerId:viewer.body.viewerId},"friend")).status,200);
  assert.equal(first.ctx.store.state.viewers.length,0);
  assert.ok(first.ctx.store.state.authorizedViewerUids.includes("friend"));
  first.ctx.store.state.pending = []; // Cleanup TTL may expire before a server-refreshed token.
  const resumed=room({SFU_PROVIDER:"livekit",...livekitEnv},first.ctx.store.state);resumed.stream.providers.set("livekit",localLiveKit());
  await resumed.stream.revokeViewer("friend");assert.equal(resumed.ctx.store.state.publisher,null);
  const next=await request(resumed.stream,"share/start",{providers:["livekit"]});assert.notEqual(next.body.livekit.room,pub.body.livekit.room);
});


test("migracao sem historico de tokens fecha geracao LiveKit e o registro novo tem limite", async () => {
  const first=room({SFU_PROVIDER:"livekit",...livekitEnv});first.stream.providers.set("livekit",localLiveKit());
  await request(first.stream,"share/start",{providers:["livekit"]});
  const saved=structuredClone(first.ctx.store.state);delete saved.authorizedViewerUids;
  const legacy=room({SFU_PROVIDER:"livekit",...livekitEnv},saved);legacy.stream.providers.set("livekit",localLiveKit());
  await legacy.stream.revokeViewer("old-viewer");assert.equal(legacy.ctx.store.state.publisher,null);
  saved.authorizedViewerUids=Array.from({length:512},(_,i)=>`viewer-${i}`);
  const full=room({SFU_PROVIDER:"livekit",...livekitEnv},saved);full.stream.providers.set("livekit",localLiveKit());
  assert.equal((await request(full.stream,"watch/start",{providers:["livekit"]},"new-viewer")).status,429);
});


test("sessao revogada entre autenticar e despachar ou durante subscribe nao recebe nova autorizacao", async () => {
  const {stream,hub,ctx}=room();const provider=fakeProvider("cloudflare");stream.providers.set("cloudflare",provider);
  await request(stream,"share/start",{offer:"offer",mid:"0"});
  let revoked=true;
  hub.assertSession=async(uid,digest)=>{assert.equal(uid,"friend");assert.equal(digest,"digest-only");if(revoked)throw new ApiError(401,"sessao_revogada","Sessao encerrada");};
  const watch=()=>request(stream,"watch/start",{},"friend","owner","digest-only");
  assert.equal((await watch()).status,401);
  assert.equal(provider.calls.filter(([name])=>name==="subscribe").length,0);
  revoked=false;
  const subscribe=provider.subscribe;
  provider.subscribe=async(...args)=>{const result=await subscribe(...args);revoked=true;return result;};
  assert.equal((await watch()).status,401);
  assert.equal(ctx.store.state.viewers.length,0);
  assert.ok(ctx.store.state.pending.length>0 || provider.calls.some(([name])=>name==="close"));
});
