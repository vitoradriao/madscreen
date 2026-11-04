import { readFile, writeFile, mkdir, lstat } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';
const NAMES = ['docker-compose.yaml', 'livekit.yaml', 'Caddyfile'];
const sha256 = data => createHash('sha256').update(data).digest('hex');

/** Copies private configuration only into a new directory; never touches a running service. */
export async function copyLiveKitState(operation, source, destination) {
  if (!['backup', 'restore'].includes(operation)) throw Error('Operação inválida.');
  const manifest = operation === 'restore' ? JSON.parse(await readFile(join(source, 'manifest.json'), 'utf8')) : null;
  if (operation === 'restore' && (!manifest || manifest.schema !== 1 || !Array.isArray(manifest.files) || manifest.files.length !== NAMES.length
    || new Set(manifest.files.map(file => file.name)).size !== NAMES.length
    || manifest.files.some(file => !NAMES.includes(file.name)))) throw Error('Manifesto de backup inválido.');
  const files = [];
  for (const name of NAMES) {
    const path = join(source, name);
    if (!(await lstat(path)).isFile()) throw Error('Backup exige arquivos regulares.');
    const data = await readFile(path);
    if (!data.length) throw Error('Configuração vazia.');
    const digest = sha256(data), expected = manifest?.files.find(file => file.name === name);
    if (expected && (expected.sha256 !== digest || expected.bytes !== data.length)) throw Error('Integridade do backup recusada.');
    files.push({ name, bytes: data.length, sha256: digest, data });
  }
  // Validate every file before creating output; an existing installation is never overwritten.
  await mkdir(destination, { mode: 0o700 });
  for (const file of files) await writeFile(join(destination, file.name), file.data, { flag: 'wx', mode: 0o600 });
  const report = { schema: 1, files: files.map(({ data: _data, ...file }) => file) };
  await writeFile(join(destination, 'manifest.json'), JSON.stringify(report, null, 2), { flag: 'wx', mode: 0o600 });
  return { operation, files: files.length, integrityVerified: true, serviceChanged: false };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const [, , operation, source, destination, ...extra] = process.argv;
    if (!source || !destination || extra.length) throw Error('Uso: livekit-state.mjs backup|restore origem pasta-nova');
    console.log(JSON.stringify(await copyLiveKitState(operation, resolve(source), resolve(destination))));
  } catch { console.error('Backup/restauração não concluído. Confira argumentos, integridade e permissões; conteúdo privado não foi registrado.'); process.exitCode = 1; }
}
