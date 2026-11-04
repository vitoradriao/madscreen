use serde::{Deserialize, Serialize};

/// Só valores finitos são aceitos pelas funções estatísticas.
pub fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() || values.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let result = values.iter().map(|v| v / values.len() as f64).sum::<f64>();
    result.is_finite().then_some(result)
}

pub fn sample_std_dev(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let avg = mean(values)?;
    let variance =
        values.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / (values.len() - 1) as f64;
    let result = variance.sqrt();
    result.is_finite().then_some(result)
}

pub fn cv_percent(values: &[f64]) -> Option<f64> {
    let avg = mean(values)?;
    (avg > 0.0).then_some(sample_std_dev(values)? / avg * 100.0)
}

/// Interpolação linear: índice (n - 1) * p, p em [0, 1] (tipo 7).
pub fn percentile(values: &[f64], p: f64) -> Option<f64> {
    if values.is_empty() || !(0.0..=1.0).contains(&p) || values.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = (sorted.len() - 1) as f64 * p;
    let lower = index.floor() as usize;
    let upper = index.ceil() as usize;
    Some(sorted[lower] + (sorted[upper] - sorted[lower]) * index.fract())
}

/// FPS dos ceil(n * fraction) frames mais lentos, mínimo 1; não é percentil de FPS.
pub fn low_fps(frame_ms: &[f64], fraction: f64) -> Option<f64> {
    if frame_ms.is_empty()
        || !(0.0 < fraction && fraction <= 1.0)
        || frame_ms.iter().any(|v| !v.is_finite() || *v <= 0.0)
    {
        return None;
    }
    let mut sorted = frame_ms.to_vec();
    sorted.sort_by(|a, b| b.total_cmp(a));
    let count = ((sorted.len() as f64 * fraction).ceil() as usize).max(1);
    Some(1000.0 / mean(&sorted[..count])?)
}

/// t bilateral 0,975, df 1..30. Para df > 30 usa aproximação normal 1,96.
pub fn t_critical(df: usize) -> Option<f64> {
    const T: [f64; 30] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
        2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
        2.052, 2.048, 2.045, 2.042,
    ];
    match df {
        0 => None,
        1..=30 => Some(T[df - 1]),
        _ => Some(1.96),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Estimate {
    pub n: usize,
    pub mean: f64,
    pub std_dev: Option<f64>,
    pub ci95_half_width: Option<f64>,
}

impl Estimate {
    pub fn from_values(values: &[f64]) -> Option<Self> {
        let avg = mean(values)?;
        let std_dev = sample_std_dev(values);
        let ci95_half_width = std_dev
            .zip(t_critical(values.len() - 1))
            .map(|(sd, t)| t * sd / (values.len() as f64).sqrt());
        Some(Self {
            n: values.len(),
            mean: avg,
            std_dev,
            ci95_half_width,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameStats {
    pub frames: usize,
    pub duration_sec: f64,
    pub average_fps: f64,
    pub low_1_fps: f64,
    pub low_01_fps: f64,
    pub p99_frame_ms: f64,
    pub mean_gpu_busy_ms: Option<f64>,
}

pub fn frame_stats(frame_ms: &[f64], gpu_busy_ms: &[f64]) -> Option<FrameStats> {
    if frame_ms.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return None;
    }
    let avg = mean(frame_ms)?;
    let duration_sec = frame_ms.iter().sum::<f64>() / 1000.0;
    if !duration_sec.is_finite() || duration_sec <= 0.0 {
        return None;
    }
    Some(FrameStats {
        frames: frame_ms.len(),
        duration_sec,
        average_fps: 1000.0 / avg,
        low_1_fps: low_fps(frame_ms, 0.01)?,
        low_01_fps: low_fps(frame_ms, 0.001)?,
        p99_frame_ms: percentile(frame_ms, 0.99)?,
        mean_gpu_busy_ms: mean(gpu_busy_ms),
    })
}
