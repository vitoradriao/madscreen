// Comparação determinística da política de recuperação; não mede rede/GPU nem glass-to-glass.
import { build } from "esbuild";
import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const shared = fileURLToPath(new URL("shared/", import.meta.url));
const before = execFileSync("git", ["show", `${process.argv[2] ?? "HEAD"}:viewer/shared/client.ts`], { cwd: root, encoding: "utf8" });
const after = await readFile(new URL("shared/client.ts", import.meta.url), "utf8");
const original = { performance: globalThis.performance, WebSocket: globalThis.WebSocket, warn: console.warn };
const results = {};
try {
  globalThis.WebSocket = { OPEN: 1 };
  console.warn = () => {};
  for (const [label, contents] of [["before", before], ["after", after]]) {
    const outfile = new URL(`../target/viewer-tests/recovery-${label}.mjs`, import.meta.url);
    await build({ stdin: { contents, resolveDir: shared, loader: "ts" }, bundle: true, outfile: fileURLToPath(outfile), platform: "node", format: "esm", target: "es2022",
      alias: { "livekit-client": fileURLToPath(new URL("tests/fake-sdk.mjs", import.meta.url)) } });
    const { WatchSession } = await import(outfile.href);
    results[label] = [];
    for (let run = 0; run < 5; run++) {
      let now = 0, firstRequest = null;
      globalThis.performance = { now: () => now };
      const session = new WatchSession({}, "owner", {}, { onState() {} });
      const pc = { getStats: async () => new Map([["video", { id: "video", type: "inbound-rtp", kind: "video", framesDecoded: 60, bytesReceived: 1000 + now, packetsReceived: 100 + now / 10, packetsLost: 0 }]]) };
      session.pc = pc;
      session.ws = { readyState: 1, send: () => { firstRequest ??= now; } };
      for (now = 0; now <= 1500 && firstRequest === null; now += 250) await session.sample(pc);
      results[label].push(firstRequest);
    }
  }
} finally {
  globalThis.performance = original.performance;
  globalThis.WebSocket = original.WebSocket;
  console.warn = original.warn;
}
console.log(JSON.stringify({ firstRecoveryRequestMs: results, simulated: true, mediaServerContacted: false }, null, 2));
