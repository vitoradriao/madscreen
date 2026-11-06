//! Consultas Windows isoladas; nunca chamadas no caminho normal de frames.
#![allow(unsafe_code)]
use super::{AdvancedColor, DisplayColorInfo, correlated_target, decide};
use crate::CaptureTarget;
use windows::{
    Win32::{
        Devices::Display::*,
        Foundation::{ERROR_SUCCESS, HWND},
        Graphics::{Dxgi::*, Gdi::*},
    },
    core::Interface,
};

/// Resolve janela pelo monitor de maior intersecao, sem cair no monitor primario.
pub fn monitor_for_target(target: CaptureTarget) -> Option<isize> {
    match target {
        CaptureTarget::Monitor(handle) => (handle != 0).then_some(handle),
        CaptureTarget::Window(handle) => {
            // SAFETY: consulta nao retentiva; HWND invalido resulta em handle nulo.
            let monitor = unsafe { MonitorFromWindow(HWND(handle as _), MONITOR_DEFAULTTONULL) };
            (!monitor.0.is_null()).then_some(monitor.0 as isize)
        }
    }
}

// windows-rs 0.62 omits this newer packet. Layout from wingdi.h SDK 26100.
#[repr(C)]
#[derive(Default)]
struct AdvancedColor2 {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    flags: u32,
    color_encoding: u32,
    bits: u32,
    active_mode: u32,
}

fn header(
    kind: i32,
    size: usize,
    path: &DISPLAYCONFIG_PATH_INFO,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: DISPLAYCONFIG_DEVICE_INFO_TYPE(kind),
        size: size as u32,
        adapterId: path.targetInfo.adapterId,
        id: path.targetInfo.id,
    }
}

fn advanced_for_monitor(monitor: HMONITOR) -> Option<AdvancedColor> {
    let mut monitor_info = MONITORINFOEXW::default();
    monitor_info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: MONITORINFOEXW extends MONITORINFO, cbSize describes full writable buffer.
    if !unsafe { GetMonitorInfoW(monitor, &mut monitor_info.monitorInfo) }.as_bool() {
        return None;
    }
    let (mut path_count, mut mode_count) = (0, 0);
    // SAFETY: initialized writable counts; active paths only, no config changes.
    if unsafe {
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
    } != ERROR_SUCCESS
    {
        return None;
    }
    // Bound allocation if a driver returns unreasonable sizes; retry on next lifecycle event.
    if path_count > 1024 || mode_count > 4096 {
        return None;
    }
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
    // SAFETY: vectors sized to returned counts. Topology races become unknown, never partial matches.
    if unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
    } != ERROR_SUCCESS
    {
        return None;
    }
    let mut sources = Vec::new();
    for path in paths.iter().take(path_count as usize) {
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: path.sourceInfo.adapterId,
                id: path.sourceInfo.id,
            },
            ..Default::default()
        };
        // SAFETY: correctly typed packet with source adapter/id from QueryDisplayConfig.
        if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0 {
            return None;
        }
        sources.push((source.viewGdiDeviceName, path));
    }
    let path = correlated_target(
        &monitor_info.szDevice,
        sources.iter().map(|(name, path)| (name.as_slice(), *path)),
    )?;
    let mut modern = AdvancedColor2 {
        header: header(15, size_of::<AdvancedColor2>(), path),
        ..Default::default()
    };
    // SAFETY: repr(C) packet is 36 bytes, type 15 (GET_ADVANCED_COLOR_INFO_2), all output initialized.
    if unsafe { DisplayConfigGetDeviceInfo(&mut modern.header) } == 0 {
        return Some(AdvancedColor {
            version2: true,
            flags: modern.flags,
            bits: modern.bits,
            active_mode: modern.active_mode,
        });
    }
    let mut legacy = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
        header: header(9, size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>(), path),
        ..Default::default()
    };
    // SAFETY: fallback packet from windows-rs with correct type/size and matching target.
    if unsafe { DisplayConfigGetDeviceInfo(&mut legacy.header) } != 0 {
        return None;
    }
    // SAFETY: union's value covers the bitfield word written by Windows.
    Some(AdvancedColor {
        version2: false,
        flags: unsafe { legacy.Anonymous.value },
        bits: legacy.bitsPerColorChannel,
        active_mode: 0,
    })
}

fn dxgi_for_monitor(monitor: HMONITOR) -> Option<(i32, u32)> {
    // SAFETY: COM enumeration owns references; returned descriptions contain no retained pointers.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut index = 0;
        loop {
            let adapter = factory.EnumAdapters1(index).ok()?;
            let mut output_index = 0;
            while let Ok(output) = adapter.EnumOutputs(output_index) {
                if output.GetDesc().ok()?.Monitor == monitor {
                    let extended: IDXGIOutput6 = output.cast().ok()?;
                    let description = extended.GetDesc1().ok()?;
                    return Some((description.ColorSpace.0, description.BitsPerColor));
                }
                output_index += 1;
            }
            index += 1;
        }
    }
}

/// HMONITOR da captura. Falha/transicao de topologia devolve campos desconhecidos.
/// Executar no inicio/recriacao/troca de monitor; nunca por frame.
pub fn monitor_color_info(monitor: isize) -> DisplayColorInfo {
    if monitor == 0 {
        return DisplayColorInfo::default();
    }
    let monitor = HMONITOR(monitor as _);
    decide(advanced_for_monitor(monitor), dxgi_for_monitor(monitor))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn newer_packet_matches_windows_sdk_layout() {
        assert_eq!(size_of::<AdvancedColor2>(), 36);
        assert_eq!(std::mem::offset_of!(AdvancedColor2, flags), 20);
        assert_eq!(std::mem::offset_of!(AdvancedColor2, active_mode), 32);
    }
    #[test]
    fn null_monitor_is_unknown() {
        assert_eq!(monitor_color_info(0), DisplayColorInfo::default());
    }
}
