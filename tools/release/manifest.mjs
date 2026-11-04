import { createHash } from "node:crypto";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function createManifest({ version, signature, filename, notes = "", publishedAt = new Date().toISOString() }) {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/.test(version)) throw Error("Versão de release inválida.");
  if (!/^[A-Za-z0-9_-][A-Za-z0-9._-]*\.exe$/.test(filename)) throw Error("Nome de instalador inválido.");
  if (typeof signature !== "string" || !/^[A-Za-z0-9+/]+={0,2}$/.test(signature.trim())) throw Error("Assinatura ausente ou inválida.");
  const lines = Buffer.from(signature.trim(), "base64").toString("utf8").trim().split(/\r?\n/);
  const versions = lines[2]?.replace(/^trusted comment: /, "").split("\t").filter(part => part.startsWith("version:")) ?? [];
  if (lines.length !== 4 || !lines[0].startsWith("untrusted comment:") || !lines[2].startsWith("trusted comment:") || versions.length !== 1 || versions[0] !== `version:${version}`) throw Error("A assinatura não está vinculada à versão anunciada.");
  if (Buffer.from(lines[1], "base64").length !== 74 || Buffer.from(lines[3], "base64").length !== 64) throw Error("Formato de assinatura inválido.");
  if (!Number.isFinite(Date.parse(publishedAt))) throw Error("Data de publicação inválida.");
  return { version, notes, pub_date: new Date(publishedAt).toISOString(), platforms: { "windows-x86_64": {
    signature: signature.trim(), url: `https://github.com/vitoradriao/madscreen-releases/releases/download/v${version}/${filename}`,
  } } };
}

async function main() {
  const options = {};
  for (let i=2;i<process.argv.length;i+=2) {
    const flag = process.argv[i];
    if (!["--bundle-dir","--out-dir","--notes-file"].includes(flag) || !process.argv[i+1]) throw Error("Opções de manifesto inválidas.");
    options[flag] = process.argv[i+1];
  }
  const config = JSON.parse(await readFile(new URL("../../apps/madscreen-app/src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  const bundle = resolve(options["--bundle-dir"] ?? "apps/madscreen-app/src-tauri/target/release/bundle/nsis");
  const output = resolve(options["--out-dir"] ?? "target/release-manifest");
  const filename = `madscreen_${config.version}_x64-setup.exe`;
  const artifact = await readFile(resolve(bundle, filename));
  const signature = await readFile(resolve(bundle, `${filename}.sig`), "utf8");
  const notes = options["--notes-file"] ? await readFile(options["--notes-file"], "utf8") : "Atualização do madscreen.";
  const manifest = createManifest({ version: config.version, filename, signature, notes });
  await mkdir(output, { recursive: true });
  await writeFile(resolve(output,"latest.json"), JSON.stringify(manifest,null,2)+"\n");
  await writeFile(resolve(output,"SHA256SUMS.txt"), `${createHash("sha256").update(artifact).digest("hex")}  ${filename}\n`);
  console.log(`Manifesto ${config.version} gerado. A assinatura criptográfica deve ser verificada antes de publicar.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(() => { console.error("Não foi possível gerar o manifesto da atualização; confira versão, instalador e assinatura."); process.exitCode=1; });
}
