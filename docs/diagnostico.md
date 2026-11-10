# Diagnóstico local correlacionado

Não coleta nem envia telemetria remota. Copiar é ação do usuário. Os relatórios
contêm contadores, tempos monotônicos locais, provedor, geração e nonce temporário;
sem nomes, tokens, SDP, IDs de candidatos, endereços ou mídia. Logs brutos do SDK
e arquivos do harness são locais/ignorados e não têm essa garantia de exportação.

## Coleta

No app, **Copiar diagnóstico da transmissão** exporta o histórico do sidecar.
No player, **Copiar diagnóstico** conserva 60 amostras recentes de 250 ms e até
120 amostras a aproximadamente 1 Hz, com áudio separado e observações de retomada.
Reconexões/RTP novo isolam segmentos; uma publicação nova limpa o histórico host.
O supervisor conserva o último histórico após EOF para permitir investigação.

O contrato IPC continua versão 3 com `diagnostic_sample` opcional. Engine antigo
é aceito com campo ausente; leitor antigo ignora o campo novo. Payload diagnóstico
tem `schema: 1`. Dados de rede ausentes são null; snapshot mostra idade e expira
em 15 s. Estatísticas RTC são consultadas a cada cinco segundos, sem inventar
um RTT a cada frame. Múltiplos pares nominados sem associação produzem ambiguidade.

`sender.sample_sequence` identifica cada coleta RTC dentro do histórico; ausência
em schema anterior assume zero e não comprova coleta nova. A idade arredondada
sozinha não distingue uma consulta de uma reutilização do mesmo snapshot. O
analisador conserva a janela de envio em `senderObservationFromMs/ToMs`, distinta
da janela dos contadores de AUs. Não comparar os deltas como intervalos idênticos.
A idade é desde a consulta local ao SDK; não é idade de pacote nem tempo de trânsito.

Captura, frames submetidos/concluídos, AUs/IDRs, bytes, ocupação, descartes,
aceitação pelo transporte, áudio e pedidos/aplicações de taxa são separados.
Aceitação da AU não significa envio/recepção. Tempos máximos de AU, espera de
encode e reconfiguração cobrem o intervalo da amostra; não são percentis.
Os novos máximos opcionais `capture_to_convert_max_ms`, `convert_submit_max_ms`
e `queue_wait_max_ms` reutilizam QPCs existentes. Submissão de conversão é tempo
CPU/driver, não execução GPU; espera de fila é do fim da submissão ao início do
encode. Campo ausente/timing inválido permanece null. Nenhuma query/sincronização
GPU adicional foi incluída para esses campos.
Campos opcionais do caminho do encoder: `encoder`, `output_width`/`output_height`,
`fps_target`, `vbv_frames`, `process_cpu_percent` (CPU do engine no intervalo, em % da
máquina inteira), `readback_map_max_ms`/`readback_copy_max_ms`/`codec_send_max_ms`
(máximos do readback FFmpeg; null no NVENC) e os acumulados `static_refine_frames`
(repetições do último quadro com a tela parada) e `idle_keyframes`. Engine anterior: null.
Estatística WGC é ausente em fonte sintética. `sampleGapMs` e `statsCollectionMs`
identificam lacunas/custo de getStats no viewer. rVFC observa entrega ao compositor.

Desligar coleta do engine: `MADSCREEN_DIAGNOSTICS=false` antes de iniciar o app,
ou `--diagnostics false` na CLI. Viewer aceita `WatchSessionOptions.diagnostics`
false para desabilitar armazenamento sem mudar recuperação. A coleta RTC e
estatísticas básicas existentes permanecem. Comparar custo antes/depois em ensaio
controlado; o smoke não mede overhead da instrumentação.

## CLI e análise

```powershell
# Acrescentar aos argumentos de uma publicação de teste; arquivo novo obrigatório.
--stats-interval 1 --diagnostics-output bench-results/ensaio/publisher.jsonl

node tools/bench/correlate-diagnostics.mjs host.jsonl viewer.json saida.json
# Somente quando offset e incerteza foram medidos por marcador comum:
node tools/bench/correlate-diagnostics.mjs host.jsonl viewer.json alinhado.json --offset-ms 123 --uncertainty-ms 15

# Inspeção conservadora de um payload H.264 realmente negociado:
node tools/bench/h264-contract.mjs video.h264 resposta.sdp 108 contrato.json
```

Offset soma ao tempo do viewer para obter o tempo do publicador. Sem ele, os
relatórios ficam separados; não comparar relógios de duas máquinas diretamente.
O operador relaciona a mesma sessão por um identificador temporário de ensaio.
O nonce do host não é uma identidade global nem credencial. Ferramentas geram
arquivos novos e recusam sobrescrita. O analisador permite unknown/null e não
atribui causalidade, episódios físicos ou latência vidro a vidro.

SPS/SDP: a ferramenta só aceita uma seção de vídeo e payload explícito H.264,
exige SPS/PPS/IDR e compara perfil/constraints/nível. Diferença requer revisão,
nunca edição de strings para mascarar o encoder. Igualdade observada não comprova
negociação/decode real ou conformidade integral; nível especial exige revisão.

## Smoke isolado LiveKit

Pré-requisitos: runtime preparado, build release com `--features publish`,
NVIDIA e credenciais de teste existentes em `.dev.vars.livekit`, sem imprimir
seus valores. Worker/Hub/Stream usam armazenamento local novo. Não apontar o
setup de contas para produção.

```powershell
# Terminal 1, a partir de tools/cloudflare/worker:
node node_modules/wrangler/bin/wrangler.js dev --env livekit --local --ip 127.0.0.1 --port 8791 --persist-to ../../../bench-results/ensaio/worker-state
# Terminal 2, raiz do repo; diretório novo:
node tools/bench/setup-media-session.mjs http://127.0.0.1:8791 bench-results/ensaio/media
$env:MADSCREEN_MEDIA_OUTPUT = '<repo>\bench-results\ensaio\media'
npm run test:media --prefix viewer
```

Gera vídeo sintético e tons de teste 600/900 Hz em processo próprio; captura só
o áudio desse processo. Usa salas próprias na VM Magalu existente, encerra seus
publicadores/viewers no finally e não altera seleção de produção. Arquivo de
sessões contém tokens locais: permanece ignorado e não deve ser anexado. Encerrar
também o Wrangler de teste. Contas reais, app instalado e lives pessoais ficam fora.

Esse smoke funcional não é comparação de SFUs, teste de chiado, condição de perda,
benchmark de jogo, aceite de duas lives no WebView2 ou medição A/V/vidro a vidro.

## Qualificação do diagnóstico e custo da coleta

```powershell
cargo build --release -p madscreen-encode-test --features publish
node tools/bench/qualify-diagnostics.mjs --config tools/cloudflare/worker/.dev.vars.livekit --output bench-results/qualificacao-nova --trials 5 --seconds 30
```

Requer as credenciais de teste existentes, NVENC/Windows, dependências do viewer e
Chrome. Gera H.264 sintético sem áudio/jogo, usa sala própria com tokens de curta
duração e não altera Worker ou provedor. Proxy TCP em localhost encaminha somente
as próprias conexões de sinalização para TLS validado. Um corte de 1,5 s exercita
reconexão/segmentação, sem simular perda UDP ou troca de par ICE. O viewer de teste
usa diretamente o SDK LiveKit: não é aceite do WatchSession/WebView2 do produto.

Após o caso de reconexão, executa pares determinísticos de coleta on/off com
mesma fonte/perfil; semente 37138 e ordem registrada. Não grava JSONL nas corridas
de custo: mede coleta do engine, sem custo de disco/exportação. O A/B fixa 12 Mbps
com piso igual ao teto apenas no ensaio. CPU é amostrada em 100 ms, da primeira
linha de stats (após warmup) até a última amostra viva, com duração própria;
p95 é do encode, não da apresentação. Sem jogo,
não fecha overhead do produto nem substitui cinco corridas de 180 s do brief.

Saída nova obrigatória. Resumo anonimizado e relatórios ficam locais; logs SDK e
perfil Chrome não devem ser publicados. A execução encerra processos/proxy/sessões
próprios no finally. Registrar tentativas incompletas também. Ver
qualificação #37/#38.

## Laboratório Cloudflare, WGC e WebView2

Ensaio opt-in de fonte gerada, sem áudio pessoal; não instalar sobre o aplicativo
do usuário. Relatório e tentativas incompletas.

```powershell
cargo build -p madscreen-encode-test --release --features diagnostic-lab
cargo build --manifest-path apps/madscreen-app/src-tauri/Cargo.toml --bin diagnostic-lab --features diagnostic-lab
node tools/bench/prepare-cloudflare-lab.mjs bench-results/cf-lab-NOVO
```

Com MCP Cloudflare autenticado, conferir conta e executar o `deploy.private.mjs`
gerado no `execute`, sem imprimir/copiar seu conteúdo no log/Git. Ele recusa
nome existente, cria app SFU/Worker/DOs próprios e guarda o secret do SFU somente
no Worker. Não usar `deploy-mcp.mjs` de produção nem suas migrações históricas.
Salvar apenas o resultado não secreto `{worker, appId, url, deployed}` em
`deployment.json` na pasta nova. `deployed` deve ser true antes do ensaio.

```powershell
node tools/bench/qualify-cloudflare-lab.mjs bench-results/cf-lab-NOVO
```

O proxy fica em loopback, adiciona a chave privada do lab e atende API/WS do
Worker exclusivo. Bootstrap/convites criam apenas duas contas do lab; sessões
ficam em arquivo ignorado para permitir repetição, sem contas de produção.
Fonte e receiver são janelas Tauri/WebView2 com perfil novo. Preflight WGC local
decodifica IDs do canvas antes de publicar, apaga seu H.264 temporário e bloqueia
o ensaio se falhar; não contornar esse gate. Guard em reprodução aborta
se o marcador sumir; pausa intencional/reconexão são observadas separadamente.

Status IPC e CSV são gravados só por caminhos explicitamente passados na CLI.
`DiagnosticProbe {sequence}` recebe `DiagnosticClock {sequence, run_id, at_ms}`
apenas quando solicitado; ausência é null. Identificar build compatível antes
de usar: protocolo 3 antigo não reconhece a sonda. Nenhum evento novo é emitido
espontaneamente para clientes antigos. Sondas não mudam taxa/codec/política.

`MADSCREEN_LAB_HOLD_RTC=1`, exclusivamente no binário com `diagnostic-lab`, retém
o oitavo report RTC real por seis segundos para cruzar a republicação própria.
Logs identificam retenção/descarte sem IDs RTC. Sem a feature, esse código não
existe; o build normal `--features publish` preserva comportamento de mídia.
Isso é injeção de atraso da observação, não condicionamento da rede.

O harness encerra atores locais e a publicação própria no finally. Depois,
remover via MCP **apenas** Worker e app retornados pelo deploy deste lab,
sem `force`, e conferir inventário/revisão de produção antes/depois. Não
reutilizar `deployment.json` de laboratório já removido. A conexão OAuth pode
permanecer para próximos ensaios; nenhuma credencial precisa ser divulgada.

Retorno: desabilitar/remover feature/binários/harness de lab e compilar
`cargo build -p madscreen-encode-test --release --features publish`. Não alterar
captura/prioridade de jogo/Windows, nem aplicar essa mudança para obter aprovação.
