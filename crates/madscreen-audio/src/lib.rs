//! Captura de audio por aplicativo (WASAPI process loopback, quando disponível) e mixagem.
//!
//! Fluxo: cada fonte (`AudioSourceSpec`) vira uma captura de loopback que escreve PCM f32
//! estereo 48 kHz num [`mixer::Ring`]; uma thread de mixagem dirigida pelo relogio de parede
//! entrega blocos de 10 ms (480 quadros, i16 intercalado) ao `sink`, que normalmente alimenta o
//! codificador Opus do WebRTC.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

pub mod mixer;
pub mod procs;

mod platform;
#[cfg(windows)]
mod win;

/// Isolated diagnostic capture; never attached to the product mixer or its hot path.
#[cfg(windows)]
pub mod diagnostic {
    pub use crate::win::loopback_ffi::{PacketObservation, PacketProbe, probe_process_packets};
}

use std::sync::Arc;

pub use procs::{AudioApp, ProcInfo, SessionInfo, group_apps, in_tree, root_pid};

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
/// Quadros por bloco de 10 ms.
pub const FRAMES_PER_BLOCK: usize = 480;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("a captura de áudio por aplicativo não está disponível nesta versão do Windows")]
    Unsupported,
    #[error("não foi possível capturar o áudio do processo {pid}: {reason}")]
    Source { pid: u32, reason: String },
    #[error("falha no sistema de áudio: {0}")]
    System(String),
}

/// De onde vem o som.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AudioSourceSpec {
    /// Somente o processo `pid` e sua arvore de filhos.
    ProcessTree { pid: u32 },
    /// Tudo o que o sistema toca, menos o processo `pid` e sua arvore (evita eco do madscreen).
    SystemExcluding { pid: u32 },
}

struct Active {
    spec: AudioSourceSpec,
    ring: Arc<mixer::Ring>,
    _capture: platform::Loopback,
}

pub struct AudioCapture {
    mixer: mixer::MixerThread,
    active: Vec<Active>,
    failed: Vec<(AudioSourceSpec, String)>,
    lab_ring_ms: Option<usize>,
}

impl AudioCapture {
    /// Inicia a mixagem e as fontes. A falha de uma fonte nao derruba as outras: veja
    /// [`AudioCapture::failed_sources`]. O `sink` recebe um bloco de 10 ms por chamada, sempre,
    /// mesmo sem fontes (silencio).
    pub fn start(sources: &[AudioSourceSpec], sink: mixer::Sink) -> Result<Self, AudioError> {
        let value = match std::env::var("AV20_RING_MS") {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err(AudioError::System("AV20_RING_MS invalido".into())),
        };
        let lab_ring_ms =
            mixer::parse_lab_ring_ms(value.as_deref()).map_err(|e| AudioError::System(e.into()))?;
        let mixer = mixer::MixerThread::start(sink)
            .map_err(|e| AudioError::System(format!("thread de mixagem: {e}")))?;
        let mut capture = Self {
            mixer,
            active: Vec::new(),
            failed: Vec::new(),
            lab_ring_ms,
        };
        capture.set_sources(sources);
        Ok(capture)
    }

    /// Troca o conjunto de fontes em execucao: mantem as que continuam, para as removidas e
    /// inicia as novas.
    pub fn set_sources(&mut self, sources: &[AudioSourceSpec]) {
        self.active.retain(|a| sources.contains(&a.spec));
        self.failed.clear();
        for spec in sources {
            if self.active.iter().any(|a| a.spec == *spec) {
                continue;
            }
            let ring = Arc::new(match self.lab_ring_ms {
                Some(ms) => match mixer::Ring::with_lab_capacity_ms(ms) {
                    Ok(ring) => ring,
                    Err(error) => {
                        self.failed.push((*spec, error.into()));
                        continue;
                    }
                },
                None => mixer::Ring::new(),
            });
            match platform::Loopback::start(*spec, ring.clone()) {
                Ok(capture) => {
                    tracing::info!(?spec, "fonte de áudio iniciada");
                    self.active.push(Active {
                        spec: *spec,
                        ring,
                        _capture: capture,
                    });
                }
                Err(error) => {
                    tracing::warn!(?spec, %error, "fonte de áudio indisponível");
                    self.failed.push((*spec, error.to_string()));
                }
            }
        }
        self.mixer
            .set_rings(self.active.iter().map(|a| a.ring.clone()).collect());
    }

    pub fn failed_sources(&self) -> &[(AudioSourceSpec, String)] {
        &self.failed
    }

    pub fn active_sources(&self) -> Vec<AudioSourceSpec> {
        self.active.iter().map(|a| a.spec).collect()
    }

    /// Blocos de 10 ms entregues ao sink desde o inicio.
    pub fn blocks_emitted(&self) -> u64 {
        self.mixer.blocks()
    }

    /// Amostras descartadas por estouro dos buffers das fontes (consumo lento).
    pub fn dropped_samples(&self) -> u64 {
        self.active.iter().map(|a| a.ring.dropped()).sum()
    }
}

/// Aplicativos com sessao de audio nos dispositivos de saida ativos. `exclude_pid` remove esse
/// processo e seus descendentes (o proprio madscreen).
pub fn list_audio_apps(exclude_pid: Option<u32>) -> Result<Vec<AudioApp>, AudioError> {
    platform::list_audio_apps(exclude_pid)
}

/// PID raiz da arvore do aplicativo que contem `pid` (mesmo executavel).
pub fn resolve_root_pid(pid: u32) -> u32 {
    platform::resolve_root_pid(pid)
}

/// PID do processo dono da janela `hwnd`; `None` se a janela nao existe.
pub fn window_process_id(hwnd: isize) -> Option<u32> {
    platform::window_process_id(hwnd)
}
