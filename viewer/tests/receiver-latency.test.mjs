import assert from "node:assert/strict";
import { test } from "node:test";
import { requestReceiverLatency } from "../shared/receiver-latency.ts";

test("receiver sem API não recebe expando que imita suporte", () => {
  const receiver = {};
  const result = requestReceiverLatency(receiver, 0, 0);
  assert.deepEqual(Object.keys(receiver), []);
  assert.equal(result.jitterBufferTarget.status, "unsupported");
  assert.equal(result.playoutDelayHint.status, "unsupported");
});

test("preferências herdadas usam unidades da API sem prometer buffer real", () => {
  const receiver = Object.create({ jitterBufferTarget: null, playoutDelayHint: null });
  const result = requestReceiverLatency(receiver, 40, 40);
  assert.equal(receiver.jitterBufferTarget, 40);
  assert.equal(receiver.playoutDelayHint, 0.04);
  assert.equal(result.playoutDelayHint.reportedMs, 40);
  assert.equal(result.jitterBufferTarget.status, "readback-matched");
});

test("setter silencioso não comprova preferência aplicada", () => {
  const receiver = { get jitterBufferTarget() { return 20; }, set jitterBufferTarget(_) {} };
  assert.deepEqual(requestReceiverLatency(receiver, 0).jitterBufferTarget,
    { requestedMs: 0, reportedMs: 20, status: "readback-different" });
});

test("API que recusa ou não fornece leitura não interrompe mídia", () => {
  const refused = { set jitterBufferTarget(_) { throw Error("unsupported"); } };
  assert.equal(requestReceiverLatency(refused, 0).jitterBufferTarget.status, "refused");
  const unreadable = { set jitterBufferTarget(_) {}, get jitterBufferTarget() { throw Error("read"); } };
  assert.equal(requestReceiverLatency(unreadable, 0).jitterBufferTarget.status, "readback-unavailable");
  assert.equal(requestReceiverLatency(undefined, 40).playoutDelayHint, undefined);
});
