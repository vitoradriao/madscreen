# Política de segurança

## Versões suportadas

O projeto está em desenvolvimento pré-1.0: correções de segurança são feitas na **versão mais recente** publicada em
[madscreen-releases](https://github.com/vitoradriao/madscreen-releases/releases/latest). Versões antigas não recebem
*backports*; o app avisa quando há atualização assinada.

## Como reportar uma vulnerabilidade

**Não abra uma issue pública.** Use o relato privado do GitHub: aba **Security → Report a vulnerability** deste
repositório. Inclua, se possível:

- o que é afetado (app, engine, Worker, viewer web, instalador, atualização) e a versão;
- os passos para reproduzir e o impacto esperado;
- se há prova de conceito, descreva-a sem dados reais de terceiros.

Respondemos assim que possível, normalmente em alguns dias, e coordenamos a divulgação depois da correção. O projeto é
mantido sem fins lucrativos e não oferece recompensa financeira.

## O que é de interesse

Contornos de autenticação ou autorização no Worker (servidores, canais, convites, revogação), vazamento de segredos
ou de mídia, execução de código a partir de dados de rede, falhas na verificação de assinatura/versão das atualizações
e problemas no instalador.

## Fora de escopo

- Ausência de assinatura Authenticode do instalador (conhecida: a atualização usa assinatura Minisign própria).
- Tokens do LiveKit auto-hospedado já emitidos não serem revogados imediatamente (limitação documentada em
  [docs/sfu.md](docs/sfu.md)).
- Ataques que exijam acesso físico ou controle prévio da máquina da vítima.

## Dados e segredos

O app não envia telemetria. O diagnóstico é local e copiado por ação do usuário, sem nomes, tokens, SDP ou endereços.
Credenciais do SFU e o segredo de sessão ficam apenas no Worker.
