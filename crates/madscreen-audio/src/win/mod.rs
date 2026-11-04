//! Implementacao Windows: process loopback, sessoes de audio e tabela de processos.

pub(crate) mod loopback_ffi;
mod process_ffi;
mod sessions_ffi;
mod system_ffi;

use crate::{AudioApp, AudioError, procs};

pub(crate) use loopback_ffi::Loopback;
pub(crate) use system_ffi::{HighResTimer, raise_thread_priority};

pub(crate) fn list_audio_apps(exclude_pid: Option<u32>) -> Result<Vec<AudioApp>, AudioError> {
    let sessions = sessions_ffi::enumerate_sessions().map_err(AudioError::System)?;
    let table = process_ffi::snapshot();
    let mut apps = procs::group_apps(&table, &sessions, exclude_pid);
    for app in &mut apps {
        if let Some(path) = process_ffi::image_path(app.pid) {
            if let Some(description) = process_ffi::file_description(&path) {
                // Sessao sem nome proprio usa o nome do produto (ex.: "Google Chrome").
                if app
                    .display_name
                    .eq_ignore_ascii_case(stem(&app.process_name))
                {
                    app.display_name = description;
                }
            }
        }
    }
    Ok(apps)
}

fn stem(exe: &str) -> &str {
    exe.rsplit_once('.').map_or(exe, |(stem, _)| stem)
}

pub(crate) fn resolve_root_pid(pid: u32) -> u32 {
    procs::root_pid(&process_ffi::snapshot(), pid)
}

pub(crate) fn window_process_id(hwnd: isize) -> Option<u32> {
    process_ffi::window_process_id(hwnd)
}
