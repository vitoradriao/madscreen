# Compartilhar — revisão visual e UX — 9/nov/2026

## Escopo e método

Continuidade após entrada, convites e primeiro servidor: preparação, seletor de
imagem/som, revisão, qualidade, controles da live e recuperação de erro.
Alterações locais, sem commit próprio, push, deploy ou instalação. Não integram
o instalador 0.1.17 preparado em outro fluxo.

Impeccable 4.8.1: avaliações independentes A (`/root/design_review`) e B
(`/root/technical_review`), sequenciais para respeitar o limite de dois agentes.
A inspecionou a galeria e capturas sem acesso ao detector; B executou o detector
e exercitou navegador, foco, nomes e amostras de contraste.

Nota heurística **antes das mudanças visuais: 25/40**. Não foi atribuída nota
posterior nem alegado ganho de desempenho.

| Heurística | Antes |
| --- | ---: |
| Visibilidade do estado | 3/4 |
| Linguagem próxima do usuário | 3/4 |
| Controle e liberdade | 3/4 |
| Consistência | 3/4 |
| Prevenção de erros | 2/4 |
| Reconhecimento | 2/4 |
| Eficiência e flexibilidade | 3/4 |
| Estética e minimalismo | 1/4 |
| Recuperação de erros | 3/4 |
| Ajuda contextual | 2/4 |

A identidade e os avisos de privacidade eram coerentes, mas a prévia desligada
dominava a tela; as decisões ficavam numa coluna secundária ou abaixo da dobra.
A avaliação A apontou carga cognitiva alta (6/8 falhas na lista da referência).

## Mudanças implementadas

| Problema | Resultado |
| --- | --- |
| P1: palco vazio ocupa o lugar da tarefa | Canal, imagem/som, qualidade e confirmação formam uma sequência linear. Vídeo só ocupa espaço quando a prévia é solicitada. |
| P1: som aparece depois dos cartões que encerram o seletor | Som vem antes das fontes, na composição e no teclado. A consequência do clique distingue revisão, início rápido e troca ao vivo. |
| P1: pouca área útil em janela baixa | Uma região rolável reúne som e fontes. Sem miniatura, as opções viram linhas em alturas reduzidas, com nome e dimensões identificáveis. |
| P2: qualidade extensa e prematura | Resumo de resolução/FPS e “Alterar qualidade”. Opções, restrições e avisos permanecem no painel expandido, com explicação de Nativa/FPS. |
| P2: diagnóstico compete com ações principais | Copiar diagnóstico fica em região secundária, fora do cabeçalho e das seções recolhidas. Pausa, mudo, troca e prévia ficam junto da imagem/som. Aplicar qualidade aparece somente após edição. |

Mantidas as correções funcionais anteriores: foco em “Som do PC todo”; erro ao
iniciar conserva fonte/som/qualidade e oferece nova tentativa explícita.
Revalidação de janela, sessão e canal, cancelamento, início rápido e limpeza da
escolha após encerramento normal continuam cobertos pelos testes.

Código: `apps/madscreen-app/src/views/share.ts`, `picker.ts`, `picker.css` e
regras locais do share em `apps/madscreen-app/src/styles.css`. Nenhum token ou
arquivo global de identidade compartilhado foi alterado por esta entrega.

## Evidência e checks

- **74 verificações focadas aprovadas**, importando as fixtures versionadas de
  fluxo, resolução, alterações e histórico; zero exceções do navegador.
- `npm run build` em `apps/madscreen-app`: TypeScript e Vite aprovados.
- Inspeção no browser nativo: preparação, revisão, qualidade expandida,
  Aplicativos/Telas, live fictícia e abertura/fechamento da área de prévia.
  Grafite, Papel e Alto contraste inspecionados; preferência restaurada.
- Mínimo 860×560: Compartilhar inteiro na primeira tela (topo 478,8, fundo 516,8).
  Seletor com fonte identificável e sem overflow horizontal. Stress 640×480
  também manteve o cartão da tela identificado.
- Live fictícia em 1280×720: conteúdo rolável de 1016 px na avaliação B para
  758 px, com 656 px úteis nos dois casos. Evidência de composição deste
  fixture, sem medição de transmissão ou desempenho.
- Espaço alternou “Som do PC todo” sem perder foco; Escape fechou e devolveu
  foco ao acionador. Som precede fontes na árvore e no DOM.
- Detector B: engine 0.1.19, saída `[]`, exit 0. Cobertura do HTML gerado por
  TS não comprovada. Amostras anteriores de contraste: 9,92:1, 10,59:1,
  7,46:1 e 21:1. Cores preservadas; não é auditoria integral de acessibilidade.
- Sem overlay: avaliação do browser é somente leitura. CSP intacta. Servidor
  próprio encerrado e viewport restaurado.

Capturas locais (ignoradas pelo Git):

- Antes: revisão.
- Depois: revisão.
- Seletor reorganizado.
- Seletor no mínimo.
- Revisão no mínimo.
- Papel.
- Alto contraste.

Resultados: `bench-results/share-ux-2026-11-09/focused-checks.json` e
`after/layout-metrics.json`. Runner focado local em
`bench-results/share-ui-2026-11-09/checks.mjs`, usando fixtures versionadas.
Snapshot da crítica arquivado em `.impeccable/critique/`; backlog encerrado
após tratar os cinco itens, preservando o histórico da nota original.

## Limites e próxima etapa

Implementado e exercitado com componentes reais e IPC fictício. Captura nativa,
WASAPI, SFU, leitor de tela real e desempenho em jogo não qualificados. A abertura
da área de prévia foi exercitada; o mock não forneceu mídia para essa recepção.
Canal ausente na live fictícia é limite registrado da simulação, sem classificá-lo
como bug de produto.

Próxima etapa independente: revisão de Assistir, uma/duas lives, ajuda,
mini-player e tela cheia. Nenhuma issue associada ou encerrada.
