//! Entrada comum da transmissão; o caminho NVENC conserva texturas registradas/zero-copy.
use crate::{AuBuffer, EncodeError, Encoder, EncoderConfig, FrameRate, FrameTiming, Result};
use crate::{
    backend::{self, BackendKind},
    ffmpeg_ffi::FfmpegEncoder,
    nvenc::{Nv12Surface, NvencEncoder},
};
use madscreen_capture::wgc::GpuDevice;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

pub enum Surface {
    Nvenc(Nv12Surface),
    Host(usize),
}
enum Backend {
    Nvenc(Box<NvencEncoder>),
    Ffmpeg(Box<FfmpegEncoder>),
}
pub struct AutoEncoder {
    kind: BackendKind,
    backend: Backend,
}
impl AutoEncoder {
    pub fn new(
        gpu: &GpuDevice,
        config: EncoderConfig,
        textures: &[ID3D11Texture2D],
        bgra: bool,
        requested: Option<BackendKind>,
    ) -> Result<Self> {
        config.validate()?;
        let adapter = crate::platform::device_adapter(&gpu.device)?;
        let (kind, backend) = backend::select(backend::candidates(adapter.vendor_id, requested), |kind| {
            let result = match kind {
                BackendKind::Nvenc => NvencEncoder::new(gpu, config.clone(), textures, bgra).map(|e| Backend::Nvenc(Box::new(e))),
                _ if bgra => Err(EncodeError::InvalidConfig("AMF/QuickSync/x264 exigem entrada NV12")),
                _ => FfmpegEncoder::new(gpu, config.clone(), textures, kind).map(|e| Backend::Ffmpeg(Box::new(e))),
            };
            if let Err(error) = &result { tracing::warn!(encoder=kind.name(), %error, "Encoder indisponível; tentando alternativa"); }
            result
        }).map_err(|errors| EncodeError::EncoderUnavailable(errors.into_iter().map(|(kind, error)| format!("{}: {error}", kind.name())).collect::<Vec<_>>().join("; ")))?;
        tracing::info!(encoder = kind.name(), "Encoder selecionado automaticamente");
        Ok(Self { kind, backend })
    }
    pub fn kind(&self) -> BackendKind {
        self.kind
    }
    /// profile-level-id do SPS real (AMF/QuickSync/x264). NVENC usa o nível configurado.
    pub fn bitstream_profile_level_id(&self) -> Option<&str> {
        match &self.backend {
            Backend::Ffmpeg(encoder) => encoder.bitstream_profile_level_id(),
            Backend::Nvenc(_) => None,
        }
    }
    pub fn surface_at(&self, index: usize) -> Result<Surface> {
        match &self.backend {
            Backend::Nvenc(e) => e.surface_at(index).map(Surface::Nvenc),
            Backend::Ffmpeg(e) if index < e.slots() => Ok(Surface::Host(index)),
            _ => Err(EncodeError::InvalidConfig("slot inválido")),
        }
    }
    pub fn end_of_stream(&mut self) -> Result<()> {
        match &mut self.backend {
            Backend::Nvenc(e) => e.end_of_stream(),
            Backend::Ffmpeg(e) => e.end_of_stream(),
        }
    }
}
impl Encoder for AutoEncoder {
    type Surface = Surface;
    fn encode(&mut self, surface: &Surface, timing: FrameTiming) -> Result<()> {
        match (&mut self.backend, surface) {
            (Backend::Nvenc(e), Surface::Nvenc(s)) => e.encode(s, timing),
            (Backend::Ffmpeg(e), Surface::Host(i)) => e.encode(*i, timing),
            _ => Err(EncodeError::InvalidConfig("superfície de outro backend")),
        }
    }
    fn poll_into(&mut self, output: &mut AuBuffer) -> Result<bool> {
        match &mut self.backend {
            Backend::Nvenc(e) => e.poll_into(output),
            Backend::Ffmpeg(e) => e.poll_into(output, true),
        }
    }
    fn try_poll_into(&mut self, output: &mut AuBuffer) -> Result<bool> {
        match &mut self.backend {
            Backend::Nvenc(e) => e.try_poll_into(output),
            Backend::Ffmpeg(e) => e.poll_into(output, false),
        }
    }
    fn reconfigure_rate(&mut self, bitrate: u32, fps: FrameRate) -> Result<()> {
        match &mut self.backend {
            Backend::Nvenc(e) => e.reconfigure_rate(bitrate, fps),
            Backend::Ffmpeg(e) => e.reconfigure_rate(bitrate, fps),
        }
    }
    fn request_keyframe(&mut self) {
        match &mut self.backend {
            Backend::Nvenc(e) => e.request_keyframe(),
            Backend::Ffmpeg(e) => e.request_keyframe(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requer sessão gráfica D3D11 e runtime FFmpeg"]
    fn runtime_gpu_cpu_feedback() -> std::result::Result<(), Box<dyn std::error::Error>> {
        use crate::{
            FrameKind,
            convert::{ConvertMode, Converter, InputFormat, SyntheticSource},
        };
        let _runtime = crate::platform::Runtime::new()?;
        let gpu = GpuDevice::new(None)?;
        let config = EncoderConfig {
            width: 320,
            height: 180,
            fps: FrameRate::new(60, 1)?,
            bitrate_bps: 2_000_000,
            h264_level: Some(31),
            ..Default::default()
        };
        let mut converter = Converter::new_ring(
            &gpu,
            (320, 180),
            &config,
            ConvertMode::Shader,
            InputFormat::Nv12,
            4,
        )?;
        let mut source = SyntheticSource::new(&gpu, (320, 180))?;
        let mut encoder = AutoEncoder::new(
            &gpu,
            config.clone(),
            converter.textures(),
            false,
            Some(BackendKind::X264),
        )?;
        let mut output = AuBuffer::new(config.au_capacity());
        for index in 0..8 {
            let slot = index as usize % 4;
            converter.begin_frame(slot);
            converter.begin_convert(slot);
            let frame = source.render(index)?;
            let mut timing = FrameTiming {
                slot,
                qpc_capture: frame.qpc_capture,
                ..Default::default()
            };
            timing.qpc_convert_start = qpc()?;
            converter.submit(frame, slot)?;
            converter.flush_slot(slot)?;
            converter.drain_slot(slot)?;
            timing.qpc_convert_end = qpc()?;
            if index == 3 {
                encoder.request_keyframe();
            }
            if index == 5 {
                encoder.reconfigure_rate(1_000_000, FrameRate::new(30, 1)?)?;
            }
            encoder.encode(&encoder.surface_at(slot)?, timing)?;
            if index == 2 {
                assert!(matches!(
                    encoder.reconfigure_rate(1_000_000, config.fps),
                    Err(EncodeError::Busy)
                ));
            }
            assert!(encoder.poll_into(&mut output)?);
            assert_eq!(output.index, index);
            assert_eq!(output.timing.slot, slot);
            assert_eq!(output.kind == FrameKind::Key, matches!(index, 0 | 3 | 5));
            assert!(!encoder.try_poll_into(&mut output)?);
        }
        encoder.end_of_stream()?;
        assert!(!encoder.poll_into(&mut output)?);
        Ok(())
    }
    use crate::platform::qpc;
}
