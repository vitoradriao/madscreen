# madscreen-signal

Worker + Durable Objects (SQLite) com contas, servidores, canais, presença e sinalização do Realtime SFU da
Cloudflare. Documentação completa, rotas, login e passo a passo do Discord em
[docs/cloudflare-auth.md](../../../docs/cloudflare-auth.md).

```sh
cd tools/cloudflare/worker
npm ci --prefix ../../../viewer  # SDK compartilhado pelo viewer web e app
npm run typecheck                 # tsc do Worker e do código compartilhado do viewer
npm test                          # contrato de SFU e integração de servidores/convites em SQLite local
node build.mjs [--minify]         # embute o viewer web (viewer/web + viewer/shared) em src/web.generated.ts
node deploy-mcp.mjs /tmp/ms-deploy.mjs [flags]   # gera o código para `execute` do MCP Cloudflare
```

Abstração de SFU e reserva LiveKit na Magalu: [docs/sfu.md](../../../docs/sfu.md).
Cloudflare permanece como padrão. Engine e viewer seguem o provedor escolhido pelo Worker;
os testes de mídia na reserva aguardam o usuário ligar a VM.

- `HubDO` (singleton): contas, identidades, servidores/canais/membros, amizades, convites, OAuth, presença por
  WebSocket e linhas "ao vivo". `StreamDO` (um por usuário): estado do SFU, viewers, alarmes de
  limpeza e eventos.
- Bindings: `CF_REALTIME_APP_ID` (texto), `HUB` e `STREAM` (DOs) e os secrets `CF_REALTIME_APP_SECRET`,
  `SESSION_SECRET`, `TURN_KEY_ID`, `TURN_KEY_API_TOKEN`, `DISCORD_CLIENT_ID`, `DISCORD_CLIENT_SECRET`,
  `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET` (os quatro últimos opcionais). Os deploys preservam
  bindings `secret_text` e `plain_text`. `APP_DOWNLOAD_URL` (HTTPS) configura o download do instalador.
- Flags do gerador: `--app-id ID`, `--migration-from TAG`, `--migration-to TAG`,
  `--drop-binding NOME`, `--generate-secret NOME` (gera um valor aleatório no ambiente do MCP, sem
  exibir), `--create-turn-key`. O arquivo de saída é criado com modo `0600`, não sobrescreve outro e
  nunca contém secrets.
- O diretório aplica sua migração SQLite idempotente sem alterar classes ou namespaces de DO.
  Não reaplique os resets históricos de classes v1–v8 em produção: eles apagam contas e sessões.
- **Nunca commite segredos, arquivos de deploy ou `.dev.vars`.** Nenhum log guarda tokens, secrets
  ou SDP.
