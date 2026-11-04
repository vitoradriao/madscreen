//! Testes GPU opt-in. Readback somente de imagens sintéticas minúsculas, fora do produto.
use super::*;

fn read_pixels(gpu: &GpuDevice, frame: &GpuFrame) -> Result<Vec<[u8; 4]>> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: saídas locais válidas; staging é exclusivo deste teste de pixels sintéticos.
    unsafe {
        frame.texture.GetDesc(&mut desc);
    }
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging = None;
    // SAFETY: descritor válido de staging, sem bind GPU; saída local inicializada.
    unsafe {
        gpu.device
            .CreateTexture2D(&desc, None, Some(&mut staging))?;
    }
    let staging = required(staging)?;
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: cópia no mesmo device; Map aguarda conclusão e só lê o staging do teste.
    unsafe {
        gpu.context.CopyResource(&staging, &frame.texture);
        gpu.context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
    }
    let mut pixels = Vec::new();
    for y in 0..frame.size.1 {
        // SAFETY: Map bem-sucedido, row pitch fornecido pelo driver, largura dentro do recurso.
        let row = unsafe {
            std::slice::from_raw_parts(
                mapped
                    .pData
                    .cast::<u8>()
                    .add((y * mapped.RowPitch) as usize),
                frame.size.0 as usize * 4,
            )
        };
        for pixel in row.chunks_exact(4) {
            pixels.push([pixel[0], pixel[1], pixel[2], pixel[3]]);
        }
    }
    // SAFETY: balanceia o Map bem-sucedido; nenhuma referência ao mapeamento escapa.
    unsafe {
        gpu.context.Unmap(&staging, 0);
    }
    Ok(pixels)
}

#[test]
#[ignore = "requer Windows com GPU D3D11; executar explicitamente"]
fn gpu_rotations_and_pointer_blending() -> Result<()> {
    let gpu = GpuDevice::new(None).map_err(|_| windows::core::Error::from_hresult(E_INVALIDARG))?;
    let bytes: Vec<u8> = (1..=6).flat_map(|n| [n, 20, 40, 255]).collect();
    let input = texture(&gpu, (3, 2), false, Some(&bytes))?;
    let mut compositor = Compositor::new(&gpu)?;
    for (rotation, size, expected) in [
        (DXGI_MODE_ROTATION_IDENTITY, (3, 2), [1, 2, 3, 4, 5, 6]),
        (DXGI_MODE_ROTATION_ROTATE90, (2, 3), [4, 1, 5, 2, 6, 3]),
        (DXGI_MODE_ROTATION_ROTATE180, (3, 2), [6, 5, 4, 3, 2, 1]),
        (DXGI_MODE_ROTATION_ROTATE270, (2, 3), [3, 6, 2, 5, 1, 4]),
    ] {
        compositor.reset();
        let output = render_frame(&gpu, size)?;
        compositor.compose(&gpu, &output, &input.texture, rotation, false)?;
        let pixels = read_pixels(&gpu, &output)?;
        assert_eq!(
            pixels.iter().map(|p| p[0]).collect::<Vec<_>>(),
            expected,
            "rotation={rotation:?}"
        );
    }
    let background = texture(
        &gpu,
        (4, 1),
        false,
        Some(&[
            10, 20, 40, 255, 10, 20, 40, 255, 10, 20, 40, 255, 10, 20, 40, 255,
        ]),
    )?;
    let output = render_frame(&gpu, (4, 1))?;
    compositor.reset();
    let (mono, size) = crate::pointer::decode_shape(1, 4, 2, 1, &[0b00110000, 0b01010000])
        .ok_or_else(|| windows::core::Error::from_hresult(E_INVALIDARG))?;
    compositor.pointer = Some(texture(&gpu, size, false, Some(&mono))?);
    compositor.pointer_kind = 1;
    compositor.pointer_visible = true;
    compositor.compose(
        &gpu,
        &output,
        &background.texture,
        DXGI_MODE_ROTATION_IDENTITY,
        true,
    )?;
    assert_eq!(
        read_pixels(&gpu, &output)?,
        [
            [0, 0, 0, 255],
            [255, 255, 255, 255],
            [10, 20, 40, 255],
            [245, 235, 215, 255]
        ]
    );
    compositor.pointer = Some(texture(
        &gpu,
        (2, 1),
        false,
        Some(&[1, 2, 3, 0, 255, 0, 255, 255]),
    )?);
    compositor.pointer_kind = 4;
    compositor.compose(
        &gpu,
        &output,
        &background.texture,
        DXGI_MODE_ROTATION_IDENTITY,
        true,
    )?;
    assert_eq!(
        &read_pixels(&gpu, &output)?[..2],
        [[1, 2, 3, 255], [245, 20, 215, 255]]
    );
    compositor.pointer = Some(texture(
        &gpu,
        (2, 1),
        false,
        Some(&[200, 100, 80, 255, 200, 100, 80, 0]),
    )?);
    compositor.pointer_kind = 2;
    compositor.compose(
        &gpu,
        &output,
        &background.texture,
        DXGI_MODE_ROTATION_IDENTITY,
        true,
    )?;
    assert_eq!(
        &read_pixels(&gpu, &output)?[..2],
        [[200, 100, 80, 255], [10, 20, 40, 255]]
    );
    // Cursor parcialmente fora da tela e opção desligada.
    compositor.pointer_position = (-1, 0);
    compositor.compose(
        &gpu,
        &output,
        &background.texture,
        DXGI_MODE_ROTATION_IDENTITY,
        true,
    )?;
    assert_eq!(read_pixels(&gpu, &output)?[0], [10, 20, 40, 255]);
    compositor.compose(
        &gpu,
        &output,
        &background.texture,
        DXGI_MODE_ROTATION_IDENTITY,
        false,
    )?;
    assert_eq!(read_pixels(&gpu, &output)?, [[10, 20, 40, 255]; 4]);
    Ok(())
}
