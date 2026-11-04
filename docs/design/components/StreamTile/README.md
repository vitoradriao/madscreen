# StreamTile

A live como miniatura 16:9: imagem de ponta a ponta, estado e espectadores no topo, quem e o quê na base, sobre um degradê.

- Topo: `.badge.live` (ou `.badge.over` Pausado) à esquerda e `.pill` de espectadores à direita.
- Base: avatar `.ring`, nome em 16px peso 600, fonte e qualidade em `text-2`, botão Assistir (`.btn.primary`; `.btn.over` quando pausada).
- No tema Papel o `.tile` leva `data-theme="dark"`, porque fica sobre imagem.
- Grade `.tiles` com gap `sp-5`; cada miniatura com no mínimo 340px.
- O consumidor fornece: miniatura (ou nada, que mostra `surface-2`), nome, fonte, qualidade, espectadores e o handler de Assistir.
