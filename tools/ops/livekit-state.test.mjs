import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { copyLiveKitState } from './livekit-state.mjs';
test('backup/restauração conserva bytes, recusa corrupção/path traversal e nunca sobrescreve destino', async () => {
  const root = await mkdtemp(join(tmpdir(), 'madscreen-lk-state-'));
  try {
    const source=join(root,'source'),backup=join(root,'backup'),restored=join(root,'restored'); await mkdir(source);
    for(const name of ['docker-compose.yaml','livekit.yaml','Caddyfile']) await writeFile(join(source,name),`fixture-private:${name}`);
    await copyLiveKitState('backup',source,backup); await copyLiveKitState('restore',backup,restored);
    assert.deepEqual(await readFile(join(restored,'livekit.yaml')),await readFile(join(source,'livekit.yaml')));
    await assert.rejects(copyLiveKitState('restore',backup,restored));
    await writeFile(join(backup,'livekit.yaml'),'corrupted');
    await assert.rejects(copyLiveKitState('restore',backup,join(root,'corrupted')));
    await assert.rejects(access(join(root,'corrupted')));
    const manifest=JSON.parse(await readFile(join(backup,'manifest.json')));manifest.files[0].name='../outside';
    await writeFile(join(backup,'manifest.json'),JSON.stringify(manifest));
    await assert.rejects(copyLiveKitState('restore',backup,join(root,'traversal')));
    await writeFile(join(backup,'manifest.json'),'null');
    await assert.rejects(copyLiveKitState('restore',backup,join(root,'null-manifest')));
    await rm(join(source,'Caddyfile')); await mkdir(join(source,'Caddyfile'));
    await assert.rejects(copyLiveKitState('backup',source,join(root,'directory-input')));
  } finally {
    assert.ok(resolve(root).startsWith(resolve(tmpdir())+requireSeparator(root)));
    await rm(root,{recursive:true,force:true});
  }
});
function requireSeparator(path) { return path.includes('\\') ? '\\' : '/'; }
