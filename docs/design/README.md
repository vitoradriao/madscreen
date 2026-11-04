madscreen compartilha a tela e o som de um aplicativo com amigos em servidores privados, sem pesar no jogo. A interface põe **o vídeo em primeiro plano**: as lives ocupam a largura como miniaturas grandes, e tudo ao redor é preto, branco e cinza. A única coisa colorida na tela é a imagem que alguém está compartilhando.

## Princípios

- **O vídeo manda.** As lives são o maior elemento da tela. Navegação e membros ficam nas bordas, finos e discretos.
- **Monocromia sempre.** Nenhum matiz na interface: nem para erro, sucesso, links ou marca. Estado é **palavra, ícone, peso, espessura de borda e inversão**.
- **Um destaque por região.** `accent` (branco no Grafite, preto no Papel) marca a ação principal, o AO VIVO, o servidor e a aba atuais. Se dois elementos disputam o `accent`, um está errado.
- **O jogo vem primeiro.** Nada pisca além do ponto do AO VIVO; animações de 120–200ms somem com `prefers-reduced-motion`.

## Conteúdo e voz

- Português do Brasil, **você**, frases curtas, sem exclamação e sem emoji.
- Botões dizem o que acontece: "Compartilhar tela", "Assistir", "Atualizar e reiniciar", "Apagar servidor". Nunca "OK" ou "Sim".
- Caixa de frase em títulos e botões. Caixa alta só em eyebrow (TRANSMITINDO — 2), badges (AO VIVO, PAUSADO) e papéis (DONO, ADMIN).
- Cabeçalho de grupo com travessão e contagem: "Online — 3". Metadados com ponto médio: "Jogo de demonstração · 1080p 60 FPS".
- Linha de atividade do membro começa pelo estado: "Ao vivo · …", "Pausado · …", "Assistindo **Bruno**", "Visto há 2 h".
- Erros dizem o que houve e o que fazer: "Não foi possível carregar os servidores. Verifique sua conexão."
- Prévia própria sempre avisa que é muda: "Prévia sem som".

## Cor

Três temas com os mesmos nomes de token: **Grafite** (`dark`, o padrão), **Papel** (`light`) e **Alto contraste** (`contrast`).

- Degraus do mais fundo ao mais alto: `bg-rail` → `bg` → `bg-sidebar` → `surface-1` → `surface-2` → `surface-3` → `surface-4`. Separe camadas pelo degrau e por `border` (linha fina), não por sombra.
- Texto: `text` para nomes e títulos, `text-2` para atividade e apoio, `text-3` para metadados e cabeçalhos de grupo. Todos passam de 4,5:1 sobre `bg` até `surface-4` no Grafite e no Papel, e de 7:1 no Alto contraste.
- Sobre `accent` use `accent-fg`, nunca branco ou preto literais.
- Controles usam `border-strong` (≥3:1). `border` é decorativa, exceto no Alto contraste, onde passa de 3:1 e faz a separação.
- **Tudo que fica sobre vídeo** (`.tile`, `.stage`) usa `overlay` para pílulas e `scrim` para o degradê da base, e no tema Papel leva `data-theme="dark"`: o cromo sobre imagem é sempre escuro.
- **Alto contraste** é um tema a mais: fundo preto puro, texto a 7:1 (WCAG AAA), bordas que separam superfícies, sem sombra. Ofereça-o em Conta.
- Fotos de avatar recebem `grayscale(1)`.

## Tipografia

Uma família: **Inter** (variável 100–900, `fonts/inter.woff2`) com `font-feature-settings: "cv11", "ss03"`; mono do sistema para HUD, diagnóstico e códigos.

- Títulos e nomes em **peso 600**: `display` 40px uma vez na entrada; `h1` 22px por vista, com o subtítulo em `text-2` na mesma linha ("Ao vivo agora · 2 transmissões em # Geral"); `h2` 18px em diálogos; `name` 15px na lista de membros e 16px sobre a miniatura.
- `body` 14px; `activity` 13px com reticências; `eyebrow` 12px; `badge` 11px peso 700.
- Números que mudam usam algarismos tabulares; métricas grandes usam `stat` (44px, peso 300).

## Estrutura

Três colunas e um cabeçalho:

1. **Barra de servidores** (`rail-w` 64px, `bg-rail`): marca **m.**, um botão por servidor (o atual em `accent`, com barra de 4px à esquerda; ponto `accent` = live em outro servidor), `+` tracejado e o seu avatar no rodapé, que abre Conta.
2. **Cabeçalho** (`topbar-h` 64px): servidor com menu (renomear, sair, apagar), **canais como abas** (a atual sublinhada em `accent`, com a contagem de lives), engrenagem de Opções do canal e, à direita, membros, Convidar e a ação primária. A primária é Compartilhar tela, ou Minha transmissão enquanto você está ao vivo.
3. **Conteúdo** (`bg`, margem `sp-6` 28px): a grade de miniaturas e, abaixo, a faixa tracejada "Compartilhar tela".
4. **Painel de membros** (`members-w` 248px, `bg-sidebar`): lista vertical agrupada em Transmitindo, Online e Offline.

Espaço: `sp-1` 4 · `sp-2` 8 · `sp-3` 12 · `sp-4` 16 · `sp-5` 20 · `sp-6` 28 · `sp-7` 48. Raios: `r-sm` 6, `r-md` 10 (botões, servidores, linhas), `r-lg` 14 (miniaturas, palco, cartões), `r-pill`. Sombra só no que flutua (`shadow-floating`: menus, aviso de atualização, player compacto).

## Estados e interação

- **Foco:** outline sólido de 2px em `focus`, afastado 2px, em todo elemento interativo.
- **Hover:** um degrau acima (`surface-3`); linhas de membro mostram ⋯ para quem pode gerenciar.
- **Selecionado:** inversão para `accent` (servidor, segmentado), sublinhado `accent` (aba) ou contorno duplo `accent` (opções e cartões do seletor).
- **Presença** no selo do avatar, sem cor: ícone de transmissão = ao vivo; pausa = pausado; ponto cheio = online; anel vazado = offline. Sempre repetida em texto na linha de atividade.
- **Erro:** borda de 2px em `text`, ícone de alerta e frase. **Transitório** (Iniciando, Reconectando): badge contornado.
- **Desabilitado:** opacidade `disabled` (0,4) com nota quando o motivo não for óbvio.
- Atalhos do player: M som, F tela cheia, D trocar destaque.

## Atualizações

O app verifica se há versão nova ao abrir e depois a cada hora. Quando há, mostra o **UpdateNotice** no canto inferior direito: "madscreen 0.1.7 disponível", com **Depois** e **Atualizar e reiniciar**. Baixar mostra progresso. Se você estiver ao vivo, o aviso explica que a live cai e oferece esperar até você parar. Nada é instalado sem o clique, e o atualizador valida assinatura e versão antes.

## Assistir e player

- **Uma live:** o vídeo ocupa o conteúdo todo e o painel de membros recolhe. Barras sobre o vídeo (somem sem mouse): em cima, nome, AO VIVO, fonte e espectadores; embaixo, som e volume, números em mono, Ajuda, mini-player e tela cheia. Acima do vídeo: "Assistindo · 1 de 2", Adicionar transmissão, Sair da transmissão.
- **Duas lives:** Segmented Destaque (7fr/3fr) ou Lado a lado, Trocar destaque (D) e Som só no destaque. A tela selecionada tem contorno `accent` de 2px e recebe a próxima transmissão aberta; um aviso diz isso.
- **Detalhes e ajuda:** painel de 320px no lugar dos membros, com números, observações com estado em palavra (NORMAL, PARADA) e Copiar diagnóstico como ação principal.
- **Player compacto:** cartão de 340px no canto do conteúdo quando você navega com uma live aberta. **Mini-player:** janela de 640×360 sem moldura, com Fixar, Controles e Voltar sobre o vídeo. **Tela cheia:** o mesmo player, só com as barras.

## Compartilhar e servidores

- **Minha transmissão** em três passos numerados (Canal, Fonte e som, Qualidade e FPS); o passo concluído ganha o número em `accent`. Compartilhar só habilita com fonte escolhida. "Nunca o microfone" fica dito no passo de fonte.
- **Seletor de fonte**: diálogo de 900px com Segmented Aplicativos / Telas, Atualizar, o app em foco como sugestão, grade de miniaturas e a seção Som (som do aplicativo; ou PC todo e aplicativos específicos, com TOCANDO). Rodapé com Usar o seletor do Windows e Continuar.
- **Diálogos**: título (pergunta, nas destrutivas), um campo ou a consequência, Cancelar e a ação com o mesmo verbo do título. Entrar por convite mostra o servidor de destino antes de entrar.
- **Menus**: nome do servidor no cabeçalho, engrenagem nas abas, ⋯ no membro. Só o dono renomeia e apaga o servidor e muda papéis; dono e admin gerenciam canais. Esconda o que a pessoa não pode fazer.

## Conta e serviço

- **Conta** abre pelo seu avatar no rodapé da barra de servidores (contornado enquanto aberta), com "Voltar para # Geral" no cabeçalho. Navegação de seções à esquerda; uma seção por cartão; interruptores (`.switch`) para o que liga e desliga.
- Seções: Conta, Aparência (tema), Avisos, Reprodução de vídeo, Atualizações e, só para administradores do serviço, Serviço de transmissão. Aparência é adição deste design.
- **Serviço de transmissão**: provedor das novas transmissões, uso estimado em barra, sondas com estado em palavra (SAUDÁVEL, SEM DADO) e a escolha Automático / Forçar Cloudflare / Forçar LiveKit, que vale só para transmissões novas.
- **Bandeja**: no primeiro fechamento pelo X, um diálogo mostra onde fica o ícone e o que Parar compartilhamento e Sair fazem.

## Estados de erro, vazio e espera

- **Valem para a tela toda** (diretório desconectado, falha ao carregar): faixa `.statusbar` logo abaixo do cabeçalho, com ícone, frase e uma ação. Erro engrossa a borda inferior para 2px em `text`. A tela continua utilizável; lives abertas não dependem do diretório.
- **Carregando**: esqueleto estático (`.skel`, `.skel-tile`) no lugar de miniaturas e membros. Sem animação.
- **Vazio**: EmptyState com a ação principal e, se houver, um atalho para onde há lives.
- **Falha numa ação**: o erro aparece logo acima do botão, com Copiar diagnóstico, e a ação vira Tentar de novo; nada do que a pessoa escolheu se perde.
- **Sobre o vídeo** (pausado, som bloqueado): aviso no centro (`.center-note`) sobre `overlay`. Vídeo pausado escurece a prévia.
- **De passagem** (Iniciando, Parando): badge contornado e a frase do que está acontecendo. Iniciando permite Cancelar.

## Entrada

- Tela de entrada: `display` e `lead` à esquerda, um cartão de acesso de 400–420px à direita, uma ação principal por vez.
- **Entrar sem Discord**: Segmented "Tenho um convite / Já tenho conta". Com convite: o servidor de destino aparece quando o convite é conferido, depois o nome de usuário (até 20 caracteres) e Criar conta. Com conta: nome de usuário, chave de recuperação (campo oculto, em mono) e Entrar. Continuar com Discord desce para botão contornado depois de um "ou".
- **Chave de recuperação**: mostrada uma única vez, numa caixa tracejada em mono que seleciona inteira, com Copiar chave e Guardei, continuar.
- **Convite antes de entrar**: o servidor de destino à esquerda e as formas de entrar à direita; o convite fica guardado durante o login.

## Iconografia

Ícones lineares próprios, 24×24, traço 1.5, pontas arredondadas, cor de `currentColor`, 18px (15px em `.sm`). No produto são SVG inline (`viewer/shared/icons.ts`); os arquivos do grupo Icons trazem os mesmos paths com traço `#8a8a8a`, só para visualização. `share`, `alert`, `more`, `chevron`, `eye`, `pause` e `download` foram desenhados para este sistema no mesmo traço e aparecem no componente Icon. Sem emoji.

## Marca

O nome é sempre **madscreen**, em minúsculas. A marca curta **m.** aparece no topo da barra de servidores, em Inter peso 800. O grupo Logos traz o glifo de monitor com play do código e recortes 1x das capturas ("m." e "m. madscreen"), sem vetor ainda: componha a marca em texto. O ícone atual do app (azul-violeta) ficou fora por não ser preto e branco.

## Componentes

CSS puro em `components/bundle.css`, sem framework. Carregue `tokens.css`, depois `bundle.css`, e marque `<html data-theme="dark">`, `"light"` ou `"contrast"`. O grupo **Telas** junta tudo em telas de 1280×800: Canal com duas lives (ScreenChannel), Minha transmissão (ScreenLive) e Entrada (ScreenLogin). O grupo **Telas — Assistir** traz Assistir vazio, uma live, duas lives, Detalhes e ajuda, player compacto, mini-player e tela cheia. O grupo **Telas — Compartilhar e servidores** traz Minha transmissão (vazia e pronta), o seletor nas duas abas, Nenhum servidor e as pranchas de diálogos e menus. O grupo **Telas — Conta** traz Conta, Atualizações, Serviço de transmissão e o aviso da bandeja. O grupo **Telas — Estados** traz reconexão, erro do diretório, canal vazio, falha ao compartilhar, live pausada, diagnóstico da live e a prancha de estados de passagem. O grupo **Telas — Entrada** traz Entrar sem Discord, entrar com chave, a chave de recuperação e o convite antes de entrar.
