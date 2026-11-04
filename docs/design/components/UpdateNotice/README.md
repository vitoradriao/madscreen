# UpdateNotice

Aviso flutuante no canto inferior direito quando sai versão nova: diz a versão e oferece Atualizar e reiniciar.

- Estados: disponível (Depois / Atualizar e reiniciar), baixando (barra de progresso `accent` sobre `surface-4`), com live ativa (avisa que a live cai e oferece adiar até parar) e erro (borda de 2px em `text`).
- Aparece por cima do conteúdo com `shadow-floating`, 24px acima e à esquerda dos cantos, sem cobrir o painel de membros. "Depois" esconde até a próxima verificação ou o próximo início do app.
- Só instala por ação do usuário, depois de o atualizador validar assinatura e versão.
- O consumidor fornece: versão disponível e atual, estado e progresso, e se há live ativa.
