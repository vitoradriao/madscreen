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
        let automatic = requested == CaptureBackend::Auto;
        let backend = if automatic {
            // Consulta única no início, fora do caminho por frame; desconhecido não força WGC.
            let hdr_enabled = match target {
                CaptureTarget::Monitor(monitor) => {
                    crate::display::monitor_color_info(monitor).hdr_enabled
                }
                CaptureTarget::Window(_) => None,
            };
            automatic_backend(target, version, hdr_enabled)
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
                    gpu, monitor, options, notify, automatic,
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
    /// A Desktop Duplication escolhida pelo automático parou porque o HDR foi ligado no
    /// monitor durante a sessão: o chamador deve recriar a fonte com WGC (mesmo alvo).
    pub fn needs_wgc_switch(&self) -> bool {
        match self {
            Self::Wgc(_) => false,
            Self::Desktop(s) => s.hdr_switch_required(),
        }
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
    /// Best-effort WGC cadence update; Desktop Duplication uses runner pacing.
    pub fn set_target_fps(&self, fps: Option<u32>) -> bool {
        match self {
            Self::Wgc(source) => source.set_target_fps(fps),
            Self::Desktop(_) => false,
        }
    }
}
