//! O Worker escolhe o SFU; este adaptador conserva presença/geração e feedback nos dois caminhos.
use super::Publisher;
use anyhow::{Context, Result};
use madscreen_audio::mixer::Sink;
use madscreen_transport::{LiveKitPublisher, PublishConfig, RateRequest};
use madscreen_transport_cf::{
    CfPublishConfig, CloudflarePublisher, Error, SfuBackend, WorkerClient, resolve_credentials,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{runtime::Runtime, sync::watch, task::JoinHandle};

pub(in crate::publish) fn connect(mut config: CfPublishConfig) -> Result<Box<dyn Publisher>> {
    (config.worker_url, config.token) =
        resolve_credentials(Some(&config.worker_url), Some(&config.token))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("madscreen-signal")
        .enable_all()
        .build()
        .context("criando runtime de sinalização")?;
    let worker = runtime.block_on(async { WorkerClient::new(&config) })?;
    match runtime.block_on(worker.active_provider())? {
        SfuBackend::Cloudflare => {
            drop(worker);
            drop(runtime);
            Ok(Box::new(CloudflarePublisher::connect(&config)?))
        }
        SfuBackend::Livekit => Ok(Box::new(ManagedLiveKit::connect(config, worker, runtime)?)),
    }
}

struct ManagedLiveKit {
    publisher: LiveKitPublisher,
    runtime: Runtime,
    shutdown: watch::Sender<()>,
    heartbeat: JoinHandle<()>,
    events: JoinHandle<()>,
    closer: JoinHandle<()>,
    active: Arc<AtomicBool>,
    keyframe: Arc<AtomicBool>,
}

impl ManagedLiveKit {
    fn connect(config: CfPublishConfig, worker: WorkerClient, runtime: Runtime) -> Result<Self> {
        let started = runtime.block_on(worker.start_livekit(&config))?;
        let (shutdown, stop) = watch::channel(());
        let active = Arc::new(AtomicBool::new(true));
        let keyframe = Arc::new(AtomicBool::new(true));
        let events = runtime.spawn({
            let worker = worker.clone();
            let requested = keyframe.clone();
            let stop = stop.clone();
            async move {
                worker.listen_keyframes(config.token, requested, stop).await;
            }
        });
        let heartbeat = runtime.spawn({
            let active = active.clone();
            let keyframe = keyframe.clone();
            let shutdown = shutdown.clone();
            let mut stop = stop.clone();
            let generation = started.generation;
            async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(5));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut failures = 0;
                loop {
                    tokio::select! {
                        biased;
                        _ = stop.changed() => break,
                        _ = ticker.tick() => {}
                    }
                    match worker.heartbeat(generation).await {
                        Ok(requested) => {
                            failures = 0;
                            if requested {
                                keyframe.store(true, Ordering::Release);
                            }
                        }
                        Err(Error::StaleGeneration) => break,
                        Err(Error::Status { status, .. })
                            if matches!(status.as_u16(), 401 | 403) =>
                        {
                            break;
                        }
                        Err(error) => {
                            failures += 1;
                            tracing::warn!(%error, "falha no heartbeat LiveKit/Worker");
                            if failures >= 3 {
                                break;
                            }
                        }
                    }
                }
                active.store(false, Ordering::Release);
                let _ = shutdown.send(());
                worker.stop(generation).await;
            }
        });
        let publish = PublishConfig {
            url: started.livekit.url,
            token: Some(started.livekit.token),
            room: started.livekit.room,
            identity: started.livekit.identity,
            api_key: String::new(),
            api_secret: String::new(),
            track_name: "screen".into(),
            width: config.width,
            height: config.height,
            max_bitrate_bps: config.max_bitrate_bps,
            max_fps: config.max_fps,
            audio: config.audio,
            preferred_h264_profile: config.preferred_h264_profile,
            h264_level_idc: config.h264_level_idc,
        };
        // Fora de block_on: cada publicador LiveKit possui seu próprio runtime.
        let publisher = match LiveKitPublisher::connect(&publish) {
            Ok(publisher) if active.load(Ordering::Acquire) => publisher,
            result => {
                let _ = shutdown.send(());
                runtime.block_on(async {
                    let _ = tokio::join!(heartbeat, events);
                });
                let publisher = result?;
                publisher.close();
                anyhow::bail!("a geração expirou durante a conexão LiveKit");
            }
        };
        let close_room = publisher.close_room();
        let closer = runtime.spawn(async move {
            let mut stop = stop;
            let _ = stop.changed().await;
            close_room.await;
        });
        Ok(Self {
            publisher,
            runtime,
            shutdown,
            heartbeat,
            events,
            closer,
            active,
            keyframe,
        })
    }
}

impl Publisher for ManagedLiveKit {
    fn provider(&self) -> &'static str {
        "livekit"
    }
    fn sender_stats(&self) -> Option<madscreen_ipc::diagnostics::SenderStats> {
        self.active
            .load(Ordering::Acquire)
            .then(|| self.publisher.sender_stats())
            .flatten()
    }
    fn push(&mut self, au: &[u8], keyframe: bool, timestamp_us: i64) -> bool {
        self.active.load(Ordering::Acquire) && self.publisher.push(au, keyframe, timestamp_us)
    }
    fn take_keyframe_request(&self) -> bool {
        // Consumir as duas flags mesmo se uma já estiver armada.
        let worker = self.keyframe.swap(false, Ordering::AcqRel);
        self.publisher.take_keyframe_request() || worker
    }
    fn take_rate_request(&self) -> Option<RateRequest> {
        self.publisher.take_rate_request()
    }
    fn counts(&self) -> (u64, u64) {
        self.publisher.counts()
    }
    fn reconnect_count(&self) -> u64 {
        0
    }
    fn is_connected(&self) -> Option<bool> {
        Some(self.active.load(Ordering::Acquire) && self.publisher.is_connected())
    }
    fn is_terminated(&self) -> bool {
        !self.active.load(Ordering::Acquire)
    }
    fn audio_sink(&self) -> Option<Sink> {
        let pusher = self.publisher.audio_pusher()?;
        let active = self.active.clone();
        Some(Box::new(move |block| {
            if active.load(Ordering::Acquire) {
                pusher.push(block);
            }
        }))
    }
    fn audio_counts(&self) -> (u64, u64) {
        self.publisher.audio_counts()
    }
    fn spawn_stats_logger(&self, interval: Duration) {
        self.publisher.spawn_stats_logger(interval);
    }
    fn close(self: Box<Self>) {
        let Self {
            publisher,
            runtime,
            shutdown,
            heartbeat,
            events,
            closer,
            ..
        } = *self;
        let _ = shutdown.send(());
        runtime.block_on(async {
            let _ = tokio::join!(heartbeat, events, closer);
        });
        publisher.close();
    }
}
