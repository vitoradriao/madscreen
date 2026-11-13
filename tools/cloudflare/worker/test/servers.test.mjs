import assert from "node:assert/strict";
import { test } from "node:test";
import { DatabaseSync } from "node:sqlite";
import { build } from "esbuild";
import { createHmac } from "node:crypto";

// Usa SQLite real e os handlers reais; apenas o runtime DO/SFU é substituído no teste local.
const bundle = await build({ stdin: { contents: 'export { HubDO, default as worker } from "./src/index.ts"; export { signSession } from "./src/auth.ts";', resolveDir: process.cwd() }, bundle: true, write: false, format: "esm", platform: "node", plugins: [{ name: "do-runtime", setup(builder) {
  builder.onResolve({ filter: /^cloudflare:workers$/ }, () => ({ path: "runtime", namespace: "test" }));
  builder.onLoad({ filter: /.*/, namespace: "test" }, () => ({ contents: "export class DurableObject { constructor(ctx,env) { this.ctx=ctx;this.env=env; } }" }));
} }] });
const { HubDO, worker, signSession } = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`);
globalThis.WebSocketRequestResponsePair = class {};
WebSocket.READY_STATE_OPEN = WebSocket.OPEN;

function database() {
  const db = new DatabaseSync(":memory:");
  let transactionId = 0;
  const storage = {
    getAlarm: async () => null, setAlarm: async () => {},
    sql: { exec(query, ...args) {
      let rows = [];
      if (!args.length && query.trim().split(";").filter(part => part.trim()).length > 1) db.exec(query);
      else rows = db.prepare(query).all(...args);
      return { toArray: () => rows, one: () => { assert.equal(rows.length, 1); return rows[0]; }, [Symbol.iterator]: () => rows[Symbol.iterator]() };
    } },
    transactionSync(operation) {
      const name = `tx_${++transactionId}`;
      db.exec(`SAVEPOINT ${name}`);
      try { const result = operation(); db.exec(`RELEASE ${name}`); return result; }
      catch (error) { db.exec(`ROLLBACK TO ${name}; RELEASE ${name}`); throw error; }
    },
  };
  const sockets = [];
  const ctx = { storage, getWebSockets: tag => sockets.filter(socket => socket.tag === tag), setWebSocketAutoResponse() {} };
  const media = [];
  const stream = { forceStop: async uid => { media.push(["stop", uid]); }, revokeViewer: async uid => { media.push(["revoke", uid]); }, fetch: async request => { media.push(["forward", request.headers.get("x-ms-owner"), request.headers.get("x-ms-channel"), request.headers.has("Authorization")]); return Response.json({ ok: true }); } };
  const env = { SESSION_SECRET: "local-fixture-secret-never-used-in-production", HUB: { idFromName: name => name, get: () => hub }, STREAM: { idFromName: name => name, get: () => stream } };
  let hub;
  const open = () => { hub = new HubDO(ctx, env); return hub; };
  return { db, storage, env, media, sockets, open, get hub() { return hub; } };
}
const failure = code => error => error?.code === code;

test("Discord mantém identidade entre clientes e reinício; callback e PKCE são de uso único", async () => {
  const f = database();
  try {
    let hub = f.open();
    const first = await hub.oauthBegin("discord", "client-one", null);
    const profile = { sub: "stable-discord-id", username: "first_name", displayName: "Nome", avatar: "https://cdn.discordapp.com/old.png" };
    await hub.oauthFinish(first, "discord", profile);
    assert.equal(await hub.exchange("wrong-verifier"), null);
    const uid = await hub.exchange("client-one");
    assert.ok(uid); assert.equal(await hub.exchange("client-one"), null);
    await assert.rejects(hub.oauthFinish(first, "discord", profile), failure("estado_invalido"));
    hub = f.open();
    const second = await hub.oauthBegin("discord", "client-two", null);
    await hub.oauthFinish(second, "discord", { ...profile, username: "changed_name", avatar: "https://cdn.discordapp.com/new.png" });
    assert.equal(await hub.exchange("client-two"), uid);
    assert.equal(f.db.prepare("SELECT COUNT(*) AS n FROM identities WHERE provider='discord'").get().n, 1);
    assert.equal((await hub.me(uid)).user.avatar, "https://cdn.discordapp.com/new.png");
  } finally { f.db.close(); }
});

test("estado OAuth expirado ou de outro provedor e handoff expirado são recusados", async () => {
  const f = database(), originalNow = Date.now;
  let time = originalNow(); Date.now = () => time;
  try {
    const hub = f.open(), profile = { sub: "test-sub", username: "fixture", displayName: "Fixture", avatar: null };
    const wrong = await hub.oauthBegin("discord", "wrong-provider", null);
    await assert.rejects(hub.oauthFinish(wrong, "google", profile), failure("estado_invalido"));
    const expired = await hub.oauthBegin("discord", "expired", null);
    time += 601000;
    await assert.rejects(hub.oauthFinish(expired, "discord", profile), failure("estado_invalido"));
    const valid = await hub.oauthBegin("discord", "handoff", null);
    await hub.oauthFinish(valid, "discord", profile); time += 301000;
    assert.equal(await hub.exchange("handoff"), null);
  } finally { Date.now = originalNow; f.db.close(); }
});
const profile = name => ({ sub: `discord-${name}`, username: name, displayName: name, avatar: null });
async function oauth(hub, name, invite = null) {
  const challenge = `challenge-${name}`;
  const state = await hub.oauthBegin("discord", challenge, invite);
  await hub.oauthFinish(state, "discord", profile(name));
  const uid = await hub.exchange(challenge);
  if (invite) await hub.redeemInvite(uid, "oauth-fixture", invite);
  return uid;
}
async function api(fixture, uid, method, path, body) {
  const { token } = await signSession(uid, fixture.env.SESSION_SECRET);
  return worker.fetch(new Request(`https://fixture.example${path}`, { method, headers: { Authorization: `Bearer ${token}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) }), fixture.env);
}

test("migração mantém contas, usos dos convites e círculos privados de amizade", async () => {
  const f = database();
  f.db.exec(`CREATE TABLE users(id TEXT PRIMARY KEY,username TEXT,display TEXT,avatar TEXT,recovery_hash TEXT,created_at INTEGER,last_seen INTEGER);
    CREATE TABLE friends(a TEXT,b TEXT,status TEXT,requester TEXT,created_at INTEGER,PRIMARY KEY(a,b));
    CREATE TABLE invites(code TEXT PRIMARY KEY,user_id TEXT,expires INTEGER,max_uses INTEGER,uses INTEGER);
    CREATE TABLE live(user_id TEXT PRIMARY KEY,title TEXT,since INTEGER,viewers INTEGER);
    INSERT INTO users VALUES ('u_aaaaaaaaaaaaaaaa','alice','Alice',NULL,'keep-hash',1,1),('u_bbbbbbbbbbbbbbbb','bob','Bob',NULL,NULL,2,2),('u_cccccccccccccccc','carol','Carol',NULL,NULL,3,3);
    INSERT INTO friends VALUES ('u_aaaaaaaaaaaaaaaa','u_bbbbbbbbbbbbbbbb','accepted','u_aaaaaaaaaaaaaaaa',1);
    INSERT INTO invites VALUES ('ABCD-EFGH','u_aaaaaaaaaaaaaaaa',9999999999,5,3);`);
  const hub = f.open();
  const alice = await hub.directorySnapshot("u_aaaaaaaaaaaaaaaa");
  const carol = await hub.directorySnapshot("u_cccccccccccccccc");
  assert.equal(alice.servers.length, 2);
  assert.equal(carol.servers.length, 1);
  assert(!carol.servers.some(server => server.members.some(member => member.username === "alice")));
  assert.equal(f.db.prepare("SELECT recovery_hash FROM users WHERE username='alice'").get().recovery_hash, "keep-hash");
  const preview = await hub.previewInvite("local", "ABCD-EFGH");
  assert.equal(preview.server.name, "Alice e amigos");
  assert.equal(f.db.prepare("SELECT uses FROM invites").get().uses, 3);
  f.open();
  assert.equal(f.db.prepare("SELECT COUNT(*) AS n FROM servers").get().n, 3);
  f.db.close();
});

test("convite entra no canal, é idempotente e preserva identidade OAuth", async () => {
  const f = database(); const hub = f.open();
  const alice = await oauth(hub, "alice");
  const made = await hub.changeDirectory(alice, "create", "", "Squad");
  const destination = made.snapshot.destination;
  const invitation = await hub.createInvite(alice, destination.serverId, destination.channelId);
  const bob = await oauth(hub, "bob", invitation.code);
  const joined = await hub.redeemInvite(bob, "local", invitation.code);
  assert.deepEqual(joined.destination, destination);
  assert.equal(joined.servers[0].channels[0].id, destination.channelId);
  assert.equal(f.db.prepare("SELECT uses FROM invites WHERE code=?").get(invitation.code).uses, 1);
  assert.equal(await oauth(hub, "bob", invitation.code), bob);
  await hub.redeemInvite(bob, "local", invitation.code);
  assert.equal(f.db.prepare("SELECT uses FROM invites WHERE code=?").get(invitation.code).uses, 1);
  const guestInvite = await hub.createInvite(alice, destination.serverId, destination.channelId);
  const registered = await hub.register("local", "guest", guestInvite.code);
  assert.equal((await hub.directorySnapshot(registered.uid)).servers[0].id, destination.serverId);
  const exhausted = await hub.createInvite(alice, destination.serverId, destination.channelId);
  f.db.prepare("UPDATE invites SET uses=max_uses WHERE code=?").run(exhausted.code);
  await assert.rejects(hub.previewInvite("local", exhausted.code), failure("convite_esgotado"));
  f.db.prepare("UPDATE invites SET expires=1 WHERE code=?").run(guestInvite.code);
  await assert.rejects(hub.redeemInvite(bob, "local", guestInvite.code), failure("convite_expirado"));
  await assert.rejects(hub.previewInvite("local", "ZZZZ-ZZZZ"), failure("convite_invalido"));
  await hub.changeDirectory(alice, "role", destination.serverId, "admin", bob);
  await assert.rejects(hub.changeDirectory(alice, "leave", destination.serverId), failure("transfira_propriedade"));
  await hub.changeDirectory(alice, "transfer", destination.serverId, undefined, bob);
  await hub.changeDirectory(alice, "leave", destination.serverId);
  const inherited = (await hub.directorySnapshot(bob)).servers.find(server => server.id === destination.serverId);
  assert.equal(inherited.ownerId, bob);
  assert.equal(inherited.role, "owner");
  f.db.close();
});

test("permissões, publicação, isolamento e revogação usam o servidor do canal", async () => {
  const f = database(); const hub = f.open();
  const alice = await oauth(hub, "alice"); const bob = await oauth(hub, "bob"); const carol = await oauth(hub, "carol");
  const made = await hub.changeDirectory(alice, "create", "", "Privado"); const d = made.snapshot.destination;
  const invite = await hub.createInvite(alice, d.serverId, d.channelId); await hub.redeemInvite(bob, "local", invite.code);
  let response = await api(f, bob, "POST", `/api/servers/${d.serverId}/channels`, { name: "Jogo" }); assert.equal(response.status, 403);
  response = await api(f, carol, "GET", `/api/servers/${d.serverId}`); assert.equal(response.status, 403);
  await hub.changeDirectory(alice, "role", d.serverId, "admin", bob);
  response = await api(f, bob, "POST", `/api/servers/${d.serverId}/channels`, { name: "Jogo" }); assert.equal(response.status, 200);
  let state = await response.json(); const channel = state.servers.find(server => server.id === d.serverId).channels.find(item => item.name === "Jogo");
  response = await api(f, bob, "POST", "/api/share/start", { channelId: channel.id }); assert.equal(response.status, 200);
  assert.deepEqual(f.media.at(-1), ["forward", bob, channel.id, false]);
  assert.equal(await hub.setLive(bob, { channelId: channel.id, since: 1, viewers: 0, title: "Jogo" }), true);
  assert.equal((await hub.canWatch(alice, "bob")).ownerId, bob);
  await hub.requestFriend(carol, "bob"); await hub.answerRequest(bob, carol, true);
  const other = (await hub.changeDirectory(carol, "create", "", "Carol")).snapshot.servers[0];
  const otherInvite = await hub.createInvite(carol, other.id, other.channels[0].id);
  await hub.redeemInvite(bob, "local", otherInvite.code);
  // Outro servidor em comum e um heartbeat antigo não concedem acesso ao canal privado.
  f.db.prepare("UPDATE live SET updated=1 WHERE user_id=?").run(bob);
  await assert.rejects(hub.canWatch(carol, "bob"), failure("sem_acesso_servidor"));
  await assert.rejects(hub.canWatch(carol, "bob", channel.id), failure("sem_acesso_servidor"));
  response = await api(f, carol, "POST", "/api/watch/start", { owner: "bob" }); assert.equal(response.status, 403);
  response = await api(f, bob, "DELETE", `/api/servers/${d.serverId}/channels/${channel.id}`); assert.equal(response.status, 200);
  assert(f.media.some(entry => entry[0] === "stop" && entry[1] === bob));
  assert.equal(await hub.setLive(bob, { channelId: channel.id, since: 1, viewers: 0, title: "Removed" }), false);
  await hub.setLive(alice, { channelId: d.channelId, since: 1, viewers: 0, title: null });
  response = await api(f, bob, "DELETE", `/api/servers/${d.serverId}/members/me`); assert.equal(response.status, 200);
  assert(f.media.some(entry => entry[0] === "revoke" && entry[1] === bob));
  await assert.rejects(hub.canWatch(bob, "alice"), failure("sem_acesso_servidor"));
  f.db.close();
});


test("logout revoga token copiado e descendente renovado, preserva outra sessao e persiste no reinicio", async () => {
  const f = database();
  try {
    const hub = f.open();
    const { uid } = await hub.register("127.0.0.1", "revocation_fixture", "");
    const first = await signSession(uid, f.env.SESSION_SECRET);
    const oldIat = Math.floor(Date.now()/1000)-31*86400;
    const old = await signSession(uid, f.env.SESSION_SECRET, oldIat);
    const headers = token => ({Authorization:`Bearer ${token}`});
    const get = token => worker.fetch(new Request("https://worker.example.com/api/me",{headers:headers(token)}),f.env);
    const refreshed = await (await get(old.token)).json();
    assert.equal(typeof refreshed.token,"string");
    const logout = await worker.fetch(new Request("https://worker.example.com/api/logout",{method:"POST",headers:headers(refreshed.token)}),f.env);
    assert.equal(logout.status,200);
    assert.equal((await get(old.token)).status,401);
    assert.equal((await get(refreshed.token)).status,401);
    assert.equal((await get(first.token)).status,200);
    assert.ok(f.media.some(([op,id])=>op==="stop"&&id===uid));
    f.open();
    assert.equal((await get(old.token)).status,401);
    assert.equal((await get(first.token)).status,200);
    assert.equal(f.db.prepare("SELECT COUNT(*) AS n FROM revoked_sessions").get().n,1);
  } finally { f.db.close(); }
});


test("expulsao exige administrador, preserva dono e revoga o alvo correto sem apagar contas", async () => {
  const f=database(); const hub=f.open();
  try {
    const owner=await oauth(hub,"kick_owner"), member=await oauth(hub,"kick_member"), stranger=await oauth(hub,"kick_stranger");
    const made=await hub.changeDirectory(owner,"create","","Kick fixture");const d=made.snapshot.destination;
    const invite=await hub.createInvite(owner,d.serverId,d.channelId); await hub.redeemInvite(member,"local",invite.code);
    await hub.setLive(owner,{channelId:d.channelId,since:1,viewers:1,title:null});
    assert.equal((await api(f,stranger,"DELETE",`/api/servers/${d.serverId}/members/${member}`)).status,403);
    assert.equal((await api(f,member,"DELETE",`/api/servers/${d.serverId}/members/${owner}`)).status,403);
    assert.equal((await api(f,owner,"DELETE",`/api/servers/${d.serverId}/members/${member}`)).status,200);
    assert.ok(f.media.some(([op,id])=>op==="revoke"&&id===member));
    assert.ok(!f.media.some(([op,id])=>op==="revoke"&&id===owner));
    assert.equal((await api(f,member,"POST","/api/watch/start",{owner})).status,403);
    assert.equal((await api(f,member,"GET","/api/me")).status,200);
    f.open();
    assert.equal((await api(f,member,"POST","/api/watch/start",{owner})).status,403);
  } finally {f.db.close();}
});


test("token legado sem j migra sua linhagem na renovacao e logout invalida as duas versoes", async () => {
  const f=database();const hub=f.open();
  try {
    const {uid}=await hub.register("127.0.0.1","legacy_revocation","");
    const now=Math.floor(Date.now()/1000),iat=now-31*86400;
    const encoded=Buffer.from(JSON.stringify({v:1,u:uid,iat,exp:iat+90*86400})).toString("base64url");
    const legacy=encoded+"."+createHmac("sha256",f.env.SESSION_SECRET).update(encoded).digest("base64url");
    const headers=token=>({Authorization:`Bearer ${token}`});
    const get=token=>worker.fetch(new Request("https://worker.example.com/api/me",{headers:headers(token)}),f.env);
    const response=await get(legacy);assert.equal(response.status,200);const renewed=await response.json();assert.equal(typeof renewed.token,"string");
    const logout=await worker.fetch(new Request("https://worker.example.com/api/logout",{method:"POST",headers:headers(renewed.token)}),f.env);assert.equal(logout.status,200);
    f.open();assert.equal((await get(legacy)).status,401);assert.equal((await get(renewed.token)).status,401);
    assert.equal(f.db.prepare("SELECT COUNT(*) AS n FROM users WHERE id=?").get(uid).n,1);
  } finally {f.db.close();}
});

test("troca da chave de recuperacao invalida a antiga e nao vale para conta OAuth", async () => {
  const f = database();
  try {
    const hub = f.open();
    const { uid, recoveryKey } = await hub.register("127.0.0.1", "rotation_fixture", "");
    const { token } = await signSession(uid, f.env.SESSION_SECRET);
    const call = (method, path) => worker.fetch(new Request(`https://worker.example.com${path}`, { method, headers: { Authorization: `Bearer ${token}` } }), f.env);
    assert.deepEqual((await (await call("GET", "/api/me")).json()).account, { recoveryKey: true });
    const older = (await signSession(uid, f.env.SESSION_SECRET, Math.floor(Date.now() / 1000) - 10)).token;
    const closes = [];
    f.sockets.push({ tag: `u:${uid}`, close: (code, reason) => closes.push([code, reason]) }, { tag: "u:outro", close: () => closes.push("indevido") });
    const rotated = await call("POST", "/api/me/recovery-key");
    assert.equal(rotated.status, 200);
    assert.deepEqual(closes, [[4001, "sessao encerrada"]]);
    const { recoveryKey: next, token: renewed } = await rotated.json();
    assert.equal((await worker.fetch(new Request("https://worker.example.com/api/me", { headers: { Authorization: `Bearer ${older}` } }), f.env)).status, 401);
    assert.equal((await worker.fetch(new Request("https://worker.example.com/api/me", { headers: { Authorization: `Bearer ${renewed}` } }), f.env)).status, 200);
    assert.match(next, /^[A-Z0-9]{5}(-[A-Z0-9]{5}){3}$/);
    assert.notEqual(next, recoveryKey);
    await assert.rejects(hub.login("127.0.0.1", "rotation_fixture", recoveryKey), failure("credenciais_invalidas"));
    assert.equal((await hub.login("127.0.0.1", "rotation_fixture", next)).uid, uid);
    const discord = await oauth(hub, "rotation_discord");
    const oauthToken = (await signSession(discord, f.env.SESSION_SECRET)).token;
    const oauthCall = (method, path) => worker.fetch(new Request(`https://worker.example.com${path}`, { method, headers: { Authorization: `Bearer ${oauthToken}` } }), f.env);
    assert.deepEqual((await (await oauthCall("GET", "/api/me")).json()).account, { recoveryKey: false });
    assert.equal((await oauthCall("POST", "/api/me/recovery-key")).status, 409);
  } finally { f.db.close(); }
});


function online(f, uid) {
  const socket = { tag: `u:${uid}`, readyState: 1, send() {}, close() { this.readyState = 3; } };
  f.sockets.push(socket); return socket;
}

test("convite pessoal: cadastro sem servidor, concorrência, repetição e relação removida", async () => {
  const f = database();
  try {
    const hub = f.open(), ana = await oauth(hub, "ana");
    assert.equal((await hub.friends(ana)).servers.length, 0);
    const invite = await hub.createInvite(ana, undefined, undefined, "personal");
    assert.equal(invite.maxUses, 1); assert.equal(invite.kind, "personal");
    const preview = await hub.previewInvite("preview", invite.code);
    assert.equal(preview.server, undefined); assert.equal(preview.kind, "personal");
    const results = await Promise.allSettled([hub.register("reg-a", "bruno", invite.code), hub.register("reg-b", "carla", invite.code)]);
    assert.equal(results.filter(result => result.status === "fulfilled").length, 1);
    const bruno = results.find(result => result.status === "fulfilled").value.uid;
    let snapshot = await hub.friends(bruno);
    assert.equal(snapshot.friends.length, 1); assert.equal(snapshot.friends[0].id, ana); assert.equal(snapshot.servers.length, 0);
    await hub.redeemInvite(bruno, "repeat", invite.code);
    assert.equal(f.db.prepare("SELECT uses FROM invites WHERE code = ?").get(invite.code).uses, 1);
    await hub.removeFriend(bruno, ana);
    snapshot = await hub.redeemInvite(bruno, "repeat", invite.code);
    assert.equal(snapshot.friends.length, 0); assert.equal(snapshot.servers.length, 0);
    assert.equal(f.db.prepare("SELECT COUNT(*) AS n FROM users").get().n, 2);
  } finally { f.db.close(); }
});

test("OAuth mantém convite para aceite explícito; pessoal e servidor nunca misturam efeitos", async () => {
  const f = database();
  try {
    const hub = f.open(), ana = await oauth(hub, "ana");
    const personal = await hub.createInvite(ana, undefined, undefined, "personal");
    const state = await hub.oauthBegin("discord", "bruno-login", personal.code);
    await hub.oauthFinish(state, "discord", profile("bruno"));
    const bruno = await hub.exchange("bruno-login");
    assert.equal((await hub.friends(bruno)).friends.length, 0);
    await assert.rejects(hub.redeemInvite(ana, "self", personal.code), failure("convite_proprio"));
    await hub.redeemInvite(bruno, "accept", personal.code);
    assert.equal((await hub.friends(bruno)).servers.length, 0);
    const d = (await hub.changeDirectory(ana, "create", "", "Squad")).snapshot.destination;
    const server = await hub.createInvite(ana, d.serverId, d.channelId);
    const carla = await oauth(hub, "carla"); await hub.redeemInvite(carla, "accept", server.code);
    assert.equal((await hub.friends(carla)).friends.length, 0);
    await hub.changeDirectory(carla, "leave", d.serverId);
    assert.equal((await hub.redeemInvite(carla, "repeat", server.code)).servers.length, 0);
    await hub.revokeInvite(ana, personal.code);
    await assert.rejects(hub.redeemInvite(bruno, "revoked", personal.code), failure("convite_revogado"));
    const expired = await hub.createInvite(ana, undefined, undefined, "personal");
    const pending = await hub.oauthBegin("discord", "expired-login", expired.code);
    f.db.prepare("UPDATE invites SET expires=1 WHERE code=?").run(expired.code);
    await hub.oauthFinish(pending, "discord", profile("dora"));
    const dora = await hub.exchange("expired-login");
    await assert.rejects(hub.redeemInvite(dora, "expired", expired.code), failure("convite_expirado"));
    assert.equal((await hub.friends(dora)).friends.length, 0);
  } finally { f.db.close(); }
});

test("sessão privada: terceiro do mesmo servidor é recusado, snapshots filtram e remoção revoga ambos", async () => {
  const f = database();
  try {
    const hub = f.open(), ana = await oauth(hub, "ana"), bruno = await oauth(hub, "bruno"), carla = await oauth(hub, "carla");
    await hub.requestFriend(ana, "bruno"); await hub.answerRequest(bruno, ana, true);
    await hub.requestFriend(ana, "carla"); await hub.answerRequest(carla, ana, true);
    await assert.rejects(hub.startDirect(ana, bruno), failure("amigo_offline"));
    online(f, ana); online(f, bruno); online(f, carla);
    const { sessionId } = await hub.startDirect(ana, bruno);
    assert.equal((await hub.startDirect(bruno, ana)).sessionId, sessionId);
    const destination = { kind: "direct", sessionId };
    await hub.setLive(ana, { destination, since: 1, viewers: 1, title: "Privada" });
    assert.equal((await hub.friends(bruno)).friends.find(friend => friend.id === ana).live.title, "Privada");
    // Sem o destino o espectador recebe destino_alterado ao assistir por Amigos.
    assert.deepEqual((await hub.friends(bruno)).friends.find(friend => friend.id === ana).live.destination, destination);
    assert.equal((await hub.friends(carla)).friends.find(friend => friend.id === ana).live, null);
    assert.equal((await hub.friends(carla)).directSessions.length, 0);
    const d = (await hub.changeDirectory(ana, "create", "", "Comum")).snapshot.destination;
    const invite = await hub.createInvite(ana, d.serverId, d.channelId); await hub.redeemInvite(carla, "join", invite.code);
    await assert.rejects(hub.canWatch(carla, ana), failure("sessao_privada_encerrada"));
    await assert.rejects(hub.canWatch(carla, ana, destination), failure("sessao_privada_encerrada"));
    assert.equal((await hub.friends(carla)).servers[0].channels[0].live.length, 0);
    await hub.declineDirect(bruno, sessionId);
    assert.equal((await hub.friends(bruno)).directSessions[0].declined, true);
    await hub.removeFriend(ana, bruno);
    await assert.rejects(hub.resolveShare(ana, destination), failure("sessao_privada_encerrada"));
    await assert.rejects(hub.canWatch(bruno, ana, destination), failure("sessao_privada_encerrada"));
    assert.equal(f.db.prepare("SELECT cleanup FROM direct_sessions WHERE id=?").get(sessionId).cleanup, 1);
    assert.equal((await hub.friends(carla)).servers.length, 1);
  } finally { f.db.close(); }
});

test("destino ausente falha sem criar servidor e cliente antigo recebe atualização explícita", async () => {
  const f = database();
  try {
    const hub = f.open(), uid = await oauth(hub, "ana");
    await assert.rejects(hub.resolveShare(uid), failure("destino_obrigatorio"));
    const response = await api(f, uid, "POST", "/api/share/start", {});
    assert.equal(response.status, 409); assert.equal((await response.json()).error, "destino_obrigatorio");
    assert.equal(await hub.setLive(uid, { since: 1, viewers: 0, title: null }), false);
    assert.equal((await hub.friends(uid)).servers.length, 0);
    assert.equal((await api(f, uid, "POST", "/api/invites/redeem", {code:"ABCD-EFGH"})).status, 400);
  } finally { f.db.close(); }
});

test("reconexão permite 29 segundos, expira aos 30 e nunca revive geração antiga", async () => {
  const f = database(), originalNow = Date.now;
  let time = originalNow(); Date.now = () => time;
  try {
    const hub = f.open(), ana = await oauth(hub, "ana"), bruno = await oauth(hub, "bruno");
    await hub.requestFriend(ana, "bruno"); await hub.answerRequest(bruno, ana, true);
    online(f, ana); const socket = online(f, bruno);
    const {sessionId} = await hub.startDirect(ana, bruno), target = {kind:"direct",sessionId};
    socket.close(); hub.direct.presence(bruno); time += 29000;
    await hub.canWatch(bruno, ana, target);
    await assert.rejects(hub.resolveShare(ana, target, true), failure("amigo_offline"));
    online(f, bruno); hub.direct.presence(bruno); time += 2000;
    await hub.canWatch(bruno, ana, target);
    f.sockets.filter(socket => socket.tag === `u:${bruno}`).forEach(socket => socket.close());
    hub.direct.presence(bruno); time += 30000;
    await assert.rejects(hub.canWatch(bruno, ana, target), failure("sessao_privada_encerrada"));
    online(f, bruno); hub.direct.presence(bruno);
    const next = await hub.startDirect(ana, bruno); assert.notEqual(next.sessionId, sessionId);
    await assert.rejects(hub.canWatch(bruno, ana, target), failure("sessao_privada_encerrada"));
  } finally { Date.now = originalNow; f.db.close(); }
});


test("matriz dono/admin/membro, transferência explícita e revogação de convites", async () => {
  const f = database();
  try {
    const hub=f.open(), owner=await oauth(hub,"dono"), admin=await oauth(hub,"admin_user"), member=await oauth(hub,"membro"), otherAdmin=await oauth(hub,"admin_dois");
    const d=(await hub.changeDirectory(owner,"create","","Squad")).snapshot.destination;
    const invite=await hub.createInvite(owner,d.serverId,d.channelId);
    for(const uid of [admin,member,otherAdmin])await hub.redeemInvite(uid,"join",invite.code);
    await hub.changeDirectory(owner,"role",d.serverId,"admin",admin);await hub.changeDirectory(owner,"role",d.serverId,"admin",otherAdmin);
    await assert.rejects(hub.changeDirectory(admin,"rename",d.serverId,"Outro"),failure("sem_permissao"));
    await assert.rejects(hub.changeDirectory(admin,"role",d.serverId,"member",otherAdmin),failure("sem_permissao"));
    await assert.rejects(hub.changeDirectory(admin,"kick",d.serverId,undefined,otherAdmin),failure("sem_permissao"));
    await assert.rejects(hub.changeDirectory(member,"channel_create",d.serverId,"Canal"),failure("sem_permissao"));
    await assert.rejects(hub.createInvite(member,d.serverId),failure("sem_permissao"));
    await hub.changeDirectory(admin,"channel_create",d.serverId,"Canal");
    await hub.changeDirectory(admin,"kick",d.serverId,undefined,member);
    assert.equal((await hub.friends(member)).servers.length,0);
    await hub.revokeInvite(admin,invite.code);
    await assert.rejects(hub.redeemInvite(member,"revoked",invite.code),failure("convite_revogado"));
    await assert.rejects(hub.changeDirectory(owner,"leave",d.serverId),failure("transfira_propriedade"));
    await assert.rejects(hub.changeDirectory(admin,"transfer",d.serverId,undefined,otherAdmin),failure("sem_permissao"));
    await hub.changeDirectory(owner,"transfer",d.serverId,undefined,admin);
    assert.equal((await hub.friends(admin)).servers[0].role,"owner");
    await hub.changeDirectory(owner,"leave",d.serverId);
    await hub.changeDirectory(admin,"remove",d.serverId);
    assert.equal((await hub.friends(otherAdmin)).servers.length,0);
  }finally{f.db.close();}
});
