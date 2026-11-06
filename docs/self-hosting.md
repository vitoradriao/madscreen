[English](self-hosting.en.md)

# Rodar o seu próprio servidor

O app precisa de um **Worker de sinalização** (contas, servidores, canais, convites, presença e o plano de controle do
SFU). Você pode hospedar o seu na Cloudflare e, opcionalmente, um LiveKit de reserva. Este guia lista o que é
preciso; os detalhes de rotas e login estão em [Autenticação e canais](cloudflare-auth.md) e a operação do SFU em
[SFU](sfu.md).

> Os segredos abaixo ficam **só no Worker**. Nunca os coloque no Git, no front-end, em argumentos de linha de
> comando nem em logs. O `wrangler.toml` do repositório não tem ids nem segredos.

## O que você precisa

| Item | Para quê |
| --- | --- |
| Conta Cloudflare com Workers e Durable Objects (SQLite) | Hospeda o Worker (`HubDO` e `StreamDO`) |
| App do **Realtime SFU** da Cloudflare | SFU principal: gera `CF_REALTIME_APP_ID` e `CF_REALTIME_APP_SECRET` |
| Chave **TURN** da Cloudflare | Gera `TURN_KEY_ID` e `TURN_KEY_API_TOKEN` para quem está atrás de NAT restrito |
| App OAuth do **Discord** e/ou **Google** (opcional) | Login social; sem eles, resta o acesso por usuário + convite |
| Chave de assinatura do Tauri | Assina as atualizações do app ([Atualizações](updater.md)) |
| LiveKit auto-hospedado (opcional) | SFU de reserva; ver [SFU](sfu.md) |

## Passo a passo

1. **Clone e instale.** `npm ci --prefix viewer` e `npm ci --prefix tools/cloudflare/worker` (o Worker embute o
   cliente web de `viewer/`).
2. **Configure o `wrangler.toml`.** Troque `CF_REALTIME_APP_ID` pelo App ID do seu Realtime SFU (não é segredo). As
   migrações de Durable Objects já estão declaradas para uma conta nova.
3. **Cadastre os segredos** (`wrangler secret put NOME`): `CF_REALTIME_APP_SECRET`, `SESSION_SECRET` (valor aleatório
   longo), `TURN_KEY_ID`, `TURN_KEY_API_TOKEN` e, se usar login social, `DISCORD_CLIENT_ID`,
   `DISCORD_CLIENT_SECRET`, `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`. Opcionalmente defina `APP_DOWNLOAD_URL`
   (HTTPS) com o link do instalador, usado nas páginas de convite.
4. **Publique.**

   ```powershell
   cd tools/cloudflare/worker
   npm run build              # gera src/web.generated.ts (embute o viewer web)
   npm run typecheck
   npx wrangler deploy        # o wrangler.toml roda `node build.mjs --minify` antes do deploy
   ```

   Há também `deploy-mcp.mjs`, que gera o código de deploy para o MCP da Cloudflare preservando os segredos
   existentes (veja o [README do Worker](../tools/cloudflare/worker/README.md)).
5. **Cadastre os redirecionamentos OAuth** nos painéis do Discord/Google com
   `https://<seu-worker>/auth/discord/callback` e `https://<seu-worker>/auth/google/callback` (passo a passo em
   [Autenticação e canais](cloudflare-auth.md#3-criar-o-app-no-discord-passo-a-passo)).
6. **Teste.** Abra `https://<seu-worker>/health` (deve responder `{"ok":true,...}`) e `https://<seu-worker>/watch`.
   O primeiro usuário não precisa de convite: crie a conta e **guarde a chave de recuperação**.
7. **Aponte o app para o seu Worker** ao compilar: `VITE_MADSCREEN_URL=https://<seu-worker>` (veja
   [Compilar e testar](building.md)).

## Cuidados

- **Não reaplique resets de migração em produção.** As migrações antigas de classes de Durable Objects apagam contas
  e sessões; o Worker aplica a migração SQLite do diretório de forma idempotente e sem apagar dados.
- **`SESSION_SECRET` é gerado uma vez** e preservado entre deploys; trocar invalida todas as sessões.
- **Custos:** a saída do SFU da Cloudflare tem franquia. A política automática de uso do projeto é uma
  *estimativa*, não faturamento; confira sempre o painel do provedor ([SFU](sfu.md)).
- **Privacidade:** o Worker só faz sinalização. Ele não recebe nem grava mídia, e não persiste tokens nem SDP.
- **Distribuição do app:** se você publicar builds, a licença é GPL-3.0-or-later e você deve oferecer o código-fonte
  correspondente, inclusive do runtime FFmpeg ([Runtime](runtime-distribution.md)).
