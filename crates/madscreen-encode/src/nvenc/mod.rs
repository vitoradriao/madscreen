//! NVENC síncrono sobre texturas D3D11 registradas uma vez.
mod encoder_ffi;
pub use encoder_ffi::{CodecCaps, Nv12Surface, NvencEncoder, NvencProbe, probe};
