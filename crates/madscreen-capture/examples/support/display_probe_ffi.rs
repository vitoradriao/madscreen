#![allow(unsafe_code)]
use windows::Win32::Graphics::Dxgi::*;
type Monitor = (u32, u32, String, isize);
pub fn monitors() -> windows::core::Result<Vec<Monitor>> {
    let mut rows = Vec::new();
    // SAFETY: owns COM interfaces while enumerating; descriptions are copied values.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut a = 0;
        loop {
            let adapter = match factory.EnumAdapters1(a) {
                Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
                other => other?,
            };
            let mut o = 0;
            loop {
                let output = match adapter.EnumOutputs(o) {
                    Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
                    other => other?,
                };
                let desc = output.GetDesc()?;
                if desc.AttachedToDesktop.as_bool() {
                    let len = desc
                        .DeviceName
                        .iter()
                        .position(|c| *c == 0)
                        .unwrap_or(desc.DeviceName.len());
                    rows.push((
                        a,
                        o,
                        String::from_utf16_lossy(&desc.DeviceName[..len]),
                        desc.Monitor.0 as isize,
                    ));
                }
                o += 1;
            }
            a += 1;
        }
    }
    Ok(rows)
}
