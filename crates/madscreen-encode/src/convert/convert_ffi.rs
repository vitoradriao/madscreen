#![allow(unsafe_code)]
use super::{ConvertMode, InputFormat};
use crate::{EncodeError, Result, platform};
use madscreen_capture::{
    Rect, fit_rect,
    wgc::{GpuDevice, GpuFrame},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, E_POINTER, HANDLE, RECT, WAIT_OBJECT_0},
        Graphics::{
            Direct3D::{Fxc::*, *},
            Direct3D11::*,
            Dxgi::{Common::*, DXGI_ERROR_DEVICE_REMOVED},
        },
        System::Threading::{CreateEventW, WaitForSingleObject},
    },
    core::{Interface, PCSTR, s},
};
const SHADER: &str = include_str!("convert.hlsl");
fn required<T>(value: Option<T>) -> windows::core::Result<T> {
    value.ok_or_else(|| windows::core::Error::from_hresult(E_POINTER))
}
fn compile(entry: PCSTR, target: PCSTR) -> windows::core::Result<ID3DBlob> {
    let mut code = None;
    let mut errors = None;
    // SAFETY: HLSL embutido e nomes NUL; blobs retidos durante a leitura/criação de shaders.
    let result = unsafe {
        D3DCompile(
            SHADER.as_ptr().cast(),
            SHADER.len(),
            None,
            None,
            None,
            entry,
            target,
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(error) = result {
        if let Some(errors) = errors {
            // SAFETY: ponteiro e tamanho fornecidos pelo blob vivo.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    errors.GetBufferPointer().cast::<u8>(),
                    errors.GetBufferSize(),
                )
            };
            tracing::error!(message=%String::from_utf8_lossy(bytes),"Falha no HLSL de conversão");
        }
        return Err(error);
    }
    required(code)
}
struct ShaderPass {
    gpu: GpuDevice,
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    params: ID3D11Buffer,
    filter_mode: u32,
}
impl ShaderPass {
    fn new(gpu: &GpuDevice, entry: PCSTR) -> windows::core::Result<Self> {
        let vs = compile(s!("vs_main"), s!("vs_5_0"))?;
        let ps = compile(entry, s!("ps_5_0"))?;
        let (mut vertex, mut pixel, mut sampler, mut params) = (None, None, None, None);
        // SAFETY: blobs retidos e descriptors válidos; buffer constante apenas CPU→GPU via UpdateSubresource.
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
            gpu.device.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                },
                Some(&mut sampler),
            )?;
            gpu.device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: 16,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut params),
            )?;
        }
        Ok(Self {
            gpu: gpu.clone(),
            vertex: required(vertex)?,
            pixel: required(pixel)?,
            sampler: required(sampler)?,
            params: required(params)?,
            filter_mode: 0,
        })
    }
    fn draw(
        &self,
        target: &ID3D11RenderTargetView,
        view: Option<&ID3D11ShaderResourceView>,
        rect: Rect,
        clear: [f32; 4],
        counter: u32,
    ) {
        let c = &self.gpu.context;
        let params = [counter, rect.width, rect.height, self.filter_mode];
        // SAFETY: interfaces no mesmo device; SRV do frame lida somente durante seu lease.
        // Operações usam o contexto protegido e não há Map, staging ou leitura de pixels pela CPU.
        unsafe {
            c.ClearRenderTargetView(target, &clear);
            c.UpdateSubresource(&self.params, 0, None, params.as_ptr().cast(), 0, 0);
            c.OMSetRenderTargets(Some(&[Some(target.clone())]), None);
            c.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: rect.x as f32,
                TopLeftY: rect.y as f32,
                Width: rect.width as f32,
                Height: rect.height as f32,
                MinDepth: 0.,
                MaxDepth: 1.,
            }]));
            c.IASetInputLayout(None);
            c.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            c.VSSetShader(&self.vertex, None);
            c.PSSetShader(&self.pixel, None);
            c.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            c.PSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            c.PSSetShaderResources(0, Some(&[view.cloned()]));
            c.Draw(3, 0);
            c.PSSetShaderResources(0, Some(&[None]));
            c.OMSetRenderTargets(None, None);
        }
    }
}
fn texture(
    gpu: &GpuDevice,
    size: (u32, u32),
    format: DXGI_FORMAT,
) -> windows::core::Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size.0,
        Height: size.1,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut output = None;
    // SAFETY: GPU-only texture, nenhum dado CPU ou staging.
    unsafe {
        gpu.device.CreateTexture2D(&desc, None, Some(&mut output))?;
    }
    required(output)
}
fn plane_view(
    gpu: &GpuDevice,
    texture: &ID3D11Texture2D,
    format: DXGI_FORMAT,
    plane: u32,
) -> windows::core::Result<ID3D11RenderTargetView> {
    let device: ID3D11Device3 = gpu.device.cast()?;
    let desc = D3D11_RENDER_TARGET_VIEW_DESC1 {
        Format: format,
        ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_RENDER_TARGET_VIEW_DESC1_0 {
            Texture2D: D3D11_TEX2D_RTV1 {
                MipSlice: 0,
                PlaneSlice: plane,
            },
        },
    };
    let mut output = None;
    // SAFETY: plane 0=R8, 1=R8G8 da textura NV12; driver valida suporte e permite fallback.
    unsafe {
        device.CreateRenderTargetView1(texture, Some(&desc), Some(&mut output))?;
    }
    required(output)?.cast()
}
fn bgra_view(
    gpu: &GpuDevice,
    texture: &ID3D11Texture2D,
) -> windows::core::Result<ID3D11RenderTargetView> {
    let mut output = None;
    // SAFETY: textura BGRA8 com bind render target.
    unsafe {
        gpu.device
            .CreateRenderTargetView(texture, None, Some(&mut output))?;
    }
    required(output)
}
struct Fence {
    context: ID3D11DeviceContext4,
    fence: ID3D11Fence,
    event: HANDLE,
    value: u64,
}
impl Fence {
    fn new(gpu: &GpuDevice) -> windows::core::Result<Self> {
        let device: ID3D11Device5 = gpu.device.cast()?;
        let context = gpu.context.cast()?;
        let mut fence = None;
        // SAFETY: fence privado, recurso/evento retidos até Drop, sem compartilhamento externo.
        unsafe {
            device.CreateFence(0, D3D11_FENCE_FLAG_NONE, &mut fence)?;
            Ok(Self {
                context,
                fence: required(fence)?,
                event: CreateEventW(None, false, false, None)?,
                value: 0,
            })
        }
    }
    fn signal(&mut self) -> windows::core::Result<()> {
        self.value += 1;
        // SAFETY: fence/context exclusivos e vivos; sinal não espera execução da GPU.
        unsafe {
            self.context.Signal(&self.fence, self.value)?;
            self.context.Flush();
        }
        Ok(())
    }
    fn ready(&self) -> Result<bool> {
        // SAFETY: consulta não bloqueante de fence viva; UINT64_MAX indica device removido.
        let completed = unsafe { self.fence.GetCompletedValue() };
        if completed == u64::MAX {
            return Err(windows::core::Error::from_hresult(DXGI_ERROR_DEVICE_REMOVED).into());
        }
        Ok(completed >= self.value)
    }
    fn wait(&mut self) -> windows::core::Result<()> {
        // SAFETY: sinal após todos os comandos de conversão; espera bloqueante limitada (sem busy-wait).
        unsafe {
            self.fence.SetEventOnCompletion(self.value, self.event)?;
            if WaitForSingleObject(self.event, 5000) != WAIT_OBJECT_0 {
                return Err(windows::core::Error::from_hresult(
                    DXGI_ERROR_DEVICE_REMOVED,
                ));
            }
        }
        Ok(())
    }
}
impl Drop for Fence {
    fn drop(&mut self) {
        // SAFETY: handle privado, sem espera/callback pendente após retorno de wait.
        let _ = unsafe { CloseHandle(self.event) };
    }
}
struct PlaneTargets {
    y: ID3D11RenderTargetView,
    uv: ID3D11RenderTargetView,
}
struct VideoProcessor {
    device: ID3D11VideoDevice,
    context: ID3D11VideoContext,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    inputs: Vec<(ID3D11Texture2D, ID3D11VideoProcessorInputView)>,
    outputs: Vec<ID3D11VideoProcessorOutputView>,
    input_size: (u32, u32),
    output_size: (u32, u32),
    fps: crate::FrameRate,
}
impl VideoProcessor {
    fn new(
        gpu: &GpuDevice,
        input: (u32, u32),
        output: (u32, u32),
        fps: crate::FrameRate,
        textures: &[ID3D11Texture2D],
    ) -> windows::core::Result<Self> {
        let device: ID3D11VideoDevice = gpu.device.cast()?;
        let context: ID3D11VideoContext = gpu.context.cast()?;
        let rate = DXGI_RATIONAL {
            Numerator: fps.num,
            Denominator: fps.den,
        };
        let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: input.0,
            InputHeight: input.1,
            OutputFrameRate: rate,
            OutputWidth: output.0,
            OutputHeight: output.1,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        // SAFETY: descriptors progressivos SDR com dimensões válidas; enumeração em inicialização.
        let (enumerator, processor) = unsafe {
            let enumerator = device.CreateVideoProcessorEnumerator(&desc)?;
            let processor = device.CreateVideoProcessor(&enumerator, 0)?;
            (enumerator, processor)
        };
        let mut outputs = Vec::with_capacity(textures.len());
        for texture in textures {
            let desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                },
            };
            let mut view = None;
            // SAFETY: textura NV12 retida pelo conversor; view criada uma vez por slot.
            unsafe {
                device.CreateVideoProcessorOutputView(
                    texture,
                    &enumerator,
                    &desc,
                    Some(&mut view),
                )?;
            }
            outputs.push(required(view)?);
        }
        // Bits D3D11_VIDEO_PROCESSOR_COLOR_SPACE: Usage:1 RGB_Range:1 YCbCr_Matrix:1 YCbCr_xvYCC:1 Nominal_Range:2.
        // RGB full: RGB_Range=0, Nominal_Range=2 (0..255); YCbCr BT709 limited: matrix=1, Nominal_Range=1.
        // SAFETY: estado do VP exclusivo; sem deinterlace, auto-enhancement ou frame interpolation.
        unsafe {
            context.VideoProcessorSetStreamFrameFormat(
                &processor,
                0,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            );
            context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
            context.VideoProcessorSetStreamColorSpace(
                &processor,
                0,
                &D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 2 << 4 },
            );
            context.VideoProcessorSetOutputColorSpace(
                &processor,
                &D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
                    _bitfield: (1 << 2) | (1 << 4),
                },
            );
            context.VideoProcessorSetOutputBackgroundColor(
                &processor,
                true,
                &D3D11_VIDEO_COLOR {
                    Anonymous: D3D11_VIDEO_COLOR_0 {
                        YCbCr: D3D11_VIDEO_COLOR_YCbCrA {
                            Y: 16. / 255.,
                            Cb: 128. / 255.,
                            Cr: 128. / 255.,
                            A: 1.,
                        },
                    },
                },
            );
        }
        Ok(Self {
            device,
            context,
            enumerator,
            processor,
            inputs: Vec::with_capacity(64),
            outputs,
            input_size: input,
            output_size: output,
            fps,
        })
    }
    fn draw(&mut self, frame: &GpuFrame, index: usize) -> windows::core::Result<()> {
        let input_index = if let Some(index) = self
            .inputs
            .iter()
            .position(|(texture, _)| texture.as_raw() == frame.texture.as_raw())
        {
            index
        } else {
            if self.inputs.len() == self.inputs.capacity() {
                return Err(windows::core::Error::from_hresult(E_POINTER));
            }
            let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPIV {
                        MipSlice: 0,
                        ArraySlice: 0,
                    },
                },
            };
            let mut view = None;
            // SAFETY: primeiro encontro de um recurso do pool; view cacheada para a cadência estável.
            unsafe {
                self.device.CreateVideoProcessorInputView(
                    &frame.texture,
                    &self.enumerator,
                    &desc,
                    Some(&mut view),
                )?;
            }
            self.inputs.push((frame.texture.clone(), required(view)?));
            self.inputs.len() - 1
        };
        let rect = even_rect(frame.size, self.output_size);
        let src = RECT {
            left: 0,
            top: 0,
            right: frame.size.0 as i32,
            bottom: frame.size.1 as i32,
        };
        let dst = RECT {
            left: rect.x as i32,
            top: rect.y as i32,
            right: (rect.x + rect.width) as i32,
            bottom: (rect.y + rect.height) as i32,
        };
        let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            pInputSurface: std::mem::ManuallyDrop::new(Some(self.inputs[input_index].1.clone())),
            ..Default::default()
        };
        // SAFETY: views vivas no mesmo device, sem past/future frames; struct manualdrop liberada após chamada mesmo em erro.
        let result = unsafe {
            self.context
                .VideoProcessorSetStreamSourceRect(&self.processor, 0, true, Some(&src));
            self.context
                .VideoProcessorSetStreamDestRect(&self.processor, 0, true, Some(&dst));
            self.context.VideoProcessorBlt(
                &self.processor,
                &self.outputs[index],
                0,
                std::slice::from_ref(&stream),
            )
        };
        // SAFETY: única cópia ManuallyDrop que possui o clone da interface de entrada.
        unsafe {
            std::mem::ManuallyDrop::drop(&mut stream.pInputSurface);
        }
        result
    }
}
fn even_rect(input: (u32, u32), output: (u32, u32)) -> Rect {
    let r = fit_rect(input, output);
    Rect {
        x: r.x & !1,
        y: r.y & !1,
        width: (r.width & !1).max(2),
        height: (r.height & !1).max(2),
    }
}
enum Backend {
    Shader {
        y: ShaderPass,
        uv: ShaderPass,
        targets: Vec<PlaneTargets>,
    },
    VideoProcessor(VideoProcessor),
    Bgra {
        pass: ShaderPass,
        targets: Vec<ID3D11RenderTargetView>,
    },
}
pub struct Converter {
    gpu: GpuDevice,
    textures: Vec<ID3D11Texture2D>,
    backend: Backend,
    fences: Vec<Fence>,
    queries: Vec<GpuQueries>,
    query_pending: Vec<std::cell::Cell<bool>>,
    query_active: Vec<std::cell::Cell<bool>>,
    query_delay: std::time::Duration,
    query_deadlines: Vec<std::cell::Cell<Option<std::time::Instant>>>,
    size: (u32, u32),
    mode: ConvertMode,
}
impl Converter {
    pub fn new(
        gpu: &GpuDevice,
        source_size: (u32, u32),
        config: &crate::EncoderConfig,
        mode: ConvertMode,
        input: InputFormat,
    ) -> Result<Self> {
        Self::new_ring(gpu, source_size, config, mode, input, 4)
    }
    pub fn new_ring(
        gpu: &GpuDevice,
        source_size: (u32, u32),
        config: &crate::EncoderConfig,
        mode: ConvertMode,
        input: InputFormat,
        slots: usize,
    ) -> Result<Self> {
        config.validate()?;
        if !(2..=16).contains(&slots) {
            return Err(EncodeError::InvalidConfig("anel exige 2–16 slots"));
        }
        let size = (config.width, config.height);
        let textures = (0..slots)
            .map(|_| {
                texture(
                    gpu,
                    size,
                    if input == InputFormat::Bgra {
                        DXGI_FORMAT_B8G8R8A8_UNORM
                    } else {
                        DXGI_FORMAT_NV12
                    },
                )
            })
            .collect::<windows::core::Result<Vec<_>>>()?;
        let (backend, effective) = if input == InputFormat::Bgra {
            tracing::warn!(
                "Entrada ARGB/BGRA diagnóstica: NVENC converte internamente usando CUDA; não é o caminho padrão"
            );
            (
                Backend::Bgra {
                    pass: ShaderPass::new(gpu, s!("ps_bgra"))?,
                    targets: textures
                        .iter()
                        .map(|t| bgra_view(gpu, t))
                        .collect::<windows::core::Result<Vec<_>>>()?,
                },
                ConvertMode::Shader,
            )
        } else {
            let planes = if mode == ConvertMode::Shader {
                textures
                    .iter()
                    .map(|t| {
                        Ok(PlaneTargets {
                            y: plane_view(gpu, t, DXGI_FORMAT_R8_UNORM, 0)?,
                            uv: plane_view(gpu, t, DXGI_FORMAT_R8G8_UNORM, 1)?,
                        })
                    })
                    .collect::<windows::core::Result<Vec<_>>>()
            } else {
                Err(windows::core::Error::from_hresult(E_POINTER))
            };
            match planes {
                Ok(targets) => (
                    Backend::Shader {
                        y: ShaderPass::new(gpu, s!("ps_y"))?,
                        uv: ShaderPass::new(gpu, s!("ps_uv"))?,
                        targets,
                    },
                    ConvertMode::Shader,
                ),
                Err(error) => {
                    if mode == ConvertMode::Shader {
                        tracing::warn!(%error,"RTV de plano NV12 indisponível; selecionando VideoProcessor");
                    }
                    (
                        Backend::VideoProcessor(VideoProcessor::new(
                            gpu,
                            source_size,
                            size,
                            config.fps,
                            &textures,
                        )?),
                        ConvertMode::VideoProcessor,
                    )
                }
            }
        };
        Ok(Self {
            gpu: gpu.clone(),
            textures,
            backend,
            fences: (0..slots)
                .map(|_| Fence::new(gpu))
                .collect::<windows::core::Result<Vec<_>>>()?,
            queries: (0..slots)
                .map(|_| GpuQueries::new(gpu))
                .collect::<windows::core::Result<Vec<_>>>()?,
            query_delay: std::time::Duration::ZERO,
            query_deadlines: vec![std::cell::Cell::new(None); slots],
            query_pending: vec![std::cell::Cell::new(false); slots],
            query_active: vec![std::cell::Cell::new(false); slots],
            size,
            mode: effective,
        })
    }
    /// Opt-in supersampled box reduction. The VideoProcessor baseline is explicit.
    pub fn set_box_reduction(&mut self, enabled: bool) -> Result<()> {
        match &mut self.backend {
            Backend::Shader { y, uv, .. } => {
                y.filter_mode = u32::from(enabled);
                uv.filter_mode = if enabled { 3 } else { 0 };
            }
            Backend::Bgra { pass, .. } => pass.filter_mode = u32::from(enabled),
            Backend::VideoProcessor(_) if enabled => {
                return Err(EncodeError::InvalidConfig(
                    "filtro box indisponivel no VideoProcessor",
                ));
            }
            _ => {}
        }
        Ok(())
    }
    pub fn textures(&self) -> &[ID3D11Texture2D] {
        &self.textures
    }
    pub fn effective_mode(&self) -> ConvertMode {
        self.mode
    }
    /// Submete durante o lease; espera da fence deve ocorrer depois de liberar o lease.
    pub fn submit(&mut self, frame: &GpuFrame, index: usize) -> Result<()> {
        if index >= self.textures.len() {
            return Err(EncodeError::InvalidConfig("slot de conversão inválido"));
        }
        if frame.format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(EncodeError::InvalidConfig(
                "somente SDR BGRA8; desligue HDR",
            ));
        }
        let r = even_rect(frame.size, self.size);
        match &mut self.backend {
            Backend::Shader { y, uv, targets } => {
                y.draw(
                    &targets[index].y,
                    Some(&frame.view),
                    r,
                    [16. / 255., 0., 0., 1.],
                    0,
                );
                let half = Rect {
                    x: r.x / 2,
                    y: r.y / 2,
                    width: r.width / 2,
                    height: r.height / 2,
                };
                uv.draw(
                    &targets[index].uv,
                    Some(&frame.view),
                    half,
                    [128. / 255., 128. / 255., 0., 1.],
                    0,
                );
            }
            Backend::Bgra { pass, targets } => {
                pass.draw(&targets[index], Some(&frame.view), r, [0., 0., 0., 1.], 0)
            }
            Backend::VideoProcessor(vp) => {
                if vp.input_size != frame.size {
                    // Resolução mudou: alocação de recursos permitida fora da cadência estável.
                    *vp = VideoProcessor::new(
                        &self.gpu,
                        frame.size,
                        self.size,
                        vp.fps,
                        &self.textures,
                    )?;
                }
                vp.draw(frame, index)?;
            }
        }
        Ok(())
    }
    /// Compatibilidade serial: sinaliza e drena apenas após soltar o lease.
    pub fn finish(&mut self) -> Result<()> {
        self.fences[0].signal()?;
        self.fences[0].wait()?;
        Ok(())
    }
    /// Lab-only deterministic delay of query visibility, never GPU/media work.
    pub fn set_query_delay(&mut self, delay_ms: u64) {
        self.query_delay = std::time::Duration::from_millis(delay_ms);
    }
    fn read_query(&self, slot: usize) -> Result<Option<Option<(f64, f64)>>> {
        if self.query_deadlines[slot]
            .get()
            .is_some_and(|at| std::time::Instant::now() < at)
        {
            return Ok(None);
        }
        self.queries[slot].read(&self.gpu.context)
    }
    pub fn begin_frame(&self, slot: usize) {
        // A pending query owns only its query objects, never the encoded surface.
        // Do not overwrite it when its surface is reused by media.
        let pending = self.query_pending[slot].get();
        let ready = !pending || matches!(self.read_query(slot), Ok(Some(_)));
        self.query_active[slot].set(ready);
        if !ready {
            return;
        }
        self.query_pending[slot].set(true);
        let query = &self.queries[slot];
        // SAFETY: queries exclusivas do slot reservado até a leitura; contexto imediato protegido.
        unsafe {
            self.gpu.context.Begin(&query.disjoint);
            self.gpu.context.End(&query.start);
        }
    }
    pub fn begin_convert(&self, slot: usize) {
        if !self.query_active[slot].get() {
            return;
        }
        // SAFETY: timestamp sem Begin, dentro da janela disjoint deste slot.
        unsafe {
            self.gpu.context.End(&self.queries[slot].middle);
        }
    }
    /// Flush sempre; fence por slot permite tanto polling quanto drenagem final.
    pub fn flush_slot(&mut self, slot: usize) -> Result<()> {
        // SAFETY: termina as queries antes do sinal/Flush, sem esperar GPU.
        if self.query_active[slot].get() {
            unsafe {
                self.gpu.context.End(&self.queries[slot].end);
                self.gpu.context.End(&self.queries[slot].disjoint);
            }
        }
        if self.query_active[slot].get() && !self.query_delay.is_zero() {
            self.query_deadlines[slot].set(Some(std::time::Instant::now() + self.query_delay));
        }
        self.fences[slot].signal()?;
        Ok(())
    }
    pub fn ready(&self, slot: usize) -> Result<bool> {
        self.fences[slot].ready()
    }
    pub fn drain_slot(&mut self, slot: usize) -> Result<()> {
        self.fences[slot].wait()?;
        Ok(())
    }
    /// None externo: ainda indisponível; None interno: amostra disjoint descartada.
    pub fn gpu_times(&self, slot: usize) -> Result<Option<Option<(f64, f64)>>> {
        if !self.query_active[slot].get() {
            return Ok(Some(None));
        }
        let result = self.read_query(slot)?;
        if result.is_some() {
            self.query_pending[slot].set(false);
        }
        Ok(result)
    }
}
struct GpuQueries {
    disjoint: ID3D11Query,
    start: ID3D11Query,
    middle: ID3D11Query,
    end: ID3D11Query,
}
impl GpuQueries {
    fn new(gpu: &GpuDevice) -> windows::core::Result<Self> {
        let make = |kind| {
            let mut query = None;
            // SAFETY: saída COM e descriptor válidos; alocado somente na inicialização.
            unsafe {
                gpu.device.CreateQuery(
                    &D3D11_QUERY_DESC {
                        Query: kind,
                        MiscFlags: 0,
                    },
                    Some(&mut query),
                )?;
            }
            required(query)
        };
        Ok(Self {
            disjoint: make(D3D11_QUERY_TIMESTAMP_DISJOINT)?,
            start: make(D3D11_QUERY_TIMESTAMP)?,
            middle: make(D3D11_QUERY_TIMESTAMP)?,
            end: make(D3D11_QUERY_TIMESTAMP)?,
        })
    }
    fn read(&self, context: &ID3D11DeviceContext) -> Result<Option<Option<(f64, f64)>>> {
        fn get<T>(
            context: &ID3D11DeviceContext,
            query: &ID3D11Query,
            output: &mut T,
        ) -> windows::core::Result<bool> {
            // SAFETY: saída T corresponde ao tipo de query, tamanho exato, interfaces vivas.
            // Vtable preserva S_FALSE, que o wrapper Result<()> apagaria. DONOTFLUSH nunca espera.
            let status = unsafe {
                (Interface::vtable(context).GetData)(
                    context.as_raw(),
                    query.as_raw(),
                    (output as *mut T).cast(),
                    std::mem::size_of::<T>() as u32,
                    D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
                )
            };
            status.ok()?;
            Ok(status == windows::Win32::Foundation::S_OK)
        }
        let mut disjoint = D3D11_QUERY_DATA_TIMESTAMP_DISJOINT::default();
        if !get(context, &self.disjoint, &mut disjoint)? {
            return Ok(None);
        }
        if disjoint.Disjoint.as_bool() || disjoint.Frequency == 0 {
            return Ok(Some(None));
        }
        let (mut start, mut middle, mut end) = (0u64, 0u64, 0u64);
        if !get(context, &self.start, &mut start)?
            || !get(context, &self.middle, &mut middle)?
            || !get(context, &self.end, &mut end)?
        {
            return Ok(None);
        }
        let ms = |a: u64, b: u64| b.saturating_sub(a) as f64 * 1000. / disjoint.Frequency as f64;
        Ok(Some(Some((ms(start, middle), ms(middle, end)))))
    }
}
pub struct SyntheticSource {
    frame: GpuFrame,
    target: ID3D11RenderTargetView,
    pass: ShaderPass,
}
impl SyntheticSource {
    pub fn paused(gpu: &GpuDevice, size: (u32, u32)) -> Result<Self> {
        let mut source = Self::new(gpu, size)?;
        source.pass = ShaderPass::new(gpu, s!("ps_pause"))?;
        source.render(0)?;
        Ok(source)
    }
    pub fn new(gpu: &GpuDevice, size: (u32, u32)) -> Result<Self> {
        let texture = texture(gpu, size, DXGI_FORMAT_B8G8R8A8_UNORM)?;
        let target = bgra_view(gpu, &texture)?;
        let mut view = None;
        // SAFETY: textura BGRA com bind SRV, criada uma vez e mantida na fonte.
        unsafe {
            gpu.device
                .CreateShaderResourceView(&texture, None, Some(&mut view))?;
        }
        Ok(Self {
            frame: GpuFrame {
                texture,
                view: required(view)?,
                qpc_capture: 0,
                qpc_acquired: 0,
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                size,
            },
            target,
            pass: ShaderPass::new(gpu, s!("ps_synthetic"))?,
        })
    }
    /// Laboratory upload from a preallocated, caller-owned BGRA corpus.
    pub fn upload(&mut self, bytes: &[u8]) -> Result<&GpuFrame> {
        let (width, height) = self.frame.size;
        if bytes.len() != width as usize * height as usize * 4 {
            return Err(EncodeError::InvalidConfig(
                "corpus BGRA com tamanho incorreto",
            ));
        }
        // SAFETY: exact byte length/pitch, live texture on this immediate context.
        unsafe {
            self.pass.gpu.context.UpdateSubresource(
                &self.frame.texture,
                0,
                None,
                bytes.as_ptr().cast(),
                width * 4,
                0,
            );
        }
        self.frame()
    }
    pub fn frame(&mut self) -> Result<&GpuFrame> {
        self.frame.qpc_capture = platform::qpc()?;
        self.frame.qpc_acquired = self.frame.qpc_capture;
        Ok(&self.frame)
    }
    pub fn render(&mut self, index: u64) -> Result<&GpuFrame> {
        self.frame.qpc_capture = platform::qpc()?;
        self.frame.qpc_acquired = self.frame.qpc_capture;
        self.pass.draw(
            &self.target,
            None,
            Rect {
                x: 0,
                y: 0,
                width: self.frame.size.0,
                height: self.frame.size.1,
            },
            [0., 0., 0., 1.],
            index as u32,
        );
        Ok(&self.frame)
    }
}
