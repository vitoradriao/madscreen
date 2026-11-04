//! Seletor nativo num STA com HWND próprio; a captura permanece na thread do encoder.
mod window_ffi;
use anyhow::{Result, anyhow};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::{
    Graphics::Capture::{GraphicsCaptureItem, GraphicsCapturePicker},
    Win32::Foundation::E_POINTER,
};
use windows_future::AsyncStatus;

pub fn pick(running: &AtomicBool) -> Result<Option<GraphicsCaptureItem>> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("capture-picker".into())
            .spawn_scoped(scope, || pick_on_sta(running))?
            .join()
            .map_err(|_| anyhow!("thread do seletor terminou inesperadamente"))?
    })
}

fn pick_on_sta(running: &AtomicBool) -> Result<Option<GraphicsCaptureItem>> {
    let _apartment = window_ffi::Apartment::new()?;
    let owner = window_ffi::Owner::new()?;
    if !running.load(Ordering::Acquire) {
        return Ok(None);
    }
    let picker = GraphicsCapturePicker::new()?;
    owner.initialize_picker(&picker)?;
    let operation = picker.PickSingleItemAsync()?;
    let result = (|| {
        loop {
            if !running.load(Ordering::Acquire) {
                return Ok(None);
            }
            let status = operation.Status()?;
            if status == AsyncStatus::Canceled {
                return Ok(None);
            }
            if status != AsyncStatus::Started {
                return match operation.GetResults() {
                    Ok(item) => Ok(Some(item)),
                    // Cancelamento pelo usuário devolve item nulo: windows-rs o converte em Error::empty()
                    // (HRESULT 0, "operação concluída com êxito"); E_POINTER cobre outras versões.
                    Err(error)
                        if status == AsyncStatus::Completed
                            && (error.code() == E_POINTER || error.code().is_ok()) =>
                    {
                        Ok(None)
                    }
                    Err(error) => Err(error.into()),
                };
            }
            if !owner.pump()? {
                return Ok(None);
            }
        }
    })();
    // Fecha o diálogo também ao receber Stop/EOF ou se o bombeamento falhar.
    if operation
        .Status()
        .is_ok_and(|status| status == AsyncStatus::Started)
    {
        let _ = operation.Cancel();
    }
    result
}
