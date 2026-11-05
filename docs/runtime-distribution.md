# Runtime FFmpeg, licenças e patentes

O instalador do madscreen leva, ao lado do engine, três DLLs do FFmpeg (`avcodec`, `avutil` e `swresample`) que o
engine usa para **AMD AMF, Intel QuickSync e o fallback por CPU (x264)**. O caminho NVIDIA/NVENC fala direto com o
driver e não depende delas. Este documento explica o que acompanha o instalador, quais obrigações isso cria e como
verificar.

> Este documento descreve a política do projeto; **não é parecer jurídico**.

## O que acompanha o instalador

- Um **runtime mínimo** compilado pelo projeto: FFmpeg `760d10603e` (ABI `libavcodec 62` / `libavutil 60`) + **x264**
  (build 165) + AMD AMF + Intel oneVPL, sem as dezenas de bibliotecas de uma build *full*. O pacote é publicado como
  release (`runtime-AAAA-MM-DD`) e `tools/encode/prepare-runtime.ps1` o baixa com **SHA-256 fixo** em
  `tools/encode/runtime-gpl.json`. O instalador leva cinco DLLs (`avcodec`, `avutil`, `swresample`, `libx264`,
  `libvpl`), a licença, o `SOURCE-OFFER.md` e o `tools/encode/NOTICE.md`.
- O x264 é **GPL**. Por isso o madscreen é distribuído sob a
  **GPL-3.0-or-later** ([LICENSE](../LICENSE)): quem recebe o instalador recebe também o direito ao código-fonte.
- `nonfree` **nunca** é aceito. O auditor (`tools/release/runtime-audit.mjs`) detecta `--enable-nonfree`; no CI do
  instalador o `--strict` (que faz a detecção falhar o build) é ativado nas versões 1.x ou maiores (veja abaixo).

## Obrigações ao distribuir

1. **Licença e avisos** do app e de cada componente junto do instalador (`LICENSE`, `THIRD-PARTY-NOTICES.md`,
   `NOTICE.md` e os avisos do FFmpeg).
2. **Código-fonte correspondente** do runtime: a release do runtime publica os tarballs do FFmpeg e do x264 com SHA-256,
   a configuração da build (`notices/config.mak`) e o `SOURCE-OFFER.md`; os scripts que o reconstroem estão em
   `tools/encode/` (`build-native-runtime.ps1`, `build-native-runtime-gpl.ps1`, `package-runtime-gpl.ps1`). Não há
   patches no FFmpeg nem no x264.
3. **Sem restrições adicionais** ao que a GPL concede (por exemplo, proibir engenharia reversa do app).

## Verificar o runtime

```powershell
# Auditoria da configuração do FFmpeg (política GPL aberta; --strict falha em configuração bloqueada)
node tools/release/runtime-audit.mjs target/encoder-runtime/madscreen-runtime-gpl/bin/ffmpeg.exe `
  bench-results/runtime.json --policy gpl-open-source --strict --require-encoders h264_amf,h264_qsv,libx264

# Inventário das DLLs e dos avisos que realmente vão no pacote
node tools/release/runtime-inventory.mjs target/encoder-runtime/madscreen-runtime-gpl `
  apps/madscreen-app/src-tauri/encoder-runtime bench-results/runtime-inventory.json
```

O auditor extrai a configuração da build, as *flags* de cópia (GPL, `libx264`…) e o hash do executável. Na política
`gpl-open-source`, GPL é aceita e `nonfree` bloqueia; a política `preserve-private-application-sources` (a anterior,
para quem não pode abrir o código) bloqueia GPL e `libx264`. `--require-encoders` recusa um runtime que não traga os
encoders de que o código depende. O inventário compara as DLLs e os avisos copiados com a origem e registra tamanho e
SHA-256. Nada disso substitui a revisão das demais obrigações do pacote.

## Patentes

A licença de copyright **não cobre patentes**. O H.264/AVC é coberto por patentes de um *pool* de licenciamento, e
isso vale também para software livre e sem fins lucrativos. Na prática:

- O caminho principal usa o **encoder do driver da sua GPU** (NVENC, AMF, QuickSync); quem o licencia é o fabricante.
- O **x264** (fallback por CPU) não traz cobertura de royalties de ninguém.
- Uma DLL **OpenH264** compilada por nós **não** tem a cobertura da Cisco, que vale apenas para o **binário que a
  própria Cisco compila e hospeda** e é baixado dela em tempo de execução.

**Decisão do projeto (4/nov/2026):** o x264 **fica no instalador** como fallback por CPU, sob a GPL-3.0-or-later do app,
com o código-fonte correspondente publicado em cada release. O risco de patentes AVC é aceito conscientemente pelo
mantenedor. O objetivo é distribuir um runtime **mínimo e reproduzível** (só FFmpeg + x264 + AMF/oneVPL), em vez do pacote
*full* de terceiros usado até aqui, cujas DLLs embutem dezenas de bibliotecas que o madscreen não usa.

## Voltar a um runtime anterior

O pacote do runtime é fixado por `tools/encode/runtime-gpl.json` (URL do release, SHA-256 do ZIP e tamanho). Cada release do runtime (`runtime-AAAA-MM-DD`) permanece publicado com suas fontes e `SHA256SUMS.txt`: **não apague releases antigos**.

Para voltar a um runtime anterior:

1. Altere `tools/encode/runtime-gpl.json` para apontar ao release anterior (URL, SHA-256 e tamanho do ZIP dele) e faça o commit.
2. O CI (`app.yml`) baixa o pacote, confere o SHA-256 e o auditor (`tools/release/runtime-audit.mjs`) volta a inventariar os binários. Sem mudar o código do app, o instalador novo passa a empacotar o runtime anterior.
3. Publique o resultado como uma **versão nova do app** (número maior). O atualizador só oferece instaladores de versão maior que a instalada, então o retorno chega aos usuários como uma atualização com o runtime antigo, e não como um downgrade.
4. Quem instalou manualmente pode usar o instalador de uma versão anterior publicada no repositório de releases. O teste `tools/release/installer-smoke.ps1` (executado no CI) cobre instalar uma versão por cima da outra e desinstalar.

## Candidato OpenH264 (experimental)

O repositório inclui um backend `--encoder openh264` (FFmpeg com `libopenh264`, LGPL) usado apenas em laboratório:
fica **fora da seleção automática**, **não publica** e emite perfis `42c0`/`640c`, diferentes dos rótulos
`42e0`/`6400` que o contrato de receiver espera. O script `tools/encode/build-native-runtime.ps1` constrói um runtime
LGPL (AMF/QuickSync/Media Foundation) das fontes, com hashes fixados em `tools/encode/native-runtime-sources.json`.
Nenhum desses pacotes é promovido a padrão.
