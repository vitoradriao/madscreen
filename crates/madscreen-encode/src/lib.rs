//! Codificação de vídeo com seleção NVENC D3D11, AMD AMF, Intel QSV e CPU x264.
pub mod backend;
pub mod bitstream;
pub mod config;
pub mod metrics;
pub mod pacer;
pub mod pipeline;
pub mod rate;
// Bindings e configuração ABI também são testados sem GPU no Linux x64.
#[cfg(windows)]
pub mod auto;
pub mod convert;
#[cfg(windows)]
mod ffmpeg_ffi;
#[cfg(windows)]
pub mod nvenc;
#[allow(dead_code)]
#[path = "nvenc/nvenc_sys.rs"]
pub mod nvenc_sys;
pub mod parameters_ffi;
#[cfg(windows)]
pub mod platform;
pub mod versions;

pub use config::{EncoderConfig, FrameRate, MultiPass, Preset, VideoCodec};

#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    #[error("configuração inválida: {0}")]
    InvalidConfig(&'static str),
    #[error("EncoderUnavailable: {0}")]
    EncoderUnavailable(String),
    #[error("{operation}: {status} ({code}); {hint}")]
    Nvenc {
        operation: &'static str,
        status: &'static str,
        code: i32,
        hint: &'static str,
    },
    #[error("buffer insuficiente: precisa de {required} bytes, capacidade {capacity}")]
    BufferTooSmall { required: usize, capacity: usize },
    #[error("encoder ocupado; descarte o frame antigo")]
    Busy,
    #[error("sessão de vídeo falhou; recrie o encoder antes de enviar outro frame")]
    SessionFailed,
    #[error("bitstream inválido: {0}")]
    InvalidBitstream(&'static str),
    #[cfg(windows)]
    #[error(transparent)]
    Windows(#[from] windows::core::Error),
}
pub type Result<T> = std::result::Result<T, EncodeError>;

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct FrameTiming {
    pub qpc_capture: i64,
    pub qpc_convert_start: i64,
    pub qpc_convert_end: i64,
    pub qpc_encode_start: i64,
    pub qpc_encode_end: i64,
    pub qpc_encode_return: i64,
    pub slot: usize,
    pub readback_submit_ms: Option<f64>,
    pub readback_map_ms: Option<f64>,
    pub readback_copy_ms: Option<f64>,
    pub codec_send_ms: Option<f64>,
    pub gen_gpu_ms: Option<f64>,
    pub convert_gpu_ms: Option<f64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    Key,
    Delta,
}

/// Access unit em armazenamento do consumidor. Reservar antes do primeiro encode.
/// `poll_into` nunca aumenta a capacidade: retorna erro se a reserva for insuficiente.
pub struct AuBuffer {
    pub data: Vec<u8>,
    pub kind: FrameKind,
    pub timing: FrameTiming,
    pub index: u64,
}
pub type EncodedAccessUnit = AuBuffer;
impl AuBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            kind: FrameKind::Delta,
            timing: FrameTiming::default(),
            index: 0,
        }
    }
    pub fn copy_from(&mut self, data: &[u8]) -> Result<()> {
        if data.len() > self.data.capacity() {
            return Err(EncodeError::BufferTooSmall {
                required: data.len(),
                capacity: self.data.capacity(),
            });
        }
        self.data.clear();
        self.data.extend_from_slice(data);
        Ok(())
    }
}

/// Sessão configurada na construção. Superfície pertence ao anel do backend.
/// `poll_into` substitui o `poll` alocador do esqueleto do brief.
pub trait Encoder: Send {
    type Surface;
    fn encode(&mut self, surface: &Self::Surface, timing: FrameTiming) -> Result<()>;
    fn poll_into(&mut self, output: &mut AuBuffer) -> Result<bool>;
    /// Coleta sem esperar; backends antigos podem declarar falta de suporte.
    fn try_poll_into(&mut self, _output: &mut AuBuffer) -> Result<bool> {
        Err(EncodeError::InvalidConfig(
            "poll não bloqueante indisponível",
        ))
    }
    fn reconfigure_rate(&mut self, bitrate_bps: u32, fps: FrameRate) -> Result<()>;
    fn request_keyframe(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn au_storage_does_not_grow() -> Result<()> {
        let mut buffer = AuBuffer::new(128);
        let address = buffer.data.as_ptr();
        buffer.copy_from(&[5; 128])?;
        buffer.copy_from(&[7; 16])?;
        assert_eq!(buffer.data.as_ptr(), address);
        assert_eq!(buffer.data.capacity(), 128);
        assert!(matches!(
            buffer.copy_from(&[0; 129]),
            Err(EncodeError::BufferTooSmall { .. })
        ));
        assert_eq!(buffer.data, &[7; 16]);
        Ok(())
    }
}
