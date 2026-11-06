//! Read-only detector exercise; no GPU device, frame pool or media session.
#[cfg(windows)]
#[path = "support/display_probe_ffi.rs"]
mod ffi;
#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    for (adapter, output, name, monitor) in ffi::monitors()? {
        let info = madscreen_capture::display::monitor_color_info(monitor);
        println!("adapter={adapter} output={output} name={name} {info:?}");
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Consulta de monitor requer Windows; nenhuma captura e iniciada.");
}
