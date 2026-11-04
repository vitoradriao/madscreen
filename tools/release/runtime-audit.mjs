import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

export const POLICIES = ['preserve-private-application-sources', 'gpl-open-source'];
const COPYLEFT = ['--enable-gpl', '--enable-version3', '--enable-libx264', '--enable-libx265', '--enable-libxvid'];

/** `policy` defaults to the original private-source rule. `gpl-open-source` is for a GPL-3.0-or-later app
 * that ships the corresponding source: GPL components are allowed (copyleft obligations listed), nonfree never is. */
export function auditConfiguration(text, policy = 'preserve-private-application-sources') {
  if (!POLICIES.includes(policy)) throw Error('Política de runtime desconhecida.');
  const line = text.split(/\r?\n/).find(line => /^configuration:\s/.test(line));
  if (!line) throw Error('O executável não informou a configuração do FFmpeg.');
  const flags = [...line.matchAll(/--[a-z0-9_-]+(?:=[^\s]+)?/gi)].map(match => match[0]);
  const name = flag => flag.toLowerCase().split('=')[0];
  const blockedNames = policy === 'gpl-open-source' ? ['--enable-nonfree']
    : ['--enable-gpl', '--enable-nonfree', '--enable-libx264', '--enable-libx265', '--enable-libxvid'];
  const blocked = flags.filter(flag => blockedNames.includes(name(flag)));
  const copyleft = policy === 'gpl-open-source' ? flags.filter(flag => COPYLEFT.includes(name(flag))) : [];
  return { schema: 1, policy,
    status: blocked.length ? 'blocked-configuration' : 'requires-distribution-review', blockedFlags: blocked,
    ...(policy === 'gpl-open-source' ? { copyleftFlags: copyleft } : {}),
    configurationSha256: createHash('sha256').update(line).digest('hex'),
    requirements: policy === 'gpl-open-source' ? ['Aplicativo e runtime distribuídos sob termos compatíveis com a GPLv3 (LICENSE na raiz).',
      'Oferecer o código-fonte correspondente do runtime (versão, patches, configuração e instruções de build) junto de cada release.',
      'Preservar avisos de cada componente; nonfree continua proibido.',
      'Patentes de codecs não são resolvidas pela licença de copyright: manter a decisão de distribuição registrada.'] : ['Revisar licenças de todos os componentes e bibliotecas externas.',
      'Preservar avisos, configuração, fontes correspondentes e instruções exigidas pelo runtime.',
      'Qualificar AMF/QuickSync e o fallback CPU antes de substituir o pacote.',
      'Ausência de flags GPL/nonfree não constitui aprovação jurídica da distribuição.'] };
}

/** Encoder names required by the application's actual backend selection. */
export function auditEncoderContract(text, required) {
  if (typeof text !== 'string' || !Array.isArray(required) || !required.length || required.length > 16
    || required.some(name => typeof name !== 'string' || !/^[a-z0-9_]{1,64}$/.test(name))
    || new Set(required).size !== required.length) throw Error('Contrato de encoders inválido.');
  const available = new Set([...text.matchAll(/^\s*V[A-Z.]{5}\s+([a-z0-9_]+)\s+/gm)].map(row => row[1]));
  if (!available.size) throw Error('O executável não informou encoders de vídeo.');
  const missing = required.filter(name => !available.has(name));
  return { status: missing.length ? 'missing-required-encoders' : 'required-encoders-present',
    required: [...required], missing, present: required.filter(name => available.has(name)),
    limitation: 'Enumeração não prova inicialização, IDR, taxa, desempenho nem licença. MF não substitui automaticamente x264.' };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const [, , binary, output, ...args] = process.argv;
    let strict=false, required=null, policy='preserve-private-application-sources';
    const usage=()=>Error('Uso: node tools/release/runtime-audit.mjs ffmpeg.exe saida.json [--strict] [--policy preserve-private-application-sources|gpl-open-source] [--require-encoders h264_amf,h264_qsv,libx264]');
    if (!binary || !output) throw usage();
    for(let i=0;i<args.length;i++){
      if(args[i]==='--strict'&&!strict) strict=true;
      else if(args[i]==='--require-encoders'&&required===null&&args[i+1]) required=args[++i].split(',');
      else if(args[i]==='--policy'&&POLICIES.includes(args[i+1])) policy=args[++i];
      else throw usage();
    }
    const execute=promisify(execFile), options={ windowsHide: true, timeout: 10000, maxBuffer: 1024 * 1024 };
    const [{ stdout, stderr }, encoders] = await Promise.all([
      execute(binary, ['-version'], options), required ? execute(binary, ['-hide_banner','-encoders'], options) : null,
    ]);
    const report = { ...auditConfiguration(`${stdout}\n${stderr}`, policy),
      ...(required ? { encoderContract: auditEncoderContract(`${encoders.stdout}\n${encoders.stderr}`,required) } : {}),
      binarySha256: createHash('sha256').update(await readFile(binary)).digest('hex') };
    await writeFile(output, `${JSON.stringify(report, null, 2)}\n`, { flag: 'wx' });
    console.log(JSON.stringify({ status: report.status, blockedFlags: report.blockedFlags, ...(required?{encoderContract:report.encoderContract}: {}) }));
    if ((strict && report.status === 'blocked-configuration') || report.encoderContract?.missing.length) process.exitCode = 1;
  } catch (error) { console.error(error.code ? 'Não foi possível auditar o runtime local.' : error.message); process.exitCode = 1; }
}
