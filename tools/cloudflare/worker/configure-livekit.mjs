import { readFile, writeFile } from "node:fs/promises";
import { randomBytes } from "node:crypto";
import { homedir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { parseLiveKitConfig } from "./livekit-config.mjs";

// Credenciais ficam somente no arquivo privado de origem, dev.vars ignorado ou secrets do Worker.
const args = process.argv.slice(2);
const publish = args.includes("--publish-secrets");
const source = args.find(arg => !arg.startsWith("--")) ?? join(homedir(), ".magalu", "livekit-madscreen.txt");
if (args.some(arg => arg.startsWith("--") && arg !== "--publish-secrets")) throw new Error("Opção inválida.");
const raw = await readFile(source, "utf8");
const secrets = parseLiveKitConfig(raw);
const directory = fileURLToPath(new URL("./", import.meta.url));
if (publish) {
  // A configuração reserva nunca muda a seleção principal para LiveKit.
  const input = JSON.stringify({ ...secrets, SFU_PROVIDER: "cloudflare" });
  const cli = fileURLToPath(new URL("node_modules/wrangler/bin/wrangler.js", import.meta.url));
  const child = spawn(process.execPath, [cli, "secret", "bulk"], { cwd: directory, stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
  // Suprimir a saída do CLI também em erro: nunca refletir um corpo que possa conter secret.
  child.stdout.resume(); child.stderr.resume(); child.stdin.on("error", () => undefined);
  child.stdin.end(input);
  const status = await new Promise((resolve, reject) => { child.once("error", reject); child.once("exit", resolve); });
  if (status !== 0) throw new Error("Não foi possível gravar os secrets. Verifique o login do Wrangler e as permissões da conta.");
  process.stdout.write("Secrets LiveKit configurados; SFU_PROVIDER=cloudflare no Worker principal.\n");
} else {
  const destination = new URL(".dev.vars.livekit", import.meta.url);
  let session = randomBytes(32).toString("base64url");
  try {
    const previous = await readFile(destination, "utf8");
    const old = /^SESSION_SECRET=(.+)$/m.exec(previous)?.[1];
    if (old) session = old.startsWith('"') ? JSON.parse(old) : old;
  } catch (error) { if (error.code !== "ENOENT") throw error; }
  await writeFile(destination, Object.entries({ ...secrets, SFU_PROVIDER: "livekit", SESSION_SECRET: session }).map(([key,value])=>`${key}=${JSON.stringify(value)}`).join("\n")+"\n", { mode: 0o600 });
  process.stdout.write("Ambiente local LiveKit preparado em .dev.vars.livekit (ignorado pelo Git). Produção não alterada.\n");
}
