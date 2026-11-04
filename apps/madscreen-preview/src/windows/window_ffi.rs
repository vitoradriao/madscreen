//! Fronteira Win32: HWNDs e ponteiros permanecem neste módulo.
#![allow(unsafe_code)]
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::{
            BLACK_BRUSH, BeginPaint, EndPaint, GetMonitorInfoW, GetStockObject, HBRUSH,
            MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, PAINTSTRUCT,
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            WinRT::{RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize},
        },
        UI::{
            HiDpi::*, Input::KeyboardAndMouse::*, Shell::SHQueryUserNotificationState,
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, Result, w},
};

pub fn enable_dpi_awareness() -> Result<()> {
    // SAFETY: chamado antes de WinRT/COM, que pode criar janelas internas no STA.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
}

pub struct Apartment;
impl Apartment {
    pub fn new() -> Result<Self> {
        // SAFETY: primeira inicialização COM desta thread de UI, balanceada em Drop.
        unsafe {
            RoInitialize(RO_INIT_SINGLETHREADED)?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: executado na mesma thread que inicializou COM, após soltar objetos WinRT.
        unsafe {
            RoUninitialize();
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_PAINT {
        let mut paint = PAINTSTRUCT::default();
        // SAFETY: PAINTSTRUCT local válida; Begin/EndPaint balanceados para a janela recebida.
        unsafe {
            BeginPaint(hwnd, &mut paint);
            let _ = EndPaint(hwnd, &paint);
        }
        return LRESULT(0);
    }
    if let Some(result) = super::on_message(hwnd, message, wparam, lparam) {
        return result;
    }
    // SAFETY: parâmetros encaminhados sem modificação da callback Win32.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub struct Windows {
    pub main: HWND,
    pub video: HWND,
    pub button: HWND,
}
impl Windows {
    pub fn create() -> Result<Self> {
        // SAFETY: classe e strings estáticas válidas; callback vive por todo o processo.
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                lpszClassName: w!("MadscreenPreviewWindow"),
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                return Err(windows::core::Error::from_thread());
            }
            let main = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class.lpszClassName,
                w!("madscreen-preview — Ctrl+O para escolher"),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1100,
                760,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let button = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BUTTON"),
                w!("Escolher janela ou tela..."),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                8,
                6,
                250,
                30,
                Some(main),
                Some(HMENU(super::PICK_BUTTON as *mut _)),
                Some(instance.into()),
                None,
            )?;
            let video = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class.lpszClassName,
                w!(""),
                WS_CHILD | WS_VISIBLE,
                0,
                44,
                1100,
                716,
                Some(main),
                None,
                Some(instance.into()),
                None,
            )?;
            if SetTimer(Some(main), 1, 500, None) == 0 {
                let error = windows::core::Error::from_thread();
                let _ = DestroyWindow(main);
                return Err(error);
            }
            Ok(Self {
                main,
                video,
                button,
            })
        }
    }
    pub fn show(&self) {
        // SAFETY: HWND principal vivo.
        unsafe {
            let _ = ShowWindow(self.main, SW_SHOW);
        }
    }
    pub fn layout(&self) -> Result<(u32, u32)> {
        let mut rect = RECT::default();
        // SAFETY: saídas locais válidas e janelas filhas vivas; dimensões não negativas.
        unsafe {
            GetClientRect(self.main, &mut rect)?;
            let dpi = GetDpiForWindow(self.main).max(96);
            let bar = (44 * dpi / 96) as i32;
            let width = (rect.right - rect.left).max(0);
            let height = (rect.bottom - rect.top - bar).max(0);
            MoveWindow(
                self.button,
                (8 * dpi / 96) as i32,
                (6 * dpi / 96) as i32,
                (250 * dpi / 96) as i32,
                (30 * dpi / 96) as i32,
                true,
            )?;
            MoveWindow(self.video, 0, bar, width, height, true)?;
            Ok((width as u32, height as u32))
        }
    }
    pub fn title(&self, title: &str) -> Result<()> {
        let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        // SAFETY: string UTF-16 terminada em NUL, viva até o retorno da API.
        unsafe { SetWindowTextW(self.main, PCWSTR(title.as_ptr())) }
    }
    pub fn focus(&self) {
        // SAFETY: HWND principal vivo; falha de foco não é fatal.
        unsafe {
            let _ = SetFocus(Some(self.main));
        }
    }
    pub fn close(&self) {
        // SAFETY: usado apenas na thread dona da janela; WM_DESTROY encerra o loop.
        unsafe {
            let _ = DestroyWindow(self.main);
        }
    }
}

pub fn post(hwnd: isize, message: u32) {
    // SAFETY: mensagem sem ponteiros; resultado fica em slot sincronizado, nunca em LPARAM.
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), message, WPARAM(0), LPARAM(0));
    }
}

pub fn control_down() -> bool {
    // SAFETY: consulta de teclado sem ponteiros.
    unsafe { GetKeyState(i32::from(VK_CONTROL.0)) < 0 }
}

pub fn dpi_changed(hwnd: HWND, lparam: LPARAM) {
    // SAFETY: chamado exclusivamente para WM_DPICHANGED; LPARAM aponta para RECT válido durante a callback.
    unsafe {
        let rect = &*(lparam.0 as *const RECT);
        let _ = SetWindowPos(
            hwnd,
            None,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

pub fn initialize_picker(
    picker: &windows::Graphics::Capture::GraphicsCapturePicker,
    hwnd: HWND,
) -> Result<()> {
    use windows::{Win32::UI::Shell::IInitializeWithWindow, core::Interface};
    let initialize: IInitializeWithWindow = picker.cast()?;
    // SAFETY: HWND da janela principal vivo no STA; inicializa a UI nativa antes de abrir o seletor.
    unsafe { initialize.Initialize(hwnd) }
}

pub fn quit() {
    // SAFETY: chamado em WM_DESTROY na thread com o loop de mensagens.
    unsafe {
        PostQuitMessage(0);
    }
}

pub fn message_loop(main: HWND) -> Result<()> {
    let mut message = MSG::default();
    loop {
        // SAFETY: MSG local válida; GetMessage bloqueia sem busy-wait.
        let status = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if status == -1 {
            return Err(windows::core::Error::from_thread());
        }
        if status == 0 {
            return Ok(());
        }
        // Redireciona atalhos mesmo quando o botão filho está com foco.
        if message.message == WM_KEYDOWN
            && super::on_key(main, message.wParam.0 as u32, message.lParam.0)
        {
            continue;
        }
        // SAFETY: mensagem inicializada por GetMessage e despachada na thread proprietária.
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// A própria janela principal está marcada como topmost?
pub fn is_topmost(hwnd: HWND) -> bool {
    // SAFETY: consulta de estilo em HWND vivo, sem ponteiros.
    let style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    style & WS_EX_TOPMOST.0 != 0
}

/// Resumo da janela em primeiro plano para diagnosticar a barra de tarefas visível sobre o jogo.
pub fn foreground_report(own: HWND) -> String {
    // SAFETY: todas as saídas são buffers/structs locais válidos durante as chamadas; HWNDs
    // obtidos do sistema podem morrer entre chamadas, o que apenas faz as consultas falharem.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return "sem janela em primeiro plano".into();
        }
        let mut title = [0u16; 128];
        let title_len = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        let mut class = [0u16; 128];
        let class_len = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut monitor = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..MONITORINFO::default()
        };
        let monitor_rect = if GetMonitorInfoW(
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
            &mut monitor,
        )
        .as_bool()
        {
            monitor.rcMonitor
        } else {
            RECT::default()
        };
        let covers = rect.left <= monitor_rect.left
            && rect.top <= monitor_rect.top
            && rect.right >= monitor_rect.right
            && rect.bottom >= monitor_rect.bottom;
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let state = SHQueryUserNotificationState()
            .map(|state| format!("{}", state.0))
            .unwrap_or_else(|_| "?".into());
        format!(
            "primeiro plano: \"{}\" classe={} pid={} propria={} rect=({},{})-({},{}) monitor=({},{})-({},{}) cobre_monitor={} topmost={} QUNS={} (2=tela cheia ocupada, 3=D3D fullscreen, 4=apresentacao, 5=ok)",
            String::from_utf16_lossy(&title[..title_len.min(title.len())]),
            String::from_utf16_lossy(&class[..class_len.min(class.len())]),
            pid,
            hwnd == own,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            monitor_rect.left,
            monitor_rect.top,
            monitor_rect.right,
            monitor_rect.bottom,
            covers,
            ex_style & WS_EX_TOPMOST.0 != 0,
            state
        )
    }
}
