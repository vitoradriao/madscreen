//! `--list-sources`: janelas e monitores capturáveis, com ícone e miniatura, para o seletor do app.
mod sources_ffi;
use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use madscreen_capture::wgc::GpuDevice;
use madscreen_encode::{nvenc, platform};
use madscreen_ipc::{MonitorSource, SourceList, WindowSource};
use sources_ffi::Bitmap;

fn png_base64(bitmap: &Bitmap) -> Option<String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, bitmap.width, bitmap.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&bitmap.rgba).ok()?;
    writer.finish().ok()?;
    Some(STANDARD.encode(out))
}

/// Título atual da janela, para rotular a sessão iniciada por HWND.
pub fn title_of(hwnd: i64) -> Option<String> {
    Some(sources_ffi::window_title(hwnd)).filter(|t| !t.trim().is_empty())
}

/// A janela ainda existe (falso quando foi destruida ou o processo dela encerrou).
pub fn window_exists(hwnd: isize) -> bool {
    sources_ffi::window_exists(hwnd)
}

pub fn verify_window(hwnd: i64, expected: &str) -> Result<()> {
    anyhow::ensure!(
        sources_ffi::window_identity(hwnd).is_some_and(|(_, instance)| instance == expected),
        "A janela selecionada foi fechada ou substituída. Escolha a fonte novamente."
    );
    Ok(())
}

/// Janelas visíveis, não minimizadas nem "cloaked" e com título; `exclude_pid` omite o próprio app.
/// Com `monitors_only`, omite janelas e todas as miniaturas/ícones.
pub fn list(exclude_pid: Option<u32>, monitors_only: bool) -> Result<SourceList> {
    sources_ffi::enable_dpi_awareness();
    let windows = if monitors_only {
        Vec::new()
    } else {
        sources_ffi::windows(exclude_pid)
            .into_iter()
            .map(|w| WindowSource {
                selection_id: None,
                app_id: w.app_id,
                instance_id: w.instance_id,
                hwnd: w.hwnd,
                title: w.title,
                process: w.process,
                width: w.width,
                height: w.height,
                icon_png_base64: w.icon.as_ref().and_then(png_base64),
                thumbnail_png_base64: w.thumbnail.as_ref().and_then(png_base64),
            })
            .collect()
    };
    let monitors = sources_ffi::monitors(!monitors_only)?
        .into_iter()
        .map(|m| MonitorSource {
            index: m.index,
            name: m.name,
            width: m.width,
            height: m.height,
            primary: m.primary,
            thumbnail_png_base64: m.thumbnail.as_ref().and_then(png_base64),
        })
        .collect();
    // A consulta de capacidades é opcional: nenhuma falha deve impedir a listagem.
    let encoder = (|| -> Result<_> {
        let _runtime = platform::Runtime::new()?;
        let gpu = GpuDevice::new(None)?;
        Ok(crate::limits::h264_limits(&nvenc::probe(&gpu)?))
    })()
    .ok()
    .flatten()
    .or(Some(madscreen_ipc::EncoderLimits::default()));
    Ok(SourceList {
        windows,
        monitors,
        encoder,
    })
}
