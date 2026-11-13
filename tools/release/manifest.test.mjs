import assert from "node:assert/strict";
import { test } from "node:test";
import { createManifest } from "./manifest.mjs";

const signature = version => Buffer.from(`untrusted comment: fixture\n${Buffer.alloc(74).toString("base64")}\ntrusted comment: timestamp:1\tfile:app.exe\tversion:${version}\n${Buffer.alloc(64).toString("base64")}\n`).toString("base64");
const valid = { version:"0.1.2",filename:"madscreen_0.1.2_x64-setup.exe",signature:signature("0.1.2"),publishedAt:"2026-10-31T12:00:00Z" };

test("manifesto público usa NSIS assinado, versão e destino fixo sem código privado",()=>{
  const result=createManifest(valid);
  assert.equal(result.version,"0.1.2");
  assert.equal(result.platforms["windows-x86_64"].signature,valid.signature);
  assert.equal(result.platforms["windows-x86_64"].url,"https://github.com/vitoradriao/madscreen/releases/download/v0.1.2/madscreen_0.1.2_x64-setup.exe");
});
test("versão adulterada, versão sem assinatura e caminhos externos são recusados",()=>{
  for(const change of [{version:"0.1.3"},{version:"01.1.2"},{version:"0.1.2/escape"},{signature:"http://signature"},{signature:""},{filename:"../app.exe"},{filename:"https://attacker/app.exe"},{publishedAt:"invalid"}]) assert.throws(()=>createManifest({...valid,...change}));
  const legacy=Buffer.from(Buffer.from(valid.signature,"base64").toString().replace("\tversion:0.1.2","")).toString("base64");
  assert.throws(()=>createManifest({...valid,signature:legacy}));
});
