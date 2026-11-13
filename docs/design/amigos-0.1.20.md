# Tela de Amigos — refinamento da 0.1.20

Pedido do responsável em 10/nov/2026: melhorar a composição da tela provisória sem trocar a identidade visual do aplicativo. Manter paleta monocromática, tipografia, raio, botões e ícones lineares do sistema existente. Textos funcionais e diretos; sem slogans.

Modo Operate. O cabeçalho do workspace identifica Amigos uma única vez. Formulário de pedido e convite pessoal ficam próximos, em duas colunas no desktop e empilhados em janela estreita. O gerenciador permanece junto ao convite; filtros e busca formam uma barra da lista. Perfil é complementar, limitado a 280px; sem perfil, a lista ocupa a largura disponível. Ícones de Amigos, adicionar servidor, convite e busca seguem a biblioteca SVG local.

Conta vazia explica como adicionar alguém. Busca sem resultado e filtros vazios apresentam mensagens próprias. O estado de conexão recebe o evento real do contêiner. A mesma implementação é compartilhada pelo app e pelo viewer web; o viewer deixa de centralizar todo o workspace em um bloco limitado.

Validação local: build TypeScript/Vite do app, 45 testes do viewer e detector Impeccable sem achados. Conferidos estado vazio/preenchido, busca sem resultado, temas Grafite/Papel/Contraste e largura estreita sem overflow horizontal (380px de conteúdo para 380px úteis). Demonstração usa contas fictícias; não é teste de mídia nem de instalador. Capturas locais em `.impeccable/review/`.

Sem alteração dos tokens globais ou da identidade. Nenhum mock gerado ou slogan da direção descartada integra a interface. Sem novo pacote, instalação ou publicação neste refinamento.
