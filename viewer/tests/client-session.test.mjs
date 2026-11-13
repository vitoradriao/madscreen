import test from "node:test";
import assert from "node:assert/strict";
import { MadClient } from "../shared/client.ts";

const store = new Map();
globalThis.localStorage = { getItem: key => store.get(key) ?? null, setItem: (key, value) => store.set(key, String(value)), removeItem: key => store.delete(key) };
const session = { token: "t", exp: 9_999_999_999, user: { id: "u", username: "u", displayName: "U" } };
const reply = (status, body) => new Response(JSON.stringify(body), { status });
const revoked = () => reply(401, { error: "sessao_revogada", message: "Esta sessão foi encerrada." });

test("sessão revogada pelo Worker: logout só esquece o token local", async () => {
  globalThis.fetch = async () => revoked();
  const client = new MadClient("https://w.example", session);
  await client.logout();
  assert.equal(client.session, null);
});

test("erro de rede no logout continua sendo erro e mantém a sessão", async () => {
  globalThis.fetch = async () => { throw new Error("offline"); };
  const client = new MadClient("https://w.example", session);
  await assert.rejects(client.logout());
  assert.ok(client.session);
});

test("troca de chave sem token (Worker antigo) preserva a sessão atual", async () => {
  globalThis.fetch = async () => reply(200, { recoveryKey: "AAAAA-BBBBB-CCCCC-DDDDD", user: session.user });
  const client = new MadClient("https://w.example", session);
  assert.equal((await client.rotateRecoveryKey()).recoveryKey, "AAAAA-BBBBB-CCCCC-DDDDD");
  assert.equal(client.session.token, "t");
});

test("fechamento 4001 do hub confirma a sessão na hora, sem esperar 3 falhas", async () => {
  const sockets = [];
  globalThis.WebSocket = class { static OPEN = 1; constructor() { sockets.push(this); } close() {} };
  globalThis.fetch = async () => revoked();
  const client = new MadClient("https://w.example", session);
  let hub;
  const lost = new Promise(resolve => { hub = client.connectHub({ onSnapshot() {}, onAuthLost: resolve }); });
  sockets[0].onclose({ code: 4001 });
  await lost;
  hub.close(); // cancela o timer de reconexão
});
