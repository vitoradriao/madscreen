pub(crate) mod dll_ffi;
mod nvml_ffi;
mod platform_ffi;
pub use nvml_ffi::{GpuSample, GpuSampler};
pub use platform_ffi::*;
