import { build } from "esbuild";
import { fileURLToPath } from "node:url";

// Os testes usam Web Crypto/fetch do Node e um DO falso,
// sem publicar Worker nem acessar serviços remotos.
const output = new URL("../../../target/worker-tests/sfu.test.mjs", import.meta.url);
await build({
  entryPoints: ["tests/sfu.test.mjs"], bundle: true, outfile: fileURLToPath(output),
  platform: "node", format: "esm", target: "es2022",
  plugins: [{ name: "durable-object-test", setup(build) {
    build.onResolve({ filter: /^cloudflare:workers$/ }, () => ({ path: "workers", namespace: "test" }));
    build.onLoad({ filter: /.*/, namespace: "test" }, () => ({ contents:
      "export class DurableObject { constructor(ctx, env) { this.ctx = ctx; this.env = env; } }" }));
  } }],
});
await import(output.href);
