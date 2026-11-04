//! Recovery is a new interface/session, never seamless media continuity.
#[cfg(any(windows, test))]
use std::sync::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(any(windows, test))]
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct RendererRecovery {
    pending: Arc<AtomicBool>,
    #[cfg(any(windows, test))]
    last_reload: Arc<Mutex<Option<Instant>>>,
}

impl RendererRecovery {
    #[cfg(any(windows, test))]
    pub fn request(&self, now: Instant) -> bool {
        let mut last = self
            .last_reload
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if last.is_some_and(|previous| {
            now.saturating_duration_since(previous) < Duration::from_secs(10)
        }) {
            return false;
        }
        *last = Some(now);
        self.pending.store(true, Ordering::Release);
        true
    }

    pub fn take(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notice_is_consumed_once_and_repeated_crashes_do_not_reload_in_a_loop() {
        let state = RendererRecovery::default();
        let now = Instant::now();
        assert!(!state.take());
        assert!(state.request(now));
        assert!(state.take());
        assert!(!state.take());
        assert!(!state.request(now + Duration::from_secs(1)));
        assert!(!state.take());
        assert!(state.request(now + Duration::from_secs(11)));
        assert!(state.clone().take());
        assert!(!state.take());
    }
}
