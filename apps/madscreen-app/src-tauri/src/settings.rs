//! Configurações do app em JSON na pasta de configuração do usuário.
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Último perfil escolhido na tela principal.
    pub quality_height: u32,
    pub quality_fps: u32,
    /// O usuário escolheu o FPS no seletor; sem isso, PC sem NVENC começa em 30 FPS.
    pub quality_fps_chosen: bool,
    /// Contorno para freezes no decoder acelerado do WebView2. Requer reinício.
    pub software_video_decode: bool,
    pub tray_notice_seen: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            quality_height: 1080,
            quality_fps: 60,
            quality_fps_chosen: false,
            software_video_decode: false,
            tray_notice_seen: false,
        }
    }
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("pasta de configuração indisponível: {e}"))?;
    fs::create_dir_all(&dir)
        .map_err(|e| format!("não foi possível criar {}: {e}", dir.display()))?;
    Ok(dir.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    path(app)
        .ok()
        .and_then(|p| fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path(app)?, json).map_err(|e| format!("falha ao gravar configurações: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_preferences_keep_hardware_decode_until_opt_in() -> Result<(), serde_json::Error> {
        let old: Settings = serde_json::from_str(r#"{"quality_height":720,"quality_fps":30}"#)?;
        assert!(!old.software_video_decode);
        // O 60 FPS salvo por versões anteriores era só o último usado, não escolha explícita.
        assert!(!old.quality_fps_chosen);
        assert_eq!((old.quality_height, old.quality_fps), (720, 30));
        let enabled = Settings {
            software_video_decode: true,
            ..old
        };
        let restored: Settings = serde_json::from_slice(&serde_json::to_vec(&enabled)?)?;
        assert!(restored.software_video_decode);
        assert_eq!((restored.quality_height, restored.quality_fps), (720, 30));
        Ok(())
    }
}
