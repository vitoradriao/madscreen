//! Handles Win32, seleção de fonte, relógio e timer; nenhum acesso a pixels.
#![allow(unsafe_code)]
use madscreen_capture::CaptureTarget;
use windows::{
    Win32::{
        Foundation::{CloseHandle, E_INVALIDARG, HANDLE, HWND, LPARAM, RECT},
        Graphics::{
            Dxgi::{CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1},
            Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO},
        },
        System::{
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            Threading::*,
            WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
        },
    },
    core::{BOOL, Interface, Result},
};

pub struct Runtime(std::marker::PhantomData<std::rc::Rc<()>>);
impl Runtime {
    pub fn new() -> Result<Self> {
        // SAFETY: chamada balanceada por Drop na mesma thread.
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
        }
        Ok(Self(std::marker::PhantomData))
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        // SAFETY: balanceia RoInitialize bem-sucedido na thread criadora.
        unsafe {
            RoUninitialize();
        }
    }
}
pub fn qpc() -> Result<i64> {
    let mut value = 0;
    // SAFETY: saída local válida.
    unsafe {
        QueryPerformanceCounter(&mut value)?;
    }
    Ok(value)
}
pub fn qpc_frequency() -> Result<i64> {
    let mut value = 0;
    // SAFETY: saída local válida.
    unsafe {
        QueryPerformanceFrequency(&mut value)?;
    }
    Ok(value)
}
/// `GetLastError` pode estar zerado (ex.: `WAIT_ABANDONED`); nesse caso `Error::from_thread()` diria
/// "operação concluída com êxito". Devolve um erro com texto útil.
fn last_error() -> windows::core::Error {
    let error = windows::core::Error::from_thread();
    if error.code().is_ok() {
        windows::core::Error::new(
            windows::Win32::Foundation::E_FAIL,
            "espera do Windows terminou de forma inesperada (sem código de erro)",
        )
    } else {
        error
    }
}
pub fn set_encoder_priority() -> Result<()> {
    // SAFETY: pseudo-handle de thread atual, prioridade ABOVE_NORMAL, nunca REALTIME.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL) }
}
pub struct Timer(HANDLE);
impl Timer {
    pub fn new() -> Result<Self> {
        // SAFETY: timer privado, sem callbacks; direitos mínimos para armar/esperar.
        unsafe {
            CreateWaitableTimerExW(
                None,
                None,
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_MODIFY_STATE.0 | SYNCHRONIZATION_SYNCHRONIZE.0,
            )
        }
        .map(Self)
    }
    /// Aguarda chegada WGC ou deadline de polling com o mesmo timer de alta resolução.
    pub fn wait_for_frame(
        &self,
        event: &FrameEvent,
        deadline: i64,
        frequency: i64,
    ) -> Result<bool> {
        if frequency <= 0 {
            return Err(windows::core::Error::from_hresult(E_INVALIDARG));
        }
        let left = deadline.saturating_sub(qpc()?);
        if left <= 0 {
            return event.wait(0);
        }
        let due =
            -(i128::from(left) * 10_000_000 / i128::from(frequency)).clamp(1, 1_000_000) as i64;
        let handles = [event.0, self.0];
        // SAFETY: handles vivos e privados, timer one-shot; acorda por frame ou cadência, nunca por espera da GPU.
        let status = unsafe {
            SetWaitableTimer(self.0, &due, 0, None, None, false)?;
            WaitForMultipleObjects(&handles, false, 1000)
        };
        match status.0 {
            0 => Ok(true),
            // Timer venceu (1) ou o timeout de segurança passou: em ambos o chamador reavalia.
            1 | 0x102 => Ok(false),
            _ => Err(last_error()),
        }
    }
    pub fn wait_until(
        &self,
        deadline: i64,
        frequency: i64,
        running: &std::sync::atomic::AtomicBool,
    ) -> Result<()> {
        if frequency <= 0 {
            return Err(windows::core::Error::from_hresult(E_INVALIDARG));
        }
        while running.load(std::sync::atomic::Ordering::Acquire) {
            let left = deadline.saturating_sub(qpc()?);
            if left <= 0 {
                break;
            }
            // Esperas de no máximo 100 ms permitem Ctrl+C inclusive com FPS fracionário baixo.
            let due =
                -(i128::from(left) * 10_000_000 / i128::from(frequency)).clamp(1, 1_000_000) as i64;
            // SAFETY: timer exclusivo, one-shot relativo negativo, sem callback.
            unsafe {
                SetWaitableTimer(self.0, &due, 0, None, None, false)?;
                match WaitForSingleObject(self.0, 1000) {
                    windows::Win32::Foundation::WAIT_OBJECT_0 => {}
                    // Timeout de segurança de 1 s: não é erro, o laço rearma o timer.
                    windows::Win32::Foundation::WAIT_TIMEOUT => {}
                    _ => return Err(last_error()),
                }
            }
        }
        Ok(())
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        // SAFETY: único proprietário do handle.
        let _ = unsafe { CloseHandle(self.0) };
    }
}
struct Monitors {
    handles: Vec<isize>,
}
unsafe extern "system" fn monitor_callback(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: EnumDisplayMonitors chama sincronamente com ponteiro de Monitors vivo.
    let state = unsafe { &mut *(data.0 as *mut Monitors) };
    state.handles.push(monitor.0 as isize);
    true.into()
}
struct Windows {
    title: String,
    found: Vec<isize>,
}
unsafe extern "system" fn window_callback(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: EnumWindows chama sincronamente com ponteiro de Windows vivo.
    let state = unsafe { &mut *(data.0 as *mut Windows) };
    // SAFETY: HWND fornecido pelo enumerador; buffer de título dimensionado pela API.
    unsafe {
        if IsWindowVisible(hwnd).as_bool() {
            let len = GetWindowTextLengthW(hwnd);
            if len > 0 {
                let mut title = vec![0u16; len as usize + 1];
                let count = GetWindowTextW(hwnd, &mut title);
                if String::from_utf16_lossy(&title[..count as usize])
                    .to_lowercase()
                    .contains(&state.title)
                {
                    state.found.push(hwnd.0 as isize);
                }
            }
        }
    }
    true.into()
}
pub fn resolve_monitor(index: usize) -> crate::Result<CaptureTarget> {
    let mut state = Monitors {
        handles: Vec::new(),
    };
    // SAFETY: estado vive durante enumeração síncrona.
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(monitor_callback),
            LPARAM(&mut state as *mut _ as isize),
        )
        .ok()?;
    }
    state
        .handles
        .get(index)
        .copied()
        .map(CaptureTarget::Monitor)
        .ok_or(crate::EncodeError::InvalidConfig(
            "índice de monitor inexistente (base zero)",
        ))
}
pub fn monitor_size(target: CaptureTarget) -> Result<(u32, u32)> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let CaptureTarget::Monitor(id) = target else {
        return Err(windows::core::Error::from_hresult(E_INVALIDARG));
    };
    // SAFETY: handle validado pela API; saída de tamanho correto.
    unsafe {
        GetMonitorInfoW(HMONITOR(id as *mut _), &mut info).ok()?;
    }
    Ok((
        (info.rcMonitor.right - info.rcMonitor.left) as u32,
        (info.rcMonitor.bottom - info.rcMonitor.top) as u32,
    ))
}
pub fn resolve_window(title: &str) -> crate::Result<CaptureTarget> {
    let mut state = Windows {
        title: title.to_lowercase(),
        found: Vec::new(),
    };
    // SAFETY: estado vive durante enumeração síncrona.
    unsafe {
        EnumWindows(Some(window_callback), LPARAM(&mut state as *mut _ as isize))?;
    }
    match state.found.as_slice() {
        [hwnd] => Ok(CaptureTarget::Window(*hwnd)),
        [] => Err(crate::EncodeError::InvalidConfig("janela não encontrada")),
        _ => Err(crate::EncodeError::InvalidConfig(
            "título ambíguo: use trecho mais específico",
        )),
    }
}
pub fn window_size(target: CaptureTarget) -> Result<(u32, u32)> {
    let CaptureTarget::Window(id) = target else {
        return Err(windows::core::Error::from_hresult(E_INVALIDARG));
    };
    let mut rect = RECT::default();
    // SAFETY: HWND validado pela API; saída local válida.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowRect(HWND(id as *mut _), &mut rect)?;
    }
    Ok((
        (rect.right - rect.left).max(2) as u32,
        (rect.bottom - rect.top).max(2) as u32,
    ))
}
#[derive(Debug, serde::Serialize)]
pub struct AdapterInfo {
    pub name: String,
    pub vendor_id: u32,
    pub luid_high: i32,
    pub luid_low: u32,
}
pub fn adapters() -> Result<Vec<AdapterInfo>> {
    let mut result = Vec::new();
    // SAFETY: enumeração de interfaces COM com índices verificados pela API.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            let desc = adapter.GetDesc1()?;
            let len = desc
                .Description
                .iter()
                .position(|&v| v == 0)
                .unwrap_or(desc.Description.len());
            result.push(AdapterInfo {
                name: String::from_utf16_lossy(&desc.Description[..len]),
                vendor_id: desc.VendorId,
                luid_high: desc.AdapterLuid.HighPart,
                luid_low: desc.AdapterLuid.LowPart,
            });
            index += 1;
        }
    }
    Ok(result)
}
pub fn device_adapter(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
) -> Result<AdapterInfo> {
    // SAFETY: interface DXGI obtida do device vivo.
    let desc = unsafe { device.cast::<IDXGIDevice>()?.GetAdapter()?.GetDesc()? };
    let len = desc
        .Description
        .iter()
        .position(|&v| v == 0)
        .unwrap_or(desc.Description.len());
    Ok(AdapterInfo {
        name: String::from_utf16_lossy(&desc.Description[..len]),
        vendor_id: desc.VendorId,
        luid_high: desc.AdapterLuid.HighPart,
        luid_low: desc.AdapterLuid.LowPart,
    })
}

/// Evento auto-reset compartilhado com o callback WGC, sem transportar leases.
pub struct FrameEvent(HANDLE);
// SAFETY: handle de evento Win32 pode ser sinalizado/esperado em threads distintas; Drop após último Arc.
unsafe impl Send for FrameEvent {}
// SAFETY: operações Win32 do evento são sincronizadas pelo kernel.
unsafe impl Sync for FrameEvent {}
impl FrameEvent {
    pub fn new() -> Result<Self> {
        Self::create(false)
    }
    pub(crate) fn manual() -> Result<Self> {
        Self::create(true)
    }
    fn create(manual: bool) -> Result<Self> {
        // SAFETY: evento privado, inicialmente não sinalizado.
        unsafe { CreateEventW(None, manual, false, None) }.map(Self)
    }
    pub(crate) fn reset(&self) -> Result<()> {
        // SAFETY: evento vivo, reset antes de submeter novo frame do slot.
        unsafe { ResetEvent(self.0) }
    }
    pub(crate) fn raw(&self) -> *mut core::ffi::c_void {
        self.0.0
    }
    pub fn signal(&self) {
        // SAFETY: handle mantido vivo por Arc no callback WGC.
        let _ = unsafe { SetEvent(self.0) };
    }
    pub fn wait(&self, timeout_ms: u32) -> Result<bool> {
        // SAFETY: evento vivo; espera limitada, sem callback ou lease retido.
        let status = unsafe { WaitForSingleObject(self.0, timeout_ms) };
        match status {
            windows::Win32::Foundation::WAIT_OBJECT_0 => Ok(true),
            windows::Win32::Foundation::WAIT_TIMEOUT => Ok(false),
            _ => Err(last_error()),
        }
    }
}
impl Drop for FrameEvent {
    fn drop(&mut self) {
        // SAFETY: último dono do handle privado.
        let _ = unsafe { CloseHandle(self.0) };
    }
}
#[derive(Debug, serde::Serialize)]
pub struct PriorityResult {
    pub requested: crate::pipeline::GpuPriority,
    pub dxgi: String,
    pub process: String,
}
pub fn set_gpu_priority(
    device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    priority: crate::pipeline::GpuPriority,
) -> PriorityResult {
    let dxgi = (|| -> Result<()> {
        // SAFETY: device vivo; valores limitados a 0,+3,+7, nunca REALTIME.
        unsafe {
            device
                .cast::<IDXGIDevice>()?
                .SetGPUThreadPriority(priority.thread_priority())
        }
    })();
    let process = (|| -> crate::Result<()> {
        let library = super::dll_ffi::Library::load(windows::core::w!("gdi32.dll"))?;
        type SetPriority = unsafe extern "system" fn(HANDLE, u32) -> i32;
        // SAFETY: assinatura de D3DKMTSetProcessSchedulingPriorityClass, DLL retida na chamada.
        let set: SetPriority = unsafe {
            library.symbol(windows::core::s!("D3DKMTSetProcessSchedulingPriorityClass"))?
        };
        // SAFETY: pseudo-handle do processo atual, enum limitada a NORMAL/ABOVE_NORMAL/HIGH.
        let status = unsafe { set(GetCurrentProcess(), priority.process_class()) };
        if status < 0 {
            return Err(crate::EncodeError::EncoderUnavailable(format!(
                "NTSTATUS {status:#x}"
            )));
        }
        Ok(())
    })();
    let describe = |result: std::result::Result<(), String>| match result {
        Ok(()) => "aplicado".into(),
        Err(error)
            if error.contains("80070005")
                || error.contains("c0000022")
                || error.contains("c0000061") =>
        {
            format!("negado: {error}")
        }
        Err(error) => format!("erro: {error}"),
    };
    PriorityResult {
        requested: priority,
        dxgi: describe(dxgi.map_err(|e| format!("{:#x}: {e}", e.code().0))),
        process: describe(process.map_err(|e| e.to_string())),
    }
}
