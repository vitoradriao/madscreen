//! Fronteira Win32 da listagem de fontes: enumeração, ícones e miniaturas (GDI).
#![allow(unsafe_code)]
use anyhow::Result;
use madscreen_ipc::source_identity::{application_id, window_instance};
use std::ffi::c_void;
use windows::{
    Win32::{
        Foundation::{CloseHandle, FILETIME, HWND, LPARAM, RECT, TRUE, WPARAM},
        Graphics::{
            Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
            Gdi::*,
        },
        Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            Shell::ExtractIconExW,
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, PCWSTR, PWSTR},
};

const THUMB_MAX_W: i32 = 320;
const THUMB_MAX_H: i32 = 180;
const ICON_SIZE: i32 = 32;
const PW_RENDERFULLCONTENT: u32 = 2;

pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct WindowInfo {
    pub app_id: Option<String>,
    pub instance_id: Option<String>,
    pub hwnd: i64,
    pub title: String,
    pub process: String,
    pub width: u32,
    pub height: u32,
    pub icon: Option<Bitmap>,
    pub thumbnail: Option<Bitmap>,
}

pub struct MonitorInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    pub thumbnail: Option<Bitmap>,
}

/// Libera o DC da tela ao sair do escopo.
struct ScreenDc(HDC);
impl ScreenDc {
    fn new() -> Option<Self> {
        // SAFETY: DC da tela inteira; liberado em Drop.
        let dc = unsafe { GetDC(None) };
        (!dc.is_invalid()).then_some(Self(dc))
    }
}
impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: DC obtido por GetDC(None) nesta thread.
        unsafe {
            ReleaseDC(None, self.0);
        }
    }
}

/// DC de memória com bitmap selecionado; restaura e libera tudo em Drop.
struct MemDc {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}
impl MemDc {
    fn new(screen: &ScreenDc, width: i32, height: i32) -> Option<Self> {
        // SAFETY: handles criados aqui e liberados em Drop; falhas desfazem o que foi criado.
        unsafe {
            let dc = CreateCompatibleDC(Some(screen.0));
            if dc.is_invalid() {
                return None;
            }
            let bitmap = CreateCompatibleBitmap(screen.0, width, height);
            if bitmap.is_invalid() {
                let _ = DeleteDC(dc);
                return None;
            }
            let previous = SelectObject(dc, bitmap.into());
            Some(Self {
                dc,
                bitmap,
                previous,
            })
        }
    }
}
impl Drop for MemDc {
    fn drop(&mut self) {
        // SAFETY: restaura o objeto original antes de apagar o bitmap e o DC criados em `new`.
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

fn bitmap_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negativo: linhas de cima para baixo.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// BGRA (com padding de alpha indefinido) para RGBA opaco.
fn bgra_to_rgba_opaque(mut data: Vec<u8>) -> Vec<u8> {
    for px in data.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    data
}

fn fit(width: i32, height: i32) -> (i32, i32) {
    let scale = (THUMB_MAX_W as f64 / width as f64).min(THUMB_MAX_H as f64 / height as f64);
    let scale = scale.min(1.0);
    (
        ((width as f64 * scale).round() as i32).max(1),
        ((height as f64 * scale).round() as i32).max(1),
    )
}

/// Copia `src` (retângulo `sx,sy,sw,sh`) reduzido para uma miniatura RGBA.
fn scale_down(screen: &ScreenDc, src: HDC, sx: i32, sy: i32, sw: i32, sh: i32) -> Option<Bitmap> {
    if sw <= 0 || sh <= 0 {
        return None;
    }
    let (tw, th) = fit(sw, sh);
    let target = MemDc::new(screen, tw, th)?;
    let mut info = bitmap_info(tw, th);
    let mut data = vec![0u8; tw as usize * th as usize * 4];
    // SAFETY: DCs e bitmap válidos; `data` tem exatamente tw*th*4 bytes para GetDIBits.
    let lines = unsafe {
        SetStretchBltMode(target.dc, HALFTONE);
        let _ = SetBrushOrgEx(target.dc, 0, 0, None);
        if !StretchBlt(target.dc, 0, 0, tw, th, Some(src), sx, sy, sw, sh, SRCCOPY).as_bool() {
            return None;
        }
        GetDIBits(
            target.dc,
            target.bitmap,
            0,
            th as u32,
            Some(data.as_mut_ptr().cast::<c_void>()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    if lines == 0 {
        return None;
    }
    // Janela que o DWM não entrega (jogos em tela cheia exclusiva etc.) vem toda preta.
    if data.chunks_exact(4).all(|px| px[0] | px[1] | px[2] == 0) {
        return None;
    }
    Some(Bitmap {
        width: tw as u32,
        height: th as u32,
        rgba: bgra_to_rgba_opaque(data),
    })
}

fn window_thumbnail(screen: &ScreenDc, hwnd: HWND, width: i32, height: i32) -> Option<Bitmap> {
    let full = MemDc::new(screen, width, height)?;
    // SAFETY: HWND recém-enumerado e DC de memória válido; falha de janela fechada é tratada.
    let printed = unsafe { PrintWindow(hwnd, full.dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)) };
    if !printed.as_bool() {
        return None;
    }
    scale_down(screen, full.dc, 0, 0, width, height)
}

fn icon_bitmap(icon: HICON) -> Option<Bitmap> {
    let info = bitmap_info(ICON_SIZE, ICON_SIZE);
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY: DIB section própria de 32x32x4 bytes; `bits` aponta para ela enquanto o bitmap vive.
    unsafe {
        let screen = ScreenDc::new()?;
        let dc = CreateCompatibleDC(Some(screen.0));
        if dc.is_invalid() {
            return None;
        }
        let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(dc);
            return None;
        };
        let previous = SelectObject(dc, bitmap.into());
        let len = (ICON_SIZE * ICON_SIZE * 4) as usize;
        let mut data = None;
        if !bits.is_null() {
            std::ptr::write_bytes(bits.cast::<u8>(), 0, len);
            if DrawIconEx(dc, 0, 0, icon, ICON_SIZE, ICON_SIZE, 0, None, DI_NORMAL).is_ok() {
                data = Some(std::slice::from_raw_parts(bits.cast::<u8>(), len).to_vec());
            }
        }
        SelectObject(dc, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        let mut data = data?;
        // Ícones sem canal alpha saem com alpha 0 do GDI: torna opacos os pixels desenhados.
        let has_alpha = data.chunks_exact(4).any(|px| px[3] != 0);
        let mut any = false;
        for px in data.chunks_exact_mut(4) {
            px.swap(0, 2);
            if !has_alpha {
                px[3] = if px[0] | px[1] | px[2] != 0 { 255 } else { 0 };
            }
            any |= px[3] != 0;
        }
        any.then_some(Bitmap {
            width: ICON_SIZE as u32,
            height: ICON_SIZE as u32,
            rgba: data,
        })
    }
}

fn window_icon(hwnd: HWND, exe: Option<&[u16]>) -> Option<Bitmap> {
    let mut handle = 0usize;
    // SAFETY: mensagens com timeout curto: janela travada não bloqueia a listagem.
    unsafe {
        for kind in [ICON_BIG, ICON_SMALL2] {
            let mut result = 0usize;
            SendMessageTimeoutW(
                hwnd,
                WM_GETICON,
                WPARAM(kind as usize),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                50,
                Some(&mut result),
            );
            if result != 0 {
                handle = result;
                break;
            }
        }
        if handle == 0 {
            handle = GetClassLongPtrW(hwnd, GCLP_HICON);
        }
    }
    if handle != 0 {
        // Ícone de propriedade da janela/classe: não destruir.
        if let Some(bitmap) = icon_bitmap(HICON(handle as *mut c_void)) {
            return Some(bitmap);
        }
    }
    let exe = exe?;
    let mut large = HICON::default();
    // SAFETY: caminho terminado em NUL; o ícone extraído é nosso e destruído abaixo.
    unsafe {
        if ExtractIconExW(PCWSTR(exe.as_ptr()), 0, Some(&mut large), None, 1) == 0
            || large.is_invalid()
        {
            return None;
        }
        let bitmap = icon_bitmap(large);
        let _ = DestroyIcon(large);
        bitmap
    }
}

fn process_path(pid: u32) -> Option<Vec<u16>> {
    // SAFETY: handle fechado antes de retornar; buffer local com tamanho informado.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = vec![0u16; 1024];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        buffer.truncate(size as usize);
        buffer.push(0);
        Some(buffer)
    }
}

fn file_name(path: &[u16]) -> String {
    let text = String::from_utf16_lossy(&path[..path.len().saturating_sub(1)]);
    text.rsplit(['\\', '/']).next().unwrap_or(&text).to_string()
}

struct Candidate {
    hwnd: HWND,
    title: String,
    pid: u32,
    width: i32,
    height: i32,
}

fn window_text(hwnd: HWND) -> String {
    // SAFETY: buffer local com tamanho informado ao Win32.
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut title = vec![0u16; len as usize + 1];
        let count = GetWindowTextW(hwnd, &mut title);
        String::from_utf16_lossy(&title[..count.max(0) as usize])
    }
}

pub fn window_exists(hwnd: isize) -> bool {
    // SAFETY: IsWindow so consulta a tabela de handles do sistema e aceita qualquer valor.
    unsafe { IsWindow(Some(HWND(hwnd as *mut c_void))).as_bool() }
}

pub fn window_title(hwnd: i64) -> String {
    window_text(HWND(hwnd as isize as *mut c_void))
}

pub fn window_identity(hwnd: i64) -> Option<(String, String)> {
    // SAFETY: consultas somente leitura; handle do processo é fechado antes do retorno.
    unsafe {
        let window = HWND(hwnd as isize as *mut c_void);
        if !IsWindow(Some(window)).as_bool() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        let path = process_path(pid)?;
        let text = String::from_utf16_lossy(&path[..path.len().saturating_sub(1)]).to_lowercase();
        let app = application_id(&text);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut created = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user);
        let _ = CloseHandle(process);
        result.ok()?;
        let instance = window_instance(
            &app,
            pid,
            (created.dwHighDateTime, created.dwLowDateTime),
            hwnd,
            &class_name(window),
            &window_text(window),
        );
        Some((app, instance))
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    // SAFETY: buffer local com tamanho informado ao Win32.
    let count = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..count.max(0) as usize])
}

fn candidate(hwnd: HWND, exclude_pid: Option<u32>) -> Option<Candidate> {
    // SAFETY: HWND vindo de EnumWindows; todas as consultas são somente leitura.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast::<c_void>(),
            std::mem::size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
        {
            return None;
        }
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let app_window = ex_style & WS_EX_APPWINDOW.0 != 0;
        if ex_style & WS_EX_TOOLWINDOW.0 != 0 && !app_window {
            return None;
        }
        if !app_window && GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
            return None;
        }
        let title = window_text(hwnd);
        if title.trim().is_empty() {
            return None;
        }
        if matches!(
            class_name(hwnd).as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if exclude_pid == Some(pid) || pid == std::process::id() {
            return None;
        }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).ok()?;
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        if width < 32 || height < 32 {
            return None;
        }
        Some(Candidate {
            hwnd,
            title,
            pid,
            width,
            height,
        })
    }
}

unsafe extern "system" fn window_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam aponta para o par do chamador, vivo durante a enumeração síncrona.
    let (list, exclude) = unsafe { &mut *(lparam.0 as *mut (Vec<Candidate>, Option<u32>)) };
    if let Some(found) = candidate(hwnd, *exclude) {
        list.push(found);
    }
    TRUE
}

/// Janelas na ordem Z (a mais à frente primeiro), como no Alt+Tab.
pub fn windows(exclude_pid: Option<u32>) -> Vec<WindowInfo> {
    let mut state = (Vec::<Candidate>::new(), exclude_pid);
    // SAFETY: estado vive durante enumeração síncrona.
    let enumerated =
        unsafe { EnumWindows(Some(window_callback), LPARAM(&mut state as *mut _ as isize)) };
    if enumerated.is_err() {
        return Vec::new();
    }
    let screen = ScreenDc::new();
    state
        .0
        .into_iter()
        .map(|c| {
            let exe = process_path(c.pid);
            let identity = window_identity(c.hwnd.0 as i64);
            WindowInfo {
                app_id: identity.as_ref().map(|v| v.0.clone()),
                instance_id: identity.map(|v| v.1),
                hwnd: c.hwnd.0 as i64,
                title: c.title,
                process: exe.as_deref().map(file_name).unwrap_or_default(),
                width: c.width as u32,
                height: c.height as u32,
                icon: window_icon(c.hwnd, exe.as_deref()),
                thumbnail: screen
                    .as_ref()
                    .and_then(|s| window_thumbnail(s, c.hwnd, c.width, c.height)),
            }
        })
        .collect()
}

struct RawMonitor {
    rect: RECT,
    name: String,
    primary: bool,
}

unsafe extern "system" fn monitor_callback(
    monitor: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    // SAFETY: lparam aponta para o Vec do chamador, vivo durante a enumeração síncrona.
    let list = unsafe { &mut *(lparam.0 as *mut Vec<RawMonitor>) };
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `info` com cbSize de MONITORINFOEXW; ponteiro compatível com MONITORINFO.
    let ok = unsafe { GetMonitorInfoW(monitor, (&mut info as *mut MONITORINFOEXW).cast()) };
    if ok.as_bool() {
        let len = info
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.szDevice.len());
        list.push(RawMonitor {
            rect: info.monitorInfo.rcMonitor,
            name: String::from_utf16_lossy(&info.szDevice[..len]),
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    TRUE
}

/// Mesma ordem de `EnumDisplayMonitors` usada por `SourceSpec::Monitor`.
pub fn monitors(thumbnails: bool) -> Result<Vec<MonitorInfo>> {
    let mut list = Vec::<RawMonitor>::new();
    // SAFETY: estado vive durante enumeração síncrona.
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(monitor_callback),
            LPARAM(&mut list as *mut _ as isize),
        )
        .ok()?;
    }
    let screen = if thumbnails { ScreenDc::new() } else { None };
    Ok(list
        .into_iter()
        .enumerate()
        .map(|(index, m)| {
            let (w, h) = (m.rect.right - m.rect.left, m.rect.bottom - m.rect.top);
            MonitorInfo {
                index,
                name: m.name,
                width: w.max(0) as u32,
                height: h.max(0) as u32,
                primary: m.primary,
                thumbnail: screen
                    .as_ref()
                    .and_then(|s| scale_down(s, s.0, m.rect.left, m.rect.top, w, h)),
            }
        })
        .collect())
}

/// Sem isso o Windows entrega tamanhos virtualizados em telas com escala acima de 100%.
pub fn enable_dpi_awareness() {
    // SAFETY: chamada única no início do processo de listagem; falha (já definido) é ignorada.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}
