mod renderer_ffi;
mod window_ffi;
use crate::options::Options;
use anyhow::Context;
use madscreen_capture::{
    CaptureOptions, FpsCounter, FrameSource,
    wgc::{GpuDevice, WgcSource},
};
use renderer_ffi::Renderer;
use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use window_ffi::Windows;
use windows::{
    Graphics::Capture::{GraphicsCaptureItem, GraphicsCapturePicker},
    Win32::{
        Foundation::*,
        Graphics::Dxgi::{DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET},
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};
use windows_future::{AsyncOperationCompletedHandler, AsyncStatus, IAsyncOperation};

const PICK_BUTTON: usize = 100;
const WM_APP_PICKED: u32 = WM_APP + 1;
const WM_APP_FRAME: u32 = WM_APP + 2;
thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static MAIN: Cell<isize> = const { Cell::new(0) };
}

type PickResult = windows::core::Result<Option<GraphicsCaptureItem>>;
struct App {
    windows: Windows,
    gpu: Option<GpuDevice>,
    renderer: Option<Renderer>,
    source: Option<WgcSource>,
    options: CaptureOptions,
    picker: Option<IAsyncOperation<GraphicsCaptureItem>>,
    picked: Arc<Mutex<Option<PickResult>>>,
    alive: Arc<AtomicBool>,
    presented: FpsCounter,
    clock: Instant,
    target_name: String,
    message: String,
    size: (u32, u32),
    minimized: bool,
    diagnostics: bool,
    last_diagnostic: Duration,
}
impl App {
    fn new(options: Options) -> anyhow::Result<Self> {
        let windows = Windows::create()?;
        let size = windows.layout()?;
        let gpu = GpuDevice::new(None)?;
        let renderer = Renderer::new(gpu.clone(), windows.video, size)?;
        Ok(Self {
            windows,
            size,
            gpu: Some(gpu),
            renderer: Some(renderer),
            source: None,
            options: CaptureOptions {
                cursor: options.cursor,
                border: options.border,
                ..CaptureOptions::default()
            },
            picker: None,
            picked: Arc::new(Mutex::new(None)),
            alive: Arc::new(AtomicBool::new(true)),
            presented: FpsCounter::new(Duration::from_secs(1)),
            clock: Instant::now(),
            target_name: String::new(),
            message: "Ctrl+O para escolher".into(),
            minimized: false,
            diagnostics: options.diagnostics,
            last_diagnostic: Duration::ZERO,
        })
    }

    /// Falha do seletor nunca derruba a captura em andamento: só registra e libera o seletor.
    fn pick(&mut self) -> anyhow::Result<()> {
        if let Err(error) = self.pick_inner() {
            self.picker = None;
            tracing::warn!(
                error = format!("{error:#}"),
                "Não foi possível abrir o seletor; captura atual mantida"
            );
        }
        Ok(())
    }

    fn pick_inner(&mut self) -> anyhow::Result<()> {
        if self.picker.is_some() {
            return Ok(());
        }
        let picker = GraphicsCapturePicker::new()?;
        window_ffi::initialize_picker(&picker, self.windows.main)?;
        let operation = picker.PickSingleItemAsync()?;
        let slot = self.picked.clone();
        let hwnd = self.windows.main.0 as isize;
        let alive = self.alive.clone();
        operation.SetCompleted(&AsyncOperationCompletedHandler::new(
            move |operation, status| {
                if !alive.load(Ordering::Acquire) {
                    return Ok(());
                }
                let result = if status == AsyncStatus::Canceled {
                    Ok(None)
                } else if let Some(operation) = operation.as_ref() {
                    match operation.GetResults() {
                        Ok(item) => Ok(Some(item)),
                        // Item nulo no cancelamento: windows-rs devolve Error::empty() (HRESULT 0) ou E_POINTER.
                        Err(error)
                            if status == AsyncStatus::Completed
                                && (error.code() == E_POINTER || error.code().is_ok()) =>
                        {
                            Ok(None)
                        }
                        Err(error) => Err(error),
                    }
                } else {
                    Ok(None)
                };
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(result);
                }
                window_ffi::post(hwnd, WM_APP_PICKED);
                Ok(())
            },
        ))?;
        self.picker = Some(operation);
        Ok(())
    }

    fn accept_pick(&mut self) -> anyhow::Result<()> {
        self.picker = None;
        self.windows.focus();
        let result = self
            .picked
            .lock()
            .map_err(|_| anyhow::anyhow!("Resultado do seletor indisponível"))?
            .take();
        let Some(result) = result else {
            return Ok(());
        };
        let item = match result {
            Ok(Some(item)) => item,
            Ok(None) => {
                tracing::info!("Seleção cancelada; fonte atual mantida");
                return Ok(());
            }
            Err(error) => {
                tracing::warn!(%error, "O seletor retornou erro; fonte atual mantida");
                return Ok(());
            }
        };
        self.stop("Preparando captura");
        if self.gpu.is_none() || self.renderer.is_none() {
            let gpu = GpuDevice::new(None)?;
            self.renderer = Some(Renderer::new(gpu.clone(), self.windows.video, self.size)?);
            self.gpu = Some(gpu);
        }
        let gpu = self
            .gpu
            .as_ref()
            .context("Dispositivo D3D11 indisponível")?
            .clone();
        let hwnd = self.windows.main.0 as isize;
        let alive = self.alive.clone();
        let notify = Arc::new(move || {
            if alive.load(Ordering::Acquire) {
                window_ffi::post(hwnd, WM_APP_FRAME);
            }
        });
        let mut source = WgcSource::new(gpu, notify);
        source.start_item(item.clone(), self.options)?;
        self.target_name = item.DisplayName()?.to_string_lossy();
        self.source = Some(source);
        self.message.clear();
        tracing::info!(target = %self.target_name, "Captura WGC iniciada");
        self.status()?;
        Ok(())
    }

    fn stop(&mut self, message: &str) {
        if let Some(mut source) = self.source.take() {
            source.stop();
        }
        self.presented = FpsCounter::new(Duration::from_secs(1));
        self.target_name.clear();
        self.message = message.into();
        if let Some(renderer) = &self.renderer {
            if renderer.draw(None) {
                let _ = renderer.present();
            }
        }
        let _ = self.status();
    }

    fn render(&mut self, redraw: bool) -> anyhow::Result<()> {
        if self.minimized || self.size.0 == 0 || self.size.1 == 0 {
            return Ok(());
        }
        let Some(renderer) = &self.renderer else {
            return Ok(());
        };
        let submitted = if let Some(source) = &self.source {
            if source.is_closed() {
                self.stop("A janela/fonte foi fechada — Ctrl+O para escolher");
                tracing::info!("A fonte de captura foi fechada");
                return Ok(());
            }
            if let Some(error) = source.take_error() {
                return Err(error.into());
            }
            // O lease vive até Draw, nunca durante Present (que pode aguardar vsync).
            match source.latest_frame(redraw)? {
                Some(lease) => renderer.draw(lease.frame()),
                None => false,
            }
        } else if redraw {
            renderer.draw(None)
        } else {
            false
        };
        if submitted && renderer.present()? && self.source.is_some() {
            self.presented.record(self.clock.elapsed());
        }
        Ok(())
    }

    fn resize(&mut self) -> anyhow::Result<()> {
        self.size = self.windows.layout()?;
        if let Some(renderer) = &mut self.renderer {
            renderer.resize(self.size)?;
        }
        self.render(true)
    }

    fn status(&mut self) -> anyhow::Result<()> {
        // Rede de segurança: se a mensagem de conclusão do seletor se perder, o seletor não
        // fica "preso" (novo Ctrl+O seria ignorado enquanto `picker` existisse).
        if self
            .picker
            .as_ref()
            .is_some_and(|operation| operation.Status().is_ok_and(|s| s != AsyncStatus::Started))
        {
            self.accept_pick()?;
        }
        if self.diagnostics
            && self.clock.elapsed().saturating_sub(self.last_diagnostic) >= Duration::from_secs(2)
        {
            self.last_diagnostic = self.clock.elapsed();
            tracing::info!("{}", window_ffi::foreground_report(self.windows.main));
        }
        let title = if let Some(source) = &self.source {
            if let Some(gpu) = &self.gpu {
                gpu.check()?;
            }
            if source.is_closed() {
                self.stop("A janela/fonte foi fechada — Ctrl+O para escolher");
                return Ok(());
            }
            if let Some(error) = source.take_error() {
                return Err(error.into());
            }
            let stats = source.stats()?;
            format!(
                "madscreen-preview — {} — {}x{} | captura {:.1} FPS | apresentado {:.1} FPS | cursor {} | borda {} | intervalo mín. {}",
                self.target_name,
                stats.size.0,
                stats.size.1,
                stats.capture_fps,
                self.presented.fps(self.clock.elapsed()),
                if self.options.cursor {
                    "ligado"
                } else {
                    "desligado"
                },
                source.border_status(),
                source.min_update_interval().map_or_else(
                    || "indisponível".to_owned(),
                    |d| format!("{:.1} ms", d.as_secs_f64() * 1000.0)
                )
            )
        } else {
            format!("madscreen-preview — {}", self.message)
        };
        self.windows.title(&title)?;
        Ok(())
    }

    fn handle_error(&mut self, error: anyhow::Error) {
        let device_lost = error.chain().any(|cause| {
            cause
                .downcast_ref::<windows::core::Error>()
                .is_some_and(|error| {
                    error.code() == DXGI_ERROR_DEVICE_REMOVED
                        || error.code() == DXGI_ERROR_DEVICE_RESET
                })
        }) || self.gpu.as_ref().is_some_and(|gpu| gpu.check().is_err());
        tracing::error!(error = format!("{error:#}"), device_lost, "Falha na prévia");
        self.stop(if device_lost {
            "Dispositivo gráfico perdido — Ctrl+O para reiniciar"
        } else {
            "Falha na captura/seletor — consulte o console; Ctrl+O para escolher"
        });
        if device_lost {
            self.renderer = None;
            self.gpu = None;
        }
    }

    fn key(&mut self, key: u32) -> anyhow::Result<bool> {
        match key {
            0x4f if window_ffi::control_down() => self.pick()?,
            0x43 => {
                self.options.cursor = !self.options.cursor;
                if let Some(source) = &self.source {
                    source.set_cursor(self.options.cursor)?;
                }
                tracing::info!(enabled = self.options.cursor, "Cursor de captura alterado");
                self.status()?;
            }
            0x42 => {
                self.options.border = !self.options.border;
                if let Some(source) = &self.source {
                    source.set_border(self.options.border);
                }
                self.status()?;
            }
            key if key == u32::from(VK_ESCAPE.0) => {
                self.stop("Captura parada — Ctrl+O para escolher")
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
impl Drop for App {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        if let Some(picker) = self.picker.take() {
            let _ = picker.Cancel();
        }
        self.source = None;
        self.renderer = None;
        self.gpu = None;
    }
}

fn on_key(hwnd: HWND, key: u32, flags: isize) -> bool {
    if MAIN.with(Cell::get) != hwnd.0 as isize {
        return false;
    }
    // Evitar alternâncias repetidas com a tecla segurada.
    if flags & (1 << 30) != 0 {
        return matches!(key, 0x42 | 0x43 | 0x4f | 0x1b);
    }
    APP.with(|slot| {
        let Ok(mut app) = slot.try_borrow_mut() else {
            return false;
        };
        let Some(app) = app.as_mut() else {
            return false;
        };
        match app.key(key) {
            Ok(handled) => handled,
            Err(error) => {
                app.handle_error(error);
                true
            }
        }
    })
}

fn on_message(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if MAIN.with(Cell::get) != hwnd.0 as isize {
        return None;
    }
    if message == WM_DESTROY {
        window_ffi::quit();
        return Some(LRESULT(0));
    }
    APP.with(|slot| {
        let Ok(mut app) = slot.try_borrow_mut() else {
            // Reentrância (mensagem enviada durante outra): não perder conclusão do seletor/frames.
            if message == WM_APP_PICKED || message == WM_APP_FRAME {
                window_ffi::post(hwnd.0 as isize, message);
            }
            return None;
        };
        let app = app.as_mut()?;
        let result = match message {
            WM_COMMAND if wparam.0 & 0xffff == PICK_BUTTON => app.pick(),
            WM_APP_PICKED => app.accept_pick(),
            WM_APP_FRAME => app.render(false),
            WM_SIZE => {
                app.minimized = wparam.0 == SIZE_MINIMIZED as usize;
                app.resize()
            }
            WM_DPICHANGED => {
                window_ffi::dpi_changed(hwnd, lparam);
                app.resize()
            }
            WM_TIMER => app.status(),
            WM_CLOSE => {
                app.alive.store(false, Ordering::Release);
                app.stop("Encerrando");
                app.windows.close();
                Ok(())
            }
            _ => return None,
        };
        if let Err(error) = result {
            app.handle_error(error);
        }
        Some(LRESULT(0))
    })
}

pub fn run(options: Options) -> anyhow::Result<()> {
    window_ffi::enable_dpi_awareness()?;
    let _apartment = window_ffi::Apartment::new()?;
    let mut app = App::new(options)?;
    let hwnd = app.windows.main;
    MAIN.with(|main| main.set(hwnd.0 as isize));
    app.render(true)?;
    app.windows.show();
    tracing::info!(
        topmost = window_ffi::is_topmost(hwnd),
        "Janela principal criada (nunca topmost por padrão)"
    );
    APP.with(|slot| *slot.borrow_mut() = Some(app));
    let result = window_ffi::message_loop(hwnd);
    APP.with(|slot| {
        slot.borrow_mut().take();
    });
    MAIN.with(|main| main.set(0));
    result.context("Loop de mensagens Win32")
}
