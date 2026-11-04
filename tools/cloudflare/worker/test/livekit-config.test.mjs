import assert from "node:assert/strict";
import { test } from "node:test";
import { parseLiveKitConfig } from "../livekit-config.mjs";

const source = 'LIVEKIT_URL=wss://livekit.example.com\r\nLIVEKIT_API_KEY="test-key"\r\nLIVEKIT_API_SECRET=\'test-secret=with#characters\'\r\n';
test("configuração privada mantém valores e ignora campos alheios sem exportá-los", () => {
  const result = parseLiveKitConfig(source+"OTHER_SECRET=ignored\n");
  assert.equal(result.LIVEKIT_API_SECRET,"test-secret=with#characters");
  assert.equal(result.LIVEKIT_API_KEY,"test-key");
  assert.deepEqual(Object.keys(result).sort(),["LIVEKIT_API_KEY","LIVEKIT_API_SECRET","LIVEKIT_URL"]);
});
test("configuração inválida não reflete credenciais na mensagem de erro", () => {
  assert.throws(()=>parseLiveKitConfig(source.replace("wss://livekit.example.com","wss://user:private-password@livekit.example.com")),error=>!error.message.includes("private-password"));
  assert.throws(()=>parseLiveKitConfig(source+"LIVEKIT_API_KEY=another\n"));
  assert.throws(()=>parseLiveKitConfig(source.replace("LIVEKIT_API_SECRET", "MISSING")));
});
