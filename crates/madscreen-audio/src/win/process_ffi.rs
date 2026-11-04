#![allow(unsafe_code)]
//! Tabela de processos (Toolhelp32), caminho da imagem e descricao do produto.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND, MAX_PATH},
        Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
        },
        UI::WindowsAndMessaging::GetWindowThreadProcessId,
    },
    core::{PCWSTR, PWSTR},
};

use crate::procs::ProcInfo;

fn wide_to_string(wide: &[u16]) -> String {
    let len = wide.iter().position(|c| *c == 0).unwrap_or(wide.len());
    OsString::from_wide(&wide[..len])
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn snapshot() -> Vec<ProcInfo> {
    let mut table = Vec::new();
    // SAFETY: snapshot Toolhelp e entradas PROCESSENTRY32W com dwSize preenchido, como exige a API;
    // o handle e fechado antes de retornar.
    unsafe {
        let Ok(handle) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return table;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(handle, &mut entry).is_ok() {
            loop {
                table.push(ProcInfo {
                    pid: entry.th32ProcessID,
                    parent: entry.th32ParentProcessID,
                    exe: wide_to_string(&entry.szExeFile),
                });
                if Process32NextW(handle, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(handle);
    }
    table
}

pub(crate) fn image_path(pid: u32) -> Option<String> {
    // SAFETY: handle de consulta limitada fechado no fim; o buffer tem MAX_PATH*2 unidades e
    // `size` informa a capacidade real.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = vec![0_u16; (MAX_PATH * 2) as usize];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        result.ok()?;
        Some(wide_to_string(&buffer[..size as usize]))
    }
}

/// `FileDescription` do recurso de versao do executavel (ex.: "Google Chrome").
pub(crate) fn file_description(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    // SAFETY: o caminho termina em NUL; `data` tem o tamanho devolvido pela API; os ponteiros
    // devolvidos por VerQueryValueW apontam para dentro de `data`, que vive ate o fim da funcao,
    // e os comprimentos informados delimitam as leituras.
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0_u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, data.as_mut_ptr().cast()).ok()?;
        let mut translations: *mut core::ffi::c_void = core::ptr::null_mut();
        let mut length = 0_u32;
        let key: Vec<u16> = "\\VarFileInfo\\Translation\0".encode_utf16().collect();
        if !VerQueryValueW(
            data.as_ptr().cast(),
            PCWSTR(key.as_ptr()),
            &mut translations,
            &mut length,
        )
        .as_bool()
            || (length as usize) < 4
        {
            return None;
        }
        let pairs = core::slice::from_raw_parts(translations.cast::<u16>(), (length / 2) as usize);
        for pair in pairs.chunks_exact(2) {
            let sub: Vec<u16> = format!(
                "\\StringFileInfo\\{:04x}{:04x}\\FileDescription\0",
                pair[0], pair[1]
            )
            .encode_utf16()
            .collect();
            let mut text: *mut core::ffi::c_void = core::ptr::null_mut();
            let mut chars = 0_u32;
            if VerQueryValueW(
                data.as_ptr().cast(),
                PCWSTR(sub.as_ptr()),
                &mut text,
                &mut chars,
            )
            .as_bool()
                && chars > 1
            {
                let slice = core::slice::from_raw_parts(text.cast::<u16>(), chars as usize);
                let value = wide_to_string(slice).trim().to_owned();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
        None
    }
}

pub(crate) fn window_process_id(hwnd: isize) -> Option<u32> {
    let mut pid = 0_u32;
    // SAFETY: `pid` e um u32 valido; a API aceita HWND invalido e devolve 0.
    let thread = unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid)) };
    (thread != 0 && pid != 0).then_some(pid)
}
