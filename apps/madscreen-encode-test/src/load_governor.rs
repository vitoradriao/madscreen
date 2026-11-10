//! Optional one-second load controller. No wall clock, FFI, or encoder reconfiguration.
// Só a publicação (feature `publish`) cria o controlador; o runner usa os tipos nos dois builds.
#![cfg_attr(not(feature = "publish"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionStep {
    Native,
    Hd720,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeReason {
    SustainedOverload,
    SustainedHealth,
    /// O encoder produziu bem mais que a taxa pedida pelo WebRTC: o degrau não cabe na banda.
    BandwidthOverload,
    /// Subida depois de limite por banda: a banda pedida dobrou ou venceu a espera da nova tentativa.
    BandwidthRecovery,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub fps: u32,
    pub resolution: ResolutionStep,
}
#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub at_ms: u64,
    pub captured: u64,
    pub submitted: u64,
    pub encode_wait_max_ms: Option<f64>,
    /// Process CPU time divided by wall time; two fully busy threads are 2.0 cores.
    pub process_cpu_cores: Option<f64>,
    pub system_cpu_percent: Option<f64>,
    /// Última taxa de vídeo pedida pelo WebRTC (estimativa de banda, antes do piso).
    pub requested_bps: Option<u64>,
    /// Bits que o encoder produziu nesta janela, convertidos para bits por segundo.
    pub produced_bps: Option<u64>,
}
#[derive(Clone, Debug)]
pub struct Diagnostics {
    pub target: Target,
    pub change_count: u64,
    pub time_at_step_ms: Vec<u64>,
    pub last_reason: Option<ChangeReason>,
}
const OVERLOAD_WINDOWS: u32 = 3; // Three consecutive one-second windows reject a single spike.
const HEALTH_WINDOWS: u32 = 10; // Recover more slowly than degradation.
const MIN_DWELL_MS: u64 = 5_000; // Let capture and CPU counters settle after a change.
const MAX_CHANGES_PER_MINUTE: usize = 3; // Bounds oscillation and encoder reopen churn.
// Timer scheduling can jitter around 1 s; a larger gap means missing/suspended samples.
const MIN_WINDOW_MS: u64 = 800;
const MAX_WINDOW_MS: u64 = 1_500;
// Produzir 25% acima do pedido por 3 janelas: o encoder não consegue caber na banda
// (QP máximo ou piso de taxa acima da estimativa) e a fila de envio/perdas cresce.
const RATE_OVERLOAD_NUM: u64 = 5;
const RATE_OVERLOAD_DEN: u64 = 4;
// Abaixo disto a janela é ruído (tela parada, refino) e não indica falta de banda.
const RATE_MIN_PRODUCED_BPS: u64 = 300_000;
// O BWE pode não subir com o envio limitado pela aplicação: tenta subir mesmo assim,
// com espera dobrada a cada tentativa que voltar a estourar.
const RATE_RETRY_FIRST_MS: u64 = 60_000;
const RATE_RETRY_MAX_MS: u64 = 300_000;
// x264 uses two worker threads: 1.6 cores is 80% of that encoder budget.
const PROCESS_OVERLOAD_CORES: f64 = 1.6;
const PROCESS_HEALTHY_CORES: f64 = 1.2;
pub struct LoadGovernor {
    steps: Vec<Target>,
    current: usize,
    at_step_since_ms: u64,
    last_window_ms: Option<u64>,
    overload_streak: u32,
    health_streak: u32,
    changes: Vec<u64>,
    spent_ms: Vec<u64>,
    change_count: u64,
    last_reason: Option<ChangeReason>,
    /// Sinais de CPU/fila do encoder; desligados no NVENC, onde só a banda governa.
    cpu_signals: bool,
    /// Taxa que cada degrau produzia quando a banda forçou a descida dele: só se volta a
    /// um degrau quando o WebRTC pedir mais que isso (ou na nova tentativa).
    rate_need_bps: Vec<Option<u64>>,
    rate_retry_ms: u64,
}
impl LoadGovernor {
    pub fn new(configured_fps: u32, can_reduce_to_720: bool, at_ms: u64) -> Self {
        let initial = configured_fps.max(15);
        let mut rates = vec![initial];
        for fps in [60, 30, 20, 15] {
            if fps < initial && !rates.contains(&fps) {
                rates.push(fps);
            }
        }
        let mut steps: Vec<_> = rates
            .into_iter()
            .map(|fps| Target {
                fps,
                resolution: ResolutionStep::Native,
            })
            .collect();
        if can_reduce_to_720 {
            steps.push(Target {
                fps: 15,
                resolution: ResolutionStep::Hd720,
            });
        }
        let spent_ms = vec![0; steps.len()];
        let rate_need_bps = vec![None; steps.len()];
        Self {
            steps,
            current: 0,
            at_step_since_ms: at_ms,
            last_window_ms: None,
            overload_streak: 0,
            health_streak: 0,
            changes: Vec::with_capacity(MAX_CHANGES_PER_MINUTE),
            spent_ms,
            change_count: 0,
            last_reason: None,
            cpu_signals: true,
            rate_need_bps,
            rate_retry_ms: RATE_RETRY_FIRST_MS,
        }
    }
    /// Só a banda governa (NVENC): CPU e fila do encoder não são sinais confiáveis ali.
    pub fn rate_only(mut self) -> Self {
        self.cpu_signals = false;
        self
    }
    pub fn target(&self) -> Target {
        self.steps[self.current]
    }
    pub fn diagnostics(&self, now_ms: u64) -> Diagnostics {
        let mut time_at_step_ms = self.spent_ms.clone();
        time_at_step_ms[self.current] = time_at_step_ms[self.current]
            .saturating_add(now_ms.saturating_sub(self.at_step_since_ms));
        Diagnostics {
            target: self.target(),
            change_count: self.change_count,
            time_at_step_ms,
            last_reason: self.last_reason,
        }
    }
    pub fn observe(&mut self, sample: Window) -> Option<Target> {
        let valid_time = self.last_window_ms.is_none_or(|last| {
            sample
                .at_ms
                .checked_sub(last)
                .is_some_and(|elapsed| (MIN_WINDOW_MS..=MAX_WINDOW_MS).contains(&elapsed))
        });
        self.last_window_ms = Some(sample.at_ms);
        if !valid_time || sample.captured == 0 || sample.submitted > sample.captured {
            self.overload_streak = 0;
            self.health_streak = 0;
            return None;
        }
        let period_ms = 1000.0 / f64::from(self.target().fps);
        let fraction = sample.submitted as f64 / sample.captured as f64;
        // Counts are compared as a ratio, and wait is a per-frame maximum; both
        // are independent of a tolerable difference in window duration.
        let process = sample
            .process_cpu_cores
            .filter(|n| n.is_finite() && *n >= 0.0);
        let system = sample.system_cpu_percent.filter(|n| valid_percent(*n));
        let wait = sample
            .encode_wait_max_ms
            .filter(|n| n.is_finite() && *n >= 0.0);
        let cpu_overload = self.cpu_signals
            && (fraction < 0.92
                || wait.is_some_and(|ms| ms > 0.8 * period_ms)
                || process.is_some_and(|cores| cores > PROCESS_OVERLOAD_CORES)
                || system.is_some_and(|percent| percent > 90.0));
        let cpu_healthy = !self.cpu_signals
            || (fraction >= 0.98
                && wait.is_none_or(|ms| ms < 0.5 * period_ms)
                && process.is_none_or(|cores| cores < PROCESS_HEALTHY_CORES)
                && system.is_none_or(|percent| percent < 80.0));
        let rate_overload = matches!(
            (sample.requested_bps, sample.produced_bps),
            (Some(requested), Some(produced))
                if requested > 0
                    && produced >= RATE_MIN_PRODUCED_BPS
                    && produced.saturating_mul(RATE_OVERLOAD_DEN)
                        > requested.saturating_mul(RATE_OVERLOAD_NUM)
        );
        // O BWE colapsa durante o congestionamento; a referência para subir é o que o degrau
        // de cima produzia, não o pedido no momento da descida.
        let above_need = self
            .current
            .checked_sub(1)
            .and_then(|above| self.rate_need_bps[above]);
        let rate_recovered = above_need.is_none_or(|need| {
            sample
                .requested_bps
                .is_some_and(|requested| requested.saturating_mul(10) >= need.saturating_mul(11))
        });
        let at_step_ms = sample.at_ms.saturating_sub(self.at_step_since_ms);
        let rate_retry_due =
            above_need.is_some() && !rate_overload && at_step_ms >= self.rate_retry_ms;
        let overload = cpu_overload || rate_overload;
        let healthy = cpu_healthy && !rate_overload && rate_recovered;
        self.overload_streak = if overload {
            self.overload_streak.saturating_add(1)
        } else {
            0
        };
        self.health_streak = if healthy {
            self.health_streak.saturating_add(1)
        } else {
            0
        };
        // Um degrau mantido 60 s sem estourar confirma que a banda o comporta.
        if self.rate_need_bps[self.current].is_some()
            && !rate_overload
            && at_step_ms >= RATE_RETRY_FIRST_MS
        {
            self.rate_need_bps[self.current] = None;
            self.rate_retry_ms = RATE_RETRY_FIRST_MS;
        }
        self.changes
            .retain(|at| sample.at_ms.saturating_sub(*at) < 60_000);
        if sample.at_ms.saturating_sub(self.at_step_since_ms) < MIN_DWELL_MS
            || self.changes.len() >= MAX_CHANGES_PER_MINUTE
        {
            return None;
        }
        let next =
            if self.overload_streak >= OVERLOAD_WINDOWS && self.current + 1 < self.steps.len() {
                if rate_overload {
                    // Uma subida por nova tentativa que volta a estourar dobra a próxima espera.
                    if self.last_reason == Some(ChangeReason::BandwidthRecovery) {
                        self.rate_retry_ms = (self.rate_retry_ms * 2).min(RATE_RETRY_MAX_MS);
                    }
                    self.rate_need_bps[self.current] = sample.produced_bps;
                    self.last_reason = Some(ChangeReason::BandwidthOverload);
                } else {
                    self.last_reason = Some(ChangeReason::SustainedOverload);
                }
                Some(self.current + 1)
            } else if self.current > 0
                && cpu_healthy
                && (self.health_streak >= HEALTH_WINDOWS || rate_retry_due)
            {
                self.last_reason = Some(if above_need.is_some() {
                    ChangeReason::BandwidthRecovery
                } else {
                    ChangeReason::SustainedHealth
                });
                Some(self.current - 1)
            } else {
                None
            }?;
        self.spent_ms[self.current] = self.spent_ms[self.current]
            .saturating_add(sample.at_ms.saturating_sub(self.at_step_since_ms));
        self.current = next;
        self.at_step_since_ms = sample.at_ms;
        self.changes.push(sample.at_ms);
        self.change_count += 1;
        self.overload_streak = 0;
        self.health_streak = 0;
        Some(self.target())
    }
}
fn valid_percent(value: f64) -> bool {
    value.is_finite() && (0.0..=100.0).contains(&value)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn window(at_ms: u64, busy: bool) -> Window {
        Window {
            at_ms,
            captured: 30,
            submitted: if busy { 20 } else { 30 },
            encode_wait_max_ms: Some(if busy { 40.0 } else { 2.0 }),
            process_cpu_cores: Some(if busy { 1.9 } else { 0.4 }),
            system_cpu_percent: Some(if busy { 95.0 } else { 30.0 }),
            requested_bps: None,
            produced_bps: None,
        }
    }
    fn rate(at_ms: u64, requested: u64, produced: u64) -> Window {
        Window {
            at_ms,
            captured: 180,
            submitted: 60,
            encode_wait_max_ms: Some(2.0),
            process_cpu_cores: Some(0.05),
            system_cpu_percent: None,
            requested_bps: Some(requested),
            produced_bps: Some(produced),
        }
    }
    #[test]
    fn nvenc_rate_only_ignores_capture_above_target_and_cpu() {
        // Captura a 180 Hz com encode a 60 seria "sobrecarga" pelos sinais de CPU.
        let mut g = LoadGovernor::new(60, true, 0).rate_only();
        for t in 1..=60 {
            assert_eq!(g.observe(rate(t * 1000, 8_000_000, 7_900_000)), None);
        }
        assert_eq!(g.target().fps, 60);
    }
    #[test]
    fn encoder_overshooting_the_estimate_steps_down_and_stays_on_the_same_link() {
        // Lab #15: enlace de 1 Mbps, pedido ~0,7 Mbps, NVENC 1080p60 produz ~1,5 Mbps.
        let mut g = LoadGovernor::new(60, true, 0).rate_only();
        let mut fps = 60;
        let mut changes = Vec::new();
        for t in 1..=50 {
            // Cada degrau reduz a produção pela fração de FPS (encoder segue configurado a 60).
            let produced = 1_500_000 * u64::from(fps) / 60;
            if let Some(target) = g.observe(rate(t * 1000, 700_000, produced)) {
                fps = target.fps;
                changes.push((t, target.fps));
            }
        }
        assert_eq!(changes, vec![(5, 30)]);
        assert_eq!(
            g.diagnostics(50_000).last_reason,
            Some(ChangeReason::BandwidthOverload)
        );
        // Sem banda nova não sobe antes da espera da nova tentativa (60 s no degrau).
        for t in 51..=64 {
            assert_eq!(g.observe(rate(t * 1000, 700_000, 750_000)), None);
        }
        assert_eq!(g.target().fps, 30);
    }
    #[test]
    fn doubled_estimate_recovers_and_failed_retry_doubles_the_wait() {
        let mut g = LoadGovernor::new(60, true, 0).rate_only();
        for t in 1..=5 {
            g.observe(rate(t * 1000, 700_000, 1_500_000));
        }
        assert_eq!(g.target().fps, 30);
        // Pedido abaixo de 1,1x o que o 60 FPS produzia não basta; acima, volta a 60.
        for t in 6..=12 {
            assert_eq!(g.observe(rate(t * 1000, 1_600_000, 700_000)), None);
        }
        let mut back = None;
        for t in 13..=30 {
            back = back.or(g.observe(rate(t * 1000, 1_700_000, 700_000)).map(|_| t));
        }
        assert_eq!(back, Some(22));
        assert_eq!(g.target().fps, 60);
        // Volta a estourar: desce; nova tentativa depois de 60 s; estoura de novo: espera 120 s.
        let mut t = 31;
        while g.target().fps == 60 {
            g.observe(rate(t * 1000, 700_000, 1_500_000));
            t += 1;
        }
        let down_at = t - 1;
        while g.target().fps == 30 {
            g.observe(rate(t * 1000, 700_000, 750_000));
            t += 1;
        }
        assert!((t - 1 - down_at) * 1000 >= RATE_RETRY_FIRST_MS);
        assert_eq!(
            g.diagnostics(t * 1000).last_reason,
            Some(ChangeReason::BandwidthRecovery)
        );
        while g.target().fps == 60 {
            g.observe(rate(t * 1000, 700_000, 1_500_000));
            t += 1;
        }
        let second_down = t - 1;
        while g.target().fps == 30 && t < second_down + 400 {
            g.observe(rate(t * 1000, 700_000, 750_000));
            t += 1;
        }
        assert!((t - 1 - second_down) * 1000 >= 2 * RATE_RETRY_FIRST_MS);
    }
    #[test]
    fn idle_screen_and_brief_keyframe_spikes_do_not_count_as_overload() {
        let mut g = LoadGovernor::new(60, true, 0).rate_only();
        for t in 1..=40 {
            // Tela parada (produção mínima) e um IDR isolado a cada 10 s.
            let produced = if t % 10 == 0 { 3_000_000 } else { 100_000 };
            assert_eq!(g.observe(rate(t * 1000, 1_000_000, produced)), None);
        }
    }
    #[test]
    fn isolated_spike_and_missing_cpu_never_degrade() {
        let mut g = LoadGovernor::new(30, true, 0);
        for t in 1..=25 {
            let mut w = window(t * 1000, t == 4);
            if (9..=16).contains(&t) {
                w.process_cpu_cores = None;
            }
            assert_eq!(g.observe(w), None);
        }
        assert_eq!(g.target().fps, 30);
    }
    #[test]
    fn descends_by_one_step_and_resolution_only_after_fifteen() {
        let mut g = LoadGovernor::new(60, true, 0);
        let mut changes = Vec::new();
        for t in 1..=100 {
            if let Some(target) = g.observe(window(t * 1000, true)) {
                changes.push(target);
            }
        }
        assert_eq!(
            changes,
            vec![
                Target {
                    fps: 30,
                    resolution: ResolutionStep::Native
                },
                Target {
                    fps: 20,
                    resolution: ResolutionStep::Native
                },
                Target {
                    fps: 15,
                    resolution: ResolutionStep::Native
                },
                Target {
                    fps: 15,
                    resolution: ResolutionStep::Hd720
                }
            ]
        );
        assert!(changes.iter().all(|target| target.fps >= 15));
        assert_eq!(g.diagnostics(100_000).change_count, 4);
    }
    #[test]
    fn health_recovers_one_step_after_longer_hold() {
        let mut g = LoadGovernor::new(30, false, 0);
        for t in 1..=5 {
            g.observe(window(t * 1000, true));
        }
        assert_eq!(g.target().fps, 20);
        for t in 6..=20 {
            g.observe(window(t * 1000, false));
        }
        assert_eq!(g.target().fps, 30);
    }
    #[test]
    fn alternating_load_cannot_oscillate_beyond_rate_limit() {
        let mut g = LoadGovernor::new(60, true, 0);
        let mut changes = Vec::new();
        for t in 1..=180 {
            if g.observe(window(t * 1000, (t / 11) % 2 == 0)).is_some() {
                changes.push(t);
            }
        }
        for at in &changes {
            assert!(
                changes
                    .iter()
                    .filter(|other| **other >= *at && **other < *at + 60)
                    .count()
                    <= 3
            );
        }
    }
    #[test]
    fn missing_cpu_alone_never_degrades_and_fifteen_is_floor() {
        let mut g = LoadGovernor::new(15, false, 0);
        for t in 1..=70 {
            let mut w = window(t * 1000, t > 30);
            w.process_cpu_cores = None;
            w.system_cpu_percent = None;
            assert_eq!(g.observe(w), None);
        }
        assert_eq!(g.target().fps, 15);
    }
    #[test]
    fn timer_jitter_matches_ideal_and_suspension_resets_streak() {
        let mut ideal = LoadGovernor::new(60, true, 0);
        let mut jittered = LoadGovernor::new(60, true, 0);
        let mut elapsed = 0;
        let mut ideal_steps = Vec::new();
        let mut jittered_steps = Vec::new();
        for t in 1..=100 {
            elapsed += [997, 1004, 1012][(t as usize - 1) % 3];
            if let Some(step) = ideal.observe(window(t * 1000, true)) {
                ideal_steps.push(step);
            }
            if let Some(step) = jittered.observe(window(elapsed, true)) {
                jittered_steps.push(step);
            }
        }
        assert_eq!(jittered_steps, ideal_steps);

        let mut resumed = LoadGovernor::new(30, false, 0);
        assert_eq!(resumed.observe(window(1000, true)), None);
        assert_eq!(resumed.observe(window(2000, true)), None);
        assert_eq!(resumed.observe(window(7000, true)), None);
        assert_eq!(resumed.observe(window(8000, true)), None);
        assert_eq!(resumed.observe(window(9000, true)), None);
        assert_eq!(resumed.observe(window(10000, true)).unwrap().fps, 20);
    }
    #[test]
    fn process_cpu_is_measured_against_two_thread_budget() {
        let mut g = LoadGovernor::new(30, false, 0);
        for t in 1..=5 {
            let mut w = window(t * 1000, false);
            w.process_cpu_cores = Some(1.7);
            if t < 5 {
                assert_eq!(g.observe(w), None);
            } else {
                assert_eq!(g.observe(w).unwrap().fps, 20);
            }
        }
    }
    #[test]
    fn absent_system_cpu_allows_frame_overload_and_recovery() {
        let mut g = LoadGovernor::new(30, false, 0);
        for t in 1..=5 {
            let mut w = window(t * 1000, true);
            w.system_cpu_percent = None;
            g.observe(w);
        }
        assert_eq!(g.target().fps, 20);
        for t in 6..=20 {
            let mut w = window(t * 1000, false);
            w.system_cpu_percent = None;
            g.observe(w);
        }
        assert_eq!(g.target().fps, 30);
    }
}
