use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

use crate::{Shared, signaling::Worker};

/// Canal independente do heartbeat e da reconexão do PeerConnection.
pub(crate) async fn run(
    worker: Worker,
    token: String,
    shared: Arc<Shared>,
    shutdown: watch::Receiver<()>,
) {
    run_with_callback(
        worker,
        token,
        move || shared.keyframe_needed.store(true, Ordering::Release),
        shutdown,
    )
    .await;
}

pub(crate) async fn run_with_callback(
    worker: Worker,
    token: String,
    requested: impl Fn() + Send,
    mut shutdown: watch::Receiver<()>,
) {
    #[derive(Deserialize)]
    struct Event {
        #[serde(rename = "type")]
        kind: String,
    }
    let mut backoff = Duration::from_secs(1);
    loop {
        let connected = tokio::select! {
            biased;
            _ = shutdown.changed() => return,
            connected = worker.keyframe_socket(&token) => connected,
        };
        if let Ok(mut socket) = connected {
            tracing::info!("canal de pedidos de keyframe conectado ao Worker");
            backoff = Duration::from_secs(1);
            let mut last_message = Instant::now();
            let mut ping = tokio::time::interval(Duration::from_secs(25));
            ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => {
                        let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
                        return;
                    }
                    message = socket.next() => match message {
                        Some(Ok(Message::Text(text))) => {
                            last_message = Instant::now();
                            if serde_json::from_str::<Event>(&text).is_ok_and(|event| event.kind == "keyframe") {
                                requested();
                            }
                        }
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                        // A próxima leitura/escrita envia automaticamente o pong do protocolo.
                        Some(Ok(_)) => {}
                    },
                    _ = ping.tick() => {
                        if last_message.elapsed() > Duration::from_secs(60) {
                            break;
                        }
                        if !matches!(tokio::time::timeout(Duration::from_secs(1), socket.send(Message::Text("ping".into()))).await, Ok(Ok(()))) {
                            break;
                        }
                    }
                }
            }
        }
        // Nem o erro nem a resposta do handshake são registrados (contêm autenticação).
        tracing::warn!(
            espera_ms = backoff.as_millis(),
            "canal de keyframe indisponível; heartbeat continua ativo"
        );
        tokio::select! {
            biased;
            _ = shutdown.changed() => return,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(Duration::from_secs(10));
    }
}
