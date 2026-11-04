//! Limites H.264 obtidos do NVENC para o planejamento e a listagem de fontes.
use madscreen_encode::{VideoCodec, nvenc};
use madscreen_ipc::EncoderLimits;

pub fn h264_limits(probe: &nvenc::NvencProbe) -> Option<EncoderLimits> {
    let caps = probe
        .codecs
        .iter()
        .find(|caps| caps.codec == VideoCodec::H264)?;
    if caps.max_width <= 0 || caps.max_height <= 0 {
        return None;
    }
    let defaults = EncoderLimits::default();
    Some(EncoderLimits {
        max_width: caps.max_width as u32,
        max_height: caps.max_height as u32,
        min_width: if caps.min_width > 0 {
            caps.min_width as u32
        } else {
            defaults.min_width
        },
        min_height: if caps.min_height > 0 {
            caps.min_height as u32
        } else {
            defaults.min_height
        },
    })
}
