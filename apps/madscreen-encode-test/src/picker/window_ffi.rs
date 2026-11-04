//! Fronteira Win32 do seletor: HWND e inicialização COM ficam neste módulo.
#![allow(unsafe_code)]
use std::{marker::PhantomData, rc::Rc};
use windows::{
    Graphics::Capture::GraphicsCapturePicker,
    Win32::{
        Foundation::{ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            LibraryLoader::GetModuleHandleW,
            WinRT::{RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize},
        },
        UI::{Shell::IInitializeWithWindow, WindowsAndMessaging::*},
    },
    core::{Interface, Result, w},
};

pub struct Apartment(PhantomData<Rc<()>>);
impl Apartment {
    pub fn new() -> Result<Self> {
        // SAFETY: primeira inicialização COM desta thread; balanceada em Drop no mesmo STA.
        unsafe {
            RoInitialize(RO_INIT_SINGLETHREADED)?;
        }
        Ok(Self(PhantomData))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: o guard não é Send e os objetos WinRT locais já foram liberados.
        unsafe {
            RoUninitialize();
        }
    }
}

pub struct Owner {
    hwnd: HWND,
    _thread: PhantomData<Rc<()>>,
}
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: parâmetros recebidos diretamente do Win32 e encaminhados sem alteração.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
impl Owner {
    pub fn new() -> Result<Self> {
        // SAFETY: classe/strings estáticas, callback do sistema; janela sem WS_VISIBLE,
        // criada e destruída no mesmo STA. O timer acorda o loop para consultar Stop/EOF.
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: w!("MadscreenEncodePickerOwner"),
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                let error = windows::core::Error::from_thread();
                if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                    return Err(error);
                }
            }
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.lpszClassName,
                w!("madscreen — Escolher janela ou tela"),
                WS_OVERLAPPED,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let owner = Self {
                hwnd,
                _thread: PhantomData,
            };
            if SetTimer(Some(hwnd), 1, 100, None) == 0 {
                return Err(windows::core::Error::from_thread());
            }
            Ok(owner)
        }
    }
    pub fn initialize_picker(&self, picker: &GraphicsCapturePicker) -> Result<()> {
        let initialize: IInitializeWithWindow = picker.cast()?;
        // SAFETY: HWND vivo do STA, inicializado antes de PickSingleItemAsync.
        unsafe { initialize.Initialize(self.hwnd) }
    }
    pub fn pump(&self) -> Result<bool> {
        let mut message = MSG::default();
        // SAFETY: MSG local válida; busca todas as mensagens do STA, inclusive as do picker.
        let status = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if status == -1 {
            return Err(windows::core::Error::from_thread());
        }
        if status == 0 {
            return Ok(false);
        }
        // SAFETY: mensagem inicializada por GetMessageW, despachada na thread proprietária.
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Ok(true)
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        // SAFETY: HWND criado por esta thread e ainda vivo; destruição remove também o timer.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
