//! Cadencia a entrega ao decoder acelerado quando o receptor escoa um backlog.
//!
//! O adaptador Chromium admite oito buffers pendentes. A entrega imediata de
//! um GOP acumulado pode ultrapassa-los mesmo com a GPU respondendo normalmente;
//! o adaptador descarta referencias e passa a exigir outro IDR. O caminho de
//! baixa latencia tem pacing nativo, mas normalmente o ignora acima de oito
//! quadros no frame buffer. Mantemos o pacing ate o limite desse buffer (800).
//! 4 ms permite escoar mais rapido que os perfis de 30/60 FPS do app.
//!
//! Field trials sao internos ao Chromium: revalidar ao mudar o WebView2.
//! Evidencia e limites: docs/bench/cadencia-decoder-2026-11-02.md.
pub const PACED_DECODE_ARGS: &str = concat!(
    "--force-fieldtrials=",
    "WebRTC-ForcePlayoutDelay/min_ms:0,max_ms:100/",
    "WebRTC-ZeroPlayoutDelay/min_pacing:4ms,max_decode_queue_size:800/"
);

pub fn browser_args(software_video_decode: bool) -> String {
    let mut args = PACED_DECODE_ARGS.to_owned();
    if software_video_decode {
        args.push_str(" --disable-accelerated-video-decode");
    }
    args
}
