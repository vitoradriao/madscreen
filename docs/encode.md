# Captura e encoders

## Adaptação de taxa

O padrão do sidecar é `--rate-policy absolute-ramp`: piso absoluto de 1 Mbps
(ou `--min-bitrate` explícito), margem de 20% sobre o orçamento WebRTC, quedas
imediatas no NVENC e recuperação de até 15% por segundo. FFmpeg reabre a sessão
no máximo uma vez por segundo. Feedback novo substitui pedidos pendentes quando
o encoder está ocupado. A histerese preserva convergência exata ao piso/teto.

Retorno: `--rate-policy legacy` ou `MADSCREEN_RATE_POLICY=legacy` antes de abrir
o sidecar conserva o piso anterior de metade do teto. Não altera o app instalado.
Comparação NVENC/x264 no relay local e limites em
bloco de produto; não é qualificação do jogo,
AMD/Intel, A/V ou rede WAN.

## Captura de monitor no Windows 10

O sidecar detecta a versão real com `RtlGetVersion` (Windows 11 também informa
major 10; a build 22000 distingue o cliente). No padrão `auto`, monitor selecionado
no Windows 10 1903+ usa Desktop Duplication, sem o indicador amarelo do WGC.
Janelas e monitores no Windows 11 usam WGC. Versão desconhecida preserva WGC.
O seletor nativo WinRT, usado no caminho legado `picker`, devolve item opaco,
sem HMONITOR; esse caminho continua WGC. O seletor de monitores do app fornece
o alvo explícito e usa a seleção automática.

Desktop Duplication compartilha o device D3D11 do monitor com a conversão/encoder;
mantém mailbox de um quadro, sem readback do desktop. Cursor separado (colorido,
monocromático ou masked color) e rotação são compostos na GPU. Trocas de forma do
cursor e tamanho do monitor podem realocar seus recursos. A captura consulta DXGI
sem bloqueio e espera em timer privado de alta resolução fora da API, evitando
reter o lock do device enquanto aguarda novo frame. Nenhuma resolução global de
timer, driver ou configuração do Windows é alterada.

Perda de acesso recria a duplicação no mesmo monitor, por até cinco segundos;
falha persistente encerra com erro de captura. Não troca de monitor/backend
silenciosamente. Recuperação por mudança real de modo, múltiplos monitores e
GPUs híbridas exigem ensaios próprios. Relatório JSON registra `capture_backend`;
o log informa o método efetivo. `capture_clock_offset` continua exclusivo do WGC.

```powershell
# Comparação local: não publica mídia; o app usa auto sem precisar desta opção.
./target/release/madscreen-encode-test.exe --source monitor:0 --capture-backend desktop-duplication --fps 60 --duration 15 --output NUL
./target/release/madscreen-encode-test.exe --source monitor:0 --capture-backend wgc --fps 60 --duration 15 --output NUL
# Retorno para WGC no sidecar IPC: iniciar o app com esta variável herdada.
$env:MADSCREEN_CAPTURE_BACKEND = 'wgc'
# Remover o override antes de validar o comportamento automático.
Remove-Item Env:MADSCREEN_CAPTURE_BACKEND
```

`tools/bench/capture-test-source.ps1` abre uma fonte animada que cobre o monitor
escolhido e encerra após `-Seconds`. Uso opt-in, somente laboratório local.
Resultados e limites: relatório #60.

## Transmissão com seleção automática (30/out/2026)

O engine agora usa `AutoEncoder`: NVENC, AMD AMF (`h264_amf`), Intel QuickSync
(`h264_qsv`) e CPU (`libx264`). O padrão `--encoder auto` prefere o fabricante
do adapter da captura, tenta os outros backends de hardware e deixa x264 por
último. A configuração de resolução continua disponível quando o probe NVENC
falha; usa limites conservadores e valida a configuração real na criação.

AMF/QSV/x264 são carregados do runtime FFmpeg 8.x (ABI avcodec 62 / avutil 60).
AMF usa `flags=+low_delay`, `forced_idr=1` e `async_depth=1`: sem o primeiro,
o FFmpeg 8.1.2 retém um quadro mesmo sem B-frames e o probe de um quadro expira,
acionando indevidamente o fallback para CPU. `forced_idr` garante IDR com SPS/PPS
no probe e nos pedidos de recuperação. Ver diagnóstico e validação.
Antes de selecioná-los, o engine abre uma sessão e codifica um frame NV12,
exigindo IDR Annex B com SPS e PPS. Falhas de DLL, hardware, sessão ou dimensões
levam à alternativa seguinte; a ausência completa informa todos os erros.
O codec oferecido ao transporte continua sendo H.264 High, sem re-encode no SFU.
HEVC/AV1 e o modo de entrada BGRA do spike continuam exclusivos do backend NVENC.

NVENC preserva texturas D3D11 registradas e zero-copy. O caminho de compatibilidade
faz cópia para staging, Map NV12 e entrega planos pré-alocados ao FFmpeg; AMF/QSV
fazem upload ao encoder de hardware. x264 usa ultrafast/zerolatency, até duas threads,
sem B-frames/lookahead. Os buffers da aplicação são reservados na inicialização;
backpressure conserva os slots e tenta novamente sem aumentar a fila.

Pedidos de keyframe do WebRTC chegam a todos os backends. Mudanças de bitrate/FPS
nos backends FFmpeg reabrem apenas a sessão de codec entre frames, sem recriar o
transporte, e produzem novo IDR com headers. Com frames em voo, retornam Busy e o
feedback é reaplicado pelo publisher. NVENC mantém reconfiguração dinâmica própria.
O relatório registra o encoder efetivo e declara o readback dos novos backends.

```powershell
./tools/encode/prepare-runtime.ps1 -BundleDestination target/release/encoder-runtime
./target/release/madscreen-encode-test.exe --probe
# Forçar um backend é opcional, útil para diagnóstico/bench; o app usa auto.
./target/release/madscreen-encode-test.exe --encoder x264 --source monitor:0 --fps 60 --height 720 --duration 10
```

Para testes unitários do runtime sem GPU, configure `MADSCREEN_ENCODER_RUNTIME`
com a pasta absoluta das DLLs e rode
`cargo test -p madscreen-encode runtime_cpu_fallback_keyframe_and_rate -- --ignored`.
O CI executa esse teste com as mesmas DLLs do instalador. Veja
[`app.md`](app.md) para requisitos e `bench/encoders-2026-10-30.md`
para validação e limites. O restante deste documento descreve o spike NVENC original.

O spike implementa captura/síntese → conversão/escala GPU → NVENC → arquivo, somente
vídeo, em Windows 11 x64 com GPU NVIDIA. A lógica de configuração, ABI, parsers,
writers, estatísticas e pacer é testada também em Linux. **M0.2 permanece aberto**:
compilação e testes sem GPU não comprovam encode 1080p144 com p95 ≤ 5 ms nem
impacto no jogo. Resultados reais devem ser anexados em `docs/bench/` conforme §12.

```text
WGC BGRA8 → mailbox GPU latest-wins → shader Y/UV ou VideoProcessor → anel NV12
                                                                    ↓
synthetic (pixel shader BGRA8) ───────────────→ conversão ─────→ NVENC D3D11
                                                                    ↓
                                       AU reutilizável → Annex B / AV01 IVF / NUL
```

`madscreen-encode` contém o backend e as partes portáveis. `madscreen-encode-test`
cria o pipeline numa thread CPU dedicada `ABOVE_NORMAL`. A prioridade GPU padrão
é `above-normal`, configurável; nunca solicita REALTIME nem altera power state.
Priorizar o stream pode reduzir FPS do jogo: medir esse custo faz parte do aceite. A única adição à captura é `WgcSource::start_on_device`:
o `FrameSource::start` existente recria o device; esta entrada preserva exatamente
o `GpuDevice` passado a `new`. O preview não foi alterado. A captura existente já
chama `ID3D11Multithread::SetMultithreadProtected(true)`.

## Teste com o jogo aberto

**Todo teste de desempenho deve manter o jogo aberto carregando a GPU.** O alvo
é sustentar 144 FPS de encode enquanto o jogo usa a GPU, não medir a placa ociosa.
Prefira `--source monitor:N` no monitor do jogo para o caso realista. Use
`--source synthetic` somente para isolar geração/conversão/encoder, também com o
jogo aberto. Mantenha a mesma cena/replay, resolução, HAGS e clocks nas comparações.
Não deixe o console cobrir o jogo/captura; registre também o FPS do jogo no harness.

Medição anterior: RTX 5060 Ti, driver 610.88, API máxima 13.1,
jogo de teste aberto com GPU 86–91%, synthetic 1080p144/H.264/P1/18 Mbps por 30 s:
93,4 FPS, 1517 intervalos pulados, encode p50/p95 1,04/1,27 ms, total p50/p95/p99
6,9/10,4/10,8 ms, encoder 8%, 13,6 Mbps. A espera serial de conversão somava-se
ao encode. **Ainda não há números reais depois desta alteração**; sobreposição,
prioridade e throughput precisam de medição, não são garantias de 144 FPS.

Após compilar release, execute estes comandos na mesma cena, todos com o jogo aberto:

```powershell
# Serial antigo para comparação: inflight 1 força fence bloqueante + coleta bloqueante.
.\target\release\madscreen-encode-test.exe --source synthetic --size 1920x1080 --fps 144 --codec h264 --preset p1 --bitrate 18 --duration 30 --inflight 1 --gpu-priority normal --report serial.json --frames-csv serial.csv
# Isolar o ganho da sobreposição, com a mesma prioridade.
.\target\release\madscreen-encode-test.exe --source synthetic --size 1920x1080 --fps 144 --codec h264 --preset p1 --bitrate 18 --duration 30 --inflight 3 --sync flush --gpu-priority normal --report inflight3.json --frames-csv inflight3.csv
# Comparar sincronização, gravar e decodificar os dois arquivos completos.
.\target\release\madscreen-encode-test.exe --source monitor:0 --size 1920x1080 --fps 144 --bitrate 18 --duration 30 --inflight 3 --sync flush --gpu-priority above-normal --output flush.h264 --report flush.json --frames-csv flush.csv
.\target\release\madscreen-encode-test.exe --source monitor:0 --size 1920x1080 --fps 144 --bitrate 18 --duration 30 --inflight 3 --sync fence --gpu-priority above-normal --output fence.h264 --report fence.json --frames-csv fence.csv
# Prioridade: repetir também normal/above-normal, medir o FPS do jogo no harness.
.\target\release\madscreen-encode-test.exe --source monitor:0 --size 1920x1080 --fps 144 --bitrate 18 --duration 30 --inflight 3 --sync flush --gpu-priority high --report high.json
# Três caminhos de conversão, aquecimento individual e tabela comparativa.
.\target\release\madscreen-encode-test.exe --source monitor:0 --size 1920x1080 --fps 144 --bitrate 18 --duration 30 --compare-convert --output convert.h264 --report convert.json --frames-csv convert.csv
# Tirar o draw sintético da medição; anel pré-renderizado antes do aquecimento.
.\target\release\madscreen-encode-test.exe --source synthetic --synthetic static --size 1920x1080 --fps 144 --bitrate 18 --duration 30 --report static.json
```

`--inflight N`: padrão 3, faixa 1–15. O anel tem `max(4, N+1)` slots; cada slot
retém textura, mapeamento, bitstream, evento, fence e queries até sua conclusão.
`1` reproduz a sequência serial com fence + lock bloqueantes, mesmo com `--sync flush`;
o relatório registra `serial/fence`. Nos demais valores não há espera explícita
pela GPU no loop. Chamadas do driver ainda podem consumir CPU/bloquear internamente;
`queue_wait` e `encode_call` ajudam a identificar esse custo no hardware.
`--synthetic animated|static` usa animated por padrão. Static pré-renderiza todas
as entradas antes da medição; o conteúdo é constante, então CBR pode produzir
menos bits mesmo mantendo o FPS. `gen_gpu` fica indisponível nesse caso e em WGC.

`--sync flush` é padrão: Flush e entrega direta do recurso D3D11 registrado ao
NVENC, dependendo da **ordenação implícita do driver**. Flush não prova conclusão.
Se o arquivo apresentar corrupção visual, use **`--sync fence`**: cada slot aguarda
por polling de `GetCompletedValue`, sem esperar evento no caminho quente. Somente
a drenagem final e a comparação serial usam espera bloqueante de fence.

`--gpu-priority normal|above-normal|high`: DXGI 0/+3/+7 e classe de processo
NORMAL=2/ABOVE_NORMAL=3/HIGH=4 via `gdi32.dll`. O padrão é above-normal. Console/JSON
registram separadamente aplicado/negado/erro para cada chamada; falhas viram aviso,
sem abortar encode. Rodar como administrador pode ser necessário para elevar a
prioridade. **Isso troca FPS do jogo por frames do stream**; compare no harness.
REALTIME=5 nunca é utilizado, pelos riscos NVIDIA + HAGS descritos no brief.

Se a GPU selecionada tiver utilização média abaixo de 50%, console/JSON emitem
`AVISO: GPU media X % — medicao sem o jogo carregando a GPU nao e representativa`.
Ausência de telemetria não é utilização zero: registre e confira a carga manualmente.

## Compilar e executar

Requisitos: Rust stable, toolchain MSVC x64, Visual Studio Build Tools C++ e Windows
SDK, driver NVIDIA compatível com NVENC API **12.2**, sessão gráfica interativa e
D3D11.5/fences (Windows 11 com driver recente). HLSL embutido é compilado em runtime
por `D3DCompile`/`d3dcompiler_47.dll`. Build normal não precisa de SDK NVIDIA,
CUDA Toolkit, libclang ou bindgen. No Linux o binário informa a exigência de Windows
e sai com 1; `cargo check/clippy --target x86_64-pc-windows-msvc` verificam o código
Windows, sem executar nem produzir medições de GPU.

```powershell
cargo build --release -p madscreen-encode-test
cargo run -p madscreen-encode-test --release -- --probe
cargo run -p madscreen-encode-test --release -- --probe --source monitor:0 --report probe.json

# Com o jogo aberto: encoder isolado de WGC; 60 s após 5 s de aquecimento.
cargo run -p madscreen-encode-test --release -- --source synthetic --size 1920x1080 --fps 144 --codec h264 --preset p1 --bitrate 18 --warmup 5 --duration 60 --output out.h264 --report encode.json --frames-csv frames.csv

cargo run -p madscreen-encode-test --release -- --source monitor:0 --size 1920x1080 --fps 144 --bitrate 18 --duration 60 --output monitor.h264
cargo run -p madscreen-encode-test --release -- --source 'window:Trecho exclusivo do título' --fps 60 --bitrate 12 --duration 60 --output janela.h264

cargo run -p madscreen-encode-test --release -- --source synthetic --size 1920x1080 --fps 144 --bitrate 18 --compare-presets --duration 30 --output comparacao.h264 --frames-csv comparacao.csv --report presets.json

cargo run -p madscreen-encode-test --release -- --source synthetic --size 2560x1440 --fps 144 --codec av1 --bitrate 25 --duration 60 --output out.ivf
cargo run -p madscreen-encode-test --release -- --source monitor:0 --size 2560x1440 --fps 144 --codec av1 --fallback-hevc --bitrate 25 --duration 0 --warmup 0 --output NUL --stats-interval 5

# Forçar VP ou diagnosticar conversão interna ARGB/CUDA:
cargo run -p madscreen-encode-test --release -- --convert vp --duration 30 --output vp.h264
cargo run -p madscreen-encode-test --release -- --input bgra --duration 30 --output argb.h264
```

`monitor:N` é índice **base zero** de `EnumDisplayMonitors`, não o número exibido
pelo painel do Windows. Confira o adapter/LUID com `--probe --source monitor:N`
e a imagem em gravação curta. Janelas são enumeradas por substring, sem distinguir
maiúsculas; título ambíguo gera erro. O tamanho padrão é o conteúdo WGC (dimensões
ímpares arredondadas para baixo para NV12); synthetic usa 1920×1080. `--size`
define a saída e, no synthetic, o tamanho do padrão. Mudanças de tamanho WGC
recriam apenas recursos que dependem da entrada; a saída continua fixa.

`--fps` aceita inteiro ou racional, como `60000/1001`. `--bitrate` usa Mbps decimais.
`--duration` conta segundos **após** `--warmup`; zero continua até Ctrl+C. `--output`
ausente, vazio ou `NUL` descarta bytes, mantendo encode/lock e medições ativos.
`--compare-presets` exige duração positiva, executa P1–P4 com aquecimento individual
e acrescenta `-p1`…`-p4` aos nomes de bitstream/CSV. JSON contém um array dos perfis.
`--compare-convert` exige duração positiva, é incompatível com `--compare-presets`
e executa shader/NV12, VP/NV12 e shader de escala/BGRA em sequência. Acrescenta
`-shader`, `-vp`, `-bgra` aos arquivos; JSON mantém todos os perfis, incluindo modo
efetivo (o shader NV12 ainda pode cair em VP). A tabela inclui `convert_gpu`,
`queue_wait` e total p95. `--multipass quarter-res` habilita a alternativa de duas passagens em 1/4 da resolução;
padrão `disabled`. Intra-refresh: `--intra-refresh-period 144 --intra-refresh-count 8`
(exige suporte do codec; `0 < count < period`).

Ctrl+C termina o loop, para WGC, envia EOS, libera os recursos NVENC e salva JSON/CSV.
`Stop-Process` no Windows é término forçado: o SO/driver recuperam os recursos,
mas não há garantia de flush de arquivos nem relatório final. O harness usa NUL
e monitora a existência do processo; para preservar relatórios use Ctrl+C.
`--probe` lista adapters/LUID, API máxima, codecs/limites/async/intra-refresh e driver
via NVML/nvidia-smi. Ausência de DLL/GPU/sessão NVENC imprime status claro e retorna
0; falha de escrita de um `--report` explicitamente solicitado continua sendo erro.

## Sidecar do app

No Windows, `madscreen-encode-test --ipc` recebe comandos de
`madscreen-ipc` pelo stdin e escreve somente eventos JSON em linhas no stdout;
logs e mensagens do pipeline vão para stderr. Após `ready`, envie uma linha:

```json
{"cmd":"start","source":{"kind":"picker"},"quality":{"height":1080,"fps":60},"bitrate_bps":null,"livekit":null}
```

Mantenha o stdin aberto durante a sessão. `{"cmd":"stop"}` encerra com
`stopped/requested`; EOF encerra com `stopped/app_gone`; fechamento da fonte
gera `stopped/source_closed`. O seletor anuncia `picking`; cancelá-lo gera
`pick_cancelled` e saída 0. A CLI também aceita `--source picker`.

Os perfis IPC são Nativa/720p/1080p/1440p/2160p (`quality.height` 0/720/1080/1440/2160)
a 30/60 FPS, sem warmup nem duração máxima. A saída é calculada no engine a partir da
fonte real e dos limites do NVENC (ver "Resolução, ultrawide e bitrate automático").
Qualidade ou configuração inválida gera `error/invalid_request` e saída 1.
Sem `livekit`, a saída é descartada; publicar exige a feature `publish` e um
`LiveKitTarget`. `started` informa a configuração efetiva do encoder e o estado
de publicação. `status` usa diferenças dos contadores de frames/bytes entre
amostras de aproximadamente 1 s, inclusive quando nenhum frame é produzido;
`frames` e `dropped` são contagens acumuladas da sessão. Em WGC, os descartes
contam sobrescrita da mailbox e contenção, sem contar slots ainda em voo.

## Resolução, ultrawide e bitrate automático

`crates/madscreen-ipc/src/resolution.rs` concentra a regra (lógica pura, usada pelo
engine e pelo app):

- A altura escolhida (720/1080/1440/2160) ou a nativa define a saída; a largura sai da
  proporção da fonte, arredondada ao par mais próximo (NV12 4:2:0). Nunca há upscale:
  alturas maiores que a da fonte ficam indisponíveis. Ex.: 2560×1080 → 1080p 2560×1080;
  3440×1440 → 1440p 3440×1440; 3840×1080; 5120×1440 (32:9).
- Limites: `NvEncGetEncodeCaps` (largura/altura máx. e mín.) por GPU/codec em runtime, mais o
  nível 5.2 do H.264 (MaxFS/MaxMBPS). Se a saída não couber, a altura cai de 2 em 2 mantendo
  a proporção (ex.: 5120×1440 a "1440p" vira 4096×1152 porque o H.264 limita a largura a 4096)
  e `started.notice` avisa a UI. O publicador só aceita H.264; HEVC/AV1 não são usados como
  alternativa por enquanto.
- Bitrate automático (quando `bitrate_bps` é nulo): 14 Mbps a 1920×1080@60, proporcional a
  pixels × fps, de 2 a 40 Mbps.
- Level H.264 explícito (`EncoderConfig.h264_level`): o menor nível de 3.1 a 5.2
  que comporta MBs por frame e por segundo (1080p30 → 4.0; 1080p60 → 4.2; 1440p30 e
  2560×1080@30 → 5.0; 1440p60 e 2160p30 → 5.1; 2160p60 → 5.2). NVENC, AMF, QuickSync e
  x264 recebem o mesmo `level_idc` (AMF/QuickSync como inteiro `40`/`42`; x264 como `4.0`/`4.2`).
  `started.profile_level_id` é o High sem constraints (`6400LL`) gravado no SPS.
  No SDP do Cloudflare, sem `--cf-profile-level-id`, o perfil que o libwebrtc ofereceu
  (`42e0`, `4d00`, `640c`, …) é mantido e só o nível sobe (`42e01f` em 1080p30 vira `42e028`).
  Trocar o perfil inteiro por `6400LL` faz o libwebrtc rejeitar a resposta do SFU.
  Essa alteração de nível ocorre apenas no SDP: `set_codec_preferences` recebe as
  capacidades nativas intactas, somente filtradas/reordenadas. Alterar o fmtp nessa
  lista provocava "restringindo codecs do transceiver" já no nível 4.0, antes de
  publicar no Worker. A regressão usa dois PeerConnections reais e verifica ofertas e
  respostas nos níveis 3.1/4.0/4.2/5.0/5.1/5.2, sem depender de GPU ou SFU remoto.
- A conversão na GPU já escala para qualquer proporção (`fit_rect`): proporção igual
  preenche o quadro; se a janela mudar de proporção, o conteúdo ganha barras.
- CLI: `--height native|720|1080|1440|2160` (não combina com `--size`).
  `--list-sources --monitors-only` lista só telas e o campo `encoder` com os limites do H.264.

## Decisões do pipeline

- **Anel configurável:** texturas D3D11 `USAGE_DEFAULT`, `CPUAccessFlags=0`,
  registradas uma vez por sessão, com bitstream e evento por slot. FIFO de outputs
  aceitos pelo NVENC, com limite explícito, sem crescimento. Slot ocupado nunca é
  reutilizado. `MapInputResource`/`UnmapInputResource` são handles NVENC, não leitura
  de pixels. `pitch=0` para DIRECTX: o driver conhece o layout opaco.
- **Shader padrão:** RTVs explícitas por plano (`CreateRenderTargetView1`,
  Y=R8_UNORM, UV=R8G8_UNORM), duas passagens de triângulo, amostragem bilinear,
  crominância média 2×2, BT.709 full RGB → limited YCbCr. A área útil/letterbox
  é alinhada a dois pixels; barras usam Y=16/255, UV=128/255. Não há tone mapping
  HDR; validar inicialmente com HDR desligado, conforme a limitação da captura.
- **Fallback VP:** se criar RTVs de plano falhar, seleciona automaticamente
  `ID3D11VideoProcessor::VideoProcessorBlt` e registra o modo efetivo. `--convert vp`
  escolhe essa alternativa diretamente. VP progressivo, sem auto-processing ou
  frames passados/futuros, RGB full de entrada e YCbCr BT.709 limited de saída.
  Views do anel são criadas na inicialização; views de entrada são cacheadas por
  recurso (até 64 recursos distintos por tamanho), inclusive para o anel sintético,
  com comandos submetidos apenas durante o lease WGC. Recriação de
  recurso/tamanho é exceção à cadência estável. Cor/escala no driver precisam de
  comparação visual/numérica no hardware.
- **ARGB diagnóstico:** `--input bgra` mantém um anel BGRA, faz escala/letterbox
  GPU e registra as texturas como `NV_ENC_BUFFER_FORMAT_ARGB` (layout BGRA em
  little-endian). NVENC converte internamente usando CUDA, emitindo aviso.
  Não usar como prova do caminho NV12 padrão; `--convert` é substituído pelo
  shader BGRA nesse modo. Essa escolha preserva registros persistentes mesmo
  quando a fonte WGC troca de textura.
- **Sincronização:** Flush sempre após os passes; fence por slot para consulta e
  drenagem, sem `WaitForSingleObject` no modo sobreposto. Em fence, um slot ainda
  incompleto continua em voo, consultado nas próximas iterações. Não há readback.
- **Saída assíncrona:** `enableEncodeAsync=1`, eventos registrados por bitstream,
  consulta do evento com timeout zero e `LockBitstream(doNotWait=1)` para o output
  mais antigo. `LOCK_BUSY` conserva o slot e retorna “ainda não pronto”.
  `poll_into` conserva a variante bloqueante; `try_poll_into` estende a API sem
  mudar a assinatura existente. Na parada: parar captura, concluir conversões,
  enviar EOS, drenar outputs e queries, Unlock/Unmap/Unregister/Destroy. Sem suporte
  a eventos async, a inicialização falha claramente. Não há B frames/lookahead.
  `NEED_MORE_INPUT` continua erro explícito nesta configuração sem reorder.
- **Pacer sintético:** `CreateWaitableTimerExW(CREATE_WAITABLE_TIMER_HIGH_RESOLUTION)`
  com deadlines absolutos racionais; nenhuma recuperação em rajadas. Trabalho
  pendente é consultado aproximadamente a cada 0,5 ms entre deadlines, respeitando
  a granularidade real do Windows. Atrasos não geram espera duplicada de GPU.
- **Captura dirigida por chegada:** notify WGC sinaliza evento Win32 auto-reset.
  O loop espera esse evento combinado com o timer de alta resolução (~0,5 ms com
  trabalho pendente; timeout comum até 100 ms ocioso), respeita intervalo mínimo `0,9/fps`, consulta apenas `latest_frame(false)`
  e nunca repete frame sem atualização. A mailbox conserva apenas o mais recente;
  extras são descartados por sobrescrita/contenção. O limite WGC fica no padrão
  mínimo de 1 ms; o limite do encode é aplicado no consumidor. WGC estático pode
  produzir menos atualizações que o alvo. No sintético o índice avança na submissão.
- **Memória:** texturas, buffers de bitstream, AU e armazenamento de métricas
  são reservados antes do loop. `poll_into(&mut AuBuffer)` não aumenta capacidade;
  AU grande demais gera `BufferTooSmall`, após liberar o lock. Não há alocação
  por frame na cadência estável. Estatísticas periódicas, telemetria, inicialização,
  resize e caminhos de erro podem alocar. Gravação em disco pode afetar pacing;
  usar NUL para medir overhead do pipeline.

Parâmetros sobre a **cópia do preset retornado pelo driver**, nunca sobre um
`NV_ENC_CONFIG` zerado em substituição ao preset:

| Parâmetro | Valor |
|---|---|
| Tuning / preset | ULTRA_LOW_LATENCY / P1 padrão, P1…P7 selecionáveis |
| RC / average/max bitrate | CBR / bitrate solicitado |
| VBV / initial delay | `floor(bitrate_bps × fps.den × vbv_frames / fps.num)` bits, padrão um frame |
| GOP / idrPeriod | infinito |
| frameIntervalP / zeroReorderDelay | 1 / 1 |
| Lookahead / external lookahead / AQ espacial e temporal | desativados |
| multiPass / lowDelayKeyFrameScale | disabled / 1 |
| Perfis | H.264 High; HEVC Main 8-bit; AV1 Main 8-bit, 4:2:0 |
| Sequence headers | SPS/PPS (HEVC também VPS) repetidos em cada IDR; AV1 repeatSeqHdr |
| Cores | BT.709 limited explicitamente sinalizado no bitstream |
| Intra-refresh | opcional, validado contra caps |

`reconfigure_rate` conserva a sessão e usa `resetEncoder=0`/`forceIDR=0`; a aplicação
que mudar FPS também deve atualizar seu pacer. `request_keyframe` agrupa pedidos
e força IDR + OUTPUT_SPSPPS no próximo frame permitido, no máximo um por segundo
após o primeiro. Não há entrada de rede/PLI no executável deste spike.

## Relatórios e arquivos

CSV inclui somente frames submetidos após aquecimento (inclusive outputs obtidos
na drenagem), índices global/medido, QPC bruto da captura, bytes, key e tempos por
frame. JSON/console calculam p50/p95/p99/max de todos os campos abaixo. Os nomes
antigos `t_capture_convert_ms`, `t_convert_ms`, `t_encode_ms`, `t_total_ms` no CSV e
`capture_convert_ms`, `convert_ms`, `encode_ms`, `total_ms` no JSON permanecem como
aliases; `convert_ms` agora mede somente submissão, não a espera da fence.

| Campo | Significado |
|---|---|
| `gen` | CPU do draw sintético; em WGC, QPC de captura → início da conversão (idade do frame) |
| `convert_submit` | CPU da submissão dos passes, timestamps, Signal e Flush |
| `gen_gpu` | Intervalo GPU do draw via queries TIMESTAMP dentro de TIMESTAMP_DISJOINT; ausente em WGC/static |
| `convert_gpu` | Intervalo GPU dos passes de conversão/escala via as mesmas queries |
| `queue_wait` | Fim do submit → envio ao NVENC; em fence inclui fila atrás do jogo e atraso do polling; em flush só despacho/Map, pois NVENC recebe antes da conclusão da GPU |
| `encode_call` | QPC imediatamente antes/depois de NvEncEncodePicture |
| `encode_wait` / `encode` | Envio ao NVENC → bitstream observado pronto; inclui encode_call, fila de conversão implícita em flush, fila do encoder e atraso de observação do polling |
| `total` | Captura/início da geração → bitstream observado pronto; sem rede/decode/apresentação |

Queries são reservadas por slot, lidas algumas iterações depois com
`GetData(DONOTFLUSH)`: S_FALSE conserva a amostra pendente; `disjoint`/frequência
zero descarta os tempos GPU, sem inventar zero. O slot só é reciclado após coletar
os timestamps. São leituras de contadores, **não de textura**. Timestamps medem o
intervalo na linha de tempo GPU, que pode incluir preempção pelo jogo/intercalação
de comandos do contexto compartilhado; não são tempo CPU nem estimativa por fence.
O fim do bitstream é observado por polling, sem timestamp de conclusão NVENC;
um atraso de escalonamento do processo também entra no encode_wait/total.

**FPS de encode e FPS sustentado usam frames medidos/duração medida**, incluindo
a drenagem final. A redundância é explícita para comparar taxa e duração; nenhum
campo extrapola uma taxa a partir de um encode isolado. Reporta limite/média temporal/
máximo de slots em voo (inclui consultas GPU pendentes), frames sintéticos aceitos
após seu deadline, oportunidades sintéticas perdidas por anel cheio e intervalos
pulados. Estes últimos são deadlines omitidos pelo pacer ao avançar até o futuro,
não bitstreams descartados nem frames recuperados em rajada. Em WGC o pacer não é
usado: descartes são capturados menos codificados, incluindo contenção, sobrescrita
e frame final não consumido. `capture_throttled_notifications` conta notificações
retidas pelo intervalo mínimo, não estima quantidade de frames a partir de eventos
coalescidos. Total de descartes continua disponível separadamente.

A linha de latência continua **encode p95 ≤ 5 ms** em 1920×1080/144 FPS; a linha
independente de **throughput é PASS se FPS de encode ≥ 97% do pedido** (139,68 FPS
para 144). Sem frames: `SEM AMOSTRAS`. Outros tamanhos/cadências: latência `N/A`,
throughput ainda comparado ao pedido. PASS nas duas linhas não fecha M0.2 sem bench.
A sessão assíncrona pode incluir espera implícita de conversão no encode_wait em
flush: compare também fence para separar os custos, sem subtrair tempos estimados.

Bitrate alvo e real são reportados juntos. Se há menos frames que o FPS configurado,
o relatório explica que o orçamento de bits por frame usa a cadência configurada e
a produção por segundo cai; isso não prova erro do controle CBR. Conteúdo estático
ou pouco complexo também pode ficar abaixo do alvo. Bitrate p95 usa janelas de pelo
menos um segundo; não há estimativa antes da primeira janela completa.

Processo contínuo mantém as últimas 262144 amostras por distribuição, as últimas
3600 janelas de bitrate/telemetria e as últimas 4096 latências nas linhas `stats`.
Contagens, bytes e ocupação temporal acumulam; JSON declara a janela estatística.
Percentis usam interpolação linear da biblioteca de telemetria.

NVML/nvidia-smi são amostrados aproximadamente uma vez por segundo em thread
normal, fora do caminho de frames. JSON preserva snapshots de todas as GPUs com
nome/índice, clocks, temperatura, driver e P-state quando disponíveis. As estatísticas
`gpu_percent_mean/min/max` e `encoder_percent_mean` usam amostras após aquecimento,
antes da drenagem, cujo nome corresponde unicamente ao adapter selecionado. Nomes
ambíguos/diferentes ou contadores ausentes ficam null, com mensagem no console;
confira LUID/adapter e snapshots em máquinas com várias GPUs iguais. As médias/mínimos/máximos de
utilização acumulam toda a medição mesmo após reciclar os 3600 snapshots; não são
contadores exclusivos do processo. O fallback nvidia-smi pode alongar a cadência. Medição sem jogo carregando
a GPU não é representativa, mesmo se os gates de latência/throughput passarem.

Para a semântica de eventos/doNotWait, consulte o
[guia NVIDIA](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.1/nvenc-video-encoder-api-prog-guide/index.html#modes-of-operation);
para S_OK/S_FALSE das queries, a
[documentação GetData](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-getdata).
Essas APIs ainda exigem validação no driver/GPU usados, inclusive fallback VP e
ARGB: BGRA evita os passes de conversão Y/UV, mas conserva o shader de escala e
faz o NVENC converter internamente com CUDA, concorrendo com o jogo. VP pode usar
motor de vídeo/3D conforme o driver; compare engines e os custos medidos sob carga.

H.264/HEVC são Annex B com headers em keyframes; AV1 usa OBUs low-overhead dentro
de IVF AV01, timestamps por índice e timebase racional. Header IVF usa contagem
zero (arquivo contínuo sem Seek); decodificadores percorrem os registros de frame.
O arquivo inclui o aquecimento para preservar o primeiro keyframe e ser reproduzível.
A fonte synthetic é determinística em pixels/índice; **NVENC não oferece aqui
garantia de bit-exatidão entre drivers, GPUs ou presets**. Compare hash somente
com ambiente idêntico e verifique frames decodificados; duração em segundos pode
produzir contagens diferentes por escalonamento. Os timestamps do arquivo são
índices uniformes; CSV conserva os tempos reais e os atrasos.

```powershell
ffplay -f h264 out.h264
ffprobe -f h264 -show_streams -count_frames out.h264
ffmpeg -i out.h264 -f null -
ffplay -f hevc out.hevc
ffprobe -show_streams -count_frames out.ivf
ffmpeg -i out.ivf -f null -
```

Bitstreams elementares podem não sinalizar a cadência que o player escolhe;
forneça `-framerate 144` na entrada quando necessário. Verifique resolução,
perfil, 8-bit, cores BT.709 limited, contagem, headers e erros de decode.

## ABI e testes sem GPU

Bindings incluídos em `src/nvenc/nvenc_sys.rs`: header MIT de
[FFmpeg/nv-codec-headers n12.2.72.0](https://github.com/FFmpeg/nv-codec-headers/tree/n12.2.72.0),
`include/ffnvcodec/nvEncodeAPI.h`, SHA-256
`4677a397e3ec5300a6b38bf49cba42bb63a922ab26f24bc63a05ed08857cba16`.
Licença integral em `crates/madscreen-encode/NOTICE`.
Foram gerados uma vez com bindgen 0.73.2/libclang 18, alvo
`x86_64-pc-windows-msvc`, allowlists `NV_*`, `_NV*`, GUID, constantes NV*,
`NvEncode*`, derive Default, sem doc comments e com asserts de layout.
Para resolver apenas tipos Win32 usados pelo header, o include de geração
forneceu `GUID { u32, u16, u16, u8[8] }` e RECT de quatro `long` MSVC; nenhum
header de implementação do driver foi necessário. GUIDs `static const` foram
materializados como constantes literais do próprio header (bindgen emite extern
para esses inicializadores C). Macros de função/versão estão em `versions.rs`,
com testes dos valores hexadecimais esperados, inclusive o bit 31.

DLL é carregada em runtime; `NvEncodeAPIGetMaxSupportedVersion` usa major<<4|minor,
diferente do campo `NVENCAPI_VERSION` das structs. Driver abaixo de 12.2 falha
claramente antes de CreateInstance. Os bindings não são regenerados no build;
nenhum símbolo NVIDIA exige link estático.

Testes portáveis cobrem configuração/VBV racional, GUIDs/presets, versões/layouts,
Annex B e SPS/PPS/VPS, OBU/keyframe/IVF, buffers sem crescimento, latências, pacer
com relógio injetado, CLI e JSON/CSV. Uma tabela de funções falsa executa a mesma
montagem de inicialização/reconfiguração usada pelo backend, verifica sequência,
campos reservados, preservação do preset e ausência de reset/IDR. Não simula
sucesso de GPU, textura, driver, captura nem limpeza COM.

## Checklist no PC NVIDIA — aceite ainda pendente

Registrar GPU (RTX 5060 Ti), versão do driver, build do Windows, monitor 179 Hz,
resolução, HDR, HAGS, VRR, plano de energia, LUID e clocks/temperatura/P-state.

- [ ] `--probe` com fonte/monitor correto; confirmar H.264/HEVC/AV1 e limites reais.
      Testar também máquina sem driver/DLL: mensagem EncoderUnavailable, probe exit 0.
- [ ] Synthetic 1080p144, P1, NV12: gravar, decodificar todo o arquivo sem erro,
      confirmar headers/cores, contador em movimento, **encode p95 ≤ 5 ms** e **FPS ≥ 139,68**, com o jogo carregando a GPU.
- [ ] Comparar P1–P4 com mesmos parâmetros; anexar JSON/CSV, FPS, latências,
      bitrate e uso de engines. Repetir com captura real e jogo ativo.
- [ ] Validar criação de RTV por plano NV12; conferir Y/UV, crominância, preto,
      escala bilinear e letterbox com barras/gradientes conhecidos. Comparar com VP.
      Se houver fallback, registrar o modo efetivo e não atribuir números ao shader.
- [ ] Comparar inflight 1/3, sync flush/fence, prioridades normal/above-normal/high e conversões com o jogo aberto. Validar eventos async, LOCK_BUSY e queries/disjoint.
- [ ] Validar fence/Flush sob carga, resize, monitor/janela, HAGS ligado/desligado,
      suspensão/retomada e fechamento da fonte. Conferir ausência de corrupção,
      deadlock, timeouts, NEED_MORE_INPUT inesperado e recursos vazando ao repetir.
- [ ] Confirmar nenhum Map/staging de pixels no código e conferir PCIe/CPU/engines
      Video Encode/Copy/3D/Compute via Task Manager/ETW/GPUView/PresentMon.
      Contador de encoder sozinho não comprova zero-copy. A única leitura permitida
      é LockBitstream; comparar ARGB/CUDA diagnóstico sem confundir com NV12.
- [ ] AV1: suporte real, arquivo IVF/OBU decodificável e 8-bit; validar fallback
      HEVC em hardware sem AV1. Não inferir suporte somente pela geração comercial.
- [ ] Comparar GPU-priority normal (única nesta versão) com concorrência do jogo,
      clocks/P-state/temperatura; medir cold/warm e plano de energia. Não há pedido
      automático de high power mode/NVAPI nem opções high/REALTIME nesta entrega.
- [ ] Intra-refresh, reconfigure_rate sem IDR/reset e pedidos de keyframe limitados:
      validar bitstream real com aplicação que exerça a API da biblioteca.
- [ ] Monitor 179 Hz, FPS alvo 60/144 e jogo com/sem frame generation: WGC ≥ alvo
      quando há presents suficientes; observar descartes e zero fila sob saturação.
- [ ] Ctrl+C, término forçado e reinício repetido: driver libera sessão/texturas;
      relatório preservado apenas no encerramento cooperativo.
- [ ] Overhead no jogo: harness §12.1, três jogos DX11/DX12/Vulkan, S-A limitado e
      S-B GPU-bound, aquecimento 60 s, **5×180 s**, ordem randomizada; baseline
      estável, perda%, IC t de Student, engines/PCIe e controles térmicos anexados.
- [ ] Para glass-to-glass, adicionar transporte/viewer e câmera ≥240 FPS conforme
      §12.2; os números deste executável cobrem somente captura/conversão/encode.

Referências primárias: [header 12.2](https://github.com/FFmpeg/nv-codec-headers/blob/n12.2.72.0/include/ffnvcodec/nvEncodeAPI.h),
[guia NVENC](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html),
[CreateRenderTargetView](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11device-createrendertargetview),
[NVML — header](https://github.com/NVIDIA/go-nvml/blob/main/pkg/nvml/nvml.h).
