# Avisos do runtime distribuído

O instalador inclui bibliotecas do **FFmpeg** (libavcodec, libavutil, libswresample), o **x264** (`libx264`) e o
dispatcher **oneVPL** (`libvpl`), compilados pelo projeto a partir das fontes fixadas em
`tools/encode/native-runtime-sources.json` e `tools/encode/native-runtime-gpl-sources.json`.

- FFmpeg e x264 são software livre sob a GPL; o madscreen é distribuído sob a **GPL-3.0-or-later** (arquivo `LICENSE`
  do repositório). Os textos de licença acompanham o pacote (`LICENSE`, `x264-GPL-2.0.txt`).
- O **código-fonte correspondente** (FFmpeg, x264, configuração e scripts de build) está listado, com SHA-256, em
  `SOURCE-OFFER.md` e é publicado junto de cada release do runtime. É oferecido sem custo a quem recebe o instalador.
- AMD AMF e Intel oneVPL são MIT. NVIDIA NVENC usa o driver do fabricante e não depende destas bibliotecas.
- A licença de copyright **não cobre patentes**: o H.264/AVC pode exigir licenças de patentes em alguns países.
  Veja `docs/runtime-distribution.md`.

Para reproduzir o pacote: `tools/encode/build-native-runtime.ps1` (base), `tools/encode/build-native-runtime-gpl.ps1`
(FFmpeg + x264) e `tools/encode/package-runtime-gpl.ps1`. O `prepare-runtime.ps1` baixa o pacote publicado e confere o
SHA-256 fixado em `tools/encode/runtime-gpl.json`.
