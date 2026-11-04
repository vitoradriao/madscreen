//! Controle portátil do anel; nenhuma fila cresce no caminho de frames.
use crate::{EncodeError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum GpuPriority {
    Normal,
    AboveNormal,
    High,
}
impl GpuPriority {
    pub fn thread_priority(self) -> i32 {
        match self {
            Self::Normal => 0,
            Self::AboveNormal => 3,
            Self::High => 7,
        }
    }
    pub fn process_class(self) -> u32 {
        match self {
            Self::Normal => 2,
            Self::AboveNormal => 3,
            Self::High => 4,
        }
    }
}
/// Slots reservados incluem conversão, encode e leitura diferida das queries.
/// Lotação descarta a oportunidade nova; a captura continua latest-wins.
pub struct FlightSlots {
    busy: Box<[bool]>,
    limit: usize,
    count: usize,
    next: usize,
}
impl FlightSlots {
    pub fn new(slots: usize, limit: usize) -> Result<Self> {
        if limit == 0 || slots <= limit {
            return Err(EncodeError::InvalidConfig(
                "anel exige pelo menos inflight + 1 slots",
            ));
        }
        Ok(Self {
            busy: vec![false; slots].into_boxed_slice(),
            limit,
            count: 0,
            next: 0,
        })
    }
    pub fn reserve(&mut self) -> Option<usize> {
        if self.count == self.limit {
            return None;
        }
        for offset in 0..self.busy.len() {
            let slot = (self.next + offset) % self.busy.len();
            if !self.busy[slot] {
                self.busy[slot] = true;
                self.count += 1;
                self.next = (slot + 1) % self.busy.len();
                return Some(slot);
            }
        }
        None
    }
    pub fn release(&mut self, slot: usize) -> Result<()> {
        let busy = self
            .busy
            .get_mut(slot)
            .ok_or(EncodeError::InvalidConfig("slot inválido"))?;
        if !*busy {
            return Err(EncodeError::InvalidConfig("slot já liberado"));
        }
        *busy = false;
        self.count -= 1;
        Ok(())
    }
    pub fn count(&self) -> usize {
        self.count
    }
}

/// Observed time without a slot while work is due; never a lost-frame count.
#[derive(Default)]
pub struct Saturation {
    episodes: u64,
    ticks: u64,
    blocked_since: Option<i64>,
}
impl Saturation {
    pub fn observe(&mut self, now: i64, blocked: bool) {
        match (self.blocked_since, blocked) {
            (None, true) => {
                self.episodes = self.episodes.saturating_add(1);
                self.blocked_since = Some(now);
            }
            (Some(start), false) => {
                self.ticks = self
                    .ticks
                    .saturating_add(now.saturating_sub(start).max(0) as u64);
                self.blocked_since = None;
            }
            _ => {}
        }
    }
    pub fn snapshot(&self, now: i64, frequency: i64) -> (u64, f64) {
        let pending = self
            .blocked_since
            .map_or(0, |start| now.saturating_sub(start).max(0) as u64);
        (
            self.episodes,
            self.ticks.saturating_add(pending) as f64 * 1000. / frequency.max(1) as f64,
        )
    }
}
#[cfg(test)]
mod saturation_tests {
    use super::Saturation;
    #[test]
    fn polling_does_not_count_frames_or_duplicate_episodes() {
        for step in [1, 5, 10] {
            let mut metric = Saturation::default();
            for now in (10..100).step_by(step) {
                metric.observe(now, true);
            }
            assert_eq!(metric.snapshot(100, 1000), (1, 90.));
            metric.observe(100, false);
            metric.observe(200, false);
            assert_eq!(metric.snapshot(300, 1000), (1, 90.));
            metric.observe(300, true);
            metric.observe(320, false);
            assert_eq!(metric.snapshot(400, 1000), (2, 110.));
        }
    }
    #[test]
    fn reset_drops_previous_generation_and_warmup() {
        let mut metric = Saturation::default();
        metric.observe(0, true);
        metric = Saturation::default();
        assert_eq!(metric.snapshot(100, 1000), (0, 0.));
        metric.observe(100, true);
        assert_eq!(metric.snapshot(110, 1000), (1, 10.));
    }
}

/// A completed AU may proceed with explicitly absent optional telemetry.
/// Device health is checked independently by the runner; legacy keeps query errors.
pub fn resolve_gpu_sample<T, E>(
    result: std::result::Result<Option<Option<T>>, E>,
    independent: bool,
) -> std::result::Result<Option<Option<T>>, E> {
    match result {
        Ok(None) | Err(_) if independent => Ok(Some(None)),
        other => other,
    }
}

#[cfg(test)]
mod query_completion_tests {
    use super::{FlightSlots, resolve_gpu_sample};
    #[test]
    fn completed_media_does_not_depend_on_pending_disjoint_or_failed_queries() -> crate::Result<()>
    {
        for sample in [Ok(None), Ok(Some(None)), Err("query error")] {
            let mut slots = FlightSlots::new(4, 3)?;
            let slot = slots.reserve().ok_or(crate::EncodeError::Busy)?;
            let observed = resolve_gpu_sample::<(f64, f64), _>(sample, true);
            assert_eq!(observed, Ok(Some(None)));
            // Only after encode completion: telemetry can be absent, not synthetic zero.
            slots.release(slot)?;
            assert_eq!(slots.count(), 0);
        }
        assert_eq!(
            resolve_gpu_sample::<u8, _>(Ok::<_, &str>(None), false),
            Ok(None)
        );
        assert_eq!(
            resolve_gpu_sample::<u8, _>(Err("query error"), false),
            Err("query error")
        );
        assert_eq!(
            resolve_gpu_sample::<u8, &str>(Ok(Some(Some(7))), true),
            Ok(Some(Some(7)))
        );
        Ok(())
    }
}

/// Intervalo mínimo da captura: atualizações extras ficam na mailbox latest-wins.
pub fn capture_due(now: i64, last: Option<i64>, frequency: i64, fps: crate::FrameRate) -> bool {
    last.is_none_or(|last| {
        i128::from(now.saturating_sub(last)) * i128::from(fps.num) * 10
            >= i128::from(frequency) * i128::from(fps.den) * 9
    })
}

/// Média temporal de ocupação, sem favorecer iterações que fazem polling frequente.
pub struct Occupancy {
    start: i64,
    last: i64,
    count: usize,
    area: f64,
    maximum: usize,
}
impl Occupancy {
    pub fn new(now: i64) -> Self {
        Self {
            start: now,
            last: now,
            count: 0,
            area: 0.,
            maximum: 0,
        }
    }
    pub fn observe(&mut self, now: i64, count: usize) {
        self.area += now.saturating_sub(self.last).max(0) as f64 * self.count as f64;
        self.last = now;
        self.count = count;
        self.maximum = self.maximum.max(count);
    }
    pub fn mean(&self) -> f64 {
        if self.last > self.start {
            self.area / (self.last - self.start) as f64
        } else {
            0.
        }
    }
    pub fn max(&self) -> usize {
        self.maximum
    }
}

/// FIFO limitada: reservar no início e rejeitar lotação antes que VecDeque cresça.
pub struct PendingQueue<T> {
    entries: std::collections::VecDeque<T>,
    limit: usize,
}
impl<T> PendingQueue<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: std::collections::VecDeque::with_capacity(limit),
            limit,
        }
    }
    pub fn push_back(&mut self, value: T) -> Result<()> {
        if self.entries.len() == self.limit {
            return Err(EncodeError::Busy);
        }
        self.entries.push_back(value);
        Ok(())
    }
    pub fn push_front(&mut self, value: T) -> Result<()> {
        if self.entries.len() == self.limit {
            return Err(EncodeError::Busy);
        }
        self.entries.push_front(value);
        Ok(())
    }
    pub fn front(&self) -> Option<&T> {
        self.entries.front()
    }
    pub fn pop_front(&mut self) -> Option<T> {
        self.entries.pop_front()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn occupancy_is_weighted_by_time() {
        let mut occupancy = Occupancy::new(0);
        occupancy.observe(0, 3);
        occupancy.observe(10, 1);
        occupancy.observe(20, 0);
        assert_eq!(occupancy.mean(), 2.);
        assert_eq!(occupancy.max(), 3);
    }
    #[test]
    fn bounded_slots_remain_owned_until_completion() -> Result<()> {
        let mut slots = FlightSlots::new(4, 3)?;
        assert_eq!(slots.reserve(), Some(0));
        assert_eq!(slots.reserve(), Some(1));
        assert_eq!(slots.reserve(), Some(2));
        assert_eq!(slots.reserve(), None);
        slots.release(1)?;
        assert_eq!(slots.reserve(), Some(3));
        assert!(slots.release(1).is_err());
        assert_eq!(slots.count(), 3);
        assert!(FlightSlots::new(3, 3).is_err());
        Ok(())
    }
    #[test]
    fn priorities_never_select_realtime_and_capture_has_minimum_interval() -> Result<()> {
        for (priority, thread, class) in [
            (GpuPriority::Normal, 0, 2),
            (GpuPriority::AboveNormal, 3, 3),
            (GpuPriority::High, 7, 4),
        ] {
            assert_eq!(priority.thread_priority(), thread);
            assert_eq!(priority.process_class(), class);
        }
        let fps = crate::FrameRate::new(100, 1)?;
        assert!(capture_due(0, None, 1000, fps));
        assert!(!capture_due(8, Some(0), 1000, fps));
        assert!(capture_due(9, Some(0), 1000, fps));
        Ok(())
    }
}
