//! Lista janelas e monitores capturáveis pedindo ao engine (`--list-sources`).
use madscreen_ipc::SourceList;
use std::process::{Command, Stdio};

pub(crate) fn list_blocking(monitors_only: bool) -> Result<SourceList, String> {
    let exe = crate::engine::locate_engine()?;
    let mut command = Command::new(&exe);
    command.arg("--list-sources");
    if monitors_only {
        // Só telas e limites do encoder: rápido, sem miniaturas.
        command.arg("--monitors-only");
    }
    command
        .arg("--exclude-pid")
        .arg(std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: sem console piscando ao listar.
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .output()
        .map_err(|e| format!("falha ao listar as fontes ({}): {e}", exe.display()))?;
    if !output.status.success() {
        return Err("o engine não conseguiu listar as janelas e telas".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("lista de fontes inválida: {e}"))
}

pub async fn list() -> Result<SourceList, String> {
    tauri::async_runtime::spawn_blocking(|| list_blocking(false))
        .await
        .map_err(|e| format!("falha interna: {e}"))?
}

/// Telas e limites reais do encoder, sem janelas nem miniaturas.
pub async fn list_monitors() -> Result<SourceList, String> {
    tauri::async_runtime::spawn_blocking(|| list_blocking(true))
        .await
        .map_err(|e| format!("falha interna: {e}"))?
}
