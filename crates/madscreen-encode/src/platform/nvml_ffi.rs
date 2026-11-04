//! Amostragem fora do caminho de frames. Assinaturas: NVIDIA NVML nvml.h (NVIDIA/go-nvml).
#![allow(unsafe_code)]
use super::dll_ffi::Library;
use std::{
    ffi::{CStr, c_char, c_void},
    process::Command,
    ptr,
};
use windows::core::{s, w};
type Status = i32;
type Device = *mut c_void;
type Init = unsafe extern "C" fn() -> Status;
type Count = unsafe extern "C" fn(*mut u32) -> Status;
type Handle = unsafe extern "C" fn(u32, *mut Device) -> Status;
type Name = unsafe extern "C" fn(Device, *mut c_char, u32) -> Status;
type Driver = unsafe extern "C" fn(*mut c_char, u32) -> Status;
type EncoderUtil = unsafe extern "C" fn(Device, *mut u32, *mut u32) -> Status;
#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: u32,
    memory: u32,
}
type Util = unsafe extern "C" fn(Device, *mut Utilization) -> Status;
type Property = unsafe extern "C" fn(Device, *mut u32) -> Status;
type Clock = unsafe extern "C" fn(Device, u32, *mut u32) -> Status;
#[derive(Clone, Debug, serde::Serialize)]
pub struct GpuSample {
    pub gpu_index: u32,
    pub name: String,
    pub driver: Option<String>,
    pub source: &'static str,
    pub encoder_percent: Option<u32>,
    pub gpu_percent: Option<u32>,
    pub sm_clock_mhz: Option<u32>,
    pub pstate: Option<u32>,
    pub temperature_c: Option<u32>,
}
struct Nvml {
    _library: Library,
    shutdown: Init,
    devices: Vec<Device>,
    name: Name,
    driver: Option<Driver>,
    encoder: EncoderUtil,
    util: Util,
    clock: Option<Clock>,
    pstate: Option<Property>,
    temperature: Option<Clock>,
}
impl Nvml {
    fn new() -> Option<Self> {
        let library = Library::load(w!("nvml.dll")).ok().or_else(|| {
            Library::load(w!("C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvml.dll")).ok()
        })?;
        // SAFETY: assinaturas de nvml.h; DLL retida no objeto, chamadas de inicialização pareadas no Drop.
        unsafe {
            let init: Init = library.symbol(s!("nvmlInit_v2")).ok()?;
            let shutdown: Init = library.symbol(s!("nvmlShutdown")).ok()?;
            let count_fn: Count = library.symbol(s!("nvmlDeviceGetCount_v2")).ok()?;
            let handle_fn: Handle = library.symbol(s!("nvmlDeviceGetHandleByIndex_v2")).ok()?;
            let name: Name = library.symbol(s!("nvmlDeviceGetName")).ok()?;
            let encoder: EncoderUtil =
                library.symbol(s!("nvmlDeviceGetEncoderUtilization")).ok()?;
            let util: Util = library.symbol(s!("nvmlDeviceGetUtilizationRates")).ok()?;
            let driver = library.symbol(s!("nvmlSystemGetDriverVersion")).ok();
            let clock = library.symbol(s!("nvmlDeviceGetClockInfo")).ok();
            let pstate = library.symbol(s!("nvmlDeviceGetPerformanceState")).ok();
            let temperature = library.symbol(s!("nvmlDeviceGetTemperature")).ok();
            if init() != 0 {
                return None;
            }
            let mut n = 0;
            if count_fn(&mut n) != 0 || n > 64 {
                let _ = shutdown();
                return None;
            }
            let mut devices = Vec::with_capacity(n as usize);
            for i in 0..n {
                let mut device = ptr::null_mut();
                if handle_fn(i, &mut device) == 0 && !device.is_null() {
                    devices.push(device);
                } else {
                    let _ = shutdown();
                    return None;
                }
            }
            Some(Self {
                _library: library,
                shutdown,
                devices,
                name,
                driver,
                encoder,
                util,
                clock,
                pstate,
                temperature,
            })
        }
    }
    fn sample(&self) -> Vec<GpuSample> {
        self.devices
            .iter()
            .enumerate()
            .map(|(i, &device)| {
                let mut name = [0i8; 256];
                let mut version = [0i8; 96];
                let (mut enc, mut period, mut clock, mut pstate, mut temp) = (0, 0, 0, 0, 0);
                let mut util = Utilization::default();
                // SAFETY: handles NVML vivos, saídas locais válidas e comprimentos corretos; nenhuma memória de pixels.
                unsafe {
                    let _ = (self.name)(device, name.as_mut_ptr(), name.len() as u32);
                    // Garantir NUL mesmo se o driver retornar dados incompletos.
                    name[255] = 0;
                    let driver = self.driver.and_then(|f| {
                        let status = f(version.as_mut_ptr(), version.len() as u32);
                        version[95] = 0;
                        (status == 0).then(|| {
                            CStr::from_ptr(version.as_ptr())
                                .to_string_lossy()
                                .into_owned()
                        })
                    });
                    GpuSample {
                        gpu_index: i as u32,
                        name: CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned(),
                        driver,
                        source: "nvml",
                        encoder_percent: ((self.encoder)(device, &mut enc, &mut period) == 0)
                            .then_some(enc),
                        gpu_percent: ((self.util)(device, &mut util) == 0).then_some(util.gpu),
                        sm_clock_mhz: self
                            .clock
                            .and_then(|f| (f(device, 1, &mut clock) == 0).then_some(clock)),
                        pstate: self
                            .pstate
                            .and_then(|f| (f(device, &mut pstate) == 0).then_some(pstate)),
                        temperature_c: self
                            .temperature
                            .and_then(|f| (f(device, 0, &mut temp) == 0).then_some(temp)),
                    }
                }
            })
            .collect()
    }
}
impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: pareia nvmlInit_v2 bem-sucedido, antes de descarregar a DLL.
        let _ = unsafe { (self.shutdown)() };
    }
}
pub struct GpuSampler {
    nvml: Option<Nvml>,
}
impl Default for GpuSampler {
    fn default() -> Self {
        Self::new()
    }
}
impl GpuSampler {
    pub fn new() -> Self {
        Self { nvml: Nvml::new() }
    }
    /// Todas as GPUs são identificadas por índice/nome; contadores são globais, não por processo.
    pub fn sample(&self) -> Vec<GpuSample> {
        if let Some(nvml) = &self.nvml {
            return nvml.sample();
        }
        let output=Command::new("nvidia-smi").args(["--query-gpu=index,name,driver_version,utilization.encoder,utilization.gpu,clocks.sm,pstate,temperature.gpu","--format=csv,noheader,nounits"]).output();
        let Ok(output) = output else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let cells: Vec<_> = line.split(',').map(str::trim).collect();
                if cells.len() != 8 {
                    return None;
                }
                Some(GpuSample {
                    gpu_index: cells[0].parse().ok()?,
                    name: cells[1].into(),
                    driver: Some(cells[2].into()),
                    source: "nvidia-smi",
                    encoder_percent: cells[3].parse().ok(),
                    gpu_percent: cells[4].parse().ok(),
                    sm_clock_mhz: cells[5].parse().ok(),
                    pstate: cells[6].trim_start_matches('P').parse().ok(),
                    temperature_c: cells[7].parse().ok(),
                })
            })
            .collect()
    }
}
