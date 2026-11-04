//! Costura entre a logica portatil e o Windows; fora do Windows tudo e "nao suportado".

#[cfg(windows)]
pub(crate) use crate::win::{
    HighResTimer, Loopback, list_audio_apps, raise_thread_priority, resolve_root_pid,
    window_process_id,
};

#[cfg(not(windows))]
mod fallback {
    use std::sync::Arc;

    use crate::{AudioApp, AudioError, AudioSourceSpec, mixer::Ring};

    pub(crate) struct HighResTimer;
    impl HighResTimer {
        pub(crate) fn begin() -> Self {
            Self
        }
    }
    pub(crate) fn raise_thread_priority() {}

    pub(crate) struct Loopback;
    impl Loopback {
        pub(crate) fn start(_spec: AudioSourceSpec, _ring: Arc<Ring>) -> Result<Self, AudioError> {
            Err(AudioError::Unsupported)
        }
    }

    pub(crate) fn list_audio_apps(_exclude_pid: Option<u32>) -> Result<Vec<AudioApp>, AudioError> {
        Err(AudioError::Unsupported)
    }
    pub(crate) fn resolve_root_pid(pid: u32) -> u32 {
        pid
    }
    pub(crate) fn window_process_id(_hwnd: isize) -> Option<u32> {
        None
    }
}

#[cfg(not(windows))]
pub(crate) use fallback::{
    HighResTimer, Loopback, list_audio_apps, raise_thread_priority, resolve_root_pid,
    window_process_id,
};
