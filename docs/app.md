# App desktop (Tauri v2)

## Compatibilidade da reprodução

O vídeo usa decode acelerado por hardware no WebView2, com o escoamento cadenciado para evitar o estouro de fila do decoder.

Em **Conta → Reprodução de vídeo**, o modo de compatibilidade desliga o decode
acelerado do WebView2 na próxima abertura e usa CPU para reproduzir vídeo.
É um contorno reversível para freezes observados no caminho acelerado; aumenta
potencialmente o uso de CPU e não altera captura/NVENC do transmissor. O padrão
continua acelerado. Salvar não encerra a live: pare quando conveniente, escolha
Sair no menu da bandeja e abra novamente. Fechar apenas a janela mantém o app
na bandeja e não aplica a troca. Desmarcar e reabrir retorna ao modo anterior.

`apps/madscreen-app`: janela escura estilo Discord para compartilhar a tela, convidar um amigo
e assistir no mesmo app.

O app entra com Discord/Google (ou usuário + convite), mostra servidores, canais e quem está ao vivo e
permite assistir e compartilhar pelo Worker da Cloudflare, sem Tailscale nem servidor próprio. Contas,
amigos e rotas: [cloudflare-auth.md](cloudflare-auth.md). O produto é `madscreen`, executável
`madscreen-app.exe`, identificador estável `app.madscreen.desktop`: um instalador para todas as contas.

## Arquitetura

```text
Tauri (WebView2 + Rust)  ──stdin JSON──►  madscreen-encode-test --ipc  (sidecar, 1 processo por sessão)
   supervisor + tray      ◄─stdout JSON──  picker WGC → GPU → encoder automático → transporte
```

- Protocolo: crate `crates/madscreen-ipc` (uma linha JSON por mensagem). `Start` abre o seletor
  de captura do Windows (`GraphicsCapturePicker`), `Stop` ou EOF do stdin encerram; o engine manda
  `status` a cada 1 s com FPS e bitrate medidos na janela. Detalhes em `docs/encode.md`.
- Cada compartilhamento é um processo: ao parar, o processo sai e a GPU volta ao jogo.
- Tela inteira no Windows 10 usa Desktop Duplication automaticamente no engine;
  janelas e Windows 11 usam WGC. O seletor nativo legado continua WGC.
  [Comportamento, diagnóstico e limites](encode.md#captura-de-monitor-no-windows-10-60).
- Minimizar mantém a janela na barra de tarefas. Fechar esconde a janela na bandeja, após a explicação no primeiro fechamento. Oculto, o webview não recebe eventos; o engine
  segue em outro processo. Menu da bandeja: abrir, parar o compartilhamento, sair.
- O engine é localizado por `MADSCREEN_ENGINE`, ao lado do `.exe` (instalador) ou em `target/` (dev).
- Abas: **Servidores**, **Compartilhar**, **Assistir** e **Conta**; sem sessão, só a tela **Entrar**. O
  frontend importa `viewer/shared` (cliente do Worker, tokens css, painéis de entrar/servidores/assistir)
  pelo alias `@shared` (vite e tsconfig); o mesmo código roda no viewer web servido pelo Worker.
- Assistir reproduz áudio quando a transmissão o oferece, com volume e mudo lembrados por conta e
  usuário que transmite. A preferência global antiga serve como valor inicial até salvar uma escolha.
  Se o navegador bloquear autoplay com som, a imagem continua muda e aparece **Clique para ativar
  o som**. A recuperação de quadros perdidos é automática; não há botão de quadro-chave no player.
- **Copiar diagnóstico**, nos controles do player, guarda na área de transferência
  as últimas 60 amostras de rede, decode e apresentação. Use logo após uma travadinha;
  os dados ficam locais e não incluem tokens, endereços ou conteúdo da transmissão.
- O player preserva a proporção original e centraliza o vídeo, preenchendo o espaço restante com
  preto. Uma transmissão 16:9 em monitor 21:9 fica inteira com barras laterais, inclusive em tela
  cheia. O vídeo não determina a altura do layout; duas telas e a miniatura mantêm o mesmo ajuste.
  `npm run test:browser --prefix viewer` verifica uma fonte 1920×1080 em diferentes tamanhos de
  janela, duas telas, miniatura e tela cheia; evidências locais em `bench-results/livekit-browser/`.
- Com duas transmissões, **Trocar destaque** (atalho **D**) alterna um layout 70/30;
  **Telas iguais** restaura as duas metades. Em janela estreita do app, o destaque
  ocupa a maior parte da altura. A troca conserva os vídeos e as conexões.
  **Som só no destaque** silencia temporariamente a outra tela sem alterar suas
  preferências salvas. Com telas iguais, o som acompanha a tela selecionada.
  Cada cabeçalho informa se o som está ativo, desativado ou restrito ao destaque.
  **M** e **F** controlam a tela selecionada; em tela cheia, apenas a tela visível.
  Sair da tela cheia restaura o layout anterior e mantém a outra live conectada.
- Em **Compartilhar**, **Mostrar prévia (sem som)** abre uma assinatura de vídeo da própria live.
  A prévia é opcional, fecha ao trocar de aba ou parar a transmissão e não assina áudio. Assistir à
  própria live em qualquer painel também fica sempre mudo, com aviso e controles de som desabilitados;
  o Worker impede a assinatura do próprio áudio, inclusive para clientes antigos.
- Durante a live, escolha resolução/FPS e use **Aplicar qualidade**, ou use
  **Trocar janela ou tela** para selecionar outra fonte e seu áudio. O canal é
  preservado; cancelar o seletor mantém a live atual. Aplicar a mudança reinicia
  a publicação e pode causar uma breve reconexão. Se a nova captura falhar, o
  supervisor tenta restaurar a anterior uma vez. Parar, sair e instalar invalidam
  mudanças em voo. Implementação e ensaio Magalu da #61.
- Uma fonte menor que o preset selecionado passa a **Nativa**, preservando sua
  proporção sem ampliar a imagem. Uma escolha explícita de 720p é respeitada.
- **Som do PC todo** usa process loopback excluindo o processo do app e seus descendentes, para
  não recapturar o áudio de outra live reproduzida pelo WebView2.
- Login por navegador: `tauri-plugin-opener` abre `/auth/{discord|google}/start` (permitido só para
  `*.workers.dev`, em `capabilities/default.json`) e o app recebe a sessão por polling
  (`POST /auth/exchange`). A sessão fica em `localStorage` da WebView.
- Compartilhar passa também o `channelId` selecionado por comando Tauri, IPC e engine ao Worker,
  que confere a participação no servidor. **Transmitir aqui** seleciona o canal sem redigitar dados.
- O instalador 0.1.1 registra `madscreen://invite/CODIGO` para contas novas e para o app já aberto.
  O código atravessa o login e seleciona o servidor/canal após o resgate. Não há token no deep link.
- Visual: grafite e branco, tokens em `viewer/shared/tokens.css`, OpenAI Sans em toda a
  interface (incluindo diagnósticos), pesos 400/500/600/700 carregados do CDN oficial
  `cdn.openai.com`. Sem conexão, usa a fonte de sistema de reserva. Não há cópia
  redistribuída da OpenAI Sans no pacote. Elementos nativos do Windows usam a fonte do sistema.
- Marca “m.” substitui a TV no cabeçalho/login e nos ícones do app, barra de tarefas
  e bandeja. `tools/ui/generate-brand.mjs` gera SVG/PNG/ICO multirresolução/ICNS a
  partir do mesmo desenho de `viewer/shared/icons.ts` (Chrome e Python/Pillow).
- O app não instala Tailscale nem servidor LiveKit local e não expõe comandos desses fluxos antigos.
  Os crates de transporte e as flags LiveKit do engine continuam disponíveis para desenvolvimento
  e para a futura integração de outro provedor; não são necessários para usar o app.

## Requisitos para instalar e transmitir

- Windows 10 1903 x64 (build 18362) ou posterior, **quando Windows.Graphics.Capture
  estiver disponível no dispositivo**. Windows 11 24H2+ recomendado. O instalador confere
  a versão do Windows e instala WebView2 se necessário; o engine consulta `IsSupported`
  e valida a captura ao iniciar a sessão.
- GPU com Direct3D 11, feature level 11.0 ou superior, para captura e conversão. **NVIDIA
  não é obrigatória.** Sem encoder de hardware utilizável, a codificação usa o processador.
- Para acelerar: driver do fabricante atualizado com NVIDIA NVENC (API 12.2+), AMD AMF ou
  Intel QuickSync. Em PCs híbridos, QuickSync exige que a GPU Intel esteja habilitada e
  com driver; não basta a CPU ser Intel. Drivers não são instalados pelo madscreen.
- O instalador inclui o runtime FFmpeg com AMF, QSV e x264. O usuário não precisa instalar
  FFmpeg, SDKs de vídeo, Rust ou ferramentas de compilação. x264 usa até duas threads;
  resolução e FPS viáveis por CPU dependem do processador e da carga do jogo.
- Internet e acesso ao servidor de transmissão. Para assistir, permanece o suporte a
  H.264 do WebView2/navegador, independentemente do encoder utilizado pelo transmissor.

No Windows 10 1903/1909, o cursor segue o padrão do Windows: a API para desativá-lo
só existe a partir da versão 2004. Controle de borda e intervalo mínimo de captura são
opcionais e só são aplicados quando disponíveis. A captura continua funcionando sem eles.
Áudio por aplicativo depende de suporte ao process loopback (a Microsoft documenta
build 20348+); quando não houver suporte, o app informa que a tela segue sem som.
Referências: [captura Win32 no Windows 10 1903](https://blogs.windows.com/windowsdeveloper/2019/09/16/new-ways-to-do-screen-capture/),
[controle de cursor](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.iscursorcaptureenabled),
[process loopback](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/).

A tela **Compartilhar** mostra **Codificador ativo** com o nome da sessão realmente
inicializada: NVIDIA NVENC, AMD AMF, Intel QuickSync ou CPU (x264). O nome atravessa o
evento IPC `started` e é limpo ao encerrar, sem confundir a política automática com o
backend que de fato foi selecionado. Engines antigos podem omitir esse campo.

A seleção é automática a cada transmissão: primeiro o encoder de hardware do fabricante
da GPU de captura; em seguida os demais backends de hardware; por último x264. Falhas de
driver, inicialização, codec, resolução e teste de codificação são registradas e a busca
continua. Não há escolha manual obrigatória no app. A seleção busca menor impacto pela
proximidade da captura e uso de hardware; não é um ranking de qualidade medido entre GPUs.

O NVENC mantém entrada D3D11 zero-copy. AMF/QuickSync/x264 usam conversão GPU para NV12,
readback para memória do sistema e entrada no FFmpeg; AMF e QSV continuam codificando em
hardware. Esse caminho de compatibilidade tem custo de cópia e precisa de benchmarks nos
respectivos PCs antes de prometer as metas de FPS do caminho NVENC.

## Compilação local

```powershell
cargo build --release -p madscreen-encode-test --features publish
./tools/encode/prepare-runtime.ps1 -BundleDestination target/release/encoder-runtime
cd apps/madscreen-app
npm ci
npm run tauri dev        # usa target/release/madscreen-encode-test.exe como engine
```

O crate `src-tauri` tem workspace próprio (precisa do frontend buildado e das libs do WebView); em
Linux, `cargo check` exige `libwebkit2gtk-4.1-dev` e `libsoup-3.0-dev`.

## Instalador e CI

`.github/workflows/app.yml` (windows-latest): builda o engine com `publish`, copia como
`binaries/madscreen-encode-test-x86_64-pc-windows-msvc.exe` (externalBin só no Windows, via
`tauri.windows.conf.json`), builda o frontend, roda `fmt`/`clippy`/testes do app e gera o instalador NSIS
(`npx tauri build --bundles nsis`), publicado como artefato `madscreen-setup-windows-x64`.
Mudanças em `viewer/shared` também disparam esse build. O instalador inclui engine e instalação do
WebView2 se faltar, sem exigir configuração de captura/publicação separada para os amigos.

O CI executa `tools/encode/prepare-runtime.ps1`, confere SHA-256 do runtime mínimo FFmpeg + x264
(ABI avcodec 62 / avutil 60; ver `tools/encode/runtime-gpl.json`) e embute `src-tauri/encoder-runtime/` ao lado do engine pelo
mapeamento de recursos do Tauri. Para build local do instalador, execute também o script com
`-BundleDestination apps/madscreen-app/src-tauri/encoder-runtime`. As DLLs e licenças são
artefatos gerados, ignorados pelo Git. O runtime inclui x264 e é GPL; veja
[NOTICE do runtime](../tools/encode/NOTICE.md) para fontes e condições de redistribuição.

### Migração da edição antiga

O hook NSIS em `src-tauri/windows/hooks.nsh` usa o desinstalador registrado da edição antiga,
aguarda sua conclusão e conserva os dados do usuário. Se a remoção falhar, a instalação para com
mensagem clara, evitando deixar duas instalações. Não há remoção recursiva de pastas pelo hook.

Antes de abrir a primeira janela, `migration.rs` importa apenas o banco de `localStorage` do perfil
anterior, onde ficam login e preferências. A cópia é publicada por renomeação de uma pasta temporária;
falhas não deixam um banco parcialmente importado. Um banco atual nunca é sobrescrito, o original é
mantido e as configurações de qualidade são importadas sem credenciais antigas de LiveKit.

Não marque **apagar dados** se fizer a desinstalação antiga manualmente. Se ambas as edições já
têm sessões próprias, o app único mantém a sessão que já estava nele.

O mecanismo usa os [hooks de instalação do Tauri](https://v2.tauri.app/distribute/windows-installer/#extending-the-installer).

## Updater

`tauri-plugin-updater` aponta para
`https://github.com/vitoradriao/madscreen-releases/releases/latest/download/latest.json`
(repositório público, separado do código). A 0.1.2 embarca a chave pública final e
exige que a versão anunciada corresponda à versão assinada. A chave privada e a
senha ficam fora do Git e nos secrets `TAURI_SIGNING_PRIVATE_KEY` e
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` do CI.

Ao abrir, o app consulta atualizações e mostra aviso. A instalação continua
dependendo de **Instalar e reiniciar**; há progresso de download e validação antes
de executar o instalador. A 0.1.1 precisa da instalação manual da 0.1.2 uma vez.

O workflow gera `.sig`, `latest.json` e hashes, verifica assinatura e versão e
guarda o pacote como artefato. Publicação no repositório público pode ser solicitada
por `workflow_dispatch` com `publish_release=true`, exigindo `RELEASE_REPO_TOKEN`
restrito ao repositório de releases. Sem ele, o build continua gerando artefatos;
uma solicitação de publicação falha explicitamente no preflight. O token entre
repositórios foi cadastrado em 31/out/2026; conferir a execução do CI antes de
considerar a publicação automática validada.

Procedimento de assinatura, publicação e validação em
[updater.md](updater.md). Não há chave privada no repositório.

## Melhorias de UX locais — 2/nov/2026

Implementadas no código local, ainda sem publicação/instalação:

- Convite mostra seu destino antes de entrar; Discord é a opção principal e
  Entrar sem Discord abre a alternativa com convite/chave de recuperação.
- O primeiro fechamento explica a bandeja e como voltar, parar ou sair. A janela
  só é ocultada depois da confirmação; o menu da bandeja continua disponível.
- Conta → Avisos permite desligar o som de entrada de espectadores. O aviso curto
  requer primeira interação com o app e não toca na prévia própria ou conexão
  adicional da mesma pessoa.
- No player, o botão de som alterna entre mudo e o volume anterior. Após deslizar
  até zero, ele restaura o último volume audível daquela transmissão.
- Detalhes e ajuda oferece observações do último intervalo ao abrir/atualizar,
  separando recepção, decodificação e apresentação; dados insuficientes ficam
  indeterminados. O computador do amigo precisa de diagnóstico separado.

Evidência e aceites ainda necessários: relatório UX/UI.
