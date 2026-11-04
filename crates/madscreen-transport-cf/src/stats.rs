use crate::{Shared, lock};
use libwebrtc::stats::RtcStats;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

/// Em repouso, PLI até 4 s depois do último IDR é ignorado: o Chrome pede keyframe 3 s depois do
/// último quadro enquanto recebeu pacote nos últimos 5 s, e responder cada pedido criaria uma
/// cadeia de IDRs a cada 3 s com a tela parada. Viewer novo ou com perda usa o pedido pelo WebSocket.
const IDLE_PLI_MIN_GAP: Duration = Duration::from_secs(4);

/// O passthrough só expõe o pedido ao receber AU; em repouso, lê o RTCP diretamente.
pub(crate) async fn poll_keyframes(
    shared: Arc<Shared>,
    mut shutdown: tokio::sync::watch::Receiver<()>,
) {
    let poll = async {
        let mut ticker = tokio::time::interval(Duration::from_millis(250));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut generation = None;
        let mut previous = (0_u64, 0_u64);
        // Com quadros fluindo o encoder já vê os pedidos; ao entrar em repouso só toma a base.
        let mut rebase = true;
        loop {
            ticker.tick().await;
            let session = {
                let media = lock(&shared.media);
                if media.last_delivery.elapsed() <= Duration::from_millis(300) {
                    rebase = true;
                    continue;
                }
                media.session.clone()
            };
            let Some(session) = session else {
                continue;
            };
            let Ok(Ok(stats)) =
                tokio::time::timeout(Duration::from_secs(1), session.pc.get_stats()).await
            else {
                continue;
            };
            if generation != Some(session.generation) {
                generation = Some(session.generation);
                previous = (0, 0);
                rebase = false;
            }
            let mut counts = (0_u64, 0_u64);
            for stat in &stats {
                if let RtcStats::OutboundRtp(outbound) = stat {
                    if outbound.stream.kind == "video" {
                        counts.0 += u64::from(outbound.outbound.pli_count);
                        counts.1 += u64::from(outbound.outbound.fir_count);
                    }
                }
            }
            if !std::mem::take(&mut rebase) && (counts.0 > previous.0 || counts.1 > previous.1) {
                let media = lock(&shared.media);
                if media.last_delivery.elapsed() > Duration::from_millis(300)
                    && media.last_keyframe.elapsed() >= IDLE_PLI_MIN_GAP
                    && media
                        .session
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &session))
                {
                    shared.keyframe_needed.store(true, Ordering::Release);
                }
            }
            previous = counts;
        }
    };
    tokio::select! {
        biased;
        _ = shutdown.changed() => {},
        _ = poll => {},
    }
}

pub(crate) fn spawn(runtime: &tokio::runtime::Runtime, shared: Arc<Shared>, interval: Duration) {
    if interval.is_zero() {
        return;
    }
    runtime.spawn(async move {
        #[cfg(feature = "diagnostic-lab")]
        let mut lab_queries = 0_u32;
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let session = lock(&shared.media).session.clone();
            let Some(session) = session else {
                continue;
            };
            let Ok(Ok(stats)) =
                tokio::time::timeout(Duration::from_secs(2), session.pc.get_stats()).await
            else {
                continue;
            };
            // Injecao somente no binario de laboratorio: conserva o report RTC real
            // enquanto o ensaio reconecta a publicacao. Nao altera rede ou producao.
            #[cfg(feature = "diagnostic-lab")]
            {
                lab_queries = lab_queries.saturating_add(1);
                if lab_queries == 8 && std::env::var("MADSCREEN_LAB_HOLD_RTC").as_deref() == Ok("1")
                {
                    tracing::info!("diagnostic-lab: holding RTC sample");
                    tokio::time::sleep(Duration::from_secs(6)).await;
                }
            }
            // Uma consulta da conexão anterior não pode substituir a amostra da atual.
            let media = lock(&shared.media);
            if media
                .session
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &session))
            {
                shared.sender_history.observe(&stats, session.generation);
            } else {
                #[cfg(feature = "diagnostic-lab")]
                tracing::info!("diagnostic-lab: discarded stale RTC sample");
            }
        }
    });
}
