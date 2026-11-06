//! Absolute-floor rate policy, independent from the transport and hardware clock.
//! The caller can retain the legacy policy for comparison and rollback.

/// Spacing of encoder rate changes; the default matches the original NVENC policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RatePacing {
    /// Minimum time between any two successful applications.
    pub minimum_ms: u64,
    /// Minimum time before an increase (drops only obey `minimum_ms`).
    pub increase_minimum_ms: u64,
    /// Relative change ignored as estimator noise, in thousandths of the applied rate.
    pub deadband_permille: u32,
}

impl Default for RatePacing {
    fn default() -> Self {
        Self {
            minimum_ms: 0,
            increase_minimum_ms: 0,
            deadband_permille: 20,
        }
    }
}

impl RatePacing {
    /// Encoder changes the rate in the open session (NVENC reconfigure, x264, QuickSync Reset).
    pub const IN_SESSION: Self = Self {
        minimum_ms: 0,
        increase_minimum_ms: 0,
        deadband_permille: 20,
    };
    /// Encoder must be reopened (AMF, or a driver that refused the in-session change):
    /// congestion drops still apply after 1 s, recovery waits 5 s; changes under 10% are ignored
    /// (the 15% recovery step must stay above this deadband).
    pub const REOPEN: Self = Self {
        minimum_ms: 1000,
        increase_minimum_ms: 5000,
        deadband_permille: 100,
    };
}

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
        self.target_paced(
            now_ms,
            RatePacing {
                minimum_ms,
                ..RatePacing::default()
            },
        )
    }

    /// Changes that reopen the encoder cost an IDR and a stall of hundreds of ms. Small
    /// adjustments then lose more quality than they recover, so only large steps pass.
    pub fn target_paced(&self, now_ms: u64, pacing: RatePacing) -> Option<u32> {
        let since_last = self
            .last_application_ms
            .map(|last| now_ms.saturating_sub(last));
        if since_last.is_some_and(|elapsed| elapsed < pacing.minimum_ms) {
            return None;
        }
        if self.requested > self.applied
            && since_last.is_some_and(|elapsed| elapsed < pacing.increase_minimum_ms)
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
                || u64::from(target.abs_diff(self.applied)) * 1000
                    >= u64::from(self.applied) * u64::from(pacing.deadband_permille)))
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
    fn reopen_pacing_ignores_estimator_noise_but_applies_congestion() {
        use super::RatePacing;
        // Sequence seen on an i7-8565U (QuickSync, 0.1.15): BWE climbing 4.1 -> 5.9 Mbps
        // reopened the encoder nine times in 13 s.
        let mut ramp = RateRamp::new(18_000_000, 250_000);
        ramp.applied(4_000_000, 0);
        let mut applications = 0;
        for (now, bps) in (0..13_000).step_by(650).zip(
            [
                5_070_000u64,
                4_100_000,
                4_920_000,
                5_310_000,
                5_450_000,
                5_580_000,
                5_700_000,
                5_870_000,
            ]
            .into_iter()
            .cycle(),
        ) {
            ramp.request(bps);
            if let Some(target) = ramp.target_paced(now, RatePacing::REOPEN) {
                ramp.applied(target, now);
                applications += 1;
            }
        }
        assert!(applications <= 3, "{applications} reaberturas");
        // Real congestion still drops after the 1 s floor, without the 5 s recovery wait.
        ramp.applied(4_000_000, 20_000);
        ramp.request(2_000_000);
        assert_eq!(ramp.target_paced(20_999, RatePacing::REOPEN), None);
        assert_eq!(
            ramp.target_paced(21_000, RatePacing::REOPEN),
            Some(1_600_000)
        );
        // Recovery: 5 s wait, then 15% steps that clear the 10% deadband up to the cap.
        ramp.applied(1_600_000, 21_000);
        ramp.request(u64::MAX);
        assert_eq!(ramp.target_paced(25_999, RatePacing::REOPEN), None);
        let mut now = 26_000;
        while let Some(target) = ramp.target_paced(now, RatePacing::REOPEN) {
            ramp.applied(target, now);
            now += 5000;
        }
        assert_eq!(ramp.applied, 18_000_000);
    }

    #[test]
    fn in_session_pacing_matches_the_original_policy() {
        use super::RatePacing;
        let mut a = RateRamp::new(18_000_000, 250_000);
        let mut b = RateRamp::new(18_000_000, 250_000);
        for (now, bps) in [
            (0, 9_000_000u64),
            (500, 12_000_000),
            (1500, 30_000_000),
            (2600, 1_000_000),
        ] {
            a.request(bps);
            b.request(bps);
            let left = a.target(now);
            assert_eq!(left, b.target_paced(now, RatePacing::IN_SESSION));
            if let Some(target) = left {
                a.applied(target, now);
                b.applied(target, now);
            }
        }
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
