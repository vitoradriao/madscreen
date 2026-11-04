// Pure SDP transformation used by PeerTransport; independently testable.
// Payload types are scoped to a media section, not to the whole session.
pub(super) fn munge_h264_level(sdp: &str, level_idc: u32) -> String {
    if level_idc == 0 || level_idc > 0xff {
        return sdp.to_string();
    }
    let eol = if sdp.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<&str> = sdp.split(eol).collect();
    let mut output = Vec::with_capacity(lines.len());
    let mut start = 0;
    for end in 1..=lines.len() {
        if end < lines.len() && !lines[end].starts_with("m=") {
            continue;
        }
        let section = &lines[start..end];
        let video = section[0].starts_with("m=video ");
        let h264_pts: Vec<&str> = section
            .iter()
            .filter_map(|line| {
                let rest = line.strip_prefix("a=rtpmap:")?;
                let (pt, codec) = rest.split_once(' ')?;
                (video && codec.eq_ignore_ascii_case("H264/90000")).then_some(pt)
            })
            .collect();
        for line in section {
            let selected = line
                .strip_prefix("a=fmtp:")
                .and_then(|rest| rest.split_once(' '))
                .filter(|(pt, _)| h264_pts.contains(pt));
            if let Some((pt, params)) = selected {
                let params: Vec<String> = params
                    .split(';')
                    .map(
                        |param| match param.trim().strip_prefix("profile-level-id=") {
                            Some(value)
                                if value.len() == 6
                                    && value.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
                            {
                                format!("profile-level-id={}{level_idc:02x}", &value[..4])
                            }
                            _ => param.to_string(),
                        },
                    )
                    .collect();
                output.push(format!("a=fmtp:{pt} {}", params.join(";")));
            } else {
                output.push((*line).to_string());
            }
        }
        start = end;
    }
    output.join(eol)
}

#[cfg(test)]
mod tests {
    use super::munge_h264_level;

    #[test]
    fn reused_payload_type_does_not_change_another_media_section() {
        for eol in ["\n", "\r\n"] {
            let sdp = [
                "v=0",
                "m=video 9 UDP/TLS/RTP/SAVPF 96 97",
                "a=mid:first",
                "a=rtpmap:96 H264/90000",
                "a=fmtp:96 profile-level-id=42e01f;packetization-mode=1",
                "a=rtpmap:97 rtx/90000",
                "a=fmtp:97 apt=96",
                "m=video 9 UDP/TLS/RTP/SAVPF 96",
                "a=mid:second",
                "a=rtpmap:96 VP8/90000",
                "a=fmtp:96 profile-level-id=42e01f",
                "m=audio 9 UDP/TLS/RTP/SAVPF 96",
                "a=rtpmap:96 opus/48000/2",
                "a=fmtp:96 profile-level-id=42e01f",
                "",
            ]
            .join(eol);
            let expected = sdp.replacen("profile-level-id=42e01f;", "profile-level-id=42e02a;", 1);
            assert_eq!(munge_h264_level(&sdp, 42), expected);
        }
    }

    #[test]
    fn only_level_digits_change_and_invalid_values_preserve_the_description() {
        let sdp = "m=video 9 UDP/TLS/RTP/SAVPF 99\na=rtpmap:99 H264/90000\na=fmtp:99 profile-level-id=640c1f;packetization-mode=1\n";
        assert_eq!(munge_h264_level(sdp, 42), sdp.replace("640c1f", "640c2a"));
        assert_eq!(munge_h264_level(sdp, 0), sdp);
        assert_eq!(munge_h264_level(sdp, 256), sdp);
        assert_eq!(munge_h264_level("", 42), "");
    }
}
