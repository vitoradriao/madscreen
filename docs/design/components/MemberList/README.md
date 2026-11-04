# MemberList

Painel de membros à direita (`members-w`, 248px), em lista vertical agrupada, como no Discord, mas em preto e branco.

- Grupos com eyebrow e contagem: Transmitindo, Online, Offline. Quem está ao vivo vem primeiro.
- Cada membro: avatar com selo de estado, nome em `name` com o papel (`.role`) e uma linha de atividade em `activity`, com reticências.
- Hover clareia a linha (`surface-3`) e mostra ⋯ para quem pode gerenciar (dono/admin).
- O painel define `--ground` para o anel dos selos.
- Requer dados novos do diretório: "Assistindo …" e "Visto há …". Sem eles, use "Online" e "Offline".
