# Redesign aplicado — 3/nov/2026

Base `ddb51f0`, na `main`. O pedido inclui aplicar o design aprovado ao produto e
incluir Aparência em Conta. Posteriormente enviada e publicada na
[0.1.13](https://github.com/vitoradriao/madscreen-releases/releases/tag/v0.1.13),
sem instalação pelo agente. Evidência da publicação.

## Cobertura

| Referências | Aplicação funcional |
| --- | --- |
| ScreenChannel, ScreenChannelEmpty, ScreenNoServer | Rail de 64 px, canais no cabeçalho de 64 px, miniaturas grandes, compartilhar e membros à direita; contador das abas indica lives. |
| ScreenDialogs, ScreenMenus | Ações existentes, papéis e confirmações preservados; ações de confirmação usam o verbo da operação. |
| ScreenReconnecting, ScreenDirectoryError, ScreenTransient | Avisos, reconexão, tentativa de carregar novamente, esqueleto estático e estados existentes do supervisor/updater. |
| ScreenShareEmpty, ScreenShareReview | Canal, fonte/som e qualidade/FPS em três passos. Escolha de fonte no segundo passo; Compartilhar desabilitado até ter fonte e canal. |
| ScreenPickerApps, ScreenPickerScreens | Seletor de 900 px com abas, sugestão compacta, grade, som e seletor do Windows. Mantido o clique na fonte para continuar/início rápido autorizado pelo canal. |
| ScreenLive, ScreenLiveDiagnostics, ScreenLivePaused, ScreenShareFailed | Qualidade, diagnóstico, pausa, mudo, erros e prévia própria muda preservados; Cancelar durante início usa o Stop existente. |
| ScreenWatchEmpty, ScreenWatchOne, ScreenWatchHelp, ScreenWatchTwo | Uma/duas sessões independentes; identidade e controles sobre o vídeo, ajuda lateral, Lado a lado, destaque, volume e atalhos. Membros recolhem ao entrar e podem ser reabertos. |
| ScreenCompactPlayer, ScreenMiniPlayer, ScreenFullscreen | Compacto de 340 px; mini-player inicial 640×360 com controles sobre o vídeo; fullscreen mantém o mesmo palco/MediaStream. Geometria salva continua respeitada. |
| ScreenAccount, ScreenAccountUpdate, ScreenAccountAdmin, ScreenTray | Conta pelo avatar, navegação de seções, interruptores reais, três temas, updater existente, uso estimado/sondas e aviso da bandeja. Administração aparece somente quando autorizada. |
| ScreenLogin, ScreenLoginAlt, ScreenLoginKey, ScreenRecoveryKey, ScreenInviteLanding | Marca em texto, entrada em duas colunas, acesso alternativo prioritário quando aberto, chave oculta/selecionável conforme o contexto, destino de convite confirmado pelo serviço. |

Os tokens/componentes de `docs/design/` foram copiados pelo sincronizador. A Inter
fica empacotada localmente; não há dependência de CDN. Grafite é o padrão; Papel e
Alto contraste persistem no dispositivo. No Papel, o cromo sobre o vídeo continua
escuro; em Alto contraste ele recebe os tokens desse tema. Falha ao salvar conserva
a escolha anterior e informa o erro.

## Verificação

- `npm run build --prefix apps/madscreen-app`: TypeScript e Vite aprovados.
- `npm test --prefix viewer`: 35 testes aprovados e controlador de atualização aprovado.
- `npm run test:browser --prefix viewer`: 198 verificações aprovadas, zero erros JavaScript;
  regressões de navegação, foco, permissões,
  convites, fontes/perfis, pausa/mudo, updater, temas, duas lives, compactação,
  mini-player, fullscreen, proporção e redimensionamento. Sem serviços de mídia remotos.
- `node tools/ui/audit-theme.mjs`: 96 pares nos três temas; texto mínimo 5,162:1,
  controles mínimo 3,379:1; Alto contraste exige 7:1 para texto. Nenhuma cor literal
  nas adaptações de tela.
- `cargo fmt --manifest-path apps/madscreen-app/src-tauri/Cargo.toml --all -- --check`,
  Clippy Tauri offline com warnings negados e teste `miniplayer::tests`: aprovados.
- Build e typecheck Worker/viewer aprovados. O empacotamento web e os dois harnesses
  que incorporam CSS agora resolvem os imports/fontes compartilhados; o bundle gerado
  foi conferido sem `@import` remanescente, com Inter embutida. Sem deploy ou mídia real.
- `node tools/ui/capture-redesign.mjs`: 63 capturas locais, três temas e janelas de
  1280/800/510 px, sem exceções JavaScript. Seleção inspecionada visualmente;
  evidências versionadas em [`aplicado/`](aplicado/). Galeria completa ignorada em
  `bench-results/redesign-2026-11-03/index.html`.

Exemplos do produto com dados fictícios: [canal Grafite](aplicado/canal-grafite.png),
[Papel](aplicado/canal-papel.png), [Alto contraste](aplicado/canal-alto-contraste.png),
[Conta e Aparência](aplicado/conta-aparencia.png),
[Compartilhar](aplicado/compartilhar.png), [player no Papel](aplicado/player-papel.png).

Aviso preexistente de chunk LiveKit acima de 500 kB permanece. A verificação é do
código/Chrome com backend, IPC e mídia fictícios; não qualifica mídia remota,
desempenho, WebView2 instalado nem posicionamento nativo do mini-player.

## Dados e limites da adaptação

- O diretório não envia “assistindo”, última atividade, pausa remota nem imagem de
  prévia das lives. A interface usa os dados existentes: Ao vivo, Online e Offline,
  título e espectadores; a capa é neutra. Não inicia captura ou assinaturas extras
  para reproduzir o conteúdo ilustrativo dos mockups.
- O destino do convite aparece somente após consulta válida; canal é opcional.
- Fonte fechada segue o comportamento do supervisor, sem escolher outra fonte.
- A prévia própria continua voluntária e sem áudio. A segurança do updater e os
  contratos de captura/encode/transporte não foram substituídos pelo redesign.

A distribuição foi concluída na 0.1.13; a 0.1.12 foi preservada. O app instalado
não foi alterado pelo agente. A instalação da atualização continua por ação do usuário.
