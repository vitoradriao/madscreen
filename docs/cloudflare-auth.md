# Conta, servidores e transmissão pelo Worker (madscreen-signal)

Objetivo: abriu, autenticou, entrou no servidor e usou. Uma conta, servidores com canais e
transmissões ao vivo com **Assistir**. Sem configurar portas ou serviços no computador do amigo.

- Worker publicado: <https://seu-worker.workers.dev> (viewer web em `/watch`).
- Código: `tools/cloudflare/worker` (TypeScript + Durable Objects). Telas compartilhadas por app e
  web: `viewer/shared`. Telas do app: `apps/madscreen-app/src`.
- O App Secret do Realtime SFU nunca sai do Worker. App e viewer só falam com `/api/*`.

## 1. Arquitetura

```text
 app Tauri / viewer web ──HTTPS+WS──► Worker ──► HubDO  (singleton "hub")  contas, amigos, convites,
                                        │                presença (WebSockets), "ao vivo agora"
                                        └──────► StreamDO (um por usuário)  SFU: publicador, viewers,
                                                         alarmes de limpeza, eventos em WebSocket
 mídia: publicador (engine) ⇄ Realtime SFU ⇄ viewers (WebRTC; o Worker só faz sinalização)
```

**Persistência: Durable Objects com SQLite, não D1.** O hub precisa de leitura-após-escrita e de
um ponto único de serialização (aceitar amizade, resgatar convite, contar usos) e já segura os
WebSockets de presença. D1 acrescentaria um binding, migrações e consistência eventual das réplicas
sem nenhum ganho na escala de um grupo de amigos. A mídia nunca passa pelos DOs.

Sessão: token sem estado `<payload>.<hmac>` (HMAC-SHA256 com o secret `SESSION_SECRET`), válido por
90 dias e renovado por `GET /api/me` quando passa de 30. O Worker autentica, pergunta ao hub se o
usuário participa do servidor do canal da live e só então repassa ao `StreamDO` com cabeçalhos internos
`x-ms-*` (qualquer `x-ms-*` vindo do cliente é descartado). WebSocket autentica por subprotocolo
`ms.<token>` (o token não vai na URL).

## 2. Entrar

Três caminhos; o app e o viewer web usam os mesmos.

1. **Discord** (principal) e **Google**. O cliente gera um `verifier` aleatório e abre
   `/auth/{discord|google}/start?challenge=SHA256(verifier)` no navegador. Depois do consentimento o
   callback guarda a sessão sob o `challenge`; o cliente consulta `POST /auth/exchange {verifier}`
   (`202` pendente → `200` sessão). Só quem tem o `verifier` recebe a sessão (estilo PKCE). No
   viewer web o `verifier` fica em `localStorage` (`ms.pkce`) e retoma após o redirecionamento.
   O polling entrega a sessão. O protocolo `madscreen://invite/CODIGO` abre somente convites;
   ele não transporta tokens de autenticação.
2. **Usuário + convite de amigo** (funciona hoje, sem Discord): quem já tem conta gera um link/código
   `XXXX-XXXX` (vale 7 dias, até 5 pessoas, máximo 10 ativos). Ao criar a conta com o código, os dois
   viram amigos e a nova conta entra no servidor/canal do convite. A conta recebe uma **chave de recuperação** `XXXXX-XXXXX-XXXXX-XXXXX`, mostrada uma
   única vez (só o hash SHA-256 fica no servidor).
3. **Entrar de novo**: usuário + chave de recuperação.

**Primeiro usuário:** enquanto o hub está vazio, o cadastro não pede convite (`/auth/providers`
devolve `firstUser: true`). Limites por IP: 10 cadastros/logins por 10 min, 30 prévias, 10 resgates.
`DELETE /api/me` apaga a própria conta (e derruba a transmissão dela).

## 3. Criar o app no Discord (passo a passo)

O login com Discord depende dos secrets abaixo. Em 30/10/2026, `/auth/providers` em produção
retornou `discord: true` e `google: false`. Consulte a rota para o estado atual.

1. Abra <https://discord.com/developers/applications> com a sua conta e clique em **New Application**.
   Nome: `madscreen`. Aceite os termos e **Create**.
2. No menu lateral, **OAuth2**. Em **Redirects** clique em **Add Redirect** e cole **exatamente**:

   ```text
   https://seu-worker.workers.dev/auth/discord/callback
   ```

   Sem barra no final, em `https`. Clique em **Save Changes**.
3. Na mesma página, copie o **Client ID**. Clique em **Reset Secret**, confirme e copie o
   **Client Secret** (aparece uma vez).
4. Não precisa de bot, de `Public Bot`, nem de escopos além de `identify` (o Worker pede só isso).
5. Guarde os dois valores como **secrets do Worker** (nunca no repositório):
   - Painel da Cloudflare → **Workers & Pages** → `madscreen-signal` → **Settings** →
     **Variables and Secrets** → **Add** → tipo **Secret** → nomes `DISCORD_CLIENT_ID` e
     `DISCORD_CLIENT_SECRET`; **Deploy**.
   - Ou, na pasta `tools/cloudflare/worker`: `npx wrangler secret put DISCORD_CLIENT_ID` e
     `npx wrangler secret put DISCORD_CLIENT_SECRET`.
6. Teste: abra `/watch`, clique em **Continuar com Discord**. O botão fica ativo sozinho (o Worker
   lê os secrets a cada requisição; não precisa de novo deploy do código).

**Google (opcional):** <https://console.cloud.google.com/apis/credentials> → **Create credentials** →
**OAuth client ID** → tipo **Web application** → **Authorized redirect URIs**:
`https://seu-worker.workers.dev/auth/google/callback`. Configure a tela de
consentimento (escopos `openid` e `profile`) e guarde `GOOGLE_CLIENT_ID` e `GOOGLE_CLIENT_SECRET`
como secrets, igual ao Discord.

Credenciais devem usar **Secret** (`secret_text`). O gerador de deploy preserva secrets e
variáveis de texto existentes, inclusive seleção de SFU e URL pública do instalador.

## 4. Rotas

Públicas: `GET /health`, `/`, `/watch`, `/login`, `/i/{código}` (viewer web), `/web.js`, `/web.css`,
`/fonts/inter.woff2`, `GET /auth/providers`, `GET /auth/{discord|google}/start|callback`,
`POST /auth/exchange|register|login`, `GET /api/invites/preview?code=`.

Com `Authorization: Bearer <sessão>` (WebSocket: subprotocolo `ms.<sessão>`):

| Rota | Função |
|---|---|
| `GET /api/me`, `PATCH /api/me`, `DELETE /api/me` | perfil, nome de exibição, apagar conta |
| `GET /api/friends`; `POST /api/friends/request|accept|decline`; `DELETE /api/friends/{id}` | amizades |
| `POST /api/invites`, `POST /api/invites/redeem` | link/código de convite |
| `GET/POST /api/servers`; `GET/PATCH/DELETE /api/servers/:id` | listar/criar/renomear/apagar servidores |
| `GET/POST /api/servers/:id/channels`; `PATCH/DELETE /api/servers/:id/channels/:channel` | canais |
| `DELETE /api/servers/:id/members/me`; `PATCH /api/servers/:id/members/:uid` | sair e alterar papel |
| `POST /api/servers/:id/invites` | convite do servidor/canal (dono/admin) |
| `GET /api/ws` | presença, lista de amigos e "ao vivo agora" em tempo real |
| `GET /api/ice` | STUN + TURN da Cloudflare (credenciais temporárias) |
| `POST /api/share/start|heartbeat|stop` | **publicador** (engine) |
| `POST /api/watch/start|answer|heartbeat|leave` | viewer (membro do servidor da live) |
| `GET /api/stream/status?owner=`, `GET /api/stream/ws?owner=` | estado e eventos (`live`, `ended`, `viewer_joined`, `viewer_left`, `keyframe`) |

Erros: JSON `{error, message}`; `403 sem_acesso_servidor`, `409 sem_publicador`, `410` convite expirado/esgotado, `429` limite, `503` provedor
OAuth não configurado. Limites: 8 viewers por transmissão, 60 requisições/10 s por usuário.

## 5. Interface do publicador (engine)

Quem publica pelo Worker é o crate `madscreen-transport-cf` (`signaling.rs`):

```text
POST /api/share/start      {offer, mid, title?, channelId?} → {answer, sessionId, generation, trackName}
POST /api/share/heartbeat  {generation}          → {ok, viewers, generation, keyframe}
POST /api/share/stop       {generation}
GET  /api/ice                                    → {iceServers: [...]}
```

Todas com `Authorization: Bearer <token de sessão do usuário>`. O que muda é o token: **não existe
mais `OWNER_KEY`**; o engine usa a sessão de quem está transmitindo. Se `keyframe` vier `true` no
heartbeat, force um IDR (um viewer entrou ou pediu). Mande heartbeat a cada ~5 s (o `StreamDO` encerra
publicador sem heartbeat por 25 s). Cada usuário tem uma transmissão, identificada por `uid`: os
membros do servidor assistem por `owner=<usuário>`. `channelId` é verificado no Hub e repassado
em cabeçalho interno confiável. Clientes antigos sem canal usam o grupo padrão do dono.

No app Tauri o gancho já existe: `startShareCloudflare(height, fps, source, url, token)` em
`src/api.ts` chama o comando `start_share_cloudflare`. A aba **Compartilhar** passa
`WORKER_URL` (`src/session.ts`, sobrescrevível por `VITE_MADSCREEN_URL`) e o token da sessão. Para
testar o engine sem o app: `MADSCREEN_CF_URL` e `MADSCREEN_CF_TOKEN` com a URL do Worker e um token
(faça login em `/auth/login` e use o campo `token`).

## 6. Telas

**App** (`apps/madscreen-app`): *Entrar* (Discord, Google, convite ou chave), *Servidores* (lista de
servidores, canais, presença e lives com **Assistir**), *Compartilhar* (canal, seletor de janela/tela,
resolução, FPS, quem está assistindo), *Assistir* (SFU da Cloudflare, tela cheia da janela inteira) e
*Conta*. O link do Discord/Google abre no navegador do sistema (`tauri-plugin-opener`, restrito a
`*.workers.dev`).

**Viewer web** (`/watch`): mesmo login e diretório de servidores/canais. `/i/CODIGO` mostra a página
de convite, abertura no app e download. Baixa latência (`jitterBufferTarget` e
`playoutDelayHint` no mínimo), `object-fit: contain` (letterbox/pillarbox), tela cheia por botão,
duplo clique ou tecla **F**, e HUD com resolução, FPS e bitrate.

**Visual:** grafite e branco, sem cores de destaque. Tudo vem de variáveis em
`viewer/shared/tokens.css` (superfícies, texto, bordas, raios, espaçamento, movimento). Fonte Inter
vendorizada (`viewer/shared/fonts`, cópia em `apps/madscreen-app/public/fonts`), sem CDN.

## 7. Publicar o Worker

Pelo MCP da Cloudflare (`execute`), preservando os secrets:

```sh
cd tools/cloudflare/worker
npm run typecheck && node build.mjs --minify
node deploy-mcp.mjs /tmp/ms-deploy.mjs      # gera o código de deploy; não publica nem imprime secrets
# envie o conteúdo de /tmp/ms-deploy.mjs como `code` de execute (MCP user-Cloudflare)
```

Ou `npx wrangler deploy` (o `wrangler.toml` não tem ids nem secrets; troque `CF_REALTIME_APP_ID`).
`deploy-mcp.mjs` aceita `--migration-from TAG`/`--migration-to TAG` e `--drop-binding NOME`; o
`SESSION_SECRET` é gerado uma vez com `--generate-secret SESSION_SECRET` e nunca é exibido. Secrets do
Worker: `CF_REALTIME_APP_SECRET`, `SESSION_SECRET`, `TURN_KEY_ID`, `TURN_KEY_API_TOKEN` e, quando
criados, `DISCORD_*` e `GOOGLE_*`.

Os secrets antigos `OWNER_KEY` e `INVITE_SIGNING_KEY` não são mais usados e podem ser removidos do
painel (Settings → Variables and Secrets).

## 8. Como testar

1. Abra <https://seu-worker.workers.dev/watch>, crie a conta (o primeiro usuário
   não precisa de convite) e guarde a chave de recuperação.
2. **Servidores → canal → Convidar** e mande para um amigo; ao entrar, ele vira membro do servidor.
3. No app, aba **Compartilhar**, escolha a janela e comece. No viewer web do amigo aparece
   a live no canal; **Assistir** abre o vídeo.
4. `curl https://seu-worker.workers.dev/health` deve devolver `{"ok":true,...}` e
   `curl .../auth/providers` mostra quais logins estão configurados.

## 9. Diretório e migração

O SQLite do Hub guarda `servers`, `channels` e `members` (papéis `owner/admin/member`). Dono gerencia
servidor e papéis; dono/admin gerenciam canais e convites; todo membro pode publicar e assistir.
Sair transfere a propriedade para um admin ou membro mais antigo; sem membros o servidor é removido.
Apagar um canal encerra suas lives e invalida seus convites. Não se pode apagar o último canal.
Novas entradas/criações limitam 32 servidores por conta, 32 canais e 128 membros por servidor.

A primeira inicialização aplica `servers-v1` em transação, sem apagar contas, recovery hashes,
amizades, OAuth ou usos dos convites. Cada conta antiga ganha seu próprio servidor com seus amigos
aceitos e canal Geral, mantendo os círculos privados. Convites/lives antigos apontam para esse canal.
A migração é idempotente; não cria um servidor global juntando contas sem relação.

Esta atualização não exige migração de classes Durable Object. **Não reaplique os resets v1–v8**
nem exclua os namespaces de produção. O snapshot do WebSocket inclui `servers`; as rotas de
amizade continuam disponíveis para clientes antigos. Respostas, heartbeats e eventos de live
continuam vinculados ao usuário e à geração da transmissão.
