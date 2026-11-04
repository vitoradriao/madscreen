//! Importa login/preferencias da edicao antiga antes de o WebView2 abrir o banco de localStorage.
//! Nunca sobrescreve o perfil atual nem remove o original. O instalador encerra a edicao antiga.
use std::{fs, io, path::Path};

fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let destination = target.join(entry.file_name());
        if kind.is_dir() {
            fs::create_dir(&destination)?;
            copy_tree(&entry.path(), &destination)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), destination)?;
        } else {
            return Err(io::Error::other(
                "o perfil antigo contem um link ou arquivo especial",
            ));
        }
    }
    Ok(())
}

fn import_directory(source: &Path, target: &Path) -> io::Result<bool> {
    if target.exists() || !source.is_dir() {
        return Ok(false);
    }
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("pasta de destino invalida"))?;
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".profile-import-")
        .tempdir_in(parent)?;
    copy_tree(source, staging.path())?;
    // Publica o banco completo de uma vez. Uma copia interrompida nunca vira perfil ativo.
    fs::rename(staging.path(), target)?;
    Ok(true)
}

#[cfg(windows)]
pub fn migrate(app: &tauri::AppHandle) -> io::Result<()> {
    use tauri::Manager;
    let local = app.path().local_data_dir().map_err(io::Error::other)?;
    let current = app.path().app_local_data_dir().map_err(io::Error::other)?;
    let storage = Path::new("EBWebView").join("Default").join("Local Storage");
    if import_directory(
        &local.join("app.madscreen.viewer").join(&storage),
        &current.join(&storage),
    )? {
        tracing::info!("login e preferencias locais importados da edicao antiga");
    }
    let config = app.path().app_config_dir().map_err(io::Error::other)?;
    let settings = config.join("settings.json");
    if !settings.exists() {
        let previous = app
            .path()
            .config_dir()
            .map_err(io::Error::other)?
            .join("app.madscreen.viewer")
            .join("settings.json");
        if previous.is_file() {
            // Desserializar descarta campos antigos de credenciais LiveKit.
            let value: crate::settings::Settings = serde_json::from_slice(&fs::read(previous)?)?;
            fs::create_dir_all(config)?;
            fs::write(settings, serde_json::to_vec_pretty(&value)?)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_complete_database_once_without_changing_original() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("previous");
        let target = root.path().join("current").join("Local Storage");
        fs::create_dir_all(source.join("leveldb"))?;
        fs::write(
            source.join("leveldb").join("000001.log"),
            b"session-and-preferences",
        )?;
        assert!(import_directory(&source, &target)?);
        assert_eq!(
            fs::read(target.join("leveldb").join("000001.log"))?,
            b"session-and-preferences"
        );
        fs::write(source.join("leveldb").join("000001.log"), b"old-session")?;
        assert!(!import_directory(&source, &target)?);
        assert_eq!(
            fs::read(target.join("leveldb").join("000001.log"))?,
            b"session-and-preferences"
        );
        assert_eq!(
            fs::read(source.join("leveldb").join("000001.log"))?,
            b"old-session"
        );
        Ok(())
    }

    #[test]
    fn fresh_install_does_not_create_empty_database() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let target = root.path().join("current");
        assert!(!import_directory(&root.path().join("absent"), &target)?);
        assert!(!target.exists());
        Ok(())
    }
}
