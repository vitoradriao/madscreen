# Como contribuir com o madscreen

Obrigado pelo interesse! O projeto é open source (GPL-3.0-or-later) e sem fins lucrativos. Qualquer ajuda conta:
relatos de bug, testes em hardware diferente, documentação, traduções e código.

## Antes de começar

- Leia o [README](README.md), a [arquitetura](docs/architecture.md) e o [roadmap](docs/roadmap.md).
- Procure uma [issue](https://github.com/vitoradriao/madscreen/issues) existente. Os rótulos `good first issue` e
  `help wanted` indicam bons pontos de partida. Para uma mudança grande, abra uma issue antes para alinharmos o escopo.
- **Vulnerabilidades não vão em issue pública**: veja [SECURITY.md](SECURITY.md).

## Reportar um problema

Use o modelo de bug. Ajuda muito informar: versão do app, Windows (build), GPU e driver, o encoder usado
(NVENC/AMF/QuickSync/x264), o que você esperava, o que aconteceu e os passos para reproduzir. O app copia um
[diagnóstico local](docs/diagnostico.md) sem nomes, tokens nem SDP; cole-o na issue.

## Ambiente e verificações

O guia completo está em [docs/building.md](docs/building.md). Resumo:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm test --prefix viewer
```

Rode o que cobre o que você mexeu. O app Tauri (`apps/madscreen-app/src-tauri`) tem workspace Rust próprio. Mudança
em documentação isolada dispensa os testes de código.

## Regras do projeto

- **O jogo do transmissor vem primeiro.** Nada de prioridade REALTIME, filas crescentes de quadros nem alocação por
  quadro no caminho quente (use *pools*). O pacer é *latest-frame-wins*: descarte o quadro velho.
- **Só APIs oficiais** de captura (WGC e Desktop Duplication): sem injeção de DLL nem *hook* de jogo.
- `unsafe` isolado em módulos `*_sys`/`ffi`, sempre com um comentário `// SAFETY:`. Nenhum `unwrap()`/`expect()` em
  caminhos de quadro.
- `tracing` para logs, `thiserror` em bibliotecas e `anyhow` em binários. Código específico do Windows sob
  `#[cfg(windows)]`; lógica pura deve compilar e testar também em Linux.
- Código e identificadores em inglês; documentação e mensagens ao usuário em português do Brasil.
- **Não afirme desempenho que você não mediu.** Mudanças de pipeline devem trazer comparação antes/depois no cenário
  afetado e dizer em qual hardware. Números formais seguem a metodologia de [docs/building.md](docs/building.md#medir-desempenho).
- **Segredos nunca entram no Git**: chaves, tokens, `.dev.vars`, arquivos de deploy e perfis de navegador.

## Mensagens de commit e *pull requests*

- Use [Conventional Commits](https://www.conventionalcommits.org/pt-br/) com um escopo curto, por exemplo
  `fix(encode): corrige pedido de baixa latencia ao Media Foundation` ou `docs(readme): atualiza requisitos`.
- Um PR por assunto, pequeno e com a descrição do que mudou, como foi verificado e o que **não** foi coberto.
- Preencha o modelo do PR. Mudanças que alteram comportamento visível devem dizer como testar.

## Licença da contribuição

Ao enviar uma contribuição, você concorda que ela é licenciada sob a **GPL-3.0-or-later**, a mesma do projeto, e que
tem o direito de fazê-lo. Não envie código de terceiros com licença incompatível com a GPL-3.0.
