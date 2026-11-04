# Ferramenta de prévia WGC (madscreen-preview)

`madscreen-preview` abre o seletor nativo do Windows para escolher uma janela ou
um monitor e mostra a captura ao vivo, com resolução e contadores de FPS no título.
É uma prévia somente de vídeo, sem áudio, encoder ou transporte.

Requer Windows 10 versão 2004+ ou Windows 11 x64, sessão gráfica interativa e GPU
com driver D3D11 e feature level 11.0+. NVIDIA é o alvo de validação do projeto.
Para compilar: Rust stable MSVC, Visual Studio Build Tools com C++ e Windows SDK.
O HLSL é compilado em runtime pelo `D3DCompile` do Windows (`d3dcompiler_47.dll`).

```powershell
cargo run -p madscreen-preview --release
cargo run -p madscreen-preview --release -- --no-cursor --border --log debug
$env:RUST_LOG = "madscreen_capture=debug,madscreen_preview=info"
cargo run -p madscreen-preview --release
```

`--diagnostics` registra a cada 2 s a janela em primeiro plano; `--no-cursor` começa sem cursor; `--border` mantém a borda do sistema sem pedir
remoção; `--log` aceita `trace`, `debug`, `info`, `warn`, `error` e `off`.
`RUST_LOG` válido tem precedência. O console fica visível tanto em debug quanto
em release nesta fase para permitir acompanhar os logs. Em Linux, o binário
compila, informa que requer Windows e termina com código 1.

Após uma execução bem-sucedida do workflow CI no GitHub, o artifact
`madscreen-preview-windows-x64` contém `madscreen-preview.exe`, com retenção de
14 dias. Baixe e extraia o ZIP para executar sem compilar; o artifact só existe
depois que esse workflow rodar.

| Ação | Controle |
|---|---|
| Abrir seletor de janela ou monitor | Botão **Escolher janela ou tela...** ou **Ctrl+O** |
| Alternar captura do cursor | **C** |
| Alternar borda obrigatória / pedido de remoção | **B** |
| Parar captura e liberar a fonte | **Esc** |

Cancelar o seletor mantém a captura atual. Os atalhos funcionam quando o app
está em foco, inclusive com foco no botão. Fechar a fonte retorna ao estado sem
fonte. Perda/reset do dispositivo encerra a captura e solicita **Ctrl+O**; a
próxima seleção recria o dispositivo e a swapchain. Não há recuperação automática
nem F11 nesta versão.

## Caminho da imagem e decisões

O seletor `GraphicsCapturePicker` recebe a HWND principal por
`IInitializeWithWindow::Initialize`. A thread de UI inicializa WinRT em STA e
registra `SetCompleted`; o resultado fica num slot sincronizado e uma mensagem
Win32 avisa a UI. Não há espera síncrona pela escolha do usuário.

WGC usa `CreateFreeThreaded`, BGRA8 e dois buffers (a biblioteca aceita dois ou
três). O handler obtém a textura D3D11 da superfície, submete `CopyResource`
**GPU → GPU** para uma textura própria e fecha/devolve imediatamente o frame ao
pool. Não há textura staging, `Map`, readback ou imagem desenhada por GDI. O
buffer do contador é alocado na criação, e textura/SRV são reutilizadas até
mudar a resolução. Ao detectar `ContentSize` diferente, descarta-se aquele frame,
fecha-se seu empréstimo, recria-se o pool e realoca-se a mailbox para o próximo
frame, evitando copiar formatos/tamanhos incompatíveis ou regiões indefinidas.

A mailbox tem profundidade um: frames novos substituem o anterior. O handler
usa `try_lock` e descarta se a UI ainda estiver submetendo o desenho, sem esperar
pelo consumidor. O contexto imediato tem multithread protection. A UI mantém o
empréstimo somente até `Draw`, e o solta **antes de `Present`**. Notificações são
agrupadas para não acumular uma fila de mensagens por frame. Estatísticas de
sobrescritas e descartes estão disponíveis em `CaptureStats`.

A prévia usa swapchain flip-discard, BGRA8, dois buffers, fila de apresentação
limitada a um frame, triângulo sem vertex buffer e amostragem linear com letterbox.
`Present(1, 0)` usa vsync por padrão; não há modo tearing nesta fase. Só se
desenha ao receber frame novo, ao redimensionar/restaurar a janela ou ao mudar
para o estado sem fonte. O timer de 500 ms atualiza apenas o título. Não há loop
de renderização ocioso. Prioridades de processo, CPU e GPU ficam nos padrões
normais; nenhuma prioridade REALTIME é solicitada.

O item retornado pelo seletor não expõe um caminho público inverso até sua
HWND/HMONITOR. Por isso, a prévia usa o adapter de hardware padrão e registra
esse fallback. A entrada `FrameSource::start(CaptureTarget, ...)`, para handles
conhecidos, procura o adapter do monitor via DXGI (ou do monitor da janela) e usa
o mesmo adapter quando encontrado. **Validar cross-GPU em notebooks/híbridos**;
a prévia não promete que o adapter padrão coincide com o jogo escolhido.

O esqueleto do §7.1 foi adaptado para um `FrameSource` com tipo associado e um
`FrameLease` emprestado: a imagem pertence à mailbox própria, e não ao pool WGC.
Isso impede que o consumidor mantenha o frame do pool durante o vsync e permite
serializar a cópia com o envio do desenho. `next_frame` é não bloqueante, podendo
retornar antes do timeout. `SystemRelativeTime` é um `TimeSpan` em unidades de
100 ns; ele é convertido explicitamente para `Duration` e ticks QPC conforme a
frequência, sem confundir as duas unidades.

## O que os contadores medem

- **Captura FPS:** intervalos dos timestamps `SystemRelativeTime` dos frames
  recebidos, numa janela deslizante de um segundo. Conta antes dos descartes da
  mailbox; não representa só os frames desenhados. Sem novos frames por um
  segundo, o contador mostrado volta a zero.
- **Apresentado FPS:** chamadas `Present` bem-sucedidas, excluindo oclusão,
  medidas por `Instant`, também numa janela de um segundo. Um redesenho no
  redimensionamento pode apresentar a mesma imagem novamente. É taxa de
  submissão da prévia; não comprova o instante físico de scanout.
- **Resolução:** tamanho do conteúdo reportado por WGC, sem confundir com o
  tamanho redimensionado da janela de prévia. Ambos os contadores precisam de
  pelo menos dois timestamps para estimar uma taxa.

Uma prévia a 60 Hz pode apresentar menos que a captura a 144/180 Hz. Fontes
estáticas, minimizadas ou ocluídas podem não produzir atualizações contínuas;
FPS WGC não é uma medição direta do FPS interno do jogo. A captura permanece
ativa ao minimizar a prévia, mas ela não apresenta enquanto minimizada.

Não se mede latência glass-to-glass, encode, rede, qualidade de compressão nem
perda de FPS do jogo apenas olhando esses números. Compare com PresentMon e o
harness M0.3. Ainda não há exportação CSV/jitter, DDA ou comparação dos três
jogos exigidos no M0.1 completo.

## Limitações conhecidas

- Borda amarela: a remoção só é solicitada depois de
  `RequestAccessAsync(Borderless)` retornar `Allowed`. Ausência da API, recusa
  ou falha mantêm a captura com a borda do sistema e geram log. O executável
  não é empacotado e não declara `graphicsCaptureWithoutBorder` em pacote;
  portanto, a borda pode permanecer, especialmente no Windows 10. Mesmo com
  permissão, outra sessão/política pode exigir borda. O título diz **remoção
  solicitada**, sem afirmar que ela efetivamente desapareceu.
- `MinUpdateInterval` (Windows 11 24H2+, build 26100): no 24H2 o WGC pode
  limitar a captura a ~60 FPS quando a propriedade não é definida, mesmo com o
  conteúdo a mais de 100 FPS. Por isso a sessão **sempre** define o intervalo:
  1 ms sem FPS alvo (sem limitação prática) ou 90 % do período quando há
  `target_fps` (o intervalo é um limitador e o WGC pode entregar menos que o
  pedido). O título mostra `intervalo mín.` com o valor aplicado, ou
  `indisponível` em Windows anteriores à 24H2 (cadência nativa, sem limitador
  alternativo). O intervalo é um limite de atualização, não garantia de FPS.
- A captura nunca é segurada pelo vsync da prévia: `FrameArrived` roda em thread
  livre, copia GPU→GPU e devolve o frame; o `Present(1, 0)` só atrasa o desenho
  da prévia (a mailbox guarda só o frame mais novo). A prévia pode apresentar
  menos que a captura, nunca o contrário por causa do vsync.
- **Geração de quadros (frame generation).** Mods/drivers de frame generation
  (DLSS FG, FSR3, Lossless Scaling etc.) fazem o jogo renderizar, por exemplo,
  ~60 FPS reais e o overlay da NVIDIA mostrar ~180 FPS apresentados. A WGC
  entrega o que o compositor (DWM) recebe: dependendo de como o gerador de
  quadros apresenta os quadros interpolados, a captura pode ver só os quadros
  reais (~50-60) ou todos. Isso não é um bug da prévia. Para saber qual é o caso:
  rode com o frame generation desligado (a captura deve acompanhar o FPS do
  overlay, até a taxa de atualização do monitor) e compare com PresentMon
  (`PresentMon --process_name jogo.exe`, coluna `FrameTime`/`DisplayedTime`)
  ou com o overlay do próprio gerador. Em stream/gravação, o que a WGC não vê não
  pode ser transmitido: use a taxa de captura como teto de FPS entregue.
- **Barra de tarefas sobre o jogo.** A prévia nunca é `WS_EX_TOPMOST` (o log
  inicial confirma `topmost=false`), não usa AppBar nem força a barra a aparecer.
  O Windows só esconde a barra quando a janela em **primeiro plano** cobre o
  monitor inteiro: com o foco na prévia (outro monitor) ou em qualquer outra
  janela, a barra do monitor do jogo continua visível. A captura WGC em si não
  muda o modo fullscreen/flip do jogo; porém um jogo em "tela cheia" que na
  verdade é janela sem borda, ou que não cobre exatamente o monitor, também
  mantém a barra. Para diagnosticar, rode com `--diagnostics`: a cada 2 s o log
  mostra a janela em primeiro plano (título, classe, retângulo, se cobre o
  monitor, topmost e o estado `QUNS` do Windows). Dê foco ao jogo e veja se
  `cobre_monitor=true` e `QUNS=3`/`2`; se for `false`, o jogo não está em tela
  cheia de verdade (configure o jogo para tela cheia/resolução nativa).
  Contornos: clicar no jogo antes de olhar a barra; habilitar "ocultar
  automaticamente a barra de tarefas" nas configurações do Windows; ou fechar a
  prévia e verificar se a barra some só com o jogo (isso isola nossa influência).
- Conteúdo protegido por DRM pode aparecer preto; fullscreen exclusivo real
  pode não ser capturado por WGC. Não há fallback DDA nesta entrega.
- Pipeline SDR BGRA8: não há tone mapping HDR → SDR; desligue HDR na validação
  inicial para evitar cores/luminância incorretas.
- Capturar o próprio monitor que mostra a prévia pode produzir efeito de espelho;
  use outro monitor ou uma janela separada para as medições.

## Checklist manual no Windows com GPU

Registrar build do Windows, driver, GPU(s), monitor/Hz, resolução, HDR e HAGS.

- [ ] Abrir o executável, conferir console e instrução **Ctrl+O para escolher**.
- [ ] Abrir o seletor pelo botão e pelo atalho; cancelar sem travamento e sem
      perder a fonte anterior; repetir escolhas para verificar a troca de fonte.
- [ ] Escolher um monitor; confirmar imagem, nome, resolução e FPS com movimento.
- [ ] Escolher uma janela; mover e redimensionar a janela capturada, inclusive
      rapidamente, entre monitores/DPI diferentes; verificar atualização sem
      regiões antigas/indefinidas e sem erro de dispositivo.
- [ ] Redimensionar, minimizar e restaurar a prévia; conferir letterbox e ausência
      de loop de apresentação quando não há atualização.
- [ ] Fechar a janela capturada; confirmar aviso, retorno ao estado sem fonte e
      possibilidade de selecionar outra janela/monitor.
- [ ] Alternar **C** e conferir o cursor na imagem e no título.
- [ ] Alternar **B**, aceitar/negar permissão quando oferecida, conferir log e
      observar a borda real; testar também `--border` em Windows 10 e 11.
- [ ] Parar com **Esc** e fechar o app, inclusive com o seletor aberto; verificar
      que nenhuma sessão/borda permanece após o encerramento.
- [ ] Comparar FPS de captura com FPS do jogo via PresentMon, com movimento e
      fonte ativa, em DX11, DX12 e Vulkan; registrar janela, borderless,
      fullscreen exclusivo e monitor a 180 Hz. Comparar também apresentado FPS
      com o refresh do monitor da prévia.
- [ ] Conferir engines GPU (3D/Copy), CPU e PCIe no Task Manager/ETW e correlacionar
      com PresentMon; revisar a ausência de `Map`/staging/readback no caminho.
      Contadores de Task Manager/PresentMon isolados não provam ausência de
      readback; usar uma captura gráfica/ETW se houver indício de transferência.
- [ ] Em máquina híbrida, registrar adapter do jogo e do processo, testar captura
      de cada monitor e comparar overhead quando eles estiverem em GPUs distintas.
- [ ] Se houver ambiente de teste para reset/perda de device, confirmar aviso sem
      panic e reinício por **Ctrl+O**; testar também suspensão/retomada e desconexão
      de monitor. Não provocar reset durante uma partida importante.
- [ ] Para overhead, seguir §12 do brief: aquecimento de 60 s, cinco corridas de
      três minutos por condição, ordem randomizada, baseline vs. prévia,
      jogo limitado ao refresh e GPU-bound sem limite. Salvar números, perda%
      e IC t de Student em `docs/bench/` antes de declarar aceite.

**Estado da validação:** os testes portáveis e a compilação/análise estática dos
caminhos Windows são executáveis em Linux; imagem, permissões, device reset e
impacto de FPS exigem Windows com GPU. Não há números antes/depois medidos nesta
máquina Linux. Este adiantamento não fecha M0.1 nem altera o decision gate WGC/DDA.

Referências de API: [bindings `windows` 0.62.2 no docs.rs](https://docs.rs/crate/windows/0.62.2/source/src/Windows/Graphics/Capture/mod.rs),
[captura e vida útil dos frames](https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture),
[consentimento para borda](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired),
[`MinUpdateInterval`](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.minupdateinterval).
