//! Publicação de access units H.264 já codificados (NVENC) numa sala LiveKit, sem re-encode.
//! Usa `NativeVideoSource::new_encoded` + `VideoEncoderBackend::PreEncoded` (Rust SDK).

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use livekit::{
    Room, RoomEvent, RoomOptions,
    options::{AudioEncoding, TrackPublishOptions, VideoCodec, VideoEncoderBackend, VideoEncoding},
    prelude::{LocalAudioTrack, LocalTrack, LocalVideoTrack, TrackSource},
    webrtc::{
        audio_frame::AudioFrame,
        audio_source::{AudioSourceOptions, RtcAudioSource, native::NativeAudioSource},
        video_frame::{EncodedFrameType, EncodedVideoCodec, EncodedVideoFrame},
        video_source::{RtcVideoSource, VideoResolution, native::NativeVideoSource},
    },
};
use livekit_api::access_token::{AccessToken, VideoGrants};

/// Destino da publicação. `token` tem prioridade; sem ele, gera-se um token com `api_key`/`api_secret`.
#[derive(Clone)]
pub struct PublishConfig {
    pub url: String,
    pub token: Option<String>,
    pub api_key: String,
    pub api_secret: String,
    pub room: String,
    pub identity: String,
    pub track_name: String,
    pub width: u32,
    pub height: u32,
    pub max_bitrate_bps: u64,
    pub max_fps: f64,
    pub audio: bool,
    pub preferred_h264_profile: Option<String>,
    /// `level_idc` real do encoder. Com perfil nativo explícito é escrito no SDP
    /// (perfil e constraints preservados); sem ele usa o nível calculado da saída.
    pub h264_level_idc: Option<u32>,
}

fn validate_profile(preferred: Option<&str>) -> Result<()> {
    anyhow::ensure!(
        preferred.is_none_or(|profile| profile.eq_ignore_ascii_case("42e0")),
        "perfil H.264 experimental nao qualificado na Magalu; use constrained-baseline no laboratorio"
    );
    Ok(())
}

/// Limite de taxa Baseline por nível (kbit/s x1000), conservador.
fn cb_max_bitrate(level: u32) -> Option<u64> {
    match level {
        31 => Some(14_000_000),
        32 | 40 => Some(20_000_000),
        // Conservative cap for the supported levels above 4.0. Unknown levels
        // must not turn an arbitrary integer into an apparently valid contract.
        41 | 42 | 50 | 51 | 52 => Some(50_000_000),
        _ => None,
    }
}

/// Nível que o SDP vai anunciar para o perfil explícito. Recusa saída que o nível não cobre.
fn validate_cb_level(
    width: u32,
    height: u32,
    fps: f64,
    bitrate: u64,
    level: Option<u32>,
) -> Result<u32> {
    let required = madscreen_ipc::resolution::h264_level(width, height, fps);
    let announced = level.or(required);
    anyhow::ensure!(
        required.is_some_and(|required| announced.is_some_and(|level| level >= required))
            && announced
                .and_then(cb_max_bitrate)
                .is_some_and(|maximum| (1..=maximum).contains(&bitrate)),
        "perfil de laboratorio: o nivel H.264 anunciado nao cobre a saida ou excede a taxa do nivel"
    );
    Ok(announced.unwrap_or_default())
}

#[cfg(test)]
mod profile_tests {
    #[test]
    #[ignore = "codec44: native capability inventory, no network or media"]
    fn codec44_native_capability_inventory() {
        use livekit::webrtc::{MediaType, peer_connection_factory::PeerConnectionFactory};
        let factory = PeerConnectionFactory::default();
        for (direction, caps) in [
            (
                "sender",
                factory.get_rtp_sender_capabilities(MediaType::Video),
            ),
            (
                "receiver",
                factory.get_rtp_receiver_capabilities(MediaType::Video),
            ),
        ] {
            // Every codec name, so a leak of non-H264 formats (H265/AV1) shows up.
            for codec in caps.codecs.iter() {
                eprintln!(
                    "CODEC44 {direction} {} {}",
                    codec.mime_type,
                    codec.sdp_fmtp_line.as_deref().unwrap_or("none")
                );
            }
        }
    }
    #[test]
    fn explicit_profiles_require_the_validated_receiver_contract() {
        assert!(super::validate_profile(None).is_ok()); // Legacy unchanged.
        assert!(super::validate_profile(Some("42e0")).is_ok());
        assert!(super::validate_profile(Some("42E0")).is_ok());
        for rejected in ["4d00", "4D00", "6400", "42e01f", ""] {
            assert!(super::validate_profile(Some(rejected)).is_err());
        }
    }

    #[test]
    fn explicit_profile_announces_a_level_that_covers_the_output() {
        assert!(matches!(
            super::validate_cb_level(1280, 720, 30., 4_000_000, None),
            Ok(31)
        ));
        assert!(matches!(
            super::validate_cb_level(1278, 720, 30., 3_100_000, Some(31)),
            Ok(31)
        ));
        assert!(matches!(
            super::validate_cb_level(1920, 1080, 60., 14_000_000, Some(42)),
            Ok(42)
        ));
        assert!(matches!(
            super::validate_cb_level(1920, 1080, 30., 18_000_000, None),
            Ok(40)
        ));
        for (width, height, fps, bitrate, level) in [
            (1920, 1080, 30., 4_000_000, Some(31)), // nivel abaixo do exigido
            (1920, 1080, 60., 4_000_000, Some(40)),
            (1280, 720, 30., 18_000_000, None), // acima da taxa do nivel 3.1
            (1280, 720, f64::NAN, 4_000_000, None),
            (0, 720, 30., 4_000_000, None),
        ] {
            assert!(super::validate_cb_level(width, height, fps, bitrate, level).is_err());
        }
    }

    #[test]
    fn explicit_profile_rejects_reserved_levels_before_connecting() {
        for level in [0, 30, 33, 39, 43, 49, 53, 255, 256, u32::MAX] {
            assert!(super::validate_cb_level(1280, 720, 30., 4_000_000, Some(level)).is_err());
        }
        for (level, maximum) in [
            (31, 14_000_000),
            (32, 20_000_000),
            (40, 20_000_000),
            (41, 50_000_000),
            (42, 50_000_000),
            (50, 50_000_000),
            (51, 50_000_000),
            (52, 50_000_000),
        ] {
            assert!(super::validate_cb_level(1280, 720, 30., maximum, Some(level)).is_ok());
            assert!(super::validate_cb_level(1280, 720, 30., maximum + 1, Some(level)).is_err());
        }
    }
}

impl std::fmt::Debug for PublishConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublishConfig")
            .field("url", &self.url)
            .field("room", &self.room)
            .field("identity", &self.identity)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("audio", &self.audio)
            .field("credentials", &"<ocultas>")
            .finish()
    }
}

#[cfg(test)]
mod audio_negotiation_tests {
    use livekit::{
        options::{TrackPublishOptions, audio_publish_features},
        prelude::AudioTrackFeature,
    };

    #[test]
    fn stereo_music_advertises_stereo_and_no_dtx_to_sfu() {
        let options = TrackPublishOptions {
            dtx: false,
            ..Default::default()
        };
        assert_eq!(
            audio_publish_features(2, &options),
            vec![AudioTrackFeature::TfStereo, AudioTrackFeature::TfNoDtx]
        );
    }

    #[test]
    fn mono_and_preconnect_keep_their_negotiated_features() {
        let mut options = TrackPublishOptions::default();
        assert!(audio_publish_features(1, &options).is_empty());
        options.preconnect_buffer = true;
        assert_eq!(
            audio_publish_features(2, &options),
            vec![
                AudioTrackFeature::TfStereo,
                AudioTrackFeature::TfPreconnectBuffer
            ]
        );
    }
}

/// Pedidos que o WebRTC faz ao encoder (PLI/FIR, estimativa de banda).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RateRequest {
    pub bitrate_bps: u64,
    pub fps: f64,
}

pub fn make_token(
    api_key: &str,
    api_secret: &str,
    room: &str,
    identity: &str,
    publisher: bool,
) -> Result<String> {
    let grants = VideoGrants {
        room_join: true,
        room: room.to_owned(),
        can_publish: Some(publisher),
        can_subscribe: Some(!publisher),
        can_publish_data: Some(false),
        ..Default::default()
    };
    AccessToken::with_api_key(api_key, api_secret)
        .with_identity(identity)
        .with_name(identity)
        .with_ttl(Duration::from_secs(6 * 3600))
        .with_grants(grants)
        .to_jwt()
        .context("gerando token LiveKit")
}

/// Relógio de parede em microssegundos desde a época Unix (mesma base do viewer).
pub fn epoch_us() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_micros() as i64)
}

pub struct LiveKitPublisher {
    runtime: tokio::runtime::Runtime,
    room: Arc<Room>,
    source: NativeVideoSource,
    resolution: VideoResolution,
    pushed: u64,
    rejected: u64,
    sender_history: Arc<madscreen_rtc_stats::SenderHistory>,
    stats_generation: Arc<AtomicU64>,
    audio: Option<AudioPusher>,
    max_bitrate_bps: u64,
}

const AUDIO_SAMPLE_RATE: u32 = 48_000;
const AUDIO_CHANNELS: u32 = 2;
const AUDIO_FRAMES_PER_BLOCK: u32 = 480;

/// Handle usado exclusivamente pela thread do mixer; blocos de 10 ms, sem fila Rust.
#[derive(Clone)]
pub struct AudioPusher {
    source: NativeAudioSource,
    handle: tokio::runtime::Handle,
    pushed: Arc<AtomicU64>,
    rejected: Arc<AtomicU64>,
}

impl AudioPusher {
    pub fn push(&self, block: &[i16]) -> bool {
        let accepted = block.len() == (AUDIO_FRAMES_PER_BLOCK * AUDIO_CHANNELS) as usize
            && self
                .handle
                .block_on(self.source.capture_frame(&AudioFrame {
                    data: std::borrow::Cow::Borrowed(block),
                    sample_rate: AUDIO_SAMPLE_RATE,
                    num_channels: AUDIO_CHANNELS,
                    samples_per_channel: AUDIO_FRAMES_PER_BLOCK,
                }))
                .is_ok();
        let counter = if accepted {
            &self.pushed
        } else {
            &self.rejected
        };
        counter.fetch_add(1, Ordering::Relaxed);
        accepted
    }
}

impl LiveKitPublisher {
    /// Conecta, cria a fonte pré-codificada (H.264) e publica a faixa de vídeo. Bloqueante.
    pub fn connect(config: &PublishConfig) -> Result<Self> {
        // High capability rehearsal stays explicitly scoped to the exclusive local SFU.
        // The product default and Magalu contract remain unchanged until end-to-end approval.
        let local_high = config.url == "ws://127.0.0.1:17880"
            && config.preferred_h264_profile.as_deref() == Some("6400");
        if !local_high {
            validate_profile(config.preferred_h264_profile.as_deref())?;
        }
        let mut level_idc = None;
        if config.preferred_h264_profile.is_some() {
            level_idc = Some(validate_cb_level(
                config.width,
                config.height,
                config.max_fps,
                config.max_bitrate_bps,
                config.h264_level_idc,
            )?);
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("madscreen-livekit")
            .enable_all()
            .build()
            .context("criando runtime tokio")?;
        let token = match &config.token {
            Some(token) => token.clone(),
            None => make_token(
                &config.api_key,
                &config.api_secret,
                &config.room,
                &config.identity,
                true,
            )?,
        };
        let resolution = VideoResolution {
            width: config.width,
            height: config.height,
        };
        // Opt-in: a fonte como "tela" (sondagem de banda em tela parada) ainda não foi medida.
        let source = if std::env::var_os("MADSCREEN_SCREENCAST_SOURCE").is_some() {
            NativeVideoSource::new_encoded_screencast(resolution.clone())
        } else {
            NativeVideoSource::new_encoded(resolution.clone())
        };
        let (room, mut events) = runtime
            .block_on(async {
                tokio::time::timeout(
                    Duration::from_secs(20),
                    Room::connect(&config.url, &token, RoomOptions::default()),
                )
                .await
            })
            .context("tempo limite conectando ao LiveKit")?
            .with_context(|| format!("conectando em {}", config.url))?;
        let stats_generation = Arc::new(AtomicU64::new(0));
        let event_generation = stats_generation.clone();
        runtime.spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    RoomEvent::ParticipantConnected(p) => {
                        tracing::info!(identity = %p.identity(), "viewer entrou");
                    }
                    RoomEvent::ParticipantDisconnected(p) => {
                        tracing::info!(identity = %p.identity(), "viewer saiu");
                    }
                    RoomEvent::Disconnected { reason } => {
                        event_generation.fetch_add(1, Ordering::AcqRel);
                        tracing::warn!(?reason, "desconectado da sala");
                    }
                    RoomEvent::Reconnecting => {
                        event_generation.fetch_add(1, Ordering::AcqRel);
                        tracing::warn!("reconectando à sala");
                    }
                    RoomEvent::Reconnected => tracing::info!("reconectado à sala"),
                    _ => {}
                }
            }
        });
        let track = LocalVideoTrack::create_video_track(
            &config.track_name,
            RtcVideoSource::Native(source.clone()),
        );
        let options = TrackPublishOptions {
            video_codec: VideoCodec::H264,
            h264_profile_prefix: config.preferred_h264_profile.clone(),
            h264_level_idc: level_idc,
            video_encoder: VideoEncoderBackend::PreEncoded,
            video_encoding: Some(VideoEncoding {
                max_bitrate: config.max_bitrate_bps,
                max_framerate: config.max_fps,
            }),
            simulcast: false,
            source: TrackSource::Screenshare,
            stream: "madscreen".into(),
            ..Default::default()
        };
        runtime
            .block_on(
                room.local_participant()
                    .publish_track(LocalTrack::Video(track), options),
            )
            .context("publicando faixa de vídeo")?;
        let audio = if config.audio {
            let source = NativeAudioSource::new(
                AudioSourceOptions {
                    echo_cancellation: false,
                    noise_suppression: false,
                    auto_gain_control: false,
                },
                AUDIO_SAMPLE_RATE,
                AUDIO_CHANNELS,
                0,
            );
            let track = LocalAudioTrack::create_audio_track(
                "audio",
                RtcAudioSource::Native(source.clone()),
            );
            let options = TrackPublishOptions {
                source: TrackSource::ScreenshareAudio,
                stream: "madscreen".into(),
                audio_encoding: Some(AudioEncoding {
                    max_bitrate: 128_000,
                }),
                dtx: false,
                ..Default::default()
            };
            if let Err(error) = runtime.block_on(
                room.local_participant()
                    .publish_track(LocalTrack::Audio(track), options),
            ) {
                let _ = runtime.block_on(room.close());
                return Err(error).context("publicando faixa de áudio");
            }
            Some(AudioPusher {
                source,
                handle: runtime.handle().clone(),
                pushed: Arc::new(AtomicU64::new(0)),
                rejected: Arc::new(AtomicU64::new(0)),
            })
        } else {
            None
        };
        tracing::info!(
            room = %config.room,
            width = config.width,
            height = config.height,
            "faixa H.264 pré-codificada publicada"
        );
        Ok(Self {
            runtime,
            room: Arc::new(room),
            source,
            resolution,
            pushed: 0,
            rejected: 0,
            sender_history: Arc::new(madscreen_rtc_stats::SenderHistory::default()),
            stats_generation,
            audio,
            max_bitrate_bps: config.max_bitrate_bps,
        })
    }

    /// Pede ao GCC uma sondagem de banda até 2x o teto do encoder: a sondagem vai até 2x a alocação
    /// (probe_controller.cc), e com o teto do transporte igual ao do encoder a rajada media menos
    /// que o link (lab 13/nov: 10,6-13,8 Mbps num enlace de 30). O encoder segue na própria alocação.
    pub fn request_probe(&self) {
        if let Err(error) = self
            .room
            .request_publisher_bandwidth_probe(self.max_bitrate_bps.saturating_mul(2))
        {
            tracing::debug!(%error, "pedido de sondagem de banda recusado");
        }
    }

    /// Entrega um access unit Annex B completo. `timestamp_us` é o instante de captura (época Unix).
    pub fn push(&mut self, access_unit: &[u8], keyframe: bool, timestamp_us: i64) -> bool {
        let frame = EncodedVideoFrame {
            codec: EncodedVideoCodec::H264,
            payload: access_unit,
            timestamp_us,
            frame_type: if keyframe {
                EncodedFrameType::Key
            } else {
                EncodedFrameType::Delta
            },
            resolution: self.resolution.clone(),
            frame_metadata: None,
        };
        let accepted = self.source.capture_encoded_frame(&frame);
        if accepted {
            self.pushed += 1;
        } else {
            self.rejected += 1;
        }
        accepted
    }

    /// Registra a cada `interval` uma linha com o que o libwebrtc fez com os access units
    /// entregues: quantos chegaram ao encoder, keyframes, PLI/FIR/NACK, alvo de banda e estimativa.
    pub fn spawn_stats_logger(&self, interval: Duration) {
        if interval.is_zero() {
            return;
        }
        let room = self.room.clone();
        let history = self.sender_history.clone();
        let generation = self.stats_generation.clone();
        self.runtime.spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let queried_generation = generation.load(Ordering::Acquire);
                if let Ok(session) =
                    tokio::time::timeout(Duration::from_secs(2), room.get_stats()).await
                    && let Ok(session) = session
                    && queried_generation == generation.load(Ordering::Acquire)
                {
                    history.observe(&session.publisher_stats, queried_generation);
                }
            }
        });
    }

    pub fn sender_stats(&self) -> Option<madscreen_ipc::diagnostics::SenderStats> {
        self.is_connected()
            .then(|| {
                self.sender_history
                    .latest_for_generation(self.stats_generation.load(Ordering::Acquire))
            })
            .flatten()
    }

    pub fn take_keyframe_request(&self) -> bool {
        self.source.take_keyframe_request()
    }

    pub fn take_rate_request(&self) -> Option<RateRequest> {
        self.source
            .take_rate_control_request()
            .map(|r| RateRequest {
                bitrate_bps: r.target_bitrate_bps,
                fps: r.framerate_fps,
            })
    }

    pub fn counts(&self) -> (u64, u64) {
        (self.pushed, self.rejected)
    }

    pub fn is_connected(&self) -> bool {
        self.room.connection_state() == livekit::ConnectionState::Connected
    }

    /// Fechamento assíncrono usado pelo lease do Worker; não leva o runtime do publicador.
    pub fn close_room(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let room = self.room.clone();
        async move {
            let _ = room.close().await;
        }
    }

    pub fn audio_pusher(&self) -> Option<AudioPusher> {
        self.audio.clone()
    }

    pub fn audio_counts(&self) -> (u64, u64) {
        self.audio.as_ref().map_or((0, 0), |audio| {
            (
                audio.pushed.load(Ordering::Relaxed),
                audio.rejected.load(Ordering::Relaxed),
            )
        })
    }

    pub fn close(self) {
        let _ = self.runtime.block_on(self.room.close());
    }
}
