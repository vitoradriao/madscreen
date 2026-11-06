// Prova o lançador sem Chrome real: um "chrome" falso em Node trava, sai ou sobe conforme o cenário.
import assert from "node:assert/strict";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { launchChrome } from "./chrome-launch.mjs";

const root = await mkdtemp(path.join(tmpdir(), "chrome-launch-"));
const fake = path.join(root, "fake-chrome.mjs");
await writeFile(fake, `
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
const [counter, plan] = process.argv.slice(2, 4);
const profile = process.argv.find(a => a.startsWith("--user-data-dir=")).slice(16);
appendFileSync(counter, "x");
const n = readFileSync(counter, "utf8").length;
const action = plan.split(",")[n - 1] ?? "hang";
if (action === "exit") process.exit(7);
if (action === "empty") writeFileSync(path.join(profile, "DevToolsActivePort"), "");
if (action === "up") writeFileSync(path.join(profile, "DevToolsActivePort"), "45678\\n/devtools/browser/x");
setInterval(() => {}, 1000);
`);

async function scenario(name, plan, options = {}) {
  const counter = path.join(root, `${name}.count`);
  await writeFile(counter, "");
  return launchChrome({ profileRoot: path.join(root, name), command: process.execPath, commandArgs: [fake, counter, plan], attemptTimeoutMs: 1500, ...options });
}

// 1. Trava na primeira tentativa (o caso do runner): encerra e sobe na segunda.
{
  const chrome = await scenario("hang-then-up", "hang,up");
  assert.equal(chrome.port, 45678);
  assert.equal(chrome.attempt, 2);
  chrome.close();
}

// 2. Arquivo ainda vazio não vale como porta; depois sai com código: a terceira tentativa sobe.
{
  const chrome = await scenario("empty-exit-up", "empty,exit,up");
  assert.equal(chrome.attempt, 3);
  chrome.close();
}

// 3. Todas travam: erro claro com o motivo de cada tentativa.
await assert.rejects(scenario("all-hang", "hang,hang,hang"),
  error => /não iniciou em 3 tentativa/.test(error.message) && (error.message.match(/travado sem DevToolsActivePort/g) ?? []).length === 3);

// 4. Executável inexistente: falha logo, sem repetir.
await assert.rejects(launchChrome({ profileRoot: path.join(root, "missing"), command: path.join(root, "nao-existe.exe"), attemptTimeoutMs: 1500 }),
  error => /em 1 tentativa/.test(error.message) && /falha ao executar/.test(error.message));

await rm(root, { recursive: true, force: true, maxRetries: 5 }).catch(() => {});
console.log("chrome-launch: 4 cenários ok");
