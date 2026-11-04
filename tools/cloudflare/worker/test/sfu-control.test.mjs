import assert from "node:assert/strict";
import { test } from "node:test";
import { DatabaseSync } from "node:sqlite";
import { build } from "esbuild";

const bundle = await build({ stdin: { contents: 'export { SfuControl, usagePeriod } from "./src/sfu-control.ts"; export { HubDO, default as worker } from "./src/index.ts"; export { signSession } from "./src/auth.ts";', resolveDir: process.cwd() }, bundle: true, write: false, format: "esm", platform: "node", plugins: [{ name: "do-runtime", setup(builder) {
  builder.onResolve({ filter: /^cloudflare:workers$/ }, () => ({ path: "runtime", namespace: "test" }));
  builder.onLoad({ filter: /.*/, namespace: "test" }, () => ({ contents: "export class DurableObject { constructor(ctx,env) { this.ctx=ctx;this.env=env; } }" }));
} }] });
const { SfuControl, usagePeriod, HubDO, worker, signSession } = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`);
globalThis.WebSocketRequestResponsePair = class {};

async function fixture(run, options = {}) {
  const originalNow = Date.now, originalFetch = globalThis.fetch;
  let now = Date.UTC(2026, 9, 31, 12);
  Date.now = () => now;
  globalThis.fetch = async () => { throw Error("Unexpected network access"); };
  const db = new DatabaseSync(":memory:");
  const env = { SESSION_SECRET: "local-test-session", SFU_FAILOVER_ENABLED: "1", CF_REALTIME_APP_ID: "app", CF_REALTIME_APP_SECRET: "cf-test-secret", LIVEKIT_URL: "wss://livekit.example", LIVEKIT_API_KEY: "test-key", LIVEKIT_API_SECRET: "lk-test-secret", ...options };
  const sql = { exec(query, ...args) {
    let rows = [];
    if (!args.length && query.trim().split(";").filter(Boolean).length > 1) db.exec(query);
    else rows = db.prepare(query).all(...args);
    return { toArray: () => rows, [Symbol.iterator]: () => rows[Symbol.iterator]() };
  } };
  const failing = new Set(), probes = [];
  const probe = async id => { probes.push(id); if (failing.has(id)) throw Error("private-upstream-secret"); };
  const control = new SfuControl(sql, env, probe);
  try { await run({ db, env, sql, control, failing, probes, probe, advance: ms => { now += ms; }, setTime: at => { now = at; } }); }
  finally { db.close(); Date.now = originalNow; globalThis.fetch = originalFetch; }
}

test("duas falhas trocam para LiveKit; três sucessos e cooldown retornam à Cloudflare", async () => {
  await fixture(async ({ control, failing, advance }) => {
    assert.equal(await control.select(), "cloudflare");
    failing.add("cloudflare"); advance(30000); await control.check();
    assert.equal(control.status().health.cloudflare.status, "degraded");
    advance(30000); await control.check();
    assert.equal(await control.select(), "livekit");
    assert.equal(control.status().transitions.at(-1).reason, "cloudflare-down");
    failing.clear();
    for (let i=0;i<2;i++) { advance(30000); await control.check(); assert.equal(await control.select(), "livekit"); }
    advance(30000); await control.check();
    assert.equal(await control.select(), "cloudflare");
    assert.equal(control.status().transitions.at(-1).reason, "cloudflare-recovered");
    assert(!JSON.stringify(control.status()).includes("private-upstream-secret"));
  });
});

test("uso estimado cruza 90%, persiste em SQLite e força reserva", async () => {
  await fixture(async ({ control, sql, env, probe, advance }) => {
    await control.check();
    control.noteUsage("owner", 1, "cloudflare", 2);
    advance(500);
    assert.equal(await control.select(), "livekit");
    assert.equal(control.status().usage.bytes.cloudflare, 1000);
    assert.equal(control.status().transitions.at(-1).reason, "usage-threshold");
    control.noteUsage("owner", 2, null, 0);
    const reopened = new SfuControl(sql, env, probe);
    assert.equal(await reopened.select(), "livekit");
    assert.equal(reopened.status().usage.bytes.cloudflare, 1000);
    advance(60000);
    assert.equal(reopened.status().usage.bytes.cloudflare, 1000);
  }, { SFU_MONTHLY_QUOTA_BYTES: "1000", SFU_USAGE_BITRATE_BPS: "8000", SFU_USAGE_CALIBRATED: "1" });
});

test("contabilidade divide no ciclo UTC, aceita dia 28 e limita leases órfãos a 40 s", async () => {
  await fixture(async ({ control, advance, setTime, sql, env, probe }) => {
    setTime(Date.UTC(2026,9,31,23,59,59,500));
    control.noteUsage("owner", 1, "cloudflare", 1);
    advance(1000); control.status();
    const saved = JSON.parse(sql.exec("SELECT data FROM sfu_control WHERE id=1").toArray()[0].data);
    assert.equal(saved.usage["2026-10-01"].cloudflare, 500);
    assert.equal(saved.usage["2026-11-01"].cloudflare, 500);
    advance(120000);
    const reopened = new SfuControl(sql, env, probe);
    assert.equal(reopened.status().usage.bytes.cloudflare, 39500);
    assert.equal(usagePeriod(Date.UTC(2026,1,27),28).key, "2026-01-28");
    assert.equal(usagePeriod(Date.UTC(2026,1,28),28).key, "2026-02-28");
  }, { SFU_USAGE_BITRATE_BPS: "8000" });
});

test("virada do mês libera a cota e permite retorno controlado", async () => {
  await fixture(async ({ control, advance, setTime }) => {
    setTime(Date.UTC(2026,9,31,23,59,59)); await control.check();
    control.noteUsage("owner",1,"cloudflare",1); advance(900);
    assert.equal(await control.select(),"livekit");
    control.noteUsage("owner",2,null,0);
    advance(120000); await control.check(); await control.check();
    assert.equal(await control.select(),"cloudflare");
    assert.equal(control.status().usage.bytes.cloudflare,0);
  }, { SFU_MONTHLY_QUOTA_BYTES:"1000",SFU_USAGE_BITRATE_BPS:"8000",SFU_USAGE_CALIBRATED:"1" });
});

test("geração encerrada rejeita uso atrasado e mudo/pausa de vídeo não zeram estimativa", async () => {
  await fixture(async ({ control, advance }) => {
    control.noteUsage("owner",3,"cloudflare",1); advance(1000);
    control.noteUsage("owner",4,null,0); advance(1000);
    control.noteUsage("owner",3,"cloudflare",8); advance(1000);
    assert.equal(control.status().usage.bytes.cloudflare,1000);
  }, { SFU_USAGE_BITRATE_BPS:"8000" });
});

test("falha de publicação acelera failover e falha da reserva permite retorno de emergência", async () => {
  await fixture(async ({ control }) => {
    await control.check(); control.outcome("cloudflare",false,15,502,true);
    assert.equal(await control.select(),"livekit");
    for(let i=0;i<3;i++)control.outcome("cloudflare",true,10);
    control.outcome("livekit",false,10,503,true);
    assert.equal(await control.select(),"cloudflare");
  });
});

test("cota atingida com reserva indisponível recusa novas alocações", async () => {
  await fixture(async ({ control, failing, advance }) => {
    failing.add("livekit"); await control.check();
    control.noteUsage("owner",1,"cloudflare",1); advance(1000);
    await assert.rejects(control.select(),error=>error.code==="sfu_indisponivel");
    control.setMode("cloudflare");
    assert.equal(await control.select(),"cloudflare");
  }, { SFU_MONTHLY_QUOTA_BYTES:"1000",SFU_USAGE_BITRATE_BPS:"8000",SFU_USAGE_CALIBRATED:"1" });
});

test("checagens simultâneas são coalescidas e configuração inválida não aparece em erros", async () => {
  await fixture(async ({ control, probes, env, sql }) => {
    await Promise.all([control.check(),control.check(),control.select()]);
    assert.equal(probes.length,2);
    for(const options of [{SFU_CYCLE_DAY:"29"},{SFU_CYCLE_DAY:"1.5"},{SFU_MONTHLY_QUOTA_BYTES:"secret-value"},{SFU_FAILOVER_THRESHOLD:"2"}]) {
      assert.throws(()=>new SfuControl(sql,{...env,...options}),error=>error.code==="sfu_configuracao"&&!error.message.includes("secret-value"));
    }
  });
});

test("sucesso de controle não prova mídia; sonda antiga fica desconhecida", async () => {
  await fixture(async ({ control, advance }) => {
    assert.equal(control.status().health.cloudflare.status, "unknown");
    await control.check();
    const health = control.status().health.cloudflare;
    assert.equal(health.status, "healthy");
    assert.equal(health.scope, "control");
    assert.equal(health.mediaStatus, "unknown");
    advance(90001);
    assert.equal(control.status().health.cloudflare.status, "unknown");
    assert.equal(control.status().health.cloudflare.latencyMs, null);
    assert.equal(control.status().health.cloudflare.errorRate, null);
    control.outcome("cloudflare", true, 1, 0, false, Date.now() + 10000);
    assert.equal(control.status().health.cloudflare.status, "unknown");
  });
});

test("estimativa sem calibração não troca provedor nem bloqueia novas alocações", async () => {
  await fixture(async ({ control, failing, advance }) => {
    failing.add("livekit"); await control.check();
    control.noteUsage("owner", 1, "cloudflare", 2); advance(1000);
    assert.equal(await control.select(), "cloudflare");
    assert.equal(control.status().usage.bytes.cloudflare, 2000);
    assert.equal(control.status().usage.quotaEnforced, false);
    assert.equal(control.status().transitions.length, 0);
  }, { SFU_MONTHLY_QUOTA_BYTES: "1000", SFU_USAGE_BITRATE_BPS: "8000" });
});

test("admin exige conta autorizada; modo manual e alertas persistem sem expor leases/secrets", async () => {
  await fixture(async ({ db, sql, env, control }) => {
    const tasks=[];let alarm=null;
    const storage={sql, getAlarm:async()=>alarm,setAlarm:async at=>{alarm=at;},transactionSync:fn=>fn()};
    const ctx={storage,setWebSocketAutoResponse(){},getWebSockets:()=>[],blockConcurrencyWhile:fn=>{const pending=fn();tasks.push(pending);return pending;}};
    const hub=new HubDO(ctx,env);hub.sfuControl=control;
    env.HUB={idFromName:value=>value,get:()=>hub};
    const registered=await hub.register("local","adminuser",null);
    const uid=registered.uid;
    await assert.rejects(hub.sfuAdmin(uid),error=>error.code==="sem_permissao");
    await assert.rejects(hub.sfuAdmin("nonexistent"),error=>error.code==="sessao_invalida");
    env.SFU_ADMIN_USER_IDS=uid;
    const {token}=await signSession(uid,env.SESSION_SECRET);
    const request=(method,body,auth=token)=>worker.fetch(new Request("https://fixture.example/api/admin/sfu",{method,headers:{Authorization:`Bearer ${auth}`,"Content-Type":"application/json"},...(body?{body:JSON.stringify(body)}:{})}),env);
    assert.equal((await request("GET",null,"invalid")).status,401);
    const forced=await request("POST",{mode:"livekit"});
    assert.equal(forced.status,200);assert.equal((await forced.json()).provider,"livekit");
    const status=await hub.sfuAdmin(uid);
    assert.equal(status.transitions.at(-1).reason,"manual");
    assert.equal(status.mode,"livekit");
    const text=JSON.stringify(status);
    for(const sensitive of ["cf-test-secret","lk-test-secret","leases",uid])assert(!text.includes(sensitive));
    const persisted=JSON.parse(db.prepare("SELECT data FROM sfu_control WHERE id=1").get().data);
    assert.equal(persisted.mode,"livekit");
    await hub.alarm();assert(alarm>Date.now());
    await Promise.all(tasks);
    env.SFU_FAILOVER_ENABLED="0";
    assert.equal(await hub.selectSfu(),"cloudflare");
    assert.equal((await request("GET")).status,503);
    env.SFU_PROVIDER="livekit";
    control.setMode("cloudflare");
    assert.equal(await hub.selectSfu(),"livekit");
    assert.equal(control.status().mode,"cloudflare");
    env.SFU_FAILOVER_ENABLED="1";
    assert.equal(await hub.selectSfu(),"cloudflare");
    assert.equal(control.status().mode,"cloudflare");
  });
});
