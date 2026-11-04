use madscreen_telemetry::stats;
use serde::Serialize;
#[derive(Debug, Clone, Default, Serialize)]
pub struct LatencyStats {
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    pub max: Option<f64>,
}
impl LatencyStats {
    pub fn from_samples(samples: &[f64]) -> Self {
        Self {
            p50: stats::percentile(samples, 0.50),
            p95: stats::percentile(samples, 0.95),
            p99: stats::percentile(samples, 0.99),
            max: stats::percentile(samples, 1.0),
        }
    }
}
/// Janela circular fixa: processo contínuo não cresce em memória.
pub struct Samples {
    values: Box<[f64]>,
    len: usize,
    next: usize,
}
impl Samples {
    pub fn new(capacity: usize) -> Self {
        Self {
            values: vec![0.; capacity.max(1)].into_boxed_slice(),
            len: 0,
            next: 0,
        }
    }
    pub fn record(&mut self, value: f64) {
        if !value.is_finite() || value < 0. {
            return;
        }
        self.values[self.next] = value;
        self.next = (self.next + 1) % self.values.len();
        self.len = (self.len + 1).min(self.values.len());
    }
    pub fn values(&self) -> &[f64] {
        &self.values[..self.len]
    }
    pub fn summary(&self) -> LatencyStats {
        LatencyStats::from_samples(self.values())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_percentiles() {
        let mut s = Samples::new(100);
        for n in 1..=101 {
            s.record(n as f64);
        }
        s.record(f64::NAN);
        s.record(-1.);
        assert_eq!(s.summary().p50, Some(51.5));
        assert_eq!(s.summary().max, Some(101.));
        assert_eq!(Samples::new(0).summary().p95, None);
    }
}
