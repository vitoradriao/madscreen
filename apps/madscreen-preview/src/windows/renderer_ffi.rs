//! Único passe GPU da prévia e interop da swapchain.
#![allow(unsafe_code)]
use madscreen_capture::{
    fit_rect,
    wgc::{GpuDevice, GpuFrame},
};
use windows::{
    Win32::{
        Foundation::{DXGI_STATUS_OCCLUDED, E_POINTER, HWND},
        Graphics::{
            Direct3D::{Fxc::*, *},
            Direct3D11::*,
            Dxgi::{Common::*, *},
        },
    },
    core::{Interface, PCSTR, Result, s},
};

const SHADER: &str = r#"
struct Vertex { float4 position : SV_POSITION; float2 uv : TEXCOORD0; };
Vertex vs_main(uint id : SV_VertexID) {
    Vertex output;
    output.uv = float2((id << 1) & 2, id & 2);
    output.position = float4(output.uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return output;
}
Texture2D image : register(t0);
SamplerState linear_sampler : register(s0);
float4 ps_main(Vertex input) : SV_TARGET { return float4(image.Sample(linear_sampler, input.uv).rgb, 1); }
"#;

fn required<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(|| windows::core::Error::from_hresult(E_POINTER))
}

fn compile(entry: PCSTR, target: PCSTR) -> Result<ID3DBlob> {
    let mut code = None;
    let mut errors = None;
    // SAFETY: HLSL estático e strings terminadas em NUL; buffers de saída válidos.
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
            // SAFETY: o blob mantém seu buffer vivo e informa seu comprimento em bytes.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    errors.GetBufferPointer().cast::<u8>(),
                    errors.GetBufferSize(),
                )
            };
            tracing::error!(message = %String::from_utf8_lossy(bytes), "Falha na compilação HLSL");
        }
        return Err(error);
    }
    required(code)
}

pub struct Renderer {
    gpu: GpuDevice,
    swapchain: IDXGISwapChain1,
    target: Option<ID3D11RenderTargetView>,
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    size: (u32, u32),
}
impl Renderer {
    pub fn new(gpu: GpuDevice, hwnd: HWND, size: (u32, u32)) -> Result<Self> {
        let dxgi: IDXGIDevice = gpu.device.cast()?;
        // SAFETY: consultas em interfaces COM vivas do mesmo device.
        let factory: IDXGIFactory2 = unsafe { dxgi.GetAdapter()?.GetParent()? };
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: size.0.max(1),
            Height: size.1.max(1),
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            ..Default::default()
        };
        // SAFETY: HWND da janela de vídeo vivo; descriptor válido para flip model.
        let swapchain = unsafe {
            let chain = factory.CreateSwapChainForHwnd(&gpu.device, hwnd, &desc, None, None)?;
            factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER)?;
            chain
        };
        // Limitar a fila de apresentação a um frame sem aumentar prioridade.
        // SAFETY: interface DXGI obtida do device vivo.
        unsafe {
            dxgi.cast::<IDXGIDevice1>()?.SetMaximumFrameLatency(1)?;
        }
        let vs = compile(s!("vs_main"), s!("vs_5_0"))?;
        let ps = compile(s!("ps_main"), s!("ps_5_0"))?;
        let mut vertex = None;
        let mut pixel = None;
        let mut sampler = None;
        // SAFETY: os blobs vivem durante CreateShader; os comprimentos vêm dos próprios blobs.
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
        }
        let mut renderer = Self {
            gpu,
            swapchain,
            target: None,
            vertex: required(vertex)?,
            pixel: required(pixel)?,
            sampler: required(sampler)?,
            size,
        };
        renderer.create_target()?;
        Ok(renderer)
    }

    fn create_target(&mut self) -> Result<()> {
        let mut target = None;
        // SAFETY: buffer 0 da swapchain viva e saída local inicializada.
        unsafe {
            let buffer: ID3D11Texture2D = self.swapchain.GetBuffer(0)?;
            self.gpu
                .device
                .CreateRenderTargetView(&buffer, None, Some(&mut target))?;
        }
        self.target = Some(required(target)?);
        Ok(())
    }

    pub fn resize(&mut self, size: (u32, u32)) -> Result<()> {
        self.size = size;
        if size.0 == 0 || size.1 == 0 {
            return Ok(());
        }
        // SAFETY: remove referências do contexto ao backbuffer antes de ResizeBuffers.
        unsafe {
            self.gpu.context.OMSetRenderTargets(None, None);
        }
        self.target = None;
        // SAFETY: nenhum backbuffer retido; mantém formato e flags da swapchain.
        unsafe {
            self.swapchain.ResizeBuffers(
                2,
                size.0,
                size.1,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
        }
        self.create_target()
    }

    /// Chamado com a mailbox emprestada; apenas submete comandos, sem Present.
    pub fn draw(&self, frame: Option<&GpuFrame>) -> bool {
        if self.size.0 == 0 || self.size.1 == 0 {
            return false;
        }
        let Some(target) = &self.target else {
            return false;
        };
        let context = &self.gpu.context;
        // SAFETY: recursos vivos no mesmo device; FrameLease serializa leitura/cópia da mailbox.
        unsafe {
            context.ClearRenderTargetView(target, &[0.025, 0.03, 0.04, 1.0]);
            if let Some(frame) = frame {
                let rect = fit_rect(frame.size, self.size);
                if rect.width > 0 && rect.height > 0 {
                    context.OMSetRenderTargets(Some(&[Some(target.clone())]), None);
                    context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                        TopLeftX: rect.x as f32,
                        TopLeftY: rect.y as f32,
                        Width: rect.width as f32,
                        Height: rect.height as f32,
                        MinDepth: 0.0,
                        MaxDepth: 1.0,
                    }]));
                    context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                    context.VSSetShader(&self.vertex, None);
                    context.PSSetShader(&self.pixel, None);
                    context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
                    context.PSSetShaderResources(0, Some(&[Some(frame.view.clone())]));
                    context.Draw(3, 0);
                    context.PSSetShaderResources(0, Some(&[None]));
                    context.OMSetRenderTargets(None, None);
                }
            }
        }
        true
    }

    pub fn present(&self) -> Result<bool> {
        // SAFETY: janela/swapchain vivas; vsync padrão e sem tearing.
        let result = unsafe { self.swapchain.Present(1, DXGI_PRESENT(0)) };
        if result == DXGI_STATUS_OCCLUDED {
            return Ok(false);
        }
        result.ok()?;
        self.gpu.check().map_err(|error| match error {
            madscreen_capture::CaptureError::Windows(error) => error,
            _ => windows::core::Error::from_hresult(E_POINTER),
        })?;
        Ok(true)
    }
}
