//! Absolute-floor rate policy, independent from the transport and hardware clock.
//! The caller can retain the legacy policy for comparison and rollback.

#[derive(Debug)]
pub struct RateRamp {
    cap: u32,
    floor: u32,
    applied: u32,
    requested: u32,
    last_application_ms: Option<u64>,
}

impl RateRamp {
    pub fn new(cap: u32, floor: u32) -> Self {
        let cap = cap.max(1);
        Self {
            cap,
            floor: floor.clamp(1, cap),
            applied: cap,
            requested: cap,
            last_application_ms: None,
        }
    }

    /// Latest feedback replaces older requests, including while the encoder is busy.
    pub fn request(&mut self, bps: u64) {
        // Transport feedback budgets packetized media. Reserve headroom for
        // RTP/SRTP overhead, rate-estimator uncertainty and recovery keyframes.
        // Integer division first avoids overflow for untrusted/extreme feedback.
        let budget = bps / 100 * 80 + bps % 100 * 80 / 100;
        self.requested = budget.clamp(u64::from(self.floor), u64::from(self.cap)) as u32;
    }

    /// Drops are immediate; recovery grows at most 15% per successful second.
    /// A successful application, rather than a Busy attempt, starts the cooldown.
    pub fn target(&self, now_ms: u64) -> Option<u32> {
        self.target_with_minimum_interval(now_ms, 0)
    }

    /// FFmpeg reopens a session: bound successful changes separately from NVENC.
    pub fn target_with_minimum_interval(&self, now_ms: u64, minimum_ms: u64) -> Option<u32> {
        if self
            .last_application_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < minimum_ms)
        {
            return None;
        }
        let target = if self.requested <= self.applied {
            self.requested
        } else {
            if self
                .last_application_ms
                .is_some_and(|last| now_ms.saturating_sub(last) < 1000)
            {
                return None;
            }
            self.requested.min(
                self.applied
                    .saturating_add((self.applied / 100 * 15).max(1)),
            )
        };
        // The deadband suppresses estimator noise, but must not leave recovery
        // permanently just below the cap (or congestion just above the floor).
        (target != self.applied
            && (target == self.cap
                || target == self.floor
                || u64::from(target.abs_diff(self.applied)) * 50 >= u64::from(self.applied)))
        .then_some(target)
    }

    pub fn applied(&mut self, bps: u32, now_ms: u64) {
        self.applied = bps.clamp(self.floor, self.cap);
        self.last_application_ms = Some(now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::RateRamp;

    #[test]
    fn sustained_congestion_can_drop_below_half_the_cap_and_busy_keeps_latest() {
        let mut ramp = RateRamp::new(18_000_000, 250_000);
        ramp.request(2_000_000);
        assert_eq!(ramp.target(0), Some(1_600_000));
        assert_eq!(ramp.target(100), Some(1_600_000)); // Busy did not acknowledge.
        ramp.request(1_000_000);
        assert_eq!(ramp.target(101), Some(800_000));
        ramp.applied(800_000, 101);
        assert_eq!(ramp.target(2000), None);
        ramp.request(0);
        assert_eq!(ramp.target(2001), Some(250_000));
    }

    #[test]
    fn recovery_is_bounded_and_eventually_reaches_cap_without_oscillation() {
        let mut ramp = RateRamp::new(18_000_000, 250_000);
        ramp.applied(2_000_000, 0);
        ramp.request(u64::MAX);
        assert_eq!(ramp.target(999), None);
        assert_eq!(ramp.target(1000), Some(2_300_000));
        for now in (1000..40_000).step_by(1000) {
            if let Some(target) = ramp.target(now) {
                ramp.applied(target, now);
            }
        }
        assert_eq!(ramp.applied, 18_000_000);
        assert_eq!(ramp.target(40_000), None);
    }

    #[test]
    fn noise_extremes_and_new_session_are_bounded() {
        let mut ramp = RateRamp::new(u32::MAX, 0);
        ramp.request(u64::MAX);
        assert_eq!(ramp.target(0), None);
        ramp.request(u64::MAX - 1);
        assert_eq!(ramp.target(1), None);
        let mut new_session = RateRamp::new(1000, 5000);
        new_session.request(0);
        assert_eq!(new_session.target(0), None);
    }

    #[test]
    fn recovery_reaches_the_cap_even_when_the_last_step_is_inside_the_deadband() {
        let mut ramp = RateRamp::new(18_000_000, 1_000_000);
        ramp.request(22_250_000);
        assert_eq!(ramp.target(0), None); // Ordinary feedback noise is ignored.
        ramp.applied(17_800_000, 0);
        ramp.request(22_500_000);
        assert_eq!(ramp.target(999), None); // Recovery cooldown still applies.
        assert_eq!(ramp.target(1000), Some(18_000_000));
        ramp.applied(18_000_000, 1000);
        assert_eq!(ramp.target(2000), None); // No repeated application at the cap.
        ramp.applied(1_010_000, 2000);
        ramp.request(0);
        assert_eq!(ramp.target_with_minimum_interval(2999, 1000), None);
        assert_eq!(
            ramp.target_with_minimum_interval(3000, 1000),
            Some(1_000_000)
        );
        ramp.applied(1_000_000, 3000);
        assert_eq!(ramp.target(4000), None);
    }

    #[test]
    fn reopen_cooldown_keeps_latest_feedback_without_reopening_each_frame() {
        let mut ramp = RateRamp::new(18_000_000, 250_000);
        ramp.request(4_000_000);
        ramp.applied(4_000_000, 0);
        ramp.request(2_000_000);
        assert_eq!(ramp.target_with_minimum_interval(999, 1000), None);
        ramp.request(1_000_000);
        assert_eq!(ramp.target_with_minimum_interval(1000, 1000), Some(800_000));
        assert_eq!(ramp.target_with_minimum_interval(1001, 1000), Some(800_000));
    }
}
