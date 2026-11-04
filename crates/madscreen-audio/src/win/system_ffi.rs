#![allow(unsafe_code)]
//! Resolucao de timer e prioridade de thread.

use windows::Win32::{
    Media::{timeBeginPeriod, timeEndPeriod},
    System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL},
};

/// Pede timer de 1 ms enquanto a guarda existir (o sleep padrao do Windows e ~15 ms).
pub(crate) struct HighResTimer(bool);

impl HighResTimer {
    pub(crate) fn begin() -> Self {
        // SAFETY: chamada simples sem ponteiros; o par com timeEndPeriod esta em Drop.
        Self(unsafe { timeBeginPeriod(1) } == 0)
    }
}

impl Drop for HighResTimer {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: par do timeBeginPeriod(1) bem-sucedido em `begin`.
            unsafe { timeEndPeriod(1) };
        }
    }
}

/// Acima do normal, nunca tempo real: o jogo vem primeiro.
pub(crate) fn raise_thread_priority() {
    // SAFETY: o pseudo-handle da thread atual e sempre valido.
    if let Err(error) =
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL) }
    {
        tracing::debug!(%error, "não foi possível elevar a prioridade da thread de áudio");
    }
}
