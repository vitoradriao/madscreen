# Patch local LiveKit 0.10.0

Origem: crate `livekit` 0.10.0 do crates.io, revisão registrada em `.cargo_vcs_info.json`.
Licença Apache-2.0 preservada em `LICENSE` e nos cabeçalhos dos arquivos.

Esta cópia corrige o [bug upstream #1018](https://github.com/livekit/rust-sdks/issues/1018):
publicar PCM estéreo com o SDK original não anuncia `TF_STEREO`, fazendo o SFU negociar
Opus mono. `audio_publish_features` deriva os recursos da quantidade de canais e das
opções DTX/preconnect; `publish_track` usa esses recursos no `AddTrackRequest` de áudio.
O transporte Cloudflare usa sua implementação própria.

O vídeo pré-codificado também contém a preferência de perfil nativo e nível
H.264 explícito adicionados no bloco anterior. Em 3/nov/2026, o ajuste de nível
foi isolado em `rtc_engine/h264_sdp.rs`: os payload types agora são associados à
seção `m=video` correspondente. Reutilizar o mesmo número em outra seção/codec
não altera seu fmtp. Testes puros cobrem essa colisão, RTX, perfil/constraints e
terminadores LF/CRLF. Isso não corrige nem comprova o contrato High do SFU.

O manifest normalizado foi preservado, removendo somente entradas de exemplos/testes
não distribuídos nesta cópia. Testes de regressão do anúncio de áudio ficam em
`crates/madscreen-transport/src/lib.rs`. Remover o `[patch.crates-io]` e esta cópia quando
uma versão upstream corrigida for adotada e passar no teste real de separação L/R.

Em 13/nov/2026, `Room::request_publisher_bandwidth_probe` passou a expor um pedido de sondagem de
banda no PeerConnection de publicação: alterna o teto de taxa do transporte (`SetBitrate`, sem
`start_bitrate`). Depois de um pico de atraso o GCC sobe só por aumento aditivo (~50 kbps/s) e não
sonda sozinho fora de ALR; um teto que sobe faz o `ProbeController` sondar. Medição em
`docs/bench/` (relatório de 13/nov).
