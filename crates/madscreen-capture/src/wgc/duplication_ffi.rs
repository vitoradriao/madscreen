//! DXGI/D3D11: apenas metadados do cursor passam pela CPU, nunca pixels do desktop.
#![allow(unsafe_code)]
use super::super::{GpuDevice, GpuFrame};
use windows::{
    Win32::{
        Foundation::{CloseHandle, E_FAIL, E_INVALIDARG, E_POINTER, HANDLE, WAIT_OBJECT_0},
        Graphics::{
            Direct3D::{Fxc::*, *},
            Direct3D11::*,
            Dxgi::{Common::*, *},
        },
        System::Threading::{
            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW,
            SYNCHRONIZATION_SYNCHRONIZE, SetWaitableTimer, TIMER_MODIFY_STATE, WaitForSingleObject,
        },
    },
    core::{Interface, PCSTR, Result, s},
};

fn required<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(|| E_POINTER.into())
}

/// Espera fora de DXGI sem depender da granularidade de ~15,6 ms de Sleep/park.
/// Não muda a resolução global do timer do Windows.
pub(super) struct PollTimer(HANDLE);
impl PollTimer {
    pub(super) fn new() -> Result<Self> {
        // SAFETY: timer privado, sem nome, sem herança; fechado no Drop na thread produtora.
        unsafe {
            CreateWaitableTimerExW(
                None,
                None,
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_MODIFY_STATE.0 | SYNCHRONIZATION_SYNCHRONIZE.0,
            )
            .map(Self)
        }
    }
    pub(super) fn wait(&self) -> Result<()> {
        let due = -10_000_i64;
        // SAFETY: handle vivo, instante relativo de 1 ms; sem APC nem ponteiros persistentes.
        unsafe {
            SetWaitableTimer(self.0, &due, 0, None, None, false)?;
            if WaitForSingleObject(self.0, 100) != WAIT_OBJECT_0 {
                return Err(E_FAIL.into());
            }
        }
        Ok(())
    }
}
impl Drop for PollTimer {
    fn drop(&mut self) {
        // SAFETY: único dono do handle; nenhuma espera pendente nesta thread.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub(super) struct Session {
    duplication: IDXGIOutputDuplication,
    size: (u32, u32),
    rotation: DXGI_MODE_ROTATION,
}
impl Session {
    pub(super) fn open(gpu: &GpuDevice, monitor: isize) -> Result<Self> {
        let dxgi: IDXGIDevice = gpu.device.cast()?;
        // SAFETY: interfaces COM vivas; enumera somente as saídas do device compartilhado.
        unsafe {
            let adapter = dxgi.GetAdapter()?;
            let mut index = 0;
            loop {
                let output = match adapter.EnumOutputs(index) {
                    Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => {
                        return Err(E_INVALIDARG.into());
                    }
                    result => result?,
                };
                let desc = output.GetDesc()?;
                if desc.Monitor.0 as isize == monitor && desc.AttachedToDesktop.as_bool() {
                    let output: IDXGIOutput1 = output.cast()?;
                    return Ok(Self {
                        duplication: output.DuplicateOutput(&gpu.device)?,
                        size: (
                            (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).max(0)
                                as u32,
                            (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).max(0)
                                as u32,
                        ),
                        rotation: desc.Rotation,
                    });
                }
                index += 1;
            }
        }
    }
    pub(super) fn size(&self) -> (u32, u32) {
        self.size
    }
    pub(super) fn rotation(&self) -> DXGI_MODE_ROTATION {
        self.rotation
    }

    pub(super) fn acquire(&self) -> Result<Option<Acquired>> {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource = None;
        // SAFETY: saídas inicializadas. Nunca esperar dentro de DXGI: em devices com
        // proteção multithread, AcquireNextFrame pode reter o lock compartilhado e
        // bloquear conversão/NVENC. O produtor espera fora da API quando não há frame.
        match unsafe {
            self.duplication
                .AcquireNextFrame(0, &mut info, &mut resource)
        } {
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            result => result?,
        }
        // O guard existe antes de validar o recurso: toda aquisição é balanceada, inclusive erro.
        let mut frame = Acquired {
            duplication: self.duplication.clone(),
            info,
            texture: None,
        };
        frame.texture = Some(required(resource)?.cast()?);
        Ok(Some(frame))
    }
}
pub(super) struct Acquired {
    duplication: IDXGIOutputDuplication,
    pub(super) info: DXGI_OUTDUPL_FRAME_INFO,
    texture: Option<ID3D11Texture2D>,
}
impl Acquired {
    pub(super) fn texture(&self) -> Result<&ID3D11Texture2D> {
        self.texture.as_ref().ok_or_else(|| E_POINTER.into())
    }
}
impl Drop for Acquired {
    fn drop(&mut self) {
        // SAFETY: cada guard nasce após AcquireNextFrame bem-sucedido e libera uma única vez.
        unsafe {
            let _ = self.duplication.ReleaseFrame();
        }
    }
}

fn texture(
    gpu: &GpuDevice,
    size: (u32, u32),
    render: bool,
    data: Option<&[u8]>,
) -> Result<GpuFrame> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size.0,
        Height: size.1,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE.0
            | if render {
                D3D11_BIND_RENDER_TARGET.0
            } else {
                0
            }) as u32,
        ..Default::default()
    };
    let initial = data.map(|data| D3D11_SUBRESOURCE_DATA {
        pSysMem: data.as_ptr().cast(),
        SysMemPitch: size.0 * 4,
        SysMemSlicePitch: 0,
    });
    let (mut texture, mut view) = (None, None);
    // SAFETY: descritor BGRA com um subrecurso; dados opcionais contêm width*height*4 bytes.
    unsafe {
        gpu.device.CreateTexture2D(
            &desc,
            initial.as_ref().map(std::ptr::from_ref),
            Some(&mut texture),
        )?;
    }
    let texture = required(texture)?;
    // SAFETY: textura viva com bind SRV e formato BGRA conhecido.
    unsafe {
        gpu.device
            .CreateShaderResourceView(&texture, None, Some(&mut view))?;
    }
    Ok(GpuFrame {
        texture,
        view: required(view)?,
        qpc_capture: 0,
        qpc_acquired: 0,
        format: desc.Format,
        size,
    })
}
pub(super) fn render_frame(gpu: &GpuDevice, size: (u32, u32)) -> Result<GpuFrame> {
    texture(gpu, size, true, None)
}

fn compile(entry: PCSTR, profile: PCSTR) -> Result<ID3DBlob> {
    let shader = include_str!("duplication.hlsl");
    let (mut code, mut errors) = (None, None);
    // SAFETY: fonte embutida e identificadores NUL; o compilador escreve apenas nos blobs locais.
    let result = unsafe {
        D3DCompile(
            shader.as_ptr().cast(),
            shader.len(),
            None,
            None,
            None,
            entry,
            profile,
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(error) = result {
        if let Some(errors) = errors {
            // SAFETY: slice limitada pelo tamanho do blob ainda vivo.
            let message = unsafe {
                std::slice::from_raw_parts(
                    errors.GetBufferPointer().cast::<u8>(),
                    errors.GetBufferSize(),
                )
            };
            tracing::error!(message=%String::from_utf8_lossy(message), "Falha no shader de captura DXGI");
        }
        return Err(error);
    }
    required(code)
}

pub(super) struct Compositor {
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    params: ID3D11Buffer,
    raw: Option<GpuFrame>,
    output: Option<(ID3D11Texture2D, ID3D11RenderTargetView)>,
    pointer: Option<GpuFrame>,
    shape_bytes: Vec<u8>,
    pointer_kind: u32,
    pointer_position: (i32, i32),
    pointer_visible: bool,
}
impl Compositor {
    pub(super) fn new(gpu: &GpuDevice) -> Result<Self> {
        let vs = compile(s!("vs_main"), s!("vs_5_0"))?;
        let ps = compile(s!("ps_main"), s!("ps_5_0"))?;
        let (mut vertex, mut pixel, mut params) = (None, None, None);
        // SAFETY: blobs vivos e descritores inicializados no device compartilhado.
        unsafe {
            gpu.device.CreateVertexShader(
                std::slice::from_raw_parts(vs.GetBufferPointer().cast(), vs.GetBufferSize()),
                None,
                Some(&mut vertex),
            )?;
            gpu.device.CreatePixelShader(
                std::slice::from_raw_parts(ps.GetBufferPointer().cast(), ps.GetBufferSize()),
                None,
                Some(&mut pixel),
            )?;
            gpu.device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: 32,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut params),
            )?;
        }
        Ok(Self {
            vertex: required(vertex)?,
            pixel: required(pixel)?,
            params: required(params)?,
            raw: None,
            output: None,
            pointer: None,
            shape_bytes: Vec::new(),
            pointer_kind: 0,
            pointer_position: (0, 0),
            pointer_visible: false,
        })
    }
    pub(super) fn reset(&mut self) {
        self.raw = None;
        self.output = None;
        self.pointer = None;
        self.pointer_visible = false;
        self.pointer_kind = 0;
    }
    pub(super) fn update_pointer(&mut self, gpu: &GpuDevice, frame: &Acquired) -> Result<()> {
        if frame.info.LastMouseUpdateTime != 0 {
            self.pointer_position = (
                frame.info.PointerPosition.Position.x,
                frame.info.PointerPosition.Position.y,
            );
            self.pointer_visible = frame.info.PointerPosition.Visible.as_bool();
        }
        let size = frame.info.PointerShapeBufferSize;
        if size == 0 {
            return Ok(());
        }
        // Aloca somente em troca de forma/tamanho do cursor, nunca para pixels do desktop.
        if size > 4 * 1024 * 1024 {
            return Err(E_INVALIDARG.into());
        }
        self.shape_bytes.resize(size as usize, 0);
        let mut info = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
        let mut needed = 0;
        // SAFETY: buffer dimensionado pelo tamanho informado por DXGI, saídas locais válidas.
        unsafe {
            frame.duplication.GetFramePointerShape(
                size,
                self.shape_bytes.as_mut_ptr().cast(),
                &mut needed,
                &mut info,
            )?;
        }
        let (pixels, dimensions) = super::super::super::pointer::decode_shape(
            info.Type,
            info.Width,
            info.Height,
            info.Pitch,
            &self.shape_bytes,
        )
        .ok_or_else(|| windows::core::Error::from_hresult(E_INVALIDARG))?;
        self.pointer = Some(texture(gpu, dimensions, false, Some(&pixels))?);
        self.pointer_kind = info.Type;
        Ok(())
    }
    pub(super) fn compose(
        &mut self,
        gpu: &GpuDevice,
        dst: &GpuFrame,
        src: &ID3D11Texture2D,
        rotation: DXGI_MODE_ROTATION,
        cursor: bool,
    ) -> Result<()> {
        if rotation == DXGI_MODE_ROTATION_IDENTITY
            && (!cursor || !self.pointer_visible || self.pointer.is_none())
        {
            super::super::ffi::copy(gpu, dst, src);
            return Ok(());
        }
        let raw_size = super::super::ffi::texture_size(src);
        if self.raw.as_ref().is_none_or(|raw| raw.size != raw_size) {
            self.raw = Some(texture(gpu, raw_size, false, None)?);
        }
        if self
            .output
            .as_ref()
            .is_none_or(|(texture, _)| *texture != dst.texture)
        {
            let mut rtv = None;
            // SAFETY: destino com bind render target no mesmo device; saída local válida.
            unsafe {
                gpu.device
                    .CreateRenderTargetView(&dst.texture, None, Some(&mut rtv))?;
            }
            self.output = Some((dst.texture.clone(), required(rtv)?));
        }
        let raw = self
            .raw
            .as_ref()
            .ok_or_else(|| windows::core::Error::from_hresult(E_POINTER))?;
        let (_, output) = self
            .output
            .as_ref()
            .ok_or_else(|| windows::core::Error::from_hresult(E_POINTER))?;
        let pointer = self.pointer.as_ref();
        let pointer_size = pointer.map_or((0, 0), |p| p.size);
        let params = [
            dst.size.0 as i32,
            dst.size.1 as i32,
            rotation.0,
            if cursor && self.pointer_visible {
                self.pointer_kind as i32
            } else {
                0
            },
            self.pointer_position.0,
            self.pointer_position.1,
            pointer_size.0 as i32,
            pointer_size.1 as i32,
        ];
        let c = &gpu.context;
        // SAFETY: recursos no mesmo device; o produtor mantém a mailbox durante toda esta
        // sequência e o consumidor mantém o FrameLease durante converter.submit. Isso impede
        // intercalar os dois passes; polling/Flush fora do lease não muda estado de desenho.
        unsafe {
            c.CopyResource(&raw.texture, src);
            c.UpdateSubresource(&self.params, 0, None, params.as_ptr().cast(), 0, 0);
            c.OMSetRenderTargets(Some(&[Some(output.clone())]), None);
            c.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: dst.size.0 as f32,
                Height: dst.size.1 as f32,
                MaxDepth: 1.,
                ..Default::default()
            }]));
            c.IASetInputLayout(None);
            c.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            c.VSSetShader(&self.vertex, None);
            c.PSSetShader(&self.pixel, None);
            c.PSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            c.PSSetShaderResources(
                0,
                Some(&[Some(raw.view.clone()), pointer.map(|p| p.view.clone())]),
            );
            c.Draw(3, 0);
            c.PSSetShaderResources(0, Some(&[None, None]));
            c.OMSetRenderTargets(None, None);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "duplication_tests.rs"]
mod tests;
