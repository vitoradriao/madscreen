import { readFile, realpath, writeFile } from "node:fs/promises";
import { basename, dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

// Gera o código JavaScript que o `execute` do MCP da Cloudflare roda para publicar o Worker.
// Este script NÃO publica nada e nunca lê segredos existentes: o deploy usa keep_bindings
// ["secret_text", "plain_text"], preservando credenciais, provedor e URL pública do instalador.
//
//   node deploy-mcp.mjs OUT.mjs [--app-id ID] [--migration-from none|v1|v2]
//        [--generate-secret NOME]... [--create-turn-key]
//
// --generate-secret NOME  cria NOME como secret_text aleatório DENTRO da execução (nunca impresso).
// --create-turn-key       cria uma chave TURN da Cloudflare e a liga como TURN_KEY_ID/TURN_KEY_API_TOKEN.
// --migration-to TAG      para de aplicar migrações nessa tag; --drop-binding NOME remove o binding (deleção de classe).
// --migration-from TAG    inclui as migrações de Durable Objects posteriores a TAG (none = primeira vez).
const root = new URL("./", import.meta.url);
const repository = await realpath(fileURLToPath(new URL("../../../", root)));

const MIGRATIONS = [
  { tag: "v1", new_sqlite_classes: ["RoomDO"] },
  { tag: "v2", new_sqlite_classes: ["HubDO", "StreamDO"], deleted_classes: ["RoomDO"] },
  // Reinício das salas (instâncias presas em código antigo durante o desenvolvimento).
  { tag: "v3", deleted_classes: ["StreamDO"] },
  { tag: "v4", new_sqlite_classes: ["StreamDO"] },
  { tag: "v5", deleted_classes: ["StreamDO"] },
  { tag: "v6", new_sqlite_classes: ["StreamDO"] },
  // Limpeza dos dados de teste antes da entrega (zera contas e reabre o cadastro do primeiro usuário).
  { tag: "v7", deleted_classes: ["HubDO", "StreamDO"] },
  { tag: "v8", new_sqlite_classes: ["HubDO", "StreamDO"] },
];

function inRepository(path) {
  const part = relative(repository, path);
  return part === "" || (part !== ".." && !part.startsWith(`..${sep}`) && !isAbsolute(part));
}

async function main() {
  const args = process.argv.slice(2);
  let output;
  let appId = "f47500b8647898765190dd54d150ba25";
  let from = null;
  let turn = false;
  let to = null;
  const dropped = [];
  const generated = [];
  for (let index = 0; index < args.length; index++) {
    const arg = args[index];
    if (arg === "--create-turn-key") {
      turn = true;
    } else if (arg === "--app-id" || arg === "--generate-secret" || arg === "--migration-from" || arg === "--migration-to" || arg === "--drop-binding") {
      const value = args[++index];
      if (!value || value.startsWith("--")) throw new Error("Falta um valor para a opção de deploy.");
      if (arg === "--app-id") appId = value;
      if (arg === "--generate-secret") generated.push(value);
      if (arg === "--migration-from") from = value;
      if (arg === "--migration-to") to = value;
      if (arg === "--drop-binding") dropped.push(value);
    } else if (arg.startsWith("--")) {
      throw new Error("Opção de deploy inválida.");
    } else {
      if (output) throw new Error("Informe somente um arquivo de saída.");
      output = arg;
    }
  }
  if (!/^[0-9a-f]{32}$/i.test(appId)) throw new Error("App ID inválido.");
  for (const name of generated) if (!/^[A-Z][A-Z0-9_]*$/.test(name)) throw new Error("Nome de secret inválido.");

  let outputPath;
  if (output && output !== "-") {
    const path = resolve(output);
    outputPath = resolve(await realpath(dirname(path)), basename(path));
  }

  const bindings = [
    { type: "plain_text", name: "CF_REALTIME_APP_ID", text: appId },
    { type: "durable_object_namespace", name: "HUB", class_name: "HubDO" },
    { type: "durable_object_namespace", name: "STREAM", class_name: "StreamDO" },
  ];
  const kept = bindings.filter((binding) => !dropped.includes(binding.name));
  const metadata = { main_module: "worker.js", compatibility_date: "2026-10-01", keep_bindings: ["secret_text", "plain_text"], bindings: kept };
  if (from !== null) {
    const start = from === "none" ? 0 : MIGRATIONS.findIndex((item) => item.tag === from) + 1;
    if (start === 0 && from !== "none") throw new Error("Tag de migração desconhecida.");
    const end = to === null ? MIGRATIONS.length : MIGRATIONS.findIndex((item) => item.tag === to) + 1;
    const steps = MIGRATIONS.slice(start, end);
    if (steps.length) {
      metadata.migrations = { ...(from === "none" ? {} : { old_tag: from }), new_tag: steps[steps.length - 1].tag,
        steps: steps.map(({ tag: _tag, ...step }) => step) };
    }
  }
  const bundle = await readFile(new URL("dist/worker.js", root), "utf8");
  const code = `async () => {
  const bundle = ${JSON.stringify(bundle)};
  const metadata = ${JSON.stringify(metadata)};
  const generated = ${JSON.stringify(generated)};
  const wantTurn = ${JSON.stringify(turn)};
  // A mensagem do MCP traz só método, caminho, status e erros da API (sem corpo da requisição).
  const fail = (e) => { throw new Error("Falha ao publicar madscreen-signal: " + String(e && e.message).slice(0, 400)); };
  // Segredos criados aqui dentro nunca são devolvidos nem registrados.
  for (const name of generated) {
    const bytes = crypto.getRandomValues(new Uint8Array(32));
    metadata.bindings.push({ type: "secret_text", name, text: btoa(String.fromCharCode(...bytes)).replace(/[+/=]/g, "") });
  }
  let turn = "ignorado";
  if (wantTurn) {
    try {
      const key = await cloudflare.request({ method: "POST", path: \`/accounts/\${accountId}/calls/turn_keys\`, body: { name: "madscreen-turn" } });
      const id = key.result && (key.result.uid || key.result.id);
      const token = key.result && key.result.secret;
      if (!id || !token) throw new Error("x");
      metadata.bindings.push({ type: "secret_text", name: "TURN_KEY_ID", text: id });
      metadata.bindings.push({ type: "secret_text", name: "TURN_KEY_API_TOKEN", text: token });
      turn = "criada";
    } catch { turn = "indisponivel"; }
  }
  const form = new FormData();
  form.append("metadata", new Blob([JSON.stringify(metadata)], { type: "application/json" }));
  form.append("worker.js", new Blob([bundle], { type: "application/javascript+module" }), "worker.js");
  try {
    const result = await cloudflare.request({
      method: "PUT",
      path: \`/accounts/\${accountId}/workers/scripts/madscreen-signal\`,
      body: form,
    });
    return { success: result.success === true, status: result.status, script: "madscreen-signal", turn, secretsCriados: generated };
  } catch (e) { fail(e); }
}
`;
  if (outputPath) {
    // Criação exclusiva: não sobrescreve arquivo nem segue symlink. O código não contém segredos.
    await writeFile(outputPath, code, { mode: 0o600, flag: "wx" });
    process.stderr.write(`Arquivo de deploy gerado${inRepository(outputPath) ? " (dentro do repositório: não commite)" : ""}.\n`);
  } else {
    process.stdout.write(code);
  }
}

try {
  await main();
} catch (error) {
  const message = error instanceof Error && !Object.hasOwn(error, "code")
    ? error.message : "Não foi possível ler ou escrever os arquivos do deploy.";
  process.stderr.write(`${message}\n`);
  process.exitCode = 1;
}
