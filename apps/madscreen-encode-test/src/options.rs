use clap::{Parser, ValueEnum};
use madscreen_encode::{EncoderConfig, FrameRate, MultiPass, Preset, VideoCodec};
use std::{path::PathBuf, str::FromStr};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum EncoderArg {
    Auto,
    Nvenc,
    Amf,
    Qsv,
    X264,
    /// Candidato CPU isolado; nao participa da selecao automatica.
    Mf,
    /// Candidato CPU FFmpeg libopenh264 (I420), opt-in; nao participa da selecao automatica
    /// e nao publica ate existir contrato de receiver para o SPS real (42c0/640c).
    #[value(name = "openh264")]
    OpenH264,
}
impl EncoderArg {
    pub fn requested(self) -> Option<madscreen_encode::backend::BackendKind> {
        use madscreen_encode::backend::BackendKind;
        match self {
            Self::Auto => None,
            Self::Nvenc => Some(BackendKind::Nvenc),
            Self::Amf => Some(BackendKind::Amf),
            Self::Qsv => Some(BackendKind::Qsv),
            Self::X264 => Some(BackendKind::X264),
            Self::Mf => Some(BackendKind::Mf),
            Self::OpenH264 => Some(BackendKind::OpenH264),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Transport {
    Livekit,
    Cloudflare,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Synthetic,
    Picker,
    Monitor(usize),
    Window(String),
    /// HWND exato, como listado por `--list-sources`.
    WindowHandle(i64),
}
impl FromStr for Source {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        if s == "synthetic" {
            return Ok(Self::Synthetic);
        }
        if s == "picker" {
            return Ok(Self::Picker);
        }
        if let Some(index) = s.strip_prefix("monitor:") {
            return index
                .parse()
                .map(Self::Monitor)
                .map_err(|_| "monitor deve usar índice base zero".into());
        }
        if let Some(hwnd) = s.strip_prefix("hwnd:") {
            return hwnd
                .parse()
                .ok()
                .filter(|&h: &i64| h != 0)
                .map(Self::WindowHandle)
                .ok_or_else(|| "hwnd inválido".into());
        }
        if let Some(title) = s.strip_prefix("window:").filter(|v| !v.trim().is_empty()) {
            return Ok(Self::Window(title.into()));
        }
        Err("fonte esperada: synthetic, picker, monitor:N, window:trecho ou hwnd:N".into())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}
impl FromStr for Size {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (w, h) = s.split_once('x').ok_or("use WxH")?;
        let width = w.parse().map_err(|_| "largura inválida")?;
        let height = h.parse().map_err(|_| "altura inválida")?;
        let c = EncoderConfig {
            width,
            height,
            ..Default::default()
        };
        c.validate().map_err(|e| e.to_string())?;
        Ok(Self { width, height })
    }
}
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum CaptureBackendArg {
    #[default]
    Auto,
    Wgc,
    DesktopDuplication,
}
impl From<CaptureBackendArg> for madscreen_capture::CaptureBackend {
    fn from(value: CaptureBackendArg) -> Self {
        match value {
            CaptureBackendArg::Auto => Self::Auto,
            CaptureBackendArg::Wgc => Self::Wgc,
            CaptureBackendArg::DesktopDuplication => Self::DesktopDuplication,
        }
    }
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CodecArg {
    H264,
    Hevc,
    Av1,
}
impl From<CodecArg> for VideoCodec {
    fn from(v: CodecArg) -> Self {
        match v {
            CodecArg::H264 => Self::H264,
            CodecArg::Hevc => Self::Hevc,
            CodecArg::Av1 => Self::Av1,
        }
    }
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PresetArg {
    P1,
    P2,
    P3,
    P4,
    P5,
    P6,
    P7,
}
impl From<PresetArg> for Preset {
    fn from(v: PresetArg) -> Self {
        match v {
            PresetArg::P1 => Self::P1,
            PresetArg::P2 => Self::P2,
            PresetArg::P3 => Self::P3,
            PresetArg::P4 => Self::P4,
            PresetArg::P5 => Self::P5,
            PresetArg::P6 => Self::P6,
            PresetArg::P7 => Self::P7,
        }
    }
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ConvertArg {
    Shader,
    Vp,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum InputArg {
    Nv12,
    Bgra,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum MultiPassArg {
    Disabled,
    QuarterRes,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SyncArg {
    Flush,
    Fence,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SyntheticArg {
    Static,
    Animated,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PriorityArg {
    Normal,
    AboveNormal,
    High,
}
impl From<PriorityArg> for madscreen_encode::pipeline::GpuPriority {
    fn from(value: PriorityArg) -> Self {
        match value {
            PriorityArg::Normal => Self::Normal,
            PriorityArg::AboveNormal => Self::AboveNormal,
            PriorityArg::High => Self::High,
        }
    }
}
fn bitrate(value: &str) -> Result<u32, String> {
    let mbps: f64 = value.parse().map_err(|_| "bitrate inválido")?;
    let bits = mbps * 1_000_000.;
    if !bits.is_finite() || bits < 1. || bits > u32::MAX as f64 {
        return Err("bitrate deve ser positivo e caber em u32 bps".into());
    }
    Ok(bits.round() as u32)
}
fn fps(value: &str) -> Result<FrameRate, String> {
    let (num, den) = value.split_once('/').unwrap_or((value, "1"));
    FrameRate::new(
        num.parse().map_err(|_| "FPS inválido")?,
        den.parse().map_err(|_| "denominador inválido")?,
    )
    .map_err(|e| e.to_string())
}
fn height(value: &str) -> Result<u32, String> {
    match value {
        "native" => Ok(0),
        "720" => Ok(720),
        "1080" => Ok(1080),
        "1440" => Ok(1440),
        "2160" => Ok(2160),
        _ => Err("altura esperada: native, 720, 1080, 1440 ou 2160".into()),
    }
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum H264ProfileArg {
    High,
    Main,
    ConstrainedBaseline,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum RatePolicyArg {
    Legacy,
    AbsoluteRamp,
}
#[derive(Clone, Debug, Parser)]
#[command(
    about = "Captura e transmissão Windows: NVENC, AMD AMF, Intel QuickSync e CPU x264",
    version
)]
pub struct Options {
    #[arg(skip)]
    pub expected_window_instance: Option<String>,
    /// Automático: hardware na GPU da captura, outros backends e CPU por último.
    #[arg(long, value_enum, default_value = "auto")]
    pub encoder: EncoderArg,
    /// Controla uma sessão pelo protocolo JSON em linhas no stdin/stdout.
    #[arg(long)]
    pub ipc: bool,
    /// Imprime uma linha JSON com janelas e monitores capturáveis e sai.
    #[arg(long)]
    pub list_sources: bool,
    /// Com `--list-sources`: lista só monitores, sem miniaturas nem ícones.
    #[arg(long, requires = "list_sources")]
    pub monitors_only: bool,
    /// Com `--list-sources`: omite as janelas deste processo (o app que chamou).
    #[arg(long, requires = "list_sources")]
    pub exclude_pid: Option<u32>,
    #[arg(long, default_value = "synthetic")]
    pub source: Source,
    #[arg(long, value_enum, default_value = "h264")]
    pub codec: CodecArg,
    /// Laboratorio: perfil real do encoder, nao apenas alteracao de SDP.
    #[arg(
        long,
        env = "MADSCREEN_H264_PROFILE",
        value_enum,
        default_value = "constrained-baseline"
    )]
    pub h264_profile: H264ProfileArg,
    #[arg(long, value_enum, default_value = "p1")]
    pub preset: PresetArg,
    /// Bitrate em Mbps decimais.
    #[arg(long,default_value="18",value_parser=bitrate)]
    pub bitrate: u32,
    /// Histórico local de contadores; desligue com --diagnostics false ou MADSCREEN_DIAGNOSTICS=false.
    #[arg(long, env = "MADSCREEN_DIAGNOSTICS", default_value_t = true, action = clap::ArgAction::Set)]
    pub diagnostics: bool,
    /// Salva amostras de publicação em JSONL local, sem sobrescrever arquivos.
    #[arg(long)]
    pub diagnostics_output: Option<std::path::PathBuf>,
    /// Laboratorio com midia propria: salva ate 16 MiB de AUs publicados para inspecao.
    #[arg(long, env = "MADSCREEN_LAB_AU_OUTPUT")]
    pub lab_au_output: Option<PathBuf>,
    #[arg(long,default_value="144",value_parser=fps)]
    pub fps: FrameRate,
    /// Piso do bitrate (Mbps) aceito do WebRTC; sem opção usa 1 Mbps.
    /// Com --rate-policy legacy, preserva o piso anterior de metade de --bitrate.
    #[arg(long,value_parser=bitrate)]
    pub min_bitrate: Option<u32>,
    /// Adaptação com piso absoluto e subida gradual; legacy permite A/B e retorno à política anterior.
    #[arg(
        long,
        env = "MADSCREEN_RATE_POLICY",
        value_enum,
        default_value = "absolute-ramp"
    )]
    pub rate_policy: RatePolicyArg,
    /// Saída WxH; sem opção usa tamanho da fonte (synthetic: 1920x1080).
    #[arg(long)]
    pub size: Option<Size>,
    /// Altura da saída (native, 720, 1080, 1440 ou 2160), preservando a proporção da fonte.
    /// Com fonte synthetic, `--size` define o tamanho da fonte simulada (ex.: 5120x1440);
    /// com captura, a fonte real manda e `--size` é ignorado.
    #[arg(long, value_parser = height)]
    pub height: Option<u32>,
    /// Escolha efetiva de altura para planejamento em runtime; zero significa nativa.
    #[arg(skip)]
    pub height_choice: Option<u32>,
    /// Recalcula o bitrate pela saída planejada; ativado pelo IPC quando não há bitrate explícito.
    #[arg(skip)]
    pub bitrate_auto: bool,
    #[arg(long,default_value_t=1,value_parser=clap::value_parser!(u32).range(1..))]
    pub vbv_frames: u32,
    /// Segundos medidos, após warmup. Zero continua até Ctrl+C.
    #[arg(long, default_value_t = 30)]
    pub duration: u32,
    #[arg(long, default_value_t = 5)]
    pub warmup: u32,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "shader")]
    pub convert: ConvertArg,
    /// Laboratorio: reducao box superamostrada 4x4 no shader, bilinear por padrao.
    #[arg(long)]
    pub reduction_box: bool,
    #[arg(long, value_enum, default_value = "nv12")]
    pub input: InputArg,
    #[arg(long)]
    pub compare_presets: bool,
    #[arg(long, conflicts_with = "compare_presets")]
    pub compare_convert: bool,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=15))]
    pub inflight: u32,
    /// Laboratorio: libera superficies sem aguardar telemetria GPU opcional.
    #[arg(
        long,
        env = "MADSCREEN_INDEPENDENT_GPU_METRICS",
        default_value_t = false
    )]
    pub independent_gpu_metrics: bool,
    /// Laboratorio A/B: desliga somente a instrumentacao de saturacao.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub pipeline_saturation_metrics: bool,
    /// Laboratorio: atraso artificial de visibilidade das queries, nao da GPU.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u64).range(0..=1000))]
    pub gpu_query_delay_ms: u64,
    #[arg(long, value_enum, default_value = "flush")]
    pub sync: SyncArg,
    /// Limita o WGC ao FPS pedido (MinUpdateInterval = 90% do período) em vez de entregar todo
    /// frame do DWM; reduz cópias/conversões desperdiçadas quando o jogo roda acima do alvo.
    #[arg(long)]
    pub capture_limit: bool,
    /// Backend de captura; auto usa Desktop Duplication para monitor no Windows 10.
    /// Override de laboratório/retorno também aceito pelo sidecar IPC.
    #[arg(
        long,
        env = "MADSCREEN_CAPTURE_BACKEND",
        value_enum,
        default_value = "auto"
    )]
    pub capture_backend: CaptureBackendArg,
    #[arg(long, value_enum, default_value = "above-normal")]
    pub gpu_priority: PriorityArg,
    #[arg(long, value_enum, default_value = "animated")]
    pub synthetic: SyntheticArg,
    /// Laboratorio: quadros BGRA proprios pre-carregados, tamanho igual a --size.
    #[arg(long)]
    pub synthetic_frames: Option<PathBuf>,
    #[arg(long)]
    pub probe: bool,
    #[arg(long)]
    pub report: Option<PathBuf>,
    #[arg(long)]
    pub frames_csv: Option<PathBuf>,
    #[arg(long,default_value_t=5,value_parser=clap::value_parser!(u32).range(1..))]
    pub stats_interval: u32,
    /// Se AV1 não existir na GPU, usa HEVC e registra codec efetivo.
    #[arg(long)]
    pub fallback_hevc: bool,
    #[arg(long, value_enum, default_value = "disabled")]
    pub multipass: MultiPassArg,
    /// Laboratorio NVENC: AQ espacial; nao habilita lookahead ou AQ temporal.
    #[arg(long)]
    pub spatial_aq: bool,
    /// Laboratorio: tempos CPU dos estagios de readback FFmpeg.
    #[arg(long)]
    pub readback_metrics: bool,
    #[arg(long,requires="intra_refresh_count",value_parser=clap::value_parser!(u32).range(1..))]
    pub intra_refresh_period: Option<u32>,
    #[arg(long,requires="intra_refresh_period",value_parser=clap::value_parser!(u32).range(1..))]
    pub intra_refresh_count: Option<u32>,
    /// Transporte de publicação (build com `--features publish`). Cloudflare ativa publicação.
    #[arg(
        long,
        env = "MADSCREEN_TRANSPORT",
        value_enum,
        default_value = "livekit"
    )]
    pub transport: Transport,
    /// URL do Worker Cloudflare; fallback em MADSCREEN_HOME/server.env ou APPDATA/madscreen/server.env.
    #[arg(long, env = "MADSCREEN_CF_URL")]
    pub cf_url: Option<String>,
    /// Canal de vídeo escolhido no servidor.
    #[arg(long, env = "MADSCREEN_CF_CHANNEL")]
    pub cf_channel: Option<String>,
    /// Token de dono/publicador do Worker (também aceita server.env).
    #[arg(long, env = "MADSCREEN_CF_TOKEN", hide_env_values = true)]
    pub cf_token: Option<String>,
    /// Perfil H.264 anunciado no SDP, seis dígitos hexadecimais (ex.: 640c1f).
    #[arg(long, value_parser = parse_profile_level_id)]
    pub cf_profile_level_id: Option<String>,
    /// Estimativa inicial de banda em Mbps (padrão: metade de --bitrate).
    #[arg(long)]
    pub cf_start_bitrate: Option<f64>,
    /// Reenvia o último IDR a cada 5 s sem AUs para manter a faixa no SFU.
    #[arg(long)]
    pub cf_keepalive: bool,
    /// Publica também o som do PC todo (menos o madscreen) como faixa Opus.
    #[arg(long)]
    pub audio_system: bool,
    /// Publica o som da árvore de processos deste PID raiz (repetível).
    #[arg(long = "audio-pid")]
    pub audio_pids: Vec<u32>,
    /// Publica o som do aplicativo dono da janela capturada.
    #[arg(long)]
    pub audio_follow_window: bool,
    /// Raiz da árvore excluída do som do PC; sem valor, o próprio processo.
    #[arg(long)]
    pub audio_exclude_pid: Option<u32>,
    /// Publica o bitstream H.264 numa sala LiveKit (build com `--features publish`), ex.: ws://127.0.0.1:7880.
    #[arg(long)]
    pub livekit_url: Option<String>,
    /// Token de acesso pronto; sem ele usa --livekit-api-key/--livekit-api-secret.
    #[arg(long, env = "LIVEKIT_TOKEN", hide_env_values = true)]
    pub livekit_token: Option<String>,
    #[arg(long, default_value = "devkey")]
    pub livekit_api_key: String,
    #[arg(
        long,
        env = "LIVEKIT_API_SECRET",
        hide_env_values = true,
        default_value = "secret"
    )]
    pub livekit_api_secret: String,
    #[arg(long, default_value = "madscreen")]
    pub room: String,
    #[arg(long, default_value = "madscreen-host")]
    pub identity: String,
}
impl Options {
    #[cfg(all(windows, feature = "publish"))]
    pub fn audio_requested(&self) -> bool {
        self.audio_system || !self.audio_pids.is_empty() || self.audio_follow_window
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn publishing(&self) -> bool {
        self.transport == Transport::Cloudflare || self.livekit_url.is_some()
    }

    pub fn config(&self, width: u32, height: u32, preset: Preset) -> anyhow::Result<EncoderConfig> {
        anyhow::ensure!(
            !(self.publishing() && matches!(self.encoder, EncoderArg::OpenH264)),
            "--encoder openh264 nao publica: o SPS real (42c0/640c) ainda nao tem contrato de receiver demonstrado"
        );
        anyhow::ensure!(
            !(self.compare_presets || self.compare_convert) || self.duration > 0,
            "comparações exigem --duration > 0"
        );
        let c = EncoderConfig {
            codec: self.codec.into(),
            h264_level: None,
            h264_profile: match self.h264_profile {
                H264ProfileArg::High => madscreen_encode::config::H264Profile::High,
                H264ProfileArg::Main => madscreen_encode::config::H264Profile::Main,
                H264ProfileArg::ConstrainedBaseline => {
                    madscreen_encode::config::H264Profile::ConstrainedBaseline
                }
            },
            width,
            height,
            fps: self.fps,
            bitrate_bps: self.bitrate,
            vbv_frames: self.vbv_frames,
            preset,
            spatial_aq: self.spatial_aq,
            readback_metrics: self.readback_metrics,
            multipass: match self.multipass {
                MultiPassArg::Disabled => MultiPass::Disabled,
                MultiPassArg::QuarterRes => MultiPass::QuarterRes,
            },
            intra_refresh: self
                .intra_refresh_period
                .zip(self.intra_refresh_count)
                .map(|(period, count)| madscreen_encode::config::IntraRefresh { period, count }),
        };
        c.validate()?;
        Ok(c)
    }
}

fn parse_profile_level_id(value: &str) -> Result<String, String> {
    if value.len() == 6 && value.bytes().all(|c| c.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err("profile-level-id deve conter seis dígitos hexadecimais".into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pipeline_flags_and_defaults() -> anyhow::Result<()> {
        let defaults = Options::try_parse_from(["encode"])?;
        assert_eq!(defaults.inflight, 3);
        assert!(matches!(defaults.sync, SyncArg::Flush));
        assert!(matches!(defaults.gpu_priority, PriorityArg::AboveNormal));
        assert!(matches!(defaults.synthetic, SyntheticArg::Animated));
        let forced =
            Options::try_parse_from(["encode", "--capture-backend", "desktop-duplication"])?;
        assert_eq!(
            madscreen_capture::CaptureBackend::from(forced.capture_backend),
            madscreen_capture::CaptureBackend::DesktopDuplication
        );
        assert!(Options::try_parse_from(["encode", "--capture-backend", "invalid"]).is_err());
        let parsed = Options::try_parse_from([
            "encode",
            "--inflight",
            "1",
            "--sync",
            "fence",
            "--gpu-priority",
            "high",
            "--synthetic",
            "static",
            "--compare-convert",
        ])?;
        assert_eq!(parsed.inflight, 1);
        assert!(parsed.compare_convert);
        assert!(matches!(parsed.sync, SyncArg::Fence));
        assert_eq!(
            madscreen_encode::pipeline::GpuPriority::from(parsed.gpu_priority).process_class(),
            4
        );
        for args in [
            vec!["encode", "--inflight", "0"],
            vec!["encode", "--inflight", "16"],
            vec!["encode", "--gpu-priority", "realtime"],
            vec!["encode", "--sync", "wait"],
            vec!["encode", "--compare-presets", "--compare-convert"],
        ] {
            assert!(Options::try_parse_from(args).is_err());
        }
        let unlimited =
            Options::try_parse_from(["encode", "--compare-convert", "--duration", "0"])?;
        assert!(unlimited.config(1920, 1080, Preset::P1).is_err());
        Ok(())
    }
    #[test]
    fn cli_validation() -> anyhow::Result<()> {
        assert_eq!(
            Options::try_parse_from(["encode"])?.rate_policy,
            RatePolicyArg::AbsoluteRamp
        );
        assert_eq!(
            Options::try_parse_from(["encode", "--rate-policy", "legacy"])?.rate_policy,
            RatePolicyArg::Legacy
        );
        let experimental = Options::try_parse_from([
            "encode",
            "--rate-policy",
            "absolute-ramp",
            "--min-bitrate",
            "0.5",
        ])?;
        assert_eq!(experimental.rate_policy, RatePolicyArg::AbsoluteRamp);
        assert_eq!(experimental.min_bitrate, Some(500_000));

        assert!(
            Options::try_parse_from(["encode"])?
                .encoder
                .requested()
                .is_none()
        );
        for name in ["nvenc", "amf", "qsv", "x264"] {
            assert!(
                Options::try_parse_from(["encode", "--encoder", name])?
                    .encoder
                    .requested()
                    .is_some()
            );
        }
        assert!(Options::try_parse_from(["encode", "--encoder", "unknown"]).is_err());
        let local = Options::try_parse_from(["encode", "--encoder", "openh264"])?;
        assert_eq!(
            local.encoder.requested(),
            Some(madscreen_encode::backend::BackendKind::OpenH264)
        );
        assert!(local.config(1920, 1080, Preset::default()).is_ok());
        for publish in [
            ["--transport", "cloudflare"],
            ["--livekit-url", "ws://127.0.0.1:17880"],
        ] {
            let blocked = Options::try_parse_from([
                "encode",
                "--encoder",
                "openh264",
                publish[0],
                publish[1],
            ])?;
            assert!(blocked.config(1920, 1080, Preset::default()).is_err());
        }
        let o = Options::try_parse_from([
            "encode",
            "--fps",
            "60000/1001",
            "--bitrate",
            "12.5",
            "--source",
            "window:Meu jogo",
            "--size",
            "1920x1080",
        ])?;
        assert_eq!(o.bitrate, 12_500_000);
        assert_eq!(o.fps.den, 1001);
        assert_eq!(o.source, Source::Window("Meu jogo".into()));
        o.config(1920, 1080, Preset::P1)?;
        for args in [
            vec!["encode", "--fps", "0"],
            vec!["encode", "--size", "1919x1080"],
            vec!["encode", "--source", "window:"],
            vec!["encode", "--bitrate", "NaN"],
            vec!["encode", "--vbv-frames", "0"],
            vec!["encode", "--stats-interval", "0"],
        ] {
            assert!(Options::try_parse_from(args).is_err());
        }
        let o = Options::try_parse_from(["encode", "--duration", "0", "--compare-presets"])?;
        assert!(o.config(1920, 1080, Preset::P1).is_err());
        Ok(())
    }
}
