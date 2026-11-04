# SFU, LiveKit e *failover*

A mídia passa por um **SFU** (servidor WebRTC que repassa os pacotes sem transcodificar). O **Cloudflare Realtime SFU**
é o provedor principal; um **LiveKit auto-hospedado** pode ficar como reserva. O Worker escolhe o provedor e os
clientes seguem essa escolha.

## Abstração de provedor

`SfuProvider` controla criação de sessão, publicação, assinatura, renegociação, encerramento, ICE, camada e
estatísticas do plano de controle. Os adaptadores são `CloudflareProvider` e `LiveKitProvider`. Bitrate, RTT, jitter e
perda são medidos pelos SDKs nos clientes; contadores que o plano de controle não expõe não são estimados no Worker.

- O Worker escolhe por `SFU_PROVIDER` (segredo): ausente ou `cloudflare` usa a Cloudflare; `livekit` usa a reserva.
  `GET /api/sfu` exige autenticação e informa a seleção ao engine.
- Publicadores, espectadores e limpezas **guardam o provedor** que alocou seus recursos. Estados antigos sem o campo
  pertencem à Cloudflare. Trocar o segredo nunca move uma live aberta.
- `expectedProvider` impede que uma troca entre a consulta e o início substitua a transmissão atual com uma
  negociação incompatível.
- O H.264 continua em Annex B, sem recodificação. O encoder pré-codificado publica **uma camada espacial**:
  `preferredRid: h` funciona nos dois provedores e `m`/`l` são recusadas de forma explícita.

### Trocar o provedor estático

```powershell
# Sem --apply só mostra a operação; não altera produção.
node tools/cloudflare/worker/select-static-sfu.mjs livekit
# Verifica uma sala temporária autenticada no LiveKit e seleciona novas lives.
node tools/cloudflare/worker/select-static-sfu.mjs livekit --apply
# Volta a encaminhar novas lives à Cloudflare (as abertas continuam no LiveKit).
node tools/cloudflare/worker/select-static-sfu.mjs cloudflare --apply
```

O seletor altera só `SFU_PROVIDER`, por stdin, sem publicar código nem registrar credenciais. O pré-teste cria e
apaga uma sala aleatória própria; se falhar, a seleção não muda.

## LiveKit de reserva

Segredos do Worker: `LIVEKIT_URL`, `LIVEKIT_API_KEY`, `LIVEKIT_API_SECRET` e `SFU_PROVIDER`.
`tools/cloudflare/worker/configure-livekit.mjs` prepara um ambiente local isolado e, com `--publish-secrets`,
grava os segredos no Worker mantendo `SFU_PROVIDER=cloudflare`.

**Sessões.** Cada publicação tem uma sala aleatória própria, confirmada no Room Service antes de anunciar "ao vivo".
Os JWTs de entrada duram 60 s; o publicador só publica tela e áudio; espectadores só assinam; canais de dados ficam
desligados; tokens não são persistidos. Fechar o publicador apaga a sala; fechar um espectador remove só ele.
Limitação conhecida: o LiveKit auto-hospedado não revoga imediatamente tokens já emitidos; o cliente encerra ao
perder o acesso por *heartbeat*/eventos.

**Áudio estéreo.** O SDK Rust 0.10.0 omite o sinalizador `TF_STEREO` ([bug upstream](https://github.com/livekit/rust-sdks/issues/1018)).
A cópia em `third_party/livekit` corrige o anúncio estéreo/DTX; veja `third_party/livekit/MADSCREEN_PATCH.md`.

**Operação do servidor.**

- TLS de uma autoridade confiável para a sinalização, com a API (7880) atrás de um *proxy* TLS; confirme também o
  ICE/TCP 7881 e a faixa UDP configurada. Um *health check* HTTP não prova UDP/TURN.
- Reconstrua sempre a partir da imagem/*digest* realmente em uso (nunca `latest`) e preserve compose, `livekit.yaml`
  e a configuração do *proxy*.
- `tools/ops/livekit-state.mjs` faz backup e restauração **só da configuração** (`docker-compose.yaml`,
  `livekit.yaml`, `Caddyfile`). O backup pode conter chaves: não o versione nem o anexe.
- Referências: [implantação](https://docs.livekit.io/transport/self-hosting/deployment/) e
  [portas](https://docs.livekit.io/transport/self-hosting/ports-firewall/).

## Política automática de saúde, uso e *failover* (opt-in)

Desligada por padrão: `SFU_PROVIDER` continua selecionando o provedor. Para habilitar, defina no Worker:

| Nome | Padrão | Significado |
| --- | --- | --- |
| `SFU_FAILOVER_ENABLED` | ausente | `1` habilita a política automática |
| `SFU_ADMIN_USER_IDS` | ausente | IDs internos das contas administradoras, separados por vírgula |
| `SFU_MONTHLY_QUOTA_BYTES` | `1000000000000` | Orçamento de saída da Cloudflare usado na estimativa |
| `SFU_FAILOVER_THRESHOLD` | `0.9` | Fração do orçamento que encaminha novas transmissões à reserva |
| `SFU_USAGE_BITRATE_BPS` | `30128000` | Hipótese de bitrate total por espectador; ajuste ao seu perfil |
| `SFU_USAGE_CALIBRATED` | ausente | Só `1` permite trocar/bloquear por estimativa; defina depois de calibrar com o painel do provedor |
| `SFU_CYCLE_DAY` | `1` | Primeiro dia do ciclo em UTC (1 a 28) |

**Saúde.** O alarme do Hub sonda os dois planos de controle a cada 30 s (Cloudflare: cria uma sessão sem faixas;
LiveKit: cria e apaga uma sala vazia). Duas falhas marcam o provedor indisponível; uma falha real de alocação o marca
na hora. O retorno à Cloudflare exige três sucessos e 60 s desde a troca. As sondas confirmam o **plano de controle**,
não UDP/TURN, decode nem sincronia A/V. Sem provedor elegível, novas alocações recebem 503.

**Estimativa de uso.** Bitrate configurado × tempo × espectadores aceitos (inclusive a prévia), por provedor e por
ciclo (UTC), guardada em SQLite por até 13 ciclos. É uma estimativa, **não faturamento**: compare com o painel do
provedor e ajuste a hipótese. Sessões abertas continuam consumindo depois do limiar.

**Administração.** `GET /api/admin/sfu` (sessão válida de administrador) devolve modo, provedor, saúde e as últimas
transições, sem tokens, SDP nem URLs privadas. `POST /api/admin/sfu` aceita `{"mode":"auto"|"cloudflare"|"livekit"}`
ou `{"refresh":true}`. No app, **Conta → Serviço de transmissão** aparece só para administradores.

Desligar a política restaura a seleção estática para novas lives; as abertas mantêm seu provedor. Sessões existentes
nunca são migradas pela política.
