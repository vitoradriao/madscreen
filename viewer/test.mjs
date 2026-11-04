import { build } from "esbuild";
import { fileURLToPath } from "node:url";

const output = new URL("../target/viewer-tests/livekit.test.mjs", import.meta.url);
await build({
  entryPoints: [fileURLToPath(new URL("tests/livekit.test.mjs", import.meta.url))],
  bundle: true, outfile: fileURLToPath(output), platform: "node", format: "esm", target: "es2022",
  alias: { "livekit-client": fileURLToPath(new URL("tests/fake-sdk.mjs", import.meta.url)) },
});
await import(output.href);

// Controlador de atualização do app (lógica pura, sem Tauri).
const updatesOutput = new URL("../target/viewer-tests/updates.test.mjs", import.meta.url);
await build({
  entryPoints: [fileURLToPath(new URL("tests/updates.test.mjs", import.meta.url))],
  bundle: true, outfile: fileURLToPath(updatesOutput), platform: "node", format: "esm", target: "es2022",
  alias: {
    "@tauri-apps/api/core": fileURLToPath(new URL("tests/fake-tauri-core.mjs", import.meta.url)),
    "@tauri-apps/api/event": fileURLToPath(new URL("tests/fake-tauri-event.mjs", import.meta.url)),
  },
  external: ["@tauri-apps/plugin-updater"],
});
await import(updatesOutput.href);
