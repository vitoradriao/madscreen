[Português](building.md)

# Build, test and debug

## Requirements

- Windows 10 1903 x64 or later (the engine uses Windows APIs; pure logic also builds and tests on Linux).
- Stable Rust, edition 2024, MSVC toolchain, Visual Studio Build Tools (C++) and Windows SDK.
- Node 22 or later and the WebView2 Runtime.
- A GPU with a D3D11 driver to run the engine end to end.

## Configure the app's server

The app does not include any server address. Set your Worker (see [Run your own server](self-hosting.md))
at build time or during development:

```powershell
# desenvolvimento: crie apps/madscreen-app/.env.local (veja .env.example)
VITE_MADSCREEN_URL=https://seu-worker.workers.dev
```

Without the variable, the app points to `https://madscreen.invalid` and the connection fails explicitly.

## Build and run

```powershell
# engine (sidecar) com transporte
cargo build --release -p madscreen-encode-test --features publish

# runtime FFmpeg (versão e SHA-256 fixos; ver runtime-distribution.md)
./tools/encode/prepare-runtime.ps1 -BundleDestination target/release/encoder-runtime

# front-end e app
npm ci --prefix viewer
npm ci --prefix apps/madscreen-app
npm run tauri --prefix apps/madscreen-app -- dev
```

The engine is located through `MADSCREEN_ENGINE`, beside the app's `.exe` (installer) or in `target/`
(development). The FFmpeg runtime is looked up in `MADSCREEN_ENCODER_RUNTIME`, in `encoder-runtime/` beside
the engine or in the engine's own folder.

To generate the installer (the same steps as the `.github/workflows/app.yml` workflow):

```powershell
cargo build --release -p madscreen-encode-test --features publish
./tools/encode/prepare-runtime.ps1 -BundleDestination apps/madscreen-app/src-tauri/encoder-runtime
New-Item -ItemType Directory -Force apps/madscreen-app/src-tauri/binaries | Out-Null
Copy-Item target/release/madscreen-encode-test.exe apps/madscreen-app/src-tauri/binaries/madscreen-encode-test-x86_64-pc-windows-msvc.exe
npm ci --prefix viewer; npm ci --prefix apps/madscreen-app
npm run tauri --prefix apps/madscreen-app -- build
```

The sidecar is included as `externalBin` (with the target triple in its name) and the `encoder-runtime/` folder
as a Tauri resource (`tauri.windows.conf.json`). Update signing uses `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (see [Updates](updater.md)).

## Checks

The root Rust workspace and the Tauri app workspace (`apps/madscreen-app/src-tauri`) are separate; run checks
in both when changing both.

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

npm test --prefix viewer                 # lógica compartilhada do viewer e do app
npm run test:browser --prefix viewer     # Chrome headless
npm run build --prefix apps/madscreen-app

# Worker (o build gera src/web.generated.ts, embutindo o viewer web; rode-o antes do typecheck e dos testes)
npm ci --prefix viewer
npm ci --prefix tools/cloudflare/worker
npm run build --prefix tools/cloudflare/worker
npm run typecheck --prefix tools/cloudflare/worker
npm test --prefix tools/cloudflare/worker

# ferramentas de release
node --test tools/release/*.test.mjs
```

Tests requiring a GPU, the FFmpeg runtime or a graphical session are marked `#[ignore]` and state their requirements.
Run the set focused on what you changed; the full suite is for cross-cutting changes and releases.

## Code conventions

- `tracing` for logs; `thiserror` in libraries and `anyhow` in binaries.
- `unsafe` isolated in `*_sys`/`ffi` modules, always with a `// SAFETY:` comment.
- No `unwrap()`/`expect()` in frame paths and no per-frame allocations in the hot path (use pools).
- Windows-specific code under `#[cfg(windows)]`; pure logic (statistics, parsing, configuration) must also build
  and test on Linux.
- Code and identifiers in English; documentation and user-facing messages in Brazilian Portuguese.

## Debug a session

The app copies local diagnostics (counters, timings and provider, without names, tokens or SDP): see
[Local diagnostics](diagnostico.md). Engine logs go to `stderr`/`tracing`, and the sidecar's `stdout` is reserved
for the JSON IPC protocol.

## Measure performance

FPS, latency or A/V synchronization claims require reproducible measurements: PresentMon 2.x, five three-minute
runs in randomized order, a 60 s warm-up, percentage loss and a Student's t confidence interval (n = 5).
Supporting scripts are in `tools/bench/` (`run_bench.ps1`, `presentmon.ps1`) and the analyzer is the
`apps/madscreen-bench` crate. Record the game/scene, GPU, driver, Windows build, resolution, encoder and network
conditions.
