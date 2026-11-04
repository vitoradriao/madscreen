//! Saída opcional LiveKit/Cloudflare (feature `publish`): publica os access units H.264
//! do NVENC sem re-encode e repassa pedidos de keyframe/taxa do WebRTC ao encoder.
use crate::options::{Options, Transport};
use anyhow::Result;
use madscreen_audio::{AudioCapture, AudioSourceSpec};
use madscreen_encode::{AuBuffer, Encoder, EncoderConfig, FrameKind, FrameRate};
use madscreen_encode::{
    auto::AutoEncoder,
    platform::{qpc, qpc_frequency},
};
use madscreen_transport::{LiveKitPublisher, PublishConfig, epoch_us};
use madscreen_transport_cf::CfPublishConfig;

mod transport;
use transport::Publisher;

pub struct Live {
    publisher: Box<dyn Publisher>,
    lab_bitstream: Option<std::io::BufWriter<std::fs::File>>,
    lab_bytes: usize,
    audio: Option<AudioCapture>,
    audio_gate: madscreen_audio::mixer::MuteGate,
    ipc: bool,
    cap_bps: u32,
    floor_bps: u32,
    rate_ramp: Option<madscreen_encode::rate::RateRamp>,
    rate_origin: std::time::Instant,
    last_bps: u32,
    fps: FrameRate,
    frequency: i64,
    keyframe_requests: u64,
    keyframes_forced: u64,
    keyframe_pending: bool,
    keyframe_armed: bool,
    redraw_requested: bool,
    last_keyframe: i64,
    last_au: Option<i64>,
    delta_since_keyframe: bool,
    idle_keyframes: u64,
    rate_requests: u64,
    rate_applied: u64,
    rate_busy: u64,
    wanted_bps: Option<u32>,
    last_request_bps: u64,
    diagnostic: Option<DiagnosticWindow>,
}

struct DiagnosticWindow {
    origin: std::time::Instant,
    run_id: String,
    frames: u64,
    bytes: u64,
    keyframes: u64,
    last_au_bytes: usize,
    max_au_bytes: usize,
    capture_to_au_max_ms: f64,
    capture_to_convert_max_ms: Option<f64>,
    convert_submit_max_ms: Option<f64>,
    queue_wait_max_ms: Option<f64>,
    encode_wait_max_ms: f64,
    reconfigure_max_ms: f64,
}

impl DiagnosticWindow {
    fn new() -> Self {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut hash);
        std::process::id().hash(&mut hash);
        Self {
            origin: std::time::Instant::now(),
            run_id: format!("{:016x}", hash.finish()),
            frames: 0,
            bytes: 0,
            keyframes: 0,
            last_au_bytes: 0,
            max_au_bytes: 0,
            capture_to_au_max_ms: 0.,
            capture_to_convert_max_ms: None,
            convert_submit_max_ms: None,
            queue_wait_max_ms: None,
            encode_wait_max_ms: 0.,
            reconfigure_max_ms: 0.,
        }
    }
}

/// Intervalo mínimo entre IDRs forçados: o WebRTC pede keyframe a cada reconfiguração de taxa,
/// e uma rajada de IDRs de 1080p degrada a estimativa de banda e o bitrate efetivo.
const MIN_KEYFRAME_INTERVAL_S: f64 = 0.5;
const MIN_BITRATE_BPS: u64 = 1_000_000;

impl Live {
    pub fn is_connected(&self) -> Option<bool> {
        self.publisher.is_connected()
    }

    pub fn connect(options: &Options, config: &EncoderConfig) -> Result<Self> {
        anyhow::ensure!(
            config.codec == madscreen_encode::VideoCodec::H264,
            "publicação suporta apenas H.264 por enquanto (use --codec h264)"
        );
        let publisher: Box<dyn Publisher> = match options.transport {
            Transport::Livekit => {
                let publish = PublishConfig {
                    url: options.livekit_url.clone().unwrap_or_default(),
                    token: options.livekit_token.clone(),
                    api_key: options.livekit_api_key.clone(),
                    api_secret: options.livekit_api_secret.clone(),
                    room: options.room.clone(),
                    identity: options.identity.clone(),
                    track_name: "screen".into(),
                    width: config.width,
                    height: config.height,
                    max_bitrate_bps: u64::from(config.bitrate_bps),
                    max_fps: config.fps.num as f64 / config.fps.den as f64,
                    audio: options.audio_requested(),
                    preferred_h264_profile: {
                        // Explicit laboratory opt-in; never change the product default.
                        let codec44_local_high = std::env::var("MADSCREEN_CODEC44_LOCAL_HIGH")
                            .is_ok_and(|value| value == "1");
                        if codec44_local_high {
                            anyhow::ensure!(
                                options.livekit_url.as_deref() == Some("ws://127.0.0.1:17880")
                                    && matches!(
                                        config.h264_profile,
                                        madscreen_encode::config::H264Profile::High
                                    ),
                                "MADSCREEN_CODEC44_LOCAL_HIGH=1 exige SFU local ws://127.0.0.1:17880 e --h264-profile high"
                            );
                            Some("6400".into())
                        } else {
                            match config.h264_profile {
                                madscreen_encode::config::H264Profile::Main => Some("4d00".into()),
                                madscreen_encode::config::H264Profile::ConstrainedBaseline => {
                                    Some("42e0".into())
                                }
                                _ => None,
                            }
                        }
                    },
                    h264_level_idc: config.h264_level,
                };
                let publisher = LiveKitPublisher::connect(&publish)?;
                reportln!(options.ipc;
                    "Publicando em {} sala '{}' como '{}' ({}x{} H.264 pré-codificado, sem re-encode)",
                    publish.url, publish.room, publish.identity, config.width, config.height
                );
                Box::new(publisher)
            }
            Transport::Cloudflare => {
                let publish = CfPublishConfig {
                    worker_url: options.cf_url.clone().unwrap_or_default(),
                    token: options.cf_token.clone().unwrap_or_default(),
                    channel_id: options.cf_channel.clone(),
                    track_name: "screen".into(),
                    width: config.width,
                    height: config.height,
                    max_bitrate_bps: u64::from(config.bitrate_bps),
                    max_fps: config.fps.num as f64 / config.fps.den as f64,
                    // Perfil explícito substitui o fmtp. No caminho normal só o nível sobe:
                    // 6400LL (High sem constraints) não está na oferta do libwebrtc.
                    h264_profile_level_id: options.cf_profile_level_id.clone(),
                    preferred_h264_profile: match config.h264_profile {
                        madscreen_encode::config::H264Profile::Main => Some("4d00".into()),
                        madscreen_encode::config::H264Profile::ConstrainedBaseline => {
                            Some("42e0".into())
                        }
                        _ => None,
                    },
                    h264_level_idc: if options.cf_profile_level_id.is_none() {
                        config.h264_level
                    } else {
                        None
                    },
                    only_h264: true,
                    keepalive: options.cf_keepalive,
                    start_bitrate_bps: options.cf_start_bitrate.map(|mbps| (mbps * 1e6) as u64),
                    audio: options.audio_requested(),
                };
                if options.cf_profile_level_id.is_none()
                    && let Some(level) = publish.h264_level_idc
                {
                    reportln!(options.ipc;
                        "SDP conserva o perfil H.264 do libwebrtc e anuncia o nível {}.{} (perfil configurado {:?})",
                        level / 10,
                        level % 10,
                        config.h264_profile
                    );
                }
                let publisher = transport::connect_worker(publish).map_err(|error| {
                    if let Some(transport) = error.downcast_ref::<madscreen_transport_cf::Error>() {
                        let (summary, body) = transport.publish_notes();
                        anyhow::Error::new(madscreen_ipc::ShareDiagnostic { summary, body })
                    } else {
                        error
                    }
                })?;
                reportln!(options.ipc;
                    "Publicando pelo provedor selecionado no Worker ({}x{} H.264 pré-codificado, sem re-encode)",
                    config.width, config.height
                );
                publisher
            }
        };
        publisher.spawn_stats_logger(std::time::Duration::from_secs(5));
        let audio_gate = madscreen_audio::mixer::MuteGate::default();
        let audio = start_audio(options, publisher.as_ref(), &audio_gate);
        Ok(Self {
            publisher,
            lab_bitstream: options
                .lab_au_output
                .as_ref()
                .map(|path| {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)
                        .map(std::io::BufWriter::new)
                })
                .transpose()?,
            lab_bytes: 0,
            audio,
            audio_gate,
            ipc: options.ipc,
            cap_bps: config.bitrate_bps,
            floor_bps: options
                .min_bitrate
                .unwrap_or(config.bitrate_bps / 2)
                .max(MIN_BITRATE_BPS as u32)
                .min(config.bitrate_bps),
            rate_ramp: (options.rate_policy == crate::options::RatePolicyArg::AbsoluteRamp).then(
                || {
                    madscreen_encode::rate::RateRamp::new(
                        config.bitrate_bps,
                        options.min_bitrate.unwrap_or(MIN_BITRATE_BPS as u32),
                    )
                },
            ),
            rate_origin: std::time::Instant::now(),
            last_bps: config.bitrate_bps,
            fps: config.fps,
            frequency: qpc_frequency()?,
            keyframe_requests: 0,
            keyframes_forced: 0,
            keyframe_pending: false,
            keyframe_armed: false,
            redraw_requested: false,
            last_keyframe: i64::MIN / 2,
            last_au: None,
            delta_since_keyframe: false,
            idle_keyframes: 0,
            rate_requests: 0,
            rate_applied: 0,
            rate_busy: 0,
            wanted_bps: None,
            last_request_bps: 0,
            diagnostic: options.diagnostics.then(DiagnosticWindow::new),
        })
    }

    fn poll_keyframe_request(&mut self) {
        // Coalesce a flag rearmada pelo libwebrtc enquanto o IDR ainda está em voo.
        if self.publisher.take_keyframe_request() && !self.keyframe_pending && !self.keyframe_armed
        {
            self.keyframe_requests += 1;
            self.keyframe_pending = true;
        }
    }

    fn force_keyframe(&mut self, encoder: &mut AutoEncoder, now: i64, idle_refresh: bool) {
        let waited = (now - self.last_keyframe) as f64 / self.frequency as f64;
        if !self.keyframe_armed
            && (self.keyframe_pending || idle_refresh)
            && waited >= MIN_KEYFRAME_INTERVAL_S
        {
            if self.keyframe_pending {
                self.keyframes_forced += 1;
            } else {
                self.idle_keyframes += 1;
            }
            self.keyframe_pending = false;
            self.keyframe_armed = true;
            self.last_keyframe = now;
            encoder.request_keyframe();
        }
    }

    /// Consulta feedback mesmo sem AUs; o runner reutiliza o último quadro respeitando a cadência.
    pub fn wants_redraw(&mut self, encoder: &mut AutoEncoder) -> Result<bool> {
        self.poll_keyframe_request();
        let now = qpc()?;
        let idle_s = self
            .last_au
            .map_or(0., |at| (now - at).max(0) as f64 / self.frequency as f64);
        self.force_keyframe(encoder, now, self.delta_since_keyframe && idle_s >= 1.);
        let redraw_after_s = (2. * self.fps.den as f64 / self.fps.num as f64).max(0.05);
        if self.keyframe_armed && !self.redraw_requested && idle_s >= redraw_after_s {
            self.redraw_requested = true;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn write(&mut self, au: &AuBuffer, encoder: &mut AutoEncoder) -> Result<bool> {
        if let Some(file) = &mut self.lab_bitstream {
            use std::io::Write;
            if self.lab_bytes.saturating_add(au.data.len()) <= 16 * 1024 * 1024 {
                file.write_all(&au.data)?;
                self.lab_bytes += au.data.len();
            }
        }
        let now = qpc()?;
        let age_us = (now - au.timing.qpc_capture).max(0) * 1_000_000 / self.frequency;
        if let Some(window) = &mut self.diagnostic {
            window.frames += 1;
            window.bytes += au.data.len() as u64;
            window.keyframes += u64::from(au.kind == FrameKind::Key);
            window.last_au_bytes = au.data.len();
            window.max_au_bytes = window.max_au_bytes.max(au.data.len());
            window.capture_to_au_max_ms = window.capture_to_au_max_ms.max(age_us as f64 / 1000.);
            // Reutiliza timestamps já coletados, sem query/sincronização GPU adicional.
            let stage = |from: i64, to: i64| {
                (from > 0 && to >= from)
                    .then(|| to.saturating_sub(from) as f64 * 1000. / self.frequency as f64)
            };
            for (maximum, value) in [
                (
                    &mut window.capture_to_convert_max_ms,
                    stage(au.timing.qpc_capture, au.timing.qpc_convert_start),
                ),
                (
                    &mut window.convert_submit_max_ms,
                    stage(au.timing.qpc_convert_start, au.timing.qpc_convert_end),
                ),
                (
                    &mut window.queue_wait_max_ms,
                    stage(au.timing.qpc_convert_end, au.timing.qpc_encode_start),
                ),
            ] {
                if let Some(value) = value {
                    *maximum = Some(maximum.unwrap_or(0.).max(value));
                }
            }
            let encode_ms = (au.timing.qpc_encode_end - au.timing.qpc_encode_start).max(0) as f64
                * 1000.
                / self.frequency as f64;
            window.encode_wait_max_ms = window.encode_wait_max_ms.max(encode_ms);
        }
        let accepted =
            self.publisher
                .push(&au.data, au.kind == FrameKind::Key, epoch_us() - age_us);
        if !accepted {
            tracing::debug!(index = au.index, "transporte recusou o access unit");
        }
        self.last_au = Some(now);
        self.poll_keyframe_request();
        if au.kind == FrameKind::Key {
            self.last_keyframe = now;
            self.keyframe_pending = false;
            self.keyframe_armed = false;
            self.redraw_requested = false;
        }
        if accepted {
            self.delta_since_keyframe = au.kind != FrameKind::Key;
        }
        self.force_keyframe(encoder, now, false);
        if let Some(request) = self.publisher.take_rate_request() {
            self.rate_requests += 1;
            self.last_request_bps = request.bitrate_bps;
            let target = request
                .bitrate_bps
                .clamp(u64::from(self.floor_bps), u64::from(self.cap_bps))
                as u32;
            if let Some(ramp) = self.rate_ramp.as_mut() {
                ramp.request(request.bitrate_bps);
            } else {
                self.wanted_bps = Some(target);
            }
        }
        let rate_now_ms = self.rate_ramp.as_ref().map_or(0, |_| {
            u64::try_from(self.rate_origin.elapsed().as_millis()).unwrap_or(u64::MAX)
        });
        if let Some(ramp) = &self.rate_ramp {
            let minimum_ms = if encoder.kind() == madscreen_encode::backend::BackendKind::Nvenc {
                0
            } else {
                1000
            };
            self.wanted_bps = ramp.target_with_minimum_interval(rate_now_ms, minimum_ms);
        }
        // O pedido fica guardado até o encoder aceitar (reconfigurar exige o pipeline vazio).
        if let Some(target) = self.wanted_bps {
            let delta = target.abs_diff(self.last_bps);
            // RateRamp already applies its deadband, including exact cap/floor
            // convergence. A second legacy filter would discard its final step.
            if self.rate_ramp.is_none() && u64::from(delta) * 50 < u64::from(self.last_bps) {
                self.wanted_bps = None;
            } else {
                let reconfigure_started =
                    self.diagnostic.as_ref().map(|_| std::time::Instant::now());
                let result = encoder.reconfigure_rate(target, self.fps);
                if let (Some(window), Some(started)) = (&mut self.diagnostic, reconfigure_started) {
                    window.reconfigure_max_ms = window
                        .reconfigure_max_ms
                        .max(started.elapsed().as_secs_f64() * 1000.);
                }
                match result {
                    Ok(()) => {
                        tracing::info!(
                            pedido_bps = self.last_request_bps,
                            aplicado_bps = target,
                            "bitrate ajustado pelo WebRTC"
                        );
                        if let Some(ramp) = self.rate_ramp.as_mut() {
                            ramp.applied(target, rate_now_ms);
                        }
                        self.last_bps = target;
                        self.rate_applied += 1;
                        self.wanted_bps = None;
                    }
                    Err(madscreen_encode::EncodeError::Busy) => self.rate_busy += 1,
                    Err(error) => {
                        tracing::warn!(%error, "falha ao reconfigurar bitrate");
                        self.wanted_bps = None;
                    }
                }
            }
        }
        Ok(accepted)
    }

    pub fn set_audio_muted(&self, muted: bool) {
        self.audio_gate.set(muted);
    }

    pub fn audio_status(&self) -> Option<(u32, Vec<String>)> {
        let audio = self.audio.as_ref()?;
        Some((
            audio.active_sources().len() as u32,
            audio
                .failed_sources()
                .iter()
                .map(|(_, reason)| reason.clone())
                .collect(),
        ))
    }

    pub fn diagnostic_clock(&self) -> Option<(String, std::time::Instant)> {
        self.diagnostic
            .as_ref()
            .map(|window| (window.run_id.clone(), window.origin))
    }

    pub fn diagnostic_sample(&mut self) -> Option<madscreen_ipc::diagnostics::PublisherSample> {
        let window = self.diagnostic.as_mut()?;
        let (pushed, rejected) = self.publisher.counts();
        let (audio_pushed, audio_rejected) = self.publisher.audio_counts();
        let sample = madscreen_ipc::diagnostics::PublisherSample {
            schema: 1,
            run_id: window.run_id.clone(),
            at_ms: u64::try_from(window.origin.elapsed().as_millis()).unwrap_or(u64::MAX),
            provider: self.publisher.provider().into(),
            frames_completed: window.frames,
            bytes_produced: window.bytes,
            keyframes_produced: window.keyframes,
            last_au_bytes: window.last_au_bytes,
            max_au_bytes: window.max_au_bytes,
            capture_to_au_max_ms: window.capture_to_au_max_ms,
            capture_to_convert_max_ms: window.capture_to_convert_max_ms,
            convert_submit_max_ms: window.convert_submit_max_ms,
            queue_wait_max_ms: window.queue_wait_max_ms,
            encode_wait_max_ms: window.encode_wait_max_ms,
            pushed,
            rejected,
            audio_pushed,
            audio_rejected,
            audio_dropped_samples: self.audio.as_ref().map(AudioCapture::dropped_samples),
            rate_requests: self.rate_requests,
            rate_applied: self.rate_applied,
            rate_busy: self.rate_busy,
            requested_bitrate_bps: (self.last_request_bps > 0).then_some(self.last_request_bps),
            applied_bitrate_bps: self.last_bps,
            reconfigure_max_ms: window.reconfigure_max_ms,
            sender: self.publisher.sender_stats(),
            ..Default::default()
        };
        window.max_au_bytes = 0;
        window.capture_to_au_max_ms = 0.;
        window.capture_to_convert_max_ms = None;
        window.convert_submit_max_ms = None;
        window.queue_wait_max_ms = None;
        window.encode_wait_max_ms = 0.;
        window.reconfigure_max_ms = 0.;
        Some(sample)
    }

    pub fn finish(mut self) {
        if let Some(audio) = self.audio.take() {
            let dropped = audio.dropped_samples();
            drop(audio);
            let (pushed, rejected) = self.publisher.audio_counts();
            reportln!(self.ipc;
                "Áudio: {pushed} blocos de 10 ms entregues ao WebRTC, {rejected} recusados, {dropped} amostras descartadas"
            );
        }
        let (pushed, rejected) = self.publisher.counts();
        reportln!(self.ipc;
            "WebRTC: {pushed} access units entregues, {rejected} recusados, {} pedidos de keyframe ({} atendidos), {} IDRs de repouso, {} de taxa ({} aplicados, {} adiados por pipeline ocupado), {} tentativas de reconexão",
            self.keyframe_requests,
            self.keyframes_forced,
            self.idle_keyframes,
            self.rate_requests,
            self.rate_applied,
            self.rate_busy,
            self.publisher.reconnect_count()
        );
        self.publisher.close();
    }
}

/// Raiz da árvore do aplicativo dono da janela capturada.
fn window_audio_pid(options: &Options) -> Option<u32> {
    let hwnd = match &options.source {
        crate::options::Source::WindowHandle(hwnd) => *hwnd as isize,
        crate::options::Source::Window(title) => {
            match madscreen_encode::platform::resolve_window(title).ok()? {
                madscreen_capture::CaptureTarget::Window(hwnd) => hwnd,
                _ => return None,
            }
        }
        _ => return None,
    };
    let pid = madscreen_audio::window_process_id(hwnd)?;
    Some(madscreen_audio::resolve_root_pid(pid))
}

/// Fontes pedidas nas opções: som do PC (menos a árvore do madscreen) ou aplicativos.
fn audio_specs(options: &Options) -> Vec<AudioSourceSpec> {
    let mut specs = Vec::new();
    if options.audio_system {
        specs.push(AudioSourceSpec::SystemExcluding {
            pid: options.audio_exclude_pid.unwrap_or_else(std::process::id),
        });
        return specs;
    }
    for pid in &options.audio_pids {
        specs.push(AudioSourceSpec::ProcessTree {
            pid: madscreen_audio::resolve_root_pid(*pid),
        });
    }
    if options.audio_follow_window {
        match window_audio_pid(options) {
            Some(pid) => specs.push(AudioSourceSpec::ProcessTree { pid }),
            None => tracing::warn!("janela sem processo identificável: sem áudio do aplicativo"),
        }
    }
    specs.dedup();
    specs
}

/// Inicia a captura/mixagem e liga o mixer ao WebRTC. Falhas viram aviso: o vídeo segue sem áudio.
fn start_audio(
    options: &Options,
    publisher: &dyn Publisher,
    gate: &madscreen_audio::mixer::MuteGate,
) -> Option<AudioCapture> {
    if !options.audio_requested() {
        return None;
    }
    let Some(sink) = publisher.audio_sink() else {
        tracing::warn!("transporte sem faixa de áudio; áudio desativado");
        return None;
    };
    let specs = audio_specs(options);
    match AudioCapture::start(&specs, gate.wrap(sink)) {
        Ok(capture) => {
            reportln!(options.ipc; "Áudio: {} fonte(s) ativa(s), {} falha(s)", capture.active_sources().len(), capture.failed_sources().len());
            Some(capture)
        }
        Err(error) => {
            tracing::warn!(%error, "não foi possível iniciar o áudio");
            None
        }
    }
}
