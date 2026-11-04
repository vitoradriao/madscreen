//! Tickets locais de seleção, invalidados por destruição mesmo se HWND/PID/título repetirem.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager};

#[derive(Default)]
pub struct Registry {
    pub ready: bool,
    pub synchronizer: Option<std::sync::mpsc::SyncSender<std::sync::mpsc::Sender<()>>>,
    epoch: u64,
    floor_epoch: u64,
    destroyed: HashMap<i64, u64>,
    next: u64,
    windows: HashMap<i64, u64>,
}
#[derive(Default, Clone)]
pub struct WindowGuard(pub Arc<Mutex<Registry>>);

impl Registry {
    #[cfg(any(windows, test))]
    pub fn destroyed(&mut self, hwnd: i64) {
        self.epoch = self.epoch.wrapping_add(1);
        self.windows.remove(&hwnd);
        if self.destroyed.len() >= 4096 {
            self.destroyed.clear();
            self.windows.clear();
            self.floor_epoch = self.epoch;
        }
        self.destroyed.insert(hwnd, self.epoch);
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn bind(&mut self, hwnd: i64, epoch: u64) -> Option<u64> {
        if !self.ready
            || epoch < self.floor_epoch
            || epoch > self.epoch
            || self
                .destroyed
                .get(&hwnd)
                .is_some_and(|destroyed| *destroyed > epoch)
        {
            return None;
        }
        if let Some(ticket) = self.windows.get(&hwnd) {
            return Some(*ticket);
        }
        // Tickets são números inteiros exatos no JavaScript. Esgotamento falha fechado.
        self.next = self.next.checked_add(1)?;
        if self.next > 9_007_199_254_740_991 {
            self.ready = false;
            return None;
        }
        self.windows.insert(hwnd, self.next);
        Some(self.next)
    }
    pub fn valid(&self, hwnd: i64, ticket: u64) -> bool {
        self.ready && self.windows.get(&hwnd) == Some(&ticket)
    }
}

pub fn validate(
    app: &AppHandle,
    source: &Option<madscreen_ipc::SourceSpec>,
    ticket: Option<u64>,
) -> Result<(), String> {
    if matches!(source, Some(madscreen_ipc::SourceSpec::WindowHandle { .. })) {
        return Err("A janela não possui identidade verificável. Use o seletor do Windows.".into());
    }
    if let Some(madscreen_ipc::SourceSpec::VerifiedWindow { hwnd, .. }) = source {
        synchronize(app)?;
        let state = app.state::<WindowGuard>();
        let valid = state
            .0
            .lock()
            .map_err(|_| "Não foi possível conferir a seleção.")?
            .valid(
                *hwnd,
                ticket.ok_or("Seleção expirada. Escolha a janela novamente.")?,
            );
        if !valid {
            return Err("A janela foi encerrada ou recriada. Escolha a fonte novamente.".into());
        }
    }
    Ok(())
}

/// Esvazia eventos já pendentes na thread do observador antes de usar o ticket.
pub fn synchronize(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<WindowGuard>();
    let sender = state
        .0
        .lock()
        .map_err(|_| "Observador indisponível.")?
        .synchronizer
        .clone()
        .ok_or("Observador indisponível. Use o seletor do Windows.")?;
    let (ack, reply) = std::sync::mpsc::channel();
    sender
        .try_send(ack)
        .map_err(|_| "Aguarde a atualização da lista de janelas.")?;
    reply
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| "O observador de janelas não respondeu. Escolha novamente.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_recreated_handle_never_inherits_ticket_and_unobserved_windows_fail_closed()
    -> Result<(), String> {
        let mut state = Registry::default();
        assert_eq!(state.bind(10, 0), None);
        state.ready = true;
        let first = state.bind(10, 0).ok_or("ticket ausente")?;
        assert!(state.valid(10, first));
        state.destroyed(10);
        assert!(!state.valid(10, first));
        assert_eq!(state.bind(10, 0), None); // enumeração anterior à destruição
        let second = state
            .bind(10, state.epoch())
            .ok_or("segundo ticket ausente")?;
        assert_ne!(first, second);
        assert!(!state.valid(10, first));
        assert!(state.valid(10, second));
        state.destroyed(999);
        assert_eq!(state.bind(10, state.epoch() - 1), Some(second));
        Ok(())
    }
}
