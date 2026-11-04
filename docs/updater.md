# Atualizações assinadas

O app usa o `tauri-plugin-updater` (2.x) com **chave pública embarcada** e `requireSignedVersion = true`. O formato do
manifesto segue o [updater do Tauri](https://v2.tauri.app/plugin/updater/).

## Como funciona

- Um controlador único (`apps/madscreen-app/src/updates.ts`) consulta o manifesto 10 s depois de abrir o app e depois
  a cada hora. A consulta **não baixa nem encerra mídia**.
- Havendo versão nova, aparece um aviso no canto inferior direito, em qualquer aba, com **Depois** e **Atualizar e
  reiniciar**. "Depois" esconde o aviso até a próxima consulta horária encontrar a versão de novo.
- Com transmissão local ativa, o aviso informa que a live cai; a instalação só ocorre por ação do usuário.
- O download mostra progresso e é verificado por inteiro (assinatura e versão) **antes** de chamar o instalador. Falha de
  rede ou de assinatura antes disso não executa instalador nem substitui o app atual. Não se promete *rollback*
  transacional de uma falha dentro do instalador NSIS.
- `requireSignedVersion` impede combinar uma assinatura válida de uma versão antiga com um número de versão novo.
- A assinatura é Minisign do Tauri. **Não é Authenticode**: o Windows pode exibir o aviso de editor desconhecido.

## Chaves

Gere o par com `npm run tauri --prefix apps/madscreen-app -- signer generate` e **guarde a chave privada e a senha em
local seguro**. Nunca regenere nem sobrescreva a chave entre releases: as versões instaladas confiam na chave pública
que já embarcaram. Só a chave pública é versionada (`tauri.conf.json`).

## Build e publicação

1. O CI (`.github/workflows/app.yml`) compila o instalador NSIS e gera `.exe` e `.exe.sig` quando
   `TAURI_SIGNING_PRIVATE_KEY` e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` estão configurados como segredos.
2. `tools/release/manifest.mjs` cria `latest.json` e `SHA256SUMS.txt`, escolhendo exatamente o instalador da versão
   configurada e recusando assinatura sem versão, com versão diferente ou com destino arbitrário.
3. O verificador `apps/madscreen-app/src-tauri/examples/verify-update.rs` valida a criptografia com a mesma biblioteca Minisign do plugin e compara
   a versão assinada à anunciada.
4. O fluxo `workflow_dispatch` com `publish_release=true` cria um *draft* no repositório de releases, envia os arquivos
   verificados e só então publica como *latest*. Ele não sobrescreve uma tag existente. Precisa de um segredo
   (`RELEASE_REPO_TOKEN`) com permissão *Contents: write* **somente** no repositório de releases. Se a promoção
   automática falhar, é possível publicar manualmente com os mesmos arquivos e verificador.

O endpoint de atualização fica em `tauri.conf.json` e aponta para o `latest.json` do repositório público de releases
(só instaladores, assinaturas, manifestos, hashes e notas; sem código-fonte). Se você publica as suas próprias builds,
troque esse endpoint **e** a chave pública.

## Link de download estável

O CI também publica `madscreen-setup.exe` (e `madscreen-setup.exe.sha256`) com os mesmos bytes do instalador
versionado. É o alias que as páginas de convite usam para baixar sempre a versão mais recente. O manifesto do updater
continua usando o nome versionado.

## Primeira instalação e limites

- Builds anteriores à chave pública final não conseguem se atualizar por este mecanismo; instale a primeira versão
  assinada manualmente uma vez.
- Upgrade entre versões, preservação de conta e preferências e teste em PC limpo ainda dependem de validação
  registrada; veja o [roadmap](roadmap.md).
