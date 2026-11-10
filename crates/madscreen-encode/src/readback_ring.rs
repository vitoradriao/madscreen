//! Ordering and staging-slot ownership for optional delayed readback.
//! The caller owns D3D11 textures and performs Map/Unmap; this module has no FFI.
use std::time::Duration;

/// A quiet capture source must not leave its final frame staged indefinitely.
pub fn tail_due(age: Duration, frame_period: Duration) -> bool {
    age >= frame_period
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingError {
    InvalidCapacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapMode {
    /// Probe with D3D11_MAP_FLAG_DO_NOT_WAIT. Keep the head on STILL_DRAWING.
    NonBlocking,
    /// A full ring or end-of-stream must make progress before accepting more frames.
    Blocking,
}

pub struct ReadbackRing<T> {
    slots: Vec<Option<T>>,
    head: usize,
    len: usize,
}

impl<T> ReadbackRing<T> {
    pub fn new(capacity: usize) -> Result<Self, RingError> {
        if capacity < 2 {
            return Err(RingError::InvalidCapacity);
        }
        Ok(Self {
            slots: std::iter::repeat_with(|| None).take(capacity).collect(),
            head: 0,
            len: 0,
        })
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    pub fn next_slot(&self) -> Option<usize> {
        (self.len < self.capacity()).then(|| (self.head + self.len) % self.capacity())
    }

    /// Returns the staging slot to copy into. On full, returns ownership to caller.
    pub fn push(&mut self, pending: T) -> Result<usize, T> {
        if self.len == self.capacity() {
            return Err(pending);
        }
        let slot = (self.head + self.len) % self.capacity();
        self.slots[slot] = Some(pending);
        self.len += 1;
        Ok(slot)
    }

    /// Only the oldest frame can be mapped/sent. A full ring forces blocking
    /// fallback so that a permanently busy head cannot stall every later frame.
    pub fn map_head(&self, draining: bool) -> Option<(usize, &T, MapMode)> {
        let pending = self.slots[self.head].as_ref()?;
        let mode = if draining || self.len == self.capacity() {
            MapMode::Blocking
        } else {
            MapMode::NonBlocking
        };
        Some((self.head, pending, mode))
    }

    /// Call only after Map, copy and codec_send all succeed. On STILL_DRAWING
    /// or encoder Busy, leave the head in place and retry before later frames.
    pub fn complete_head(&mut self) -> Option<T> {
        let completed = self.slots[self.head].take()?;
        self.head = (self.head + 1) % self.capacity();
        self.len -= 1;
        Some(completed)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct FakeFrame {
        index: u64,
        capture_qpc: i64,
    }

    #[test]
    fn preserves_order_timestamps_and_skipped_input_index() {
        let mut ring = ReadbackRing::new(3).unwrap();
        for index in [10, 12, 13] {
            ring.push(FakeFrame {
                index,
                capture_qpc: (index * 100) as i64,
            })
            .unwrap();
        }
        assert_eq!(ring.map_head(false).unwrap().2, MapMode::Blocking);
        for index in [10, 12, 13] {
            let frame = ring.complete_head().unwrap();
            assert_eq!(frame.index, index);
            assert_eq!(frame.capture_qpc, (index * 100) as i64);
        }
        assert_eq!(ring.len(), 0);
    }

    #[test]
    fn still_drawing_keeps_head_and_full_ring_requires_blocking() {
        let mut ring = ReadbackRing::<u8>::new(2).unwrap();
        ring.push(7).unwrap();
        assert_eq!(ring.map_head(false).unwrap().2, MapMode::NonBlocking);
        // Fake DO_NOT_WAIT returns STILL_DRAWING; caller does not complete head.
        assert_eq!(ring.map_head(false).unwrap().1, &7);
        ring.push(8).unwrap();
        assert_eq!(ring.map_head(false).unwrap().2, MapMode::Blocking);
        assert_eq!(ring.push(9), Err(9));
        assert_eq!(ring.complete_head(), Some(7));
        assert_eq!(ring.complete_head(), Some(8));
    }

    #[test]
    fn flush_and_resize_require_drain_and_reuse_slots() {
        let mut ring = ReadbackRing::new(2).unwrap();
        ring.push(1).unwrap();
        assert_eq!(ring.map_head(true).unwrap().2, MapMode::Blocking);
        assert_eq!(ring.complete_head(), Some(1));
        // The encoder recreates its staging textures and ring after draining.
        let mut ring = ReadbackRing::new(3).unwrap();
        assert_eq!(ring.capacity(), 3);
        for value in 2..=4 {
            ring.push(value).unwrap();
        }
        for value in 2..=4 {
            assert_eq!(ring.complete_head(), Some(value));
        }
    }

    #[test]
    fn rejects_invalid_capacity() {
        assert!(matches!(
            ReadbackRing::<u8>::new(1),
            Err(RingError::InvalidCapacity)
        ));
        assert!(matches!(
            ReadbackRing::<u8>::new(0),
            Err(RingError::InvalidCapacity)
        ));
    }

    #[test]
    fn one_frame_without_successor_is_delivered_by_one_period() {
        let period = Duration::from_nanos(33_333_334);
        let mut ring = ReadbackRing::new(2).unwrap();
        ring.push(42).unwrap();
        assert!(!tail_due(period - Duration::from_nanos(1), period));
        assert_eq!(ring.len(), 1);
        if tail_due(period, period) {
            assert_eq!(ring.complete_head(), Some(42));
        }
        assert_eq!(ring.len(), 0);
    }

    #[test]
    fn burst_then_silence_flushes_last_frame_in_order() {
        let period = Duration::from_nanos(16_666_667);
        let mut ring = ReadbackRing::new(3).unwrap();
        ring.push(10).unwrap();
        assert_eq!(ring.complete_head(), Some(10));
        ring.push(11).unwrap();
        assert_eq!(ring.complete_head(), Some(11));
        ring.push(12).unwrap();
        // No next input arrives: the nonblocking poll at one period drains 12.
        if tail_due(period, period) {
            assert_eq!(ring.map_head(true).unwrap().2, MapMode::Blocking);
            assert_eq!(ring.complete_head(), Some(12));
        }
        assert_eq!(ring.len(), 0);
    }
}
