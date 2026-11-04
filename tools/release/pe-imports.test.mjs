import test from 'node:test';
import assert from 'node:assert/strict';
import { peImports } from './pe-imports.mjs';

function fixture(){
  const bytes=Buffer.alloc(2048);bytes.write('MZ');bytes.writeUInt32LE(128,0x3c);bytes.write('PE\0\0',128,'binary');bytes.writeUInt16LE(1,134);bytes.writeUInt16LE(240,148);bytes.writeUInt16LE(0x20b,152);
  bytes.writeUInt32LE(0x1000,152+120);bytes.writeUInt32LE(40,152+124);
  const section=152+240;bytes.writeUInt32LE(0x1000,section+12);bytes.writeUInt32LE(1024,section+16);bytes.writeUInt32LE(512,section+20);
  bytes.writeUInt32LE(0x1080,512+12);bytes.write('KERNEL32.dll\0',640);
  return bytes;
}
test('interpreta RVA da secao sem executar DLL e recusa nomes/cabecalhos truncados',()=>{
  const valid=fixture();assert.deepEqual(peImports(valid),[{name:'kernel32.dll',kind:'import'}]);
  assert.throws(()=>peImports(valid.subarray(0,300)));
  const corrupt=fixture();corrupt.writeUInt32LE(0xffff0000,524);assert.throws(()=>peImports(corrupt));
  const path=fixture();path.write('../bad.dll\0',640);assert.throws(()=>peImports(path));
});
