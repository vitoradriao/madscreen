#![allow(unsafe_code)]
//! Captura por processo via `ActivateAudioInterfaceAsync` (process loopback).

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use windows::Win32::System::Com::{IAgileObject, IAgileObject_Impl};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        Media::Audio::{
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
            AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
            ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
            IActivateAudioInterfaceCompletionHandler,
            IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
            PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
        },
        System::{
            Com::{
                BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
                StructuredStorage::{
                    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                },
            },
            Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject},
            Variant::VT_BLOB,
        },
    },
    core::{IUnknown, Interface, Ref, Result as WinResult, implement},
};

use crate::{AudioError, AudioSourceSpec, CHANNELS, SAMPLE_RATE, mixer::Ring};

/// WAVE_FORMAT_IEEE_FLOAT
const FORMAT_FLOAT: u16 = 3;
const ACTIVATION_TIMEOUT_MS: u32 = 5_000;

#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct ActivationDone {
    event: HANDLE,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivationDone_Impl {
    fn ActivateCompleted(
        &self,
        _operation: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> WinResult<()> {
        // SAFETY: o evento e criado e so fechado por quem espera, depois do sinal ou sem fechar
        // quando a espera expira.
        unsafe { SetEvent(self.event) }
    }
}

impl IAgileObject_Impl for ActivationDone_Impl {}

/// Captura em andamento; soltar o valor para a thread e libera o dispositivo.
pub(crate) struct Loopback {
    stop: Arc<AtomicBool>,
    stop_event: HANDLE,
    thread: Option<JoinHandle<()>>,
}

// SAFETY: o HANDLE e um evento do kernel, valido de qualquer thread.
unsafe impl Send for Loopback {}

impl Loopback {
    pub(crate) fn start(spec: AudioSourceSpec, ring: Arc<Ring>) -> Result<Self, AudioError> {
        let (pid, exclude) = match spec {
            AudioSourceSpec::ProcessTree { pid } => (pid, false),
            AudioSourceSpec::SystemExcluding { pid } => (pid, true),
        };
        // SAFETY: evento anonimo de reset manual; fechado em Drop.
        let stop_event = unsafe { CreateEventW(None, true, false, None) }
            .map_err(|e| AudioError::System(e.to_string()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let thread = {
            let stop = stop.clone();
            let stop_raw = stop_event.0 as usize;
            thread::Builder::new()
                .name("madscreen-audio-capture".into())
                .spawn(move || {
                    let stop_event = HANDLE(stop_raw as *mut _);
                    run(pid, exclude, &ring, &stop, stop_event, &ready_tx);
                })
                .map_err(|e| AudioError::System(e.to_string()))?
        };
        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                stop_event,
                thread: Some(thread),
            }),
            Ok(Err(reason)) => {
                let _ = thread.join();
                // SAFETY: a thread terminou e nao usa mais o evento.
                unsafe {
                    let _ = CloseHandle(stop_event);
                }
                Err(AudioError::Source { pid, reason })
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                // SAFETY: sinaliza a thread para sair; o evento so e fechado depois do join.
                unsafe {
                    let _ = SetEvent(stop_event);
                }
                let _ = thread.join();
                // SAFETY: a thread terminou e nao usa mais o evento.
                unsafe {
                    let _ = CloseHandle(stop_event);
                }
                Err(AudioError::Source {
                    pid,
                    reason: "tempo esgotado ao iniciar a captura".into(),
                })
            }
        }
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // SAFETY: o evento segue valido ate o join abaixo; so entao e fechado.
        unsafe {
            let _ = SetEvent(self.stop_event);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // SAFETY: a thread de captura terminou e nao usa mais o evento.
        unsafe {
            let _ = CloseHandle(self.stop_event);
        }
    }
}

struct ComGuard;

impl ComGuard {
    fn new() -> Self {
        // SAFETY: inicializacao COM (MTA) da thread de captura, desfeita em Drop.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: par do CoInitializeEx da thread atual.
        unsafe { CoUninitialize() };
    }
}

fn run(
    pid: u32,
    exclude: bool,
    ring: &Ring,
    stop: &AtomicBool,
    stop_event: HANDLE,
    ready: &mpsc::Sender<Result<(), String>>,
) {
    let _com = ComGuard::new();
    crate::win::system_ffi::raise_thread_priority();
    // SAFETY: todo o uso COM/WASAPI acontece nesta thread, com COM inicializado; os eventos
    // criados aqui so sao fechados depois que o cliente de audio parou.
    unsafe {
        let setup = match open(pid, exclude) {
            Ok(setup) => setup,
            Err(reason) => {
                let _ = ready.send(Err(reason));
                return;
            }
        };
        let _ = ready.send(Ok(()));
        capture_loop(&setup, ring, stop, stop_event);
        let _ = setup.client.Stop();
        let _ = CloseHandle(setup.event);
    }
}

struct Setup {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    event: HANDLE,
}

unsafe fn open(pid: u32, exclude: bool) -> Result<Setup, String> {
    // SAFETY: ver o contrato de `run`; os ponteiros passados a WASAPI vivem ate o retorno.
    unsafe {
        let client = activate(pid, exclude)?;
        let format = WAVEFORMATEX {
            wFormatTag: FORMAT_FLOAT,
            nChannels: CHANNELS as u16,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * CHANNELS as u32 * 4,
            nBlockAlign: CHANNELS as u16 * 4,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                200_000,
                0,
                &format,
                None,
            )
            .map_err(|e| format!("Initialize: {e}"))?;
        let event = CreateEventW(None, false, false, None).map_err(|e| e.to_string())?;
        if let Err(error) = client.SetEventHandle(event) {
            let _ = CloseHandle(event);
            return Err(format!("SetEventHandle: {error}"));
        }
        let capture = match client.GetService::<IAudioCaptureClient>() {
            Ok(capture) => capture,
            Err(error) => {
                let _ = CloseHandle(event);
                return Err(format!("GetService: {error}"));
            }
        };
        if let Err(error) = client.Start() {
            let _ = CloseHandle(event);
            return Err(format!("Start: {error}"));
        }
        Ok(Setup {
            client,
            capture,
            event,
        })
    }
}

unsafe fn activate(pid: u32, exclude: bool) -> Result<IAudioClient, String> {
    // SAFETY: `params` e `prop` vivem ate o fim da chamada; o PROPVARIANT e um BLOB que aponta
    // para `params` e nunca e limpo com PropVariantClear (nao e dono da memoria).
    unsafe {
        let event = CreateEventW(None, true, false, None).map_err(|e| e.to_string())?;
        let handler: IActivateAudioInterfaceCompletionHandler = ActivationDone { event }.into();
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: if exclude {
                        PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE
                    } else {
                        PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE
                    },
                },
            },
        };
        // PROPVARIANT tem Drop que chama PropVariantClear: o blob aponta para a pilha, entao
        // a limpeza nunca pode rodar.
        let prop = core::mem::ManuallyDrop::new(PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: core::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 {
                        blob: BLOB {
                            cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                            pBlobData: (&raw mut params).cast(),
                        },
                    },
                }),
            },
        });
        let operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*prop),
            &handler,
        )
        .map_err(|e| format!("ActivateAudioInterfaceAsync: {e}"))?;
        if WaitForSingleObject(event, ACTIVATION_TIMEOUT_MS) != WAIT_OBJECT_0 {
            // O handler ainda pode disparar depois: deixa o evento aberto (vazamento pontual).
            return Err("tempo esgotado ativando o dispositivo de loopback".into());
        }
        let _ = CloseHandle(event);
        let mut result = windows::core::HRESULT(0);
        let mut unknown: Option<IUnknown> = None;
        operation
            .GetActivateResult(&mut result, &mut unknown)
            .map_err(|e| format!("GetActivateResult: {e}"))?;
        result.ok().map_err(|e| format!("ativação recusada: {e}"))?;
        unknown
            .ok_or_else(|| "ativação sem interface".to_owned())?
            .cast::<IAudioClient>()
            .map_err(|e| e.to_string())
    }
}

unsafe fn capture_loop(setup: &Setup, ring: &Ring, stop: &AtomicBool, stop_event: HANDLE) {
    let handles = [setup.event, stop_event];
    while !stop.load(Ordering::Acquire) {
        // SAFETY: os dois handles seguem validos durante o laco (fechados so depois dele).
        let waited = unsafe { WaitForMultipleObjects(&handles, false, 200) };
        if waited.0 == WAIT_OBJECT_0.0 + 1 {
            break;
        }
        // Timeout ou evento de dados: esvazia todos os pacotes disponiveis.
        loop {
            // SAFETY: GetBuffer devolve um ponteiro para `frames` quadros f32 estereo, valido
            // ate ReleaseBuffer; so lemos dentro desse intervalo e liberamos em seguida.
            let drained = unsafe {
                let Ok(packet) = setup.capture.GetNextPacketSize() else {
                    break;
                };
                if packet == 0 {
                    break;
                }
                let mut data = core::ptr::null_mut();
                let mut frames = 0_u32;
                let mut flags = 0_u32;
                if setup
                    .capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                    .is_err()
                {
                    break;
                }
                let samples = frames as usize * CHANNELS;
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    ring.push_silence(samples);
                } else {
                    ring.push(core::slice::from_raw_parts(data.cast::<f32>(), samples));
                }
                let _ = setup.capture.ReleaseBuffer(frames);
                true
            };
            if !drained {
                break;
            }
        }
    }
}

/// One packet from a separate laboratory capture. QPC is already converted by
/// WASAPI to 100 ns units, not raw QueryPerformanceCounter ticks.
#[derive(serde::Serialize)]
pub struct PacketObservation {
    pub qpc_100ns: u64,
    pub device_position: u64,
    pub frames: u32,
    pub flags: u32,
    pub null_data: bool,
    pub first_signal_frame: Option<u32>,
    pub peak: f32,
}

#[derive(serde::Serialize)]
pub struct PacketProbe {
    pub packets: Vec<PacketObservation>,
    pub capacity_drops: u64,
    pub empty_polls: u64,
    pub get_buffer_errors: u64,
    pub release_errors: u64,
}

/// Capture only an explicitly supplied generated-source process tree. This
/// synchronous diagnostic uses its own client and does not touch Ring/Mixer.
/// Allocation is bounded and happens before capture; no per-packet I/O.
pub fn probe_process_packets(pid: u32, duration: Duration) -> Result<PacketProbe, AudioError> {
    if pid == 0 || duration.is_zero() || duration > Duration::from_secs(600) {
        return Err(AudioError::System("invalid diagnostic pid/duration".into()));
    }
    let capacity = (duration.as_millis() as usize).saturating_add(1024);
    let mut report = PacketProbe {
        packets: Vec::with_capacity(capacity),
        capacity_drops: 0,
        empty_polls: 0,
        get_buffer_errors: 0,
        release_errors: 0,
    };
    let _com = ComGuard::new();
    // SAFETY: this diagnostic owns the COM client/event and uses each packet
    // pointer only until ReleaseBuffer. It never shares interfaces across threads.
    unsafe {
        let setup = open(pid, false).map_err(|reason| AudioError::Source { pid, reason })?;
        let started = std::time::Instant::now();
        while started.elapsed() < duration {
            let _ = WaitForSingleObject(setup.event, 100);
            loop {
                let available = match setup.capture.GetNextPacketSize() {
                    Ok(value) => value,
                    Err(_) => {
                        report.get_buffer_errors += 1;
                        break;
                    }
                };
                if available == 0 {
                    report.empty_polls += 1;
                    break;
                }
                let mut data = core::ptr::null_mut();
                let (mut frames, mut flags, mut device_position, mut qpc_100ns) = (0, 0, 0, 0);
                if setup
                    .capture
                    .GetBuffer(
                        &mut data,
                        &mut frames,
                        &mut flags,
                        Some(&mut device_position),
                        Some(&mut qpc_100ns),
                    )
                    .is_err()
                {
                    report.get_buffer_errors += 1;
                    break;
                }
                let mut row = PacketObservation {
                    qpc_100ns,
                    device_position,
                    frames,
                    flags,
                    null_data: data.is_null(),
                    first_signal_frame: None,
                    peak: 0.0,
                };
                if !data.is_null() && flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 == 0 {
                    let samples =
                        core::slice::from_raw_parts(data.cast::<f32>(), frames as usize * CHANNELS);
                    for (frame, channels) in samples.chunks_exact(CHANNELS).enumerate() {
                        let peak = channels.iter().fold(0.0_f32, |a, b| a.max(b.abs()));
                        row.peak = row.peak.max(peak);
                        if peak >= 0.015 && row.first_signal_frame.is_none() {
                            row.first_signal_frame = Some(frame as u32);
                        }
                    }
                }
                if setup.capture.ReleaseBuffer(frames).is_err() {
                    report.release_errors += 1;
                }
                if report.packets.len() < capacity {
                    report.packets.push(row);
                } else {
                    report.capacity_drops += 1;
                }
                if started.elapsed() >= duration {
                    break;
                }
            }
        }
        let _ = setup.client.Stop();
        let _ = CloseHandle(setup.event);
    }
    Ok(report)
}
