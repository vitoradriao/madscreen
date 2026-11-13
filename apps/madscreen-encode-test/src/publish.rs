//! Saída opcional LiveKit/Cloudflare (feature `publish`): publica os access units H.264
//! do NVENC sem re-encode e repassa pedidos de keyframe/taxa do WebRTC ao encoder.
use crate::load_governor::{LoadGovernor, Target, Window};
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
    /// Repetições do último quadro depois de cada IDR com a tela parada.
    refine: madscreen_encode::refine::StaticRefine,
    idle_keyframes: u64,
    rate_requests: u64,
    rate_applied: u64,
    rate_busy: u64,
    wanted_bps: Option<u32>,
    last_request_bps: u64,
    /// O degrau do governador mudou o FPS e o encoder ainda orça a taxa pelo anterior.
    fps_dirty: bool,
    diagnostic: Option<DiagnosticWindow>,
    governor: Option<LoadGovernor>,
    governor_wait_max_ms: f64,
    governor_at_ms: u64,
    /// Bytes de AU desde a última janela do governador (sinal de banda).
    governor_bytes: u64,
    /// Pede sondagem quando a estimativa despenca; `MADSCREEN_PROBE_NUDGE=0` desliga.
    probe_nudge: Option<ProbeNudge>,
    /// Laboratório (MADSCREEN_LAB_PROBE_AT_S): uma sondagem forçada, para exercitar o caminho do
    /// transporte numa sala real sem precisar derrubar a estimativa.
    forced_probe_at: Option<std::time::Instant>,
}

/// Depois de um pico de atraso o GCC sobe só ~50 kbps/s (aumento aditivo) e não sonda sozinho fora
/// de ALR. Numa queda brusca (pedido abaixo de 70% do maior dos últimos 5 s) pede no máximo duas
/// sondagens, em +3 s e +8 s (a imediata é recusada enquanto o atraso sobe). Volta a 70% do nível
/// anterior ou esgota as tentativas: só uma nova queda brusca pede outra. Sondar só por estar
/// abaixo de um pico antigo repetia rajadas num link limitado (lab 13/nov, 10 Mbps: perda 2-3x).
struct ProbeNudge {
    origin: std::time::Instant,
    /// Maior pedido por segundo, nos últimos `NUDGE_WINDOW_S + 1` segundos (sem alocar por quadro).
    buckets: [(u64, u64); NUDGE_WINDOW_S as usize + 1],
    event: Option<NudgeEvent>,
    count: u64,
    /// Eventos seguidos, cada um até `NUDGE_COOLDOWN_MAX_S` depois do anterior: quedas que se repetem
    /// são o dente de serra de um link limitado, não um pico isolado numa rede folgada.
    streak: u32,
    last_event_end: Option<std::time::Instant>,
    cooldown_until: Option<std::time::Instant>,
}

#[derive(Clone, Copy)]
struct NudgeEvent {
    start: std::time::Instant,
    level_bps: u64,
    attempts: usize,
}

const NUDGE_WINDOW_S: u64 = 5;
const NUDGE_WARMUP_S: u64 = 10;
const NUDGE_ATTEMPTS_S: [u64; 2] = [3, 8];
const NUDGE_GIVE_UP_S: u64 = 13;
/// Espera após cada evento, dobrando enquanto eles se repetem. Num link limitado a estimativa oscila
/// em dente de serra e volta sozinha, então "recuperou" não prova que a sondagem ajudou (lab 13/nov:
/// 6-10 sondagens em 2 min a 10 Mbps com a regra anterior).
const NUDGE_COOLDOWN_S: u64 = 60;
const NUDGE_COOLDOWN_MAX_S: u64 = 300;

impl ProbeNudge {
    fn new(now: std::time::Instant) -> Self {
        Self {
            origin: now,
            buckets: [(u64::MAX, 0); NUDGE_WINDOW_S as usize + 1],
            event: None,
            count: 0,
            streak: 0,
            last_event_end: None,
            cooldown_until: None,
        }
    }

    fn due(&mut self, request_bps: u64, now: std::time::Instant) -> bool {
        use std::time::Duration;
        if request_bps == 0 {
            return false;
        }
        let second = now.saturating_duration_since(self.origin).as_secs();
        let recent = self
            .buckets
            .iter()
            .filter(|(s, _)| *s != u64::MAX && second.saturating_sub(*s) <= NUDGE_WINDOW_S)
            .map(|(_, bps)| *bps)
            .max()
            .unwrap_or(0);
        let slot = &mut self.buckets[(second % (NUDGE_WINDOW_S + 1)) as usize];
        if slot.0 != second {
            *slot = (second, 0);
        }
        slot.1 = slot.1.max(request_bps);
        let Some(mut event) = self.event else {
            let cooling = self.cooldown_until.is_some_and(|until| now < until);
            if !cooling && second >= NUDGE_WARMUP_S && request_bps * 10 < recent * 7 {
                self.event = Some(NudgeEvent {
                    start: now,
                    level_bps: recent,
                    attempts: 0,
                });
            }
            return false;
        };
        let elapsed = now.saturating_duration_since(event.start);
        let recovered = request_bps * 10 >= event.level_bps * 7;
        if recovered || elapsed >= Duration::from_secs(NUDGE_GIVE_UP_S) {
            let repeated = self.last_event_end.is_some_and(|end| {
                event.start.saturating_duration_since(end)
                    < Duration::from_secs(NUDGE_COOLDOWN_MAX_S)
            });
            self.streak = if repeated {
                self.streak.saturating_add(1)
            } else {
                0
            };
            let wait = NUDGE_COOLDOWN_S
                .saturating_mul(1 << self.streak.min(8))
                .min(NUDGE_COOLDOWN_MAX_S);
            self.cooldown_until = Some(now + Duration::from_secs(wait));
            self.last_event_end = Some(now);
            self.event = None;
            return false;
        }
        let due = NUDGE_ATTEMPTS_S
            .get(event.attempts)
            .is_some_and(|at| elapsed >= Duration::from_secs(*at));
        if due {
            event.attempts += 1;
            self.count += 1;
        }
        self.event = Some(event);
        due
    }
}

struct DiagnosticWindow {
    origin: std::time::Instant,
    run_id: String,
    frames: u64,
    bytes: u64,
    keyframes: u64,
    /// Soma do QP médio por quadro e quadros com QP, acumulados (só backends que informam).
    qp_sum: u64,
    qp_frames: u64,
    last_au_bytes: usize,
    max_au_bytes: usize,
    capture_to_au_max_ms: f64,
    capture_to_convert_max_ms: Option<f64>,
    convert_submit_max_ms: Option<f64>,
    queue_wait_max_ms: Option<f64>,
    encode_wait_max_ms: f64,
    reconfigure_max_ms: f64,
    readback_map_max_ms: Option<f64>,
    readback_copy_max_ms: Option<f64>,
    codec_send_max_ms: Option<f64>,
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
            qp_sum: 0,
            qp_frames: 0,
            last_au_bytes: 0,
            max_au_bytes: 0,
            capture_to_au_max_ms: 0.,
            capture_to_convert_max_ms: None,
            convert_submit_max_ms: None,
            queue_wait_max_ms: None,
            encode_wait_max_ms: 0.,
            reconfigure_max_ms: 0.,
            readback_map_max_ms: None,
            readback_copy_max_ms: None,
            codec_send_max_ms: None,
        }
    }
}

/// Intervalo mínimo entre IDRs forçados: o WebRTC pede keyframe a cada reconfiguração de taxa,
/// e uma rajada de IDRs de 1080p degrada a estimativa de banda e o bitrate efetivo.
const MIN_KEYFRAME_INTERVAL_S: f64 = 0.5;
const MIN_BITRATE_BPS: u64 = 1_000_000;

impl Live {
    /// O refino de tela parada ainda tem repetições a fazer.
    pub fn refining(&self) -> bool {
        self.refine.active()
    }

    pub fn is_connected(&self) -> Option<bool> {
        self.publisher.is_connected()
    }
    pub fn is_terminated(&self) -> bool {
        self.publisher.is_terminated()
    }

    pub fn connect(
        options: &Options,
        config: &EncoderConfig,
        governor_enabled: bool,
        governor_cpu_signals: bool,
    ) -> Result<Self> {
        // Laboratório #14: AV1 só pelo transporte do Worker/laboratório local, com opt-in explícito.
        let lab_av1 = config.codec == madscreen_encode::VideoCodec::Av1
            && std::env::var("MADSCREEN_LAB_AV1_PUBLISH").is_ok_and(|value| value == "1")
            && matches!(options.transport, Transport::Cloudflare);
        anyhow::ensure!(
            config.codec == madscreen_encode::VideoCodec::H264 || lab_av1,
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
                    direct_session_id: options.cf_direct_session.clone(),
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
                    h264_level_idc: if options.cf_profile_level_id.is_none() && !lab_av1 {
                        config.h264_level
                    } else {
                        None
                    },
                    only_h264: !lab_av1,
                    keepalive: options.cf_keepalive,
                    start_bitrate_bps: options.cf_start_bitrate.map(|mbps| (mbps * 1e6) as u64),
                    audio: options.audio_requested(),
                    lab_av1,
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
            refine: madscreen_encode::refine::StaticRefine::new(),
            idle_keyframes: 0,
            rate_requests: 0,
            rate_applied: 0,
            rate_busy: 0,
            wanted_bps: None,
            last_request_bps: 0,
            fps_dirty: false,
            diagnostic: options.diagnostics.then(DiagnosticWindow::new),
            governor: governor_enabled.then(|| {
                let governor =
                    LoadGovernor::new(config.fps.as_f64().ceil() as u32, config.height > 720, 0);
                if governor_cpu_signals {
                    governor
                } else {
                    governor.rate_only()
                }
            }),
            governor_wait_max_ms: 0.,
            governor_at_ms: 0,
            governor_bytes: 0,
            probe_nudge: std::env::var_os("MADSCREEN_PROBE_NUDGE")
                .is_none_or(|v| v != "0")
                .then(|| ProbeNudge::new(std::time::Instant::now())),
            forced_probe_at: std::env::var("MADSCREEN_LAB_PROBE_AT_S")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map(|s| std::time::Instant::now() + std::time::Duration::from_secs(s)),
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
        // Repetições de refino não contam como conteúdo: não rearmam o IDR de repouso.
        self.force_keyframe(
            encoder,
            now,
            self.refine.content_since_keyframe() && idle_s >= 1.,
        );
        let redraw_after_s = (2. * self.fps.den as f64 / self.fps.num as f64).max(0.05);
        if self.keyframe_armed && !self.redraw_requested && idle_s >= redraw_after_s {
            self.redraw_requested = true;
            return Ok(true);
        }
        // Depois do IDR com a tela parada, repete o último quadro a cada período do FPS pedido
        // (no máximo 60 Hz) para o encoder refinar a imagem como P.
        let period_ms = (1000. * self.fps.den as f64 / self.fps.num as f64).max(1000. / 60.);
        let now_ms = (now.max(0) as f64 * 1000. / self.frequency as f64) as u64;
        Ok(self.refine.wants(
            now_ms,
            (idle_s * 1000.) as u64,
            period_ms as u64,
            self.keyframe_armed || self.keyframe_pending,
        ))
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
            if let Some(qp) = au.qp {
                window.qp_sum += u64::from(qp);
                window.qp_frames += 1;
            }
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
                // Só o caminho FFmpeg preenche estes tempos (readback NV12 explícito).
                (&mut window.readback_map_max_ms, au.timing.readback_map_ms),
                (&mut window.readback_copy_max_ms, au.timing.readback_copy_ms),
                (&mut window.codec_send_max_ms, au.timing.codec_send_ms),
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
        if self.governor.is_some() {
            // Async staging includes a deliberate frame of queueing in QPC elapsed;
            // codec_send is the relevant CPU wait when that path is active.
            let wait_ms = au.timing.codec_send_ms.unwrap_or_else(|| {
                (au.timing.qpc_encode_end - au.timing.qpc_encode_start).max(0) as f64 * 1000.
                    / self.frequency as f64
            });
            self.governor_wait_max_ms = self.governor_wait_max_ms.max(wait_ms);
            self.governor_bytes = self.governor_bytes.saturating_add(au.data.len() as u64);
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
        if accepted && self.refine.on_access_unit(au.kind == FrameKind::Key) {
            // Antes da primeira mudança de taxa last_bps é o teto; o pedido do WebRTC é a taxa real.
            let rate = match self.last_request_bps {
                0 => u64::from(self.last_bps),
                request => request.min(u64::from(self.last_bps)),
            };
            let budget = rate / 8 * u64::from(self.fps.den) / u64::from(self.fps.num.max(1));
            self.refine
                .keep_going_if_spending(au.data.len(), budget as usize);
        }
        self.force_keyframe(encoder, now, false);
        if let Some(request) = self.publisher.take_rate_request() {
            self.rate_requests += 1;
            self.last_request_bps = request.bitrate_bps;
            let target = request
                .bitrate_bps
                .clamp(u64::from(self.floor_bps), u64::from(self.cap_bps))
                as u32;
            if self.rate_ramp.is_none() {
                self.wanted_bps = Some(target);
            }
        }
        if self
            .forced_probe_at
            .is_some_and(|at| std::time::Instant::now() >= at)
        {
            self.forced_probe_at = None;
            self.publisher.request_probe();
            tracing::info!(
                pedido_bps = self.last_request_bps,
                "sondagem de banda forçada (laboratório)"
            );
        }
        if let Some(nudge) = self.probe_nudge.as_mut()
            && nudge.due(self.last_request_bps, std::time::Instant::now())
        {
            self.publisher.request_probe();
            tracing::info!(
                pedido_bps = self.last_request_bps,
                nivel_anterior_bps = nudge.event.map_or(0, |e| e.level_bps),
                sondagens = nudge.count,
                "sondagem de banda pedida apos queda da estimativa"
            );
        }
        let rate_now_ms = self.rate_ramp.as_ref().map_or(0, |_| {
            u64::try_from(self.rate_origin.elapsed().as_millis()).unwrap_or(u64::MAX)
        });
        if let Some(ramp) = self.rate_ramp.as_mut() {
            // Todo quadro: o WebRTC só avisa mudanças, e o piso sobe em degraus pelo relógio.
            if self.last_request_bps > 0 {
                ramp.request_at(self.last_request_bps, rate_now_ms);
            }
            use madscreen_encode::{backend::BackendKind, rate::RatePacing};
            let pacing = if encoder.rate_change_reopens() {
                RatePacing::REOPEN
            } else if encoder.kind() == BackendKind::Qsv {
                // MFXVideoENCODE_Reset drena a fila e pode iniciar nova sequência [VERIFICAR].
                RatePacing {
                    minimum_ms: 1000,
                    ..RatePacing::IN_SESSION
                }
            } else {
                RatePacing::IN_SESSION
            };
            self.wanted_bps = ramp.target_paced(rate_now_ms, pacing);
        }
        // O pedido fica guardado até o encoder aceitar (reconfigurar exige o pipeline vazio).
        if let Some(target) = self.wanted_bps.or(self.fps_dirty.then_some(self.last_bps)) {
            let delta = target.abs_diff(self.last_bps);
            // RateRamp already applies its deadband, including exact cap/floor
            // convergence. A second legacy filter would discard its final step.
            if self.rate_ramp.is_none()
                && !self.fps_dirty
                && u64::from(delta) * 50 < u64::from(self.last_bps)
            {
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
                        self.fps_dirty = false;
                    }
                    Err(madscreen_encode::EncodeError::Busy) => self.rate_busy += 1,
                    Err(error) => {
                        tracing::warn!(%error, "falha ao reconfigurar bitrate");
                        self.wanted_bps = None;
                        self.fps_dirty = false;
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

    pub fn governor_tick(
        &mut self,
        at_ms: u64,
        captured: u64,
        submitted: u64,
        process_cpu_cores: Option<f64>,
    ) -> Option<Target> {
        let governor = self.governor.as_mut()?;
        let wait_ms = std::mem::take(&mut self.governor_wait_max_ms);
        let bytes = std::mem::take(&mut self.governor_bytes);
        let elapsed_ms = at_ms.saturating_sub(self.governor_at_ms);
        // A primeira janela não tem início conhecido; o governador valida a duração das demais.
        let produced_bps = (self.governor_at_ms > 0 && elapsed_ms > 0)
            .then(|| bytes.saturating_mul(8_000) / elapsed_ms);
        self.governor_at_ms = at_ms;
        let changed = governor.observe(Window {
            at_ms,
            captured,
            submitted,
            encode_wait_max_ms: Some(wait_ms),
            process_cpu_cores,
            system_cpu_percent: None,
            requested_bps: (self.last_request_bps > 0).then_some(self.last_request_bps),
            produced_bps,
        });
        // Sem isto o encoder segue orçando taxa/60 por quadro e produz metade do alvo a 30 FPS.
        if let Some(target) = changed
            && let Ok(fps) = FrameRate::new(target.fps, 1)
        {
            self.fps = fps;
            self.fps_dirty = true;
        }
        changed
    }
    pub fn disable_governor(&mut self) {
        self.governor = None;
    }
    /// Última taxa aceita pelo encoder; um encoder recriado deve começar dela, não do teto.
    pub fn applied_bitrate_bps(&self) -> u32 {
        self.last_bps
    }

    pub fn diagnostic_sample(&mut self) -> Option<madscreen_ipc::diagnostics::PublisherSample> {
        let window = self.diagnostic.as_mut()?;
        let (pushed, rejected) = self.publisher.counts();
        let (audio_pushed, audio_rejected) = self.publisher.audio_counts();
        let governor_diag = self
            .governor
            .as_ref()
            .map(|governor| governor.diagnostics(self.governor_at_ms));
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
            readback_map_max_ms: window.readback_map_max_ms,
            readback_copy_max_ms: window.readback_copy_max_ms,
            codec_send_max_ms: window.codec_send_max_ms,
            static_refine_frames: Some(self.refine.frames),
            idle_keyframes: Some(self.idle_keyframes),
            requested_keyframes: Some(self.keyframes_forced),
            encoder_qp_sum: (window.qp_frames > 0).then_some(window.qp_sum),
            encoder_qp_frames: (window.qp_frames > 0).then_some(window.qp_frames),
            governor_fps: governor_diag.as_ref().map(|g| g.target.fps),
            governor_resolution: governor_diag.as_ref().map(|g| match g.target.resolution {
                crate::load_governor::ResolutionStep::Native => "native".into(),
                crate::load_governor::ResolutionStep::Hd720 => "720p".into(),
                crate::load_governor::ResolutionStep::Hd540 => "540p".into(),
            }),
            governor_changes: governor_diag.as_ref().map(|g| g.change_count),
            governor_time_at_step_ms: governor_diag.as_ref().map(|g| g.time_at_step_ms.clone()),
            governor_last_reason: governor_diag.as_ref().and_then(|g| g.last_reason).map(
                |reason| match reason {
                    crate::load_governor::ChangeReason::SustainedOverload => {
                        "sustained_overload".into()
                    }
                    crate::load_governor::ChangeReason::SustainedHealth => {
                        "sustained_health".into()
                    }
                    crate::load_governor::ChangeReason::BandwidthOverload => {
                        "bandwidth_overload".into()
                    }
                    crate::load_governor::ChangeReason::BandwidthRecovery => {
                        "bandwidth_recovery".into()
                    }
                },
            ),
            ..Default::default()
        };
        window.readback_map_max_ms = None;
        window.readback_copy_max_ms = None;
        window.codec_send_max_ms = None;
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

#[cfg(test)]
mod probe_nudge_tests {
    use super::ProbeNudge;
    use std::time::{Duration, Instant};

    /// Alimenta pedidos (segundo, bps) a 10 Hz e devolve os segundos em que houve sondagem.
    fn run(steps: &[(u64, u64)], until_s: u64) -> Vec<u64> {
        let t0 = Instant::now();
        let mut nudge = ProbeNudge::new(t0);
        let mut probes = Vec::new();
        for tenth in 0..until_s * 10 {
            let now = t0 + Duration::from_millis(tenth * 100);
            let second = tenth / 10;
            let bps = steps
                .iter()
                .rev()
                .find(|(at, _)| *at <= second)
                .map_or(0, |(_, b)| *b);
            if nudge.due(bps, now) {
                probes.push(second);
            }
        }
        probes
    }

    #[test]
    fn sharp_drop_asks_twice_then_waits_for_the_next_drop() {
        // Pico de atraso aos 30 s: 17,6 -> 9,6 Mbps e subida lenta; sondagens em +3 s e +8 s.
        let probes = run(&[(0, 17_600_000), (30, 9_600_000), (45, 10_500_000)], 90);
        assert_eq!(probes, vec![33, 38]);
    }

    #[test]
    fn recovery_after_the_first_probe_stops_the_second() {
        let probes = run(&[(0, 17_600_000), (30, 9_600_000), (34, 17_000_000)], 60);
        assert_eq!(probes, vec![33]);
    }

    #[test]
    fn a_limited_link_settling_low_never_probes() {
        // Enlace de 10 Mbps: a estimativa passa do link e assenta aos poucos, sem queda brusca.
        let steps: Vec<(u64, u64)> = (0..60)
            .map(|s| (s, 15_000_000u64.saturating_sub(s * 150_000).max(9_000_000)))
            .collect();
        assert!(run(&steps, 90).is_empty());
    }

    #[test]
    fn a_link_that_stays_low_backs_off_between_drops() {
        // Dente de serra num link limitado: a estimativa cai a cada 20 s e volta sozinha. Duas
        // sondagens no primeiro evento; os seguintes esperam 60 s e depois 120 s.
        let mut steps = Vec::new();
        for cycle in 0..15u64 {
            steps.push((15 + cycle * 20, 10_000_000));
            steps.push((25 + cycle * 20, 6_000_000));
        }
        let probes = run(&steps, 300);
        assert_eq!(probes, vec![28, 33, 108, 113, 248, 253]);
    }

    #[test]
    fn startup_ramp_is_ignored() {
        assert!(run(&[(0, 18_000_000), (5, 4_000_000), (12, 4_000_000)], 30).is_empty());
    }
}
