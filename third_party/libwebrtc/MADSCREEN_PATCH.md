# libwebrtc 0.3.55 — acréscimo do madscreen

Cópia do crate `libwebrtc` 0.3.55 (Apache-2.0, LiveKit) com um acréscimo:

- `src/native/video_source.rs` e `src/video_source.rs`: `NativeVideoSource::new_encoded_screencast`, igual a `new_encoded`
  mas com `is_screencast = true`. `new_encoded` (upstream) segue com `false`.
- `crates/madscreen-transport` usa a nova função **só** com a variável de ambiente
  `MADSCREEN_SCREENCAST_SOURCE` definida. Padrão: comportamento upstream.

Motivo: com `false` o libwebrtc trata a faixa como câmera e, provavelmente, não liga a sondagem de
banda em região limitada pela aplicação (ALR) nem marca o conteúdo como "screenshare" no RTP.
O efeito **não foi medido** (exige SFU + viewer; `inbound-rtp.contentType` deve virar `screenshare`
e a estimativa deve subir em tela parada), por isso fica desligado por padrão.

Retorno: remover a linha `libwebrtc` de `[patch.crates-io]` no `Cargo.toml` da raiz e a ramificação
em `crates/madscreen-transport/src/lib.rs`.
