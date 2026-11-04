# Convites de servidor e canal

O mesmo instalador `madscreen-setup-windows-x64` permite transmitir e assistir. O amigo não precisa
instalar Tailscale, LiveKit ou configurar portas. O viewer web também está disponível em
[Abrir madscreen](https://seu-worker.workers.dev/watch).

## Enviar e aceitar

1. Em **Servidores**, o dono ou administrador escolhe um canal e usa **Convidar**.
2. O endereço `https://<worker>/i/XXXX-XXXX` mostra servidor, canal e quem convidou. Oferece
   **Já tenho o app**, download para Windows e **Continuar no navegador**. Depois de instalar,
   volte à página e abra o convite no app. Também é possível colar o código ou link no app.
3. O amigo entra por um provedor configurado ou cria usuário com o convite. Quem se cadastra por
   usuário precisa guardar a chave de recuperação exibida pelo app.
4. Após o login, o resgate entra automaticamente no servidor e seleciona o canal correto.
   **Transmitir aqui** abre o compartilhamento nesse canal; **Assistir** abre uma live do canal.

O convite atual dura sete dias, aceita até cinco usos e tem limite de dez convites ativos por conta.
Um convite inválido, expirado ou esgotado recebe mensagem em pt-BR. O primeiro cadastro do Worker
não exige convite; depois disso, um cadastro por usuário depende de convite válido.

O instalador 0.1.1 registra `madscreen://invite/XXXX-XXXX`. O código fica guardado durante o login e
só é removido após o resgate. Reabrir com a mesma conta já membro não consome outro uso.
O resgate mantém também a amizade com o criador para compatibilidade; amizade isolada não permite
assistir a lives de servidores dos quais a conta não participa.

## Contrato do Worker

| Rota | Uso |
|---|---|
| `POST /api/servers/:id/invites` com `{channelId}` | Dono/admin cria convite do canal |
| `POST /api/invites` com `{serverId?,channelId?}` | Compatibilidade; sem destino usa o grupo padrão |
| `GET /api/invites/preview?code=` | Prévia pública, validade, servidor e canal |
| `POST /api/invites/redeem` com `{code}` | Resgate autenticado; retorna snapshot e `destination` |
| `POST /auth/register` | Cadastro por usuário, nome e convite |
| `GET /auth/{provider}/start?...&invite=` | OAuth conserva o convite durante o login |

Contas, autenticação, permissões e operação do Worker: [cloudflare-auth.md](cloudflare-auth.md).
Instalação e migração de sessão: [app.md](app.md).

## Download público

Configure `APP_DOWNLOAD_URL` no Worker com uma URL HTTPS pública do instalador validado.
`GET /app/info` informa a disponibilidade e `GET /download/windows` redireciona para ela.
Sem URL configurada, a página explica a indisponibilidade e mantém a opção de usar o navegador.
Publicar o executável e configurar essa URL são etapas de entrega separadas da implementação.
