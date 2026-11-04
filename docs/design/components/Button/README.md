# Button

Botão com contorno, preenchido só na ação principal da região, com rótulo em peso 600.

- `.btn` padrão: contorno `border-strong`, raio `r-md`. `.btn.primary` (fundo `accent`) aparece uma vez por região: Compartilhar tela, Assistir, Atualizar e reiniciar.
- `.btn.ghost` para ações leves do cabeçalho (Convidar, Depois); com `aria-pressed="true"` marca um painel aberto. `.btn.danger` para destrutivas (borda `text`, inverte no hover).
- `.btn.over` é o botão secundário sobre vídeo (fundo `overlay`).
- Tamanhos: padrão 38px, `.sm` 30px, `.lg` 46px. `.icon-only` exige `aria-label`.
- O consumidor fornece: rótulo em pt-BR que diz o que acontece, ícone opcional antes do texto, `disabled` quando indisponível.
