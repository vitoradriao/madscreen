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
    transport_budget: u32,
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
            transport_budget: cap,
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
        // WebRTC already removes RTP overhead before SetRates (GetPayloadBitrateBps in
        // video_send_stream_impl.cc). The former extra 20% left NVENC at ~60% of the estimate in
        // game (Cyberpunk on Magalu, 21/nov), and GCC never grows past 1.5x what it receives.
        // A request at the cap means the link has room (WebRTC clamps its estimate to ~97% of it).
        // Integer division first avoids overflow for untrusted/extreme feedback.
        if bps >= u64::from(self.cap) / 100 * 95 {
            u64::from(self.cap)
        } else {
            bps
        }
    }

    /// Latest feedback replaces older requests, including while the encoder is busy.
    pub fn request(&mut self, bps: u64) {
        self.transport_budget = self.budget(bps) as u32;
        self.requested = self.transport_budget.max(self.floor);
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

    /// Drops are immediate. Growth above the transport budget is limited to 15% per step.
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
            // Follow measured capacity after the cooldown. Advancing only 15% after a
            // successful probe can leave the link underused until the new estimate expires.
            let next_floor_step = self
                .applied
                .saturating_add((self.applied / 100 * 15).max(1));
            self.requested
                .min(self.transport_budget.max(next_floor_step))
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

/// Window over which the encoder output is compared with the rate it was running at.
const FILL_WINDOW_MS: u64 = 1_000;
/// Aim slightly below the WebRTC target: keyframes and estimator noise need room.
const FILL_TARGET: f64 = 0.95;
/// NVENC delivered 70–79% of the applied rate in game (21/nov); a larger gain would overshoot the
/// link when the content suddenly fills the VBV.
const FILL_MAX_GAIN: f64 = 1.35;
/// Frames under half of their budget are limited by the content (static screen, menus), not by
/// the rate control; such windows keep the gain for when motion returns.
const FILL_CONTENT_LIMITED: f64 = 0.5;

/// Closed-loop correction of encoder undershoot. GCC only raises its estimate up to 1.5x the
/// throughput it sees (aimd_rate_control.cc), so an encoder delivering 75% of each request pins
/// the estimate; scaling the request by the measured shortfall lets the estimate grow.
#[derive(Debug)]
pub struct FillGain {
    gain: f64,
    window_start_ms: Option<u64>,
    last_ms: u64,
    bits: f64,
    frame_budget_bits: f64,
    rate_bit_ms: f64,
}

impl Default for FillGain {
    fn default() -> Self {
        Self {
            gain: 1.,
            window_start_ms: None,
            last_ms: 0,
            bits: 0.,
            frame_budget_bits: 0.,
            rate_bit_ms: 0.,
        }
    }
}

impl FillGain {
    pub fn gain(&self) -> f64 {
        self.gain
    }

    /// Encoder ceiling for a configured `cap_bps`: at the cap NVENC still delivered 8–11 of
    /// 14 Mbps in game (23/nov), so the correction must be able to ask for more than the cap.
    pub fn ceiling(cap_bps: u32) -> u32 {
        (f64::from(cap_bps) * FILL_MAX_GAIN) as u32
    }

    /// Rate to ask the encoder for so that it delivers about `bps` (float casts saturate).
    pub fn apply(&self, bps: u64) -> u64 {
        (bps as f64 * self.gain) as u64
    }

    /// One access unit of `bytes`, produced while the encoder ran at `encoder_bps` and `fps`.
    pub fn observe(&mut self, bytes: usize, encoder_bps: u32, fps: f64, now_ms: u64) {
        let Some(start) = self.window_start_ms else {
            self.restart(now_ms);
            return;
        };
        let elapsed = now_ms.saturating_sub(start);
        if elapsed > 2 * FILL_WINDOW_MS {
            // Capture paused: time without frames says nothing about the rate control.
            self.restart(now_ms);
            return;
        }
        self.rate_bit_ms += f64::from(encoder_bps) * now_ms.saturating_sub(self.last_ms) as f64;
        self.last_ms = now_ms;
        self.bits += bytes as f64 * 8.;
        if fps > 0. {
            self.frame_budget_bits += f64::from(encoder_bps) / fps;
        }
        if elapsed < FILL_WINDOW_MS {
            return;
        }
        if self.frame_budget_bits > 0.
            && self.rate_bit_ms > 0.
            && self.bits / self.frame_budget_bits >= FILL_CONTENT_LIMITED
        {
            let efficiency = self.bits * 1000. / self.rate_bit_ms;
            let wanted = (FILL_TARGET / efficiency).clamp(1., FILL_MAX_GAIN);
            // Rise halfway per window, fall at once: overshooting the link costs more.
            self.gain = if wanted > self.gain {
                (self.gain + wanted) / 2.
            } else {
                wanted
            };
        }
        self.restart(now_ms);
    }

    fn restart(&mut self, now_ms: u64) {
        self.window_start_ms = Some(now_ms);
        self.last_ms = now_ms;
        self.bits = 0.;
        self.frame_budget_bits = 0.;
        self.rate_bit_ms = 0.;
    }
}

#[cfg(test)]
mod tests {
    use super::{FillGain, RateRamp};

    /// Feeds `seconds` of 60 fps output where each frame is `fill` of its budget at `bps`.
    fn feed(gain: &mut FillGain, bps: u32, fill: f64, from_ms: u64, seconds: u64) -> u64 {
        for frame in 1..=seconds * 60 {
            let bytes = (f64::from(bps) / 60. * fill / 8.) as usize;
            gain.observe(bytes, bps, 60., from_ms + frame * 1000 / 60);
        }
        from_ms + seconds * 1000
    }

    #[test]
    fn fill_gain_corrects_game_undershoot_and_holds_on_static_content() {
        let mut gain = FillGain::default();
        // NVENC at 75% of the applied rate (Cyberpunk on Magalu, 21/nov).
        let mut now = feed(&mut gain, 4_000_000, 0.75, 0, 6);
        assert!((gain.gain() - 0.95 / 0.75).abs() < 0.01, "{}", gain.gain());
        assert_eq!(gain.apply(4_000_000), (4_000_000. * gain.gain()) as u64);
        // Static screen: after the mixed transition window, tiny repeat frames say nothing
        // about the rate control.
        now = feed(&mut gain, 4_000_000, 0.02, now, 2);
        let held = gain.gain();
        assert!(held > 1.2, "{held}");
        now = feed(&mut gain, 4_000_000, 0.02, now, 5);
        assert_eq!(gain.gain(), held);
        // Hard content filling the VBV: back to 1 within one window.
        feed(&mut gain, 5_000_000, 1.0, now, 2);
        assert_eq!(gain.gain(), 1.);
    }

    #[test]
    fn fill_gain_is_bounded_and_ignores_pauses() {
        let mut gain = FillGain::default();
        feed(&mut gain, 8_000_000, 0.55, 0, 10);
        assert!(
            (gain.gain() - super::FILL_MAX_GAIN).abs() < 0.001,
            "{}",
            gain.gain()
        );
        // A 10 s capture pause followed by one full window keeps the measured efficiency only.
        let mut paused = FillGain::default();
        paused.observe(10_000, 8_000_000, 60., 0);
        paused.observe(10_000, 8_000_000, 60., 10_000);
        assert_eq!(paused.gain(), 1.);
        assert_eq!(FillGain::default().apply(u64::MAX), u64::MAX);
        assert_eq!(FillGain::ceiling(14_000_000), 18_900_000);
        assert_eq!(FillGain::ceiling(u32::MAX), u32::MAX);
    }

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
        assert_eq!(ramp.target(0), Some(2_000_000));
        assert_eq!(ramp.target(100), Some(2_000_000)); // Busy did not acknowledge.
        ramp.request(1_000_000);
        assert_eq!(ramp.target(101), Some(1_000_000));
        ramp.applied(1_000_000, 101);
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
        // Below the saturation band the encoder follows the payload rate WebRTC asked for.
        ramp.request(17_000_000);
        assert_eq!(ramp.target(now), Some(17_000_000));
    }

    #[test]
    fn recovery_uses_the_transport_budget_without_waiting_for_floor_steps() {
        let mut ramp = RateRamp::new(18_000_000, 250_000);
        ramp.applied(2_000_000, 0);
        ramp.request(11_682_000);
        assert_eq!(ramp.target(999), None);
        assert_eq!(ramp.target(1000), Some(11_682_000));
        ramp.applied(11_682_000, 1000);
        ramp.request(1_696_000);
        assert_eq!(ramp.target(1001), Some(1_696_000));
    }

    #[test]
    fn unconfirmed_floor_still_rises_in_small_steps() {
        let mut ramp = RateRamp::new(14_000_000, 1_000_000);
        ramp.request_at(14_000_000, 0);
        ramp.request_at(500_000, 1000);
        ramp.applied(1_000_000, 1000);
        ramp.request_at(500_000, 3000);
        assert_eq!(ramp.target(3000), Some(1_150_000));
    }

    #[test]
    fn upward_floor_steps_keep_their_ceiling_across_a_hold_bucket_boundary() {
        let mut ramp = RateRamp::new(14_000_000, 1_000_000);
        ramp.request_at(14_000_000, 1_000);
        ramp.request_at(600_000, 59_000);
        ramp.applied(1_000_000, 59_000);
        ramp.request_at(600_000, 61_000);
        assert_eq!(ramp.probe_ceiling, 7_000_000);
        assert_eq!(ramp.target(61_000), Some(1_150_000));
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
            Some(2_000_000)
        );
        // Recovery: 5 s wait, then one reopen if the transport budget supports the cap.
        ramp.applied(2_000_000, 21_000);
        ramp.request(u64::MAX);
        assert_eq!(ramp.target_paced(25_999, RatePacing::REOPEN), None);
        assert_eq!(
            ramp.target_paced(26_000, RatePacing::REOPEN),
            Some(18_000_000)
        );
        ramp.applied(18_000_000, 26_000);
        assert_eq!(ramp.target_paced(31_000, RatePacing::REOPEN), None);
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
        assert_eq!(
            ramp.target_with_minimum_interval(1000, 1000),
            Some(1_000_000)
        );
        assert_eq!(
            ramp.target_with_minimum_interval(1001, 1000),
            Some(1_000_000)
        );
    }

    #[test]
    fn collapse_probes_back_in_steps_and_restarts_when_the_estimate_falls() {
        // 1080p60 cap 14 Mbps, floor 1 Mbps; the link proved 10 Mbps, then collapsed.
        let mut ramp = RateRamp::new(14_000_000, 1_000_000);
        ramp.request_at(10_000_000, 1_000);
        ramp.request_at(500_000, 3_000); // drop: ceiling latched at 5 Mbps, floor at base
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
        assert_eq!(steps.last(), Some(&5_000_000));
        // Within a step nothing changes, however often it is called.
        ramp.request_at(500_000, t + 1_999);
        assert_eq!(ramp.requested, 5_000_000);
        // The estimate rising with what we send is no congestion; falling again restarts.
        ramp.request_at(5_000_000, t + 2_000);
        assert_eq!(ramp.requested, 5_000_000);
        ramp.request_at(3_000_000, t + 2_100);
        assert_eq!(ramp.requested, 3_000_000);
        assert_eq!(ramp.probe_ceiling, 5_000_000);
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
