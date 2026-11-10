//! Captura WGC livre de dispatcher, com uma única textura GPU latest-wins.
pub(crate) mod duplication;
pub(crate) mod ffi;
use crate::{
    CaptureError, CaptureOptions, CaptureStats, CaptureTarget, ClockOffset, FpsCounter,
    FrameSource, system_time_duration, system_time_to_qpc,
};
use std::{
    sync::{
        Arc, Mutex, MutexGuard, TryLockError,
        atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};
use windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    Graphics::{
        Capture::*,
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    Security::Authorization::AppCapabilityAccess::AppCapabilityAccessStatus,
    Win32::Graphics::{
        Direct3D11::{
            ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D,
        },
        Dxgi::Common::DXGI_FORMAT,
    },
    core::{AgileReference, HRESULT, IInspectable},
};
use windows_future::{AsyncOperationCompletedHandler, AsyncStatus, IAsyncOperation};

/// 1903/1909 capturam normalmente, mas não expõem IGraphicsCaptureSession2 (2004+).
/// Nesses sistemas o cursor segue o padrão do Windows; outras falhas continuam sendo erros.
fn optional_cursor_control(result: windows::core::Result<()>) -> windows::core::Result<()> {
    match result {
        Err(error) if error.code() == windows::Win32::Foundation::E_NOINTERFACE => {
            tracing::info!("Controle de cursor indisponível; usando o cursor padrão do Windows");
            Ok(())
        }
        other => other,
    }
}

#[cfg(test)]
mod compatibility_tests {
    use super::optional_cursor_control;
    use windows::Win32::Foundation::{E_ACCESSDENIED, E_NOINTERFACE};
    #[test]
    fn older_cursor_interface_is_optional_but_access_failure_is_not() {
        assert!(optional_cursor_control(Err(E_NOINTERFACE.into())).is_ok());
        assert!(optional_cursor_control(Ok(())).is_ok());
        assert_eq!(
            optional_cursor_control(Err(E_ACCESSDENIED.into()))
                .err()
                .map(|e| e.code()),
            Some(E_ACCESSDENIED)
        );
    }
}

#[derive(Clone)]
pub struct GpuDevice {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    winrt: AgileReference<IDirect3DDevice>,
    pub(crate) frequency: i64,
}
impl GpuDevice {
    pub fn new(target: Option<CaptureTarget>) -> Result<Self, CaptureError> {
        Ok(ffi::create_device(target)?)
    }
    pub fn check(&self) -> Result<(), CaptureError> {
        Ok(ffi::check_device(self)?)
    }
}

/// Só acessar enquanto o FrameLease estiver vivo; não reter clones da textura/SRV.
pub struct GpuFrame {
    pub texture: ID3D11Texture2D,
    pub view: ID3D11ShaderResourceView,
    /// Instante de captura em QPC, limitado à chegada: SystemRelativeTime do WGC
    /// ou LastPresentTime/LastMouseUpdateTime do DXGI.
    pub qpc_capture: i64,
    /// Aquisição observada localmente. Distinta do timestamp do conteúdo: uma
    /// imagem estática recém-adquirida pode ter SystemRelativeTime antigo.
    pub qpc_acquired: i64,
    pub format: DXGI_FORMAT,
    pub size: (u32, u32),
}
struct Mailbox {
    frame: Option<GpuFrame>,
    size: SizeInt32,
    dirty: bool,
    overwritten: u64,
}
struct Metrics {
    counter: FpsCounter,
}
pub(crate) struct Shared {
    display: Mutex<DisplaySession>,
    display_warned: Arc<AtomicBool>,
    mailbox: Mutex<Mailbox>,
    metrics: Mutex<Metrics>,
    frames: AtomicU64,
    fps_bits: AtomicU64,
    timestamp: AtomicI64,
    min_interval: AtomicI64,
    active: AtomicBool,
    closed: AtomicBool,
    pending: AtomicBool,
    error: AtomicI32,
    contention: AtomicU64,
    offset_min: AtomicI64,
    offset_max: AtomicI64,
    offset_sum: AtomicI64,
    offset_samples: AtomicU64,
    offset_future: AtomicU64,
    border: AtomicU8,
    border_generation: AtomicU64,
    notify: Arc<dyn Fn() + Send + Sync>,
}
#[derive(Default)]
struct DisplaySession {
    target: Option<CaptureTarget>,
    monitor: Option<isize>,
    info: crate::display::DisplayColorInfo,
}
impl Shared {
    fn new(notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self::with_display_warning(notify, Arc::new(AtomicBool::new(false)))
    }
    fn with_display_warning(
        notify: Arc<dyn Fn() + Send + Sync>,
        display_warned: Arc<AtomicBool>,
    ) -> Self {
        Self {
            display: Mutex::new(DisplaySession::default()),
            display_warned,
            mailbox: Mutex::new(Mailbox {
                frame: None,
                size: SizeInt32::default(),
                dirty: false,
                overwritten: 0,
            }),
            metrics: Mutex::new(Metrics {
                counter: FpsCounter::new(Duration::from_secs(1)),
            }),
            frames: AtomicU64::new(0),
            fps_bits: AtomicU64::new(0),
            timestamp: AtomicI64::new(0),
            min_interval: AtomicI64::new(0),
            active: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            pending: AtomicBool::new(false),
            error: AtomicI32::new(0),
            contention: AtomicU64::new(0),
            offset_min: AtomicI64::new(i64::MAX),
            offset_max: AtomicI64::new(i64::MIN),
            offset_sum: AtomicI64::new(0),
            offset_samples: AtomicU64::new(0),
            offset_future: AtomicU64::new(0),
            border: AtomicU8::new(0),
            border_generation: AtomicU64::new(0),
            notify,
        }
    }

    fn set_display_target(&self, target: Option<CaptureTarget>) {
        if let Ok(mut display) = self.display.lock() {
            display.target = target;
        }
        self.refresh_display(true);
    }

    /// Called at lifecycle boundaries or statistics polling (window monitor changes), never per frame.
    fn refresh_display(&self, force: bool) -> crate::display::DisplayColorInfo {
        let Ok(mut display) = self.display.lock() else {
            return crate::display::DisplayColorInfo::default();
        };
        let monitor = display.target.and_then(crate::display::monitor_for_target);
        if force || display.monitor != monitor {
            display.monitor = monitor;
            display.info = monitor
                .map(crate::display::monitor_color_info)
                .unwrap_or_default();
            if crate::display::first_hdr_warning(display.info, &self.display_warned) {
                let color = display.info;
                tracing::warn!(
                    hdr_supported = ?color.hdr_supported,
                    hdr_enabled = true,
                    color_space = ?color.color_space,
                    bits = ?color.bits,
                    "HDR ativo na fonte; captura BGRA8/SDR sem conversão HDR qualificada"
                );
            }
        }
        display.info
    }

    fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            (self.notify)();
        }
    }
    fn fail(&self, error: windows::core::Error) {
        self.error.store(error.code().0, Ordering::Release);
        self.wake();
    }
}

pub struct FrameLease<'a> {
    guard: MutexGuard<'a, Mailbox>,
}
impl FrameLease<'_> {
    pub fn frame(&self) -> Option<&GpuFrame> {
        self.guard.frame.as_ref()
    }
}

pub struct WgcSource {
    prior_counts: CaptureStats,
    gpu: GpuDevice,
    item: Option<GraphicsCaptureItem>,
    pool: Option<Direct3D11CaptureFramePool>,
    session: Option<GraphicsCaptureSession>,
    frame_token: Option<i64>,
    closed_token: Option<i64>,
    border_request: Mutex<Option<IAsyncOperation<AppCapabilityAccessStatus>>>,
    shared: Arc<Shared>,
}
impl WgcSource {
    pub fn new(gpu: GpuDevice, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            prior_counts: CaptureStats::default(),
            gpu,
            item: None,
            pool: None,
            session: None,
            frame_token: None,
            closed_token: None,
            border_request: Mutex::new(None),
            shared: Arc::new(Shared::new(notify)),
        }
    }

    /// Inicia no device fornecido a `new`, permitindo compartilhar o mesmo device com NVENC.
    pub fn start_on_device(
        &mut self,
        target: CaptureTarget,
        options: CaptureOptions,
    ) -> Result<(), CaptureError> {
        self.start_item_for_target(ffi::item_for_target(target)?, options, Some(target))
    }

    pub fn start_item(
        &mut self,
        item: GraphicsCaptureItem,
        options: CaptureOptions,
    ) -> Result<(), CaptureError> {
        // WinRT picker items have no reverse HMONITOR API. Never guess the primary display.
        self.start_item_for_target(item, options, None)
    }

    fn start_item_for_target(
        &mut self,
        item: GraphicsCaptureItem,
        options: CaptureOptions,
        target: Option<CaptureTarget>,
    ) -> Result<(), CaptureError> {
        let options = options.validate()?;
        self.stop();
        // Cada sessão possui seu estado: callbacks atrasados não atingem a próxima fonte.
        self.shared = Arc::new(Shared::with_display_warning(
            self.shared.notify.clone(),
            self.shared.display_warned.clone(),
        ));
        self.shared.set_display_target(target);
        if !GraphicsCaptureSession::IsSupported()? {
            return Err(CaptureError::Unsupported);
        }
        let size = item.Size()?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err(CaptureError::Unsupported);
        }
        *self
            .shared
            .mailbox
            .lock()
            .map_err(|_| CaptureError::Synchronization)? = Mailbox {
            frame: Some(ffi::create_frame(
                &self.gpu,
                (size.Width as u32, size.Height as u32),
            )?),
            size,
            dirty: false,
            overwritten: 0,
        };
        // Registrar os recursos antes dos handlers permite Drop limpar falhas parciais.
        self.item = Some(item.clone());
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &self.gpu.winrt.resolve()?,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            options.buffer_count,
            size,
        )?;
        self.pool = Some(pool.clone());
        let session = pool.CreateCaptureSession(&item)?;
        self.session = Some(session.clone());
        optional_cursor_control(session.SetIsCursorCaptureEnabled(options.cursor))?;
        // No Windows 11 24H2 o intervalo padrão limita a captura a ~60 FPS (mesmo com o
        // conteúdo a mais de 100 FPS). Sempre definimos um intervalo mínimo explícito.
        let interval = TimeSpan {
            Duration: crate::min_update_interval_100ns(options.target_fps),
        };
        match session.SetMinUpdateInterval(interval) {
            Ok(()) => {
                self.shared
                    .min_interval
                    .store(interval.Duration, Ordering::Release);
                tracing::info!(
                    interval_us = interval.Duration / 10,
                    "MinUpdateInterval definido"
                );
            }
            Err(error) => {
                tracing::warn!(%error, "MinUpdateInterval indisponível (Windows anterior à 24H2); a cadência de captura é a do sistema");
            }
        }
        let shared = self.shared.clone();
        let gpu = self.gpu.clone();
        self.shared.active.store(true, Ordering::Release);
        self.frame_token = Some(pool.FrameArrived(&TypedEventHandler::<
            Direct3D11CaptureFramePool,
            IInspectable,
        >::new(move |sender, _| {
            if !shared.active.load(Ordering::Acquire) {
                return Ok(());
            }
            if let Some(pool) = sender.as_ref() {
                if let Err(error) = arrived(pool, &gpu, &shared, options.buffer_count) {
                    shared.fail(error);
                }
            }
            Ok(())
        }))?);
        let shared = self.shared.clone();
        self.closed_token = Some(item.Closed(&TypedEventHandler::<
            GraphicsCaptureItem,
            IInspectable,
        >::new(move |_, _| {
            if shared.active.load(Ordering::Acquire) {
                shared.closed.store(true, Ordering::Release);
                shared.wake();
            }
            Ok(())
        }))?);
        self.set_border(options.border);
        session.StartCapture()?;
        Ok(())
    }

    pub fn set_cursor(&self, enabled: bool) -> Result<(), CaptureError> {
        if let Some(session) = &self.session {
            optional_cursor_control(session.SetIsCursorCaptureEnabled(enabled))?;
        }
        Ok(())
    }

    /// Change capture cadence without recreating the pool or capture item.
    pub fn set_target_fps(&self, fps: Option<u32>) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        let interval = TimeSpan {
            Duration: crate::min_update_interval_100ns(fps),
        };
        match session.SetMinUpdateInterval(interval) {
            Ok(()) => {
                self.shared
                    .min_interval
                    .store(interval.Duration, Ordering::Release);
                tracing::info!(
                    ?fps,
                    interval_us = interval.Duration / 10,
                    "Cadência WGC ajustada"
                );
                true
            }
            Err(error) => {
                static WARN_ONCE: std::sync::Once = std::sync::Once::new();
                WARN_ONCE.call_once(|| {
                    tracing::warn!(%error, "MinUpdateInterval indisponível; usando a grade de captura do runner");
                });
                false
            }
        }
    }

    pub fn set_border(&self, required: bool) {
        let Some(session) = &self.session else {
            return;
        };
        let generation = self.shared.border_generation.fetch_add(1, Ordering::AcqRel) + 1;
        if let Ok(mut request) = self.border_request.lock() {
            if let Some(operation) = request.take() {
                let _ = operation.Cancel();
            }
        }
        self.shared
            .border
            .store(if required { 0 } else { 1 }, Ordering::Release);
        if required {
            match session.SetIsBorderRequired(true) {
                Ok(()) => tracing::info!("Borda solicitada ao Windows"),
                Err(error) => {
                    tracing::info!(%error, "Controle de borda indisponível; mantendo borda do sistema")
                }
            }
            return;
        }
        let operation = match GraphicsCaptureAccess::RequestAccessAsync(
            GraphicsCaptureAccessKind::Borderless,
        ) {
            Ok(operation) => operation,
            Err(error) => {
                self.shared.border.store(3, Ordering::Release);
                tracing::info!(%error, "Sem permissão/API para remover borda; captura continua com borda");
                return;
            }
        };
        let session = session.clone();
        let shared = self.shared.clone();
        let result = operation.SetCompleted(&AsyncOperationCompletedHandler::new(move |operation, status| {
            if !shared.active.load(Ordering::Acquire)
                || shared.border_generation.load(Ordering::Acquire) != generation
                || shared.border.load(Ordering::Acquire) != 1 { return Ok(()); }
            let result = operation.as_ref().map(|op| op.GetResults());
            if status == AsyncStatus::Completed && matches!(result, Some(Ok(AppCapabilityAccessStatus::Allowed))) {
                match session.SetIsBorderRequired(false) {
                    Ok(()) => { shared.border.store(2, Ordering::Release); tracing::info!("Remoção da borda solicitada; outras sessões/políticas ainda podem exibi-la"); }
                    Err(error) => { shared.border.store(3, Ordering::Release); tracing::info!(%error, "Borda não removida; captura continua"); }
                }
            } else {
                shared.border.store(3, Ordering::Release);
                tracing::info!("Remoção da borda não autorizada; mantendo borda do sistema");
            }
            Ok(())
        }));
        match result {
            Ok(()) => {
                if let Ok(mut request) = self.border_request.lock() {
                    *request = Some(operation);
                }
            }
            Err(error) => {
                self.shared.border.store(3, Ordering::Release);
                tracing::info!(%error, "Falha ao solicitar borda; mantendo borda do sistema");
            }
        }
    }

    pub fn border_status(&self) -> &'static str {
        match self.shared.border.load(Ordering::Acquire) {
            1 => "aguardando permissão",
            2 => "remoção solicitada",
            3 => "sistema",
            _ => "ligada",
        }
    }
    /// Intervalo mínimo aplicado à sessão; `None` quando o Windows não oferece a propriedade.
    pub fn min_update_interval(&self) -> Option<Duration> {
        let units = self.shared.min_interval.load(Ordering::Acquire);
        u64::try_from(units)
            .ok()
            .filter(|units| *units > 0)
            .map(|units| Duration::from_nanos(units * 100))
    }
    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }
    pub fn take_error(&self) -> Option<windows::core::Error> {
        let code = self.shared.error.swap(0, Ordering::AcqRel);
        (code != 0).then(|| windows::core::Error::from_hresult(HRESULT(code)))
    }
    pub fn stats(&self) -> Result<CaptureStats, CaptureError> {
        Ok(self
            .shared
            .stats(self.gpu.frequency)?
            .with_prior_counts(self.prior_counts))
    }

    /// Nova aquisição do mesmo item autorizado, mantendo device e publicação.
    /// start_item troca Shared, impedindo callbacks antigos de repor a mailbox.
    pub fn refresh(&mut self, options: CaptureOptions) -> Result<(), CaptureError> {
        let item = self.item.clone().ok_or(CaptureError::Unsupported)?;
        let target = self
            .shared
            .display
            .lock()
            .ok()
            .and_then(|display| display.target);
        self.prior_counts = self.stats()?;
        self.start_item_for_target(item, options, target)
    }
    pub fn latest_frame(&self, redraw: bool) -> Result<Option<FrameLease<'_>>, CaptureError> {
        self.shared.latest_frame(redraw)
    }
}
impl FrameSource for WgcSource {
    type Frame<'a> = FrameLease<'a>;
    fn start(
        &mut self,
        target: CaptureTarget,
        options: CaptureOptions,
    ) -> Result<(), CaptureError> {
        self.stop();
        self.gpu = GpuDevice::new(Some(target))?;
        self.start_on_device(target, options)
    }
    fn next_frame(&mut self, _timeout: Duration) -> Result<Option<Self::Frame<'_>>, CaptureError> {
        self.latest_frame(false)
    }
    fn stop(&mut self) {
        self.shared.active.store(false, Ordering::Release);
        if let Ok(mut request) = self.border_request.lock() {
            if let Some(operation) = request.take() {
                let _ = operation.Cancel();
            }
        }
        if let Some(pool) = &self.pool {
            if let Some(token) = self.frame_token.take() {
                let _ = pool.RemoveFrameArrived(token);
            }
        }
        if let Some(item) = &self.item {
            if let Some(token) = self.closed_token.take() {
                let _ = item.RemoveClosed(token);
            }
        }
        if let Some(session) = self.session.take() {
            let _ = session.Close();
        }
        if let Some(pool) = self.pool.take() {
            let _ = pool.Close();
        }
        self.item = None;
        if let Ok(mut mailbox) = self.shared.mailbox.lock() {
            mailbox.frame = None;
            mailbox.dirty = false;
        }
    }
}
impl Drop for WgcSource {
    fn drop(&mut self) {
        self.stop();
    }
}

struct PoolFrame(Direct3D11CaptureFrame);
impl Drop for PoolFrame {
    fn drop(&mut self) {
        let _ = self.0.Close();
    }
}

fn arrived(
    pool: &Direct3D11CaptureFramePool,
    gpu: &GpuDevice,
    shared: &Shared,
    buffers: i32,
) -> windows::core::Result<()> {
    let frame = match pool.TryGetNextFrame() {
        Ok(frame) => PoolFrame(frame),
        Err(error) if error.code() == windows::Win32::Foundation::E_POINTER => return Ok(()),
        Err(error) => return Err(error),
    };
    let time = frame.0.SystemRelativeTime()?.Duration;
    // Chegada em unidades de 100 ns do mesmo QPC; o WGC pode carimbar o frame com o instante
    // (futuro) de apresentação do DWM, então o capture nunca passa da chegada.
    let arrival = i64::try_from(ffi::now(gpu.frequency)?.as_nanos() / 100).unwrap_or(i64::MAX);
    let offset = arrival.saturating_sub(time);
    shared.offset_min.fetch_min(offset, Ordering::Relaxed);
    shared.offset_max.fetch_max(offset, Ordering::Relaxed);
    shared.offset_sum.fetch_add(offset, Ordering::Relaxed);
    shared.offset_samples.fetch_add(1, Ordering::Relaxed);
    if offset < 0 {
        shared.offset_future.fetch_add(1, Ordering::Relaxed);
    }
    shared.frames.fetch_add(1, Ordering::Relaxed);
    shared.timestamp.store(time, Ordering::Release);
    if let (Some(timestamp), Ok(mut metrics)) =
        (system_time_duration(time), shared.metrics.try_lock())
    {
        metrics.counter.record(timestamp);
        let fps = metrics.counter.fps(timestamp);
        shared.fps_bits.store(fps.to_bits(), Ordering::Release);
    }
    let size = frame.0.ContentSize()?;
    if size.Width <= 0 || size.Height <= 0 {
        return Ok(());
    }
    let mut mailbox = match shared.mailbox.try_lock() {
        Ok(mailbox) => mailbox,
        Err(_) => {
            shared.contention.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
    };
    if size != mailbox.size {
        mailbox.frame = None;
        mailbox.size = size;
        mailbox.dirty = false;
        drop(mailbox);
        drop(frame);
        pool.Recreate(
            &gpu.winrt.resolve()?,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            buffers,
            size,
        )?;
        shared.refresh_display(true);
        return Ok(());
    }
    let texture = ffi::surface_texture(&frame.0.Surface()?)?;
    let expected = (size.Width as u32, size.Height as u32);
    if ffi::texture_size(&texture) != expected {
        return Ok(());
    }
    if mailbox.frame.is_none() {
        mailbox.frame = Some(ffi::create_frame(gpu, expected)?);
    }
    if mailbox.dirty {
        mailbox.overwritten += 1;
    }
    if let Some(dst) = &mut mailbox.frame {
        ffi::copy(gpu, dst, &texture);
        dst.qpc_capture = system_time_to_qpc(time.min(arrival), gpu.frequency).unwrap_or_default();
        dst.qpc_acquired = system_time_to_qpc(arrival, gpu.frequency).unwrap_or_default();
    }
    mailbox.dirty = true;
    drop(texture);
    drop(frame);
    drop(mailbox);
    shared.wake();
    Ok(())
}

impl Shared {
    pub(crate) fn stats(&self, frequency: i64) -> Result<CaptureStats, CaptureError> {
        let display = self.refresh_display(false);
        // A UI lê snapshots atômicos; nunca disputa o contador com FrameArrived.
        let now = ffi::now(frequency)?;
        let recent = system_time_duration(self.timestamp.load(Ordering::Acquire))
            .is_some_and(|last| now.saturating_sub(last) <= Duration::from_secs(1));
        let fps = if recent {
            f64::from_bits(self.fps_bits.load(Ordering::Acquire))
        } else {
            0.0
        };
        let mailbox = self
            .mailbox
            .lock()
            .map_err(|_| CaptureError::Synchronization)?;
        let samples = self.offset_samples.load(Ordering::Relaxed);
        let ms = |ticks: i64| ticks as f64 / 10_000.;
        let clock_offset = if samples == 0 {
            ClockOffset::default()
        } else {
            ClockOffset {
                samples,
                min_ms: ms(self.offset_min.load(Ordering::Relaxed)),
                mean_ms: ms(self.offset_sum.load(Ordering::Relaxed)) / samples as f64,
                max_ms: ms(self.offset_max.load(Ordering::Relaxed)),
                future_frames: self.offset_future.load(Ordering::Relaxed),
            }
        };
        Ok(CaptureStats {
            display,
            size: (
                mailbox.size.Width.max(0) as u32,
                mailbox.size.Height.max(0) as u32,
            ),
            capture_fps: fps,
            frames: self.frames.load(Ordering::Relaxed),
            overwritten: mailbox.overwritten,
            contention_drops: self.contention.load(Ordering::Relaxed),
            clock_offset,
        })
    }

    /// Manter o empréstimo apenas até submeter Draw; soltar antes de Present/vsync.
    pub fn latest_frame(&self, redraw: bool) -> Result<Option<FrameLease<'_>>, CaptureError> {
        self.pending.store(false, Ordering::Release);
        let mut guard = match self.mailbox.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => return Err(CaptureError::Synchronization),
        };
        if (!guard.dirty && !redraw) || guard.frame.is_none() {
            return Ok(None);
        }
        guard.dirty = false;
        Ok(Some(FrameLease { guard }))
    }
}
