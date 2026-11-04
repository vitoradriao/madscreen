# Avatar

Círculo com iniciais (ou foto em escala de cinza) e um selo de estado no canto, sem usar cor.

- Selos: ícone de transmissão = ao vivo; ícone de pausa = transmissão pausada; ponto cheio `accent` = online; anel vazado `text-3` = offline.
- O anel do selo usa `--ground`, a cor do fundo onde o avatar está (`bg-sidebar` no painel, `surface-3` na linha em hover). Defina `--ground` no contêiner.
- `avatar` 40px na lista; `.sm` 32px; `.ring` (borda `text`) sobre a miniatura.
- O estado também aparece em texto na linha do membro ("Ao vivo · …").
