//! Sessão do sidecar e tradução do protocolo, sem estado por frame.
#![cfg_attr(not(windows), allow(dead_code))]
use crate::options::{Options, Source, Transport};
use anyhow::{Result, ensure};
use clap::Parser;
use madscreen_encode::FrameRate;
use madscreen_ipc::resolution::auto_bitrate_bps;
use madscreen_ipc::{Event, SourceSpec, StartRequest};

/// Canonical configured profile, not a claim about a negotiated payload or SPS.
fn configured_profile_id(profile: madscreen_encode::config::H264Profile, level: u32) -> String {
    use madscreen_encode::config::H264Profile;
    let prefix = match profile {
        H264Profile::High => "6400",
        H264Profile::Main => "4d00",
        H264Profile::ConstrainedBaseline => "42e0",
    };
    format!("{prefix}{level:02x}")
}

#[cfg(test)]
mod configured_profile_tests {
    #[test]
    fn canonical_reference_preserves_the_configured_profile_and_level() {
        use madscreen_encode::config::H264Profile;
        assert_eq!(
            super::configured_profile_id(H264Profile::High, 42),
            "64002a"
        );
        assert_eq!(
            super::configured_profile_id(H264Profile::Main, 31),
            "4d001f"
        );
        assert_eq!(
            super::configured_profile_id(H264Profile::ConstrainedBaseline, 31),
            "42e01f"
        );
    }
}

pub fn options_from_request(request: StartRequest) -> Result<Options> {
    ensure!(
        request.quality.is_supported(),
        "qualidade não suportada: use nativa, 720p, 1080p, 1440p ou 2160p a 30/60 FPS"
    );
    let mut options = Options::try_parse_from(["madscreen-encode-test", "--ipc"])?;
    options.source = match request.source {
        SourceSpec::Picker => Source::Picker,
        SourceSpec::Monitor { index } => Source::Monitor(index),
        SourceSpec::Window { title } => {
            ensure!(!title.trim().is_empty(), "título da janela vazio");
            Source::Window(title)
        }
        SourceSpec::WindowHandle { hwnd } => {
            ensure!(hwnd != 0, "HWND inválido");
            Source::WindowHandle(hwnd)
        }
        SourceSpec::VerifiedWindow { hwnd, instance_id } => {
            ensure!(
                hwnd != 0
                    && instance_id.len() == 64
                    && instance_id.bytes().all(|b| b.is_ascii_hexdigit()),
                "identidade da janela inválida"
            );
            #[cfg(windows)]
            crate::sources::verify_window(hwnd, &instance_id)?;
            options.expected_window_instance = Some(instance_id);
            Source::WindowHandle(hwnd)
        }
    };
    options.size = None;
    options.height_choice = Some(request.quality.height);
    options.fps = FrameRate::new(request.quality.fps, 1)?;
    options.bitrate_auto = request.bitrate_bps.is_none();
    options.bitrate =
        request
            .bitrate_bps
            .unwrap_or(auto_bitrate_bps(1920, 1080, options.fps.as_f64()));
    options.duration = 0;
    options.warmup = 0;
    options.stats_interval = 1;
    options.output = Some("NUL".into());
    // A sessão não herda um token de outra sala do ambiente do processo.
    options.livekit_token = None;
    if let Some(target) = request.livekit {
        options.transport = Transport::Livekit;
        ensure!(!target.url.trim().is_empty(), "URL LiveKit vazia");
        options.livekit_url = Some(target.url);
        options.livekit_token = target.token;
        options.room = target.room;
        options.identity = target.identity;
        if let Some(key) = target.api_key {
            options.livekit_api_key = key;
        }
        if let Some(secret) = target.api_secret {
            options.livekit_api_secret = secret;
        }
    }
    if let Some(target) = request.cloudflare {
        options.transport = Transport::Cloudflare;
        options.livekit_url = None;
        options.cf_url = target.url.filter(|v| !v.trim().is_empty());
        options.cf_token = target.token.filter(|v| !v.trim().is_empty());
        options.cf_channel = target.channel_id.filter(|v| !v.trim().is_empty());
        options.cf_profile_level_id = target.profile_level_id.filter(|v| !v.trim().is_empty());
        options.cf_keepalive = true;
    }
    if options.publishing() {
        if let Some(audio) = request.audio {
            options.audio_system = audio.system;
            options.audio_pids = if audio.system { Vec::new() } else { audio.apps };
            options.audio_follow_window = audio.follow_window;
            options.audio_exclude_pid = audio.exclude_pid;
        }
    }
    // Dimensões provisórias: a fonte real e o NVENC definem a saída no runner.
    options.config(1920, 1080, options.preset.into())?;
    Ok(options)
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    #[test]
    fn livekit_preserves_audio_request_without_cloudflare() -> Result<()> {
        let request: StartRequest = serde_json::from_value(serde_json::json!({
            "source": { "kind": "monitor", "index": 0 },
            "quality": { "height": 1080, "fps": 60 },
            "livekit": { "url": "wss://livekit.example.com", "room": "test", "identity": "host", "token": "test-token" },
            "audio": { "apps": [1234], "follow_window": true, "exclude_pid": 4321 }
        }))?;
        let options = options_from_request(request)?;
        assert_eq!(options.transport, Transport::Livekit);
        assert_eq!(options.livekit_token.as_deref(), Some("test-token"));
        assert_eq!(options.audio_pids, vec![1234]);
        assert!(options.audio_follow_window);
        assert_eq!(options.audio_exclude_pid, Some(4321));
        Ok(())
    }

    #[test]
    fn cloudflare_still_takes_precedence_with_system_audio() -> Result<()> {
        let request: StartRequest = serde_json::from_value(serde_json::json!({
            "source": { "kind": "monitor", "index": 0 },
            "quality": { "height": 1080, "fps": 60 },
            "livekit": { "url": "wss://livekit.example.com", "room": "test", "identity": "host" },
            "cloudflare": { "url": "https://worker.example.com", "token": "worker-token", "channel_id": "channel" },
            "audio": { "system": true, "apps": [1234] }
        }))?;
        let options = options_from_request(request)?;
        assert_eq!(options.transport, Transport::Cloudflare);
        assert!(options.livekit_url.is_none());
        assert!(options.audio_system);
        assert!(options.audio_pids.is_empty());
        assert_eq!(options.cf_channel.as_deref(), Some("channel"));
        Ok(())
    }
}

/// Diferenças de contadores desde a última amostra; inclui segundos sem AUs.
#[derive(Default)]
pub struct StatusWindow {
    seconds: f64,
    frames: u64,
    bytes: u64,
}
impl StatusWindow {
    pub fn sample(&mut self, seconds: f64, frames: u64, bytes: u64, dropped: u64) -> Option<Event> {
        let elapsed = seconds - self.seconds;
        if elapsed < 1. {
            return None;
        }
        let event = Event::Status {
            fps: frames.saturating_sub(self.frames) as f64 / elapsed,
            bitrate_bps: (bytes.saturating_sub(self.bytes) as f64 * 8. / elapsed) as u64,
            frames,
            dropped,
            publishing_connected: None,
            diagnostic_sample: None,
        };
        self.seconds = seconds;
        self.frames = frames;
        self.bytes = bytes;
        Some(event)
    }
}

#[cfg(windows)]
pub use session::*;

#[cfg(windows)]
mod session {
    use super::*;
    use madscreen_ipc::{
        Command, ErrorCode, IpcError, PROTOCOL_VERSION, StopReason, read_line, write_line,
    };
    use std::{
        io::BufReader,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicU8, Ordering},
        },
        thread,
    };

    #[derive(Debug)]
    pub struct FailureStage(pub ErrorCode);
    impl std::fmt::Display for FailureStage {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self.0 {
                ErrorCode::Capture => "falha na captura",
                ErrorCode::Transport => "falha na publicação",
                ErrorCode::Unsupported => "resolução indisponível",
                _ => "falha no encoder",
            })
        }
    }
    impl std::error::Error for FailureStage {}

    pub enum Outcome {
        Stopped,
        PickCancelled,
    }

    pub struct Session {
        pub running: AtomicBool,
        reason: AtomicU8,
        diagnostic_clock: Mutex<Option<(String, std::time::Instant)>>,
        privacy_tx: std::sync::mpsc::SyncSender<madscreen_ipc::privacy::Request>,
        privacy_rx: Mutex<std::sync::mpsc::Receiver<madscreen_ipc::privacy::Request>>,
    }
    impl Session {
        pub fn take_privacy(&self) -> Option<madscreen_ipc::privacy::Request> {
            self.privacy_rx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .try_recv()
                .ok()
        }
        pub fn set_diagnostic_clock(&self, clock: Option<(String, std::time::Instant)>) {
            *self
                .diagnostic_clock
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = clock;
        }
        fn probe(&self, sequence: u32) -> Result<()> {
            let clock = self
                .diagnostic_clock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let (run_id, at_ms) = clock.as_ref().map_or((None, None), |(id, origin)| {
                (
                    Some(id.clone()),
                    Some(origin.elapsed().as_secs_f64() * 1000.),
                )
            });
            drop(clock);
            self.emit(&Event::DiagnosticClock {
                sequence,
                run_id,
                at_ms,
            })
        }
        pub fn started(
            &self,
            options: &Options,
            config: &madscreen_encode::EncoderConfig,
            plan: Option<&madscreen_ipc::OutputPlan>,
            encoder: madscreen_encode::backend::BackendKind,
        ) -> Result<()> {
            let source = match &options.source {
                Source::Synthetic => "synthetic".into(),
                Source::Picker => "picker".into(),
                Source::Monitor(index) => format!("monitor:{index}"),
                Source::Window(title) => format!("window:{title}"),
                Source::WindowHandle(hwnd) => {
                    crate::sources::title_of(*hwnd).unwrap_or_else(|| format!("hwnd:{hwnd}"))
                }
            };
            let mut notice = plan.and_then(|plan| plan.notice.clone());
            if encoder == madscreen_encode::backend::BackendKind::X264 {
                let cpu = "A transmissão está usando o processador. Se o jogo perder desempenho, reduza a resolução ou os FPS.";
                notice = Some(
                    notice.map_or_else(|| cpu.to_string(), |notice| format!("{notice} {cpu}")),
                );
            }
            self.emit(&Event::Started {
                source,
                encoder: Some(encoder.name().into()),
                width: config.width,
                height: config.height,
                source_width: plan.map_or(config.width, |plan| plan.source_width),
                source_height: plan.map_or(config.height, |plan| plan.source_height),
                notice,
                level: config.h264_level.or_else(|| plan.map(|plan| plan.level)),
                profile_level_id: config
                    .h264_level
                    .or_else(|| plan.map(|plan| plan.level))
                    .map(|level| super::configured_profile_id(config.h264_profile, level)),
                fps_target: config.fps.as_f64(),
                bitrate_target_bps: config.bitrate_bps,
                publishing: options.publishing(),
            })
        }
        pub fn status(
            &self,
            window: &mut StatusWindow,
            elapsed: f64,
            measurements: &crate::report::Measurements,
            capture: Option<(
                madscreen_capture::CaptureStats,
                madscreen_capture::CaptureStats,
            )>,
            full_drops: u64,
            publication: (
                Option<bool>,
                Option<madscreen_ipc::diagnostics::PublisherSample>,
            ),
        ) -> Result<()> {
            let dropped = capture.map_or(full_drops, |(stats, baseline)| {
                stats.overwritten.saturating_sub(baseline.overwritten)
                    + stats
                        .contention_drops
                        .saturating_sub(baseline.contention_drops)
            });
            if let Some(mut event) =
                window.sample(elapsed, measurements.frames, measurements.bytes, dropped)
            {
                if let Event::Status {
                    publishing_connected: connected,
                    diagnostic_sample,
                    ..
                } = &mut event
                {
                    *connected = publication.0;
                    *diagnostic_sample = publication.1.map(Box::new);
                }
                self.emit(&event)?;
            }
            Ok(())
        }
        pub fn stop(&self, reason: StopReason) {
            let code = match reason {
                StopReason::Requested => 1,
                StopReason::AppGone => 2,
                StopReason::SourceClosed => 3,
            };
            // A primeira causa vence, inclusive se EOF chegar durante a drenagem.
            let _ = self
                .reason
                .compare_exchange(0, code, Ordering::AcqRel, Ordering::Acquire);
            self.running.store(false, Ordering::Release);
        }
        fn reason(&self) -> StopReason {
            match self.reason.load(Ordering::Acquire) {
                2 => StopReason::AppGone,
                3 => StopReason::SourceClosed,
                _ => StopReason::Requested,
            }
        }
        pub fn emit(&self, event: &Event) -> Result<()> {
            match emit(event) {
                Err(error) if error.downcast_ref::<IpcError>().is_some_and(|error| {
                    matches!(error, IpcError::Io(error) if error.kind() == std::io::ErrorKind::BrokenPipe)
                }) => {
                    self.stop(StopReason::AppGone);
                    Ok(())
                }
                result => result,
            }
        }
    }

    fn emit(event: &Event) -> Result<()> {
        write_line(&mut std::io::stdout().lock(), event)?;
        Ok(())
    }
    fn invalid(error: anyhow::Error) -> anyhow::Error {
        let _ = emit(&Event::Error {
            code: ErrorCode::InvalidRequest,
            message: format!("{error:#}"),
            diagnostic: None,
        });
        error
    }

    pub fn run(
        report: Option<std::path::PathBuf>,
        frames_csv: Option<std::path::PathBuf>,
        capture_backend: crate::options::CaptureBackendArg,
    ) -> Result<()> {
        let (privacy_tx, privacy_rx) = std::sync::mpsc::sync_channel(8);
        let session = Arc::new(Session {
            running: AtomicBool::new(true),
            reason: AtomicU8::new(0),
            diagnostic_clock: Mutex::new(None),
            privacy_tx,
            privacy_rx: Mutex::new(privacy_rx),
        });
        let signal = session.clone();
        ctrlc::set_handler(move || signal.stop(StopReason::Requested))?;
        session.emit(&Event::Ready {
            version: PROTOCOL_VERSION,
            engine: format!("madscreen-encode-test {}", env!("CARGO_PKG_VERSION")),
        })?;
        let mut input = BufReader::new(std::io::stdin());
        let command = read_line::<Command>(&mut input).map_err(|error| invalid(error.into()))?;
        let request = match command {
            Some(Command::Start(request)) => request,
            Some(Command::DiagnosticProbe { .. } | Command::Privacy(_)) => {
                return Err(invalid(anyhow::anyhow!(
                    "Start deve preceder a sonda de diagnóstico"
                )));
            }
            Some(Command::Stop) | None => {
                session.stop(if command.is_some() {
                    StopReason::Requested
                } else {
                    StopReason::AppGone
                });
                return session.emit(&Event::Stopped {
                    reason: session.reason(),
                });
            }
        };
        let mut options = options_from_request(request).map_err(invalid)?;
        // Overrides locais explícitos do processo de ensaio (caminhos/backend).
        // Não herdar credenciais/opções de publicação nem aceitar caminhos pelo IPC.
        options.report = report;
        options.frames_csv = frames_csv;
        options.capture_backend = capture_backend;
        #[cfg(not(feature = "publish"))]
        if options.publishing() {
            session.emit(&Event::Error {
                code: ErrorCode::Unsupported,
                message: "publicação exige compilar com --features publish".into(),
                diagnostic: None,
            })?;
            anyhow::bail!("publicação indisponível neste build");
        }
        let reader = session.clone();
        // Move o mesmo BufReader: Stop pode já estar no buffer após a linha Start.
        thread::Builder::new()
            .name("ipc-stdin".into())
            .spawn(move || {
                loop {
                    match read_line::<Command>(&mut input) {
                        Ok(Some(Command::Privacy(request))) => {
                            if reader.privacy_tx.try_send(request).is_err() {
                                // Nunca perder silenciosamente uma solicitação de proteção.
                                reader.stop(StopReason::Requested);
                                break;
                            }
                        }
                        Ok(Some(Command::DiagnosticProbe { sequence })) => {
                            if reader.probe(sequence).is_err() {
                                break;
                            }
                        }
                        Ok(Some(Command::Stop)) => {
                            reader.stop(StopReason::Requested);
                            break;
                        }
                        Ok(None) | Err(IpcError::Io(_)) => {
                            reader.stop(StopReason::AppGone);
                            break;
                        }
                        Ok(Some(Command::Start(_))) => {
                            let _ = reader.emit(&Event::Error {
                                code: ErrorCode::InvalidRequest,
                                message: "a sessão já recebeu Start".into(),
                                diagnostic: None,
                            });
                        }
                        Err(error) => {
                            let _ = reader.emit(&Event::Error {
                                code: ErrorCode::InvalidRequest,
                                message: error.to_string(),
                                diagnostic: None,
                            });
                        }
                    }
                }
            })?;
        let pipeline = session.clone();
        let result = thread::Builder::new()
            .name("nvenc-encode".into())
            .spawn(move || -> Result<Outcome> {
                use madscreen_encode::platform;
                if !pipeline.running.load(Ordering::Acquire) {
                    return Ok(Outcome::Stopped);
                }
                let _runtime = platform::Runtime::new()?;
                platform::set_encoder_priority()?;
                let report = crate::runner::run_profile(
                    &options,
                    options.preset.into(),
                    &pipeline.running,
                    Some(&pipeline),
                )?;
                if let (Some(path), Some(report)) = (&options.report, &report) {
                    serde_json::to_writer_pretty(std::fs::File::create(path)?, &[report])?;
                }
                Ok(
                    if report.is_none() && pipeline.running.load(Ordering::Acquire) {
                        Outcome::PickCancelled
                    } else {
                        Outcome::Stopped
                    },
                )
            })?
            .join()
            .unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "thread do encoder terminou inesperadamente"
                ))
            });
        match result {
            Ok(Outcome::PickCancelled) => session.emit(&Event::PickCancelled),
            Ok(Outcome::Stopped) => session.emit(&Event::Stopped {
                reason: session.reason(),
            }),
            Err(error) => {
                let diagnostic = error.chain().find_map(|cause| {
                    cause
                        .downcast_ref::<madscreen_ipc::ShareDiagnostic>()
                        .map(|item| item.body.clone())
                });
                session.emit(&Event::Error {
                    code: error
                        .downcast_ref::<FailureStage>()
                        .map_or(ErrorCode::Encode, |stage| stage.0),
                    message: format!("{error:#}"),
                    diagnostic,
                })?;
                Err(error)
            }
        }
    }
}
