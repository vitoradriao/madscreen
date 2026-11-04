# Arquitetura

O madscreen tem três partes que falam entre si: o **app desktop** (Tauri 2 + WebView2) com um **engine** de captura
e codificação por transmissão, um **Worker** autenticado na Cloudflare e um **SFU** (servidor de mídia WebRTC).

```text
App Tauri 2 / WebView2
  ├─ UI compartilhada com o cliente web: login, convites, canais e dois players
  └─ supervisor ──stdin JSON──► sidecar madscreen-encode-test --ipc   (um processo por transmissão)
                    ◄─stdout JSON─  WGC / Desktop Duplication → D3D11 (escala + BGRA→NV12) → encoder automático
                                    WASAPI process loopback → fonte de áudio → Opus
                                                  │
                          Worker autenticado: HubDO + StreamDO (SQLite, WebSockets)
                                   ├─ Cloudflare Realtime SFU   (principal)
                                   └─ LiveKit auto-hospedado    (reserva)
                                                  │
                                    WebRTC → player no app ou no navegador
```

## Componentes

| Parte | Onde | Responsabilidade |
| --- | --- | --- |
| App desktop | `apps/madscreen-app` | Janela (Tauri + WebView2), bandeja, supervisor do engine, atualização assinada |
| Engine (sidecar) | `apps/madscreen-encode-test` | Captura → conversão → encoder → transporte; fala com o app por IPC JSON |
| Captura | `crates/madscreen-capture` | WGC e Desktop Duplication, mailbox *latest-frame-wins* |
| Encoders | `crates/madscreen-encode` | NVENC (D3D11 zero-copy), AMF, QuickSync e x264 via FFmpeg |
| Áudio | `crates/madscreen-audio` | WASAPI *process loopback*, mixagem, Opus |
| Transporte | `crates/madscreen-transport`, `crates/madscreen-transport-cf` | Publicação no LiveKit e na Cloudflare |
| IPC | `crates/madscreen-ipc` | Protocolo JSON, uma linha por mensagem |
| Viewer/UI | `viewer/shared`, `viewer/web` | Cliente do Worker, login, diretório e player, usados pelo app e pela web |
| Worker | `tools/cloudflare/worker` | Contas, servidores, canais, convites, presença e sinalização do SFU |
| SDK vendorizado | `third_party/livekit`, `third_party/webrtc-sys` | Cópias com patches mínimos (ver os avisos em cada pasta) |

## Princípios de desempenho

O objetivo central é **não tirar FPS do jogo de quem transmite**.

- **Um processo por transmissão.** Parar a live encerra o processo e devolve a GPU ao jogo.
- **Pacer *latest-frame-wins*.** O mailbox de captura tem profundidade um: nunca se enfileiram quadros, o velho é
  descartado. O encoder tem poucos quadros em voo e não há alocação por quadro no caminho quente.
- **NVENC sem *readback*.** Escala, conversão BGRA→NV12 e encode ficam na GPU. AMF, QuickSync e x264 usam *readback*
  explícito de NV12 para o FFmpeg; esses caminhos são de compatibilidade e não prometem o mesmo custo.
- **Nunca prioridade REALTIME.** O jogo vem primeiro.
- **Só APIs oficiais.** WGC e Desktop Duplication; sem injeção de DLL e sem *hook* de jogo.
- **Sem transcodificação no servidor.** O H.264 do engine chega ao espectador como foi codificado.

## Fluxo de uma transmissão

1. O app pede ao Worker uma sessão (`/api/*`, autenticada). O Worker decide o SFU e devolve o que o engine precisa.
2. O app inicia o sidecar com a janela/monitor escolhido; o engine captura, converte e codifica.
3. O engine publica o H.264 (Annex B) e o Opus no SFU por WebRTC. O Worker só faz sinalização e não vê mídia.
4. Os espectadores abrem a live no app ou no navegador e recebem a mídia direto do SFU.
5. O app mostra FPS e bitrate medidos a cada segundo; a prévia da própria live é sempre muda.

## Captura

- **Janelas e Windows 11:** Windows Graphics Capture (WGC).
- **Monitor no Windows 10:** Desktop Duplication, sem o indicador amarelo do WGC. O seletor nativo legado continua WGC.
- Cursor, rotação e troca de forma do cursor são compostos na GPU. A captura de janela não é substituída por um
  recorte de monitor.

## Áudio

WASAPI *process loopback* de 48 kHz, estéreo, com Opus (até 128 kbps, DTX desligado). O áudio é por aplicativo; o
som do PC exclui o próprio app e seus descendentes para não recapturar o que o espectador ouve. Sem a API no
Windows instalado, o vídeo continua. A sincronia A/V ainda está em medição.

## Servidor e SFU

O Worker guarda contas, identidades, servidores, canais, convites e presença em Durable Objects com SQLite
(`HubDO` único e um `StreamDO` por usuário). Escolhe o provedor de SFU por configuração e cada publicador,
espectador e limpeza **registra qual SFU alocou seus recursos**: trocar a configuração nunca muda sessões abertas.
O LiveKit é uma reserva; a política automática de saúde e uso é opt-in. Veja [SFU](sfu.md).

## Segurança e privacidade

- Credenciais do SFU e o segredo de sessão ficam só no Worker; app e viewer só falam com `/api/*`.
- Tokens, segredos e SDP não são gravados em log nem em estado persistente.
- O diagnóstico é local, copiado por ação do usuário e sem nomes, tokens, SDP ou endereços.
- O app não pede microfone. A atualização valida assinatura e versão antes de instalar ([Atualizações](updater.md)).
- Para reportar uma vulnerabilidade, veja [SECURITY.md](../SECURITY.md).

## Fora do escopo atual

macOS e Linux (a lógica pura continua compilando em Linux), HDR ponta a ponta, HEVC/AV1, *simulcast*, player
nativo e chat de texto ou voz. Alguns desses itens estão em avaliação; veja o [roadmap](roadmap.md).
