//! Contrato da pausa, integrado ao protocolo v4.
//!
//! O supervisor deve serializar comandos e confirmações. Uma confirmação de pausa
//! só pode ser produzida depois de bloquear a fonte, invalidar todas as filas e
//! disponibilizar um quadro neutro para receptores atuais e novos.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    /// Identidade única da publicação, nunca reutilizada após reiniciar o sidecar.
    pub generation: u64,
    /// Sequência estritamente crescente dentro da geração.
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoState {
    Active,
    Pausing,
    Paused,
    Resuming,
    /// Falha mantém o bloqueio; exige nova ação explícita ou encerramento.
    Failed,
    Stopped,
}

/// Comando v4; o supervisor só o envia após Ready v4 e Started.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Request {
    Pause { operation: Operation },
    Resume { operation: Operation },
    SetAudioMuted { operation: Operation, muted: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PrivacyError {
    #[error("geração de publicação inválida")]
    Generation,
    #[error("operação antiga ou repetida")]
    Stale,
    #[error("transmissão encerrada")]
    Stopped,
    #[error("transição de privacidade inválida")]
    Transition,
    #[error("confirmação não corresponde à operação em andamento")]
    Confirmation,
    #[error("barreira de privacidade incompleta")]
    IncompleteBarrier,
}

/// Evidência local que o engine deve preencher após executar as barreiras.
/// Não prova que uma imagem já recebida tenha sido apagada remotamente.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PauseBarrier {
    pub source_blocked: bool,
    pub pending_frames_discarded: bool,
    pub neutral_frame_ready: bool,
}

/// Máquina pura usada pelo runner; os efeitos de mídia pertencem ao integrador.
/// O consumidor aplica os efeitos e só então confirma a operação correspondente.
#[derive(Debug)]
pub struct PrivacySession {
    generation: u64,
    last_sequence: u64,
    video_operation: Option<Operation>,
    video: VideoState,
    desired_audio_muted: bool,
    source_epoch: u64,
}

impl PrivacySession {
    /// Reinício/reconexão com proteção anterior começa bloqueado. Nunca inferir
    /// consentimento de retomada a partir do estado da conexão.
    pub fn new(generation: u64, protected: bool, audio_muted: bool) -> Self {
        Self {
            generation,
            last_sequence: 0,
            video_operation: None,
            video: if protected {
                VideoState::Failed
            } else {
                VideoState::Active
            },
            desired_audio_muted: audio_muted,
            source_epoch: 0,
        }
    }

    pub fn video(&self) -> VideoState {
        self.video
    }

    /// Estado desejado; não anunciar áudio silenciado antes do ack do mixer.
    pub fn desired_audio_muted(&self) -> bool {
        self.desired_audio_muted
    }

    pub fn source_epoch(&self) -> u64 {
        self.source_epoch
    }

    /// Cada quadro deve carregar a época de admissão até o ponto final de envio.
    /// Quadros de antes de pausar não voltam a ser aceitos após retomar.
    pub fn allows_source_frame(&self, epoch: u64) -> bool {
        self.video == VideoState::Active && epoch == self.source_epoch
    }

    pub fn request(&mut self, request: Request) -> Result<(), PrivacyError> {
        let operation = match request {
            Request::Pause { operation }
            | Request::Resume { operation }
            | Request::SetAudioMuted { operation, .. } => operation,
        };
        self.validate(operation)?;
        match request {
            Request::Pause { .. } => {
                // Uma pausa pode cancelar inclusive uma retomada pendente.
                self.source_epoch = self
                    .source_epoch
                    .checked_add(1)
                    .ok_or(PrivacyError::Transition)?;
                self.video = VideoState::Pausing;
                self.video_operation = Some(operation);
            }
            Request::Resume { .. } => {
                if !matches!(self.video, VideoState::Paused | VideoState::Failed) {
                    return Err(PrivacyError::Transition);
                }
                // Inclui retomadas após falha: nenhum quadro de uma tentativa
                // anterior pode atravessar a barreira desta retomada.
                self.source_epoch = self
                    .source_epoch
                    .checked_add(1)
                    .ok_or(PrivacyError::Transition)?;
                self.video = VideoState::Resuming;
                self.video_operation = Some(operation);
            }
            Request::SetAudioMuted { muted, .. } => self.desired_audio_muted = muted,
        }
        self.last_sequence = operation.sequence;
        Ok(())
    }

    pub fn confirm_pause(
        &mut self,
        operation: Operation,
        barrier: PauseBarrier,
    ) -> Result<(), PrivacyError> {
        self.confirm(operation, VideoState::Pausing)?;
        if !(barrier.source_blocked
            && barrier.pending_frames_discarded
            && barrier.neutral_frame_ready)
        {
            self.video = VideoState::Failed;
            self.video_operation = None;
            return Err(PrivacyError::IncompleteBarrier);
        }
        self.video = VideoState::Paused;
        self.video_operation = None;
        Ok(())
    }

    /// Confirmação só após um quadro novo decodificável pronto e invalidação
    /// dos quadros anteriores; a integração deve manter neutro até esse ponto.
    pub fn confirm_resume(
        &mut self,
        operation: Operation,
        fresh_keyframe_ready: bool,
    ) -> Result<(), PrivacyError> {
        self.confirm(operation, VideoState::Resuming)?;
        self.video_operation = None;
        if !fresh_keyframe_ready {
            self.video = VideoState::Failed;
            return Err(PrivacyError::IncompleteBarrier);
        }
        self.video = VideoState::Active;
        Ok(())
    }

    /// Falha de IPC/sinalização/reconfiguração preserva a proteção.
    pub fn fail(&mut self) {
        if self.video != VideoState::Stopped {
            self.video = VideoState::Failed;
            self.video_operation = None;
        }
    }

    /// Stop/logout/revogação são terminais, independente da sequência pendente.
    pub fn stop(&mut self) {
        self.video = VideoState::Stopped;
        self.video_operation = None;
        self.desired_audio_muted = true;
    }

    fn validate(&self, operation: Operation) -> Result<(), PrivacyError> {
        if self.video == VideoState::Stopped {
            return Err(PrivacyError::Stopped);
        }
        if operation.generation != self.generation {
            return Err(PrivacyError::Generation);
        }
        if operation.sequence <= self.last_sequence {
            return Err(PrivacyError::Stale);
        }
        Ok(())
    }

    fn confirm(&self, operation: Operation, state: VideoState) -> Result<(), PrivacyError> {
        if self.video != state || self.video_operation != Some(operation) {
            return Err(PrivacyError::Confirmation);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(sequence: u64) -> Operation {
        Operation {
            generation: 17,
            sequence,
        }
    }

    fn barrier() -> PauseBarrier {
        PauseBarrier {
            source_blocked: true,
            pending_frames_discarded: true,
            neutral_frame_ready: true,
        }
    }

    #[test]
    fn pause_invalidates_old_frames_even_after_resume() -> Result<(), PrivacyError> {
        let mut session = PrivacySession::new(17, false, false);
        let old = session.source_epoch();
        assert!(session.allows_source_frame(old));
        session.request(Request::Pause { operation: op(1) })?;
        assert!(!session.allows_source_frame(old));
        session.confirm_pause(op(1), barrier())?;
        assert_eq!(session.video(), VideoState::Paused);
        assert!(!session.desired_audio_muted());
        session.request(Request::Resume { operation: op(2) })?;
        assert!(!session.allows_source_frame(session.source_epoch()));
        session.confirm_resume(op(2), true)?;
        assert!(session.allows_source_frame(session.source_epoch()));
        assert!(!session.allows_source_frame(old));
        Ok(())
    }

    #[test]
    fn every_pause_barrier_is_required() -> Result<(), PrivacyError> {
        for mask in 0..7 {
            let mut session = PrivacySession::new(17, false, true);
            session.request(Request::Pause { operation: op(1) })?;
            assert_eq!(
                session.confirm_pause(
                    op(1),
                    PauseBarrier {
                        source_blocked: mask & 1 != 0,
                        pending_frames_discarded: mask & 2 != 0,
                        neutral_frame_ready: mask & 4 != 0,
                    }
                ),
                Err(PrivacyError::IncompleteBarrier)
            );
            assert_eq!(session.video(), VideoState::Failed);
            assert!(!session.allows_source_frame(session.source_epoch()));
        }
        Ok(())
    }

    #[test]
    fn later_pause_cancels_resume_and_late_ack() -> Result<(), PrivacyError> {
        let mut session = PrivacySession::new(17, true, false);
        session.request(Request::Resume { operation: op(1) })?;
        session.request(Request::Pause { operation: op(2) })?;
        assert_eq!(
            session.confirm_resume(op(1), true),
            Err(PrivacyError::Confirmation)
        );
        session.confirm_pause(op(2), barrier())?;
        assert_eq!(session.video(), VideoState::Paused);
        Ok(())
    }

    #[test]
    fn audio_change_does_not_invalidate_video_ack() -> Result<(), PrivacyError> {
        let mut session = PrivacySession::new(17, false, false);
        session.request(Request::Pause { operation: op(1) })?;
        session.request(Request::SetAudioMuted {
            operation: op(2),
            muted: true,
        })?;
        session.confirm_pause(op(1), barrier())?;
        assert!(session.desired_audio_muted());
        session.request(Request::SetAudioMuted {
            operation: op(3),
            muted: false,
        })?;
        assert_eq!(session.video(), VideoState::Paused);
        Ok(())
    }

    #[test]
    fn stale_and_other_generation_requests_cannot_resume() -> Result<(), PrivacyError> {
        let mut session = PrivacySession::new(17, false, false);
        session.request(Request::Pause { operation: op(2) })?;
        session.confirm_pause(op(2), barrier())?;
        for sequence in [0, 1, 2] {
            assert_eq!(
                session.request(Request::Resume {
                    operation: op(sequence)
                }),
                Err(PrivacyError::Stale)
            );
        }
        assert_eq!(
            session.request(Request::Resume {
                operation: Operation {
                    generation: 16,
                    sequence: 99
                },
            }),
            Err(PrivacyError::Generation)
        );
        assert_eq!(session.video(), VideoState::Paused);
        Ok(())
    }

    #[test]
    fn failure_and_restart_never_resume_implicitly() -> Result<(), PrivacyError> {
        let mut session = PrivacySession::new(17, true, true);
        assert!(!session.allows_source_frame(0));
        session.request(Request::Resume { operation: op(1) })?;
        assert_eq!(
            session.confirm_resume(op(1), false),
            Err(PrivacyError::IncompleteBarrier)
        );
        session.request(Request::Resume { operation: op(2) })?;
        session.fail();
        assert_eq!(
            session.confirm_resume(op(2), true),
            Err(PrivacyError::Confirmation)
        );
        assert!(!session.allows_source_frame(0));
        let previous_epoch = session.source_epoch();
        session.request(Request::Resume { operation: op(3) })?;
        session.confirm_resume(op(3), true)?;
        assert!(!session.allows_source_frame(previous_epoch));
        Ok(())
    }

    #[test]
    fn stop_dominates_every_state_and_pending_confirmation() -> Result<(), PrivacyError> {
        for state in [
            VideoState::Active,
            VideoState::Pausing,
            VideoState::Paused,
            VideoState::Resuming,
            VideoState::Failed,
            VideoState::Stopped,
        ] {
            let mut session = PrivacySession::new(17, false, false);
            session.video = state;
            session.video_operation = Some(op(1));
            session.stop();
            session.fail();
            assert_eq!(session.video(), VideoState::Stopped);
            assert!(session.desired_audio_muted());
            assert_eq!(
                session.confirm_pause(op(1), barrier()),
                Err(PrivacyError::Confirmation)
            );
            assert_eq!(
                session.request(Request::Resume { operation: op(2) }),
                Err(PrivacyError::Stopped)
            );
        }
        Ok(())
    }

    #[test]
    fn epoch_exhaustion_refuses_transition_without_unblocking() {
        let mut session = PrivacySession::new(17, true, false);
        session.source_epoch = u64::MAX;
        assert_eq!(
            session.request(Request::Resume { operation: op(1) }),
            Err(PrivacyError::Transition)
        );
        assert!(!session.allows_source_frame(u64::MAX));
        assert_eq!(
            session.request(Request::Pause { operation: op(1) }),
            Err(PrivacyError::Transition)
        );
    }

    #[test]
    fn request_roundtrip_and_rejected_transition_do_not_consume_sequence()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut session = PrivacySession::new(17, false, false);
        assert_eq!(
            session.request(Request::Resume { operation: op(1) }),
            Err(PrivacyError::Transition)
        );
        let request = Request::Pause { operation: op(1) };
        let decoded = serde_json::from_str(&serde_json::to_string(&request)?)?;
        assert_eq!(request, decoded);
        session.request(decoded)?;
        Ok(())
    }
}
