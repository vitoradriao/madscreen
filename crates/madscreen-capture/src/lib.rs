//! Tipos portáveis e captura oficial WGC/DXGI com mailbox na GPU.
use std::time::Duration;
pub mod display;
#[cfg(any(windows, test))]
mod pointer;

#[cfg(windows)]
pub mod source;
#[cfg(windows)]
pub mod wgc;

/// Escolha explícita para comparação/retorno; o app usa Auto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureBackend {
    #[default]
    Auto,
    Wgc,
    DesktopDuplication,
}

/// Windows 11 também informa major=10; a build 22000 distingue o cliente.
/// Versão desconhecida preserva WGC. Janelas nunca viram recortes de monitor.
/// Monitor com HDR ativo usa WGC: a Desktop Duplication BGRA8 entrega o desktop HDR
/// com brilho ~2x e estourado (medido em 7/nov/2026, #12); o WGC BGRA8 entrega o SDR correto.
pub fn automatic_backend(
    target: CaptureTarget,
    version: Option<(u32, u32)>,
    hdr_enabled: Option<bool>,
) -> CaptureBackend {
    if matches!(target, CaptureTarget::Monitor(_))
        && matches!(version, Some((10, 18362..22000)))
        && hdr_enabled != Some(true)
    {
        CaptureBackend::DesktopDuplication
    } else {
        CaptureBackend::Wgc
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureTarget {
    Monitor(isize),
    Window(isize),
}

#[derive(Clone, Copy, Debug)]
pub struct CaptureOptions {
    pub cursor: bool,
    pub border: bool,
    pub target_fps: Option<u32>,
    pub buffer_count: i32,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            cursor: true,
            border: false,
            target_fps: None,
            buffer_count: 2,
        }
    }
}

impl CaptureOptions {
    pub fn validate(self) -> Result<Self, CaptureError> {
        if !(2..=3).contains(&self.buffer_count) || self.target_fps == Some(0) {
            return Err(CaptureError::InvalidOptions);
        }
        Ok(self)
    }
}

/// Intervalo mínimo (unidades de 100 ns) entre frames de captura do WGC.
///
/// Sem FPS alvo: 1 ms, ou seja, sem limitação prática (o WGC entrega até a cadência
/// de composição do DWM). Com FPS alvo: 90 % do período, porque o intervalo é um
/// limitador e o WGC pode entregar menos que o pedido (ex.: 100 Hz → 50 FPS).
pub fn min_update_interval_100ns(target_fps: Option<u32>) -> i64 {
    const ONE_MS: i64 = 10_000;
    match target_fps {
        None | Some(0) => ONE_MS,
        Some(fps) => (9_000_000_i64 / i64::from(fps)).max(ONE_MS),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("use 2–3 buffers e FPS maior que zero")]
    InvalidOptions,
    #[error("sincronização de captura indisponível")]
    Synchronization,
    #[error("captura não suportada neste dispositivo")]
    Unsupported,
    #[cfg(windows)]
    #[error("não foi possível iniciar a thread de captura: {0}")]
    Worker(std::io::Error),
    #[cfg(windows)]
    #[error(transparent)]
    Windows(#[from] windows::core::Error),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CaptureStats {
    /// Estado consultado fora do caminho por frame; None significa desconhecido.
    pub display: display::DisplayColorInfo,
    pub size: (u32, u32),
    pub capture_fps: f64,
    pub frames: u64,
    pub overwritten: u64,
    pub contention_drops: u64,
    /// `chegada do callback - SystemRelativeTime` em ms: positivo = o relógio do WGC está atrás do QPC.
    pub clock_offset: ClockOffset,
}

impl CaptureStats {
    /// Reiniciar a aquisição não reinicia os contadores da publicação.
    pub fn with_prior_counts(mut self, prior: Self) -> Self {
        self.frames = self.frames.saturating_add(prior.frames);
        self.overwritten = self.overwritten.saturating_add(prior.overwritten);
        self.contention_drops = self.contention_drops.saturating_add(prior.contention_drops);
        self
    }
}

/// Resumo da diferença entre o QPC na chegada do frame e o `SystemRelativeTime` do WGC.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClockOffset {
    pub samples: u64,
    pub min_ms: f64,
    pub mean_ms: f64,
    pub max_ms: f64,
    /// Frames cujo timestamp do WGC está no futuro em relação à chegada (offset negativo).
    pub future_frames: u64,
}

/// O empréstimo do frame impede sobrescrever a textura durante o envio dos comandos GPU.
/// `next_frame` pode retornar antes de `timeout`; nunca espera por um frame novo.
pub trait FrameSource: Send {
    type Frame<'a>
    where
        Self: 'a;
    fn start(&mut self, target: CaptureTarget, options: CaptureOptions)
    -> Result<(), CaptureError>;
    fn next_frame(&mut self, timeout: Duration) -> Result<Option<Self::Frame<'_>>, CaptureError>;
    fn stop(&mut self);
}

/// Contador com armazenamento alocado na criação; nenhuma alocação ao registrar timestamps.
/// Até 4096 amostras na janela; em saturação usa as últimas 4096.
pub struct FpsCounter {
    samples: Box<[Duration]>,
    first: usize,
    len: usize,
    window: Duration,
}

impl FpsCounter {
    pub fn new(window: Duration) -> Self {
        Self {
            samples: vec![Duration::ZERO; 4096].into_boxed_slice(),
            first: 0,
            len: 0,
            window,
        }
    }

    fn prune(&mut self, now: Duration) {
        while self.len > 0 && now.saturating_sub(self.samples[self.first]) > self.window {
            self.first = (self.first + 1) % self.samples.len();
            self.len -= 1;
        }
    }

    pub fn record(&mut self, timestamp: Duration) {
        if self.len > 0 {
            let last = self.samples[(self.first + self.len - 1) % self.samples.len()];
            if timestamp <= last {
                return;
            }
        }
        self.prune(timestamp);
        if self.len == self.samples.len() {
            self.first = (self.first + 1) % self.samples.len();
            self.len -= 1;
        }
        self.samples[(self.first + self.len) % self.samples.len()] = timestamp;
        self.len += 1;
    }

    pub fn fps(&mut self, now: Duration) -> f64 {
        self.prune(now);
        if self.len < 2 {
            return 0.0;
        }
        let last = self.samples[(self.first + self.len - 1) % self.samples.len()];
        let elapsed = last.saturating_sub(self.samples[self.first]).as_secs_f64();
        if elapsed == 0.0 {
            0.0
        } else {
            (self.len - 1) as f64 / elapsed
        }
    }
}

/// WinRT TimeSpan/SystemRelativeTime usa unidades de 100 ns, não ticks QPC brutos.
pub fn system_time_duration(ticks_100ns: i64) -> Option<Duration> {
    u64::try_from(ticks_100ns)
        .ok()
        .map(|ticks| Duration::new(ticks / 10_000_000, ((ticks % 10_000_000) * 100) as u32))
}

pub fn system_time_to_qpc(ticks_100ns: i64, frequency: i64) -> Option<i64> {
    if ticks_100ns < 0 || frequency <= 0 {
        return None;
    }
    i64::try_from(i128::from(ticks_100ns) * i128::from(frequency) / 10_000_000).ok()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub fn fit_rect(src: (u32, u32), dst: (u32, u32)) -> Rect {
    let (sw, sh) = src;
    let (dw, dh) = dst;
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return Rect::default();
    }
    let (width, height) = if u64::from(dw) * u64::from(sh) <= u64::from(dh) * u64::from(sw) {
        (dw, (u64::from(dw) * u64::from(sh) / u64::from(sw)) as u32)
    } else {
        ((u64::from(dh) * u64::from(sw) / u64::from(sh)) as u32, dh)
    };
    Rect {
        x: (dw - width) / 2,
        y: (dh - height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reacquisition_preserves_cumulative_counts_and_current_measurements() {
        use super::CaptureStats;
        let first = CaptureStats {
            frames: 100,
            overwritten: 4,
            contention_drops: 2,
            size: (1920, 1080),
            capture_fps: 0.0,
            ..Default::default()
        };
        let second = CaptureStats {
            frames: 1,
            overwritten: 0,
            contention_drops: 0,
            size: (1280, 720),
            capture_fps: 1.0,
            ..Default::default()
        }
        .with_prior_counts(first);
        assert_eq!(
            (second.frames, second.overwritten, second.contention_drops),
            (101, 4, 2)
        );
        assert_eq!((second.size, second.capture_fps), ((1280, 720), 1.0));
        let third = CaptureStats {
            frames: 2,
            overwritten: 1,
            ..Default::default()
        }
        .with_prior_counts(second);
        assert_eq!(
            (third.frames, third.overwritten, third.contention_drops),
            (103, 5, 2)
        );
        assert_eq!(
            CaptureStats {
                frames: 1,
                ..Default::default()
            }
            .with_prior_counts(CaptureStats {
                frames: u64::MAX,
                ..Default::default()
            })
            .frames,
            u64::MAX
        );
    }
    #[test]
    fn backend_selection_preserves_window_isolation_and_unknown_versions() {
        use super::{CaptureBackend::*, CaptureTarget::*, automatic_backend};
        for build in [18362, 19041, 19044, 19045] {
            assert_eq!(
                automatic_backend(Monitor(1), Some((10, build)), None),
                DesktopDuplication
            );
            assert_eq!(
                automatic_backend(Monitor(1), Some((10, build)), Some(false)),
                DesktopDuplication
            );
            assert_eq!(
                automatic_backend(Monitor(1), Some((10, build)), Some(true)),
                Wgc
            );
            assert_eq!(automatic_backend(Window(1), Some((10, build)), None), Wgc);
        }
        for version in [
            None,
            Some((6, 9600)),
            Some((10, 17763)),
            Some((10, 22000)),
            Some((10, 26100)),
            Some((11, 30000)),
        ] {
            assert_eq!(automatic_backend(Monitor(1), version, None), Wgc);
            assert_eq!(automatic_backend(Monitor(1), version, Some(true)), Wgc);
            assert_eq!(automatic_backend(Window(1), version, None), Wgc);
        }
    }
    #[test]
    fn min_interval_defaults_to_one_millisecond() {
        assert_eq!(super::min_update_interval_100ns(None), 10_000);
        assert_eq!(super::min_update_interval_100ns(Some(0)), 10_000);
        assert_eq!(super::min_update_interval_100ns(Some(1000)), 10_000);
        assert_eq!(super::min_update_interval_100ns(Some(60)), 150_000);
        assert_eq!(super::min_update_interval_100ns(Some(144)), 62_500);
    }

    use super::*;
    #[test]
    fn fps_empty_window_and_idle() {
        let mut fps = FpsCounter::new(Duration::from_secs(1));
        assert_eq!(fps.fps(Duration::ZERO), 0.0);
        fps.record(Duration::ZERO);
        assert_eq!(fps.fps(Duration::ZERO), 0.0);
        for ms in (10..=2000).step_by(10) {
            fps.record(Duration::from_millis(ms));
        }
        assert!((fps.fps(Duration::from_secs(2)) - 100.0).abs() < 0.001);
        assert_eq!(fps.fps(Duration::from_secs(4)), 0.0);
    }
    #[test]
    fn fps_jitter_duplicates_and_bounded_storage() {
        let mut fps = FpsCounter::new(Duration::from_secs(1));
        for ms in [0, 9, 21, 30, 42, 50, 50, 49] {
            fps.record(Duration::from_millis(ms));
        }
        assert!((fps.fps(Duration::from_millis(50)) - 100.0).abs() < 0.001);
        for us in 1..=10000 {
            fps.record(Duration::from_secs(1) + Duration::from_micros(us));
        }
        assert!((fps.fps(Duration::from_millis(1010)) - 1_000_000.0).abs() < 0.001);
    }
    #[test]
    fn letterbox() {
        assert_eq!(
            fit_rect((1920, 1080), (1000, 1000)),
            Rect {
                x: 0,
                y: 219,
                width: 1000,
                height: 562
            }
        );
        assert_eq!(
            fit_rect((1000, 1000), (1920, 1080)),
            Rect {
                x: 420,
                y: 0,
                width: 1080,
                height: 1080
            }
        );
        assert_eq!(
            fit_rect((600, 800), (800, 600)),
            Rect {
                x: 175,
                y: 0,
                width: 450,
                height: 600
            }
        );
        assert_eq!(
            fit_rect((3840, 2160), (1920, 1080)),
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(
            fit_rect((u32::MAX, u32::MAX), (u32::MAX, u32::MAX)).width,
            u32::MAX
        );
        for (src, dst) in [
            ((0, 10), (20, 20)),
            ((10, 0), (20, 20)),
            ((10, 10), (0, 20)),
            ((10, 10), (20, 0)),
        ] {
            assert_eq!(fit_rect(src, dst), Rect::default());
        }
    }
    #[test]
    fn time_conversion() {
        assert_eq!(
            system_time_duration(10_000_001),
            Some(Duration::new(1, 100))
        );
        assert_eq!(system_time_to_qpc(15_000_000, 3_000_000), Some(4_500_000));
        assert_eq!(system_time_to_qpc(-1, 10), None);
        assert_eq!(system_time_to_qpc(1, 0), None);
        assert_eq!(system_time_to_qpc(i64::MAX, i64::MAX), None);
        assert_eq!(system_time_duration(-1), None);
    }
    #[test]
    fn invalid_options() {
        assert!(
            CaptureOptions {
                buffer_count: 1,
                ..CaptureOptions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            CaptureOptions {
                target_fps: Some(0),
                ..CaptureOptions::default()
            }
            .validate()
            .is_err()
        );
    }
}
