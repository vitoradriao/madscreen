import assert from 'node:assert/strict';
import { test } from 'node:test';
import { auditConfiguration, auditEncoderContract, POLICIES } from './runtime-audit.mjs';

test('runtime full GPL é bloqueado para a política de código privado', () => {
  const result = auditConfiguration('ffmpeg version test\nconfiguration: --enable-shared --enable-gpl --enable-libx264 --enable-version3');
  assert.equal(result.status, 'blocked-configuration');
  assert.deepEqual(result.blockedFlags, ['--enable-gpl', '--enable-libx264']);
});
test('configuração não GPL exige revisão em vez de inventar aprovação jurídica', () => {
  assert.equal(auditConfiguration('configuration: --enable-shared --enable-amf --enable-libvpl').status, 'requires-distribution-review');
  assert.throws(() => auditConfiguration('configuration unavailable'));
  assert.equal(auditConfiguration('configuration: --enable-nonfree').status, 'blocked-configuration');
});

test('flags bloqueadas com valor explícito ou caixa diferente não contornam o gate', () => {
  for(const flags of ['--enable-gpl=yes','--ENABLE-NONFREE','--enable-libx264=true']) {
    assert.equal(auditConfiguration('configuration: '+flags).status,'blocked-configuration');
  }
});

test('runtime sem GPL mas sem o fallback atual nao satisfaz o contrato do app', () => {
  const listing='Encoders:\n V..... = Video\n V....D h264_amf AMD AMF\n V..... h264_qsv Intel\n V....D h264_mf MF (description mentions libx264)\n A....D libx264 audio is not a video encoder\n';
  const result=auditEncoderContract(listing,['h264_amf','h264_qsv','libx264']);
  assert.equal(result.status,'missing-required-encoders');
  assert.deepEqual(result.missing,['libx264']);
  assert.deepEqual(result.present,['h264_amf','h264_qsv']);
});

test('encoder listado exatamente nao aprova licenca nem funcionamento', () => {
  const listing=' V....D libx264rgb RGB\r\n V....D libx264 CPU\r\n';
  const result=auditEncoderContract(listing,['libx264']);
  assert.equal(result.status,'required-encoders-present');
  assert.match(result.limitation,/não prova/);
  assert.throws(()=>auditEncoderContract(' V....D libx264rgb RGB\n',['libx264','libx264']));
  assert.throws(()=>auditEncoderContract(listing,['--flag']));
  assert.throws(()=>auditEncoderContract('no encoder table',['libx264']));
});

test('politica gpl-open-source aceita GPL/libx264 com obrigacoes, mas nunca nonfree', () => {
  const gpl = auditConfiguration('configuration: --enable-gpl --enable-version3 --enable-libx264 --enable-shared', 'gpl-open-source');
  assert.equal(gpl.status, 'requires-distribution-review');
  assert.deepEqual(gpl.blockedFlags, []);
  assert.deepEqual(gpl.copyleftFlags, ['--enable-gpl', '--enable-version3', '--enable-libx264']);
  assert.match(gpl.requirements.join(' '), /código-fonte correspondente/);
  for (const flags of ['--enable-nonfree', '--ENABLE-NONFREE=yes', '--enable-gpl --enable-nonfree --enable-libfdk-aac']) {
    assert.equal(auditConfiguration('configuration: ' + flags, 'gpl-open-source').status, 'blocked-configuration');
  }
});

test('politica padrao continua a de codigo privado e politica invalida e recusada', () => {
  assert.equal(auditConfiguration('configuration: --enable-gpl').status, 'blocked-configuration');
  assert.equal(auditConfiguration('configuration: --enable-gpl').policy, 'preserve-private-application-sources');
  assert.deepEqual(POLICIES, ['preserve-private-application-sources', 'gpl-open-source']);
  assert.throws(() => auditConfiguration('configuration: --enable-shared', 'other'), /Política/);
});
