import { readFile, readdir, lstat, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';

const REQUIRED = ['avcodec-62.dll', 'avutil-60.dll', 'swresample-6.dll', 'LICENSE', 'README.txt', 'NOTICE.md'];
const hash = bytes => createHash('sha256').update(bytes).digest('hex');

/** Inventory the files actually packaged, separately from ffmpeg.exe's build flags. */
export async function inventoryRuntime(source, bundle) {
  const files = [], errors = [];
  const entries = await readdir(bundle, { withFileTypes: true });
  for (const name of REQUIRED) if (!entries.some(entry => entry.name === name)) errors.push(`missing:${name}`);
  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name, 'en'))) {
    const path = join(bundle, entry.name);
    if (!(await lstat(path)).isFile()) { errors.push(`not-regular-file:${entry.name}`); continue; }
    const bytes = await readFile(path);
    const item = { name: entry.name, bytes: bytes.length, sha256: hash(bytes), matchesSource: null };
    if (bytes.length === 0) errors.push(`empty:${entry.name}`);
    if (/\.dll$/i.test(entry.name) || ['LICENSE', 'README.txt'].includes(entry.name)) {
      try {
        const original = await readFile(join(source, /\.dll$/i.test(entry.name) ? 'bin' : '', entry.name));
        item.matchesSource = hash(original) === item.sha256;
        if (!item.matchesSource) errors.push(`source-mismatch:${entry.name}`);
      } catch (error) {
        if (error.code !== 'ENOENT') throw error;
        errors.push(`source-missing:${entry.name}`);
      }
    }
    if (/\.dll$/i.test(entry.name) && !REQUIRED.includes(entry.name)) errors.push(`unexpected-dll:${entry.name}`);
    files.push(item);
  }
  return { schema: 1, status: errors.length ? 'blocked-inventory' : 'inventory-matches-source', files, errors,
    distributionApproved: false,
    limitations: 'Compara bytes e presença de avisos; não resolve dependências PE transitivas, licenças, fontes correspondentes, ABI funcional ou qualificação de encoders.' };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const [, , source, bundle, output, ...extra] = process.argv;
    if (!source || !bundle || !output || extra.length) throw Error('Uso: node tools/release/runtime-inventory.mjs pasta-runtime pasta-bundle saida.json');
    const result = await inventoryRuntime(resolve(source), resolve(bundle));
    await writeFile(output, `${JSON.stringify(result, null, 2)}\n`, { flag: 'wx' });
    console.log(JSON.stringify({ status: result.status, files: result.files.length, errors: result.errors }));
    if (result.errors.length) process.exitCode = 1;
  } catch (error) { console.error(error.code ? 'Não foi possível inventariar os arquivos locais.' : error.message); process.exitCode = 1; }
}
