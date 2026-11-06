// #9 (público): falhas do atualizador. O plugin Tauri (tauri-plugin-updater 2.12, `Update::download`)
// lê o corpo inteiro, verifica a assinatura minisign e a versão assinada e só então devolve os bytes;
// erro de rede ou de assinatura rejeita o `download` antes de qualquer `install`. Aqui o backend é falso,
// mas o controlador é o código real do app.
import assert from "node:assert/strict";
import { createUpdateController, UPDATE_CHECK_INTERVAL_MS, UPDATE_FIRST_CHECK_DELAY_MS } from "../../apps/madscreen-app/src/updates.ts";

function timers() {
  let now = 0, next = 1;
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
  };
}

// `failures` lista o erro de cada tentativa de download (null = sucesso).
function update(version, log, failures) {
  return {
    version, currentVersion: "0.1.6",
    async close() {},
    async download(onEvent) {
      const failure = failures.shift() ?? null;
      log.push(`download ${version}`);
      onEvent({ event: "Started", data: { contentLength: 200 } });
      onEvent({ event: "Progress", data: { chunkLength: 100 } });
      if (failure) throw failure;
      onEvent({ event: "Progress", data: { chunkLength: 100 } });
      onEvent({ event: "Finished" });
    },
    async install(options) { log.push(`install ${version} restart=${options?.restartAfterInstall}`); },
  };
}

const backend = (found, log) => ({ check: async () => found, prepareInstall: async () => { log.push("prepare"); } });

// 1. Rede cortada no meio do download: erro claro, live intacta, nada instalado; nova tentativa baixa de novo.
{
  const log = [];
  const found = update("0.1.7", log, ["error decoding response body: connection closed before message completed"]);
  const ctl = createUpdateController(backend(found, log), timers());
  await ctl.check(true);
  await ctl.install();
  assert.equal(ctl.status.kind, "error");
  assert.equal(ctl.status.stage, "install");
  assert.match(ctl.status.message, /connection closed/);
  assert.deepEqual(log, ["download 0.1.7"], "sem preparar (encerrar a live) nem instalar após rede cortada");
  await ctl.install();
  assert.deepEqual(log, ["download 0.1.7", "download 0.1.7", "prepare", "install 0.1.7 restart=true"],
    "a nova tentativa refaz o download inteiro e só então encerra a live e instala");
  ctl.dispose();
}

// 2. Assinatura inválida (bytes adulterados): mesma garantia; um download recusado nunca é reaproveitado.
{
  const log = [];
  const found = update("0.1.7", log, ["the signature verification failed", "the signature verification failed"]);
  const ctl = createUpdateController(backend(found, log), timers());
  await ctl.check(true);
  await ctl.install();
  await ctl.install();
  assert.equal(ctl.status.kind, "error");
  assert.match(ctl.status.message, /signature/);
  assert.deepEqual(log, ["download 0.1.7", "download 0.1.7"], "assinatura inválida nunca chega a prepare/install");
  ctl.dispose();
}

// 3. Sem ação do usuário nada é baixado nem instalado, mesmo com versão nova por várias consultas.
{
  const t = timers();
  const log = [];
  let checks = 0;
  const found = update("0.1.7", log, []);
  const ctl = createUpdateController({ check: async () => { checks++; return found; }, prepareInstall: async () => { log.push("prepare"); } }, t);
  ctl.start();
  await t.advance(UPDATE_FIRST_CHECK_DELAY_MS + 3 * UPDATE_CHECK_INTERVAL_MS);
  assert.equal(checks, 4);
  assert.equal(ctl.status.kind, "available");
  assert.deepEqual(log, [], "consulta automática não baixa, não encerra a live e não instala");
  ctl.dispose();
}

console.log("updates-failures: 3 cenários ok");
