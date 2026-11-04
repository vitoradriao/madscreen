use crate::{Error, Result};

/// Não entrega AUs H.264 a um sender negociado para outro codec ou faixa rejeitada.
pub(crate) fn validate_answer(sdp: &str) -> Result<()> {
    let mut video = sdp.lines().skip_while(|line| !line.starts_with("m=video "));
    let mut media = video
        .next()
        .ok_or(Error::WebRtc("resposta sem vídeo"))?
        .split_whitespace();
    let _kind = media.next();
    if media.next().is_none_or(|port| port == "0") {
        return Err(Error::WebRtc(
            "vídeo rejeitado pelo SFU; verifique o perfil H.264 anunciado",
        ));
    }
    let _protocol = media.next();
    let payload = media
        .next()
        .ok_or(Error::WebRtc("resposta sem payload de vídeo"))?;
    let mut h264 = false;
    for line in video.take_while(|line| !line.starts_with("m=")) {
        if matches!(line, "a=inactive" | "a=sendonly") {
            return Err(Error::WebRtc("SFU não aceitou receber vídeo"));
        }
        if let Some((pt, codec)) = line
            .strip_prefix("a=rtpmap:")
            .and_then(|v| v.split_once(' '))
        {
            h264 |= pt == payload && codec.eq_ignore_ascii_case("H264/90000");
        }
    }
    if !h264 {
        return Err(Error::WebRtc(
            "SFU não selecionou H.264 para o vídeo pré-codificado",
        ));
    }
    Ok(())
}

/// Mantém perfil e constraints e troca só o nível dos `profile-level-id` H.264.
/// `42e01f` no nível 4.0 vira `42e028`. Não vira `640028`: esse perfil não está na
/// lista do libwebrtc e a resposta do SFU é rejeitada em `setRemoteDescription`.
pub(crate) fn fmtp_with_h264_level(fmtp: &str, level_idc: u32) -> String {
    let mut saw = false;
    let mut parts = Vec::new();
    for param in fmtp.split(';') {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }
        if let Some(value) = param.strip_prefix("profile-level-id=")
            && let Some(updated) =
                madscreen_ipc::resolution::h264_with_level(value.trim(), level_idc)
        {
            parts.push(format!("profile-level-id={updated}"));
            saw = true;
            continue;
        }
        parts.push(param.to_owned());
    }
    if !saw && let Some(fallback) = madscreen_ipc::resolution::h264_with_level("42e01f", level_idc)
    {
        parts.push(format!("profile-level-id={fallback}"));
    }
    parts.join(";")
}

/// Sobe o nível de cada fmtp H.264. Mantém RTX/apt, o perfil oferecido e os demais parâmetros.
pub(crate) fn raise_h264_level(sdp: &str, level_idc: u32) -> Result<String> {
    if level_idc > 0xff {
        return Err(Error::Config("nível H.264 fora de 0..=255"));
    }
    let payloads: Vec<&str> = sdp
        .lines()
        .filter_map(|line| {
            let (pt, codec) = line.strip_prefix("a=rtpmap:")?.split_once(' ')?;
            codec
                .split('/')
                .next()?
                .eq_ignore_ascii_case("H264")
                .then_some(pt)
        })
        .collect();
    let mut output = String::with_capacity(sdp.len() + 16);
    let mut rewritten = 0;
    for line in sdp.lines() {
        if let Some((pt, params)) = line
            .strip_prefix("a=fmtp:")
            .and_then(|value| value.split_once(' '))
            .filter(|(pt, _)| payloads.contains(pt))
        {
            output.push_str("a=fmtp:");
            output.push_str(pt);
            output.push(' ');
            output.push_str(&fmtp_with_h264_level(params, level_idc));
            rewritten += 1;
        } else {
            output.push_str(line);
        }
        output.push_str("\r\n");
    }
    if rewritten == 0 {
        return Err(Error::WebRtc("a oferta não contém fmtp H.264"));
    }
    Ok(output)
}

/// Reescreve somente fmtp de PTs H264. Mantém RTX/apt e demais parâmetros.
pub(crate) fn rewrite_h264_profile(sdp: &str, profile: &str) -> Result<String> {
    if profile.len() != 6 || !profile.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::Config(
            "--cf-profile-level-id (seis dígitos hexadecimais)",
        ));
    }
    let payloads: Vec<&str> = sdp
        .lines()
        .filter_map(|line| {
            let (pt, codec) = line.strip_prefix("a=rtpmap:")?.split_once(' ')?;
            codec
                .split('/')
                .next()?
                .eq_ignore_ascii_case("H264")
                .then_some(pt)
        })
        .collect();
    let mut output = String::with_capacity(sdp.len());
    let mut rewritten = 0;
    for line in sdp.lines() {
        if let Some((pt, params)) = line
            .strip_prefix("a=fmtp:")
            .and_then(|v| v.split_once(' '))
            .filter(|(pt, _)| payloads.contains(pt))
        {
            output.push_str("a=fmtp:");
            output.push_str(pt);
            output.push(' ');
            for param in params.split(';') {
                if !param.trim().starts_with("profile-level-id=") && !param.trim().is_empty() {
                    output.push_str(param.trim());
                    output.push(';');
                }
            }
            output.push_str("profile-level-id=");
            output.push_str(profile);
            rewritten += 1;
        } else {
            output.push_str(line);
        }
        output.push_str("\r\n");
    }
    if rewritten == 0 {
        return Err(Error::WebRtc("a oferta não contém fmtp H.264"));
    }
    Ok(output)
}

/// A resposta do SFU aceitou a m-line de áudio (porta diferente de zero e não inativa).
pub(crate) fn audio_accepted(sdp: &str) -> bool {
    let mut audio = sdp.lines().skip_while(|line| !line.starts_with("m=audio "));
    let Some(media) = audio.next() else {
        return false;
    };
    if media
        .split_whitespace()
        .nth(1)
        .is_none_or(|port| port == "0")
    {
        return false;
    }
    !audio
        .take_while(|line| !line.starts_with("m="))
        .any(|line| matches!(line.trim_end(), "a=inactive" | "a=sendonly"))
}

/// Anuncia Opus estéreo: `stereo=1;sprop-stereo=1` e teto de bitrate, só nos PTs Opus.
pub(crate) fn rewrite_opus_stereo(sdp: &str, max_bitrate_bps: u64) -> String {
    let payloads: Vec<&str> = sdp
        .lines()
        .filter_map(|line| {
            let (pt, codec) = line.strip_prefix("a=rtpmap:")?.split_once(' ')?;
            codec
                .split('/')
                .next()?
                .eq_ignore_ascii_case("opus")
                .then_some(pt)
        })
        .collect();
    let extra = format!("stereo=1;sprop-stereo=1;maxaveragebitrate={max_bitrate_bps}");
    let mut output = String::with_capacity(sdp.len() + 128);
    let mut with_fmtp: Vec<&str> = Vec::new();
    for line in sdp.lines() {
        if let Some((pt, params)) = line
            .strip_prefix("a=fmtp:")
            .and_then(|v| v.split_once(' '))
            .filter(|(pt, _)| payloads.contains(pt))
        {
            with_fmtp.push(pt);
            output.push_str("a=fmtp:");
            output.push_str(pt);
            output.push(' ');
            for param in params.split(';') {
                let param = param.trim();
                let key = param.split('=').next().unwrap_or("");
                if !param.is_empty()
                    && !matches!(key, "stereo" | "sprop-stereo" | "maxaveragebitrate")
                {
                    output.push_str(param);
                    output.push(';');
                }
            }
            output.push_str(&extra);
        } else {
            output.push_str(line);
        }
        output.push_str("\r\n");
        // Sem fmtp original: cria a linha logo após o rtpmap do Opus.
        if let Some((pt, _)) = line
            .strip_prefix("a=rtpmap:")
            .and_then(|v| v.split_once(' '))
            .filter(|(pt, codec)| {
                payloads.contains(pt) && codec.to_ascii_lowercase().starts_with("opus/")
            })
        {
            let has_fmtp = sdp.lines().any(|l| {
                l.strip_prefix("a=fmtp:")
                    .is_some_and(|v| v.split(' ').next() == Some(pt))
            });
            if !has_fmtp {
                output.push_str(&format!("a=fmtp:{pt} {extra}\r\n"));
            }
        }
    }
    output
}

/// Anexa os candidatos ICE e `end-of-candidates` ao fim da primeira seção de mídia.
pub(crate) fn insert_candidates(sdp: &str, candidates: &[String]) -> String {
    let mut block = String::new();
    for candidate in candidates {
        block.push_str("a=");
        block.push_str(candidate);
        block.push_str("\r\n");
    }
    block.push_str("a=end-of-candidates\r\n");
    let mut output = String::with_capacity(sdp.len() + block.len());
    let mut sections = 0;
    let mut inserted = false;
    for line in sdp.lines() {
        if line.starts_with("m=") {
            sections += 1;
            if sections == 2 {
                output.push_str(&block);
                inserted = true;
            }
        }
        output.push_str(line);
        output.push_str("\r\n");
    }
    if !inserted {
        output.push_str(&block);
    }
    output
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const OFFER: &str = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 102\r\na=mid:0\r\na=rtpmap:102 H264/90000\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:1\r\na=rtpmap:111 opus/48000/2\r\na=fmtp:111 minptime=10;useinbandfec=1\r\n";

    #[test]
    fn opus_stereo_rewrites_only_opus_fmtp() {
        let out = rewrite_opus_stereo(OFFER, 128_000);
        assert!(out.contains(
            "a=fmtp:111 minptime=10;useinbandfec=1;stereo=1;sprop-stereo=1;maxaveragebitrate=128000\r\n"
        ));
        assert!(out.contains("a=rtpmap:102 H264/90000\r\n"));
        // Idempotente.
        assert_eq!(rewrite_opus_stereo(&out, 128_000), out);
    }

    #[test]
    fn opus_stereo_creates_missing_fmtp() {
        let offer =
            "m=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\na=sendonly\r\n";
        let out = rewrite_opus_stereo(offer, 64_000);
        assert!(out.contains(
            "a=rtpmap:111 opus/48000/2\r\na=fmtp:111 stereo=1;sprop-stereo=1;maxaveragebitrate=64000\r\na=sendonly\r\n"
        ));
    }

    #[test]
    fn candidates_go_to_first_section() {
        let out = insert_candidates(OFFER, &["candidate:1 1 udp 1 1.2.3.4 5 typ host".into()]);
        let video = out.find("m=video").unwrap();
        let cand = out.find("a=candidate:1").unwrap();
        let audio = out.find("m=audio").unwrap();
        assert!(video < cand && cand < audio);
        assert!(out.contains("a=end-of-candidates\r\nm=audio"));
        let single = insert_candidates("v=0\r\nm=video 9 X 1\r\n", &[]);
        assert!(single.ends_with("m=video 9 X 1\r\na=end-of-candidates\r\n"));
    }

    #[test]
    fn raises_level_without_replacing_the_offered_profile() {
        let offer = "m=video 9 UDP/TLS/RTP/SAVPF 96 97 98\r\n\
a=rtpmap:96 H264/90000\r\n\
a=fmtp:96 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f\r\n\
a=rtpmap:97 rtx/90000\r\n\
a=fmtp:97 apt=96\r\n\
a=rtpmap:98 H264/90000\r\n\
a=fmtp:98 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=640c1f\r\n";
        let out = raise_h264_level(offer, 40).unwrap();
        assert!(out.contains("profile-level-id=42e028"));
        assert!(out.contains("profile-level-id=640c28"));
        assert!(out.contains("a=fmtp:97 apt=96\r\n"));
        assert!(!out.contains("640028"));
        assert!(!out.contains("42e01f"));
        assert!(!out.contains("640c1f"));
        assert_eq!(raise_h264_level(&out, 40).unwrap(), out);
        let wide = raise_h264_level(offer, 52).unwrap();
        assert!(wide.contains("profile-level-id=42e034"));
        assert!(wide.contains("profile-level-id=640c34"));
        assert!(raise_h264_level("m=video 9 X 96\r\n", 40).is_err());
    }

    #[test]
    fn audio_answer_acceptance() {
        assert!(audio_accepted(
            "m=video 9 X 1\r\nm=audio 9 X 111\r\na=recvonly\r\n"
        ));
        assert!(!audio_accepted("m=video 9 X 1\r\nm=audio 0 X 111\r\n"));
        assert!(!audio_accepted("m=audio 9 X 111\r\na=inactive\r\n"));
        assert!(!audio_accepted("m=video 9 X 1\r\n"));
    }
}
