export interface ReceiverPreference {
  requestedMs: number;
  reportedMs: number | null;
  status: "unsupported" | "refused" | "readback-matched" | "readback-different" | "readback-unavailable";
}

export interface ReceiverLatency {
  jitterBufferTarget: ReceiverPreference;
  playoutDelayHint?: ReceiverPreference;
}

/** Readback confirms only a preference. It does not measure the actual playout buffer. */
function request(receiver: object | undefined, key: string, requestedMs: number, unitsPerMs: number): ReceiverPreference {
  const result: ReceiverPreference = { requestedMs, reportedMs: null, status: "unsupported" };
  if (!Number.isFinite(requestedMs) || requestedMs < 0) throw new RangeError("Invalid receiver latency");
  if (!receiver || !(key in receiver)) return result;
  const target = receiver as Record<string, unknown>;
  try { target[key] = requestedMs * unitsPerMs; }
  catch { return { ...result, status: "refused" }; }
  try {
    const value = target[key];
    if (typeof value !== "number" || !Number.isFinite(value)) return { ...result, status: "readback-unavailable" };
    const reportedMs = value / unitsPerMs;
    return { ...result, reportedMs, status: Math.abs(reportedMs - requestedMs) < 0.001 ? "readback-matched" : "readback-different" };
  } catch { return { ...result, status: "readback-unavailable" }; }
}

export function requestReceiverLatency(receiver: object | undefined, jitterMs: number, playoutMs?: number): ReceiverLatency {
  return {
    jitterBufferTarget: request(receiver, "jitterBufferTarget", jitterMs, 1),
    ...(playoutMs === undefined ? {} : { playoutDelayHint: request(receiver, "playoutDelayHint", playoutMs, 0.001) }),
  };
}

export function copyReceiverLatency(value: ReceiverLatency | null): ReceiverLatency | null {
  return value && {
    jitterBufferTarget: { ...value.jitterBufferTarget },
    ...(value.playoutDelayHint ? { playoutDelayHint: { ...value.playoutDelayHint } } : {}),
  };
}
