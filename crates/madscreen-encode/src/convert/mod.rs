//! Conversão/escala GPU, fonte sintética e separação NV12/I420 em buffers do chamador.
#[cfg(windows)]
mod convert_ffi;
#[cfg(windows)]
pub use convert_ffi::{Converter, SyntheticSource};
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) mod nv12_i420;
pub mod reduction;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvertMode {
    Shader,
    VideoProcessor,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputFormat {
    Nv12,
    Bgra,
}
