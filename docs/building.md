# Compilar, testar e depurar

## Requisitos

- Windows 10 1903 x64 ou posterior (o engine usa APIs do Windows; a lógica pura compila e testa também em Linux).
- Rust estável, edição 2024, toolchain MSVC, Build Tools do Visual Studio (C++) e Windows SDK.
- Node 22 ou superior e o WebView2 Runtime.
- Uma GPU com driver D3D11 para rodar o engine de ponta a ponta.

## Configurar o servidor do app

O app não traz o endereço de nenhum servidor. Defina o seu Worker (veja [Rodar o seu servidor](self-hosting.md)) em
build ou desenvolvimento:

```powershell
# desenvolvimento: crie apps/madscreen-app/.env.local (veja .env.example)
VITE_MADSCREEN_URL=https://seu-worker.workers.dev
```

Sem a variável, o app aponta para `https://madscreen.invalid` e a conexão falha de forma explícita.

## Compilar e rodar

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

O engine é localizado pela variável `MADSCREEN_ENGINE`, ao lado do `.exe` do app (instalador) ou em `target/`
(desenvolvimento). O runtime FFmpeg é procurado em `MADSCREEN_ENCODER_RUNTIME`, em `encoder-runtime/` ao lado do
engine ou na própria pasta do engine.

Para gerar o instalador: `npm run tauri --prefix apps/madscreen-app -- build` (a assinatura de atualização usa as
variáveis `TAURI_SIGNING_PRIVATE_KEY` e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; veja [Atualizações](updater.md)).

## Verificações

O workspace Rust da raiz e o do app Tauri (`apps/madscreen-app/src-tauri`) são separados; rode as verificações nos
dois quando mexer em ambos.

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

Testes que exigem GPU, runtime FFmpeg ou sessão gráfica estão marcados com `#[ignore]` e dizem o que exigem.
Rode o conjunto focado no que você alterou; a suíte completa serve para mudanças transversais e releases.

## Convenções de código

- `tracing` para logs; `thiserror` em bibliotecas e `anyhow` em binários.
- `unsafe` isolado em módulos `*_sys`/`ffi`, sempre com comentário `// SAFETY:`.
- Nenhum `unwrap()`/`expect()` em caminhos de quadro e nenhuma alocação por quadro no caminho quente (use *pools*).
- Código específico do Windows sob `#[cfg(windows)]`; lógica pura (estatística, parsing, configuração) deve compilar
  e testar também em Linux.
- Código e identificadores em inglês; documentação e mensagens ao usuário em português do Brasil.

## Depurar uma sessão

O app copia um diagnóstico local (contadores, tempos e provedor, sem nomes, tokens ou SDP): veja
[Diagnóstico local](diagnostico.md). Logs do engine saem em `stderr`/`tracing`, e o `stdout` do sidecar é reservado
ao protocolo JSON do IPC.

## Medir desempenho

Afirmações de FPS, latência ou sincronia A/V precisam de medição reproduzível: PresentMon 2.x, cinco corridas de três
minutos em ordem aleatória, aquecimento de 60 s, perda percentual e intervalo de confiança t de Student (n = 5). Os
scripts de apoio estão em `tools/bench/` (`run_bench.ps1`, `presentmon.ps1`) e o analisador é o crate
`apps/madscreen-bench`. Registre jogo/cena, GPU, driver, build do Windows, resolução, encoder e condições de rede.
