# Runtime FFmpeg, licenças e patentes

O instalador do madscreen leva, ao lado do engine, três DLLs do FFmpeg (`avcodec`, `avutil` e `swresample`) que o
engine usa para **AMD AMF, Intel QuickSync e o fallback por CPU (x264)**. O caminho NVIDIA/NVENC fala direto com o
driver e não depende delas. Este documento explica o que acompanha o instalador, quais obrigações isso cria e como
verificar.

> Este documento descreve a política do projeto; **não é parecer jurídico**.

## O que acompanha o instalador

- FFmpeg **8.1.2 *full shared*** (ABI `libavcodec 62` / `libavutil 60`), baixado por
  `tools/encode/prepare-runtime.ps1` com versão e **SHA-256 fixos**. O script copia só as três DLLs, a licença, o
  `README.txt` com a configuração da build e o `tools/encode/NOTICE.md`.
- Essa build inclui componentes **GPL** (por exemplo `libx264`). Por isso o madscreen é distribuído sob a
  **GPL-3.0-or-later** ([LICENSE](../LICENSE)): quem recebe o instalador recebe também o direito ao código-fonte.
- `nonfree` **nunca** é aceito. O auditor do CI bloqueia `--enable-nonfree` (veja abaixo).

## Obrigações ao distribuir

1. **Licença e avisos** do app e de cada componente junto do instalador (`LICENSE`, `THIRD-PARTY-NOTICES.md`,
   `NOTICE.md` e os avisos do FFmpeg).
2. **Código-fonte correspondente** do runtime em cada release: a versão exata do FFmpeg, os patches, a configuração
   da build (a linha `configuration:` de `ffmpeg -version` e o `README.txt` do pacote) e as instruções para
   reconstruí-lo. Hospede os arquivos fonte junto da release ou aponte para um endereço estável e verificável por hash.
3. **Sem restrições adicionais** ao que a GPL concede (por exemplo, proibir engenharia reversa do app).

## Verificar o runtime

```powershell
# Auditoria da configuração do FFmpeg (política GPL aberta; --strict falha em configuração bloqueada)
node tools/release/runtime-audit.mjs target/encoder-runtime/ffmpeg-8.1.2-full_build-shared/bin/ffmpeg.exe `
  bench-results/runtime.json --policy gpl-open-source --strict --require-encoders h264_amf,h264_qsv,libx264

# Inventário das DLLs e dos avisos que realmente vão no pacote
node tools/release/runtime-inventory.mjs target/encoder-runtime/ffmpeg-8.1.2-full_build-shared `
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

Decida conscientemente se e como distribui um encoder por CPU. Alternativas: manter só os encoders de hardware, ou
tratar o x264 como componente opcional instalado pelo usuário.

## Candidato OpenH264 (experimental)

O repositório inclui um backend `--encoder openh264` (FFmpeg com `libopenh264`, LGPL) usado apenas em laboratório:
fica **fora da seleção automática**, **não publica** e emite perfis `42c0`/`640c`, diferentes dos rótulos
`42e0`/`6400` que o contrato de receiver espera. O script `tools/encode/build-native-runtime.ps1` constrói um runtime
LGPL (AMF/QuickSync/Media Foundation) das fontes, com hashes fixados em `tools/encode/native-runtime-sources.json`.
Nenhum desses pacotes é promovido a padrão.
