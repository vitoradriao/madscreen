//! Interop D3D11/WinRT; nenhum acesso à memória de pixels pela CPU.
#![allow(unsafe_code)]
use super::{GpuDevice, GpuFrame};
use crate::CaptureTarget;
use windows::{
    Graphics::{
        Capture::GraphicsCaptureItem,
        DirectX::Direct3D11::{IDirect3DDevice, IDirect3DSurface},
    },
    Win32::{
        Foundation::{E_FAIL, E_POINTER, HMODULE, HWND},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN},
            Direct3D11::*,
            Dxgi::{Common::*, *},
            Gdi::{HMONITOR, MONITOR_DEFAULTTONEAREST, MonitorFromWindow},
        },
        System::{
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            WinRT::{Direct3D11::*, Graphics::Capture::IGraphicsCaptureItemInterop},
        },
    },
    core::{AgileReference, Interface, Result, factory},
};

fn adapter_for_monitor(monitor: HMONITOR) -> Result<IDXGIAdapter1> {
    // SAFETY: enumeração DXGI com índices verificados pela API; estruturas retornadas por valor.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut index = 0;
        loop {
            let adapter = factory.EnumAdapters1(index)?;
            let mut output_index = 0;
            while let Ok(output) = adapter.EnumOutputs(output_index) {
                if output.GetDesc()?.Monitor == monitor {
                    return Ok(adapter);
                }
                output_index += 1;
            }
            index += 1;
        }
    }
}

pub(crate) fn create_device(target: Option<CaptureTarget>) -> Result<GpuDevice> {
    // SAFETY: handles opacos são usados apenas pelas APIs de validação do Windows.
    let monitor = unsafe {
        match target {
            Some(CaptureTarget::Monitor(id)) => Some(HMONITOR(id as *mut _)),
            Some(CaptureTarget::Window(id)) => Some(MonitorFromWindow(
                HWND(id as *mut _),
                MONITOR_DEFAULTTONEAREST,
            )),
            None => None,
        }
    };
    let adapter = monitor.and_then(|monitor| match adapter_for_monitor(monitor) {
        Ok(adapter) => Some(adapter),
        Err(error) => {
            tracing::warn!(%error, "Adapter do alvo indisponível; usando hardware padrão");
            None
        }
    });
    if target.is_none() {
        tracing::info!(
            "Seletor não expõe o adapter do item; usando hardware padrão (validar em PCs com múltiplas GPUs)"
        );
    }
    let adapter: Option<IDXGIAdapter> = adapter.map(|adapter| adapter.cast()).transpose()?;
    let mut device = None;
    let mut context = None;
    // SAFETY: saídas válidas e inicializadas; nenhuma memória de aplicação é exposta à GPU.
    unsafe {
        D3D11CreateDevice(
            adapter.as_ref(),
            if adapter.is_some() {
                D3D_DRIVER_TYPE_UNKNOWN
            } else {
                D3D_DRIVER_TYPE_HARDWARE
            },
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device = device.ok_or_else(|| windows::core::Error::from_hresult(E_POINTER))?;
    let context = context.ok_or_else(|| windows::core::Error::from_hresult(E_POINTER))?;
    let multithread: ID3D11Multithread = context.cast()?;
    let dxgi: IDXGIDevice = device.cast()?;
    // SAFETY: interfaces COM vivas; proteção habilitada antes de compartilhar o contexto.
    let winrt: IDirect3DDevice = unsafe {
        let _ = multithread.SetMultithreadProtected(true);
        CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?
    };
    let mut frequency = 0;
    // SAFETY: ponteiro para inteiro local válido.
    unsafe {
        QueryPerformanceFrequency(&mut frequency)?;
    }
    Ok(GpuDevice {
        device,
        context,
        winrt: AgileReference::new(&winrt)?,
        frequency,
    })
}

pub(crate) fn item_for_target(target: CaptureTarget) -> Result<GraphicsCaptureItem> {
    let interop: IGraphicsCaptureItemInterop =
        factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    // SAFETY: handles não são dereferenciados em Rust; a API valida o alvo.
    unsafe {
        match target {
            CaptureTarget::Monitor(id) => interop.CreateForMonitor(HMONITOR(id as *mut _)),
            CaptureTarget::Window(id) => interop.CreateForWindow(HWND(id as *mut _)),
        }
    }
}

pub(crate) fn surface_texture(surface: &IDirect3DSurface) -> Result<ID3D11Texture2D> {
    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
    // SAFETY: interface obtida da superfície WGC viva; chamada solicita o IID de textura D3D11.
    unsafe { access.GetInterface() }
}

pub(crate) fn texture_size(texture: &ID3D11Texture2D) -> (u32, u32) {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: saída aponta para descriptor local inicializado.
    unsafe {
        texture.GetDesc(&mut desc);
    }
    (desc.Width, desc.Height)
}

pub(crate) fn create_frame(gpu: &GpuDevice, size: (u32, u32)) -> Result<GpuFrame> {
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
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    let mut view = None;
    // SAFETY: descriptor válido para recurso GPU sem acesso CPU; saídas locais válidas.
    unsafe {
        gpu.device
            .CreateTexture2D(&desc, None, Some(&mut texture))?;
    }
    let texture = texture.ok_or_else(|| windows::core::Error::from_hresult(E_POINTER))?;
    // SAFETY: recurso vivo com bind SRV; descriptor padrão herda o formato da textura.
    unsafe {
        gpu.device
            .CreateShaderResourceView(&texture, None, Some(&mut view))?;
    }
    let view = view.ok_or_else(|| windows::core::Error::from_hresult(E_POINTER))?;
    Ok(GpuFrame {
        texture,
        view,
        qpc_capture: 0,
        qpc_acquired: 0,
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        size,
    })
}

pub(crate) fn copy(gpu: &GpuDevice, dst: &GpuFrame, src: &ID3D11Texture2D) {
    // SAFETY: texturas no mesmo device com descritores idênticos; mailbox serializa cópia/desenho.
    unsafe {
        gpu.context.CopyResource(&dst.texture, src);
    }
}

pub(crate) fn now(frequency: i64) -> Result<std::time::Duration> {
    let mut value = 0;
    // SAFETY: saída aponta para inteiro local válido.
    unsafe {
        QueryPerformanceCounter(&mut value)?;
    }
    if value < 0 || frequency <= 0 {
        return Err(windows::core::Error::from_hresult(E_FAIL));
    }
    let seconds = (value / frequency) as u64;
    let nanos = ((value % frequency) as u128 * 1_000_000_000 / frequency as u128) as u32;
    Ok(std::time::Duration::new(seconds, nanos))
}

pub(crate) fn check_device(gpu: &GpuDevice) -> Result<()> {
    // SAFETY: device COM válido; consulta não acessa memória da aplicação.
    unsafe { gpu.device.GetDeviceRemovedReason() }
}

pub(crate) fn windows_version() -> Option<(u32, u32)> {
    use windows::{
        Wdk::System::SystemServices::RtlGetVersion,
        Win32::System::SystemInformation::OSVERSIONINFOW,
    };
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: estrutura local com tamanho correto; RtlGetVersion não depende do manifesto do app.
    let status = unsafe { RtlGetVersion(&mut version) };
    status
        .is_ok()
        .then_some((version.dwMajorVersion, version.dwBuildNumber))
}
