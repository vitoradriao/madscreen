use madscreen_encode::{
    AuBuffer, EncoderConfig, FrameKind,
    metrics::{LatencyStats, Samples},
};
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub struct FrameRow {
    pub index: u64,
    pub measured_index: u64,
    pub qpc_capture: i64,
    pub t_capture_convert_ms: f64,
    pub t_convert_ms: f64,
    pub t_encode_ms: f64,
    pub t_total_ms: f64,
    pub gen_ms: f64,
    pub convert_submit_ms: f64,
    pub convert_gpu_ms: Option<f64>,
    pub gen_gpu_ms: Option<f64>,
    pub queue_wait_ms: f64,
    pub encode_call_ms: f64,

    pub encode_wait_ms: f64,
    pub bytes: usize,
    pub key: bool,
    pub readback_submit_ms: Option<f64>,
    pub readback_map_ms: Option<f64>,
    pub readback_copy_ms: Option<f64>,
    pub codec_send_ms: Option<f64>,
}
impl FrameRow {
    pub fn from_au(au: &AuBuffer, measured_index: u64, frequency: i64) -> Self {
        let t = au.timing;
        let ms = |a: i64, b: i64| b.saturating_sub(a).max(0) as f64 * 1000. / frequency as f64;
        Self {
            index: au.index,
            measured_index,
            qpc_capture: t.qpc_capture,
            t_capture_convert_ms: ms(t.qpc_capture, t.qpc_convert_start),
            t_convert_ms: ms(t.qpc_convert_start, t.qpc_convert_end),
            t_encode_ms: ms(t.qpc_encode_start, t.qpc_encode_end),
            t_total_ms: ms(t.qpc_capture, t.qpc_encode_end),
            gen_ms: ms(t.qpc_capture, t.qpc_convert_start),
            convert_submit_ms: ms(t.qpc_convert_start, t.qpc_convert_end),
            convert_gpu_ms: t.convert_gpu_ms,
            gen_gpu_ms: t.gen_gpu_ms,
            queue_wait_ms: ms(t.qpc_convert_end, t.qpc_encode_start),
            readback_submit_ms: t.readback_submit_ms,
            readback_map_ms: t.readback_map_ms,
            readback_copy_ms: t.readback_copy_ms,
            codec_send_ms: t.codec_send_ms,
            encode_call_ms: ms(t.qpc_encode_start, t.qpc_encode_return),
            encode_wait_ms: ms(t.qpc_encode_start, t.qpc_encode_end),
            bytes: au.data.len(),
            key: au.kind == FrameKind::Key,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub encoder: &'static str,
    pub config: EncoderConfig,
    pub source: String,
    pub capture_backend: Option<&'static str>,
    pub convert: &'static str,
    pub input: &'static str,
    pub measured_seconds: f64,
    pub encoded: u64,
    pub discarded: u64,
    pub pacing_intervals_skipped: u64,
    pub captured: Option<u64>,
    pub encode_fps: f64,
    pub sustained_fps: f64,
    pub throughput_acceptance: &'static str,
    pub target_bitrate_mbps: f64,
    pub bitrate_note: Option<String>,
    pub gen_ms: LatencyStats,
    pub convert_submit_ms: LatencyStats,
    pub convert_gpu_ms: LatencyStats,
    pub gen_gpu_ms: LatencyStats,
    pub queue_wait_ms: LatencyStats,
    pub encode_call_ms: LatencyStats,
    pub readback_submit_ms: LatencyStats,
    pub readback_map_ms: LatencyStats,
    pub readback_copy_ms: LatencyStats,
    pub codec_send_ms: LatencyStats,

    pub encode_wait_ms: LatencyStats,
    pub inflight_limit: u32,
    pub gpu_metrics_unavailable: u64,
    pub independent_gpu_metrics: bool,
    pub gpu_query_delay_ms: u64,
    pub reduction_filter: &'static str,
    pub inflight_mean: f64,
    pub inflight_max: usize,
    pub pacer_late_frames: u64,
    pub pipeline_full_drops: u64,
    pub pipeline_blocked_episodes: Option<u64>,
    pub pipeline_blocked_ms: Option<f64>,
    pub capture_throttled_notifications: u64,
    pub sync: String,
    pub synthetic: String,
    pub gpu_priority: serde_json::Value,
    pub gpu_percent_mean: Option<f64>,
    pub gpu_percent_min: Option<f64>,
    pub gpu_percent_max: Option<f64>,
    pub encoder_percent_mean: Option<f64>,
    pub gpu_warning: Option<String>,
    pub capture_fps: Option<f64>,
    /// Diferença QPC na chegada menos `SystemRelativeTime` do WGC (ms) e frames com timestamp no futuro.
    pub capture_clock_offset: Option<serde_json::Value>,
    pub encode_ms: LatencyStats,
    pub total_ms: LatencyStats,
    pub convert_ms: LatencyStats,
    pub capture_convert_ms: LatencyStats,
    pub bitrate_mean_mbps: Option<f64>,
    pub bitrate_p95_mbps: Option<f64>,
    pub keyframes: u64,
    pub statistics_window_frames: usize,
    pub acceptance_m02: &'static str,
    pub no_readback: &'static str,
    pub gpu_samples: serde_json::Value,
}
pub const NO_READBACK: &str = "Sem readback de pixels garantido pelo código (nenhum Map/staging); conferir engines Video Encode/Copy, PCIe e CPU no PresentMon/Task Manager/ETW.";
pub struct Measurements {
    encode: Samples,
    recent_encode: Samples,
    total: Samples,
    convert: Samples,
    capture_convert: Samples,
    bitrate: Samples,
    gen_gpu: Samples,
    convert_gpu: Samples,
    queue_wait: Samples,
    encode_call: Samples,
    readback_submit_ms: Samples,
    readback_map_ms: Samples,
    readback_copy_ms: Samples,
    codec_send_ms: Samples,

    pub frames: u64,
    pub bytes: u64,
    pub keyframes: u64,
    bytes_window: u64,
    last_bitrate_time: f64,
}
impl Default for Measurements {
    fn default() -> Self {
        Self::new()
    }
}
impl Measurements {
    pub fn new() -> Self {
        Self {
            encode: Samples::new(262144),
            recent_encode: Samples::new(4096),
            total: Samples::new(262144),
            convert: Samples::new(262144),
            capture_convert: Samples::new(262144),
            bitrate: Samples::new(3600),
            gen_gpu: Samples::new(262144),
            convert_gpu: Samples::new(262144),
            queue_wait: Samples::new(262144),
            encode_call: Samples::new(262144),
            readback_submit_ms: Samples::new(262144),
            readback_map_ms: Samples::new(262144),
            readback_copy_ms: Samples::new(262144),
            codec_send_ms: Samples::new(262144),

            frames: 0,
            bytes: 0,
            keyframes: 0,
            bytes_window: 0,
            last_bitrate_time: 0.,
        }
    }
    pub fn record(&mut self, row: &FrameRow, elapsed: f64) {
        if let Some(value) = row.gen_gpu_ms {
            self.gen_gpu.record(value);
        }
        if let Some(value) = row.convert_gpu_ms {
            self.convert_gpu.record(value);
        }
        if let Some(value) = row.readback_submit_ms {
            self.readback_submit_ms.record(value);
        }
        if let Some(value) = row.readback_map_ms {
            self.readback_map_ms.record(value);
        }
        if let Some(value) = row.readback_copy_ms {
            self.readback_copy_ms.record(value);
        }
        if let Some(value) = row.codec_send_ms {
            self.codec_send_ms.record(value);
        }
        self.queue_wait.record(row.queue_wait_ms);
        self.encode_call.record(row.encode_call_ms);
        self.frames += 1;
        self.bytes += row.bytes as u64;
        self.bytes_window += row.bytes as u64;
        self.keyframes += u64::from(row.key);
        self.encode.record(row.t_encode_ms);
        self.recent_encode.record(row.t_encode_ms);
        self.total.record(row.t_total_ms);
        self.convert.record(row.t_convert_ms);
        self.capture_convert.record(row.t_capture_convert_ms);
        let seconds = elapsed - self.last_bitrate_time;
        if seconds >= 1. {
            self.bitrate
                .record(self.bytes_window as f64 * 8. / seconds / 1e6);
            self.bytes_window = 0;
            self.last_bitrate_time = elapsed;
        }
    }
    pub fn encode_stats(&self) -> LatencyStats {
        self.recent_encode.summary()
    }
    #[allow(clippy::too_many_arguments)]
    pub fn report(
        &self,
        config: EncoderConfig,
        source: String,
        convert: &'static str,
        input: &'static str,
        elapsed: f64,
        discarded: u64,
        skipped: u64,
        captured: Option<u64>,
    ) -> Report {
        let measured = elapsed > 0.;
        let acceptance = if config.width != 1920
            || config.height != 1080
            || u64::from(config.fps.num) != 144 * u64::from(config.fps.den)
        {
            "N/A (exige 1080p144)"
        } else if self.frames == 0 {
            "SEM AMOSTRAS"
        } else if self.encode.summary().p95.is_some_and(|v| v <= 5.) {
            "PASS (latência de encode; aceite completo exige GPU/bench)"
        } else {
            "FAIL"
        };
        let sustained_fps = if measured {
            self.frames as f64 / elapsed
        } else {
            0.
        };
        let encode_fps = sustained_fps;
        let target_fps = config.fps.as_f64();
        let target_bitrate_mbps = f64::from(config.bitrate_bps) / 1e6;
        let throughput_acceptance = if self.frames == 0 {
            "SEM AMOSTRAS"
        } else if encode_fps >= 0.97 * target_fps {
            "PASS"
        } else {
            "FAIL"
        };
        Report {
            readback_submit_ms: self.readback_submit_ms.summary(),
            readback_map_ms: self.readback_map_ms.summary(),
            readback_copy_ms: self.readback_copy_ms.summary(),
            codec_send_ms: self.codec_send_ms.summary(),
            encoder: "NVIDIA NVENC",
            config,
            source,
            capture_backend: None,
            convert,
            input,
            measured_seconds: elapsed,
            encoded: self.frames,
            discarded,
            pacing_intervals_skipped: skipped,
            captured,
            encode_fps,
            sustained_fps,
            throughput_acceptance,
            target_bitrate_mbps,
            bitrate_note: (self.frames > 0 && sustained_fps < target_fps * 0.97).then(|| "Menos frames que o FPS configurado reduzem o bitrate real por segundo; compare também o conteúdo e o controle CBR do driver.".into()),
            gen_ms: self.capture_convert.summary(),
            convert_submit_ms: self.convert.summary(),
            gen_gpu_ms: self.gen_gpu.summary(),
            convert_gpu_ms: self.convert_gpu.summary(),
            queue_wait_ms: self.queue_wait.summary(),
            encode_call_ms: self.encode_call.summary(),
            encode_wait_ms: self.encode.summary(),
            inflight_limit: 0, gpu_metrics_unavailable: 0, independent_gpu_metrics: false, gpu_query_delay_ms: 0, reduction_filter: "bilinear", inflight_mean: 0., inflight_max: 0, pacer_late_frames: 0, pipeline_full_drops: 0, pipeline_blocked_episodes: None, pipeline_blocked_ms: None, capture_throttled_notifications: 0,
            sync: String::new(), synthetic: String::new(), gpu_priority: serde_json::Value::Null,
            gpu_percent_mean: None, gpu_percent_min: None, gpu_percent_max: None, encoder_percent_mean: None, gpu_warning: None,
            capture_fps: captured.filter(|_| measured).map(|n| n as f64 / elapsed),
            capture_clock_offset: None,
            encode_ms: self.encode.summary(),
            total_ms: self.total.summary(),
            convert_ms: self.convert.summary(),
            capture_convert_ms: self.capture_convert.summary(),
            bitrate_mean_mbps: measured.then_some(self.bytes as f64 * 8. / elapsed / 1e6),
            bitrate_p95_mbps: self.bitrate.summary().p95,
            keyframes: self.keyframes,
            statistics_window_frames: self.encode.values().len(),
            acceptance_m02: acceptance,
            no_readback: NO_READBACK,
            gpu_samples: serde_json::Value::Null,
        }
    }
}
pub struct Metric(pub Option<f64>);
impl std::fmt::Display for Metric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(value) => write!(f, "{value:.3}"),
            None => f.write_str("indisponível"),
        }
    }
}
impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{}: {:?} {:?} {}x{} {:.3} FPS; conversão={}, entrada={}",
            self.encoder,
            self.config.codec,
            self.config.preset,
            self.config.width,
            self.config.height,
            self.config.fps.as_f64(),
            self.convert,
            self.input
        )?;
        writeln!(
            f,
            "Frames: {} codificados, {} descartados; encode {:.2} FPS; captura {} FPS; intervalos pulados {} (deadlines vencidos omitidos, sem rajada de recuperação)",
            self.encoded,
            self.discarded,
            self.encode_fps,
            Metric(self.capture_fps),
            self.pacing_intervals_skipped
        )?;
        writeln!(
            f,
            "FPS sustentado (frames/duração): {:.2}; throughput >= 97% do pedido: {}",
            self.sustained_fps, self.throughput_acceptance
        )?;
        for (name, stats) in [
            ("gen", &self.gen_ms),
            ("convert_submit", &self.convert_submit_ms),
            ("gen_gpu", &self.gen_gpu_ms),
            ("convert_gpu", &self.convert_gpu_ms),
            ("queue_wait", &self.queue_wait_ms),
            ("encode_call", &self.encode_call_ms),
            ("encode_wait/encode", &self.encode_wait_ms),
            ("total", &self.total_ms),
        ] {
            writeln!(
                f,
                "{name} ms p50/p95/p99/max: {}/{}/{}/{}",
                Metric(stats.p50),
                Metric(stats.p95),
                Metric(stats.p99),
                Metric(stats.max)
            )?;
        }
        writeln!(
            f,
            "Em voo média temporal/máximo: {:.2}/{} (limite {}); frames aceitos atrasados pelo pacer: {}; descartes por lotação: {}; notificações retidas pelo intervalo mínimo de captura: {}",
            self.inflight_mean,
            self.inflight_max,
            self.inflight_limit,
            self.pacer_late_frames,
            self.pipeline_full_drops,
            self.capture_throttled_notifications
        )?;
        writeln!(
            f,
            "Sincronização: {}; synthetic: {}; prioridade GPU: {}",
            self.sync, self.synthetic, self.gpu_priority
        )?;
        writeln!(
            f,
            "GPU média/mín/máx: {}/{}/{}%; encoder médio: {}%",
            Metric(self.gpu_percent_mean),
            Metric(self.gpu_percent_min),
            Metric(self.gpu_percent_max),
            Metric(self.encoder_percent_mean)
        )?;
        if let Some(warning) = &self.gpu_warning {
            writeln!(f, "{warning}")?;
        }
        writeln!(f, "Bitrate alvo: {:.3} Mbps", self.target_bitrate_mbps)?;
        if let Some(note) = &self.bitrate_note {
            writeln!(f, "{note}")?;
        }
        writeln!(
            f,
            "Bitrate médio/p95 (janelas de 1 s) Mbps: {}/{}; keyframes: {}",
            Metric(self.bitrate_mean_mbps),
            Metric(self.bitrate_p95_mbps),
            self.keyframes
        )?;
        writeln!(
            f,
            "Aceite M0.2 encode p95 <= 5 ms (1080p144): {}",
            self.acceptance_m02
        )?;
        writeln!(f, "{}", self.no_readback)
    }
}

/// Acumula toda a medição, independente da janela limitada de snapshots.
#[derive(Default)]
pub struct UtilizationSummary {
    gpu_count: u64,
    gpu_sum: u64,
    gpu_min: Option<u32>,
    gpu_max: Option<u32>,
    encoder_count: u64,
    encoder_sum: u64,
}
impl UtilizationSummary {
    pub fn record(&mut self, gpu: Option<u32>, encoder: Option<u32>) {
        if let Some(value) = gpu {
            self.gpu_count += 1;
            self.gpu_sum += u64::from(value);
            self.gpu_min = Some(self.gpu_min.map_or(value, |previous| previous.min(value)));
            self.gpu_max = Some(self.gpu_max.map_or(value, |previous| previous.max(value)));
        }
        if let Some(value) = encoder {
            self.encoder_count += 1;
            self.encoder_sum += u64::from(value);
        }
    }
}
impl Report {
    /// Somente a GPU selecionada, durante a medição e antes da drenagem.
    pub fn set_gpu_utilization(&mut self, summary: &UtilizationSummary) {
        self.gpu_percent_mean =
            (summary.gpu_count > 0).then(|| summary.gpu_sum as f64 / summary.gpu_count as f64);
        self.gpu_percent_min = summary.gpu_min.map(f64::from);
        self.gpu_percent_max = summary.gpu_max.map(f64::from);
        self.encoder_percent_mean = (summary.encoder_count > 0)
            .then(|| summary.encoder_sum as f64 / summary.encoder_count as f64);
        self.gpu_warning = self.gpu_percent_mean.filter(|&mean| mean < 50.).map(|mean| format!("AVISO: GPU media {mean:.1} % — medicao sem o jogo carregando a GPU nao e representativa"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn throughput_threshold_is_inclusive_and_independent_of_latency() -> anyhow::Result<()> {
        let mut measurements = Measurements::new();
        let au = AuBuffer::new(0);
        let row = FrameRow::from_au(&au, 0, 1000);
        for n in 0..97 {
            measurements.record(&row, n as f64);
        }
        let config = EncoderConfig {
            fps: madscreen_encode::FrameRate::new(100, 1)?,
            ..Default::default()
        };
        let at_limit = measurements.report(
            config.clone(),
            "synthetic".into(),
            "shader",
            "nv12",
            1.,
            0,
            0,
            None,
        );
        assert_eq!(at_limit.encode_fps, 97.);
        assert_eq!(at_limit.sustained_fps, 97.);
        assert_eq!(at_limit.throughput_acceptance, "PASS");
        let below_limit = measurements.report(
            config,
            "synthetic".into(),
            "shader",
            "nv12",
            97. / 96.9,
            0,
            0,
            None,
        );
        assert_eq!(below_limit.throughput_acceptance, "FAIL");
        Ok(())
    }
    #[test]
    fn pipeline_percentiles_throughput_and_gpu_warning() -> anyhow::Result<()> {
        let mut measurements = Measurements::new();
        let mut au = AuBuffer::new(8);
        for n in 1..=100 {
            au.timing = madscreen_encode::FrameTiming {
                qpc_capture: n * 1000,
                qpc_convert_start: n * 1000 + n,
                qpc_convert_end: n * 1000 + n * 2,
                qpc_encode_start: n * 1000 + n * 3,
                qpc_encode_return: n * 1000 + n * 4,
                qpc_encode_end: n * 1000 + n * 5,
                gen_gpu_ms: Some(n as f64),
                convert_gpu_ms: Some(2. * n as f64),
                ..Default::default()
            };
            measurements.record(&FrameRow::from_au(&au, n as u64, 1000), n as f64);
        }
        let mut report = measurements.report(
            EncoderConfig::default(),
            "synthetic".into(),
            "shader",
            "nv12",
            100.,
            0,
            0,
            None,
        );
        assert_eq!(report.gen_ms.p50, Some(50.5));
        for stats in [
            &report.gen_ms,
            &report.convert_submit_ms,
            &report.queue_wait_ms,
            &report.encode_call_ms,
        ] {
            assert_eq!(stats.p95, Some(95.05));
            assert_eq!(stats.p99, Some(99.01));
            assert_eq!(stats.max, Some(100.));
        }
        assert_eq!(report.convert_gpu_ms.p50, Some(101.));
        assert_eq!(report.encode_wait_ms.p50, Some(101.));
        assert_eq!(report.total_ms.p50, Some(252.5));
        assert_eq!(report.sustained_fps, 1.);
        assert_eq!(report.encode_fps, 1.);
        assert_eq!(report.throughput_acceptance, "FAIL");
        assert!(report.bitrate_note.is_some());
        let mut summary = UtilizationSummary::default();
        summary.record(Some(30), Some(8));
        summary.record(Some(40), Some(10));
        summary.record(Some(50), None);
        report.set_gpu_utilization(&summary);
        assert_eq!(report.gpu_percent_mean, Some(40.));
        assert_eq!(report.gpu_percent_min, Some(30.));
        assert_eq!(report.gpu_percent_max, Some(50.));
        assert_eq!(report.encoder_percent_mean, Some(9.));
        assert!(
            report
                .gpu_warning
                .as_ref()
                .is_some_and(|s| s.contains("AVISO: GPU media 40.0 %"))
        );
        let mut summary = UtilizationSummary::default();
        summary.record(Some(50), None);
        summary.record(Some(90), None);
        report.set_gpu_utilization(&summary);
        assert!(report.gpu_warning.is_none());
        report.set_gpu_utilization(&UtilizationSummary::default());
        assert_eq!(report.gpu_percent_mean, None);
        assert!(report.gpu_warning.is_none());
        Ok(())
    }
    #[test]
    fn report_json_csv_and_empty() -> anyhow::Result<()> {
        let mut m = Measurements::new();
        let empty = m.report(
            EncoderConfig::default(),
            "synthetic".into(),
            "shader",
            "nv12",
            0.,
            0,
            0,
            None,
        );
        assert!(serde_json::to_string(&empty)?.contains("SEM AMOSTRAS"));
        let mut au = AuBuffer::new(1024);
        au.copy_from(&[1, 2, 3])?;
        au.kind = FrameKind::Key;
        au.timing = madscreen_encode::FrameTiming {
            qpc_capture: 0,
            qpc_convert_start: 10,
            qpc_convert_end: 20,
            qpc_encode_start: 20,
            qpc_encode_end: 40,
            qpc_encode_return: 25,
            ..Default::default()
        };
        let row = FrameRow::from_au(&au, 0, 10000);
        assert_eq!(row.t_encode_ms, 2.);
        assert_eq!(m.encode_stats().p95, None);
        assert_eq!(row.t_total_ms, 4.);
        m.record(&row, 1.);
        let report = m.report(
            EncoderConfig::default(),
            "synthetic".into(),
            "shader",
            "nv12",
            1.,
            2,
            0,
            None,
        );
        assert!(report.acceptance_m02.starts_with("PASS"));
        assert_eq!(report.encode_fps, 1.);
        assert_eq!(report.keyframes, 1);
        let json = serde_json::to_value(&report)?;
        assert_eq!(json["encode_ms"]["p95"], 2.);
        assert!(report.to_string().contains("Aceite M0.2"));
        assert!(report.to_string().contains("2.000/2.000/2.000/2.000"));
        assert!(empty.to_string().contains("indisponível"));
        let slow = EncoderConfig {
            fps: madscreen_encode::FrameRate::new(1, u32::MAX)?,
            ..Default::default()
        };
        let slow_report = m.report(slow, "synthetic".into(), "shader", "nv12", 1., 0, 0, None);
        assert!(slow_report.acceptance_m02.starts_with("N/A"));
        let mut csv = csv::Writer::from_writer(Vec::new());
        csv.serialize(&row)?;
        let bytes = csv.into_inner()?;
        let text = String::from_utf8(bytes)?;
        assert!(text.starts_with("index,measured_index,qpc_capture,"));
        assert!(text.contains("convert_submit_ms,convert_gpu_ms,gen_gpu_ms,queue_wait_ms,encode_call_ms,encode_wait_ms,bytes,key"));
        Ok(())
    }
}
