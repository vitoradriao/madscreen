import { readFile } from 'node:fs/promises';
import { createHmac, randomUUID } from 'node:crypto';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawn } from 'node:child_process';
import { parseLiveKitConfig } from './livekit-config.mjs';

export function parseSelection(args) {
  if (args.length < 1 || args.length > 2 || !['cloudflare', 'livekit'].includes(args[0])
    || (args.length === 2 && args[1] !== '--apply')) {
    throw Error('Uso: node select-static-sfu.mjs cloudflare|livekit [--apply]');
  }
  return { provider: args[0], apply: args[1] === '--apply' };
}

/** Testa somente uma sala aleatoria propria; nunca enumera ou remove salas de usuarios. */
export async function probeLiveKit(config, request = fetch) {
  const room = `ms-preflight-${randomUUID()}`;
  const encode = value => Buffer.from(JSON.stringify(value)).toString('base64url');
  const now = Math.floor(Date.now() / 1000);
  const payload = `${encode({ alg: 'HS256', typ: 'JWT' })}.${encode({
    iss: config.LIVEKIT_API_KEY, nbf: now - 5, exp: now + 60,
    video: { room, roomCreate: true, roomAdmin: true },
  })}`;
  const token = `${payload}.${createHmac('sha256', config.LIVEKIT_API_SECRET).update(payload).digest('base64url')}`;
  const origin = new URL(config.LIVEKIT_URL);
  origin.protocol = 'https:';
  async function rpc(method, body) {
    const response = await request(new URL(`/twirp/livekit.RoomService/${method}`, origin), {
      method: 'POST', redirect: 'error', signal: AbortSignal.timeout(10_000),
      headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, body: JSON.stringify(body),
    }).catch(() => { throw Error('A Magalu nao respondeu ao preflight HTTPS.'); });
    if (!response.ok) throw Error(`Room Service indisponivel: ${method} HTTP ${response.status}.`);
    return response.json();
  }
  let created = false;
  try {
    const result = await rpc('CreateRoom', { name: room, empty_timeout: 10, max_participants: 1 });
    created = true;
    if (result?.name !== room) throw Error('O Room Service nao confirmou a sala de verificacao.');
  } finally {
    if (created) await rpc('DeleteRoom', { room });
  }
}

async function main() {
  const selection = parseSelection(process.argv.slice(2));
  if (!selection.apply) {
    console.log(`Previa: SFU_PROVIDER=${selection.provider} no Worker madscreen-signal. Use --apply para alterar.`);
    return;
  }
  if (selection.provider === 'livekit') {
    const config = parseLiveKitConfig(await readFile(join(homedir(), '.magalu', 'livekit-madscreen.txt'), 'utf8'));
    await probeLiveKit(config);
  }
  const cwd = fileURLToPath(new URL('./', import.meta.url));
  const cli = fileURLToPath(new URL('node_modules/wrangler/bin/wrangler.js', import.meta.url));
  const child = spawn(process.execPath, [cli, 'secret', 'put', 'SFU_PROVIDER', '--name', 'madscreen-signal', '--env', ''],
    { cwd, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  child.stdout.resume(); child.stderr.resume(); child.stdin.on('error', () => undefined);
  child.stdin.end(`${selection.provider}\n`);
  const status = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
  if (status !== 0) throw Error('Selecao nao confirmada. Confira autenticacao/permissoes do Wrangler.');
  console.log(`SFU_PROVIDER=${selection.provider} configurado. Vale para novas transmissoes; sessoes existentes preservam seu provedor.`);
  console.log('Este comando governa somente o modo estatico. Com failover automatico habilitado, use o painel administrativo.');
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  main().catch(() => { console.error('Nao foi possivel concluir a selecao estatica. Nenhuma credencial foi registrada.'); process.exitCode = 1; });
}
