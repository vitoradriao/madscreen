//! CPU do próprio processo (GetProcessTimes), para o diagnóstico da publicação.
#![allow(unsafe_code)]
use std::time::Instant;
use windows::Win32::{
    Foundation::FILETIME,
    System::Threading::{GetCurrentProcess, GetProcessTimes},
};

/// Tempo de CPU (kernel + usuário) do processo, em unidades de 100 ns.
fn process_cpu_100ns() -> Option<u64> {
    let mut created = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: pseudo-handle do próprio processo (não precisa fechar) e saídas locais válidas.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    }
    .ok()?;
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Some(ticks(kernel).saturating_add(ticks(user)))
}

/// Uso médio entre duas leituras, em % da máquina inteira (todos os núcleos lógicos).
pub struct ProcessCpu {
    last: Option<(Instant, u64)>,
    logical_cpus: f64,
}

impl ProcessCpu {
    pub fn new() -> Self {
        Self {
            last: process_cpu_100ns().map(|cpu| (Instant::now(), cpu)),
            logical_cpus: std::thread::available_parallelism().map_or(1., |n| n.get() as f64),
        }
    }

    pub fn sample(&mut self) -> Option<f64> {
        let now = Instant::now();
        let cpu = process_cpu_100ns()?;
        let previous = self.last.replace((now, cpu));
        let (at, before) = previous?;
        let wall_s = now.duration_since(at).as_secs_f64();
        (wall_s > 0.)
            .then(|| cpu.saturating_sub(before) as f64 / 1e7 / wall_s / self.logical_cpus * 100.)
    }
}
