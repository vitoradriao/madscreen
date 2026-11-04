//! Contratos aditivos de diagnóstico: somente contadores e tempos locais, sem identificadores RTC.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IceSelection {
    Selected,
    UniqueNominated,
    Ambiguous,
    #[default]
    Unavailable,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityLimitation {
    #[default]
    None,
    Cpu,
    Bandwidth,
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SenderStats {
    pub segment: u64,
    /// Identifica a coleta RTC, sem deduzir novidade de relógios arredondados.
    #[serde(default)]
    pub sample_sequence: u64,
    pub sample_age_ms: u64,
    pub frames_encoded: u64,
    pub frames_sent: u64,
    pub key_frames_encoded: u64,
    pub bytes_sent: u64,
    pub packets_sent: u64,
    /// Cumulative packet send delay in seconds; unavailable/SDK default zero stays None.
    #[serde(default)]
    pub total_packet_send_delay_seconds: Option<f64>,
    pub pli: u64,
    pub fir: u64,
    pub nack: u64,
    pub target_bitrate_bps: Option<f64>,
    pub available_outgoing_bitrate_bps: Option<f64>,
    pub rtt_ms: Option<f64>,
    pub ice_selection: IceSelection,
    pub limitation: QualityLimitation,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PublisherSample {
    pub schema: u32,
    /// Nonce local da execução, sem identidade, processo, sala ou credencial.
    pub run_id: String,
    pub at_ms: u64,
    pub provider: String,
    pub frames_submitted: u64,
    pub frames_completed: u64,
    pub captured_frames: Option<u64>,
    pub capture_overwritten: Option<u64>,
    pub capture_contention_drops: Option<u64>,
    /// Legacy synthetic opportunities; not WGC/DDA lost frames.
    pub pipeline_full_drops: u64,
    /// Cumulative episodes since warmup; None means unavailable coverage.
    #[serde(default)]
    pub pipeline_blocked_episodes: Option<u64>,
    /// Cumulative observed milliseconds, including the ongoing episode.
    #[serde(default)]
    pub pipeline_blocked_ms: Option<f64>,
    /// Unavailable/disjoint/skipped GPU telemetry rows, cumulative since warmup.
    #[serde(default)]
    pub gpu_metrics_unavailable: Option<u64>,
    #[serde(default)]
    pub gpu_metrics_independent: Option<bool>,
    pub pacing_intervals_skipped: u64,
    pub inflight: usize,
    pub bytes_produced: u64,
    pub keyframes_produced: u64,
    pub last_au_bytes: usize,
    pub max_au_bytes: usize,
    pub capture_to_au_max_ms: f64,
    #[serde(default)]
    pub capture_to_convert_max_ms: Option<f64>,
    /// Submissão CPU da conversão; não é tempo de execução GPU.
    #[serde(default)]
    pub convert_submit_max_ms: Option<f64>,
    #[serde(default)]
    pub queue_wait_max_ms: Option<f64>,
    pub encode_wait_max_ms: f64,
    pub pushed: u64,
    pub rejected: u64,
    pub audio_pushed: u64,
    pub audio_rejected: u64,
    pub audio_dropped_samples: Option<u64>,
    pub rate_requests: u64,
    pub rate_applied: u64,
    pub rate_busy: u64,
    pub requested_bitrate_bps: Option<u64>,
    pub applied_bitrate_bps: u32,
    pub reconfigure_max_ms: f64,
    pub sender: Option<SenderStats>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublisherReport {
    pub schema: u32,
    pub app_version: String,
    pub samples: Vec<PublisherSample>,
}

/// Um histórico por execução. Não permite que contadores de duas lives sejam combinados.
pub struct PublisherHistory(std::collections::VecDeque<PublisherSample>);

impl Default for PublisherHistory {
    fn default() -> Self {
        Self(std::collections::VecDeque::with_capacity(120))
    }
}

impl PublisherHistory {
    pub fn record(&mut self, sample: PublisherSample) {
        if sample.schema != 1
            || sample.run_id.len() != 16
            || !sample.run_id.bytes().all(|b| b.is_ascii_hexdigit())
            || !matches!(sample.provider.as_str(), "cloudflare" | "livekit")
        {
            return;
        }
        if let Some(last) = self.0.back() {
            if last.run_id != sample.run_id || last.provider != sample.provider {
                self.0.clear();
            } else if sample.at_ms <= last.at_ms {
                return;
            }
        }
        while self
            .0
            .front()
            .is_some_and(|first| sample.at_ms.saturating_sub(first.at_ms) > 120_000)
        {
            self.0.pop_front();
        }
        if self.0.len() == 120 {
            self.0.pop_front();
        }
        self.0.push_back(sample);
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn report(&self, app_version: String) -> PublisherReport {
        PublisherReport {
            schema: 1,
            app_version,
            samples: self.0.iter().cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_samples_leave_saturation_unavailable() -> Result<(), Box<dyn std::error::Error>> {
        let mut value = serde_json::to_value(PublisherSample::default())?;
        let object = value.as_object_mut().ok_or("sample object")?;
        object.remove("pipeline_blocked_episodes");
        object.remove("pipeline_blocked_ms");
        let old: PublisherSample = serde_json::from_value(value)?;
        assert_eq!(old.pipeline_blocked_episodes, None);
        assert_eq!(old.pipeline_blocked_ms, None);
        let new = PublisherSample {
            pipeline_blocked_episodes: Some(0),
            pipeline_blocked_ms: Some(0.),
            ..old
        };
        let roundtrip: PublisherSample = serde_json::from_str(&serde_json::to_string(&new)?)?;
        assert_eq!(roundtrip.pipeline_blocked_ms, Some(0.));
        Ok(())
    }
    #[test]
    fn history_is_bounded_and_separates_runs() {
        let mut history = PublisherHistory::default();
        let mut sample = PublisherSample {
            schema: 1,
            run_id: "0123456789abcdef".into(),
            provider: "cloudflare".into(),
            ..Default::default()
        };
        for i in 0..130 {
            sample.at_ms = i * 1000;
            history.record(sample.clone());
        }
        let report = history.report("test".into());
        assert_eq!(report.samples.len(), 120);
        assert_eq!(report.samples[0].at_ms, 10_000);
        sample.at_ms = 0;
        history.record(sample.clone());
        assert_eq!(history.report("test".into()).samples.len(), 120);
        sample.run_id = "abcdef0123456789".into();
        history.record(sample.clone());
        assert_eq!(history.report("test".into()).samples.len(), 1);
        sample.at_ms = 130_000;
        history.record(sample);
        assert_eq!(history.report("test".into()).samples.len(), 1);
        history.clear();
        assert!(history.report("test".into()).samples.is_empty());
    }
    #[test]
    fn additive_status_accepts_old_engines_and_keeps_optional_network_data()
    -> Result<(), Box<dyn std::error::Error>> {
        let old = r#"{"event":"status","fps":60,"bitrate_bps":12000000,"frames":60,"dropped":0}"#;
        let event: crate::Event = serde_json::from_str(old)?;
        assert!(matches!(
            event,
            crate::Event::Status {
                diagnostic_sample: None,
                ..
            }
        ));
        let sample = PublisherSample {
            schema: 1,
            sender: Some(SenderStats::default()),
            ..Default::default()
        };
        let event = crate::Event::Status {
            fps: 60.,
            bitrate_bps: 12_000_000,
            frames: 60,
            dropped: 0,
            publishing_connected: Some(true),
            diagnostic_sample: Some(Box::new(sample)),
        };
        let json = serde_json::to_string(&event)?;
        assert!(json.contains("\"rtt_ms\":null"));
        assert_eq!(serde_json::from_str::<crate::Event>(&json)?, event);
        Ok(())
    }

    #[test]
    fn prior_diagnostic_schema_and_legacy_status_reader_remain_compatible()
    -> Result<(), Box<dyn std::error::Error>> {
        #[derive(Deserialize)]
        #[serde(tag = "event", rename_all = "snake_case")]
        enum LegacyEvent {
            Status {
                fps: f64,
                bitrate_bps: u64,
                frames: u64,
                dropped: u64,
                publishing_connected: Option<bool>,
            },
        }
        let mut sample = PublisherSample {
            schema: 1,
            run_id: "0123456789abcdef".into(),
            provider: "livekit".into(),
            sender: Some(SenderStats::default()),
            ..Default::default()
        };
        let mut json = serde_json::to_value(&sample)?;
        for field in [
            "capture_to_convert_max_ms",
            "convert_submit_max_ms",
            "queue_wait_max_ms",
        ] {
            json.as_object_mut().ok_or("objeto")?.remove(field);
        }
        json.get_mut("sender")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or("sender")?
            .remove("sample_sequence");
        let old: PublisherSample = serde_json::from_value(json)?;
        assert_eq!(old.capture_to_convert_max_ms, None);
        assert_eq!(old.sender.ok_or("sender")?.sample_sequence, 0);
        sample.convert_submit_max_ms = Some(0.4);
        let event = crate::Event::Status {
            fps: 60.,
            bitrate_bps: 12_000_000,
            frames: 60,
            dropped: 0,
            publishing_connected: Some(true),
            diagnostic_sample: Some(Box::new(sample)),
        };
        let legacy: LegacyEvent = serde_json::from_value(serde_json::to_value(event)?)?;
        let LegacyEvent::Status {
            fps,
            bitrate_bps,
            frames,
            dropped,
            publishing_connected,
        } = legacy;
        assert_eq!(
            (fps, bitrate_bps, frames, dropped, publishing_connected),
            (60., 12_000_000, 60, 0, Some(true))
        );
        Ok(())
    }

    #[test]
    fn provider_and_run_changes_reset_history_and_invalid_identity_is_rejected() {
        let mut history = PublisherHistory::default();
        let mut sample = PublisherSample {
            schema: 1,
            run_id: "0123456789abcdef".into(),
            provider: "livekit".into(),
            ..Default::default()
        };
        history.record(sample.clone());
        sample.provider = "cloudflare".into();
        sample.at_ms = 1000;
        history.record(sample.clone());
        assert_eq!(history.report("test".into()).samples.len(), 1);
        sample.run_id = "private-room-name".into();
        history.record(sample);
        let json = serde_json::to_string(&history.report("test".into()));
        assert!(json.is_ok_and(|text| !text.contains("private-room-name")));
    }
}
