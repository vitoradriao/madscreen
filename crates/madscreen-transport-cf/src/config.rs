use std::{collections::HashMap, env, fmt, fs, path::Path};

use crate::{Error, Result};

/// Destino do Worker; o token é de dono/publicador, nunca o App Secret do SFU.
#[derive(Clone)]
pub struct CfPublishConfig {
    pub worker_url: String,
    pub token: String,
    pub channel_id: Option<String>,
    pub direct_session_id: Option<String>,
    pub track_name: String,
    pub width: u32,
    pub height: u32,
    pub max_bitrate_bps: u64,
    pub max_fps: f64,
    pub h264_profile_level_id: Option<String>,
    /// Nível H.264 do encoder. Sem `h264_profile_level_id`, o SDP conserva o perfil
    /// oferecido pelo libwebrtc e só troca o nível.
    pub h264_level_idc: Option<u32>,
    pub preferred_h264_profile: Option<String>,
    pub only_h264: bool,
    pub keepalive: bool,
    /// Estimativa inicial do GCC; `None` usa metade do teto (o padrão do libwebrtc começa em ~0,3 Mbps
    /// e leva ~10 s para chegar a 18 Mbps).
    pub start_bitrate_bps: Option<u64>,
    /// Publica também uma faixa de áudio Opus 48 kHz estéreo na mesma sessão.
    pub audio: bool,
    /// Laboratório: oferece só AV1 e entrega AUs AV1 do encoder. Sem fallback H.264;
    /// nunca ligado pelo app.
    pub lab_av1: bool,
}

impl Default for CfPublishConfig {
    fn default() -> Self {
        Self {
            worker_url: String::new(),
            token: String::new(),
            channel_id: None,
            direct_session_id: None,
            track_name: "screen".into(),
            width: 1920,
            height: 1080,
            max_bitrate_bps: 18_000_000,
            max_fps: 60.,
            h264_profile_level_id: None,
            h264_level_idc: None,
            preferred_h264_profile: None,
            only_h264: true,
            keepalive: false,
            start_bitrate_bps: None,
            audio: false,
            lab_av1: false,
        }
    }
}

impl fmt::Debug for CfPublishConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CfPublishConfig")
            .field("track_name", &self.track_name)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("max_bitrate_bps", &self.max_bitrate_bps)
            .field("max_fps", &self.max_fps)
            .field("h264_profile_level_id", &self.h264_profile_level_id)
            .field("h264_level_idc", &self.h264_level_idc)
            .field("only_h264", &self.only_h264)
            .field("keepalive", &self.keepalive)
            .field("audio", &self.audio)
            .finish_non_exhaustive()
    }
}

/// Lê CHAVE=VALOR, ignorando comentários e linhas vazias; não altera o ambiente.
/// Arquivo ausente/ilegível resulta em mapa vazio. Aspas externas são opcionais.
pub fn load_server_env(path: impl AsRef<Path>) -> HashMap<String, String> {
    let Ok(contents) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

/// Prioridade: argumentos explícitos, ambiente, MADSCREEN_HOME/server.env,
/// APPDATA/madscreen/server.env. Nunca escreve nem registra credenciais.
pub fn resolve_credentials(url: Option<&str>, token: Option<&str>) -> Result<(String, String)> {
    let path = env::var_os("MADSCREEN_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| env::var_os("APPDATA").map(|p| Path::new(&p).join("madscreen")))
        .map(|p| p.join("server.env"));
    let values = path.map(load_server_env).unwrap_or_default();
    let get = |explicit: Option<&str>, key: &'static str| {
        explicit
            .filter(|v| !v.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| env::var(key).ok().filter(|v| !v.trim().is_empty()))
            .or_else(|| values.get(key).filter(|v| !v.trim().is_empty()).cloned())
            .ok_or(Error::Config(key))
    };
    Ok((
        get(url, "MADSCREEN_CF_URL")?,
        get(token, "MADSCREEN_CF_TOKEN")?,
    ))
}
