//! Fronteira entre o loop de AUs/feedback e os SDKs de cada provedor.
use madscreen_audio::mixer::Sink;
use madscreen_transport::{LiveKitPublisher, RateRequest};
use madscreen_transport_cf::CloudflarePublisher;
use std::time::Duration;

mod worker;
pub(super) use worker::connect as connect_worker;

pub(super) trait Publisher {
    fn provider(&self) -> &'static str;
    fn push(&mut self, au: &[u8], keyframe: bool, timestamp_us: i64) -> bool;
    fn take_keyframe_request(&self) -> bool;
    fn take_rate_request(&self) -> Option<RateRequest>;
    /// Laboratório: pede ao WebRTC uma sondagem de banda até o teto. Sem efeito onde não há suporte.
    fn request_probe(&self) {}
    fn sender_stats(&self) -> Option<madscreen_ipc::diagnostics::SenderStats>;
    fn counts(&self) -> (u64, u64);
    fn reconnect_count(&self) -> u64;
    fn is_connected(&self) -> Option<bool>;
    fn is_terminated(&self) -> bool {
        false
    }
    fn audio_sink(&self) -> Option<Sink>;
    fn audio_counts(&self) -> (u64, u64);
    fn spawn_stats_logger(&self, interval: Duration);
    fn close(self: Box<Self>);
}

macro_rules! common_methods {
    ($publisher:ty) => {
        fn push(&mut self, au: &[u8], keyframe: bool, timestamp_us: i64) -> bool {
            <$publisher>::push(self, au, keyframe, timestamp_us)
        }
        fn take_keyframe_request(&self) -> bool {
            <$publisher>::take_keyframe_request(self)
        }
        fn sender_stats(&self) -> Option<madscreen_ipc::diagnostics::SenderStats> {
            <$publisher>::sender_stats(self)
        }
        fn counts(&self) -> (u64, u64) {
            <$publisher>::counts(self)
        }
        fn audio_counts(&self) -> (u64, u64) {
            <$publisher>::audio_counts(self)
        }
        fn audio_sink(&self) -> Option<Sink> {
            let pusher = self.audio_pusher()?;
            Some(Box::new(move |block: &[i16]| {
                pusher.push(block);
            }))
        }
        fn spawn_stats_logger(&self, interval: Duration) {
            <$publisher>::spawn_stats_logger(self, interval);
        }
        fn close(self: Box<Self>) {
            <$publisher>::close(*self);
        }
    };
}

impl Publisher for LiveKitPublisher {
    fn provider(&self) -> &'static str {
        "livekit"
    }
    common_methods!(LiveKitPublisher);

    fn take_rate_request(&self) -> Option<RateRequest> {
        LiveKitPublisher::take_rate_request(self)
    }
    fn request_probe(&self) {
        LiveKitPublisher::request_probe(self);
    }
    fn reconnect_count(&self) -> u64 {
        0
    }
    fn is_connected(&self) -> Option<bool> {
        Some(LiveKitPublisher::is_connected(self))
    }
}

impl Publisher for CloudflarePublisher {
    fn provider(&self) -> &'static str {
        "cloudflare"
    }
    common_methods!(CloudflarePublisher);

    fn request_probe(&self) {
        CloudflarePublisher::request_probe(self);
    }
    fn take_rate_request(&self) -> Option<RateRequest> {
        CloudflarePublisher::take_rate_request(self).map(|r| RateRequest {
            bitrate_bps: r.bitrate_bps,
            fps: r.fps,
        })
    }
    fn reconnect_count(&self) -> u64 {
        CloudflarePublisher::reconnect_count(self)
    }
    fn is_connected(&self) -> Option<bool> {
        Some(CloudflarePublisher::is_connected(self))
    }
}
