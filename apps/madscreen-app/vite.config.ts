import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// O Tauri carrega o frontend de uma porta fixa em desenvolvimento.
// `@shared` aponta para viewer/shared: cliente, tokens css e componentes usados também no viewer web.
export default defineConfig({
  clearScreen: false,
  resolve: { alias: { "@shared": fileURLToPath(new URL("../../viewer/shared", import.meta.url)) } },
  server: { port: 1420, strictPort: true, host: "127.0.0.1", fs: { allow: ["../.."] } },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
});
