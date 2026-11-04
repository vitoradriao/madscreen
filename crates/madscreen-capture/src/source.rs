//! Seleção de backend no engine; nenhum fallback transforma janela em monitor.
use crate::wgc::{FrameLease, GpuDevice, WgcSource, duplication::DesktopSource};
use crate::{
    CaptureBackend, CaptureError, CaptureOptions, CaptureStats, CaptureTarget, automatic_backend,
};
use std::sync::Arc;
use windows::Graphics::Capture::GraphicsCaptureItem;

pub enum CaptureSource {
    Wgc(WgcSource),
    Desktop(DesktopSource),
}

impl CaptureSource {
    pub fn start(
        gpu: GpuDevice,
        target: CaptureTarget,
        options: CaptureOptions,
        requested: CaptureBackend,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, CaptureError> {
        let version = crate::wgc::ffi::windows_version();
        let backend = if requested == CaptureBackend::Auto {
            automatic_backend(target, version)
        } else {
            requested
        };
        tracing::info!(?version, ?backend, "Backend de captura selecionado");
        match backend {
            CaptureBackend::DesktopDuplication => {
                let CaptureTarget::Monitor(monitor) = target else {
                    return Err(CaptureError::Unsupported);
                };
                Ok(Self::Desktop(DesktopSource::start(
                    gpu, monitor, options, notify,
                )?))
            }
            _ => {
                let mut source = WgcSource::new(gpu, notify);
                source.start_on_device(target, options)?;
                Ok(Self::Wgc(source))
            }
        }
    }
    /// O picker WinRT devolve item opaco, sem HMONITOR: mantém WGC.
    pub fn start_item(
        gpu: GpuDevice,
        item: GraphicsCaptureItem,
        options: CaptureOptions,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, CaptureError> {
        let mut source = WgcSource::new(gpu, notify);
        source.start_item(item, options)?;
        Ok(Self::Wgc(source))
    }
    pub fn backend_name(&self) -> &'static str {
        match self {
            Self::Wgc(_) => "wgc",
            Self::Desktop(_) => "desktop-duplication",
        }
    }
    pub fn stats(&self) -> Result<CaptureStats, CaptureError> {
        match self {
            Self::Wgc(s) => s.stats(),
            Self::Desktop(s) => s.stats(),
        }
    }
    pub fn latest_frame(&self, redraw: bool) -> Result<Option<FrameLease<'_>>, CaptureError> {
        match self {
            Self::Wgc(s) => s.latest_frame(redraw),
            Self::Desktop(s) => s.latest_frame(redraw),
        }
    }
    pub fn take_error(&self) -> Option<windows::core::Error> {
        match self {
            Self::Wgc(s) => s.take_error(),
            Self::Desktop(s) => s.take_error(),
        }
    }
    pub fn is_closed(&self) -> bool {
        match self {
            Self::Wgc(s) => s.is_closed(),
            Self::Desktop(_) => false,
        }
    }
    pub fn stop(&mut self) {
        match self {
            Self::Wgc(s) => crate::FrameSource::stop(s),
            Self::Desktop(s) => s.stop(),
        }
    }
    /// Reabre somente a aquisição do alvo atual, sem seletor ou troca de alvo.
    pub fn refresh(&mut self, options: CaptureOptions) -> Result<(), CaptureError> {
        match self {
            Self::Wgc(s) => s.refresh(options),
            Self::Desktop(s) => s.refresh(),
        }
    }
}
