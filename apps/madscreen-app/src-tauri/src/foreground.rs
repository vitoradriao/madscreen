use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

#[derive(Clone, Serialize)]
pub struct ForegroundWindow {
    pub hwnd: i64,
    pub instance_id: String,
}
#[derive(Default)]
pub struct Foreground(pub Arc<Mutex<Option<ForegroundWindow>>>);

pub fn start(app: &AppHandle) {
    #[cfg(windows)]
    {
        if let Ok(mut slot) = app.state::<Foreground>().0.lock() {
            observe(&mut slot, crate::foreground_sys::current());
        }
        let weak = Arc::downgrade(&app.state::<Foreground>().0);
        let guards = Arc::downgrade(&app.state::<crate::window_guard::WindowGuard>().0);
        let engine = app.state::<crate::engine::Engine>().inner().clone();
        std::thread::spawn(move || crate::foreground_sys::watch(weak, guards, engine));
        // Instalar a observação antes de construir/focar a janela principal.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while std::time::Instant::now() < deadline {
            if app
                .state::<crate::window_guard::WindowGuard>()
                .0
                .lock()
                .is_ok_and(|state| state.ready)
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

#[tauri::command]
pub fn get_foreground_suggestion(app: AppHandle) -> Option<ForegroundWindow> {
    crate::window_guard::synchronize(&app).ok()?;
    app.state::<Foreground>()
        .0
        .lock()
        .ok()
        .and_then(|v| v.clone())
}

#[cfg(any(windows, test))]
pub enum Observation {
    Own,
    External(Option<ForegroundWindow>),
}

#[cfg(any(windows, test))]
pub(crate) fn observe(slot: &mut Option<ForegroundWindow>, observation: Observation) {
    if let Observation::External(window) = observation {
        *slot = window;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn own_focus_preserves_last_external_but_invalid_external_clears_it() {
        let mut slot = None;
        observe(&mut slot, Observation::Own);
        assert!(slot.is_none());
        observe(
            &mut slot,
            Observation::External(Some(ForegroundWindow {
                hwnd: 1,
                instance_id: "opaque".into(),
            })),
        );
        observe(&mut slot, Observation::Own);
        assert_eq!(slot.as_ref().map(|w| w.hwnd), Some(1));
        observe(&mut slot, Observation::External(None));
        observe(&mut slot, Observation::Own);
        assert!(slot.is_none());
    }
}
