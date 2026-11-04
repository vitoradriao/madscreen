//! Publicador H.264 Annex B no Cloudflare Realtime SFU, via Worker e libwebrtc direto.
//! Nenhum frame cru é criado: a fonte e o sender usam o caminho PreEncoded.

#[cfg(test)]
mod codec_tests;
mod config;
mod events;
#[cfg(test)]
mod loopback;
mod sdp;
mod signaling;
mod stats;

pub use config::{CfPublishConfig, load_server_env, resolve_credentials};
pub use signaling::{LiveKitConnection, LiveKitStart, SfuBackend, Worker as WorkerClient};

/// Taxa de amostragem e canais da faixa de áudio (Opus 48 kHz estéreo, blocos de 10 ms).
pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u32 = 2;
pub const AUDIO_FRAMES_PER_BLOCK: usize = 480;
/// Teto do bitrate Opus anunciado (estéreo de música a 128 kbps).
const OPUS_MAX_BITRATE_BPS: u64 = 128_000;

use std::{
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Sem AUs novos, o último IDR é reenviado a cada 10 s: o SFU descarta a faixa após 30 s sem pacotes.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

use libwebrtc::{
    MediaType,
    audio_frame::AudioFrame,
    audio_source::{AudioSourceOptions, native::NativeAudioSource},
    peer_connection::{
        BitrateSettings, IceGatheringState, OfferOptions, PeerConnection, PeerConnectionState,
    },
    peer_connection_factory::{
        ContinualGatheringPolicy, IceTransportsType, PeerConnectionFactory, RtcConfiguration,
        native::PeerConnectionFactoryExt,
    },
    rtp_parameters::RtpEncodingParameters,
    rtp_sender::VideoEncoderBackend,
    rtp_transceiver::{RtpTransceiver, RtpTransceiverDirection, RtpTransceiverInit},
    session_description::{SdpType, SessionDescription},
    video_frame::{EncodedFrameType, EncodedVideoCodec, EncodedVideoFrame},
    video_source::{VideoResolution, native::NativeVideoSource},
};
use reqwest::Method;
use serde::Serialize;
use signaling::{Generation, Heartbeat, StartResponse, Worker};
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};

pub type Result<T> = std::result::Result<T, Error>;

/// Erros não incluem SDP, token ou corpo de resposta do servidor.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("configuração ausente ou inválida: {0}")]
    Config(&'static str),
    #[error("falha ao criar runtime Cloudflare: {0}")]
    Runtime(#[source] std::io::Error),
    #[error("falha HTTP na sinalização Cloudflare: {0}")]
    Http(#[source] reqwest::Error),
    #[error("Worker devolveu HTTP {status} em {route}{detail}")]
    Status {
        route: &'static str,
        status: reqwest::StatusCode,
        /// Só `error`/`upstream`/`detail` do Worker (códigos de erro, sem SDP).
        detail: String,
    },
    #[error("geração antiga no Worker; é necessário republicar")]
    StaleGeneration,
    #[error("resposta inválida do Worker em {0}")]
    Protocol(&'static str),
    #[error("falha WebRTC: {0}")]
    WebRtc(&'static str),
    /// A resposta do SFU não pôde ser aplicada. O texto curto fica na mensagem;
    /// oferta, resposta e o erro do libwebrtc ficam no diagnóstico copiável.
    #[error("falha WebRTC: {message}")]
    Negotiation {
        message: &'static str,
        detail: String,
        offer_sdp: String,
        answer_sdp: String,
    },
    #[error("tempo limite aguardando {0}")]
    Timeout(&'static str),
}

impl Error {
    /// Resumo para o aviso e texto completo (SDP incluso) para copiar na tela de compartilhar.
    pub fn publish_notes(&self) -> (String, String) {
        match self {
            Self::Negotiation {
                message,
                detail,
                offer_sdp,
                answer_sdp,
            } => {
                let summary = if detail.is_empty() {
                    format!("falha WebRTC: {message}")
                } else {
                    format!("falha WebRTC: {message}: {detail}")
                };
                let body = format!(
                    "{summary}\n\n--- oferta SDP ---\n{offer_sdp}\n--- resposta SDP ---\n{answer_sdp}"
                );
                (summary, body)
            }
            other => {
                let summary = other.to_string();
                (summary.clone(), summary)
            }
        }
    }
}

/// Mesmo contrato do transporte LiveKit, sem dependência dele.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RateRequest {
    pub bitrate_bps: u64,
    pub fps: f64,
}

struct Session {
    pc: PeerConnection,
    source: NativeVideoSource,
    /// Fonte de áudio desta sessão; `None` sem áudio ou se o SFU recusou a faixa.
    audio_source: Option<NativeAudioSource>,
    generation: u64,
    // A factory deve sobreviver à conexão e à faixa.
    _factory: PeerConnectionFactory,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.pc.close();
    }
}

impl Session {
    async fn connect(config: &CfPublishConfig, worker: &Worker) -> Result<Self> {
        let factory = PeerConnectionFactory::default();
        let mut rtc = RtcConfiguration::default();
        rtc.ice_servers = worker.ice_servers().await?;
        rtc.continual_gathering_policy = ContinualGatheringPolicy::GatherOnce;
        if std::env::var("MADSCREEN_CF_FORCE_RELAY").is_ok_and(|value| value == "1") {
            rtc.ice_transport_type = IceTransportsType::Relay;
            tracing::info!("ICE restrito a relay para teste de TURN");
        }
        let pc = factory
            .create_peer_connection(rtc)
            .map_err(|_| Error::WebRtc("criando PeerConnection"))?;
        let source = NativeVideoSource::new_encoded(VideoResolution {
            width: config.width,
            height: config.height,
        });
        // Fila zero: caminho direto, blocos de exatamente 10 ms entregues pelo mixer.
        let audio_source = config.audio.then(|| {
            NativeAudioSource::new(
                AudioSourceOptions {
                    echo_cancellation: false,
                    noise_suppression: false,
                    auto_gain_control: false,
                },
                AUDIO_SAMPLE_RATE,
                AUDIO_CHANNELS,
                0,
            )
        });
        let mut session = Self {
            pc,
            source,
            audio_source,
            generation: 0,
            _factory: factory,
        };
        // Drop fecha o PC também em qualquer falha parcial da negociação.
        session.negotiate(config, worker).await?;
        Ok(session)
    }

    fn video_transceiver(&self, config: &CfPublishConfig) -> Result<RtpTransceiver> {
        let track = self
            ._factory
            .create_video_track(&config.track_name, self.source.clone());
        let transceiver = self
            .pc
            .add_transceiver(
                track.into(),
                RtpTransceiverInit {
                    direction: RtpTransceiverDirection::SendOnly,
                    stream_ids: vec!["madscreen".into()],
                    send_encodings: vec![RtpEncodingParameters {
                        max_bitrate: Some(config.max_bitrate_bps),
                        max_framerate: Some(config.max_fps),
                        ..Default::default()
                    }],
                },
            )
            .map_err(|_| Error::WebRtc("adicionando transceiver sendonly"))?;
        // Mesma ordem do LiveKit 0.10: add_transceiver -> backend -> codecs -> offer.
        transceiver
            .sender()
            .set_video_encoder_backend(VideoEncoderBackend::PreEncoded);
        let mut codecs = self
            ._factory
            .get_rtp_sender_capabilities(MediaType::Video)
            .codecs;
        if !codecs
            .iter()
            .any(|c| c.mime_type.eq_ignore_ascii_case("video/H264"))
        {
            return Err(Error::WebRtc("H.264 indisponível no libwebrtc"));
        }
        if config.only_h264 {
            // RTX continua anunciado para retransmissão de perdas, sem outro codec de mídia.
            codecs.retain(|c| {
                c.mime_type.eq_ignore_ascii_case("video/H264")
                    || c.mime_type.eq_ignore_ascii_case("video/rtx")
            });
        }
        if let Some(prefix) = &config.preferred_h264_profile {
            codecs.retain(|codec| {
                !codec.mime_type.eq_ignore_ascii_case("video/H264")
                    || codec.sdp_fmtp_line.as_deref().is_some_and(|line| {
                        line.split(';')
                            .filter_map(|part| part.trim().split_once('='))
                            .any(|(key, value)| {
                                key == "profile-level-id"
                                    && value.to_ascii_lowercase().starts_with(prefix.as_str())
                            })
                    })
            });
            if !codecs
                .iter()
                .any(|codec| codec.mime_type.eq_ignore_ascii_case("video/H264"))
            {
                return Err(Error::WebRtc(
                    "perfil H.264 do encoder indisponivel nas capacidades nativas",
                ));
            }
        }
        // set_codec_preferences aceita apenas capacidades nativas, filtradas/reordenadas.
        // Alterar fmtp aqui faz o libwebrtc rejeitar até o nível 4.0 em 1080p.
        // O nível do encoder é sinalizado depois, no SDP, preservando perfil e constraints.
        codecs.sort_by_key(|c| !c.mime_type.eq_ignore_ascii_case("video/H264"));
        transceiver.set_codec_preferences(codecs).map_err(|error| {
            tracing::error!(%error, "libwebrtc rejeitou as preferências de vídeo");
            Error::WebRtc("restringindo codecs do transceiver")
        })?;
        Ok(transceiver)
    }

    async fn negotiate(&mut self, config: &CfPublishConfig, worker: &Worker) -> Result<()> {
        let transceiver = self.video_transceiver(config)?;

        let audio_transceiver = match &self.audio_source {
            Some(source) => {
                let track = self
                    ._factory
                    .create_audio_track("madscreen-audio", source.clone());
                let transceiver = self
                    .pc
                    .add_transceiver(
                        track.into(),
                        RtpTransceiverInit {
                            direction: RtpTransceiverDirection::SendOnly,
                            stream_ids: vec!["madscreen".into()],
                            send_encodings: vec![RtpEncodingParameters {
                                max_bitrate: Some(OPUS_MAX_BITRATE_BPS),
                                ..Default::default()
                            }],
                        },
                    )
                    .map_err(|_| Error::WebRtc("adicionando transceiver de áudio"))?;
                let mut audio_codecs = self
                    ._factory
                    .get_rtp_sender_capabilities(MediaType::Audio)
                    .codecs;
                audio_codecs.retain(|c| c.mime_type.eq_ignore_ascii_case("audio/opus"));
                if audio_codecs.is_empty() {
                    return Err(Error::WebRtc("Opus indisponível no libwebrtc"));
                }
                transceiver
                    .set_codec_preferences(audio_codecs)
                    .map_err(|_| Error::WebRtc("restringindo codecs de áudio"))?;
                Some(transceiver)
            }
            None => None,
        };

        // O binding só expõe current_local_description (não a oferta pendente).
        // Guardamos candidatos do callback e os anexamos à única m-line de vídeo.
        let candidates = Arc::new(Mutex::new(Vec::<String>::new()));
        let collected = candidates.clone();
        self.pc.on_ice_candidate(Some(Box::new(move |candidate| {
            if candidate.sdp_mline_index() == 0 {
                lock(&collected).push(candidate.candidate());
            }
        })));
        let offer = self
            .pc
            .create_offer(OfferOptions::default())
            .await
            .map_err(|_| Error::WebRtc("criando oferta"))?;
        let mut offer_sdp = offer.to_string();
        if let Some(profile) = &config.h264_profile_level_id {
            offer_sdp = sdp::rewrite_h264_profile(&offer_sdp, profile)?;
        } else if let Some(level) = config.h264_level_idc {
            offer_sdp = sdp::raise_h264_level(&offer_sdp, level)?;
        }
        if audio_transceiver.is_some() {
            offer_sdp = sdp::rewrite_opus_stereo(&offer_sdp, OPUS_MAX_BITRATE_BPS);
        }
        let local = SessionDescription::parse(&offer_sdp, SdpType::Offer)
            .map_err(|_| Error::WebRtc("interpretando oferta H.264"))?;
        self.pc
            .set_local_description(local)
            .await
            .map_err(|_| Error::WebRtc("aplicando oferta local"))?;
        // Sem trickle ICE: espera a coleta completa, mas não depende dela. Com IPv6 ou STUN lento o
        // libwebrtc demora a declarar "complete"; com candidato srflx/relay (ou um host após
        // 3 s) a oferta já é útil para o SFU.
        let gather_start = Instant::now();
        let mut reachable_at: Option<Instant> = None;
        loop {
            if self.pc.ice_gathering_state() == IceGatheringState::Complete {
                break;
            }
            let (any, reachable) = {
                let list = lock(&candidates);
                (
                    !list.is_empty(),
                    list.iter()
                        .any(|c| c.contains(" typ srflx") || c.contains(" typ relay")),
                )
            };
            if reachable {
                let first = *reachable_at.get_or_insert_with(Instant::now);
                if first.elapsed() >= Duration::from_millis(800) {
                    break;
                }
            }
            if any && gather_start.elapsed() >= Duration::from_secs(3) {
                break;
            }
            if gather_start.elapsed() >= Duration::from_secs(8) {
                return Err(Error::Timeout("coleta ICE (8 s)"));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        self.pc.on_ice_candidate(None);
        {
            let list = lock(&candidates);
            tracing::info!(
                host = list.iter().filter(|c| c.contains(" typ host")).count(),
                srflx = list.iter().filter(|c| c.contains(" typ srflx")).count(),
                relay = list.iter().filter(|c| c.contains(" typ relay")).count(),
                "tipos de candidatos ICE locais coletados"
            );
        }
        // Com BUNDLE os candidatos pertencem à primeira m-line (vídeo).
        offer_sdp = sdp::insert_candidates(&offer_sdp, &lock(&candidates));
        let mid = transceiver
            .mid()
            .ok_or(Error::WebRtc("MID ausente após oferta local"))?;
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Start<'a> {
            expected_provider: &'static str,
            offer: &'a str,
            mid: &'a str,
            track_name: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            audio_mid: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            channel_id: Option<&'a str>,
        }
        let audio_mid = audio_transceiver.as_ref().and_then(|t| t.mid());
        let started: StartResponse = worker
            .request(
                Method::POST,
                "/api/share/start",
                &Start {
                    expected_provider: "cloudflare",
                    offer: &offer_sdp,
                    mid: &mid,
                    track_name: &config.track_name,
                    audio_mid: audio_mid.as_deref(),
                    channel_id: config.channel_id.as_deref(),
                },
            )
            .await?;
        self.generation = started.generation;
        let result = async {
            sdp::validate_answer(&started.answer)?;
            if audio_transceiver.is_some() && !sdp::audio_accepted(&started.answer) {
                // Sem áudio é melhor que sem transmissão: segue só com o vídeo.
                tracing::warn!("SFU recusou a faixa de áudio; transmitindo só o vídeo");
                self.audio_source = None;
            }
            tracing::info!(
                nack = started.answer.lines().any(|line| {
                    line.starts_with("a=rtcp-fb:") && line.trim_end().ends_with(" nack")
                }),
                rtx = started.answer.lines().any(|line| {
                    line.starts_with("a=rtpmap:")
                        && line.to_ascii_lowercase().ends_with(" rtx/90000")
                }),
                "recuperação de perdas anunciada na resposta do SFU"
            );
            // Sem `stereo=1` na resposta o libwebrtc codifica o Opus em mono.
            let answer_sdp = if audio_transceiver.is_some() && self.audio_source.is_some() {
                sdp::rewrite_opus_stereo(&started.answer, OPUS_MAX_BITRATE_BPS)
            } else {
                started.answer.clone()
            };
            let answer = SessionDescription::parse(&answer_sdp, SdpType::Answer)
                .map_err(|_| Error::WebRtc("interpretando resposta do SFU"))?;
            self.pc
                .set_remote_description(answer)
                .await
                .map_err(|error| Error::Negotiation {
                    message: "aplicando resposta do SFU",
                    detail: error.to_string(),
                    offer_sdp: offer_sdp.clone(),
                    answer_sdp: answer_sdp.clone(),
                })?;
            // Sem isso o GCC parte de ~0,3 Mbps e só chega ao teto depois de ~10 s.
            let start = config
                .start_bitrate_bps
                .unwrap_or(config.max_bitrate_bps / 2)
                .clamp(300_000, config.max_bitrate_bps);
            self.pc
                .set_bitrate(BitrateSettings {
                    min_bitrate_bps: None,
                    start_bitrate_bps: Some(start),
                    max_bitrate_bps: Some(config.max_bitrate_bps),
                })
                .map_err(|_| Error::WebRtc("definindo bitrate inicial"))?;
            tokio::time::timeout(Duration::from_secs(15), async {
                loop {
                    match self.pc.connection_state() {
                        PeerConnectionState::Connected => return Ok(()),
                        PeerConnectionState::Failed | PeerConnectionState::Closed => {
                            return Err(Error::WebRtc("conexão com SFU falhou"));
                        }
                        _ => tokio::time::sleep(Duration::from_millis(25)).await,
                    }
                }
            })
            .await
            .map_err(|_| Error::Timeout("conexão com SFU"))?
        }
        .await;
        if result.is_err() {
            worker.stop(self.generation).await;
        }
        result
    }
}

// Um panic em outra thread não torna o estado de mídia inutilizável.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Keepalive {
    keyframe: Vec<u8>,
    delta_since_keyframe: bool,
}

struct Media {
    session: Option<Arc<Session>>,
    keepalive: Option<Keepalive>,
    last_timestamp_us: i64,
    last_delivery: Instant,
    /// Último IDR entregue (real, de repouso ou keepalive).
    last_keyframe: Instant,
}

struct Shared {
    media: Mutex<Media>,
    resolution: VideoResolution,
    pushed: AtomicU64,
    rejected: AtomicU64,
    keepalives: AtomicU64,
    reconnects: AtomicU64,
    keyframe_needed: AtomicBool,
    audio_pushed: AtomicU64,
    audio_rejected: AtomicU64,
    sender_history: madscreen_rtc_stats::SenderHistory,
}

fn capture(
    source: &NativeVideoSource,
    resolution: &VideoResolution,
    au: &[u8],
    keyframe: bool,
    timestamp_us: i64,
) -> bool {
    source.capture_encoded_frame(&EncodedVideoFrame {
        codec: EncodedVideoCodec::H264,
        payload: au,
        timestamp_us,
        frame_type: if keyframe {
            EncodedFrameType::Key
        } else {
            EncodedFrameType::Delta
        },
        resolution: resolution.clone(),
        frame_metadata: None,
    })
}

impl Shared {
    fn push(&self, au: &[u8], keyframe: bool, timestamp_us: i64) -> bool {
        let mut media = lock(&self.media);
        let timestamp_us = timestamp_us.max(media.last_timestamp_us.saturating_add(1));
        let accepted = media
            .session
            .as_ref()
            .is_some_and(|s| capture(&s.source, &self.resolution, au, keyframe, timestamp_us));
        if accepted {
            self.pushed.fetch_add(1, Ordering::Relaxed);
            media.last_timestamp_us = timestamp_us;
            media.last_delivery = Instant::now();
            if keyframe {
                media.last_keyframe = media.last_delivery;
            }
            if let Some(cache) = &mut media.keepalive {
                if keyframe {
                    // Só copia IDR com keepalive habilitado; reutiliza a capacidade entre IDRs.
                    cache.keyframe.clear();
                    cache.keyframe.extend_from_slice(au);
                }
                cache.delta_since_keyframe = !keyframe;
            }
        } else {
            self.rejected.fetch_add(1, Ordering::Relaxed);
        }
        accepted
    }

    fn keepalive(&self) {
        let mut media = lock(&self.media);
        let (Some(session), Some(cache)) = (&media.session, &media.keepalive) else {
            return;
        };
        if cache.keyframe.is_empty() || media.last_delivery.elapsed() < KEEPALIVE_INTERVAL {
            return;
        }
        // Avança o relógio também para IDRs repetidos; nunca repete o timestamp RTP.
        let timestamp_us = media.last_timestamp_us.saturating_add(
            media
                .last_delivery
                .elapsed()
                .as_micros()
                .min(i64::MAX as u128) as i64,
        );
        if capture(
            &session.source,
            &self.resolution,
            &cache.keyframe,
            true,
            timestamp_us,
        ) {
            if cache.delta_since_keyframe {
                // O decoder voltou ao IDR antigo; o próximo quadro real deve renovar referências.
                self.keyframe_needed.store(true, Ordering::Release);
            }
            self.pushed.fetch_add(1, Ordering::Relaxed);
            self.keepalives.fetch_add(1, Ordering::Relaxed);
            media.last_timestamp_us = timestamp_us;
            media.last_delivery = Instant::now();
            media.last_keyframe = media.last_delivery;
        }
    }
}

/// Entrega blocos de áudio de outra thread (o mixer). Sempre usa a sessão atual: sobrevive a
/// reconexões. Não pode ser chamado de dentro do runtime tokio.
#[derive(Clone)]
pub struct AudioPusher {
    shared: Arc<Shared>,
    handle: tokio::runtime::Handle,
}

impl AudioPusher {
    /// Um bloco de 10 ms: 480 quadros, estéreo intercalado, i16.
    pub fn push(&self, block: &[i16]) -> bool {
        let source = lock(&self.shared.media)
            .session
            .as_ref()
            .and_then(|s| s.audio_source.clone());
        let ok = source.is_some_and(|source| {
            block.len() == AUDIO_FRAMES_PER_BLOCK * AUDIO_CHANNELS as usize
                && self
                    .handle
                    .block_on(source.capture_frame(&AudioFrame {
                        data: std::borrow::Cow::Borrowed(block),
                        sample_rate: AUDIO_SAMPLE_RATE,
                        num_channels: AUDIO_CHANNELS,
                        samples_per_channel: AUDIO_FRAMES_PER_BLOCK as u32,
                    }))
                    .is_ok()
        });
        let counter = if ok {
            &self.shared.audio_pushed
        } else {
            &self.shared.audio_rejected
        };
        counter.fetch_add(1, Ordering::Relaxed);
        ok
    }
}

pub struct CloudflarePublisher {
    runtime: tokio::runtime::Runtime,
    shared: Arc<Shared>,
    shutdown: oneshot::Sender<()>,
    supervisor: JoinHandle<()>,
    audio: bool,
}

impl CloudflarePublisher {
    /// Bloqueia até a conexão inicial. Reconexões posteriores são feitas no runtime próprio.
    pub fn connect(config: &CfPublishConfig) -> Result<Self> {
        let mut config = config.clone();
        (config.worker_url, config.token) =
            resolve_credentials(Some(&config.worker_url), Some(&config.token))?;
        if config.width == 0
            || config.height == 0
            || config.max_bitrate_bps == 0
            || !config.max_fps.is_finite()
            || config.max_fps <= 0.
        {
            return Err(Error::Config("resolução, bitrate ou FPS"));
        }
        if let Some(profile) = &config.h264_profile_level_id {
            if profile.len() != 6 || !profile.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(Error::Config(
                    "--cf-profile-level-id (seis dígitos hexadecimais)",
                ));
            }
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("madscreen-cf")
            .enable_all()
            .build()
            .map_err(Error::Runtime)?;
        // Client e fonte são construídos dentro do runtime (inclusive em builds sem LiveKit).
        let (worker, session) = runtime.block_on(async {
            let worker = Worker::new(&config)?;
            let session = Session::connect(&config, &worker).await?;
            Ok::<_, Error>((worker, Arc::new(session)))
        })?;
        let shared = Arc::new(Shared {
            media: Mutex::new(Media {
                session: Some(session.clone()),
                keepalive: config.keepalive.then(|| Keepalive {
                    keyframe: Vec::new(),
                    delta_since_keyframe: false,
                }),
                last_timestamp_us: 0,
                last_delivery: Instant::now(),
                last_keyframe: Instant::now(),
            }),
            resolution: VideoResolution {
                width: config.width,
                height: config.height,
            },
            pushed: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            keepalives: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
            keyframe_needed: AtomicBool::new(true),
            audio_pushed: AtomicU64::new(0),
            audio_rejected: AtomicU64::new(0),
            sender_history: madscreen_rtc_stats::SenderHistory::default(),
        });
        let (shutdown, receiver) = oneshot::channel();
        let audio = config.audio;
        let supervisor =
            runtime.spawn(supervise(config, worker, shared.clone(), session, receiver));
        tracing::info!("faixa H.264 pré-codificada publicada no Cloudflare");
        Ok(Self {
            runtime,
            shared,
            shutdown,
            supervisor,
            audio,
        })
    }

    /// AU Annex B completo, com SPS/PPS no IDR; nenhuma cópia Rust com keepalive desligado.
    pub fn push(&mut self, au: &[u8], keyframe: bool, timestamp_us: i64) -> bool {
        self.shared.push(au, keyframe, timestamp_us)
    }

    pub fn take_keyframe_request(&self) -> bool {
        let requested = lock(&self.shared.media)
            .session
            .as_ref()
            .is_some_and(|s| s.source.take_keyframe_request());
        self.shared.keyframe_needed.swap(false, Ordering::AcqRel) || requested
    }

    pub fn take_rate_request(&self) -> Option<RateRequest> {
        lock(&self.shared.media)
            .session
            .as_ref()?
            .source
            .take_rate_control_request()
            .map(|r| RateRequest {
                bitrate_bps: r.target_bitrate_bps,
                fps: r.framerate_fps,
            })
    }

    /// Entregues à fonte (inclui keepalive), recusados. Não implica envio confirmado no RTP.
    pub fn counts(&self) -> (u64, u64) {
        (
            self.shared.pushed.load(Ordering::Relaxed),
            self.shared.rejected.load(Ordering::Relaxed),
        )
    }

    /// Número de tentativas de recriar a sessão após a conexão inicial.
    pub fn reconnect_count(&self) -> u64 {
        self.shared.reconnects.load(Ordering::Relaxed)
    }

    /// Estado atual da conexão de mídia; não confundir com AUs aceitos pelo encoder.
    pub fn is_connected(&self) -> bool {
        let session = lock(&self.shared.media).session.clone();
        session
            .is_some_and(|session| session.pc.connection_state() == PeerConnectionState::Connected)
    }

    pub fn sender_stats(&self) -> Option<madscreen_ipc::diagnostics::SenderStats> {
        let media = lock(&self.shared.media);
        let session = media.session.as_ref()?;
        if session.pc.connection_state() != PeerConnectionState::Connected {
            return None;
        }
        self.shared
            .sender_history
            .latest_for_generation(session.generation)
    }

    pub fn spawn_stats_logger(&self, interval: Duration) {
        stats::spawn(&self.runtime, self.shared.clone(), interval);
    }

    /// Handle para empurrar áudio; `None` quando a publicação foi configurada sem áudio.
    pub fn audio_pusher(&self) -> Option<AudioPusher> {
        self.audio.then(|| AudioPusher {
            shared: self.shared.clone(),
            handle: self.runtime.handle().clone(),
        })
    }

    /// Blocos de áudio entregues ao WebRTC e recusados (sem sessão ou erro).
    pub fn audio_counts(&self) -> (u64, u64) {
        (
            self.shared.audio_pushed.load(Ordering::Relaxed),
            self.shared.audio_rejected.load(Ordering::Relaxed),
        )
    }

    /// Aguarda a sinalização em voo e encerra a geração atual antes de destruir o runtime.
    pub fn close(self) {
        let _ = self.shutdown.send(());
        if let Err(error) = self.runtime.block_on(self.supervisor) {
            tracing::warn!(%error, "supervisor Cloudflare terminou inesperadamente");
        }
    }
}

async fn supervise(
    config: CfPublishConfig,
    worker: Worker,
    shared: Arc<Shared>,
    mut session: Arc<Session>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let (background_shutdown, receiver) = watch::channel(());
    let events = tokio::spawn(events::run(
        worker.clone(),
        config.token.clone(),
        shared.clone(),
        receiver.clone(),
    ));
    let feedback = tokio::spawn(stats::poll_keyframes(shared.clone(), receiver));
    let mut heartbeat_at = Instant::now();
    let mut heartbeat_errors = 0;
    let mut disconnected_at = None;
    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    'running: loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break,
            _ = ticker.tick() => {}
        }
        shared.keepalive();
        let state = session.pc.connection_state();
        if state == PeerConnectionState::Disconnected {
            disconnected_at.get_or_insert_with(Instant::now);
        } else {
            disconnected_at = None;
        }
        let mut reconnect = matches!(
            state,
            PeerConnectionState::Failed | PeerConnectionState::Closed
        ) || disconnected_at
            .is_some_and(|at| at.elapsed() >= Duration::from_secs(10));
        if !reconnect && heartbeat_at.elapsed() >= Duration::from_secs(5) {
            match worker
                .request::<Heartbeat>(
                    Method::POST,
                    "/api/share/heartbeat",
                    &Generation {
                        generation: session.generation,
                    },
                )
                .await
            {
                Ok(beat) => {
                    reconnect = !beat.ok || beat.generation != session.generation;
                    if beat.keyframe {
                        shared.keyframe_needed.store(true, Ordering::Release);
                    }
                    heartbeat_errors = 0;
                }
                Err(Error::StaleGeneration) => reconnect = true,
                Err(error) => {
                    heartbeat_errors += 1;
                    tracing::warn!(%error, "falha no heartbeat Cloudflare");
                    reconnect = heartbeat_errors >= 3;
                }
            }
            heartbeat_at = Instant::now();
        }
        if !reconnect {
            continue;
        }
        lock(&shared.media).session = None;
        session.pc.close();
        worker.stop(session.generation).await;
        let mut backoff = Duration::from_secs(1);
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown => break 'running,
                _ = tokio::time::sleep(backoff) => {}
            }
            let attempt = shared.reconnects.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::warn!(tentativa = attempt, "recriando sessão Cloudflare");
            // Não cancela share/start: close precisa saber a geração para fechá-la.
            match Session::connect(&config, &worker).await {
                Ok(next) => {
                    session = Arc::new(next);
                    lock(&shared.media).session = Some(session.clone());
                    shared.keyframe_needed.store(true, Ordering::Release);
                    heartbeat_at = Instant::now();
                    heartbeat_errors = 0;
                    disconnected_at = None;
                    tracing::info!("sessão Cloudflare restabelecida");
                    break;
                }
                Err(error) => {
                    tracing::warn!(%error, "falha ao recriar sessão Cloudflare");
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
            }
        }
    }
    let _ = background_shutdown.send(());
    let _ = tokio::join!(events, feedback);
    lock(&shared.media).session = None;
    session.pc.close();
    worker.stop(session.generation).await;
}
