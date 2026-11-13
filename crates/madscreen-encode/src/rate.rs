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

/// Window of the capacity hold: the floor follows the best budget of the last 30–60 s.
const HOLD_BUCKET_MS: u64 = 30_000;
/// The probing floor climbs 25% above the estimate every 2 s while the estimate does not fall.
const PROBE_STEP_MS: u64 = 2_000;
/// A budget below 85% of its peak since the last restart reads as congestion.
const PROBE_DROP_PERCENT: u32 = 85;

#[derive(Debug)]
pub struct RateRamp {
    cap: u32,
    /// Configured floor; `floor` may sit above it while the capacity hold is active.
    base_floor: u32,
    floor: u32,
    applied: u32,
    requested: u32,
    last_application_ms: Option<u64>,
    hold_bucket: u64,
    hold_current: u32,
    hold_previous: u32,
    probe_floor: u32,
    /// Half of the proven rate when probing restarted; latched so the hold window aging out
    /// mid-climb does not dip the floor.
    probe_ceiling: u32,
    probe_peak: u32,
    probe_step_ms: u64,
}

impl RateRamp {
    pub fn new(cap: u32, floor: u32) -> Self {
        let cap = cap.max(1);
        let floor = floor.clamp(1, cap);
        Self {
            cap,
            base_floor: floor,
            floor,
            applied: cap,
            requested: cap,
            last_application_ms: None,
            hold_bucket: 0,
            hold_current: 0,
            hold_previous: 0,
            probe_floor: floor,
            probe_ceiling: floor,
            probe_peak: 0,
            probe_step_ms: 0,
        }
    }

    fn budget(&self, bps: u64) -> u64 {
        // Transport feedback budgets packetized media. Reserve headroom for
        // RTP/SRTP overhead, rate-estimator uncertainty and recovery keyframes.
        // Integer division first avoids overflow for untrusted/extreme feedback.
        // A request at the cap means the link has room (WebRTC clamps its estimate to the
        // cap, ~97% of it); discounting it would pin the encoder at 80% of the cap forever.
        if bps >= u64::from(self.cap) / 100 * 95 {
            u64::from(self.cap)
        } else {
            bps / 100 * 80 + bps % 100 * 80 / 100
        }
    }

    /// Latest feedback replaces older requests, including while the encoder is busy.
    pub fn request(&mut self, bps: u64) {
        self.requested = self
            .budget(bps)
            .clamp(u64::from(self.floor), u64::from(self.cap)) as u32;
    }

    /// Like `request`, but probes back toward half of the best budget of the last 30–60 s.
    /// Live of 11/nov (NVENC 1080p60): a 2 s RTT blip dropped the estimate to ~1 Mbps for 35 s
    /// with zero packet loss, and GCC only grows from what we actually send (in the lab it stayed
    /// at ~2.4 Mbps for 90 s after a cut). Jumping straight to half the proven rate fixed that but
    /// froze the picture during a real cut, so the floor climbs in steps and restarts whenever the
    /// estimate falls. Call it every frame with the latest WebRTC request: WebRTC only reports
    /// changes, and a stuck estimate is exactly the case that needs the steps.
    pub fn request_at(&mut self, bps: u64, now_ms: u64) {
        let bucket = now_ms / HOLD_BUCKET_MS;
        if bucket != self.hold_bucket {
            self.hold_previous = if bucket == self.hold_bucket + 1 {
                self.hold_current
            } else {
                0
            };
            self.hold_current = 0;
            self.hold_bucket = bucket;
        }
        let budget = self.budget(bps).min(u64::from(self.cap)) as u32;
        self.hold_current = self.hold_current.max(budget);
        if u64::from(budget) * 100 < u64::from(self.probe_peak) * u64::from(PROBE_DROP_PERCENT) {
            let proven = self.hold_current.max(self.hold_previous);
            self.probe_ceiling = (proven / 2).clamp(self.base_floor, self.cap);
            self.probe_floor = self.base_floor;
            self.probe_peak = budget;
            self.probe_step_ms = now_ms;
        } else {
            self.probe_peak = self.probe_peak.max(budget);
            if now_ms.saturating_sub(self.probe_step_ms) >= PROBE_STEP_MS {
                self.probe_step_ms = now_ms;
                let base = self.probe_floor.max(budget);
                self.probe_floor = base
                    .saturating_add(base / 4)
                    .min(self.probe_ceiling.max(self.probe_floor));
            }
        }
        self.floor = self.probe_floor.clamp(self.base_floor, self.cap);
        self.request(bps);
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
        // Registra o que o encoder recebeu: presa ao piso, a rampa "achava" que já estava nele
        // quando o piso em degraus subia antes da taxa e parava de subir (campo, AMF, 12/nov: 45 s).
        self.applied = bps.min(self.cap);
        self.last_application_ms = Some(now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::RateRamp;

    #[test]
    fn encoder_reaches_a_floor_that_climbs_above_it() {
        // Pedidos do WebRTC (ms, bps) de uma live real com AMF (12/nov, 3440x1440): o piso em
        // degraus passou da taxa aplicada, a rampa gravou o piso como aplicado e o encoder ficou
        // 45 s abaixo dele. AMF reabre a cada troca: subida no ritmo REOPEN.
        const FIELD: &[(u64, u64)] = &[
            (69573, 1359127),
            (70571, 1409595),
            (71562, 1394126),
            (72563, 1360444),
            (73575, 1339735),
            (74579, 1280189),
            (75587, 1230023),
            (76593, 1208325),
            (77596, 1269716),
            (78599, 1369456),
            (79599, 1519726),
            (80605, 1519362),
            (81621, 1544975),
            (82628, 1416690),
            (83637, 1401200),
            (84637, 1442665),
            (85667, 1530974),
            (86679, 1259652),
            (87679, 1132489),
            (88672, 1127942),
            (89682, 1455076),
            (90678, 1813038),
            (91681, 1942858),
            (92709, 1966788),
            (93727, 1921449),
            (94736, 1941638),
            (95772, 2076975),
            (96813, 2081239),
            (97884, 2171089),
            (98874, 2094871),
            (99888, 2119498),
            (100915, 2177780),
            (101919, 2230959),
            (102941, 2217730),
            (103929, 2215064),
            (104959, 2285133),
            (105945, 2444853),
            (106958, 2562065),
            (107961, 2734458),
            (109021, 2881990),
            (110031, 2906786),
            (111044, 2792337),
            (112016, 2830426),
            (113078, 2618348),
            (114113, 2475976),
            (115135, 2685993),
            (116141, 3125277),
            (117147, 3685235),
            (118165, 4354574),
            (119186, 4355776),
            (120243, 4358000),
            (121207, 4359000),
            (122253, 4359000),
            (123244, 893000),
            (124269, 826174),
            (125306, 808973),
            (126279, 781794),
            (127309, 539623),
            (128325, 485997),
            (129333, 414328),
            (130343, 396394),
            (131389, 413839),
            (132394, 428179),
            (133385, 444721),
            (134397, 486602),
            (135403, 610167),
            (136417, 670843),
            (137478, 700348),
            (138410, 789447),
            (139431, 856490),
            (140433, 914636),
            (141442, 1158940),
            (142465, 1200363),
            (143465, 1400483),
            (144492, 1402570),
            (145484, 1373029),
            (146489, 1428859),
            (147520, 1422204),
            (148509, 1474839),
            (149524, 1475812),
            (150521, 1484363),
            (151543, 1465301),
            (152544, 1492950),
            (153606, 1493486),
            (154601, 1505869),
            (155613, 1513300),
            (156628, 1511263),
            (157682, 1548000),
            (158666, 1548000),
            (159650, 1548000),
            (160670, 1518203),
        ];
        let mut ramp = RateRamp::new(33_400_000, 1_000_000);
        let mut encoder = 1_011_386;
        ramp.applied(encoder, FIELD[0].0);
        let mut row = 0;
        for t in (FIELD[0].0..=FIELD[FIELD.len() - 1].0).step_by(16) {
            while row + 1 < FIELD.len() && FIELD[row + 1].0 <= t {
                row += 1;
            }
            ramp.request_at(FIELD[row].1, t);
            if let Some(target) = ramp.target_paced(t, super::RatePacing::REOPEN) {
                encoder = target;
                ramp.applied(target, t);
            }
        }
        assert!(ramp.floor > 1_000_000, "o piso deveria ter subido");
        assert_eq!(encoder, ramp.floor);
    }

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
    fn free_link_request_saturated_by_webrtc_reaches_the_cap_not_80_percent() {
        // Lab governor-bw free-on: cap 18 Mbps, WebRTC asked 17.458 Mbps on every sample.
        let mut ramp = RateRamp::new(18_000_000, 250_000);
        ramp.applied(13_000_000, 0);
        ramp.request(17_458_000);
        let mut now = 1000;
        while let Some(target) = ramp.target(now) {
            ramp.applied(target, now);
            now += 1000;
        }
        assert_eq!(ramp.applied, 18_000_000);
        // Below the saturation band the 20% headroom still applies.
        ramp.request(17_000_000);
        assert_eq!(ramp.target(now), Some(13_600_000));
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

    #[test]
    fn collapse_probes_back_in_steps_and_restarts_when_the_estimate_falls() {
        // 1080p60 cap 14 Mbps, floor 1 Mbps; the link proved 10 Mbps (budget 8), then collapsed.
        let mut ramp = RateRamp::new(14_000_000, 1_000_000);
        ramp.request_at(10_000_000, 1_000);
        ramp.request_at(500_000, 3_000); // drop: ceiling latched at 4 Mbps, floor at base
        assert_eq!(ramp.requested, 1_000_000);
        // Stuck estimate: +25% every 2 s up to half of the proven rate, never above it.
        let mut t = 3_000;
        let mut steps = vec![];
        for _ in 0..8 {
            t += 2_000;
            ramp.request_at(500_000, t);
            steps.push(ramp.requested);
        }
        assert_eq!(&steps[..3], &[1_250_000, 1_562_500, 1_953_125]);
        assert_eq!(steps.last(), Some(&4_000_000));
        // Within a step nothing changes, however often it is called.
        ramp.request_at(500_000, t + 1_999);
        assert_eq!(ramp.requested, 4_000_000);
        // The estimate rising with what we send is no congestion; falling again restarts.
        ramp.request_at(5_000_000, t + 2_000);
        assert_eq!(ramp.requested, 4_000_000);
        ramp.request_at(3_000_000, t + 2_100);
        assert_eq!(ramp.requested, 2_400_000);
        assert_eq!(ramp.probe_ceiling, 4_000_000);
        // A lasting drop forgets the old peak at the next restart.
        let mut gap = RateRamp::new(14_000_000, 1_000_000);
        gap.request_at(14_000_000, 0);
        gap.request_at(500_000, 90_000);
        gap.request_at(500_000, 120_000);
        assert_eq!(gap.requested, 1_000_000);
        // Without any drop (free link at the cap) the floor stays at its base.
        let mut free = RateRamp::new(14_000_000, 1_000_000);
        for t in (0..20_000).step_by(1_000) {
            free.request_at(14_000_000, t);
        }
        assert_eq!(free.floor, 1_000_000);
    }
}
