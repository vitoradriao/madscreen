# Estado e roadmap

Última revisão: 4/nov/2026, versão publicada **0.1.14** (pré-1.0).

O madscreen funciona de ponta a ponta, mas **ainda não declara metas de desempenho atingidas**. A regra do projeto é
simples: compilar, passar em teste sintético ou ver a interface funcionando **não** prova FPS, latência nem sincronia
A/V. Só entra como "medido" o que tem número reproduzível no hardware citado.

## O que já funciona

- Compartilhar janela ou monitor com captura oficial do Windows (WGC e Desktop Duplication) e áudio por aplicativo
  (WASAPI *process loopback*, Opus estéreo).
- H.264 com NVENC zero-copy e fallbacks AMF, QuickSync e x264; adaptação de taxa por feedback do WebRTC.
- Contas (Discord, Google, usuário + convite com chave de recuperação), servidores, canais, convites e presença.
- Até duas lives simultâneas por espectador, com volume independente; prévia própria sempre sem som.
- Cloudflare Realtime SFU como principal, LiveKit como reserva e política opt-in de saúde/uso/*failover*.
- Atualização assinada com verificação de versão, mini-player, bandeja, diagnóstico local e três temas.
- Cliente web com os mesmos módulos do app.

## O que já foi medido (e seus limites)

| Tema | Resultado | Limite |
| --- | --- | --- |
| Adaptação de taxa (NVENC, relay UDP local) | Na queda para 3 Mbps, o FPS recebido subiu de 5,5 para 56,5 com a política atual | Cenário sintético em laboratório, não é jogo nem WAN |
| H.264 High × Constrained Baseline (NVENC, SSIM/PSNR) | CB perde cerca de 1 a 1,6 dB de PSNR e gasta 4–20% mais bytes; a diferença é ≈ 0 a partir de 8 Mbps em desktop | Exploratório, só NVENC |
| Atualização assinada | Verificação de assinatura, de versão e do manifesto cobertas por testes | Upgrade entre versões e PC limpo ainda sem resultado |
| Revogação de acesso (kick, saída, exclusão) | Seis ações exercitadas contra o Worker e o LiveKit reais; sem quadros indevidos depois da revogação | Tokens já emitidos do LiveKit auto-hospedado não são revogados na hora |

## Rumo à v1.0

Os critérios abaixo estão abertos como issues; a v1.0 não fecha enquanto não houver medição:

1. **Beta com amigos** — ciclos curtos de uso real, com registro de incidentes ([#2](https://github.com/vitoradriao/madscreen/issues/2)).
2. **Sincronia A/V** — defasagem e deriva medidas por eventos de áudio e imagem correlacionados; alvo aproximado de
   40 ms ([#3](https://github.com/vitoradriao/madscreen/issues/3)).
3. **SFU de reserva** — qualificar o LiveKit e compará-lo com a Cloudflare ([#4](https://github.com/vitoradriao/madscreen/issues/4)).
4. **Contrato de perfil H.264** — o perfil/nível que o encoder gera precisa ser o que o SDP negocia e o receptor
   suporta, sem editar o SPS para parecer compatível ([#5](https://github.com/vitoradriao/madscreen/issues/5)).
5. **Runtime e distribuição** — pacote GPL com código-fonte correspondente e decisão sobre o fallback por CPU
   ([#6](https://github.com/vitoradriao/madscreen/issues/6)).
6. **Impacto no jogo por hardware** — PresentMon 2.x, cinco corridas de três minutos, intervalo de confiança
   ([#7](https://github.com/vitoradriao/madscreen/issues/7)).
7. **AMD e Intel em hardware real** ([#8](https://github.com/vitoradriao/madscreen/issues/8)) e **instalação/upgrade em PC limpo** ([#9](https://github.com/vitoradriao/madscreen/issues/9)).

Metas de desempenho mantidas do projeto (ainda **não** atingidas ou medidas por completo): perda de FPS do jogo ≤ 3%
com *cap* e ≤ 6% em GPU saturada (NVENC), 1080p60 entregue ≥ 58 FPS por dez minutos, latência vidro a vidro p50 ≤ 40 ms
na LAN, p50 ≤ 100 ms na WAN da mesma região, encode NVENC 1080p144 com p95 ≤ 5 ms.

## Em avaliação (não prometidos)

| Tema | Pergunta |
| --- | --- |
| Buffer de jitter | Um alvo maior que o atual melhora o *stutter* sem piorar a latência e a sincronia A/V? ([#10](https://github.com/vitoradriao/madscreen/issues/10)) |
| 90 e 120 FPS | É viável sem prejudicar o jogo? ([#11](https://github.com/vitoradriao/madscreen/issues/11)) |
| HDR | Dá para entregar HDR ponta a ponta e converter para SDR? ([#12](https://github.com/vitoradriao/madscreen/issues/12)) |
| HEVC | Funciona em WebRTC e nos SFUs? ([#13](https://github.com/vitoradriao/madscreen/issues/13)) |
| AV1 | Funciona ponta a ponta, com fallback para H.264? ([#14](https://github.com/vitoradriao/madscreen/issues/14)) |
| *Simulcast* | O benefício de duas camadas compensa o custo? ([#15](https://github.com/vitoradriao/madscreen/issues/15)) |

## Fora do escopo por ora

macOS e Linux (a lógica pura continua compilando em Linux), chat de texto ou voz, canais públicos de descoberta,
injeção de DLL ou *hook* de jogo e transcodificação no servidor.

## Como ajudar

Veja as issues com os rótulos `good first issue` e `help wanted` e o [CONTRIBUTING](../CONTRIBUTING.md). Testes em
hardware AMD e Intel, relatos de incompatibilidade de GPU/driver e traduções da documentação são especialmente úteis
([#8](https://github.com/vitoradriao/madscreen/issues/8), [#16](https://github.com/vitoradriao/madscreen/issues/16)).
