use crate::{EncodeError, Result, nvenc_sys as sys};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    #[default]
    H264,
    Hevc,
    Av1,
}
impl VideoCodec {
    pub fn guid(self) -> sys::GUID {
        match self {
            Self::H264 => sys::NV_ENC_CODEC_H264_GUID,
            Self::Hevc => sys::NV_ENC_CODEC_HEVC_GUID,
            Self::Av1 => sys::NV_ENC_CODEC_AV1_GUID,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    #[default]
    P1,
    P2,
    P3,
    P4,
    P5,
    P6,
    P7,
}
impl Preset {
    pub fn guid(self) -> sys::GUID {
        match self {
            Self::P1 => sys::NV_ENC_PRESET_P1_GUID,
            Self::P2 => sys::NV_ENC_PRESET_P2_GUID,
            Self::P3 => sys::NV_ENC_PRESET_P3_GUID,
            Self::P4 => sys::NV_ENC_PRESET_P4_GUID,
            Self::P5 => sys::NV_ENC_PRESET_P5_GUID,
            Self::P6 => sys::NV_ENC_PRESET_P6_GUID,
            Self::P7 => sys::NV_ENC_PRESET_P7_GUID,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum MultiPass {
    #[default]
    Disabled,
    QuarterRes,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct FrameRate {
    pub num: u32,
    pub den: u32,
}
impl FrameRate {
    pub fn new(num: u32, den: u32) -> Result<Self> {
        if num == 0 || den == 0 || u64::from(num) > 1000 * u64::from(den) {
            return Err(EncodeError::InvalidConfig(
                "FPS deve estar em (0, 1000] e denominador > 0",
            ));
        }
        Ok(Self { num, den })
    }
    pub fn as_f64(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct IntraRefresh {
    pub period: u32,
    pub count: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum H264Profile {
    #[default]
    High,
    Main,
    ConstrainedBaseline,
}
#[derive(Clone, Debug, Serialize)]
pub struct EncoderConfig {
    pub codec: VideoCodec,
    /// Nível NVENC para H.264; None seleciona o nível automaticamente.
    pub h264_level: Option<u32>,
    pub h264_profile: H264Profile,
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    pub bitrate_bps: u32,
    pub vbv_frames: u32,
    pub preset: Preset,
    pub multipass: MultiPass,
    /// Laboratory spatial AQ; disabled by default. No temporal AQ/lookahead.
    pub spatial_aq: bool,
    pub readback_metrics: bool,
    pub intra_refresh: Option<IntraRefresh>,
}
impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            codec: VideoCodec::H264,
            h264_level: None,
            h264_profile: H264Profile::ConstrainedBaseline,
            width: 1920,
            height: 1080,
            fps: FrameRate { num: 144, den: 1 },
            bitrate_bps: 18_000_000,
            vbv_frames: 1,
            preset: Preset::P1,
            multipass: MultiPass::Disabled,
            spatial_aq: false,
            readback_metrics: false,
            intra_refresh: None,
        }
    }
}
impl EncoderConfig {
    pub fn validate(&self) -> Result<()> {
        FrameRate::new(self.fps.num, self.fps.den)?;
        if self.codec == VideoCodec::H264
            && self.h264_level.is_some_and(|level| {
                !i32::try_from(level).is_ok_and(|level| {
                    matches!(
                        level,
                        sys::NV_ENC_LEVEL_H264_1
                            | sys::NV_ENC_LEVEL_H264_11
                            | sys::NV_ENC_LEVEL_H264_12
                            | sys::NV_ENC_LEVEL_H264_13
                            | sys::NV_ENC_LEVEL_H264_2
                            | sys::NV_ENC_LEVEL_H264_21
                            | sys::NV_ENC_LEVEL_H264_22
                            | sys::NV_ENC_LEVEL_H264_3
                            | sys::NV_ENC_LEVEL_H264_31
                            | sys::NV_ENC_LEVEL_H264_32
                            | sys::NV_ENC_LEVEL_H264_4
                            | sys::NV_ENC_LEVEL_H264_41
                            | sys::NV_ENC_LEVEL_H264_42
                            | sys::NV_ENC_LEVEL_H264_5
                            | sys::NV_ENC_LEVEL_H264_51
                            | sys::NV_ENC_LEVEL_H264_52
                            | sys::NV_ENC_LEVEL_H264_60
                            | sys::NV_ENC_LEVEL_H264_61
                            | sys::NV_ENC_LEVEL_H264_62
                    )
                })
            })
        {
            return Err(EncodeError::InvalidConfig("nível H.264 inválido"));
        }
        if self.width == 0
            || self.height == 0
            || self.width > 16384
            || self.height > 16384
            || self.width % 2 != 0
            || self.height % 2 != 0
        {
            return Err(EncodeError::InvalidConfig(
                "NV12 exige dimensões pares, entre 2 e 16384",
            ));
        }
        if self.bitrate_bps == 0 || self.vbv_frames == 0 || self.vbv_bits()? == 0 {
            return Err(EncodeError::InvalidConfig(
                "bitrate e VBV devem ser positivos",
            ));
        }
        if let Some(i) = self.intra_refresh {
            if i.count == 0 || i.period <= i.count {
                return Err(EncodeError::InvalidConfig(
                    "intra-refresh exige 0 < count < period",
                ));
            }
        }
        Ok(())
    }
    pub fn vbv_bits(&self) -> Result<u32> {
        if self.fps.num == 0 || self.fps.den == 0 {
            return Err(EncodeError::InvalidConfig("FPS inválido"));
        }
        let bits =
            u128::from(self.bitrate_bps) * u128::from(self.fps.den) * u128::from(self.vbv_frames)
                / u128::from(self.fps.num);
        u32::try_from(bits).map_err(|_| EncodeError::InvalidConfig("VBV excede u32"))
    }
    /// Reserva conservadora para AU intra; erro explícito em vez de realocação se excedida.
    pub fn au_capacity(&self) -> usize {
        (self.width as usize * self.height as usize * 4).max(1024 * 1024)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_and_rational_vbv() -> Result<()> {
        let mut c = EncoderConfig::default();
        c.validate()?;
        assert_eq!(c.vbv_bits()?, 125000);
        c.fps = FrameRate::new(60000, 1001)?;
        c.bitrate_bps = 12_000_000;
        assert_eq!(c.vbv_bits()?, 200200);
        c.vbv_frames = 2;
        assert_eq!(c.vbv_bits()?, 400400);
        c.width = 1919;
        assert!(c.validate().is_err());
        assert!(FrameRate::new(0, 1).is_err());
        assert!(FrameRate::new(1, 0).is_err());
        c.width = 1920;
        c.vbv_frames = u32::MAX;
        assert!(c.validate().is_err());
        Ok(())
    }
    #[test]
    fn guid_values() {
        fn hex(guid: sys::GUID) -> String {
            let mut bytes = [0u8; 16];
            bytes[..4].copy_from_slice(&guid.Data1.to_be_bytes());
            bytes[4..6].copy_from_slice(&guid.Data2.to_be_bytes());
            bytes[6..8].copy_from_slice(&guid.Data3.to_be_bytes());
            bytes[8..].copy_from_slice(&guid.Data4);
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        }
        assert_eq!(
            hex(VideoCodec::H264.guid()),
            "6bc827624e634ca4aa851e50f321f6bf"
        );
        assert_eq!(
            hex(VideoCodec::Hevc.guid()),
            "790cdc8845224d7b9425bda9975f7603"
        );
        assert_eq!(
            hex(VideoCodec::Av1.guid()),
            "0a3522890aa74759862d5d15cd16d254"
        );
        assert_eq!(hex(Preset::P1.guid()), "fc0a8d3e45f84cf880c7298871590ebf");
        assert_eq!(hex(Preset::P2.guid()), "f581cfb888d6438193f0df13f9c27dab");
        assert_eq!(hex(Preset::P3.guid()), "368501103a07441f94d53670631f91f6");
        assert_eq!(hex(Preset::P4.guid()), "90a7b826df064862b9d2cd6d73a08681");
        assert_eq!(hex(Preset::P5.guid()), "21c6e6b4297a4cba998fb6cbde72ade3");
        assert_eq!(hex(Preset::P6.guid()), "8e75c27962994ab683020b215a335cf5");
        assert_eq!(hex(Preset::P7.guid()), "84848c126f714c13931b53e283f57974");
    }
}
