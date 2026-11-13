use crate::{
    options::{ConvertArg, InputArg, Options, Size, Source, SyncArg, SyntheticArg},
    report::{FrameRow, Measurements, Report, UtilizationSummary},
};
use anyhow::{Context, Result};
use madscreen_capture::{CaptureOptions, CaptureStats, source::CaptureSource, wgc::GpuDevice};
use madscreen_encode::{
    AuBuffer, Encoder, EncoderConfig, FrameRate, FrameTiming, Preset, VideoCodec,
    auto::AutoEncoder,
    backend::BackendKind,
    bitstream::{AnnexBWriter, IvfWriter},
    convert::{
        ConvertMode, Converter, InputFormat, SyntheticSource, reduction::ReductionPreference,
    },
    nvenc,
    pacer::Pacer,
    pipeline::{FlightSlots, capture_due},
    platform::{self, GpuSample, GpuSampler, Timer, qpc, qpc_frequency},
};
use madscreen_ipc::resolution::{
    HEIGHT_NATIVE, PlanError, auto_bitrate_bps, h264_level_from_profile_level_id,
    h264_profile_level_id, plan_output,
};
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

fn reduction_preference(
    options: &Options,
    kind: madscreen_encode::backend::BackendKind,
    compat_tuning: bool,
    shader_available: bool,
) -> ReductionPreference {
    if !compat_tuning {
        ReductionPreference::Automatic
    } else if options.reduction_box {
        ReductionPreference::Box4x4
    } else if options.reduction_bilinear {
        ReductionPreference::Bilinear
    } else if madscreen_encode::config::compatibility_backend(kind) && shader_available {
        ReductionPreference::Box4x4
    } else {
        ReductionPreference::Automatic
    }
}

/// Instante de captura de um quadro enviado agora. Repetição do mesmo quadro (IDR de repouso,
/// refino de tela parada) recebe o instante do reenvio: o timestamp vai ao RTP e, com o da
/// captura original, voltaria segundos no tempo (o LiveKit não força monotonia).
fn presented_capture_qpc(frame_qpc: i64, last_captured_qpc: Option<i64>, now_qpc: i64) -> i64 {
    if last_captured_qpc.is_some_and(|last| frame_qpc <= last) {
        now_qpc
    } else {
        frame_qpc
    }
}

/// Taxa de um encoder recriado na troca de resolução: a aplicada pelo WebRTC, nunca acima do teto.
fn rebuild_bitrate_bps(cap_bps: u32, applied_bps: Option<u32>) -> u32 {
    applied_bps.map_or(cap_bps, |applied| applied.clamp(1, cap_bps))
}

fn tuning_decision(
    active: bool,
    reason: &'static str,
) -> madscreen_ipc::diagnostics::TuningDecision {
    madscreen_ipc::diagnostics::TuningDecision {
        active,
        reason: reason.into(),
    }
}

fn restore_capture_cadence(
    capture: &Option<CaptureSource>,
    options: &mut CaptureOptions,
    capture_fps: &mut FrameRate,
    capture_period: &mut i64,
    configured_fps: FrameRate,
    capture_limit: bool,
    frequency: i64,
) {
    *capture_fps = configured_fps;
    *capture_period = (i128::from(frequency) * i128::from(configured_fps.den)
        / i128::from(configured_fps.num.max(1)))
    .clamp(1, i128::from(i64::MAX)) as i64;
    options.target_fps = capture_limit.then(|| configured_fps.as_f64().ceil() as u32);
    if let Some(source) = capture {
        source.set_target_fps(options.target_fps);
    }
}

#[cfg(test)]
mod compatibility_policy_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn repeated_frame_is_stamped_when_sent_again() {
        // Quadro novo conserva o instante de captura (latência real).
        assert_eq!(presented_capture_qpc(1_000, None, 5_000), 1_000);
        assert_eq!(presented_capture_qpc(2_000, Some(1_000), 5_000), 2_000);
        // Repetição do mesmo quadro (refino/IDR de repouso) sai com o instante do reenvio.
        assert_eq!(presented_capture_qpc(2_000, Some(2_000), 180_000), 180_000);
        assert_eq!(presented_capture_qpc(1_500, Some(2_000), 180_000), 180_000);
    }

    #[test]
    fn rebuilt_encoder_keeps_the_rate_webrtc_applied() {
        // Rede sustentando 3 Mbps sob teto de 7 Mbps (1080p30 automático).
        assert_eq!(rebuild_bitrate_bps(7_000_000, Some(3_000_000)), 3_000_000);
        assert_eq!(rebuild_bitrate_bps(7_000_000, Some(9_000_000)), 7_000_000);
        // Sem publicação (laboratório local) conserva o teto.
        assert_eq!(rebuild_bitrate_bps(7_000_000, None), 7_000_000);
        assert_eq!(rebuild_bitrate_bps(7_000_000, Some(0)), 1);
    }

    #[test]
    fn default_filter_keeps_nvenc_amf_and_enables_compatibility_only() -> Result<()> {
        let options = Options::try_parse_from(["encode"])?;
        for kind in [BackendKind::Nvenc, BackendKind::Amf] {
            assert_eq!(
                reduction_preference(&options, kind, true, true),
                ReductionPreference::Automatic
            );
        }
        for kind in [
            BackendKind::X264,
            BackendKind::Qsv,
            BackendKind::Mf,
            BackendKind::OpenH264,
        ] {
            assert_eq!(
                reduction_preference(&options, kind, true, true),
                ReductionPreference::Box4x4
            );
            assert_eq!(
                reduction_preference(&options, kind, false, true),
                ReductionPreference::Automatic
            );
            assert_eq!(
                reduction_preference(&options, kind, true, false),
                ReductionPreference::Automatic
            );
        }
        Ok(())
    }
}

pub fn run(options: Options) -> Result<()> {
    let _runtime = platform::Runtime::new()?;
    if options.probe {
        return probe(&options);
    }
    platform::set_encoder_priority()?;
    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    ctrlc::set_handler(move || signal.store(false, Ordering::Release))?;
    let presets = if options.compare_presets {
        vec![Preset::P1, Preset::P2, Preset::P3, Preset::P4]
    } else {
        vec![options.preset.into()]
    };
    let profiles = if options.compare_convert {
        [
            (ConvertArg::Shader, InputArg::Nv12, "shader"),
            (ConvertArg::Vp, InputArg::Nv12, "vp"),
            (ConvertArg::Shader, InputArg::Bgra, "bgra"),
        ]
        .into_iter()
        .map(|(convert, input, name)| {
            let mut profile = options.clone();
            profile.convert = convert;
            profile.input = input;
            profile.output = profile.output.map(|p| comparison_path(&p, name));
            profile.frames_csv = profile.frames_csv.map(|p| comparison_path(&p, name));
            profile
        })
        .collect::<Vec<_>>()
    } else {
        vec![options.clone()]
    };
    let mut reports = Vec::with_capacity(presets.len() * profiles.len());
    for profile in profiles {
        for &preset in &presets {
            if !running.load(Ordering::Acquire) {
                break;
            }
            let Some(report) = run_profile(&profile, preset, &running, None)? else {
                break;
            };
            println!("{report}");
            reports.push(report);
        }
    }
    if options.compare_presets || options.compare_convert {
        println!(
            "Preset | codec | conversão/entrada | FPS | encode p50/p95/p99/max ms | convert_gpu p95 | queue_wait p95 | total p95 ms | Mbps"
        );
        for r in &reports {
            println!(
                "{:?} | {:?} | {}/{} | {:.2} | {:?}/{:?}/{:?}/{:?} | {:?} | {:?} | {:?} | {:?}",
                r.config.preset,
                r.config.codec,
                r.convert,
                r.input,
                r.encode_fps,
                r.encode_ms.p50,
                r.encode_ms.p95,
                r.encode_ms.p99,
                r.encode_ms.max,
                r.convert_gpu_ms.p95,
                r.queue_wait_ms.p95,
                r.total_ms.p95,
                r.bitrate_mean_mbps
            );
        }
    }
    if let Some(path) = &options.report {
        let mut file = BufWriter::new(File::create(path)?);
        serde_json::to_writer_pretty(&mut file, &reports)?;
        file.write_all(b"\n")?;
        file.flush()?;
    }
    Ok(())
}
fn target(source: &Source) -> Result<Option<madscreen_capture::CaptureTarget>> {
    Ok(match source {
        Source::Synthetic | Source::Picker => None,
        Source::Monitor(n) => Some(platform::resolve_monitor(*n)?),
        Source::Window(title) => Some(platform::resolve_window(title)?),
        Source::WindowHandle(hwnd) => {
            Some(madscreen_capture::CaptureTarget::Window(*hwnd as isize))
        }
    })
}
fn probe(options: &Options) -> Result<()> {
    println!("NVENC: API usada 12.2; teste de disponibilidade sem GPU é permitido.");
    match platform::adapters() {
        Ok(adapters) => {
            for a in adapters {
                println!(
                    "GPU: {} vendor={:#x} LUID={:08x}:{:08x}",
                    a.name, a.vendor_id, a.luid_high as u32, a.luid_low
                );
            }
        }
        Err(e) => println!("Enumeração de GPUs indisponível: {e}"),
    }
    let status = (|| -> Result<_> {
        let gpu = GpuDevice::new(target(&options.source)?)?;
        let adapter = platform::device_adapter(&gpu.device)?;
        println!(
            "Adapter selecionado: {} LUID={:08x}:{:08x}",
            adapter.name, adapter.luid_high as u32, adapter.luid_low
        );
        Ok(nvenc::probe(&gpu)?)
    })();
    let nvenc_json = match status {
        Ok(probe) => {
            println!(
                "NVENC disponível, API máxima {}.{}",
                probe.max_api_major, probe.max_api_minor
            );
            for c in &probe.codecs {
                println!(
                    "{:?}: mínimo {}x{}, máximo {}x{}, nível máximo={}, macroblocos/s máximo={}, assíncrono={}, intra-refresh={}, bitrate dinâmico={}",
                    c.codec,
                    c.min_width,
                    c.min_height,
                    c.max_width,
                    c.max_height,
                    c.level_max,
                    c.mb_per_sec_max,
                    c.asynchronous,
                    c.intra_refresh,
                    c.dynamic_bitrate
                );
            }
            serde_json::json!({"status":"disponivel","nvenc":probe})
        }
        Err(e) => {
            println!("NVENC indisponível: {e:#}");
            serde_json::json!({"status":"indisponivel","error":format!("{e:#}")})
        }
    };
    let selection = (|| -> Result<_> {
        let gpu = GpuDevice::new(target(&options.source)?)?;
        let size = options.size.unwrap_or(Size {
            width: 1280,
            height: 720,
        });
        let config = options.config(size.width, size.height, options.preset.into())?;
        let converter = Converter::new_ring(
            &gpu,
            (size.width, size.height),
            &config,
            ConvertMode::Shader,
            InputFormat::Nv12,
            4,
        )?;
        let encoder = AutoEncoder::new(
            &gpu,
            config,
            converter.textures(),
            false,
            options.encoder.requested(),
        )?;
        Ok(encoder.kind())
    })();
    let json = match selection {
        Ok(kind) => {
            println!("Encoder disponível para transmissão: {}", kind.name());
            serde_json::json!({"status":"disponivel", "selected": kind, "nvenc": nvenc_json})
        }
        Err(error) => {
            println!("Nenhum encoder disponível: {error:#}");
            serde_json::json!({"status":"indisponivel", "error":format!("{error:#}"), "nvenc": nvenc_json})
        }
    };
    let samples = GpuSampler::new().sample();
    if samples.is_empty() {
        println!("Driver/utilização NVML/nvidia-smi: indisponível");
    } else {
        for s in &samples {
            println!(
                "Telemetria GPU {}: {} driver={:?}, encoder={:?}%, GPU={:?}% ({})",
                s.gpu_index, s.name, s.driver, s.encoder_percent, s.gpu_percent, s.source
            );
        }
    }
    if let Some(path) = &options.report {
        serde_json::to_writer_pretty(
            BufWriter::new(File::create(path)?),
            &serde_json::json!({"probe":json,"gpus":samples}),
        )?;
    }
    Ok(())
}
fn comparison_path(path: &Path, name: &str) -> PathBuf {
    if path.as_os_str().is_empty() || path.to_string_lossy().eq_ignore_ascii_case("NUL") {
        return path.into();
    }
    let mut stem = path.file_stem().unwrap_or_default().to_os_string();
    stem.push(format!("-{name}"));
    if let Some(ext) = path.extension() {
        stem.push(".");
        stem.push(ext);
    }
    path.with_file_name(stem)
}
fn profile_path(path: &Path, preset: Preset, compare: bool) -> PathBuf {
    if !compare || path.as_os_str().is_empty() || path.to_string_lossy().eq_ignore_ascii_case("NUL")
    {
        return path.into();
    }
    let mut name = path.file_stem().unwrap_or_default().to_os_string();
    name.push(format!("-{}", format!("{preset:?}").to_lowercase()));
    if let Some(ext) = path.extension() {
        name.push(".");
        name.push(ext);
    }
    path.with_file_name(name)
}
enum Output {
    Discard,
    #[cfg(feature = "publish")]
    Live(Box<crate::publish::Live>),
    Annex(AnnexBWriter<BufWriter<File>>),
    Ivf(IvfWriter<BufWriter<File>>),
}
fn publish_diagnostic(
    error: anyhow::Error,
    encoder: &str,
    config: &EncoderConfig,
) -> anyhow::Error {
    let summary = format!("{error:#}");
    let prior = error.chain().find_map(|cause| {
        cause
            .downcast_ref::<madscreen_ipc::ShareDiagnostic>()
            .map(|item| item.body.clone())
    });
    let level = config
        .h264_level
        .map(|value| format!("{}.{}", value / 10, value % 10))
        .unwrap_or_else(|| "automático".into());
    let profile = config
        .h264_level
        .map(h264_profile_level_id)
        .unwrap_or_else(|| "não definido".into());
    let header = format!(
        "Codificador: {encoder}\nSaída: {}x{} a {:.3} fps\nNível H.264 do encoder: {level}\nprofile-level-id do encoder (High): {profile}",
        config.width,
        config.height,
        config.fps.as_f64()
    );
    let body = match prior {
        Some(prior) => format!("{header}\n\n{prior}"),
        None => format!("{header}\n\n{summary}"),
    };
    anyhow::Error::new(madscreen_ipc::ShareDiagnostic { summary, body })
}

impl Output {
    fn new(
        options: &Options,
        preset: Preset,
        config: &EncoderConfig,
        governor_enabled: bool,
        governor_cpu_signals: bool,
    ) -> Result<Self> {
        #[cfg(not(feature = "publish"))]
        let _ = (governor_enabled, governor_cpu_signals);
        if options.publishing() {
            #[cfg(feature = "publish")]
            return Ok(Self::Live(Box::new(crate::publish::Live::connect(
                options,
                config,
                governor_enabled,
                governor_cpu_signals,
            )?)));
            #[cfg(not(feature = "publish"))]
            anyhow::bail!("publicação exige compilar com --features publish");
        }
        let Some(path) = options.output.as_ref().filter(|p| {
            !p.as_os_str().is_empty() && !p.to_string_lossy().eq_ignore_ascii_case("NUL")
        }) else {
            return Ok(Self::Discard);
        };
        let path = profile_path(path, preset, options.compare_presets);
        let file = BufWriter::with_capacity(
            1024 * 1024,
            File::create(&path).with_context(|| format!("criando {}", path.display()))?,
        );
        Ok(if config.codec == VideoCodec::Av1 {
            Self::Ivf(IvfWriter::new(
                file,
                config.width,
                config.height,
                config.fps,
            )?)
        } else {
            Self::Annex(AnnexBWriter(file))
        })
    }
    fn write(&mut self, au: &AuBuffer, encoder: &mut AutoEncoder) -> Result<bool> {
        #[cfg(not(feature = "publish"))]
        let _ = &encoder;
        match self {
            Self::Discard => {}
            #[cfg(feature = "publish")]
            Self::Live(live) => return live.write(au, encoder),
            Self::Annex(w) => w.write_au(&au.data)?,
            Self::Ivf(w) => w.write_au(&au.data, au.index)?,
        }
        Ok(true)
    }
    /// Refino de tela parada em andamento: o laço não pode dormir 100 ms entre repetições.
    fn refining(&self) -> bool {
        match self {
            #[cfg(feature = "publish")]
            Self::Live(live) => live.refining(),
            _ => false,
        }
    }
    fn set_audio_muted(&self, muted: bool) {
        #[cfg(not(feature = "publish"))]
        let _ = muted;
        match self {
            #[cfg(feature = "publish")]
            Self::Live(live) => live.set_audio_muted(muted),
            _ => {}
        }
    }
    fn wants_redraw(&mut self, encoder: &mut AutoEncoder) -> Result<bool> {
        #[cfg(not(feature = "publish"))]
        let _ = &encoder;
        match self {
            #[cfg(feature = "publish")]
            Self::Live(live) => live.wants_redraw(encoder),
            _ => Ok(false),
        }
    }
    /// Fontes de áudio ativas e motivos das que falharam; `None` sem áudio publicado.
    fn audio_status(&self) -> Option<(u32, Vec<String>)> {
        match self {
            #[cfg(feature = "publish")]
            Self::Live(live) => live.audio_status(),
            _ => None,
        }
    }
    fn diagnostic_clock(&self) -> Option<(String, std::time::Instant)> {
        match self {
            #[cfg(feature = "publish")]
            Self::Live(live) => live.diagnostic_clock(),
            _ => None,
        }
    }
    fn diagnostic_sample(&mut self) -> Option<madscreen_ipc::diagnostics::PublisherSample> {
        match self {
            #[cfg(feature = "publish")]
            Self::Live(live) => live.diagnostic_sample(),
            _ => None,
        }
    }
    fn governor_tick(
        &mut self,
        at_ms: u64,
        captured: u64,
        submitted: u64,
        process_cpu_cores: Option<f64>,
    ) -> Option<crate::load_governor::Target> {
        #[cfg(not(feature = "publish"))]
        let _ = (at_ms, captured, submitted, process_cpu_cores);
        match self {
            #[cfg(feature = "publish")]
            Self::Live(live) => live.governor_tick(at_ms, captured, submitted, process_cpu_cores),
            _ => None,
        }
    }
    fn disable_governor(&mut self) {
        match self {
            #[cfg(feature = "publish")]
            Self::Live(live) => live.disable_governor(),
            _ => {}
        }
    }
    fn applied_bitrate_bps(&self) -> Option<u32> {
        match self {
            #[cfg(feature = "publish")]
            Self::Live(live) => Some(live.applied_bitrate_bps()),
            _ => None,
        }
    }
    fn is_connected(&self) -> Option<bool> {
        match self {
            #[cfg(feature = "publish")]
            Self::Live(live) => live.is_connected(),
            _ => None,
        }
    }
    fn is_terminated(&self) -> bool {
        match self {
            #[cfg(feature = "publish")]
            Self::Live(live) => live.is_terminated(),
            _ => false,
        }
    }
    fn finish(self) -> Result<()> {
        match self {
            Self::Discard => {}
            #[cfg(feature = "publish")]
            Self::Live(live) => (*live).finish(),
            Self::Annex(mut w) => w.0.flush()?,
            Self::Ivf(w) => w.into_inner().flush()?,
        }
        Ok(())
    }
}
#[derive(serde::Serialize)]
struct TimedGpuSample {
    seconds: f64,
    devices: Vec<GpuSample>,
}
#[derive(Default)]
struct SampleBatch {
    samples: Vec<TimedGpuSample>,
    utilization: UtilizationSummary,
}
struct Sampling {
    stop: mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<SampleBatch>>,
}
impl Sampling {
    fn new(warmup: u32, adapter_name: String) -> Result<Self> {
        let (stop, rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("gpu-telemetry".into())
            .spawn(move || {
                let sampler = GpuSampler::new();
                let start = Instant::now();
                let mut samples = VecDeque::with_capacity(3600);
                let mut utilization = UtilizationSummary::default();
                loop {
                    let seconds = start.elapsed().as_secs_f64() - f64::from(warmup);
                    if seconds >= 0. {
                        if samples.len() == 3600 {
                            samples.pop_front();
                        }
                        let devices = sampler.sample();
                        // Se a amostragem estava em andamento na parada, não incluir drenagem.
                        if rx.try_recv().is_ok() {
                            break;
                        }
                        let mut matching =
                            devices.iter().filter(|device| device.name == adapter_name);
                        if let Some(device) = matching.next().filter(|_| matching.next().is_none())
                        {
                            utilization.record(device.gpu_percent, device.encoder_percent);
                        }
                        samples.push_back(TimedGpuSample { seconds, devices });
                    }
                    if !matches!(
                        rx.recv_timeout(Duration::from_secs(1)),
                        Err(mpsc::RecvTimeoutError::Timeout)
                    ) {
                        break;
                    }
                }
                SampleBatch {
                    samples: samples.into_iter().collect(),
                    utilization,
                }
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
    fn stop(&self) {
        let _ = self.stop.send(());
    }
    fn finish(&mut self) -> Result<SampleBatch> {
        self.stop();
        self.thread
            .take()
            .map_or(Ok(SampleBatch::default()), |thread| {
                thread
                    .join()
                    .map_err(|_| anyhow::anyhow!("telemetria GPU encerrou inesperadamente"))
            })
    }
}
impl Drop for Sampling {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn capture_stats(source: &Option<CaptureSource>) -> Result<Option<CaptureStats>> {
    source
        .as_ref()
        .map(CaptureSource::stats)
        .transpose()
        .map_err(Into::into)
}
pub(crate) fn run_profile(
    options: &Options,
    preset: Preset,
    running: &AtomicBool,
    session: Option<&crate::ipc::Session>,
) -> Result<Option<Report>> {
    anyhow::ensure!(
        options.diagnostics_output.is_none() || (options.diagnostics && options.publishing()),
        "--diagnostics-output exige publicação e diagnóstico habilitado"
    );
    let mut diagnostic_file = options
        .diagnostics_output
        .as_ref()
        .map(|path| {
            File::create_new(path)
                .map(BufWriter::new)
                .with_context(|| format!("criando diagnóstico local {}", path.display()))
        })
        .transpose()?;
    // Sem planejamento, manter a validação antes de iniciar captura/alocar recursos.
    let default_size = options.size.unwrap_or(Size {
        width: 1920,
        height: 1080,
    });
    if options.height_choice.is_none() {
        options.config(default_size.width, default_size.height, preset)?;
    }
    let item = if matches!(options.source, Source::Picker) {
        if let Some(session) = session {
            session.emit(&madscreen_ipc::Event::Picking)?;
        }
        let Some(item) = crate::picker::pick(running)
            .context(crate::ipc::FailureStage(madscreen_ipc::ErrorCode::Capture))?
        else {
            return Ok(None);
        };
        Some(item)
    } else {
        None
    };
    if !running.load(Ordering::Acquire) {
        return Ok(None);
    }
    let target = target(&options.source)
        .context(crate::ipc::FailureStage(madscreen_ipc::ErrorCode::Capture))?;
    if let (Source::WindowHandle(hwnd), Some(expected)) =
        (&options.source, &options.expected_window_instance)
    {
        crate::sources::verify_window(*hwnd, expected)?;
    }
    // Janela capturada por HWND: conferida tambem fora do evento Closed do WGC.
    let watched_window = match target {
        Some(madscreen_capture::CaptureTarget::Window(hwnd)) => Some(hwnd),
        _ => None,
    };
    let gpu = GpuDevice::new(target)?;
    let adapter = platform::device_adapter(&gpu.device)?;
    reportln!(options.ipc;
        "Adapter da captura/encode: {} LUID={:08x}:{:08x}",
        adapter.name, adapter.luid_high as u32, adapter.luid_low
    );
    let priority = platform::set_gpu_priority(&gpu.device, options.gpu_priority.into());
    reportln!(options.ipc;
        "Prioridade GPU {:?}: DXGI={}, processo={}",
        priority.requested, priority.dxgi, priority.process
    );
    if priority.dxgi != "aplicado" || priority.process != "aplicado" {
        reportln!(options.ipc;
            "AVISO: prioridade GPU não aplicada integralmente; confira os resultados acima e permissões de administrador."
        );
    }
    let frame_event = Arc::new(platform::FrameEvent::new()?);
    let notify = frame_event.clone();
    let mut capture_options = CaptureOptions {
        target_fps: options
            .capture_limit
            .then(|| options.fps.as_f64().ceil() as u32),
        ..Default::default()
    };
    let capture_notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || notify.signal());
    let mut capture = if target.is_some() || item.is_some() {
        let notify = capture_notify.clone();
        // FrameSource::start recria o device. Esta entrada aditiva mantém o mesmo device para NVENC.
        let source = if let Some(item) = item {
            CaptureSource::start_item(gpu.clone(), item, capture_options, notify)
        } else if let Some(target) = target {
            CaptureSource::start(
                gpu.clone(),
                target,
                capture_options,
                options.capture_backend.into(),
                notify,
            )
        } else {
            anyhow::bail!("fonte de captura indisponível")
        }
        .context(crate::ipc::FailureStage(madscreen_ipc::ErrorCode::Capture))?;
        reportln!(options.ipc; "Backend de captura: {}", source.backend_name());
        Some(source)
    } else {
        None
    };
    let source_size =
        capture_stats(&capture)?.map_or((default_size.width, default_size.height), |s| s.size);
    if let (Source::WindowHandle(hwnd), Some(expected)) =
        (&options.source, &options.expected_window_instance)
    {
        crate::sources::verify_window(*hwnd, expected)?;
    }
    let probe = nvenc::probe(&gpu).ok();
    let fps = options.fps.as_f64();
    let plan = if let Some(choice) = options.height_choice {
        // Falha de NVENC nunca impede planejar uma transmissão AMF/QSV/CPU.
        // Os backends alternativos validam as dimensões reais ao inicializar/codificar.
        let limits = probe
            .as_ref()
            .and_then(crate::limits::h264_limits)
            .unwrap_or_default();
        let unsupported = || crate::ipc::FailureStage(madscreen_ipc::ErrorCode::Unsupported);
        // A UI mede a fonte antes da captura (janela com borda, DPI); se o quadro real vier
        // menor que a altura pedida, transmite na nativa em vez de falhar (nunca amplia).
        let plan = match plan_output(source_size, choice, fps, limits) {
            Err(PlanError::Unavailable { source_height }) => {
                let mut plan =
                    plan_output(source_size, HEIGHT_NATIVE, fps, limits).context(unsupported())?;
                let fallback = format!(
                    "A fonte capturada tem {source_height}p, menos que {choice}p: transmitindo na resolução nativa, sem ampliar."
                );
                plan.notice = Some(match plan.notice.take() {
                    Some(notice) => format!("{fallback} {notice}"),
                    None => fallback,
                });
                plan
            }
            other => other.context(unsupported())?,
        };
        Some(plan)
    } else {
        None
    };
    let size = plan.as_ref().map_or_else(
        || {
            options.size.unwrap_or(Size {
                width: source_size.0 & !1,
                height: source_size.1 & !1,
            })
        },
        |plan| Size {
            width: plan.width,
            height: plan.height,
        },
    );
    let mut options = options.clone();
    if options.bitrate_auto {
        options.bitrate = auto_bitrate_bps(size.width, size.height, fps);
    }
    let mut config = options.config(size.width, size.height, preset)?;
    config.h264_level = plan.as_ref().map(|p| p.level);
    config.clamp_h264_bitrate();
    options.bitrate = config.bitrate_bps;
    let options = &options;
    if let Some(plan) = &plan {
        config.h264_level = Some(plan.level);
        let profile_reference = match config.h264_profile {
            madscreen_encode::config::H264Profile::Main => format!("4d00{:02x}", plan.level),
            madscreen_encode::config::H264Profile::ConstrainedBaseline => {
                format!("42e0{:02x}", plan.level)
            }
            _ => plan.profile_level_id.clone(),
        };
        reportln!(options.ipc;
            "Fonte {}x{} -> saída {}x{} ({}{}{}), nível H.264 {}.{}, profile-level-id {}, bitrate alvo {} bps{}{}",
            plan.source_width, plan.source_height, plan.width, plan.height,
            if plan.native { "nativa" } else { "altura escolhida" },
            if plan.ultrawide { ", ultrawide" } else { "" },
            if plan.reduced { ", reduzida" } else { "" },
            plan.level / 10, plan.level % 10, profile_reference, config.bitrate_bps,
            if plan.notice.is_some() { "; aviso: " } else { "" },
            plan.notice.as_deref().unwrap_or_default()
        );
    }
    if config.h264_profile != madscreen_encode::config::H264Profile::High
        && config.h264_level.is_none()
    {
        config.h264_level =
            madscreen_ipc::resolution::h264_level(config.width, config.height, config.fps.as_f64());
    }
    if config.codec == VideoCodec::Av1
        && options.fallback_hevc
        && !probe
            .as_ref()
            .is_some_and(|p| p.codecs.iter().any(|c| c.codec == VideoCodec::Av1))
    {
        anyhow::ensure!(
            probe
                .as_ref()
                .is_some_and(|p| p.codecs.iter().any(|c| c.codec == VideoCodec::Hevc)),
            "AV1 e fallback HEVC indisponíveis"
        );
        tracing::warn!("AV1 indisponível; perfil usará HEVC");
        config.codec = VideoCodec::Hevc;
    }
    let base_config = config.clone();
    let input = match options.input {
        InputArg::Nv12 => InputFormat::Nv12,
        InputArg::Bgra => InputFormat::Bgra,
    };
    let mode = match options.convert {
        ConvertArg::Shader => ConvertMode::Shader,
        ConvertArg::Vp => ConvertMode::VideoProcessor,
    };
    let ring_size = (options.inflight as usize + 1).max(4);
    let mut converter = Converter::new_ring(&gpu, source_size, &config, mode, input, ring_size)?;
    converter.set_query_delay(options.gpu_query_delay_ms);
    let effective_mode = match converter.effective_mode() {
        ConvertMode::Shader => "shader",
        ConvertMode::VideoProcessor => "vp",
    };
    let effective_input = match input {
        InputFormat::Nv12 => "nv12",
        InputFormat::Bgra => "bgra/ARGB",
    };
    let mut encoder = AutoEncoder::new(
        &gpu,
        config.clone(),
        converter.textures(),
        input == InputFormat::Bgra,
        options.encoder.requested(),
    )?;
    let mut encoder_kind = encoder.kind();
    config = madscreen_encode::config::effective_config(encoder_kind, &config);
    let mut governor_enabled =
        options.publishing() && options.governor_enabled(encoder_kind, config.compat_tuning);
    let selected_reduction_preference = reduction_preference(
        options,
        encoder_kind,
        config.compat_tuning,
        matches!(converter.effective_mode(), ConvertMode::Shader),
    );
    converter.set_reduction_preference(selected_reduction_preference)?;
    let governor_cpu_signals = options.governor_cpu_signals(encoder_kind);
    // Só o governador de CPU limita a captura ao FPS alvo; no NVENC (só banda) a captura
    // segue como antes e o limite entra apenas quando um degrau for aplicado.
    if governor_enabled && governor_cpu_signals {
        let target_fps = config.fps.as_f64().ceil() as u32;
        capture_options.target_fps = Some(target_fps);
        if let Some(source) = &capture {
            source.set_target_fps(Some(target_fps));
        }
    }
    reportln!(options.ipc; "Encoder selecionado: {}", encoder_kind.name());
    if let Some(actual) = encoder.bitstream_profile_level_id() {
        reportln!(options.ipc; "SPS do encoder: profile-level-id {actual}");
        if let Some(actual_level) = h264_level_from_profile_level_id(actual) {
            let planned = config.h264_level.unwrap_or(0);
            if actual_level > planned {
                reportln!(options.ipc;
                    "O encoder elevou o nível H.264 de {}.{} para {}.{} para caber na saída",
                    planned / 10, planned % 10, actual_level / 10, actual_level % 10
                );
                config.h264_level = Some(actual_level);
            }
        }
    }
    let mut corpus = Vec::new();
    if let Some(directory) = &options.synthetic_frames {
        anyhow::ensure!(capture.is_none(), "corpus exige fonte sintetica");
        let expected = source_size.0 as usize * source_size.1 as usize * 4;
        anyhow::ensure!(
            expected <= 64 * 1024 * 1024,
            "quadro de corpus maior que 64 MiB"
        );
        for index in 0..8 {
            let path = directory.join(format!("{index:03}.bgra"));
            anyhow::ensure!(
                std::fs::metadata(&path)?.len() == expected as u64,
                "corpus BGRA com tamanho incorreto"
            );
            corpus.push(std::fs::read(path)?);
        }
    }
    let mut synthetic = Vec::with_capacity(ring_size);
    if capture.is_none() {
        for _ in 0..ring_size {
            let mut source = SyntheticSource::new(&gpu, source_size)?;
            if matches!(options.synthetic, SyntheticArg::Static) {
                source.render(0)?;
            }
            synthetic.push(source);
        }
        if matches!(options.synthetic, SyntheticArg::Static) {
            converter.finish()?;
        }
    }
    let frames_csv = options
        .frames_csv
        .as_ref()
        .map(|p| -> Result<_> {
            let writer = csv::WriterBuilder::new()
                .has_headers(true)
                .from_writer(BufWriter::new(File::create(profile_path(
                    p,
                    preset,
                    options.compare_presets,
                ))?));
            Ok(writer)
        })
        .transpose()?;
    let frequency = qpc_frequency()?;
    let timer = Timer::new()?;
    let mut sampling = Sampling::new(options.warmup, adapter.name.clone())?;
    // CPU do engine por amostra de diagnóstico; a primeira cobre desde o início da sessão.
    let mut process_cpu = crate::cpu_ffi::ProcessCpu::new();
    let mut governor_cpu = crate::cpu_ffi::ProcessCpu::new();
    let logical_cpus = std::thread::available_parallelism().map_or(1., |n| n.get() as f64);
    let start = qpc()?;
    let mut pacer = Pacer::new(start, frequency, config.fps)?;
    let mut collector = Collector {
        discard_output: false,
        keyframe_accepted: false,
        au: AuBuffer::new(config.au_capacity()),
        output: Output::new(
            options,
            preset,
            &config,
            governor_enabled,
            governor_cpu_signals,
        )
        .map_err(|error| {
            let error = if options.publishing() {
                publish_diagnostic(error, encoder_kind.name(), &config)
            } else {
                error
            };
            error.context(crate::ipc::FailureStage(
                madscreen_ipc::ErrorCode::Transport,
            ))
        })?,
        csv: frames_csv,
        measurements: Measurements::new(),
        rows: VecDeque::with_capacity(ring_size),
        measured_start: None,
        frequency,
        independent_gpu_metrics: options.independent_gpu_metrics,
        gpu_metrics_unavailable: 0,
        measure_generation: capture.is_none()
            && matches!(options.synthetic, SyntheticArg::Animated),
    };
    let mut slots = FlightSlots::new(ring_size, options.inflight as usize)?;
    if let Some(session) = session {
        session.set_diagnostic_clock(collector.output.diagnostic_clock());
    }
    let mut conversions = VecDeque::<(usize, FrameTiming)>::with_capacity(ring_size);
    let mut capture_baseline = None;
    let mut last_stats = 0.;
    let mut produced = 0;
    let mut skipped = 0u64;
    let mut late = 0u64;
    let mut full_drops = 0u64;
    let mut saturation = madscreen_encode::pipeline::Saturation::default();
    let mut throttled = 0u64;
    let mut last_capture_encode: Option<i64> = None;
    // Captura do último quadro entregue pela fonte, para distinguir repetição de quadro novo.
    let mut last_captured_qpc: Option<i64> = None;
    let mut capture_fps = config.fps;
    // Período em ticks do QPC para a grade de captura (FPS pedido).
    let mut capture_period = (i128::from(frequency) * i128::from(capture_fps.den)
        / i128::from(capture_fps.num.max(1)))
    .clamp(1, i128::from(i64::MAX)) as i64;
    let mut governor_last_ms = 0u64;
    let mut governor_last_captured = 0u64;
    let mut governor_last_submitted = 0u64;
    let mut pending_resolution: Option<Size> = None;
    let mut capture_pending = false;
    let mut redraw = false;
    let mut next_window_check = start;
    let mut occupancy = madscreen_encode::pipeline::Occupancy::new(start);
    let serial = options.inflight == 1;
    let mut status_window = crate::ipc::StatusWindow::default();
    use madscreen_ipc::privacy::{Operation, PauseBarrier, PrivacySession, Request, VideoState};
    let mut privacy: Option<PrivacySession> = None;
    let mut privacy_operation: Option<Operation> = None;
    let mut privacy_deadline = 0i64;
    let mut privacy_draining = false;
    let mut neutral = false;
    let mut neutral_source: Option<SyntheticSource> = None;
    let mut minimum_acquisition_qpc = i64::MIN;
    if let Some(session) = session {
        session.started(options, &config, plan.as_ref(), encoder_kind)?;
        if let Some((active, failed)) = collector.output.audio_status() {
            session.emit(&madscreen_ipc::Event::Audio { active, failed })?;
        }
    }
    reportln!(options.ipc;
        "Codificando {:?} {:?}, conversão={}, entrada={}, inflight={}, sync={:?}. Ctrl+C encerra e salva relatórios.",
        config.codec,
        config.preset,
        effective_mode,
        effective_input,
        options.inflight,
        options.sync
    );
    loop {
        if !running.load(Ordering::Acquire) {
            break;
        }
        if collector.output.is_terminated() {
            return Err(anyhow::anyhow!("A sessão de compartilhamento foi encerrada. Inicie outra transmissão para continuar.")
                .context(crate::ipc::FailureStage(madscreen_ipc::ErrorCode::Transport)));
        }
        let now = qpc()?;
        if let Some(session) = session {
            while let Some(request) = session.take_privacy() {
                let operation = match request {
                    Request::Pause { operation }
                    | Request::Resume { operation }
                    | Request::SetAudioMuted { operation, .. } => operation,
                };
                let machine = privacy
                    .get_or_insert_with(|| PrivacySession::new(operation.generation, false, false));
                machine.request(request)?;
                if let Request::SetAudioMuted { muted, .. } = request {
                    collector.output.set_audio_muted(muted);
                    session.emit(&madscreen_ipc::Event::Privacy {
                        operation,
                        video: machine.video(),
                        audio_muted: muted,
                    })?;
                } else {
                    privacy_operation = Some(operation);
                    privacy_deadline = now.saturating_add(5 * frequency);
                    privacy_draining = true;
                    collector.discard_output = true;
                    collector.keyframe_accepted = false;
                }
            }
        }
        if privacy_operation.is_some() && now > privacy_deadline {
            anyhow::bail!(
                "A pausa/retomada não foi confirmada a tempo; transmissão encerrada por segurança"
            );
        }
        if collector.measured_start.is_none()
            && (now - start) as f64 / frequency as f64 >= f64::from(options.warmup)
        {
            collector.measured_start = Some(now);
            capture_baseline = capture_stats(&capture)?;
            governor_last_captured = capture_baseline.map_or(0, |stats| stats.frames);
            governor_last_submitted = produced;
            governor_cpu = crate::cpu_ffi::ProcessCpu::new();
            skipped = 0;
            late = 0;
            collector.gpu_metrics_unavailable = 0;
            full_drops = 0;
            saturation = madscreen_encode::pipeline::Saturation::default();
            throttled = 0;
            occupancy = madscreen_encode::pipeline::Occupancy::new(now);
            occupancy.observe(now, slots.count());
        }
        let elapsed = collector
            .measured_start
            .map_or(0., |s| (now - s) as f64 / frequency as f64);
        if collector.measured_start.is_some()
            && options.duration > 0
            && elapsed >= f64::from(options.duration)
        {
            break;
        }
        // Conversões em ordem de submissão; fence consulta somente o valor concluído.
        while let Some(&(slot, timing)) = conversions.front() {
            if !converter.ready(slot)? {
                break;
            }
            match encoder.encode(&encoder.surface_at(slot)?, timing) {
                Err(madscreen_encode::EncodeError::Busy) => break,
                result => result?,
            }
            conversions.pop_front();
        }
        collector.poll(&mut encoder, &converter, &mut slots, false)?;
        if privacy_draining && slots.count() == 0 {
            // Nenhuma conversão/AU da época anterior permanece no pipeline.
            neutral = privacy
                .as_ref()
                .is_some_and(|p| p.video() == VideoState::Pausing);
            if neutral && neutral_source.is_none() {
                neutral_source = Some(SyntheticSource::paused(&gpu, source_size)?);
            }
            minimum_acquisition_qpc = qpc()?;
            if !neutral && let Some(source) = capture.as_mut() {
                // Uma sessão nova entrega inclusive a imagem estática atual.
                // Não reusar textura/timestamp da época anterior à retomada.
                source
                    .refresh(capture_options)
                    .context(crate::ipc::FailureStage(madscreen_ipc::ErrorCode::Capture))?;
            }
            collector.discard_output = false;
            collector.keyframe_accepted = false;
            privacy_draining = false;
            capture_pending = true;
            redraw = true;
            last_capture_encode = None;
            encoder.request_keyframe();
        }
        if !privacy_draining
            && collector.keyframe_accepted
            && let Some(operation) = privacy_operation.take()
            && let Some(machine) = privacy.as_mut()
        {
            if neutral {
                machine.confirm_pause(
                    operation,
                    PauseBarrier {
                        source_blocked: true,
                        pending_frames_discarded: true,
                        neutral_frame_ready: true,
                    },
                )?;
            } else {
                machine.confirm_resume(operation, true)?;
            }
            if let Some(session) = session {
                session.emit(&madscreen_ipc::Event::Privacy {
                    operation,
                    video: machine.video(),
                    audio_muted: machine.desired_audio_muted(),
                })?;
            }
        }
        if collector.output.wants_redraw(&mut encoder)? && capture.is_some() {
            capture_pending = true;
            redraw = true;
        }
        occupancy.observe(qpc()?, slots.count());
        if capture
            .as_ref()
            .is_some_and(CaptureSource::needs_wgc_switch)
            && let Some(target) = target
        {
            // HDR ligado durante a live com Desktop Duplication: mesmo monitor via WGC, sem
            // encerrar a sessão. Contadores de captura recomeçam na fonte nova.
            if let Some(mut previous) = capture.take() {
                previous.stop();
            }
            let replacement = CaptureSource::start(
                gpu.clone(),
                target,
                capture_options,
                madscreen_capture::CaptureBackend::Wgc,
                capture_notify.clone(),
            )
            .context(crate::ipc::FailureStage(madscreen_ipc::ErrorCode::Capture))?;
            reportln!(options.ipc; "HDR ligado no monitor: captura trocada para {}", replacement.backend_name());
            capture = Some(replacement);
            capture_pending = true;
            redraw = true;
        }
        if let Some(source) = &capture {
            if let Some(error) = source.take_error() {
                return Err(anyhow::Error::new(error)
                    .context(crate::ipc::FailureStage(madscreen_ipc::ErrorCode::Capture)));
            }
            // O Closed do WGC nem sempre chega quando a janela some (visto com Chrome fechado):
            // a cada 0,5 s confere se o HWND ainda existe, senao a sessao fica ao vivo a 0 FPS.
            let window_gone = match watched_window {
                Some(hwnd) if now >= next_window_check => {
                    next_window_check = now.saturating_add(frequency / 2);
                    !crate::sources::window_exists(hwnd)
                }
                _ => false,
            };
            if source.is_closed() || window_gone {
                if let Some(session) = session {
                    session.stop(madscreen_ipc::StopReason::SourceClosed);
                }
                reportln!(options.ipc; "Fonte fechada; encerrando captura.");
                break;
            }
        }
        if let Some(desired) = pending_resolution
            && slots.count() == 0
            && conversions.is_empty()
        {
            let mut requested_config = base_config.clone();
            requested_config.width = desired.width;
            requested_config.height = desired.height;
            // base_config guarda o FPS configurado; o degrau atual pode ser menor.
            requested_config.fps = capture_fps;
            // O publicador não é avisado da recriação: abrir no teto inicial passaria da banda
            // que o WebRTC está permitindo até a estimativa cair de novo.
            requested_config.bitrate_bps = rebuild_bitrate_bps(
                base_config.bitrate_bps,
                collector.output.applied_bitrate_bps(),
            );
            let rebuilt = (|| -> Result<(Converter, AutoEncoder, EncoderConfig)> {
                requested_config.validate()?;
                let mut next_converter = Converter::new_ring(
                    &gpu,
                    source_size,
                    &requested_config,
                    mode,
                    input,
                    ring_size,
                )?;
                next_converter.set_query_delay(options.gpu_query_delay_ms);
                let next_encoder = AutoEncoder::new(
                    &gpu,
                    requested_config.clone(),
                    next_converter.textures(),
                    input == InputFormat::Bgra,
                    options.encoder.requested(),
                )?;
                let effective = madscreen_encode::config::effective_config(
                    next_encoder.kind(),
                    &requested_config,
                );
                let preference = reduction_preference(
                    options,
                    next_encoder.kind(),
                    effective.compat_tuning,
                    matches!(next_converter.effective_mode(), ConvertMode::Shader),
                );
                next_converter.set_reduction_preference(preference)?;
                Ok((next_converter, next_encoder, effective))
            })();
            match rebuilt {
                Ok((next_converter, next_encoder, effective)) => {
                    converter = next_converter;
                    encoder = next_encoder;
                    encoder_kind = encoder.kind();
                    config = effective;
                    if !options.governor_enabled(encoder_kind, config.compat_tuning) {
                        collector.output.disable_governor();
                        governor_enabled = false;
                        restore_capture_cadence(
                            &capture,
                            &mut capture_options,
                            &mut capture_fps,
                            &mut capture_period,
                            base_config.fps,
                            options.capture_limit,
                            frequency,
                        );
                    }
                    capture_pending = true;
                    redraw = true;
                    last_capture_encode = None;
                    tracing::info!(
                        width = config.width,
                        height = config.height,
                        "Saída do encoder recriada após troca de resolução"
                    );
                }
                Err(error) => {
                    tracing::warn!(%error, "Troca de resolução falhou; mantendo encoder anterior e desligando ajuste automático");
                    collector.output.disable_governor();
                    governor_enabled = false;
                    restore_capture_cadence(
                        &capture,
                        &mut capture_options,
                        &mut capture_fps,
                        &mut capture_period,
                        base_config.fps,
                        options.capture_limit,
                        frequency,
                    );
                }
            }
            pending_resolution = None;
        }
        let due = if privacy_draining {
            false
        } else if neutral {
            capture_pending = true;
            capture_due(now, last_capture_encode, frequency, capture_fps)
                && pending_resolution.is_none()
        } else if capture.is_some() {
            let notified = frame_event.wait(0)?;
            capture_pending |= notified;
            let due = capture_due(now, last_capture_encode, frequency, capture_fps);
            if notified && !due && collector.measured_start.is_some() {
                throttled += 1;
            }
            capture_pending && due && pending_resolution.is_none()
        } else {
            // Fonte sintética segue o degrau do governador como a captura real; sem isto o
            // encoder orça a 30 FPS, recebe 60 e o governador lê estouro de banda permanente.
            now >= pacer.deadline()
                && (capture_fps == base_config.fps
                    || capture_due(now, last_capture_encode, frequency, capture_fps))
        };
        if due {
            let slot = slots.reserve();
            if options.pipeline_saturation_metrics {
                saturation.observe(now, slot.is_none());
            }
            if let Some(slot) = slot {
                occupancy.observe(qpc()?, slots.count());
                let mut timing = FrameTiming {
                    slot,
                    ..Default::default()
                };
                let mut available = false;
                if neutral {
                    if let Some(source) = neutral_source.as_mut() {
                        let frame = source.frame()?;
                        timing.qpc_capture = frame.qpc_capture;
                        timing.qpc_convert_start = qpc()?;
                        converter.begin_frame(slot);
                        converter.begin_convert(slot);
                        converter.submit(frame, slot)?;
                        available = true;
                    }
                } else if !synthetic.is_empty() {
                    converter.begin_frame(slot);
                    let frame = if !corpus.is_empty() {
                        synthetic[slot].upload(&corpus[produced as usize % corpus.len()])?
                    } else if matches!(options.synthetic, SyntheticArg::Static) {
                        synthetic[slot].frame()?
                    } else {
                        synthetic[slot].render(produced)?
                    };
                    timing.qpc_capture = frame.qpc_capture;
                    timing.qpc_convert_start = qpc()?;
                    converter.begin_convert(slot);
                    converter.submit(frame, slot)?;
                    available = true;
                } else if let Some(source) = &capture {
                    if let Some(lease) = source.latest_frame(redraw)? {
                        redraw = false;
                        if let Some(frame) = lease
                            .frame()
                            .filter(|frame| frame.qpc_acquired >= minimum_acquisition_qpc)
                        {
                            let now_qpc = qpc()?;
                            timing.qpc_capture = presented_capture_qpc(
                                frame.qpc_capture,
                                last_captured_qpc,
                                now_qpc,
                            );
                            last_captured_qpc = Some(frame.qpc_capture);
                            timing.qpc_convert_start = now_qpc;
                            converter.begin_frame(slot);
                            converter.begin_convert(slot);
                            converter.submit(frame, slot)?;
                            available = true;
                        }
                    }
                    capture_pending = redraw;
                }
                if available {
                    // Lease já liberado; Flush envia trabalho sem aguardar execução.
                    converter.flush_slot(slot)?;
                    timing.qpc_convert_end = qpc()?;
                    if serial {
                        // Modo de comparação explicitamente serial, reproduz a espera antiga.
                        converter.drain_slot(slot)?;
                        encoder.encode(&encoder.surface_at(slot)?, timing)?;
                        collector.poll(&mut encoder, &converter, &mut slots, true)?;
                    } else if matches!(options.sync, SyncArg::Flush) && conversions.is_empty() {
                        match encoder.encode(&encoder.surface_at(slot)?, timing) {
                            Err(madscreen_encode::EncodeError::Busy) => {
                                conversions.push_back((slot, timing))
                            }
                            result => result?,
                        }
                    } else {
                        conversions.push_back((slot, timing));
                    }
                    if capture.is_some() || capture_fps != base_config.fps {
                        // Grade fixa no FPS pedido: com o monitor acima do alvo (ex.: 179 Hz) o
                        // intervalo mínimo de 90% sozinho deixava passar ~65 FPS para 60 pedidos.
                        // Atraso de dois períodos ou mais (tela parada) reinicia a grade.
                        last_capture_encode = Some(match last_capture_encode {
                            Some(last) if now.saturating_sub(last) < 2 * capture_period => {
                                last + capture_period
                            }
                            _ => now,
                        });
                    }
                    if collector.measured_start.is_some()
                        && capture.is_none()
                        && now > pacer.deadline()
                    {
                        late += 1;
                    }
                    produced += 1;
                } else {
                    slots.release(slot)?;
                }
            } else if capture.is_none() && collector.measured_start.is_some() {
                full_drops += 1;
            }
            if capture.is_none() {
                let omitted = pacer.advance(qpc()?);
                if collector.measured_start.is_some() {
                    skipped += omitted;
                }
            }
        }
        if !due && options.pipeline_saturation_metrics {
            saturation.observe(now, false);
        }
        occupancy.observe(qpc()?, slots.count());
        // Sem captura (fonte sintética de laboratório) a contagem cai para os quadros submetidos.
        if governor_enabled && collector.measured_start.is_some() {
            let at_ms = (elapsed * 1000.) as u64;
            if at_ms.saturating_sub(governor_last_ms) >= 1000 {
                let captured_total = capture_stats(&capture)?.map_or(0, |stats| stats.frames);
                let submitted = produced.saturating_sub(governor_last_submitted);
                // Static redraw/refinement can submit without a new WGC frame.
                let captured = captured_total
                    .saturating_sub(governor_last_captured)
                    .max(submitted);
                let process_cpu_cores = governor_cpu
                    .sample()
                    .map(|percent| percent * logical_cpus / 100.);
                let changed =
                    collector
                        .output
                        .governor_tick(at_ms, captured, submitted, process_cpu_cores);
                governor_last_ms = at_ms;
                governor_last_captured = captured_total;
                governor_last_submitted = produced;
                if let Some(target) = changed {
                    capture_fps = FrameRate::new(target.fps, 1)?;
                    capture_period = (i128::from(frequency) * i128::from(capture_fps.den)
                        / i128::from(capture_fps.num))
                    .clamp(1, i128::from(i64::MAX)) as i64;
                    capture_options.target_fps = Some(target.fps);
                    if let Some(source) = &capture {
                        source.set_target_fps(Some(target.fps));
                    }
                    last_capture_encode = None;
                    let desired = match target.resolution.height() {
                        None => Size {
                            width: base_config.width,
                            height: base_config.height,
                        },
                        Some(height) => Size {
                            width: ((u64::from(base_config.width) * u64::from(height)
                                / u64::from(base_config.height))
                                as u32)
                                & !1,
                            height,
                        },
                    };
                    if desired.width != config.width || desired.height != config.height {
                        pending_resolution = Some(desired);
                    }
                    tracing::info!(fps = target.fps, ?target.resolution, "Degrau de captura ajustado");
                }
            }
        }
        if collector.measured_start.is_some()
            && elapsed - last_stats >= f64::from(options.stats_interval)
        {
            let capture_now = capture_stats(&capture)?;
            let mut diagnostic = collector.output.diagnostic_sample();
            if let Some(sample) = &mut diagnostic {
                sample.frames_submitted = produced;
                sample.captured_frames = capture_now.map(|stats| stats.frames);
                sample.capture_overwritten = capture_now.map(|stats| stats.overwritten);
                sample.capture_contention_drops = capture_now.map(|stats| stats.contention_drops);
                sample.pipeline_full_drops = full_drops;
                sample.gpu_metrics_unavailable = Some(collector.gpu_metrics_unavailable);
                sample.gpu_metrics_independent = Some(options.independent_gpu_metrics);
                if options.pipeline_saturation_metrics {
                    let (episodes, milliseconds) = saturation.snapshot(now, frequency);
                    sample.pipeline_blocked_episodes = Some(episodes);
                    sample.pipeline_blocked_ms = Some(milliseconds);
                }
                sample.pacing_intervals_skipped = skipped;
                sample.inflight = slots.count();
                sample.encoder = Some(encoder_kind.id().into());
                sample.encoder_fallback = encoder.fallback_reason().map(Into::into);
                sample.output_width = Some(config.width);
                sample.output_height = Some(config.height);
                sample.fps_target = Some(config.fps.as_f64());
                sample.vbv_frames = Some(config.vbv_frames);
                sample.process_cpu_percent = process_cpu.sample();
                sample.reduction_filter = Some(converter.reduction_filter().name().into());
                let compatibility = madscreen_encode::config::compatibility_backend(encoder_kind);
                let rollback = !config.compat_tuning;
                let screen_active = !rollback
                    && (encoder_kind == BackendKind::Qsv
                        || (encoder_kind == BackendKind::X264
                            && config.fps.as_f64() <= 30.
                            && config.x264_screen_preset_override != Some(false)));
                sample.screen_preset_tuning = Some(tuning_decision(
                    screen_active,
                    if rollback {
                        "MADSCREEN_COMPAT_TUNING=off"
                    } else if screen_active {
                        "politica de compatibilidade"
                    } else {
                        "backend/FPS ou override nao elegivel"
                    },
                ));
                let vbv_active = !rollback
                    && matches!(encoder_kind, BackendKind::X264 | BackendKind::Qsv)
                    && config.vbv_frames > 1;
                sample.compat_vbv_tuning = Some(tuning_decision(
                    vbv_active,
                    if rollback {
                        "MADSCREEN_COMPAT_TUNING=off"
                    } else if vbv_active {
                        "VBV efetivo do backend"
                    } else {
                        "backend excluido ou override de 1 quadro"
                    },
                ));
                sample.load_governor_tuning = Some(tuning_decision(
                    governor_enabled,
                    if rollback {
                        "MADSCREEN_COMPAT_TUNING=off"
                    } else if governor_enabled {
                        "compatibilidade ou opt-in CLI"
                    } else {
                        "backend excluido, opt-out ou falha de recriacao"
                    },
                ));
                sample.async_readback_tuning = Some(tuning_decision(
                    config.async_readback,
                    if rollback {
                        "MADSCREEN_COMPAT_TUNING=off"
                    } else if config.async_readback {
                        "compatibilidade, AMF ou opt-in CLI"
                    } else {
                        "NVENC ou opt-out CLI"
                    },
                ));
                let box_active = converter.reduction_filter()
                    == madscreen_encode::convert::reduction::ReductionFilter::Box4x4;
                sample.reduction_box_tuning = Some(tuning_decision(
                    box_active,
                    if rollback {
                        "MADSCREEN_COMPAT_TUNING=off"
                    } else if box_active {
                        "reducao shader no backend elegivel"
                    } else if !compatibility {
                        "NVENC/AMF"
                    } else {
                        "resolucao nativa, VP ou opt-out CLI"
                    },
                ));
                if let Some(file) = &mut diagnostic_file {
                    serde_json::to_writer(&mut *file, sample)?;
                    file.write_all(b"\n")?;
                    file.flush()?;
                }
            }
            if let Some(session) = session {
                session.status(
                    &mut status_window,
                    elapsed,
                    &collector.measurements,
                    capture_now.zip(capture_baseline),
                    full_drops,
                    (collector.output.is_connected(), diagnostic),
                )?;
            }
            reportln!(options.ipc;
                "stats: {:.1}s frames={} encode_fps={:.2} encode_p95_ms={:?} em_voo={} intervalos_pulados={}",
                elapsed,
                collector.measurements.frames,
                collector.measurements.frames as f64 / elapsed,
                collector.measurements.encode_stats().p95,
                slots.count(),
                skipped
            );
            last_stats = elapsed;
        }
        gpu.check()?;
        // Espera apenas por cadência/fonte. Com trabalho em voo, revisita polls em ~0,5 ms.
        let current = qpc()?;
        let poll_deadline = current.saturating_add((frequency / 2000).max(1));
        if capture.is_none() {
            let deadline = if slots.count() > 0 {
                pacer.deadline().min(poll_deadline)
            } else {
                pacer.deadline()
            };
            timer.wait_until(deadline, frequency, running)?;
        } else if capture_pending {
            // A mailbox conserva só o mais recente enquanto o limite mínimo/anel impede envio.
            timer.wait_until(poll_deadline, frequency, running)?;
        } else {
            capture_pending = if slots.count() > 0 {
                timer.wait_for_frame(&frame_event, poll_deadline, frequency)?
            } else {
                // Com a tela parada o refino repete o quadro a cada ~16 ms; esperar 100 ms o
                // limitava a ~10 repetições por segundo (medido no diagnóstico de 10/nov).
                frame_event.wait(if collector.output.refining() { 4 } else { 100 })?
            };
        }
    }
    saturation.observe(qpc()?, false);
    sampling.stop();
    let final_capture = capture_stats(&capture)?;
    let clock_offset = final_capture.map(|s| s.clock_offset);
    let captured = final_capture
        .zip(capture_baseline)
        .map(|(s, b)| s.frames.saturating_sub(b.frames));
    if let Some(source) = capture.as_mut() {
        source.stop();
    }
    // Stop não publica AUs restantes, inclusive se ocorrer no meio de uma pausa.
    collector.discard_output = true;
    // Único caminho de espera de GPU fora da comparação serial: drenagem final limitada.
    while let Some((slot, timing)) = conversions.pop_front() {
        converter.drain_slot(slot)?;
        loop {
            match encoder.encode(&encoder.surface_at(slot)?, timing) {
                Err(madscreen_encode::EncodeError::Busy) => {
                    collector.poll(&mut encoder, &converter, &mut slots, true)?
                }
                result => {
                    result?;
                    break;
                }
            }
        }
    }
    loop {
        match encoder.end_of_stream() {
            Err(madscreen_encode::EncodeError::Busy) => {
                collector.poll(&mut encoder, &converter, &mut slots, true)?
            }
            result => {
                result?;
                break;
            }
        }
    }
    collector.poll(&mut encoder, &converter, &mut slots, true)?;
    let drain_deadline = qpc()?.saturating_add(5 * frequency);
    while slots.count() > 0 {
        collector.finish_queries(&converter, &mut slots)?;
        if qpc()? > drain_deadline {
            anyhow::bail!("timeout na drenagem das queries GPU");
        }
        if slots.count() > 0 {
            timer.wait_until(qpc()? + frequency / 1000, frequency, &AtomicBool::new(true))?;
        }
    }
    let end = qpc()?;
    occupancy.observe(end, 0);
    let seconds = collector
        .measured_start
        .map_or(0., |s| (end - s).max(0) as f64 / frequency as f64);
    let mut report = collector.measurements.report(
        config,
        match &options.source {
            Source::Synthetic => "synthetic".into(),
            Source::Picker => "picker".into(),
            Source::Monitor(n) => format!("monitor:{n}"),
            Source::Window(t) => format!("window:{t}"),
            Source::WindowHandle(h) => format!("hwnd:{h}"),
        },
        effective_mode,
        effective_input,
        seconds,
        captured.map_or(full_drops, |n| {
            n.saturating_sub(collector.measurements.frames)
        }),
        skipped,
        captured,
    );
    report.capture_backend = capture.as_ref().map(CaptureSource::backend_name);
    report.capture_clock_offset = clock_offset.filter(|c| c.samples > 0).map(|c| {
        serde_json::json!({
            "samples": c.samples,
            "min_ms": c.min_ms,
            "mean_ms": c.mean_ms,
            "max_ms": c.max_ms,
            "future_frames": c.future_frames,
        })
    });
    report.gpu_metrics_unavailable = collector.gpu_metrics_unavailable;
    report.independent_gpu_metrics = options.independent_gpu_metrics;
    report.gpu_query_delay_ms = options.gpu_query_delay_ms;
    report.reduction_filter = converter.reduction_filter().name();
    report.inflight_limit = options.inflight;
    report.encoder = encoder_kind.name();
    if encoder_kind != madscreen_encode::backend::BackendKind::Nvenc {
        report.no_readback = "Readback NV12 GPU→CPU no backend de compatibilidade; medir Copy/PCIe/CPU e overhead no jogo.";
        report.acceptance_m02 = "N/A: aceite M0.2 exclusivo do caminho NVENC zero-copy";
    }
    report.inflight_mean = occupancy.mean();
    report.inflight_max = occupancy.max();
    report.pacer_late_frames = late;
    report.pipeline_full_drops = full_drops;
    if options.pipeline_saturation_metrics {
        let (episodes, milliseconds) = saturation.snapshot(end, frequency);
        report.pipeline_blocked_episodes = Some(episodes);
        report.pipeline_blocked_ms = Some(milliseconds);
    }
    report.capture_throttled_notifications = throttled;
    report.sync = if serial {
        "serial/fence".into()
    } else {
        format!("{:?}", options.sync).to_lowercase()
    };
    report.synthetic = format!("{:?}", options.synthetic).to_lowercase();
    report.gpu_priority = serde_json::to_value(priority)?;
    drop(encoder);
    collector.output.finish()?;
    if let Some(csv) = &mut collector.csv {
        csv.flush()?;
    }
    let batch = sampling.finish()?;
    report.set_gpu_utilization(&batch.utilization);
    if report.gpu_percent_mean.is_none() {
        reportln!(options.ipc;
            "Uso da GPU selecionada indisponível: telemetria ausente ou nome de adapter ambíguo/diferente."
        );
    }
    report.gpu_samples = serde_json::to_value(batch.samples)?;
    Ok(Some(report))
}

struct Collector {
    discard_output: bool,
    keyframe_accepted: bool,
    au: AuBuffer,
    output: Output,
    csv: Option<csv::Writer<BufWriter<File>>>,
    measurements: Measurements,
    rows: VecDeque<(FrameRow, FrameTiming)>,
    measured_start: Option<i64>,
    frequency: i64,
    measure_generation: bool,
    independent_gpu_metrics: bool,
    gpu_metrics_unavailable: u64,
}
impl Collector {
    fn poll(
        &mut self,
        encoder: &mut AutoEncoder,
        converter: &Converter,
        slots: &mut FlightSlots,
        drain: bool,
    ) -> Result<()> {
        loop {
            let available = if drain {
                encoder.poll_into(&mut self.au)?
            } else {
                encoder.try_poll_into(&mut self.au)?
            };
            if !available {
                break;
            }
            if !self.discard_output {
                let accepted = self.output.write(&self.au, encoder)?;
                self.keyframe_accepted |=
                    accepted && self.au.kind == madscreen_encode::FrameKind::Key;
            }
            let row = FrameRow::from_au(&self.au, 0, self.frequency);
            self.rows.push_back((row, self.au.timing));
        }
        self.finish_queries(converter, slots)
    }
    fn finish_queries(&mut self, converter: &Converter, slots: &mut FlightSlots) -> Result<()> {
        while let Some((_, timing)) = self.rows.front() {
            let Some(times) = madscreen_encode::pipeline::resolve_gpu_sample(
                converter.gpu_times(timing.slot),
                self.independent_gpu_metrics,
            )?
            else {
                break;
            };
            if times.is_none() {
                self.gpu_metrics_unavailable += 1;
            }
            let Some((mut row, timing)) = self.rows.pop_front() else {
                break;
            };
            if let Some((gen_ms, convert_ms)) = times {
                row.gen_gpu_ms = self.measure_generation.then_some(gen_ms);
                row.convert_gpu_ms = Some(convert_ms);
            }
            if let Some(start) = self
                .measured_start
                .filter(|&start| timing.qpc_convert_start >= start)
            {
                row.measured_index = self.measurements.frames;
                self.measurements.record(
                    &row,
                    (timing.qpc_encode_end - start) as f64 / self.frequency as f64,
                );
                if let Some(csv) = &mut self.csv {
                    csv.serialize(&row)?;
                }
            }
            slots.release(timing.slot)?;
        }
        Ok(())
    }
}
