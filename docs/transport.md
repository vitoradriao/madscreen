# Transporte e publicadores

## Publicador Cloudflare (experimental)

O build com `--features publish` também inclui `madscreen-transport-cf`, que depende
diretamente de `libwebrtc = 0.3.55`, sem depender de `livekit`. O padrão continua
`--transport livekit`; `--livekit-url` mantém o comportamento anterior.

```powershell
$env:MADSCREEN_CF_URL = "https://seu-worker.workers.dev"
$env:MADSCREEN_CF_TOKEN = "<token de dono ou convite publisher>"
cargo build --release -p madscreen-encode-test --features publish
.\target\release\madscreen-encode-test.exe --source monitor:0 --fps 60 --bitrate 18 --duration 0 --warmup 0 --transport cloudflare --cf-keepalive
```

`MADSCREEN_TRANSPORT=cloudflare` substitui `--transport cloudflare`. URL/token também
aceitam `--cf-url`/`--cf-token`. A precedência é argumentos, ambiente e então
`$MADSCREEN_HOME/server.env` (ou `%APPDATA%\madscreen\server.env` quando
`MADSCREEN_HOME` não estiver definido):

```dotenv
MADSCREEN_CF_URL=https://seu-worker.workers.dev
MADSCREEN_CF_TOKEN=<token de dono ou convite publisher>
```

Não coloque o App Secret do SFU no host: essas credenciais ficam no Worker. O parser
aceita `CHAVE=VALOR`, aspas externas opcionais, comentários iniciados por `#` e linhas
vazias. O arquivo não altera o ambiente do processo.

A factory cria a faixa a partir de `NativeVideoSource::new_encoded`; a ordem é
`add_transceiver(sendonly)` → `sender.set_video_encoder_backend(PreEncoded)` →
`set_codec_preferences(H264 + RTX)` → oferta. Sem `--cf-profile-level-id`, cada
`profile-level-id` H.264 conserva o perfil do libwebrtc e só troca o nível pelo do
encoder (`42e01f` em 1080p30 vira `42e028`). `--cf-profile-level-id` ainda reescreve
o perfil inteiro, antes de aplicar a descrição local. O binding só expõe a descrição local *current*, não
a oferta *pending*: os candidatos do callback ICE são anexados à única seção de
vídeo da oferta assim que há candidato srflx/relay (+0,8 s), qualquer candidato (+3 s) ou coleta completa (erro em 8 s).

A sinalização segue o contrato v1 do Worker: GET `/api/ice`, POST
`/api/share/start`, heartbeat a cada 5 s e `/api/share/stop` no fechamento, sempre com
Bearer. Falha do PC, desconexão por 10 s, geração divergente/409 ou três falhas de
heartbeat recriam PC/fonte/sessão, com backoff de 1, 2, 4, 8 e no máximo 10 s. O
contador `reconexoes` mede tentativas e também aparece no relatório final. A captura
não espera a reconexão; AUs nesse intervalo são recusados e um novo IDR é solicitado
ao restabelecer a sessão. A coalescência de IDR de 0,5 s e o controle de bitrate NVENC
continuam no mesmo caminho dos dois transportes.

`--cf-keepalive` guarda uma cópia do último IDR (reutilizando o `Vec` entre keyframes)
e o reenvia com timestamp crescente a cada 10 s sem entregas (o SFU descarta a faixa após 30 s sem pacotes; a medição anterior de mais de 60 s usou intervalo de 5 s). Desligado, não há cópia
Rust de AU. `--cf-start-bitrate <Mbps>` define a estimativa inicial do GCC (padrão: metade de `--bitrate`; o padrão do libwebrtc começa em ~0,3 Mbps e levava ~10 s para chegar ao teto, com congelamentos na entrada do viewer).
`webrtc:` mantém os campos do LiveKit e acrescenta tentativas de
reconexão e AUs de keepalive. No modo IPC, os logs Cloudflare usam `tracing` em stderr.

Pedidos de keyframe são consultados em cada volta do loop, coalescidos e atendidos
com limitador de 0,5 s entre IDRs forçados, também respeitado pelo NVENC. Sem AU
por dois períodos de quadro (mínimo 50 ms), um pedido pendente recodifica o último quadro WGC; após 1 s de
repouso com deltas enviados desde o IDR, um único IDR de repouso atualiza o cache.
O keepalive de 10 s solicita novo IDR se precisar repetir um cache anterior aos
deltas. O publicador mantém um WebSocket em `/api/stream/ws?owner=<id>` usando o
ID do próprio usuário retornado por `GET /api/me` e o subprotocolo `ms.<token>`.
Pedidos `type=keyframe` chegam diretamente aos hosts e marcam a solicitação de IDR;
o canal reconecta com backoff de 1 a 10 s e fecha com o publicador. O heartbeat de
5 s continua como fallback se o WebSocket falhar. Após mais de 300 ms sem entrega
de AU (mesmo sem keepalive habilitado), uma tarefa independente consulta as stats
outbound do PeerConnection a cada 250 ms e converte aumentos de PLI/FIR em pedidos,
sem esperar um AU passar pelo encoder passthrough. Com AUs fluindo, essa consulta
de recuperação não roda.
Em repouso, PLI que chega até 4 s depois do último IDR é ignorado: o Chrome pede keyframe
3 s depois do último quadro enquanto recebeu pacote nos últimos 5 s, e responder a cada pedido
manteria uma cadeia de IDRs a cada 3 s com a tela parada.
O viewer amostra a recuperação a cada 250 ms, mantendo o `onStats` a cada 1 s:
pede keyframe após 1 s conectado/com faixa recebida sem o primeiro quadro,
quando há aumento de perda e pelo menos 300 ms sem avanço de decode, ou após
1 s de bytes/pacotes chegando sem decode. O intervalo mínimo é 1,5 s, inclusive
para o botão manual de atualizar; tela parada sem perda, com `packetsReceived`
parado e algum quadro já decodificado, não gera pedidos.
H.264 anuncia RTX e feedback NACK; o log da resposta SDP ignora espaços finais.
A medição real mostrou NACKs no Chrome sem retransmissão nem repasse ao publicador
pelo SFU, por isso a recuperação rápida usa também o canal do Worker. No TURN, o Worker
filtra URLs da porta 53, bloqueada pelos navegadores. Medições antes/depois e TURN real em
bench/keyframe-turn-cf.md. Para testar NAT restritivo,
use `$env:MADSCREEN_CF_FORCE_RELAY="1"` no publicador e
`localStorage.setItem("ms.forceRelay", "1")` no viewer antes de conectar; remova
essas opções depois. Os logs ICE mostram apenas os tipos host/srflx/relay, sem IPs.

Resultados medidos em Windows/NVENC (High pelo SFU até o Chrome, RTCP, rampa de bitrate,
keepalive, reconexão, latência vidro a vidro) estão em `docs/bench/cf-verificar.md`;
o teste com o jogo de teste aberto ainda falta. Alterar o perfil
no SDP não altera o SPS do NVENC; a interoperabilidade é um experimento (§4 de
`cloudflare-requisitos.md`). Não há alegação de paridade de desempenho nesta etapa.

Smoke local em Linux (29/10/2026, sinalização simulada + segundo PeerConnection):
oferta `42e01f`, AU H.264 High sintético, 14 entregas / 0 recusas em cerca de 14 s,
11 keepalives, duas sessões e dois stops após injetar 409; o receptor contabilizou
frames decodificados. No SFU real, `--cf-profile-level-id 640c1f` também falha (libwebrtc rejeita a resposta). Oferta `640c1f` foi rejeitada pelo receptor libwebrtc local
(m-line de vídeo com porta zero); isso não determina o suporte do SFU/Chrome. Nenhum
teste unitário foi adicionado. A box exigiu ferramentas temporárias Rust 1.90
(`zip 9.1.3` já no lock exige ≥1.88) e Clang 21 para os headers herméticos do WebRTC.

O bitstream H.264 do NVENC é publicado numa sala LiveKit **sem re-encode**: o crate
`madscreen-transport` usa `NativeVideoSource::new_encoded` + `VideoEncoderBackend::PreEncoded`
(livekit 0.10.0 / libwebrtc 0.3.55), uma camada, sem simulcast. Só vídeo.

## Peças

- `crates/madscreen-transport`: `LiveKitPublisher` (conecta, publica a faixa, entrega access
  units Annex B com SPS/PPS nos keyframes, expõe pedidos de keyframe e de taxa do WebRTC).
- `apps/madscreen-encode-test` com a feature `publish`: `--livekit-url ws://host:7880` troca a
  saída de arquivo pela sala. Pedidos de keyframe do WebRTC viram IDR (no máximo 1 a cada 0,5 s,
  senão cada reconfiguração de taxa gera um IDR) e o alvo de bitrate do WebRTC reconfigura o
  NVENC, limitado ao `--bitrate` pedido.
- O viewer estático e seu servidor local usados nessas corridas foram retirados na issue #5.
  O viewer atual está em `viewer/shared` e `viewer/web`, servido pelo Worker; instruções atuais em
  [cloudflare-auth.md](cloudflare-auth.md). O app único também permite assistir.
- Os scripts de relógio e resumo permanecem em `tools/bench/` para novas medições.
- Windows precisa de CRT estática (`.cargo/config.toml`) para linkar o libwebrtc pré-compilado.

## Ambiente histórico das medições

A configuração abaixo registra o ambiente LiveKit usado nas corridas antigas; o app distribuído
atualmente usa Cloudflare Realtime e não precisa desse servidor local.

`lk-local.yaml` usado no teste (HTTP/sinalização só em loopback; o WebRTC precisa de um IP de
interface porque o libwebrtc ignora candidatos de loopback):

```yaml
port: 7880
bind_addresses: [127.0.0.1]
rtc:
  tcp_port: 0
  udp_port: 7882
  use_external_ip: false
  node_ip: 192.168.1.2     # IP LAN da própria máquina; nada é encaminhado para a internet
  ips:
    includes: [192.168.1.2/32]
keys:
  devkey: secret
```

## Medições (PC de teste, RTX 5060 Ti, jogo de teste aberto, monitor:0 1920x1080)

Servidor LiveKit 1.13.11 local, publisher e viewer (Chrome, segundo monitor) na mesma máquina.
Encode: H.264 P1, alvo 18 Mbps, jogo GPU-bound em cerca de 108 FPS.

| Corrida | Duração | AUs entregues | Recusados | Keyframes pedidos / atendidos |
|---|---:|---:|---:|---|
| 1 (sem limite de IDR) | 240 s | 22567 | 0 | 248 / 248 |
| 2 (IDR a cada 2 s no máx.) | 150 s | 14471 | 0 | 656 / 26 |

Viewer (amostras de 1 s, corrida 1: 211 amostras; corrida 2: 144 amostras):

| Métrica | Corrida 1 média (p50 / p95) | Corrida 2 média (p50 / p95) |
|---|---|---|
| FPS decodificado | 91,9 (92,2 / 96,8) | 91,1 (93,9 / 97,9) |
| FPS renderizado (rVFC, limitado pelo monitor) | 74,7 (75,8 / 81,2) | 72,7 (74,9 / 80,9) |
| Bitrate recebido (Mbps) | 10,1 (10,2 / 10,9) | 10,2 (10,9 / 11,7) |
| Jitter buffer (ms) | 9,1 (8,9 / 12,6) | 10,6 (10,6 / 11,7) |
| Decode (ms) | 0,35 (0,35 / 0,38) | 0,37 (0,36 / 0,39) |
| Recepção até a tela (ms) | 19,4 (19,4 / 21,0) | 19,2 (19,2 / 20,0) |
| RTT (ms) | 0,06 | 0,00 |
| Pacotes perdidos / frames descartados | 0 / 0 | 0 / 0 |
| Freezes (contador do navegador) | 1 | 3 |

Lado do encoder (corrida 2): 94,4 FPS codificados, `encode_wait` p50/p95 2,2/9,4 ms, bitrate
médio 10,4 Mbps (p95 11,9). A captura visual no monitor do viewer mostrou o vídeo do jogo
(menu do jogo de teste) com o HUD.

### Latência aproximada

Não foi medida de ponta a ponta com relógio óptico (o relógio em ms foi escrito, mas a
medição não chegou a rodar). Estimativa por componentes, no mesmo PC: captura até AU pronto
2 a 9 ms (p50/p95 do `encode_wait` sob carga do jogo) + SFU/rede em loopback menos de 1 ms
(RTT medido 0 a 6 ms) + recepção até a tela 19 ms (jitter buffer 9 a 11 ms, decode 0,4 ms e
espera de vsync do monitor do viewer) = cerca de 22 a 30 ms, mais até um intervalo de captura
(~10 ms a 95 FPS) e o scanout do monitor: ordem de 30 a 40 ms. O campo `capture_to_present_ms`
do viewer ficou `null` (o navegador não recebeu `captureTime`), então a medição exata pede o
relógio óptico ou metadados de pacote (`frame_metadata`) em uma próxima passada.

### Observações

- O WebRTC estima 10 a 13 Mbps mesmo com teto de 18 Mbps e 0% de perda em loopback; o encoder
  segue esse alvo (320 pedidos de taxa em 4 min). Investigar a estimativa de banda (GCC/pacer)
  antes de subir o teto de qualidade.
- Encode ~94 FPS é o ritmo real do jogo na cena (capture-driven); não há 144 FPS a entregar.
- O contador de freezes do navegador subiu 1 a 3 vezes por corrida (provavelmente no início
  e nos IDRs); não houve perda de pacotes nem descarte.
- Erro cosmético ao fechar a sala ("failed to negotiate the publisher ... closed").
- O aceite do M0.4 (1080p60 estável por 10 min, e2e LAN medido) ainda não foi fechado: as
  corridas foram de 2,5 e 4 min, sem medição óptica.

Ajustes e novas medições (bitrate, keyframes, latência vidro a vidro, corrida de 10 min):
bench/m04-transport-tuning.md.
