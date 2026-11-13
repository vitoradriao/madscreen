import { readFile, mkdir, writeFile, rename } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { readCss } from "../../ui/read-css.mjs";

// Gera src/web.generated.ts (viewer web + tokens + fonte) e dist/worker.js (ESM, ES2022).
const root = new URL("./", import.meta.url);
const viewer = new URL("../../../viewer/", root);
const minify = process.argv.includes("--minify");
if (process.argv.slice(2).some((arg) => arg !== "--minify")) throw new Error("Uso: node build.mjs [--minify]");

async function optional(url, encoding = "utf8") {
  try {
    return await readFile(url, encoding);
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

const webMain = fileURLToPath(new URL("web/main.ts", viewer));
const hasWeb = (await optional(new URL("web/main.ts", viewer))) !== null;
let js = "";
if (hasWeb) {
  const result = await build({
    entryPoints: [webMain],
    bundle: true, write: false, format: "iife", platform: "browser", target: "es2022",
    minify, legalComments: "none", logLevel: "warning",
  });
  js = result.outputFiles[0].text;
}
const [html, tokens, webCss, font] = await Promise.all([
  optional(new URL("web/index.html", viewer)),
  optional(new URL("shared/tokens.css", viewer)),
  optional(new URL("web/web.css", viewer)),
  optional(new URL("shared/fonts/inter-latin.woff2", viewer), null),
]);
// Resolve os componentes importados e incorpora a mesma Inter local do app.
const bundledTokens = tokens ? await readCss(new URL("shared/tokens.css", viewer)) : "";
const placeholder = '<!doctype html><html lang="pt-BR"><meta charset="utf-8"><title>madscreen</title><p>Viewer indisponível neste build.</p></html>';
const assets = {
  html: html ?? placeholder,
  js,
  css: `${bundledTokens}\n${webCss ?? ""}`,
  fontBase64: font ? font.toString("base64") : "",
  // WSORIGIN é trocado pelo origin do Worker (wss://host) ao servir a página.
  csp: "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self' data:; img-src 'self' data: blob: https://cdn.discordapp.com https://lh3.googleusercontent.com; media-src 'self' blob: mediastream:; worker-src 'self' blob:; connect-src 'self' WSORIGIN SFUORIGINS; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
};
await mkdir(new URL("src/", root), { recursive: true });
const generated = new URL("src/web.generated.ts", root);
const contents = `// Gerado por build.mjs; não editar.\nexport const webAssets = ${JSON.stringify(assets)};\n`;
// Wrangler watches source files. Rewriting identical output retriggers its build;
// replace changed output atomically so TypeScript never reads a truncated module.
if (await optional(generated) !== contents) {
  const temporary = new URL(`src/web.generated.${process.pid}.tmp`, root);
  await writeFile(temporary, contents);
  await rename(temporary, generated);
}
await build({
  absWorkingDir: fileURLToPath(root),
  entryPoints: ["src/index.ts"],
  outfile: "dist/worker.js",
  bundle: true,
  format: "esm",
  platform: "neutral",
  target: "es2022",
  external: ["cloudflare:workers"],
  define: { __HUB_REVISION__: JSON.stringify(process.env.MADSCREEN_BUILD_REVISION ?? "development") },
  minify,
  legalComments: "none",
  logLevel: "info",
});
if (!hasWeb) process.stderr.write("viewer/web ausente: placeholder embutido.\n");
