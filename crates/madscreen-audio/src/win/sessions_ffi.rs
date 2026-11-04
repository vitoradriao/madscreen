#![allow(unsafe_code)]
//! Enumeracao das sessoes de audio dos dispositivos de saida ativos.

use windows::{
    Win32::{
        Media::Audio::{
            AudioSessionStateActive, DEVICE_STATE_ACTIVE, Endpoints::IAudioMeterInformation,
            IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator,
            eRender,
        },
        System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
            CoUninitialize,
        },
    },
    core::Interface,
};

use crate::procs::SessionInfo;

struct ComGuard(bool);

impl ComGuard {
    fn new() -> Self {
        // SAFETY: inicializacao COM da thread atual; so desfaz (CoUninitialize) se esta chamada
        // teve sucesso (S_OK ou S_FALSE).
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: par do CoInitializeEx bem-sucedido em `new`.
            unsafe { CoUninitialize() };
        }
    }
}

pub(crate) fn enumerate_sessions() -> Result<Vec<SessionInfo>, String> {
    let _com = ComGuard::new();
    let mut result = Vec::new();
    // SAFETY: interfaces COM usadas apenas nesta thread, enquanto o COM esta inicializado; a
    // string do DisplayName e liberada com CoTaskMemFree depois de copiada.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let devices = enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|e| e.to_string())?;
        for device_index in 0..devices.GetCount().map_err(|e| e.to_string())? {
            let Ok(device) = devices.Item(device_index) else {
                continue;
            };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            let Ok(sessions) = manager.GetSessionEnumerator() else {
                continue;
            };
            let Ok(count) = sessions.GetCount() else {
                continue;
            };
            for index in 0..count {
                let Ok(control) = sessions.GetSession(index) else {
                    continue;
                };
                let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
                    continue;
                };
                if control2.IsSystemSoundsSession().0 == 0 {
                    continue;
                }
                let Ok(pid) = control2.GetProcessId() else {
                    continue;
                };
                let active = control
                    .GetState()
                    .is_ok_and(|s| s == AudioSessionStateActive);
                let peak = control
                    .cast::<IAudioMeterInformation>()
                    .and_then(|meter| meter.GetPeakValue())
                    .unwrap_or(0.0);
                let display_name = control
                    .GetDisplayName()
                    .map(|name| {
                        let text = name.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(name.as_ptr().cast()));
                        text
                    })
                    .unwrap_or_default();
                result.push(SessionInfo {
                    pid,
                    active,
                    peak,
                    display_name,
                });
            }
        }
    }
    Ok(result)
}
