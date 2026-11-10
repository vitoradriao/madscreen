# Revisão das telas do madscreen — 9/nov/2026

Pedido: avançar até concluir a revisão de interface/UX de todas as superfícies
montadas no app e seus módulos compartilhados. Preservar as ações, os dados e
o trabalho local de outros fluxos. Publicação e instalação não fazem parte.

**Estado: revisão visual concluída em todas as telas existentes**, com os limites
da última seção. Interface revisada não equivale a transmissão qualificada.

## Matriz de cobertura

Capturas em `bench-results/ui-complete-2026-11-09/after/prints/` (75 PNGs,
1280×800 salvo indicação). Baseline em `before/` (63). Exemplos fictícios; vídeo
de canvas local; IPC e serviços simulados.

| Área | Telas e estados | Resultado | Evidência |
| --- | --- | --- | --- |
| Entrada | Discord, convite, conta com convite, entrada por chave, chave nova | Concluída. Título da chave de recuperação perdeu o contorno de foco programático (parecia campo de texto) | 01–03, 53 |
| Convite externo | Destino, baixar/abrir app, navegador, sem instalador, erro | Concluída sem defeito. Inspecionada no navegador por harness temporário (desktop e 375 px); sem PNG na galeria | Seção "Convite externo" |
| Diretório | Canal com lives, membros, vazio, sem servidor, reconectando, erro | Concluída. Lista de membros vira painel sobreposto abaixo de 960 px, começa fechada nessa largura e não fica mais com 17 px; nome do canal visível em 510 px | 04–07, 21, 22, 67, 69 |
| Administração de servidor | Criar/entrar, convidar, renomear, sair/apagar, canais, papéis | Concluída sem defeito novo | 08–20 |
| Compartilhar | Preparação, seletor, revisão, live, pausa, erro, iniciando, parando | Concluída (revisão própria anterior). Nesta rodada, Mbps com vírgula decimal | 23–30, 54, 65, 66 |
| Assistir | Vazio, uma live, duas, destaque, lado a lado, seleção e som | Concluída. Seletor marca a tela escolhida; Lado a lado mostra estado; uma moldura por tela; nome nunca some antes do estado do som; dica em uma linha | 31–37, 47, 71 |
| Ajuda do player | Abrir, fechar, trocar entre pessoas, diagnóstico | Concluída. Ajuda cobre só a área das telas e abre do lado oposto ao da pessoa, que continua visível | 33, 35, 72, 73 |
| Tela cheia | Controles, ajuda aberta, saída | Concluída. Ajuda flutua entre o nome e os controles: Sair, Ajuda e saída da tela cheia continuam visíveis | 48, 49 |
| Estados de recepção | Espera, conexão, reconexão, erro, encerrada, som bloqueado | Concluída por checks de comportamento; sem PNG por estado transitório | `browser-checks.log` |
| Player compacto | Uma/duas lives, recolhido, retorno, durante a Conta | Concluída. Nomes inteiros; em janela baixa as duas telas continuam empilhadas (antes ficavam espremidas lado a lado) | 38, 39, 74, 75 |
| Mini-player | Fixar, controles, retornar, aviso na janela principal | Concluída. Aviso "está no mini-player" ocupa o lugar do vídeo, centralizado, com espaço entre texto e botão | 50, 51 |
| Conta | Seções, foco, seleção preservada, temas, 800/510 px | Concluída. Título focado ao trocar de seção sem contorno de campo | 40–43, 55, 58, 61, 68, 70 |
| Atualizações | Disponível, aviso, live ativa, erro | Concluída. MB do download com vírgula decimal | 44, 64, 65 |
| Serviço de transmissão | Autorização, provedor, uso, saúde, perda de autorização | Concluída. Uso em formato pt-BR (`0,00 GB de 1.000 GB`); ao perder a autorização com a seção aberta, a Conta volta e o foco vai ao título | 45, 52, 75 |
| Bandeja | Aviso, continuar, ir para a bandeja | Concluída sem defeito | 46 |
| Temas | Grafite, Papel, Alto contraste | Concluída | 55–63 |
| Janela mínima 860×560 | Duas lives, ajuda trocada, compacto, Conta | Concluída. Capturas novas na galeria | 71–75 |

## Defeitos encontrados na inspeção final e correções

| Defeito demonstrado | Correção |
| --- | --- |
| Tela 1/Tela 2 iguais: a seleção só aparecia no texto | `aria-pressed` ganha preenchimento; com uma live o botão só identifica e não tem estado |
| Lado a lado sem estado visível | Botão pressionado com fundo e borda próprios |
| Duas molduras na tela selecionada; anel em live única | Uma moldura; nenhuma com uma live |
| Nome cortado para "B…"/"D…" no compacto e na tela menor | Estado do som encolhe antes do nome; some no compacto |
| Ajuda cobria Trocar destaque/Lado a lado e a própria pessoa | Ajuda dentro da grade e do lado oposto; troca entre pessoas exercitada |
| Ajuda em tela cheia cobria Sair e a saída da tela cheia | Painel flutuante entre o topo e os controles |
| Estatísticas forçavam quebra dos controles em telas estreitas | Estatísticas saem abaixo de 620 px de tela (seguem em Detalhes técnicos) |
| `0.00 GB de 1000 GB`, `0.0 Mbps`, `8.0`, `x.x MB` | Formato pt-BR |
| Títulos com contorno de campo após foco por script (Conta, chave) | Sem contorno em títulos `tabindex=-1` |
| Aviso do mini-player colado ao botão, no canto | Placeholder centralizado na área do vídeo |
| Membros com 17 px e barra horizontal em 800/510 px | Painel sobreposto abaixo de 960 px, fechado por padrão nessa largura |
| Canal reduzido a "#" em 510 px | Compartilhar tela só com ícone abaixo de 600 px (nome acessível mantido) |
| Compacto com duas lives ilegível em 860×560 | Mantém empilhado no compacto |
| Perda de autorização deixava o foco no corpo da página | Foco no título da seção Conta |

## Convite externo

`createInviteLanding` montado com cliente fictício e o CSS do cliente web
(`tokens.css`, `redesign.css`, `web.css`). Estados: convite válido com destino,
instalador indisponível (Assistir pelo navegador vira a ação principal) e convite
expirado. Desktop e 375 px sem corte ou rolagem lateral. Contraste do aviso de
erro medido: `#f5f5f5` sobre `#1a1a1a`. Harness descartado depois da inspeção.

## Verificação

- `npm run build` em `apps/madscreen-app`: aprovado (aviso conhecido do chunk LiveKit).
- `node viewer/browser-test.mjs`: **242 checks**, zero erros de navegador.
  Novos: ajuda da Tela 1 só sobre a grade e com uma tela marcada; troca para a
  Tela 2 fecha a anterior e abre do outro lado. A verificação de menus em
  860/510 px abre a lista de membros pelo botão e exige largura ≥ 200 px.
- `npm test` em `viewer`: 41/41.
- Galeria: 75 capturas, exit 0. A galeria agora falha se houver estouro
  horizontal em 860×560, membros espremidos, nome do canal escondido ou ajuda
  de uma pessoa cobrindo a própria tela.
- Perda de autorização do admin exercitada manualmente no servidor de inspeção.
- Logs: `browser-checks.log`, `viewer-test.log`, `app-build.log`,
  `gallery-after.log` em `bench-results/ui-complete-2026-11-09/`.

## Fora do escopo e limites

- OAuth externo, seletor do Windows, menu nativo da bandeja, instalador e
  moldura nativa continuam superfícies do provedor/sistema. A barra de título
  própria foi validada em outro fluxo.
- Amigos legados não são uma aba montada e não foram reintroduzidos.
- Fixtures não qualificam captura, áudio, SFU, FPS do jogo, latência,
  posicionamento real do mini-player nem compatibilidade em campo.
- Sem NVDA, DPI físico ou zoom de 200% qualificados nesta rodada.
- Mantido sem mudança: o botão Instalar e reiniciar (Conta) e o aviso Atualizar
  e reiniciar nomeiam a mesma ação de formas diferentes; o rótulo da Conta está
  em notas de versão publicadas, então a unificação fica para quem decidir o texto.
- O fixture de compartilhamento mostra "Canal da transmissão não informado" por
  dado incompleto da simulação, não por defeito do produto.
