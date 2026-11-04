import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, copyFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { inventoryRuntime } from './runtime-inventory.mjs';

test('inventário confronta DLLs e avisos distribuídos com a origem e não aprova licença', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'madscreen-runtime-'));
  const source = join(directory, 'source'), bundle = join(directory, 'bundle');
  try {
    await mkdir(join(source, 'bin'), { recursive: true }); await mkdir(bundle);
    for (const name of ['avcodec-62.dll', 'avutil-60.dll', 'swresample-6.dll', 'libx264-165.dll', 'libvpl.dll', 'LICENSE', 'README.txt']) {
      const path = join(source, name.endsWith('.dll') ? 'bin' : '', name);
      await writeFile(path, `original ${name}`); await copyFile(path, join(bundle, name));
    }
    await writeFile(join(bundle, 'NOTICE.md'), 'notice');
    const good = await inventoryRuntime(source, bundle);
    assert.equal(good.status, 'inventory-matches-source');
    assert.equal(good.distributionApproved, false);
    await writeFile(join(bundle, 'avcodec-62.dll'), 'modified');
    await writeFile(join(bundle, 'unreviewed.dll'), 'extra');
    await rm(join(bundle, 'LICENSE'));
    const bad = await inventoryRuntime(source, bundle);
    assert.equal(bad.status, 'blocked-inventory');
    assert.ok(bad.errors.includes('source-mismatch:avcodec-62.dll'));
    assert.ok(bad.errors.includes('unexpected-dll:unreviewed.dll'));
    assert.ok(bad.errors.includes('missing:LICENSE'));
  } finally { await rm(directory, { recursive: true, force: true }); }
});
