// Gera THIRD-PARTY-NOTICES.md a partir de Cargo.lock (cargo metadata, Windows) e dos package-lock.json.
// Determinístico e offline. Lista nome/versão/licença declarada; não substitui os textos de licença
// que acompanham cada componente distribuído nem aprova patentes de codec.
// Uso: node tools/release/third-party-notices.mjs [saida.md]   (padrão: THIRD-PARTY-NOTICES.md)
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';

const root = resolve(new URL('../../', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'));

export function rustPackages(directory) {
  const text = execFileSync('cargo', ['metadata', '--offline', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc'],
    { cwd: directory, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, windowsHide: true });
  const rows = new Map();
  for (const p of JSON.parse(text).packages) {
    if (p.source === null) continue; // crates do próprio projeto: GPL-3.0-or-later (LICENSE)
    rows.set(`${p.name}@${p.version}`, { name: p.name, version: p.version, license: p.license ?? 'ver arquivo de licença do crate', repository: p.repository ?? '' });
  }
  return rows;
}

export function npmPackages(lockPath) {
  const lock = JSON.parse(readFileSync(lockPath, 'utf8'));
  const rows = new Map();
  for (const [path, p] of Object.entries(lock.packages ?? {})) {
    if (!path) continue;
    const name = p.name ?? path.replace(/^.*node_modules\//, '');
    rows.set(`${name}@${p.version}`, { name, version: p.version, license: p.license ?? 'desconhecida', dev: Boolean(p.dev) });
  }
  return rows;
}

export function render({ rust, npm }) {
  const lines = ['# Avisos de terceiros', '',
    'O MadScreen é distribuído sob a **GNU GPL v3 ou posterior** (arquivo `LICENSE`). Este arquivo lista as licenças *declaradas*',
    'pelas dependências; cada componente mantém sua própria licença e seus avisos. Gerado por',
    '`node tools/release/third-party-notices.mjs`; não aprova patentes de codec nem substitui a revisão dos textos de licença.', '',
    '## Componentes binários distribuídos separadamente', '',
    '| Componente | Licença | Observação |', '| --- | --- | --- |',
    '| FFmpeg (libavcodec/libavutil/libswresample) | LGPL-2.1+ ou GPL (conforme a build) | A build fornecida inclui x264 (GPL); código-fonte correspondente e configuração acompanham cada release (`docs/runtime-distribution.md`). |',
    '| x264 | GPL-2.0+ | Encoder CPU de fallback. Patentes AVC não são cobertas pela licença de copyright. |',
    '| AMD AMF, Intel oneVPL | MIT | Dispatcher/headers; o encoder em si é do driver do fabricante. |',
    '| NVIDIA Video Codec SDK (headers NVENC) | MIT | O encoder é do driver NVIDIA. |',
    '| libwebrtc (via LiveKit/webrtc-sys) | BSD-3-Clause | Pré-compilado do projeto LiveKit. |',
    '| LiveKit Rust SDK (`third_party/livekit`, `third_party/libwebrtc`, `third_party/webrtc-sys`) | Apache-2.0 | Cópia com modificações; ver os avisos nas pastas. |',
    '| Cisco OpenH264 (opcional) | BSD-2-Clause | Somente o binário oficial da Cisco cobre royalties AVC; uma DLL compilada localmente não. |', ''];
  const table = (title, rows, extra) => {
    lines.push(`## ${title}`, '', `| Pacote | Versão | Licença${extra ? ' | Dev' : ''} |`, `| --- | --- | ---${extra ? ' | ---' : ''} |`);
    for (const row of [...rows.values()].sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))) {
      lines.push(`| ${row.name} | ${row.version} | ${row.license.replaceAll('|', '/')}${extra ? ` | ${row.dev ? 'sim' : ''}` : ''} |`);
    }
    lines.push('');
  };
  table('Crates Rust (Windows)', rust, false);
  for (const [name, rows] of npm) table(`npm: ${name}`, rows, true);
  return `${lines.join('\n')}\n`;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const rust = new Map([...rustPackages(root), ...rustPackages(join(root, 'apps/madscreen-app/src-tauri'))]);
  const npm = new Map(['viewer', 'apps/madscreen-app', 'tools/cloudflare/worker'].map(dir => [dir, npmPackages(join(root, dir, 'package-lock.json'))]));
  const output = resolve(process.argv[2] ?? join(root, 'THIRD-PARTY-NOTICES.md'));
  writeFileSync(output, render({ rust, npm }));
  console.log(JSON.stringify({ output, rust: rust.size, npm: Object.fromEntries([...npm].map(([k, v]) => [k, v.size])) }));
}
