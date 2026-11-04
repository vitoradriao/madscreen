//! Protocolo JSON em linhas entre o app (Tauri) e o engine de captura/encode (sidecar).
//!
//! O app escreve um `Command` por linha no stdin do engine e lê um `Event` por linha do stdout.
//! Cada sessão de compartilhamento é um processo: o engine encerra ao parar, o que devolve
//! GPU e memória ao sistema. Fim do stdin equivale a `Stop` (o app morreu).
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};

pub mod diagnostics;
pub mod privacy;
pub mod resolution;
pub mod source_identity;

pub use resolution::{EncoderLimits, OutputOption, OutputPlan, PlanError};

/// Versão do protocolo; o engine a anuncia em `Event::Ready`.
pub const PROTOCOL_VERSION: u32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("linha JSON inválida: {0}")]
    Json(#[from] serde_json::Error),
    #[error("falha de E/S no canal do engine: {0}")]
    Io(#[from] io::Error),
}

/// Perfil de qualidade da tela principal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quality {
    /// Altura da saída: 720, 1080, 1440 ou 2160; 0 = nativa da fonte.
    pub height: u32,
    /// Quadros por segundo: 30 ou 60.
    pub fps: u32,
}

impl Quality {
    pub fn is_supported(self) -> bool {
        matches!(self.height, 0 | 720 | 1080 | 1440 | 2160) && matches!(self.fps, 30 | 60)
    }
}

/// Origem da captura.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceSpec {
    /// Abre o seletor de captura do Windows (GraphicsCapturePicker).
    Picker,
    /// Índice base zero de `EnumDisplayMonitors`.
    Monitor { index: usize },
    /// Trecho do título da janela.
    Window { title: String },
    /// Janela exata, pelo HWND devolvido por `--list-sources` (sem ambiguidade de título).
    WindowHandle { hwnd: i64 },
    /// Janela selecionada com identidade do processo e criação, contra HWND reciclado.
    VerifiedWindow { hwnd: i64, instance_id: String },
}

/// Janela capturável listada pelo engine (`--list-sources`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_id: Option<u64>,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub instance_id: Option<String>,
    pub hwnd: i64,
    pub title: String,
    /// Nome do executável (ex.: `jogo.exe`).
    pub process: String,
    pub width: u32,
    pub height: u32,
    /// PNG em base64 (32x32); ausente se o ícone não pôde ser lido.
    pub icon_png_base64: Option<String>,
    /// PNG em base64 (miniatura); ausente se a janela não pôde ser copiada.
    pub thumbnail_png_base64: Option<String>,
}

/// Monitor listado pelo engine; `index` é o mesmo de `SourceSpec::Monitor`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorSource {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    pub thumbnail_png_base64: Option<String>,
}

/// Saída de `madscreen-encode-test --list-sources`: uma linha JSON e o processo sai.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceList {
    pub windows: Vec<WindowSource>,
    pub monitors: Vec<MonitorSource>,
    #[serde(default)]
    pub encoder: Option<EncoderLimits>,
}

/// Destino LiveKit; sem ele o engine captura e codifica sem publicar (útil para teste local).
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveKitTarget {
    pub url: String,
    pub room: String,
    pub identity: String,
    /// Token pronto; tem prioridade sobre o par de chaves de desenvolvimento.
    pub token: Option<String>,
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
}

impl std::fmt::Debug for LiveKitTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveKitTarget")
            .field("url", &self.url)
            .field("room", &self.room)
            .field("identity", &self.identity)
            .field("credenciais", &"<ocultas>")
            .finish()
    }
}

/// Destino Cloudflare Realtime (via Worker de sinalização). Sem `url`/`token` o engine usa
/// `MADSCREEN_CF_URL`/`MADSCREEN_CF_TOKEN` do ambiente ou de `%APPDATA%\madscreen\server.env`.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CloudflareTarget {
    pub url: Option<String>,
    /// Token de dono ou de convite `publisher` do Worker; nunca o App Secret do SFU.
    pub token: Option<String>,
    /// Canal escolhido no app; ausente em clientes antigos usa o canal padrão do dono.
    #[serde(default)]
    pub channel_id: Option<String>,
    /// Perfil H.264 anunciado no SDP (hex de 6 dígitos), para experimentos.
    pub profile_level_id: Option<String>,
}

impl std::fmt::Debug for CloudflareTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudflareTarget")
            .field("url", &self.url)
            .field("channel_id", &self.channel_id)
            .field("token", &"<oculto>")
            .field("profile_level_id", &self.profile_level_id)
            .finish()
    }
}

/// Áudio compartilhado junto com o vídeo (Opus 48 kHz estéreo na mesma sessão).
/// Sem este campo o engine não captura áudio.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioRequest {
    /// Som do PC todo, exceto a árvore de processos do madscreen. Tem prioridade sobre `apps`.
    #[serde(default)]
    pub system: bool,
    /// PIDs raiz dos aplicativos escolhidos (cada um com sua árvore de processos).
    #[serde(default)]
    pub apps: Vec<u32>,
    /// Captura o aplicativo dono da janela compartilhada (e a árvore dele).
    #[serde(default)]
    pub follow_window: bool,
    /// Raiz da árvore a excluir do som do PC (o app madscreen); sem valor usa o próprio engine.
    #[serde(default)]
    pub exclude_pid: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartRequest {
    pub source: SourceSpec,
    pub quality: Quality,
    /// Bitrate alvo em bps; sem valor calcula pela resolução efetiva e pelo FPS.
    pub bitrate_bps: Option<u32>,
    pub livekit: Option<LiveKitTarget>,
    /// Tem prioridade sobre `livekit`.
    #[serde(default)]
    pub cloudflare: Option<CloudflareTarget>,
    /// Áudio por aplicativo ou do PC todo, nos transportes Cloudflare e LiveKit.
    #[serde(default)]
    pub audio: Option<AudioRequest>,
}

/// App → engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
// Uma mensagem por sessao, fora do caminho de frame: o Box so complicaria o protocolo.
#[allow(clippy::large_enum_variant)]
pub enum Command {
    Start(StartRequest),
    Stop,
    /// Somente após Ready v4 e Started; geração pertence ao processo supervisor.
    Privacy(privacy::Request),
    /// Sonda opt-in do relogio do diagnostico; nao captura nem muda a midia.
    DiagnosticProbe {
        sequence: u32,
    },
}

/// Engine → app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Ack do engine após barreiras locais; não confirma apresentação remota.
    Privacy {
        operation: privacy::Operation,
        video: privacy::VideoState,
        audio_muted: bool,
    },
    /// Engine pronto para receber `Start`.
    Ready { version: u32, engine: String },
    /// Emitido somente em resposta a DiagnosticProbe. Ausencia e explicita.
    DiagnosticClock {
        sequence: u32,
        run_id: Option<String>,
        at_ms: Option<f64>,
    },
    /// O seletor do Windows está aberto.
    Picking,
    /// Situação do áudio logo após `Started`: fontes ativas e motivos das que falharam.
    Audio {
        active: u32,
        #[serde(default)]
        failed: Vec<String>,
    },
    /// O usuário fechou o seletor sem escolher nada.
    PickCancelled,
    /// Pipeline rodando.
    Started {
        source: String,
        /// Nome do codificador realmente inicializado; ausente em engines antigos.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoder: Option<String>,
        width: u32,
        height: u32,
        #[serde(default)]
        source_width: u32,
        #[serde(default)]
        source_height: u32,
        #[serde(default)]
        notice: Option<String>,
        #[serde(default)]
        level: Option<u32>,
        #[serde(default)]
        profile_level_id: Option<String>,
        fps_target: f64,
        bitrate_target_bps: u32,
        publishing: bool,
    },
    /// Amostra de 1 s.
    Status {
        fps: f64,
        bitrate_bps: u64,
        frames: u64,
        dropped: u64,
        /// Conexão de mídia observada pelo transporte. Ausente em engines antigos ou sem suporte.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        publishing_connected: Option<bool>,
        /// Contadores locais opcionais; ausentes em engines anteriores.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diagnostic_sample: Option<Box<diagnostics::PublisherSample>>,
    },
    /// Sessão encerrada; o processo sai logo depois.
    Stopped { reason: StopReason },
    Error {
        code: ErrorCode,
        message: String,
        /// Oferta, resposta e erro completo. Ausente em engines antigos e em falhas sem SDP.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diagnostic: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Requested,
    SourceClosed,
    AppGone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    Capture,
    Encode,
    Transport,
    Unsupported,
}

/// Texto copiável de uma falha de publicação. O `Display` é o resumo; o SDP fica só em `body`.
#[derive(Debug)]
pub struct ShareDiagnostic {
    pub summary: String,
    pub body: String,
}

impl std::fmt::Display for ShareDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.summary)
    }
}

impl std::error::Error for ShareDiagnostic {}

pub fn write_line<T: Serialize>(out: &mut impl Write, value: &T) -> Result<(), IpcError> {
    serde_json::to_writer(&mut *out, value)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

/// Lê a próxima linha não vazia; `Ok(None)` no fim do fluxo.
pub fn read_line<T: for<'de> Deserialize<'de>>(
    input: &mut impl BufRead,
) -> Result<Option<T>, IpcError> {
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if !line.trim().is_empty() {
            return Ok(Some(serde_json::from_str(line.trim())?));
        }
    }
}

#[cfg(test)]
mod encoder_tests {
    use super::*;
    #[test]
    fn diagnostic_probe_is_explicit_and_reports_unavailable_clock() -> Result<(), serde_json::Error>
    {
        let probe: Command = serde_json::from_str(r#"{"cmd":"diagnostic_probe","sequence":7}"#)?;
        assert_eq!(probe, Command::DiagnosticProbe { sequence: 7 });
        assert!(
            serde_json::from_str::<Command>(r#"{"cmd":"diagnostic_probe","sequence":-1}"#).is_err()
        );
        let absent = Event::DiagnosticClock {
            sequence: 7,
            run_id: None,
            at_ms: None,
        };
        assert_eq!(
            serde_json::to_value(absent)?["at_ms"],
            serde_json::Value::Null
        );
        Ok(())
    }
    #[test]
    fn started_preserves_active_encoder_and_accepts_older_engines() -> Result<(), serde_json::Error>
    {
        let mut json = serde_json::json!({"event":"started", "source":"monitor:0", "width":1280, "height":720, "fps_target":60.0, "bitrate_target_bps":8_000_000, "publishing":true});
        let legacy: Event = serde_json::from_value(json.clone())?;
        assert!(matches!(legacy, Event::Started { encoder: None, .. }));
        for name in ["NVIDIA NVENC", "AMD AMF", "Intel QuickSync", "CPU (x264)"] {
            json["encoder"] = name.into();
            let event: Event = serde_json::from_value(json.clone())?;
            let encoded = serde_json::to_value(&event)?;
            assert_eq!(encoded["encoder"], name);
            assert!(matches!(event, Event::Started { encoder: Some(value), .. } if value == name));
        }
        let legacy_error: Event = serde_json::from_value(serde_json::json!({
            "event": "error",
            "code": "transport",
            "message": "falha na publicação"
        }))?;
        assert!(matches!(
            legacy_error,
            Event::Error {
                diagnostic: None,
                ..
            }
        ));
        Ok(())
    }
}
