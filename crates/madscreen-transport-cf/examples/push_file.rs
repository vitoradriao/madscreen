//! Publica um arquivo H.264 Annex B (um AUD por access unit) no SFU da Cloudflare, sem re-encode.
//! Serve aos experimentos de perfil/RTCP sem GPU:
//!   ffmpeg -f lavfi -i testsrc2=size=1920x1080:rate=60 -t 30 -c:v libx264 -profile:v high -bf 0 \
//!     -g 120 -x264-params aud=1:repeat-headers=1 -tune zerolatency -b:v 12M -f h264 teste.h264
//!   cargo run --release -p madscreen-transport-cf --example push_file -- teste.h264 --fps 60
//! Credenciais: MADSCREEN_CF_URL e MADSCREEN_CF_TOKEN (ambiente ou server.env).
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use madscreen_transport_cf::{CfPublishConfig, CloudflarePublisher, resolve_credentials};

fn split_access_units(data: &[u8]) -> Vec<(usize, usize, bool)> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 4 < data.len() {
        if data[i..i + 4] == [0, 0, 0, 1] && data[i + 4] & 0x1f == 9 {
            starts.push(i);
            i += 4;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::new();
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(data.len());
        let au = &data[start..end];
        let mut key = false;
        let mut j = 0;
        while j + 4 < au.len() {
            if au[j..j + 3] == [0, 0, 1] && au[j + 3] & 0x1f == 5 {
                key = true;
                break;
            }
            j += 1;
        }
        out.push((start, end, key));
    }
    out
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    let mut args = std::env::args().skip(1);
    let file = args.next().context("uso: push_file <arquivo.h264> [--fps N] [--seconds N] [--profile-level-id HEX] [--size WxH] [--bitrate-mbps N] [--no-loop] [--idle-after N] [--no-keepalive]")?;
    let (mut fps, mut seconds, mut profile, mut size, mut mbps, mut looped) =
        (60.0_f64, 60_u64, None, (1920_u32, 1080_u32), 18_u64, true);
    let (mut idle_after, mut keepalive) = (None::<u64>, true);
    while let Some(arg) = args.next() {
        let mut value = || args.next().context("valor ausente");
        match arg.as_str() {
            "--fps" => fps = value()?.parse()?,
            "--seconds" => seconds = value()?.parse()?,
            "--profile-level-id" => profile = Some(value()?),
            "--bitrate-mbps" => mbps = value()?.parse()?,
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').context("--size WxH")?;
                size = (w.parse()?, h.parse()?);
            }
            "--no-loop" => looped = false,
            // Para de entregar AUs após N s (tela estática) mantendo a sessão aberta até --seconds.
            "--idle-after" => idle_after = Some(value()?.parse()?),
            "--no-keepalive" => keepalive = false,
            other => bail!("argumento desconhecido: {other}"),
        }
    }
    let data = std::fs::read(&file).with_context(|| format!("lendo {file}"))?;
    let aus = split_access_units(&data);
    if aus.is_empty() {
        bail!("nenhum access unit com AUD (use -x264-params aud=1)");
    }
    let (worker_url, token) = resolve_credentials(None, None)?;
    let config = CfPublishConfig {
        worker_url,
        token,
        width: size.0,
        height: size.1,
        max_bitrate_bps: mbps * 1_000_000,
        max_fps: fps,
        h264_profile_level_id: profile,
        keepalive,
        ..Default::default()
    };
    let mut publisher = CloudflarePublisher::connect(&config)?;
    publisher.spawn_stats_logger(Duration::from_secs(5));
    eprintln!(
        "publicando {} AUs ({} keyframes) a {fps} fps",
        aus.len(),
        aus.iter().filter(|a| a.2).count()
    );
    let interval = Duration::from_secs_f64(1.0 / fps);
    let begin = Instant::now();
    let mut next = begin;
    let mut n = 0_usize;
    let (mut kf_req, mut rate_req) = (0_u64, 0_u64);
    let mut last_rate = None;
    while begin.elapsed() < Duration::from_secs(seconds) {
        if n >= aus.len() {
            if !looped {
                break;
            }
            n = 0;
        }
        if idle_after.is_some_and(|secs| begin.elapsed() >= Duration::from_secs(secs)) {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let (s, e, key) = aus[n];
        let ts = i64::try_from(begin.elapsed().as_micros()).unwrap_or(0);
        publisher.push(&data[s..e], key, ts);
        n += 1;
        if publisher.take_keyframe_request() {
            kf_req += 1;
        }
        if let Some(rate) = publisher.take_rate_request() {
            rate_req += 1;
            last_rate = Some(rate.bitrate_bps);
        }
        next += interval;
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    let (pushed, rejected) = publisher.counts();
    eprintln!(
        "fim: entregues={pushed} recusados={rejected} pedidos_keyframe={kf_req} pedidos_taxa={rate_req} ultima_taxa_bps={last_rate:?}"
    );
    publisher.close();
    Ok(())
}
