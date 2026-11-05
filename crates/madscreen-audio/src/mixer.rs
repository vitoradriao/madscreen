//! Mixagem portatil de varias fontes PCM f32 estereo 48 kHz.
//!
//! Cada fonte escreve num [`Ring`] limitado. A thread de mixagem e dirigida pelo relogio de
//! parede: a cada tick produz exatamente os blocos de 10 ms devidos desde o inicio, somando o que
//! houver nos rings (silencio em underrun). Assim o transporte recebe pacotes continuos mesmo sem
//! fontes e a deriva entre relogios de dispositivos nao acumula.

use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{CHANNELS, FRAMES_PER_BLOCK, SAMPLE_RATE};

/// Amostras (todos os canais) por bloco de 10 ms.
pub const SAMPLES_PER_BLOCK: usize = FRAMES_PER_BLOCK * CHANNELS;
/// Acima disso o ring descarta o audio mais antigo.
const MAX_BUFFER_MS: usize = 200;
/// Atraso maximo tolerado pelo relogio antes de pular blocos (1 s).
const MAX_CATCHUP_BLOCKS: u64 = 100;

/// Opt-in de laboratorio. Ausencia preserva 200 ms e desliga a instrumentacao.
pub fn parse_lab_ring_ms(value: Option<&str>) -> Result<Option<usize>, &'static str> {
    match value {
        None => Ok(None),
        Some(value) if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => value
            .parse::<usize>()
            .ok()
            .filter(|ms| (10..=200).contains(ms))
            .map(Some)
            .ok_or("AV20_RING_MS deve ser inteiro entre 10 e 200"),
        Some(_) => Err("AV20_RING_MS deve ser inteiro entre 10 e 200"),
    }
}

/// Duracao do PCM intercalado, nao tempo de residencia nem latencia de captura.
pub fn samples_duration_ms(samples: u64) -> f64 {
    samples as f64 * 1000.0 / (f64::from(SAMPLE_RATE) * CHANNELS as f64)
}

#[derive(Clone, Copy, Default, Debug)]
pub struct RingStats {
    pub pops: u64,
    pub occupancy_sum_samples: u64,
    pub occupancy_max_samples: u64,
    pub empty_pops: u64,
    pub underrun_pops: u64,
    pub age_observations: u64,
    pub age_sum_us: u64,
    pub age_max_us: u64,
}

struct Stamp {
    samples: usize,
    enqueued: Instant,
}

struct LabDiagnostics {
    stamps: VecDeque<Stamp>,
    stats: RingStats,
    id: u64,
}

struct RingData {
    samples: VecDeque<f32>,
    lab: Option<LabDiagnostics>,
}

impl RingData {
    fn discard(&mut self, count: usize) {
        self.samples.drain(..count);
        self.discard_stamps(count);
    }

    fn discard_stamps(&mut self, mut count: usize) {
        if let Some(lab) = &mut self.lab {
            while count > 0 {
                let Some(front) = lab.stamps.front_mut() else {
                    break;
                };
                let n = count.min(front.samples);
                count -= n;
                front.samples -= n;
                if front.samples == 0 {
                    lab.stamps.pop_front();
                }
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Buffer circular limitado de amostras f32 intercaladas.
pub struct Ring {
    data: Mutex<RingData>,
    lab_enabled: bool,
    max_samples: usize,
    dropped: AtomicU64,
}

impl Ring {
    pub fn new() -> Self {
        Self::with_capacity_ms(MAX_BUFFER_MS)
    }

    pub fn with_capacity_ms(ms: usize) -> Self {
        Self::with_options(ms, false)
    }

    /// Chamar somente com capacidade validada por parse_lab_ring_ms.
    pub fn with_lab_capacity_ms(ms: usize) -> Result<Self, &'static str> {
        if !(10..=200).contains(&ms) {
            return Err("AV20_RING_MS deve ser inteiro entre 10 e 200");
        }
        Ok(Self::with_options(ms, true))
    }

    fn with_options(ms: usize, diagnostics: bool) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let max_samples = ms * SAMPLE_RATE as usize / 1000 * CHANNELS;
        Self {
            lab_enabled: diagnostics,
            data: Mutex::new(RingData {
                samples: VecDeque::with_capacity(max_samples),
                lab: diagnostics.then(|| LabDiagnostics {
                    // No maximo um timestamp por amostra retida; nunca cresce no hot path.
                    stamps: VecDeque::with_capacity(max_samples),
                    stats: RingStats::default(),
                    id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                }),
            }),
            max_samples,
            dropped: AtomicU64::new(0),
        }
    }

    /// Acrescenta amostras; se passar do limite descarta as mais antigas.
    pub fn push(&self, samples: &[f32]) {
        self.push_values(samples.len(), samples.iter().copied(), None);
    }

    fn push_values(&self, count: usize, values: impl Iterator<Item = f32>, at: Option<Instant>) {
        let mut data = lock(&self.data);
        let skip = count.saturating_sub(self.max_samples);
        let retained = count.min(self.max_samples);
        let excess = data
            .samples
            .len()
            .saturating_add(retained)
            .saturating_sub(self.max_samples);
        data.discard(excess);
        let dropped = (excess as u64).saturating_add(skip as u64);
        if dropped > 0 {
            self.dropped.fetch_add(dropped, Ordering::Relaxed);
        }
        // Descartar ANTES de inserir evita realocacao mesmo para pacotes maiores que o ring.
        data.samples.extend(values.skip(skip).take(retained));
        if retained > 0
            && let Some(lab) = &mut data.lab
        {
            lab.stamps.push_back(Stamp {
                samples: retained,
                enqueued: at.unwrap_or_else(Instant::now),
            });
        }
    }

    /// Acrescenta `count` amostras de silencio (pacote marcado como silencioso).
    pub fn push_silence(&self, count: usize) {
        self.push_values(count, std::iter::repeat_n(0.0, count), None);
    }

    /// Preenche `out` com o que houver e completa com silencio. Devolve quantas amostras reais.
    pub fn pop_into(&self, out: &mut [f32]) -> usize {
        self.pop_at(out, None)
    }

    fn pop_at(&self, out: &mut [f32], at: Option<Instant>) -> usize {
        let mut data = lock(&self.data);
        let occupancy = data.samples.len();
        if let Some(lab) = &mut data.lab {
            let stats = &mut lab.stats;
            stats.pops = stats.pops.saturating_add(1);
            stats.occupancy_sum_samples =
                stats.occupancy_sum_samples.saturating_add(occupancy as u64);
            stats.occupancy_max_samples = stats.occupancy_max_samples.max(occupancy as u64);
            stats.empty_pops += u64::from(occupancy == 0);
            stats.underrun_pops += u64::from(occupancy < out.len());
            if let Some(stamp) = lab.stamps.front() {
                let age = at
                    .unwrap_or_else(Instant::now)
                    .saturating_duration_since(stamp.enqueued)
                    .as_micros()
                    .min(u128::from(u64::MAX)) as u64;
                stats.age_observations = stats.age_observations.saturating_add(1);
                stats.age_sum_us = stats.age_sum_us.saturating_add(age);
                stats.age_max_us = stats.age_max_us.max(age);
            }
        }
        let n = out.len().min(occupancy);
        for (slot, value) in out.iter_mut().zip(data.samples.drain(..n)) {
            *slot = value;
        }
        data.discard_stamps(n);
        out[n..].fill(0.0);
        n
    }

    pub fn len(&self) -> usize {
        lock(&self.data).samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Amostras descartadas por excesso desde a criacao.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn lab_stats(&self) -> Option<RingStats> {
        lock(&self.data).lab.as_ref().map(|lab| lab.stats)
    }

    // Fora de mix/pop: um snapshot cumulativo por segundo e um final no Drop.
    fn report_lab_stats(&self, final_snapshot: bool) {
        if !self.lab_enabled {
            return;
        }
        let snapshot = lock(&self.data).lab.as_ref().map(|lab| (lab.id, lab.stats));
        if let Some((id, s)) = snapshot {
            tracing::info!(target: "madscreen_audio::av20",
                av20_schema = 1, ring_id = id, capacity_samples = self.max_samples,
                pops = s.pops, occupancy_sum_samples = s.occupancy_sum_samples,
                occupancy_max_samples = s.occupancy_max_samples, empty_pops = s.empty_pops,
                underrun_pops = s.underrun_pops, age_observations = s.age_observations,
                age_sum_us = s.age_sum_us, age_max_us = s.age_max_us,
                dropped_samples = self.dropped(), final_snapshot = u8::from(final_snapshot),
                "AV20_RING_STATS");
        }
    }
}

impl Drop for Ring {
    fn drop(&mut self) {
        self.report_lab_stats(true);
    }
}

impl Default for Ring {
    fn default() -> Self {
        Self::new()
    }
}

/// Limitador suave: transparente ate 0,8 e comprime o excedente sem passar de +-1.
pub fn soft_limit(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let range = 1.0 - KNEE;
        x.signum() * (KNEE + range * ((a - KNEE) / range).tanh())
    }
}

/// Soma rings em um bloco i16 intercalado, sem alocar por bloco.
pub struct BlockMixer {
    acc: Vec<f32>,
    tmp: Vec<f32>,
    out: Vec<i16>,
}

impl BlockMixer {
    pub fn new() -> Self {
        Self {
            acc: vec![0.0; SAMPLES_PER_BLOCK],
            tmp: vec![0.0; SAMPLES_PER_BLOCK],
            out: vec![0; SAMPLES_PER_BLOCK],
        }
    }

    pub fn mix(&mut self, rings: &[Arc<Ring>]) -> &[i16] {
        self.acc.fill(0.0);
        for ring in rings {
            ring.pop_into(&mut self.tmp);
            for (acc, sample) in self.acc.iter_mut().zip(&self.tmp) {
                *acc += *sample;
            }
        }
        for (out, acc) in self.out.iter_mut().zip(&self.acc) {
            let limited = soft_limit(if acc.is_finite() { *acc } else { 0.0 });
            *out = (limited * 32767.0).round().clamp(-32768.0, 32767.0) as i16;
        }
        &self.out
    }
}

impl Default for BlockMixer {
    fn default() -> Self {
        Self::new()
    }
}

/// Entrega de blocos de 10 ms (960 amostras i16 intercaladas).
pub type Sink = Box<dyn FnMut(&[i16]) + Send>;

/// Barreira de mudo: set só retorna depois do push em andamento. Mantém Opus
/// contínuo com silêncio; nenhuma amostra capturada passa após a confirmação.
#[derive(Clone, Default)]
pub struct MuteGate(Arc<Mutex<bool>>);

impl MuteGate {
    pub fn set(&self, muted: bool) {
        *lock(&self.0) = muted;
    }

    pub fn wrap(&self, mut sink: Sink) -> Sink {
        let gate = self.clone();
        Box::new(move |block| {
            let muted = lock(&gate.0);
            if *muted {
                const SILENCE: [i16; SAMPLES_PER_BLOCK] = [0; SAMPLES_PER_BLOCK];
                sink(&SILENCE);
            } else {
                sink(block);
            }
        })
    }
}

/// Thread de mixagem dirigida pelo relogio de parede.
pub struct MixerThread {
    stop: Arc<AtomicBool>,
    rings: Arc<Mutex<Vec<Arc<Ring>>>>,
    blocks: Arc<AtomicU64>,
    handle: Option<JoinHandle<()>>,
}

impl MixerThread {
    pub fn start(mut sink: Sink) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let rings: Arc<Mutex<Vec<Arc<Ring>>>> = Arc::new(Mutex::new(Vec::new()));
        let blocks = Arc::new(AtomicU64::new(0));
        let handle = {
            let stop = stop.clone();
            let rings = rings.clone();
            let blocks = blocks.clone();
            thread::Builder::new()
                .name("madscreen-audio-mix".into())
                .spawn(move || {
                    let _timer = crate::platform::HighResTimer::begin();
                    crate::platform::raise_thread_priority();
                    let start = Instant::now();
                    let mut produced: u64 = 0;
                    let mut mixer = BlockMixer::new();
                    let mut snapshot: Vec<Arc<Ring>> = Vec::new();
                    let mut next_report_us = 1_000_000_u64;
                    while !stop.load(Ordering::Acquire) {
                        let elapsed_us = start.elapsed().as_micros() as u64;
                        let due = elapsed_us * u64::from(SAMPLE_RATE)
                            / 1_000_000
                            / FRAMES_PER_BLOCK as u64;
                        if due.saturating_sub(produced) > MAX_CATCHUP_BLOCKS {
                            produced = due - 1;
                        }
                        if elapsed_us >= next_report_us {
                            for ring in &snapshot {
                                ring.report_lab_stats(false);
                            }
                            next_report_us = elapsed_us.saturating_add(1_000_000);
                        }
                        if produced < due {
                            snapshot.clear();
                            snapshot.extend(lock(&rings).iter().cloned());
                            while produced < due {
                                sink(mixer.mix(&snapshot));
                                produced += 1;
                                blocks.store(produced, Ordering::Relaxed);
                            }
                        }
                        thread::sleep(Duration::from_millis(3));
                    }
                })?
        };
        Ok(Self {
            stop,
            rings,
            blocks,
            handle: Some(handle),
        })
    }

    pub fn set_rings(&self, rings: Vec<Arc<Ring>>) {
        *lock(&self.rings) = rings;
    }

    /// Blocos de 10 ms entregues ao sink desde o inicio.
    pub fn blocks(&self) -> u64 {
        self.blocks.load(Ordering::Relaxed)
    }
}

impl Drop for MixerThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lab_parse_limits_and_default_are_explicit() {
        assert_eq!(parse_lab_ring_ms(None), Ok(None));
        assert_eq!(parse_lab_ring_ms(Some("20")), Ok(Some(20)));
        for value in ["10", "200"] {
            assert!(parse_lab_ring_ms(Some(value)).is_ok());
        }
        for value in [
            "",
            "0",
            "9",
            "201",
            "20.0",
            " 20",
            "+20",
            "-1",
            "999999999999999999999999",
        ] {
            assert!(parse_lab_ring_ms(Some(value)).is_err(), "{value}");
        }
        let default = Ring::new();
        assert_eq!(default.max_samples, 19200);
        assert!(default.lab_stats().is_none());
        assert!(Ring::with_lab_capacity_ms(0).is_err());
    }

    #[test]
    fn lab_occupancy_and_oldest_residence_are_measured_before_pop() {
        let ring = Ring::with_lab_capacity_ms(20).unwrap();
        let start = Instant::now();
        ring.push_values(960, std::iter::repeat_n(0.1, 960), Some(start));
        ring.push_values(
            960,
            std::iter::repeat_n(0.2, 960),
            Some(start + Duration::from_millis(5)),
        );
        let mut out = [0.0; 960];
        ring.pop_at(&mut out, Some(start + Duration::from_millis(10)));
        assert_eq!(out[0], 0.1);
        ring.pop_at(&mut out, Some(start + Duration::from_millis(15)));
        assert_eq!(out[0], 0.2);
        ring.pop_at(&mut out, Some(start + Duration::from_millis(20)));
        let stats = ring.lab_stats().unwrap();
        assert_eq!(stats.pops, 3);
        assert_eq!(stats.occupancy_sum_samples, 2880);
        assert_eq!(
            samples_duration_ms(stats.occupancy_sum_samples / stats.pops),
            10.0
        );
        assert_eq!(samples_duration_ms(stats.occupancy_max_samples), 20.0);
        assert_eq!(stats.age_observations, 2);
        assert_eq!(stats.age_sum_us, 20000);
        assert_eq!(stats.age_max_us, 10000);
        assert_eq!(stats.empty_pops, 1);
        assert_eq!(stats.underrun_pops, 1);
    }

    #[test]
    fn lab_overflow_partial_pop_and_silence_keep_matching_timestamps() {
        let ring = Ring::with_lab_capacity_ms(20).unwrap();
        let start = Instant::now();
        ring.push_values(960, std::iter::repeat_n(1.0, 960), Some(start));
        ring.push_values(
            960,
            std::iter::repeat_n(2.0, 960),
            Some(start + Duration::from_millis(5)),
        );
        ring.push_values(
            480,
            std::iter::repeat_n(3.0, 480),
            Some(start + Duration::from_millis(10)),
        );
        assert_eq!(ring.dropped(), 480);
        ring.pop_at(&mut [0.0; 480], Some(start + Duration::from_millis(15)));
        ring.pop_at(&mut [0.0; 960], Some(start + Duration::from_millis(20)));
        ring.pop_at(&mut [0.0; 480], Some(start + Duration::from_millis(25)));
        let stats = ring.lab_stats().unwrap();
        assert_eq!(stats.age_sum_us, 45000);
        assert_eq!(stats.age_max_us, 15000);
        ring.push_silence(4800);
        assert_eq!(ring.len(), 1920);
        assert_eq!(ring.dropped(), 480 + 2880);
        assert_eq!(ring.pop_into(&mut [1.0; 1920]), 1920);
        assert_eq!(ring.lab_stats().unwrap().age_observations, 4);
    }

    #[test]
    fn lab_pushes_do_not_grow_preallocated_storage_and_keep_latest_pcm() {
        let ring = Ring::with_lab_capacity_ms(10).unwrap();
        let capacities = {
            let data = lock(&ring.data);
            (
                data.samples.capacity(),
                data.lab.as_ref().unwrap().stamps.capacity(),
            )
        };
        for _ in 0..2000 {
            ring.push(&[1.0]);
        }
        ring.push(&[2.0; 1920]);
        let data = lock(&ring.data);
        assert_eq!(data.samples.capacity(), capacities.0);
        assert_eq!(data.lab.as_ref().unwrap().stamps.capacity(), capacities.1);
        assert_eq!(data.samples.len(), 960);
        assert!(data.samples.iter().all(|v| *v == 2.0));
        assert_eq!(
            data.lab
                .as_ref()
                .unwrap()
                .stamps
                .iter()
                .map(|s| s.samples)
                .sum::<usize>(),
            960
        );
        assert_eq!(ring.dropped(), 2960);
    }

    #[test]
    fn mute_gate_sends_silence_and_restores_only_new_blocks() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let output = received.clone();
        let gate = MuteGate::default();
        let mut sink = gate.wrap(Box::new(move |block| lock(&output).push(block.to_vec())));
        sink(&[123; SAMPLES_PER_BLOCK]);
        gate.set(true);
        sink(&[456; SAMPLES_PER_BLOCK]);
        gate.set(false);
        sink(&[789; SAMPLES_PER_BLOCK]);
        let blocks = lock(&received);
        assert!(blocks[0].iter().all(|v| *v == 123));
        assert!(blocks[1].iter().all(|v| *v == 0));
        assert!(blocks[2].iter().all(|v| *v == 789));
    }

    #[test]
    fn mute_ack_waits_for_the_push_in_flight() -> Result<(), Box<dyn std::error::Error>> {
        let gate = MuteGate::default();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut sink = gate.wrap(Box::new(move |_| {
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
        }));
        let pushing = thread::spawn(move || sink(&[1; SAMPLES_PER_BLOCK]));
        entered_rx.recv_timeout(Duration::from_secs(2))?;
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        let setter = thread::spawn(move || {
            gate.set(true);
            let _ = ack_tx.send(());
        });
        assert!(ack_rx.recv_timeout(Duration::from_millis(25)).is_err());
        release_tx.send(())?;
        ack_rx.recv_timeout(Duration::from_secs(2))?;
        pushing.join().map_err(|_| "push panic")?;
        setter.join().map_err(|_| "setter panic")?;
        Ok(())
    }

    fn ring_with(samples: &[f32]) -> Arc<Ring> {
        let ring = Arc::new(Ring::new());
        ring.push(samples);
        ring
    }

    #[test]
    fn soft_limit_is_transparent_below_knee_and_bounded() {
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.8), -0.8);
        for x in [0.9_f32, 1.0, 2.0, 10.0, 1000.0] {
            let y = soft_limit(x);
            assert!(y > 0.8 && y <= 1.0, "{x} -> {y}");
            assert_eq!(soft_limit(-x), -y);
        }
        assert!(soft_limit(0.9) < soft_limit(1.5));
    }

    #[test]
    fn sums_sources() {
        let a = ring_with(&vec![0.25; SAMPLES_PER_BLOCK]);
        let b = ring_with(&vec![0.125; SAMPLES_PER_BLOCK]);
        let mut mixer = BlockMixer::new();
        let out = mixer.mix(&[a, b]);
        let expected = (0.375_f32 * 32767.0).round() as i16;
        assert!(out.iter().all(|s| *s == expected));
    }

    #[test]
    fn loud_sum_does_not_clip_or_wrap() {
        let a = ring_with(&vec![0.9; SAMPLES_PER_BLOCK]);
        let b = ring_with(&vec![0.9; SAMPLES_PER_BLOCK]);
        let mut mixer = BlockMixer::new();
        let out = mixer.mix(&[a, b]);
        assert!(out.iter().all(|s| *s > 26000));
    }

    #[test]
    fn underrun_is_silence() {
        let a = ring_with(&[0.5; 10]);
        let mut mixer = BlockMixer::new();
        let out = mixer.mix(std::slice::from_ref(&a)).to_vec();
        assert_ne!(out[0], 0);
        assert_ne!(out[9], 0);
        assert!(out[10..].iter().all(|s| *s == 0));
        assert!(a.is_empty());
        assert!(mixer.mix(&[a]).iter().all(|s| *s == 0));
    }

    #[test]
    fn no_sources_is_silence() {
        let mut mixer = BlockMixer::new();
        let out = mixer.mix(&[]);
        assert_eq!(out.len(), SAMPLES_PER_BLOCK);
        assert!(out.iter().all(|s| *s == 0));
    }

    #[test]
    fn ring_drops_oldest_when_full() {
        let ring = Ring::with_capacity_ms(10);
        let cap = SAMPLES_PER_BLOCK;
        ring.push(&vec![1.0; cap]);
        ring.push(&vec![2.0; cap / 2]);
        assert_eq!(ring.len(), cap);
        assert_eq!(ring.dropped(), (cap / 2) as u64);
        let mut out = vec![0.0; cap];
        assert_eq!(ring.pop_into(&mut out), cap);
        assert_eq!(out[0], 1.0);
        assert_eq!(out[cap - 1], 2.0);
    }

    #[test]
    fn non_finite_input_becomes_silence() {
        let a = ring_with(&vec![f32::NAN; SAMPLES_PER_BLOCK]);
        let mut mixer = BlockMixer::new();
        assert!(mixer.mix(&[a]).iter().all(|s| *s == 0));
    }

    #[test]
    fn thread_emits_continuous_10ms_blocks_and_mixes_set_rings() {
        let seen = Arc::new(Mutex::new((0_usize, 0_usize)));
        let sink_seen = seen.clone();
        let mixer = MixerThread::start(Box::new(move |block: &[i16]| {
            assert_eq!(block.len(), SAMPLES_PER_BLOCK);
            let mut seen = lock(&sink_seen);
            seen.0 += 1;
            if block.iter().any(|s| *s != 0) {
                seen.1 += 1;
            }
        }))
        .expect("thread");
        thread::sleep(Duration::from_millis(200));
        let silent_blocks = lock(&seen).1;
        assert_eq!(silent_blocks, 0);
        let ring = Arc::new(Ring::new());
        ring.push(&vec![0.5; SAMPLES_PER_BLOCK * 5]);
        mixer.set_rings(vec![ring.clone()]);
        thread::sleep(Duration::from_millis(300));
        drop(mixer);
        let (total, loud) = *lock(&seen);
        assert!((40..=60).contains(&total), "total {total}");
        assert_eq!(loud, 5);
        assert!(ring.is_empty());
    }

    #[test]
    fn set_rings_can_remove_sources() {
        let mixer = MixerThread::start(Box::new(|_| {})).expect("thread");
        let ring = Arc::new(Ring::new());
        mixer.set_rings(vec![ring]);
        mixer.set_rings(Vec::new());
        thread::sleep(Duration::from_millis(50));
        assert!(mixer.blocks() >= 1);
    }
}
