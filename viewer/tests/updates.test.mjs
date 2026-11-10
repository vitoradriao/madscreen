// Controlador de atualização do app: consulta periódica, "mesma versão" e instalação por ação.
import assert from "node:assert/strict";
import { createUpdateController, UPDATE_CHECK_INTERVAL_MS, UPDATE_FIRST_CHECK_DELAY_MS } from "../../apps/madscreen-app/src/updates.ts";

function fakeTimers() {
  let now = 0;
  let next = 1;
  const pending = new Map();
  return {
    setTimeout(fn, ms) { const id = next++; pending.set(id, { at: now + ms, fn }); return id; },
    clearTimeout(id) { pending.delete(id); },
    async advance(ms) {
      const target = now + ms;
      for (;;) {
        const due = [...pending.entries()].filter(([, t]) => t.at <= target).sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        pending.delete(due[0]);
        now = due[1].at;
        due[1].fn();
        for (let i = 0; i < 10; i++) await Promise.resolve();
      }
      now = target;
    },
    count: () => pending.size,
  };
}

function fakeUpdate(version, log) {
  return {
    version, currentVersion: "0.1.6",
    closed: false,
    async close() { this.closed = true; },
    async download(onEvent) {
      log.push(`download ${version}`);
      onEvent({ event: "Started", data: { contentLength: 200 } });
      onEvent({ event: "Progress", data: { chunkLength: 100 } });
      onEvent({ event: "Progress", data: { chunkLength: 100 } });
      onEvent({ event: "Finished" });
    },
    async install(options) { log.push(`install ${version} restart=${options?.restartAfterInstall}`); },
  };
}

async function flush() { for (let i = 0; i < 10; i++) await Promise.resolve(); }

// 1. Primeira consulta após o atraso inicial e depois a cada hora.
{
  const timers = fakeTimers();
  const log = [];
  let checks = 0;
  let answer = null;
  const ctl = createUpdateController({ check: async () => { checks++; return answer; }, prepareInstall: async () => { log.push("prepare"); } }, timers);
  const seen = [];
  ctl.subscribe(s => seen.push(s.kind));
  ctl.start();
  ctl.start(); // idempotente
  await timers.advance(UPDATE_FIRST_CHECK_DELAY_MS - 1);
  assert.equal(checks, 0, "não consulta antes do atraso inicial");
  await timers.advance(1);
  assert.equal(checks, 1);
  assert.equal(ctl.status.kind, "current");
  answer = fakeUpdate("0.1.7", log);
  await timers.advance(UPDATE_CHECK_INTERVAL_MS);
  assert.equal(checks, 2, "consulta de novo após uma hora");
  assert.equal(ctl.status.kind, "available");
  assert.equal(ctl.status.version, "0.1.7");
  assert.equal(log.length, 0, "consultar não baixa nem instala");
  const first = ctl.status;
  // Nova consulta acha a mesma versão: mantém o recurso, fecha o duplicado, emite aviso novo (reaparece após "Depois").
  const dup = fakeUpdate("0.1.7", log);
  answer = dup;
  await timers.advance(UPDATE_CHECK_INTERVAL_MS);
  assert.equal(checks, 3);
  assert.notEqual(ctl.status, first, "cada consulta gera um aviso novo");
  assert.equal(dup.closed, true, "recurso duplicado é liberado");
  assert.equal(timers.count(), 1, "sempre só um agendamento pendente");
  ctl.dispose();
  assert.equal(timers.count(), 0, "dispose cancela o agendamento");
}

// 2. Falha automática com aviso visível mantém o aviso; manual mostra o erro.
{
  const timers = fakeTimers();
  const log = [];
  let fail = false;
  const ctl = createUpdateController({ check: async () => { if (fail) throw new Error("sem rede"); return fakeUpdate("0.1.7", log); }, prepareInstall: async () => {} }, timers);
  await ctl.check(false);
  assert.equal(ctl.status.kind, "available");
  fail = true;
  await ctl.check(false);
  assert.equal(ctl.status.kind, "available", "falha automática não esconde a versão já encontrada");
  await ctl.check(true);
  assert.equal(ctl.status.kind, "error");
  assert.equal(ctl.status.stage, "check");
  assert.equal(ctl.status.message, "sem rede");
  ctl.dispose();
}

// 3. Instalação: baixa com progresso, prepara o sidecar e instala reiniciando; não consulta durante.
{
  const timers = fakeTimers();
  const log = [];
  let checks = 0;
  const ctl = createUpdateController({ check: async () => { checks++; return fakeUpdate("0.1.7", log); }, prepareInstall: async () => { log.push("prepare"); } }, timers);
  await ctl.check(true);
  const kinds = [];
  ctl.subscribe(s => kinds.push(s.kind === "downloading" ? `downloading ${s.percent}` : s.kind));
  await ctl.install();
  assert.deepEqual(log, ["download 0.1.7", "prepare", "install 0.1.7 restart=true"]);
  assert.ok(kinds.includes("downloading 50") && kinds.includes("downloading 100"), kinds.join(","));
  assert.equal(ctl.status.kind, "installing");
  await ctl.check(false);
  assert.equal(checks, 1, "não consulta enquanto instala");
  ctl.dispose();
}

// 4. Falha na preparação: erro de instalação, nada instalado, retry não baixa de novo.
{
  const log = [];
  let prepareFails = true;
  const ctl = createUpdateController({ check: async () => fakeUpdate("0.1.7", log), prepareInstall: async () => { if (prepareFails) throw "sidecar ocupado"; log.push("prepare"); } }, fakeTimers());
  await ctl.check(true);
  await ctl.install();
  assert.equal(ctl.status.kind, "error");
  assert.equal(ctl.status.stage, "install");
  assert.deepEqual(log, ["download 0.1.7"], "sem instalar após falha");
  prepareFails = false;
  await ctl.install();
  assert.deepEqual(log, ["download 0.1.7", "prepare", "install 0.1.7 restart=true"], "retry reaproveita o download validado");
  ctl.dispose();
}

// 5. Sem versão nova depois de uma encontrada: libera o recurso antigo.
{
  const log = [];
  const old = fakeUpdate("0.1.7", log);
  let answer = old;
  const ctl = createUpdateController({ check: async () => answer, prepareInstall: async () => {} }, fakeTimers());
  await ctl.check(false);
  answer = null;
  await ctl.check(false);
  await flush();
  assert.equal(ctl.status.kind, "current");
  assert.equal(old.closed, true);
  ctl.dispose();
}

// Recurso descartado no lado nativo ("resource id N is invalid"): consulta de novo e tenta uma vez.
{
  const log = [];
  const stale = fakeUpdate("0.1.7", log);
  stale.download = async () => { throw new Error("The resource id 398043307 is invalid."); };
  const fresh = fakeUpdate("0.1.7", log);
  const answers = [stale, fresh];
  const ctl = createUpdateController({ check: async () => answers.shift(), prepareInstall: async () => {} }, fakeTimers());
  await ctl.check(true);
  await ctl.install();
  await flush();
  assert.deepEqual(log, ["download 0.1.7", "install 0.1.7 restart=true"]);
  assert.equal(stale.closed, true);
  ctl.dispose();
}

// Consulta que termina durante o download não troca nem fecha o recurso em uso.
{
  const log = [];
  const current = fakeUpdate("0.1.7", log);
  let release;
  current.download = () => new Promise(resolve => { release = resolve; });
  let answer = current;
  let finishCheck;
  const ctl = createUpdateController({
    check: () => answer === current ? Promise.resolve(current) : new Promise(resolve => { finishCheck = () => resolve(null); }),
    prepareInstall: async () => {},
  }, fakeTimers());
  await ctl.check(true);
  answer = null;
  const pendingCheck = ctl.check(false);
  void ctl.install();
  await flush();
  finishCheck();
  await pendingCheck;
  await flush();
  assert.equal(current.closed, false);
  release();
  await flush();
  assert.equal(ctl.status.kind, "installing");
  ctl.dispose();
}

console.log("updates: ok");
