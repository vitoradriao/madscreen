//! Estatísticas de publicação compartilhadas pelos adaptadores, sem IDs na saída.
use libwebrtc::stats::{CandidatePairStats, IceCandidatePairState, OutboundRtpStats, RtcStats};
use madscreen_ipc::diagnostics::{IceSelection, QualityLimitation, SenderStats};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

fn video(stats: &[RtcStats]) -> Option<&OutboundRtpStats> {
    let mut tracks = stats.iter().filter_map(|stat| match stat {
        RtcStats::OutboundRtp(track) if track.stream.kind == "video" => Some(track),
        _ => None,
    });
    let track = tracks.next()?;
    tracks.next().is_none().then_some(track)
}

fn valid(pair: &CandidatePairStats) -> bool {
    pair.candidate_pair
        .state
        .is_none_or(|state| state == IceCandidatePairState::Succeeded)
}

fn pair<'a>(
    stats: &'a [RtcStats],
    track: &OutboundRtpStats,
) -> (Option<&'a CandidatePairStats>, IceSelection) {
    let transport = stats.iter().find_map(|stat| match stat {
        RtcStats::Transport(t)
            if !track.stream.transport_id.is_empty() && t.rtc.id == track.stream.transport_id =>
        {
            Some(t)
        }
        _ => None,
    });
    if let Some(transport) =
        transport.filter(|t| !t.transport.selected_candidate_pair_id.is_empty())
    {
        let selected = stats.iter().find_map(|stat| match stat {
            RtcStats::CandidatePair(p)
                if p.rtc.id == transport.transport.selected_candidate_pair_id
                    && valid(p)
                    && (p.candidate_pair.transport_id.is_empty()
                        || p.candidate_pair.transport_id == transport.rtc.id) =>
            {
                Some(p)
            }
            _ => None,
        });
        return (
            selected,
            if selected.is_some() {
                IceSelection::Selected
            } else {
                IceSelection::Unavailable
            },
        );
    }
    let mut pairs = stats.iter().filter_map(|stat| match stat {
        RtcStats::CandidatePair(p)
            if p.candidate_pair.nominated
                && valid(p)
                && (track.stream.transport_id.is_empty()
                    || p.candidate_pair.transport_id.is_empty()
                    || p.candidate_pair.transport_id == track.stream.transport_id) =>
        {
            Some(p)
        }
        _ => None,
    });
    let candidate = pairs.next();
    if pairs.next().is_some() {
        return (None, IceSelection::Ambiguous);
    }
    (
        candidate,
        if candidate.is_some() {
            IceSelection::UniqueNominated
        } else {
            IceSelection::Unavailable
        },
    )
}

fn finite(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.).then_some(value)
}

/// Uma única camada de vídeo: não mistura faixas nem escolhe a última por ordem do report.
pub fn snapshot(stats: &[RtcStats]) -> Option<SenderStats> {
    let track = video(stats)?;
    let (pair, selection) = pair(stats, track);
    Some(SenderStats {
        segment: 0,
        sample_sequence: 0,
        sample_age_ms: 0,
        frames_encoded: track.outbound.frames_encoded.into(),
        frames_sent: track.outbound.frames_sent.into(),
        key_frames_encoded: track.outbound.key_frames_encoded.into(),
        bytes_sent: track.sent.bytes_sent,
        packets_sent: track.sent.packets_sent,
        total_packet_send_delay_seconds: finite(track.outbound.total_packet_send_delay)
            .filter(|value| *value > 0.),
        pli: track.outbound.pli_count.into(),
        fir: track.outbound.fir_count.into(),
        nack: track.outbound.nack_count.into(),
        target_bitrate_bps: finite(track.outbound.target_bitrate).filter(|v| *v > 0.),
        available_outgoing_bitrate_bps: pair
            .and_then(|p| finite(p.candidate_pair.available_outgoing_bitrate).filter(|v| *v > 0.)),
        rtt_ms: pair.and_then(|p| {
            finite(p.candidate_pair.current_round_trip_time * 1000.)
                .filter(|v| *v > 0. || p.candidate_pair.responses_received > 0)
        }),
        ice_selection: selection,
        limitation: match track.outbound.quality_limitation_reason {
            libwebrtc::stats::QualityLimitationReason::None => QualityLimitation::None,
            libwebrtc::stats::QualityLimitationReason::Cpu => QualityLimitation::Cpu,
            libwebrtc::stats::QualityLimitationReason::Bandwidth => QualityLimitation::Bandwidth,
            libwebrtc::stats::QualityLimitationReason::Other => QualityLimitation::Other,
        },
    })
}

#[derive(Default)]
struct State {
    identity: Option<String>,
    generation: Option<u64>,
    segment: u64,
    sequence: u64,
    sample: Option<(Instant, SenderStats)>,
}

/// Consultado fora do caminho por frame; a primeira amostra não inventa bitrate de intervalo.
#[derive(Default)]
pub struct SenderHistory(Mutex<State>);

impl SenderHistory {
    pub fn observe(&self, stats: &[RtcStats], generation: u64) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(track) = video(stats) else {
            state.sample = None;
            state.identity = None;
            return;
        };
        let Some(mut current) = snapshot(stats) else {
            state.sample = None;
            state.identity = None;
            return;
        };
        let changed = state.identity.as_deref() != Some(track.rtc.id.as_str())
            || state.generation != Some(generation)
            || state.sample.is_some_and(|(_, old)| {
                current.bytes_sent < old.bytes_sent
                    || current.frames_sent < old.frames_sent
                    || current.frames_encoded < old.frames_encoded
                    || current
                        .total_packet_send_delay_seconds
                        .zip(old.total_packet_send_delay_seconds)
                        .is_some_and(|(current, old)| current < old)
            });
        if changed {
            state.segment += 1;
            state.identity = Some(track.rtc.id.clone());
            state.generation = Some(generation);
        }
        current.segment = state.segment;
        state.sequence = state.sequence.saturating_add(1);
        current.sample_sequence = state.sequence;
        tracing::info!(segment=current.segment, frames_sent=current.frames_sent, bytes_sent=current.bytes_sent,
            target_bitrate_bps=?current.target_bitrate_bps, available_outgoing_bitrate_bps=?current.available_outgoing_bitrate_bps,
            rtt_ms=?current.rtt_ms, ice_selection=?current.ice_selection, "estatísticas do publicador");
        state.sample = Some((Instant::now(), current));
    }

    /// Valores antigos continuam identificados pela idade; indisponibilidade não vira zero.
    pub fn latest(&self) -> Option<SenderStats> {
        self.latest_for(None)
    }

    /// O adaptador que conhece sua geração não reutiliza uma observação da conexão antiga.
    pub fn latest_for_generation(&self, generation: u64) -> Option<SenderStats> {
        self.latest_for(Some(generation))
    }

    fn latest_for(&self, generation: Option<u64>) -> Option<SenderStats> {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if generation.is_some() && generation != state.generation {
            return None;
        }
        let (at, mut sample) = state.sample?;
        let age = at.elapsed();
        if age > Duration::from_secs(15) {
            return None;
        }
        sample.sample_age_ms = u64::try_from(age.as_millis()).unwrap_or(u64::MAX);
        Some(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libwebrtc::stats::{TransportStats, dictionaries};

    fn track() -> RtcStats {
        RtcStats::OutboundRtp(OutboundRtpStats {
            rtc: dictionaries::RtcStats {
                id: "video".into(),
                ..Default::default()
            },
            stream: dictionaries::RtpStreamStats {
                kind: "video".into(),
                transport_id: "transport".into(),
                ..Default::default()
            },
            ..Default::default()
        })
    }
    fn candidate(id: &str, rtt: f64) -> RtcStats {
        RtcStats::CandidatePair(CandidatePairStats {
            rtc: dictionaries::RtcStats {
                id: id.into(),
                ..Default::default()
            },
            candidate_pair: dictionaries::CandidatePairStats {
                transport_id: "transport".into(),
                nominated: true,
                state: Some(IceCandidatePairState::Succeeded),
                current_round_trip_time: rtt,
                available_outgoing_bitrate: 12_000_000.,
                ..Default::default()
            },
        })
    }
    fn transport(selected: &str) -> RtcStats {
        RtcStats::Transport(TransportStats {
            rtc: dictionaries::RtcStats {
                id: "transport".into(),
                ..Default::default()
            },
            transport: dictionaries::TransportStats {
                selected_candidate_pair_id: selected.into(),
                ..Default::default()
            },
        })
    }

    #[test]
    fn selected_pair_is_independent_of_report_order() -> Result<(), Box<dyn std::error::Error>> {
        let mut stats = vec![
            track(),
            transport("active"),
            candidate("active", 0.06),
            candidate("old", 0.2),
        ];
        for i in 0..4 {
            stats.swap(0, i);
            for j in 1..4 {
                stats.swap(1, j);
                for k in 2..4 {
                    stats.swap(2, k);
                    let value = snapshot(&stats).ok_or("sem vídeo")?;
                    assert_eq!(value.rtt_ms, Some(60.));
                    assert_eq!(value.available_outgoing_bitrate_bps, Some(12_000_000.));
                    assert_eq!(value.ice_selection, IceSelection::Selected);
                    stats.swap(2, k);
                }
                stats.swap(1, j);
            }
            stats.swap(0, i);
        }
        Ok(())
    }
    #[test]
    fn missing_ambiguous_failed_and_invalid_are_not_zero() -> Result<(), Box<dyn std::error::Error>>
    {
        let ambiguous = snapshot(&[track(), candidate("one", 0.06), candidate("two", 0.2)])
            .ok_or("sem vídeo")?;
        assert_eq!(ambiguous.rtt_ms, None);
        assert_eq!(ambiguous.ice_selection, IceSelection::Ambiguous);
        assert_eq!(
            snapshot(&[track(), transport("missing"), candidate("one", 0.06)])
                .ok_or("sem vídeo")?
                .rtt_ms,
            None
        );
        for rtt in [f64::NAN, f64::INFINITY, -1., 0.] {
            assert_eq!(
                snapshot(&[track(), candidate("one", rtt)])
                    .ok_or("sem vídeo")?
                    .rtt_ms,
                None
            );
        }
        let mut failed = candidate("one", 0.06);
        if let RtcStats::CandidatePair(pair) = &mut failed {
            pair.candidate_pair.state = Some(IceCandidatePairState::Failed);
        }
        assert_eq!(
            snapshot(&[track(), failed]).ok_or("sem vídeo")?.rtt_ms,
            None
        );
        assert_eq!(
            snapshot(&[track(), candidate("one", 0.06)])
                .ok_or("sem vídeo")?
                .ice_selection,
            IceSelection::UniqueNominated
        );
        assert!(snapshot(&[track(), track()]).is_none());
        Ok(())
    }
    #[test]
    fn reconnect_and_counter_reset_start_new_segments() -> Result<(), Box<dyn std::error::Error>> {
        let history = SenderHistory::default();
        let mut stats = vec![track(), candidate("one", 0.06)];
        if let RtcStats::OutboundRtp(track) = &mut stats[0] {
            track.sent.bytes_sent = 100;
        }
        history.observe(&stats, 1);
        assert_eq!(history.latest().ok_or("sem amostra")?.segment, 1);
        assert_eq!(history.latest().ok_or("sem amostra")?.sample_sequence, 1);
        assert!(history.latest_for_generation(2).is_none());
        assert_eq!(
            history
                .latest_for_generation(1)
                .ok_or("sem amostra")?
                .segment,
            1
        );
        history.observe(&stats, 1);
        assert_eq!(history.latest().ok_or("sem amostra")?.segment, 1);
        assert_eq!(history.latest().ok_or("sem amostra")?.sample_sequence, 2);
        history.observe(&stats, 2);
        assert_eq!(history.latest().ok_or("sem amostra")?.segment, 2);
        if let RtcStats::OutboundRtp(track) = &mut stats[0] {
            track.sent.bytes_sent = 0;
        }
        history.observe(&stats, 2);
        assert_eq!(history.latest().ok_or("sem amostra")?.segment, 3);
        history.observe(&[], 2);
        assert!(history.latest().is_none());
        Ok(())
    }

    #[test]
    fn stale_samples_expire_and_reappearing_track_gets_new_segment()
    -> Result<(), Box<dyn std::error::Error>> {
        let history = SenderHistory::default();
        let stats = [track(), candidate("one", 0.06)];
        history.observe(&stats, 1);
        {
            let mut state = history.0.lock().map_err(|_| "lock")?;
            if let Some((at, _)) = &mut state.sample {
                *at -= Duration::from_secs(16);
            }
        }
        assert!(history.latest().is_none());
        history.observe(&[], 1);
        history.observe(&stats, 1);
        let sample = history.latest().ok_or("sem amostra")?;
        assert_eq!(sample.segment, 2);
        assert_eq!(sample.sample_sequence, 2);
        let json = serde_json::to_string(&sample)?;
        for private in ["transport", "\"id\"", "\"one\"", "\"video\""] {
            assert!(!json.contains(private));
        }
        Ok(())
    }

    #[test]
    fn selected_pair_from_another_transport_is_unavailable()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut wrong = candidate("active", 0.2);
        if let RtcStats::CandidatePair(pair) = &mut wrong {
            pair.candidate_pair.transport_id = "other".into();
        }
        let sample = snapshot(&[
            track(),
            transport("active"),
            wrong,
            candidate("fallback", 0.06),
        ])
        .ok_or("sem vídeo")?;
        assert_eq!(sample.ice_selection, IceSelection::Unavailable);
        assert_eq!(sample.rtt_ms, None);
        Ok(())
    }

    #[test]
    fn packet_delay_is_optional_and_its_reset_starts_a_segment()
    -> Result<(), Box<dyn std::error::Error>> {
        let history = SenderHistory::default();
        let mut stats = [track()];
        assert_eq!(
            snapshot(&stats)
                .ok_or("video")?
                .total_packet_send_delay_seconds,
            None
        );
        for delay in [f64::NAN, f64::INFINITY, -1., 0.] {
            if let RtcStats::OutboundRtp(track) = &mut stats[0] {
                track.outbound.total_packet_send_delay = delay;
            }
            assert_eq!(
                snapshot(&stats)
                    .ok_or("video")?
                    .total_packet_send_delay_seconds,
                None
            );
        }
        for (delay, segment) in [(2., 1), (3., 1), (1., 2)] {
            if let RtcStats::OutboundRtp(track) = &mut stats[0] {
                track.outbound.total_packet_send_delay = delay;
            }
            history.observe(&stats, 1);
            let value = history.latest().ok_or("amostra")?;
            assert_eq!(value.total_packet_send_delay_seconds, Some(delay));
            assert_eq!(value.segment, segment);
        }
        Ok(())
    }
}
