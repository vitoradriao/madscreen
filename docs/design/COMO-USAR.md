# Design system do madscreen — como usar

Cópia do design system publicado em claude.ai (privado), para quem vai aplicar o
redesign no código. Direção aprovada em 03/11/2026: **vídeo em primeiro plano**,
só preto, branco e cinza, membros em lista vertical à direita.

Estado em 03/11/2026: **aplicado no código do app**, incluindo Aparência em Conta.
Build, regressões de navegador e inspeção visual com dados fictícios executados.
Publicado na [0.1.13](https://github.com/vitoradriao/madscreen-releases/releases/tag/v0.1.13),
sem alteração do app instalado pelo agente. Cobertura, evidências e limites em
[`IMPLEMENTACAO.md`](IMPLEMENTACAO.md).

## O que tem aqui

| Caminho | O que é |
| --- | --- |
| `README.md` | Guia de marca e regras de uso (cor, tipografia, estrutura, estados, voz). Leia primeiro. |
| `tokens.json` | Fonte dos valores: cores nos temas `dark` (Grafite, padrão), `light` (Papel) e `contrast` (Alto contraste), tipografia, espaço, raio, sombra, tamanhos. |
| `tokens.css` | `tokens.json` compilado em variáveis CSS. Gerado; não edite à mão. |
| `components/bundle.css` | CSS dos componentes, com os mesmos nomes de classe do app sempre que existiam (`.btn`, `.badge.live`, `.stage`, `.pk-card`…). |
| `components/<Nome>/README.md` | Regras de cada componente. |
| `components/<Nome>/preview.html` | HTML de exemplo. Espera `tokens.css` e `bundle.css` já carregados (no claude.ai a página injeta os dois). |
| `components/Screen*/` | As 32 telas completas, 1280×800. |
| `telas/*.png` | Imagem de cada tela no tema Grafite, para comparar o resultado. |
| `assets/` | Ícones (mesmos paths de `viewer/shared/icons.ts`) e logos. |

A fonte é a Inter que já está em `apps/madscreen-app/public/fonts/inter.woff2`.

## Tela do design → captura do inventário (`GUIA.md` do pacote de redesign)

| Tela | Capturas |
| --- | --- |
| ScreenLogin, ScreenLoginAlt, ScreenLoginKey, ScreenRecoveryKey, ScreenInviteLanding | 1, 2, 46, 3; convite aberto antes do login |
| ScreenChannel, ScreenChannelEmpty, ScreenNoServer | 4, 5, 6, 7, 8 |
| ScreenDialogs, ScreenMenus | 9–20 |
| ScreenReconnecting, ScreenDirectoryError | 21, 22 |
| ScreenShareEmpty, ScreenPickerApps, ScreenPickerScreens, ScreenShareReview | 23, 24, 25, 26 |
| ScreenLive, ScreenLiveDiagnostics, ScreenLivePaused, ScreenShareFailed | 27, 28/47, 29, 30 |
| ScreenWatchEmpty, ScreenWatchOne, ScreenWatchHelp, ScreenWatchTwo | 31, 32, 33, 34/35 |
| ScreenCompactPlayer, ScreenMiniPlayer, ScreenFullscreen | 36, 43/44, 41/42 |
| ScreenAccount, ScreenAccountUpdate, ScreenAccountAdmin, ScreenTray | 37, 38, 39/45, 40 |
| ScreenTransient | estados sem captura: carregando, iniciando, parando, som bloqueado, fonte fechada, atualização com falha |

## Regras para aplicar

- Não remova nem invente funcionalidade: toda ação de hoje continua existindo, mesmo que mude de lugar (ex.: canais viram abas no cabeçalho; "Telas iguais" vira "Lado a lado").
- Uma cor de destaque por região (`accent`). Estado é palavra, ícone, peso e borda, nunca cor.
- Tudo que fica sobre vídeo (`.stage`, `.tile`) leva `data-theme="dark"` no tema Papel.
- Valores vêm de `tokens.json`/`tokens.css`; não escreva cores ou tamanhos soltos.
- Aplique por etapas (estrutura, canal, assistir, compartilhar, diálogos, conta, estados, entrada) e compare cada uma com o PNG.

## Pendências e decisões abertas

| Item | Situação |
| --- | --- |
| Seção **Aparência** (troca de tema) em Conta | Incluída: Grafite, Papel e Alto contraste, com preferência local persistida. |
| "Assistindo <nome>" e "Visto há <tempo>" na lista de membros | O diretório não envia esses dados hoje. Sem eles, use "Online" e "Offline". |
| Canal de destino no convite ("você entra em # Geral") | `previewInvite` já fornece canal opcional. Exibido somente quando a resposta confirma esse dado. |
| Cancelar durante "Iniciando" | Usa a ação existente `stopShare`; o botão recebe o rótulo Cancelar nesse estado. |
| Janela compartilhada fechada durante a live | O design só mostra o aviso; seguir o comportamento atual do app. |
| Marca "m." | Sem vetor; os PNGs em `assets/Logos` são recortes 1x das capturas. Compor em texto. |

## Manutenção no código

- `node tools/ui/sync-design.mjs` copia os tokens/componentes para
  `viewer/shared/design-tokens.css` e `design-components.css`, ajustando apenas o
  caminho da Inter local. Os arquivos de referência deste diretório são preservados.
- `viewer/shared/redesign.css` adapta as classes/estrutura funcionais às referências;
  `tokens.css` mantém a compatibilidade anterior em uma camada de menor prioridade.
- `node tools/ui/capture-redesign.mjs` reproduz a galeria local com backend, IPC e
  mídia fictícios, em 1280×800 e janelas menores. Não consulta produção.
