# Documentação do madscreen

Este é o índice da documentação. Se você chegou agora, comece por **[Arquitetura](architecture.md)** para entender
as peças e por **[Compilar e testar](building.md)** para rodar o projeto.

> A documentação principal está em português do Brasil. Os documentos de referência técnica listados no fim
> trazem notas detalhadas de engenharia (medições, limites e decisões) e podem citar marcos antigos (`M0.x`) e
> números de issues do histórico interno do projeto, que não existem neste repositório.

## Usar

| Documento | Conteúdo |
| --- | --- |
| [README](../README.md) | O que é, recursos, instalação e requisitos |
| [Convites e servidores](convites.md) | Como convidar, entrar e organizar canais |
| [App desktop](app.md) | Abas, bandeja, sidecar, atualização e diagnósticos do app |

## Entender

| Documento | Conteúdo |
| --- | --- |
| [Arquitetura](architecture.md) | Componentes, fluxo de mídia, princípios de desempenho e privacidade |
| [Captura e encoders](encode.md) | WGC/Desktop Duplication, conversão D3D11, NVENC/AMF/QuickSync/x264, adaptação de taxa |
| [Transporte](transport.md) | Publicadores Cloudflare e LiveKit, recuperação de quadro-chave, estatísticas e medições |
| [Ferramenta de prévia](preview.md) | `madscreen-preview`: seletor WGC e contadores de FPS para validar a captura |
| [Design system](design/README.md) | Princípios visuais, temas, componentes e telas |

## Operar

| Documento | Conteúdo |
| --- | --- |
| [Rodar o seu servidor](self-hosting.md) | Cloudflare Worker, SFU, OAuth e LiveKit de reserva |
| [Autenticação e canais](cloudflare-auth.md) | Rotas, login Discord/Google, chave de recuperação, permissões |
| [SFU, LiveKit e *failover*](sfu.md) | Seleção de provedor, saúde, estimativa de uso e operação do LiveKit |
| [Atualizações assinadas](updater.md) | Chaves, manifesto, publicação e comportamento do cliente |
| [Runtime FFmpeg, licenças e patentes](runtime-distribution.md) | O que acompanha o instalador e quais obrigações isso cria |
| [Diagnóstico local](diagnostico.md) | Como coletar e interpretar dados de uma sessão com problema |

## Contribuir

| Documento | Conteúdo |
| --- | --- |
| [CONTRIBUTING](../CONTRIBUTING.md) | Fluxo, convenções de código e commits |
| [Compilar e testar](building.md) | Ambiente, comandos e verificações |
| [Roadmap e estado](roadmap.md) | O que está pronto, o que está sendo avaliado e o que falta |
| [SECURITY](../SECURITY.md) | Como reportar uma vulnerabilidade |
| [Notas de versão](releases/) | Histórico das versões 0.1.x |

## Como ler as medições

Neste projeto, compilar e passar em testes sintéticos **não** é o mesmo que ter desempenho qualificado. As metas de
FPS do jogo, latência vidro a vidro e sincronia A/V só são dadas como atingidas quando há medição reproduzível no
hardware citado (PresentMon 2.x, várias corridas, intervalo de confiança). Quando um documento descreve uma
medição, ele também diz em que máquina e com quais limites; números de um hardware não valem para outro.
