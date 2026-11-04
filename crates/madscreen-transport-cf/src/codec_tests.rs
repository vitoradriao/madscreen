//! Exercita o libwebrtc real antes da sinalização, sem GPU, Worker ou rede externa.
use super::*;
use anyhow::{Context, Result};
use libwebrtc::{peer_connection::AnswerOptions, video_source::VideoResolution};

#[tokio::test]
async fn native_h264_preferences_and_local_offer_accept_encoder_levels() -> Result<()> {
    for only_h264 in [true, false] {
        for level in [31, 40, 42, 50, 51, 52] {
            let factory = PeerConnectionFactory::default();
            let pc = factory.create_peer_connection(RtcConfiguration::default())?;
            let source = NativeVideoSource::new_encoded(VideoResolution {
                width: 1920,
                height: 1080,
            });
            let session = Session {
                pc,
                source,
                audio_source: None,
                generation: 0,
                _factory: factory,
            };
            let config = CfPublishConfig {
                h264_level_idc: Some(level),
                only_h264,
                ..Default::default()
            };
            session.video_transceiver(&config).with_context(|| {
                format!("preferências nativas no nível {level}, somente H.264={only_h264}")
            })?;
            let offer = session.pc.create_offer(OfferOptions::default()).await?;
            let sdp = sdp::raise_h264_level(&offer.to_string(), level)?;
            assert!(sdp.contains(&format!("profile-level-id=42e0{level:02x}")));
            let offer = SessionDescription::parse(&sdp, SdpType::Offer)?;
            session
                .pc
                .set_local_description(offer)
                .await
                .with_context(|| format!("oferta local no nível {level}"))?;
            let receiver = session
                ._factory
                .create_peer_connection(RtcConfiguration::default())?;
            receiver
                .set_remote_description(SessionDescription::parse(&sdp, SdpType::Offer)?)
                .await?;
            let answer = receiver.create_answer(AnswerOptions::default()).await?;
            // O SFU pode devolver o nível solicitado, superior ao nível nativo anunciado
            // nas capacidades. Isso também precisa passar em set_remote_description.
            let answer_sdp = sdp::raise_h264_level(&answer.to_string(), level)?;
            sdp::validate_answer(&answer_sdp)?;
            receiver
                .set_local_description(SessionDescription::parse(&answer_sdp, SdpType::Answer)?)
                .await?;
            session
                .pc
                .set_remote_description(SessionDescription::parse(&answer_sdp, SdpType::Answer)?)
                .await
                .with_context(|| format!("resposta remota no nível {level}"))?;
            receiver.close();
        }
    }
    Ok(())
}

#[tokio::test]
async fn main_profile_preferences_select_native_main_without_rewriting_profile() -> Result<()> {
    let factory = PeerConnectionFactory::default();
    let pc = factory.create_peer_connection(RtcConfiguration::default())?;
    let source = NativeVideoSource::new_encoded(VideoResolution {
        width: 1920,
        height: 1080,
    });
    let session = Session {
        pc,
        source,
        audio_source: None,
        generation: 0,
        _factory: factory,
    };
    let config = CfPublishConfig {
        preferred_h264_profile: Some("4d00".into()),
        h264_level_idc: Some(42),
        only_h264: true,
        ..Default::default()
    };
    session.video_transceiver(&config)?;
    let offer = session.pc.create_offer(OfferOptions::default()).await?;
    let original = offer.to_string();
    assert!(original.contains("profile-level-id=4d001f"));
    assert!(!original.contains("profile-level-id=42001f"));
    assert!(!original.contains("profile-level-id=42e01f"));
    let sdp = sdp::raise_h264_level(&original, 42)?;
    assert!(sdp.contains("profile-level-id=4d002a"));
    session
        .pc
        .set_local_description(SessionDescription::parse(&sdp, SdpType::Offer)?)
        .await?;
    session.pc.close();
    Ok(())
}
