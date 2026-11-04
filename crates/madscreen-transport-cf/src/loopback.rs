//! Ida e volta local da faixa de áudio: `NativeAudioSource` -> Opus (libwebrtc) -> RTP -> decodificador,
//! entre dois PeerConnections no mesmo processo. Sem rede externa e sem tocar som algum.
//! `cargo test -p madscreen-transport-cf --release -- --ignored audio_loopback`
// Teste que falha com panic: `expect` aqui é o comportamento desejado.
#![allow(clippy::expect_used)]

use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::StreamExt;
use libwebrtc::{
    MediaType,
    audio_frame::AudioFrame,
    audio_source::{AudioSourceOptions, native::NativeAudioSource},
    audio_stream::native::NativeAudioStream,
    ice_candidate::IceCandidate,
    media_stream_track::MediaStreamTrack,
    peer_connection::{AnswerOptions, OfferOptions, PeerConnection},
    peer_connection_factory::{
        PeerConnectionFactory, RtcConfiguration, native::PeerConnectionFactoryExt,
    },
    rtp_transceiver::{RtpTransceiverDirection, RtpTransceiverInit},
    session_description::{SdpType, SessionDescription},
};

use crate::{AUDIO_CHANNELS, AUDIO_FRAMES_PER_BLOCK, AUDIO_SAMPLE_RATE, OPUS_MAX_BITRATE_BPS, sdp};

fn wire_candidates(from: &PeerConnection, to: Arc<PeerConnection>, handle: tokio::runtime::Handle) {
    from.on_ice_candidate(Some(Box::new(move |candidate| {
        let to = to.clone();
        let parsed = IceCandidate::parse(
            &candidate.sdp_mid(),
            candidate.sdp_mline_index(),
            &candidate.candidate(),
        );
        if let Ok(parsed) = parsed {
            handle.spawn(async move {
                let _ = to.add_ice_candidate(parsed).await;
            });
        }
    })));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "usa o libwebrtc de verdade (um par de PeerConnections locais)"]
async fn audio_loopback() {
    let factory = PeerConnectionFactory::default();
    let sender = Arc::new(
        factory
            .create_peer_connection(RtcConfiguration::default())
            .expect("pc emissor"),
    );
    let receiver = Arc::new(
        factory
            .create_peer_connection(RtcConfiguration::default())
            .expect("pc receptor"),
    );
    let handle = tokio::runtime::Handle::current();
    wire_candidates(&sender, receiver.clone(), handle.clone());
    wire_candidates(&receiver, sender.clone(), handle);

    let received = Arc::new(Mutex::new(None::<MediaStreamTrack>));
    let slot = received.clone();
    receiver.on_track(Some(Box::new(move |event| {
        *slot.lock().expect("lock") = Some(event.track);
    })));

    let source = NativeAudioSource::new(
        AudioSourceOptions {
            echo_cancellation: false,
            noise_suppression: false,
            auto_gain_control: false,
        },
        AUDIO_SAMPLE_RATE,
        AUDIO_CHANNELS,
        0,
    );
    let track = factory.create_audio_track("madscreen-audio", source.clone());
    let transceiver = sender
        .add_transceiver(
            track.into(),
            RtpTransceiverInit {
                direction: RtpTransceiverDirection::SendOnly,
                stream_ids: vec!["madscreen".into()],
                send_encodings: vec![],
            },
        )
        .expect("transceiver");
    let mut codecs = factory.get_rtp_sender_capabilities(MediaType::Audio).codecs;
    codecs.retain(|c| c.mime_type.eq_ignore_ascii_case("audio/opus"));
    transceiver.set_codec_preferences(codecs).expect("codecs");

    let offer = sender
        .create_offer(OfferOptions::default())
        .await
        .expect("oferta");
    let offer_sdp = sdp::rewrite_opus_stereo(&offer.to_string(), OPUS_MAX_BITRATE_BPS);
    assert!(offer_sdp.contains("stereo=1"), "oferta sem Opus estéreo");
    sender
        .set_local_description(SessionDescription::parse(&offer_sdp, SdpType::Offer).expect("sdp"))
        .await
        .expect("local");
    receiver
        .set_remote_description(SessionDescription::parse(&offer_sdp, SdpType::Offer).expect("sdp"))
        .await
        .expect("remota");
    let answer = receiver
        .create_answer(AnswerOptions::default())
        .await
        .expect("resposta");
    // A resposta do libwebrtc não repete `stereo=1`; sem ele o Opus é codificado e decodificado em
    // mono. Por isso a resposta é reescrita (o publicador faz isso com a resposta do SFU, e o viewer
    // com a própria resposta).
    println!(
        "resposta original com stereo=1: {}",
        answer.to_string().contains("stereo=1")
    );
    let answer_sdp = sdp::rewrite_opus_stereo(&answer.to_string(), OPUS_MAX_BITRATE_BPS);
    receiver
        .set_local_description(
            SessionDescription::parse(&answer_sdp, SdpType::Answer).expect("sdp"),
        )
        .await
        .expect("local do receptor");
    sender
        .set_remote_description(
            SessionDescription::parse(&answer_sdp, SdpType::Answer).expect("sdp"),
        )
        .await
        .expect("remota do emissor");

    // Esquerda (0,003, -50 dBFS) três vezes mais forte que a direita (0,001): o estéreo precisa chegar
    // separado. Nível baixíssimo de propósito: se o libwebrtc reproduzir a faixa, não incomoda.
    let pump = tokio::spawn(async move {
        let mut phase = 0.0_f64;
        let step = 2.0 * std::f64::consts::PI * 440.0 / f64::from(AUDIO_SAMPLE_RATE);
        let mut interval = tokio::time::interval(Duration::from_millis(10));
        for _ in 0..500 {
            interval.tick().await;
            let mut block = Vec::with_capacity(AUDIO_FRAMES_PER_BLOCK * 2);
            for _ in 0..AUDIO_FRAMES_PER_BLOCK {
                let v = phase.sin();
                phase += step;
                block.push((v * 0.003 * 32767.0) as i16);
                block.push((v * 0.001 * 32767.0) as i16);
            }
            let _ = source
                .capture_frame(&AudioFrame {
                    data: Cow::Borrowed(&block),
                    sample_rate: AUDIO_SAMPLE_RATE,
                    num_channels: AUDIO_CHANNELS,
                    samples_per_channel: AUDIO_FRAMES_PER_BLOCK as u32,
                })
                .await;
        }
    });

    let mut track = None;
    for _ in 0..200 {
        if let Some(t) = received.lock().expect("lock").clone() {
            track = Some(t);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let Some(MediaStreamTrack::Audio(track)) = track else {
        panic!("o receptor não recebeu a faixa de áudio");
    };
    let mut stream = NativeAudioStream::new(track, AUDIO_SAMPLE_RATE as i32, AUDIO_CHANNELS as i32);
    let (mut frames, mut left, mut right) = (0_u64, 0.0_f64, 0.0_f64);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while tokio::time::Instant::now() < deadline && frames < 250 {
        let Ok(Some(frame)) = tokio::time::timeout(Duration::from_millis(500), stream.next()).await
        else {
            continue;
        };
        // Ignora o início (rampa do decodificador e do jitter buffer).
        frames += 1;
        if frames > 50 {
            for pair in frame.data.chunks_exact(2) {
                left += f64::from(pair[0]).powi(2);
                right += f64::from(pair[1]).powi(2);
            }
        }
    }
    pump.abort();
    let (l, r) = (left.sqrt(), right.sqrt());
    println!(
        "quadros recebidos: {frames}, energia L={l:.0} R={r:.0}, razão L/R={:.2}",
        l / r.max(1.0)
    );
    assert!(frames > 100, "poucos quadros de áudio chegaram: {frames}");
    assert!(l > 0.0 && r > 0.0, "áudio recebido está mudo");
    assert!(
        (2.0..4.5).contains(&(l / r)),
        "estéreo não preservado (L/R = {})",
        l / r
    );
}
