//! Verificação manual da captura por aplicativo.
//!
//! ```text
//! probe list [--exclude-pid N]
//! probe capture (--pid N | --process nome.exe | --exclude-pid N) [--seconds 5] [--wav arquivo.wav]
//! ```
#![allow(clippy::print_stdout)]

use std::{
    fs::File,
    io::{BufWriter, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use madscreen_audio::{
    AudioCapture, AudioSourceSpec, CHANNELS, SAMPLE_RATE, list_audio_apps, resolve_root_pid,
};

#[derive(Default)]
struct Stats {
    second_sum: f64,
    second_peak: i32,
    second_samples: u64,
    total_sum: f64,
    total_peak: i32,
    total_samples: u64,
    blocks: u64,
    loud_blocks: u64,
    samples: Vec<i16>,
}

fn dbfs(value: f64) -> f64 {
    if value <= 0.0 {
        -120.0
    } else {
        20.0 * (value / 32768.0).log10()
    }
}

fn write_wav(path: &str, samples: &[i16]) -> Result<()> {
    let mut out = BufWriter::new(File::create(path).with_context(|| format!("criando {path}"))?);
    let data_len = (samples.len() * 2) as u32;
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data_len).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16_u32.to_le_bytes())?;
    out.write_all(&1_u16.to_le_bytes())?;
    out.write_all(&(CHANNELS as u16).to_le_bytes())?;
    out.write_all(&SAMPLE_RATE.to_le_bytes())?;
    out.write_all(&(SAMPLE_RATE * CHANNELS as u32 * 2).to_le_bytes())?;
    out.write_all(&(CHANNELS as u16 * 2).to_le_bytes())?;
    out.write_all(&16_u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data_len.to_le_bytes())?;
    for sample in samples {
        out.write_all(&sample.to_le_bytes())?;
    }
    Ok(out.flush()?)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    match args.first().map(String::as_str) {
        Some("list") => {
            let exclude = flag("--exclude-pid").and_then(|v| v.parse().ok());
            let apps = list_audio_apps(exclude)?;
            println!("{}", serde_json::to_string_pretty(&apps)?);
            Ok(())
        }
        Some("capture") => {
            let spec = if let Some(pid) = flag("--pid") {
                AudioSourceSpec::ProcessTree {
                    pid: pid.parse().context("--pid")?,
                }
            } else if let Some(name) = flag("--process") {
                let apps = list_audio_apps(None)?;
                let app = apps
                    .iter()
                    .find(|a| a.process_name.eq_ignore_ascii_case(&name))
                    .with_context(|| format!("{name} não tem sessão de áudio"))?;
                println!(
                    "processo {} -> PID raiz {} (resolvido {})",
                    app.process_name,
                    app.pid,
                    resolve_root_pid(app.pid)
                );
                AudioSourceSpec::ProcessTree { pid: app.pid }
            } else if let Some(pid) = flag("--exclude-pid") {
                AudioSourceSpec::SystemExcluding {
                    pid: pid.parse().context("--exclude-pid")?,
                }
            } else {
                bail!("use --pid, --process ou --exclude-pid");
            };
            let seconds: u64 = flag("--seconds").and_then(|v| v.parse().ok()).unwrap_or(5);
            let wav = flag("--wav");
            let stats = Arc::new(Mutex::new(Stats::default()));
            let sink_stats = stats.clone();
            let keep = wav.is_some();
            let capture = AudioCapture::start(
                &[spec],
                Box::new(move |block: &[i16]| {
                    let Ok(mut s) = sink_stats.lock() else {
                        return;
                    };
                    let mut peak = 0_i32;
                    for sample in block {
                        let v = i32::from(*sample);
                        peak = peak.max(v.abs());
                        s.second_sum += f64::from(v) * f64::from(v);
                        s.total_sum += f64::from(v) * f64::from(v);
                    }
                    s.second_peak = s.second_peak.max(peak);
                    s.total_peak = s.total_peak.max(peak);
                    s.second_samples += block.len() as u64;
                    s.total_samples += block.len() as u64;
                    s.blocks += 1;
                    if peak > 16 {
                        s.loud_blocks += 1;
                    }
                    if keep {
                        s.samples.extend_from_slice(block);
                    }
                }),
            )?;
            if let Some((failed, reason)) = capture.failed_sources().first() {
                bail!("fonte {failed:?} falhou: {reason}");
            }
            println!("capturando {spec:?} por {seconds} s");
            for second in 1..=seconds {
                std::thread::sleep(Duration::from_secs(1));
                let mut s = stats
                    .lock()
                    .map_err(|_| anyhow::anyhow!("estado envenenado"))?;
                let rms = (s.second_sum / s.second_samples.max(1) as f64).sqrt();
                println!(
                    "{second:>3} s  rms {:>7.1} dBFS  pico {:>7.1} dBFS",
                    dbfs(rms),
                    dbfs(f64::from(s.second_peak))
                );
                s.second_sum = 0.0;
                s.second_peak = 0;
                s.second_samples = 0;
            }
            let dropped = capture.dropped_samples();
            drop(capture);
            let s = stats
                .lock()
                .map_err(|_| anyhow::anyhow!("estado envenenado"))?;
            let rms = (s.total_sum / s.total_samples.max(1) as f64).sqrt();
            println!(
                "resumo: blocos {} ({:.2} s), rms {:.1} dBFS, pico {:.1} dBFS, não silenciosos {:.1}%, descartadas {dropped}",
                s.blocks,
                s.blocks as f64 / 100.0,
                dbfs(rms),
                dbfs(f64::from(s.total_peak)),
                s.loud_blocks as f64 * 100.0 / s.blocks.max(1) as f64
            );
            if let Some(path) = wav {
                write_wav(&path, &s.samples)?;
                println!("WAV gravado em {path}");
            }
            Ok(())
        }
        _ => bail!(
            "uso: probe list | probe capture (--pid N | --process x.exe | --exclude-pid N) [--seconds N] [--wav f.wav]"
        ),
    }
}
