//! Produtor DXGI com mailbox GPU de um quadro, compartilhada com o consumidor WGC.
#[path = "duplication_ffi.rs"]
mod ffi;
use super::{FrameLease, GpuDevice, Mailbox, Shared};
use crate::{CaptureError, CaptureOptions, CaptureStats, system_time_duration};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::{
    Graphics::SizeInt32,
    Win32::Graphics::Dxgi::*,
    core::{Error, HRESULT},
};

pub struct DesktopSource {
    monitor: isize,
    options: CaptureOptions,
    prior_counts: CaptureStats,
    gpu: GpuDevice,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}
impl DesktopSource {
    pub(crate) fn start(
        gpu: GpuDevice,
        monitor: isize,
        options: CaptureOptions,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, CaptureError> {
        Self::start_with_warning(
            gpu,
            monitor,
            options,
            notify,
            Arc::new(AtomicBool::new(false)),
        )
    }
    fn start_with_warning(
        gpu: GpuDevice,
        monitor: isize,
        options: CaptureOptions,
        notify: Arc<dyn Fn() + Send + Sync>,
        warning: Arc<AtomicBool>,
    ) -> Result<Self, CaptureError> {
        let options = options.validate()?;
        let session = ffi::Session::open(&gpu, monitor)?;
        let shared = Arc::new(Shared::with_display_warning(notify, warning));
        shared.set_display_target(Some(crate::CaptureTarget::Monitor(monitor)));
        let size = session.size();
        *shared
            .mailbox
            .lock()
            .map_err(|_| CaptureError::Synchronization)? = Mailbox {
            frame: None,
            size: SizeInt32 {
                Width: size.0 as i32,
                Height: size.1 as i32,
            },
            dirty: false,
            overwritten: 0,
        };
        shared.active.store(true, Ordering::Release);
        let producer = shared.clone();
        let device = gpu.clone();
        let worker = thread::Builder::new()
            .name("desktop-capture".into())
            .spawn(move || {
                if let Err(error) = run(&device, monitor, options, session, &producer) {
                    producer.fail(error);
                }
            })
            .map_err(CaptureError::Worker)?;
        Ok(Self {
            monitor,
            options,
            prior_counts: CaptureStats::default(),
            gpu,
            shared,
            worker: Some(worker),
        })
    }
    pub(crate) fn stats(&self) -> Result<CaptureStats, CaptureError> {
        Ok(self
            .shared
            .stats(self.gpu.frequency)?
            .with_prior_counts(self.prior_counts))
    }
    pub(crate) fn refresh(&mut self) -> Result<(), CaptureError> {
        let counts = self.stats()?;
        let gpu = self.gpu.clone();
        let notify = self.shared.notify.clone();
        self.stop();
        let mut replacement = Self::start_with_warning(
            gpu,
            self.monitor,
            self.options,
            notify,
            self.shared.display_warned.clone(),
        )?;
        replacement.prior_counts = counts;
        *self = replacement;
        Ok(())
    }
    pub(crate) fn latest_frame(
        &self,
        redraw: bool,
    ) -> Result<Option<FrameLease<'_>>, CaptureError> {
        self.shared.latest_frame(redraw)
    }
    pub(crate) fn take_error(&self) -> Option<Error> {
        let code = self.shared.error.swap(0, Ordering::AcqRel);
        (code != 0).then(|| Error::from_hresult(HRESULT(code)))
    }
    pub(crate) fn stop(&mut self) {
        self.shared.active.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            if worker.join().is_err() {
                tracing::error!("Thread de captura DXGI encerrou inesperadamente");
            }
        }
        if let Ok(mut mailbox) = self.shared.mailbox.lock() {
            mailbox.frame = None;
            mailbox.dirty = false;
        }
    }
}
impl Drop for DesktopSource {
    fn drop(&mut self) {
        self.stop();
    }
}

fn recoverable(error: &Error) -> bool {
    matches!(
        error.code(),
        DXGI_ERROR_ACCESS_LOST
            | DXGI_ERROR_NOT_CURRENTLY_AVAILABLE
            | DXGI_ERROR_UNSUPPORTED
            | windows::Win32::Foundation::E_ACCESSDENIED
    )
}

fn run(
    gpu: &GpuDevice,
    monitor: isize,
    options: CaptureOptions,
    initial: ffi::Session,
    shared: &Shared,
) -> windows::core::Result<()> {
    let mut session = Some(initial);
    let mut recovery_started = None;
    let mut compositor = ffi::Compositor::new(gpu)?;
    let timer = ffi::PollTimer::new()?;
    while shared.active.load(Ordering::Acquire) {
        if recovery_started.is_some_and(|start: Instant| start.elapsed() >= Duration::from_secs(5))
        {
            return Err(DXGI_ERROR_ACCESS_LOST.into());
        }
        if session.is_none() {
            match ffi::Session::open(gpu, monitor) {
                Ok(restored) => {
                    shared.refresh_display(true);
                    session = Some(restored);
                    compositor.reset();
                    tracing::info!("Duplicação DXGI recriada; aguardando primeiro quadro");
                }
                Err(error)
                    if recoverable(&error)
                        && recovery_started.is_some_and(|start: Instant| {
                            start.elapsed() < Duration::from_secs(5)
                        }) =>
                {
                    thread::park_timeout(Duration::from_millis(100));
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        let Some(current) = session.as_ref() else {
            continue;
        };
        let acquired = match current.acquire() {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                timer.wait()?;
                continue;
            }
            Err(error) if recoverable(&error) => {
                // A duplicação antiga precisa ser destruída antes de criar outra.
                session = None;
                recovery_started.get_or_insert_with(Instant::now);
                if let Ok(mut mailbox) = shared.mailbox.lock() {
                    mailbox.frame = None;
                    mailbox.dirty = false;
                }
                tracing::warn!(%error, "Captura DXGI interrompida; tentando recuperar o mesmo monitor");
                continue;
            }
            Err(error) => return Err(error),
        };
        if recovery_started.take().is_some() {
            tracing::info!("Captura DXGI recuperada no monitor selecionado");
        }
        compositor.update_pointer(gpu, &acquired)?;
        // DXGI também notifica movimento do cursor sem nova imagem do desktop.
        if acquired.info.LastPresentTime == 0 && !options.cursor {
            continue;
        }
        let arrival = super::ffi::now(gpu.frequency)?;
        let arrival_100ns = (arrival.as_nanos() / 100).min(i64::MAX as u128) as i64;
        let qpc_arrival =
            crate::system_time_to_qpc(arrival_100ns, gpu.frequency).unwrap_or_default();
        let timestamp = acquired.info.LastPresentTime.max(if options.cursor {
            acquired.info.LastMouseUpdateTime
        } else {
            0
        });
        let timestamp = if timestamp > 0 {
            timestamp.min(qpc_arrival)
        } else {
            qpc_arrival
        };
        shared.frames.fetch_add(1, Ordering::Relaxed);
        let time_100ns = (i128::from(timestamp) * 10_000_000 / i128::from(gpu.frequency)) as i64;
        shared.timestamp.store(time_100ns, Ordering::Release);
        if let (Some(time), Ok(mut metrics)) =
            (system_time_duration(time_100ns), shared.metrics.try_lock())
        {
            metrics.counter.record(time);
            let fps = metrics.counter.fps(time);
            shared.fps_bits.store(fps.to_bits(), Ordering::Release);
        }
        let mut mailbox = match shared.mailbox.try_lock() {
            Ok(mailbox) => mailbox,
            Err(_) => {
                shared.contention.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        let size = current.size();
        if mailbox
            .frame
            .as_ref()
            .is_none_or(|frame| frame.size != size)
        {
            mailbox.frame = Some(ffi::render_frame(gpu, size)?);
            mailbox.size = SizeInt32 {
                Width: size.0 as i32,
                Height: size.1 as i32,
            };
        }
        if mailbox.dirty {
            mailbox.overwritten += 1;
        }
        if let Some(frame) = &mut mailbox.frame {
            compositor.compose(
                gpu,
                frame,
                acquired.texture()?,
                current.rotation(),
                options.cursor,
            )?;
            frame.qpc_capture = timestamp;
            frame.qpc_acquired = qpc_arrival;
        }
        mailbox.dirty = true;
        drop(mailbox);
        drop(acquired);
        shared.wake();
    }
    Ok(())
}
