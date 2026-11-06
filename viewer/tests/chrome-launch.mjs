// Lança o Chrome de teste e devolve a porta do DevTools.
// No runner Windows, o Chrome às vezes trava na inicialização: o processo fica vivo, não escreve nada
// e nunca cria o DevToolsActivePort. Esperar mais não resolve; um processo novo, com perfil novo,
// sobe normalmente. Por isso cada tentativa tem prazo próprio: ao estourar, o processo (com a árvore
// de filhos) é encerrado e outro é lançado. O erro final traz o diagnóstico de todas as tentativas.
import { spawn, execFileSync } from "node:child_process";
import { mkdir, readFile } from "node:fs/promises";
import path from "node:path";

export const DEFAULT_CHROME = "C:/Program Files/Google/Chrome/Application/chrome.exe";
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));

function killTree(child) {
  if (child.exitCode !== null || !child.pid) return;
  try {
    if (process.platform === "win32") execFileSync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore", windowsHide: true });
    else child.kill("SIGKILL");
  } catch { /* já saiu */ }
}

async function readPort(profile) {
  try {
    const port = Number((await readFile(path.join(profile, "DevToolsActivePort"), "utf8")).split("\n")[0]);
    // O arquivo pode existir ainda vazio: só vale uma porta TCP completa.
    return Number.isInteger(port) && port > 0 && port < 65536 ? port : null;
  } catch { return null; }
}

/**
 * @param {object} options
 * @param {string} options.profileRoot pasta onde cada tentativa cria o próprio perfil
 * @param {string[]} [options.args] argumentos extras do Chrome (além de perfil e porta)
 * @param {string} [options.command] executável (padrão: CHROME_PATH ou o Chrome instalado)
 * @param {string[]} [options.commandArgs] argumentos antes dos do Chrome (usado pelo teste com binário falso)
 * @param {number} [options.attempts] padrão 3
 * @param {number} [options.attemptTimeoutMs] padrão 20 s por tentativa
 * @param {boolean} [options.captureStderr]
 */
export async function launchChrome({ profileRoot, args = [], command = process.env.CHROME_PATH ?? DEFAULT_CHROME, commandArgs = [], attempts = 3, attemptTimeoutMs = 20000, captureStderr = true }) {
  const failures = [];
  for (let attempt = 1; attempt <= attempts; attempt++) {
    const profile = path.join(profileRoot, `chrome-${Date.now()}-${attempt}`);
    await mkdir(profile, { recursive: true });
    const child = spawn(command, [...commandArgs, "--remote-debugging-port=0", `--user-data-dir=${profile}`, ...args, "about:blank"],
      { stdio: ["ignore", "ignore", captureStderr ? "pipe" : "ignore"], windowsHide: true });
    let log = "", spawnError = null;
    child.stderr?.on("data", chunk => { if (log.length < 4000) log += chunk; });
    child.on("error", error => { spawnError = error; });
    const started = Date.now();
    let port = null;
    while (Date.now() - started < attemptTimeoutMs && child.exitCode === null && !spawnError) {
      port = await readPort(profile);
      if (port) break;
      await wait(100);
    }
    if (port) {
      if (attempt > 1) console.warn(`Chrome de teste subiu na tentativa ${attempt}; anteriores: ${failures.map(f => f.reason).join(" | ")}`);
      return { child, port, profile, attempt, close: () => killTree(child) };
    }
    const reason = spawnError ? `falha ao executar: ${spawnError.message}`
      : child.exitCode !== null ? `saiu com código ${child.exitCode}`
      : `travado sem DevToolsActivePort após ${attemptTimeoutMs} ms`;
    failures.push({ attempt, reason, log: log.slice(0, 1500) });
    killTree(child);
    if (spawnError) break; // executável ausente: repetir não ajuda
  }
  throw Error(`Chrome de teste não iniciou em ${failures.length} tentativa(s); configure CHROME_PATH se o caminho estiver errado.\n`
    + failures.map(f => `#${f.attempt}: ${f.reason}${f.log ? `\n${f.log}` : ""}`).join("\n"));
}
