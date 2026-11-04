//! Supervisor do engine de captura/encode (sidecar). Cada compartilhamento é um processo:
//! ao parar, o processo sai e a GPU volta ao jogo. A UI só recebe o estado resumido.
use madscreen_ipc::{
    AudioRequest, Command, ErrorCode, Event, Quality, SourceSpec, StartRequest, StopReason,
};
use serde::Serialize;
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, ChildStdin, Stdio},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(test)]
mod media_test;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Starting,
    Picking,
    Live,
    Stopping,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ShareState {
    pub privacy_supported: bool,
    pub privacy_video: Option<madscreen_ipc::privacy::VideoState>,
    pub privacy_pending: bool,
    pub audio_muted: bool,
    pub phase: Phase,
    pub changing: bool,
    pub session_id: u64,
    pub quality_height: u32,
    pub source: Option<String>,
    pub source_instance_id: Option<String>,
    pub encoder: Option<String>,
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub level: Option<u32>,
    pub profile_level_id: Option<String>,
    pub fps: f64,
    pub bitrate_bps: u64,
    pub fps_target: f64,
    pub bitrate_target_bps: u32,
    pub publishing: bool,
    pub publishing_connected: Option<bool>,
    /// Fontes de áudio ativas; `None` quando a transmissão não pediu áudio.
    pub audio_active: Option<u32>,
    /// Último aviso ou erro para mostrar na UI.
    pub message: Option<String>,
    /// `true` quando `message` descreve uma falha; `false` para avisos (seleção cancelada etc.).
    pub message_is_error: bool,
    /// Texto completo para copiar quando a publicação falha (encoder, SDP e erro).
    pub diagnostic: Option<String>,
}

struct Session {
    #[cfg(any(windows, test))]
    notify: Listener,
    id: u64,
    child: Child,
    stdin: ChildStdin,
    request: StartRequest,
    privacy_sequence: u64,
    privacy_operation: Option<madscreen_ipc::privacy::Operation>,
    privacy_audio: Option<bool>,
}

#[derive(Default)]
struct Inner {
    session: Option<Session>,
    state: ShareState,
    publisher_history: madscreen_ipc::diagnostics::PublisherHistory,
    operation: u64,
}

impl Inner {
    fn cancel_change(&mut self) {
        self.operation = self.operation.wrapping_add(1);
        self.state.changing = false;
    }
}

#[derive(Clone, Default)]
pub struct Engine {
    inner: Arc<Mutex<Inner>>,
    next_id: Arc<AtomicU64>,
    #[cfg(test)]
    test_command: Option<Arc<Vec<String>>>,
}

type Listener = Arc<dyn Fn(&Engine, bool) + Send + Sync>;

fn listener(app: &AppHandle) -> Listener {
    let app = app.clone();
    Arc::new(move |engine, phase_changed| engine.publish(&app, phase_changed))
}

const STOP_GRACE: Duration = Duration::from_secs(4);
const START_GRACE: Duration = Duration::from_secs(45);

fn changed_request(
    previous: &StartRequest,
    quality: Quality,
    source: Option<SourceSpec>,
    audio: Option<AudioRequest>,
) -> Result<StartRequest, String> {
    if !quality.is_supported() {
        return Err("qualidade não suportada".into());
    }
    if source.is_none() && previous.source == SourceSpec::Picker {
        return Err(
            "Escolha novamente a janela ou tela para alterar esta captura do seletor do Windows."
                .into(),
        );
    }
    let mut request = previous.clone();
    request.quality = quality;
    if let Some(source) = source {
        request.source = source;
        request.audio = audio;
    }
    Ok(request)
}

fn engine_file_name() -> &'static str {
    if cfg!(windows) {
        "madscreen-encode-test.exe"
    } else {
        "madscreen-encode-test"
    }
}

/// Procura o engine: variável de ambiente, ao lado do executável (bundle) e `target/` do repo (dev).
pub(crate) fn locate_engine() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("MADSCREEN_ENGINE") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join(engine_file_name()));
    }
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target");
    candidates.push(repo.join("release").join(engine_file_name()));
    candidates.push(repo.join("debug").join(engine_file_name()));
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        format!(
            "engine de captura ({}) não encontrado; defina MADSCREEN_ENGINE ou reinstale o app",
            engine_file_name()
        )
    })
}

/// Um HRESULT 0 vindo de uma API nula (ex.: seletor cancelado) aparece como "operação concluída com
/// êxito (0x00000000)": troca por um texto que o usuário entende.
fn friendly(message: String) -> String {
    if message.contains("0x00000000") {
        "O Windows não devolveu nenhuma janela ou tela (seleção cancelada). Tente novamente.".into()
    } else {
        message
    }
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Engine {
    #[cfg(any(windows, test))]
    pub(crate) fn on_window_destroyed(&self, hwnd: i64) {
        let notify=lock(&self.inner).session.as_ref().filter(|session|
            matches!(&session.request.source, SourceSpec::VerifiedWindow {hwnd:active,..} if *active==hwnd)).map(|session|session.notify.clone());
        if let Some(notify) = notify {
            self.shutdown();
            notify(self, true);
        }
    }
    pub fn snapshot(&self) -> ShareState {
        let guard = lock(&self.inner);
        let mut state = guard.state.clone();
        if state.phase == Phase::Live && !state.changing {
            state.source_instance_id =
                guard
                    .session
                    .as_ref()
                    .and_then(|session| match &session.request.source {
                        SourceSpec::VerifiedWindow { instance_id, .. } => Some(instance_id.clone()),
                        _ => None,
                    });
        }
        state
    }

    pub fn publisher_diagnostics(
        &self,
        app_version: String,
    ) -> madscreen_ipc::diagnostics::PublisherReport {
        lock(&self.inner).publisher_history.report(app_version)
    }

    pub fn start(&self, app: &AppHandle, request: StartRequest) -> Result<(), String> {
        self.start_internal(&listener(app), request, None)
            .map(|_| ())
    }

    fn command(&self) -> Result<std::process::Command, String> {
        #[cfg(test)]
        if let Some(arguments) = &self.test_command {
            let (program, arguments) = arguments.split_first().ok_or("comando de teste vazio")?;
            let mut command = std::process::Command::new(program);
            command.args(arguments);
            return Ok(command);
        }
        Ok(std::process::Command::new(locate_engine()?))
    }

    /// Uma operação cancelada não pode recriar captura após Parar, logout ou updater.
    fn start_internal(
        &self,
        notify: &Listener,
        request: StartRequest,
        operation: Option<u64>,
    ) -> Result<bool, String> {
        if !request.quality.is_supported() {
            return Err(
                "qualidade não suportada (use 720p/1080p/1440p/2160p ou Nativa a 30/60 FPS)".into(),
            );
        }
        let mut command = self.command()?;
        let exe = command.get_program().to_string_lossy().into_owned();
        let mut guard = lock(&self.inner);
        if let Some(operation) = operation {
            if guard.operation != operation || !guard.state.changing {
                return Ok(false);
            }
        } else {
            if guard.state.changing || guard.state.phase != Phase::Idle {
                return Err("já existe um compartilhamento em andamento".into());
            }
            guard.operation = guard.operation.wrapping_add(1);
        }
        if guard.session.is_some() {
            return Err("já existe um compartilhamento em andamento".into());
        }
        command
            .arg("--ipc")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: sem console piscando ao iniciar o sidecar.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("falha ao iniciar o engine ({exe}): {e}"))?;
        let (Some(mut stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            return Err("canais do engine indisponíveis".into());
        };
        let picking = request.source == SourceSpec::Picker;
        if let Err(e) = madscreen_ipc::write_line(&mut stdin, &Command::Start(request.clone())) {
            let _ = child.kill();
            return Err(format!("falha ao enviar o pedido ao engine: {e}"));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        guard.session = Some(Session {
            #[cfg(any(windows, test))]
            notify: notify.clone(),
            id,
            child,
            stdin,
            request: request.clone(),
            privacy_sequence: 0,
            privacy_operation: None,
            privacy_audio: None,
        });
        guard.publisher_history.clear();
        guard.state = ShareState {
            changing: operation.is_some(),
            session_id: id,
            quality_height: request.quality.height,
            phase: if picking {
                Phase::Picking
            } else {
                Phase::Starting
            },
            ..ShareState::default()
        };
        drop(guard);
        notify(self, true);

        let tail = Arc::new(Mutex::new(VecDeque::<String>::new()));
        {
            let tail = tail.clone();
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    tracing::debug!(target: "engine", "{line}");
                    let mut tail = tail.lock().unwrap_or_else(|p| p.into_inner());
                    if tail.len() == 20 {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            });
        }
        let this = self.clone();
        let notify = notify.clone();
        thread::spawn(move || this.read_events(&notify, id, stdout, &tail));
        Ok(true)
    }

    pub fn stop(&self, app: &AppHandle) {
        self.request_stop(&listener(app), true);
    }

    pub fn privacy(
        &self,
        app: &AppHandle,
        pause: Option<bool>,
        muted: Option<bool>,
    ) -> Result<(), String> {
        self.privacy_internal(&listener(app), pause, muted)
    }

    fn privacy_internal(
        &self,
        notify: &Listener,
        pause: Option<bool>,
        muted: Option<bool>,
    ) -> Result<(), String> {
        use madscreen_ipc::privacy::{Operation, Request, VideoState};
        if pause.is_some() == muted.is_some() {
            return Err("Escolha uma ação de vídeo ou áudio.".into());
        }
        let mut guard = lock(&self.inner);
        if guard.state.phase != Phase::Live || guard.state.changing || guard.state.privacy_pending {
            return Err("Aguarde a operação atual terminar.".into());
        }
        if !guard.state.privacy_supported {
            return Err("Este engine não suporta pausa segura. Atualize o aplicativo.".into());
        }
        if pause == Some(false) && guard.state.privacy_video != Some(VideoState::Paused) {
            return Err("O vídeo não está pausado.".into());
        }
        let session = guard
            .session
            .as_mut()
            .ok_or("A transmissão já foi encerrada.")?;
        session.privacy_sequence = session
            .privacy_sequence
            .checked_add(1)
            .ok_or("Sequência esgotada; encerre a transmissão.")?;
        let operation = Operation {
            generation: session.id,
            sequence: session.privacy_sequence,
        };
        let request = match (pause, muted) {
            (Some(true), _) => Request::Pause { operation },
            (Some(false), _) => Request::Resume { operation },
            (_, Some(muted)) => Request::SetAudioMuted { operation, muted },
            _ => return Err("Ação inválida.".into()),
        };
        // Falha de escrita é fail-closed: finalizar o processo impede captura órfã.
        if let Err(error) =
            madscreen_ipc::write_line(&mut session.stdin, &Command::Privacy(request))
        {
            let _ = session.child.kill();
            return Err(format!(
                "Falha no controle de privacidade; transmissão encerrada: {error}"
            ));
        }
        session.privacy_operation = Some(operation);
        session.privacy_audio = muted;
        guard.state.privacy_pending = true;
        if let Some(pause) = pause {
            guard.state.privacy_video = Some(if pause {
                VideoState::Pausing
            } else {
                VideoState::Resuming
            });
        }
        drop(guard);
        notify(self, true);
        let this = self.clone();
        let notify = notify.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(6));
            let mut guard = lock(&this.inner);
            if guard
                .session
                .as_ref()
                .is_some_and(|s| s.privacy_operation == Some(operation))
            {
                if let Some(session) = guard.session.as_mut() {
                    let _ = session.child.kill();
                }
                guard.state.message =
                    Some("Privacidade não confirmada; transmissão encerrada por segurança.".into());
                guard.state.message_is_error = true;
                guard.state.phase = Phase::Stopping;
                drop(guard);
                notify(&this, true);
            }
        });
        Ok(())
    }

    fn request_stop(&self, notify: &Listener, cancel_change: bool) {
        let id = {
            let mut guard = lock(&self.inner);
            if cancel_change {
                guard.cancel_change();
            }
            if let Some(session) = guard.session.as_mut() {
                // Falha de escrita só significa que o engine já saiu.
                let _ = madscreen_ipc::write_line(&mut session.stdin, &Command::Stop);
                let id = session.id;
                guard.state.phase = Phase::Stopping;
                Some(id)
            } else {
                None
            }
        };
        notify(self, true);
        let Some(id) = id else { return };
        let this = self.clone();
        thread::spawn(move || {
            let deadline = Instant::now() + STOP_GRACE;
            while Instant::now() < deadline {
                thread::sleep(Duration::from_millis(100));
                if !this.is_session(id) {
                    return;
                }
            }
            if let Some(session) = lock(&this.inner).session.as_mut().filter(|s| s.id == id) {
                tracing::warn!("engine não encerrou a tempo; finalizando o processo");
                let _ = session.child.kill();
            }
        });
    }

    /// Chamado ao sair do app: não deixa o engine (e a GPU) para trás.
    pub fn shutdown(&self) {
        let mut guard = lock(&self.inner);
        guard.cancel_change();
        if let Some(mut session) = guard.session.take() {
            let _ = madscreen_ipc::write_line(&mut session.stdin, &Command::Stop);
            drop(session.stdin);
            thread::sleep(Duration::from_millis(300));
            let _ = session.child.kill();
            let _ = session.child.wait();
        }
        // Tambem e usado antes de instalar: se o instalador falhar, o app segue aberto.
        guard.state = ShareState::default();
    }

    pub fn prepare_update(&self, app: &AppHandle) {
        self.shutdown();
        self.publish(app, true);
    }

    /// Troca deliberada da publicação no mesmo canal. O viewer acompanha a nova geração.
    /// Aguarda confirmação do novo pipeline e tenta restaurar o pedido anterior uma vez.
    pub fn change(
        &self,
        app: &AppHandle,
        quality: Quality,
        source: Option<SourceSpec>,
        audio: Option<AudioRequest>,
    ) -> Result<bool, String> {
        self.change_internal(&listener(app), quality, source, audio)
    }

    fn change_internal(
        &self,
        notify: &Listener,
        quality: Quality,
        source: Option<SourceSpec>,
        audio: Option<AudioRequest>,
    ) -> Result<bool, String> {
        let (operation, previous, next) = {
            let mut guard = lock(&self.inner);
            if guard.state.phase != Phase::Live || guard.state.changing {
                return Err("aguarde a transmissão ficar ativa antes de alterar".into());
            }
            if guard.state.privacy_pending
                || matches!(
                    guard.state.privacy_video,
                    Some(
                        madscreen_ipc::privacy::VideoState::Paused
                            | madscreen_ipc::privacy::VideoState::Pausing
                            | madscreen_ipc::privacy::VideoState::Resuming
                            | madscreen_ipc::privacy::VideoState::Failed
                    )
                )
                || guard.state.audio_muted
            {
                return Err("Retome o vídeo e reative o áudio antes de trocar a fonte ou qualidade; ou encerre a transmissão.".into());
            }
            let previous = guard
                .session
                .as_ref()
                .ok_or("a transmissão já foi encerrada")?
                .request
                .clone();
            let next = changed_request(&previous, quality, source, audio)?;
            if next == previous {
                return Ok(true);
            }
            guard.operation = guard.operation.wrapping_add(1);
            guard.state.changing = true;
            (guard.operation, previous, next)
        };
        self.request_stop(notify, false);
        match self.wait_stopped(operation) {
            Ok(true) => {}
            Ok(false) => return Ok(false),
            Err(error) => {
                let mut guard = lock(&self.inner);
                if guard.operation == operation {
                    guard.state.changing = false;
                }
                drop(guard);
                notify(self, true);
                return Err(error);
            }
        }
        let outcome = self
            .start_internal(notify, next, Some(operation))
            .and_then(|started| {
                if started {
                    self.wait_started(notify, operation)
                } else {
                    Ok(false)
                }
            });
        match outcome {
            Ok(applied) => Ok(applied),
            Err(error) => {
                let restored = self
                    .start_internal(notify, previous, Some(operation))
                    .and_then(|started| {
                        if started {
                            self.wait_started(notify, operation)
                        } else {
                            Ok(false)
                        }
                    });
                {
                    let mut guard = lock(&self.inner);
                    if guard.operation != operation {
                        return Ok(false);
                    }
                    guard.state.changing = false;
                }
                notify(self, true);
                match restored {
                    Ok(true) => Err(format!(
                        "Não foi possível aplicar a alteração. A transmissão anterior foi restaurada. {error}"
                    )),
                    Ok(false) => Ok(false),
                    Err(_) => Err(format!(
                        "Não foi possível aplicar a alteração nem restaurar a transmissão. Inicie novamente. {error}"
                    )),
                }
            }
        }
    }

    fn wait_stopped(&self, operation: u64) -> Result<bool, String> {
        let deadline = Instant::now() + STOP_GRACE + Duration::from_secs(2);
        loop {
            {
                let guard = lock(&self.inner);
                if guard.operation != operation {
                    return Ok(false);
                }
                // EOF só libera a troca depois de recolher o processo e publicar Idle.
                if guard.session.is_none() && guard.state.phase == Phase::Idle {
                    return Ok(true);
                }
            }
            if Instant::now() >= deadline {
                return Err("o compartilhamento anterior não encerrou a tempo".into());
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn wait_started(&self, notify: &Listener, operation: u64) -> Result<bool, String> {
        let deadline = Instant::now() + START_GRACE;
        loop {
            {
                let guard = lock(&self.inner);
                if guard.operation != operation {
                    return Ok(false);
                }
                if guard.state.phase == Phase::Live {
                    return Ok(true);
                }
                if guard.session.is_none() && guard.state.phase == Phase::Idle {
                    return Err(guard
                        .state
                        .message
                        .clone()
                        .unwrap_or_else(|| "a nova captura não iniciou".into()));
                }
            }
            if Instant::now() >= deadline {
                self.request_stop(notify, false);
                self.wait_stopped(operation)?;
                return Err("tempo esgotado ao aplicar a alteração".into());
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn is_session(&self, id: u64) -> bool {
        lock(&self.inner)
            .session
            .as_ref()
            .is_some_and(|s| s.id == id)
    }

    fn read_events(
        &self,
        notify: &Listener,
        id: u64,
        stdout: impl std::io::Read,
        tail: &Mutex<VecDeque<String>>,
    ) {
        let mut reader = BufReader::new(stdout);
        loop {
            match madscreen_ipc::read_line::<Event>(&mut reader) {
                Ok(Some(event)) => self.apply(notify, id, event),
                Ok(None) => break,
                Err(madscreen_ipc::IpcError::Json(e)) => {
                    tracing::warn!("linha inesperada do engine: {e}");
                }
                Err(e) => {
                    tracing::warn!("canal do engine encerrado: {e}");
                    break;
                }
            }
        }
        // EOF: o processo saiu. Recolhe o status e volta para o estado ocioso.
        let session = {
            let mut guard = lock(&self.inner);
            if guard.session.as_ref().is_some_and(|s| s.id == id) {
                guard.session.take()
            } else {
                None
            }
        };
        let Some(mut session) = session else { return };
        let status = session.child.wait().ok();
        let mut guard = lock(&self.inner);
        // Shutdown pode ter cancelado a troca e uma nova sessão pode já existir.
        if guard.session.is_some() || (guard.state.phase == Phase::Idle && !guard.state.changing) {
            return;
        }
        let had_error = guard.state.message.is_some();
        let mut message_is_error = guard.state.message_is_error;
        let message = if had_error {
            guard.state.message.take()
        } else if status.is_some_and(|s| !s.success()) {
            message_is_error = true;
            let lines: Vec<String> = tail
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .rev()
                .take(3)
                .rev()
                .cloned()
                .collect();
            Some(if lines.is_empty() {
                "o engine encerrou inesperadamente".to_string()
            } else {
                format!(
                    "o engine encerrou inesperadamente: {}",
                    friendly(lines.join(" | "))
                )
            })
        } else {
            None
        };
        guard.state = ShareState {
            changing: guard.state.changing,
            message,
            message_is_error,
            ..ShareState::default()
        };
        drop(guard);
        notify(self, true);
    }

    fn apply(&self, notify: &Listener, id: u64, event: Event) {
        let mut phase_changed = true;
        {
            let mut guard = lock(&self.inner);
            if !guard.session.as_ref().is_some_and(|s| s.id == id) {
                return;
            }
            if let Event::Privacy {
                operation,
                video,
                audio_muted,
            } = &event
            {
                use madscreen_ipc::privacy::VideoState;
                let expected = match guard.state.privacy_video {
                    Some(VideoState::Pausing) => VideoState::Paused,
                    Some(VideoState::Resuming) => VideoState::Active,
                    other => other.unwrap_or(VideoState::Active),
                };
                let session = guard
                    .session
                    .as_mut()
                    .filter(|s| s.privacy_operation == Some(*operation));
                let Some(session) = session else { return };
                if session.privacy_audio.is_none() && *video != expected {
                    return;
                }
                if session
                    .privacy_audio
                    .is_some_and(|wanted| wanted != *audio_muted)
                {
                    return;
                }
                if session.privacy_audio.is_none()
                    && !matches!(
                        video,
                        madscreen_ipc::privacy::VideoState::Paused
                            | madscreen_ipc::privacy::VideoState::Active
                    )
                {
                    return;
                }
                session.privacy_operation = None;
                session.privacy_audio = None;
            }
            if let Event::Status {
                diagnostic_sample: Some(sample),
                ..
            } = &event
            {
                guard.publisher_history.record((**sample).clone());
            }
            let state = &mut guard.state;
            match event {
                Event::Ready { version, .. } => state.privacy_supported = version >= 4,
                Event::DiagnosticClock { .. } => phase_changed = false,
                Event::Privacy {
                    video, audio_muted, ..
                } => {
                    if state.phase == Phase::Live {
                        state.privacy_video = Some(video);
                        state.audio_muted = audio_muted;
                        state.privacy_pending = false;
                    }
                }
                Event::Picking => state.phase = Phase::Picking,
                Event::Audio { active, failed } => {
                    state.audio_active = Some(active);
                    if !failed.is_empty() && !state.message_is_error {
                        state.message = Some(if active == 0 {
                            "Não foi possível capturar o áudio escolhido; a tela segue sem som."
                                .into()
                        } else {
                            "Parte do áudio escolhido não pôde ser capturada.".into()
                        });
                    }
                }
                Event::PickCancelled => {
                    state.message = Some("Seleção cancelada.".into());
                    state.message_is_error = false;
                }
                Event::Started {
                    source,
                    encoder,
                    width,
                    height,
                    source_width,
                    source_height,
                    notice,
                    level,
                    profile_level_id,
                    fps_target,
                    bitrate_target_bps,
                    publishing,
                    ..
                } => {
                    state.phase = Phase::Live;
                    state.changing = false;
                    state.source = Some(source);
                    state.encoder = encoder;
                    state.width = width;
                    state.height = height;
                    state.source_width = source_width;
                    state.source_height = source_height;
                    state.level = level;
                    state.profile_level_id = profile_level_id;
                    state.fps_target = fps_target;
                    state.bitrate_target_bps = bitrate_target_bps;
                    state.publishing = publishing;
                    state.message = notice;
                    state.message_is_error = false;
                    state.diagnostic = None;
                }
                Event::Status {
                    fps,
                    bitrate_bps,
                    publishing_connected,
                    ..
                } => {
                    state.fps = fps;
                    state.bitrate_bps = bitrate_bps;
                    phase_changed = state.publishing_connected != publishing_connected;
                    state.publishing_connected = publishing_connected;
                }
                Event::Stopped { reason } => {
                    if reason == StopReason::SourceClosed {
                        state.message =
                            Some("A janela ou monitor compartilhado foi fechado.".into());
                        state.message_is_error = false;
                    } else if !state.message_is_error {
                        // Aviso do inicio (ex.: resolucao ajustada) nao vale mais depois de parar.
                        state.message = None;
                    }
                }
                Event::Error {
                    code,
                    message,
                    diagnostic,
                } => {
                    if code == ErrorCode::Transport {
                        state.publishing_connected = Some(false);
                    }
                    state.message_is_error = true;
                    let message = friendly(message);
                    state.message = Some(match code {
                        ErrorCode::Unsupported => format!("Não suportado: {message}"),
                        _ => message,
                    });
                    state.diagnostic = diagnostic.filter(|text| !text.trim().is_empty());
                }
            }
        }
        notify(self, phase_changed);
    }

    /// Envia o estado ao frontend só com a janela visível: oculta no tray, o webview não trabalha.
    fn publish(&self, app: &AppHandle, phase_changed: bool) {
        let state = self.snapshot();
        if phase_changed {
            crate::tray::update(app, &state);
        }
        let visible = app
            .get_webview_window("main")
            .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false));
        if visible {
            let _ = app.emit("share-state", &state);
        }
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::{Engine, Inner, Phase, changed_request, lock};
    use madscreen_ipc::{AudioRequest, CloudflareTarget, Quality, SourceSpec, StartRequest};
    use std::{
        sync::Arc,
        thread,
        time::{Duration, Instant},
    };

    struct Fixture(Engine);
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }

    fn fixture() -> Result<(Fixture, super::Listener), String> {
        let engine = Engine {
            test_command: Some(Arc::new(vec![
                if cfg!(windows) { "python" } else { "python3" }.into(),
                "-u".into(),
                "-c".into(),
                include_str!("fixtures/supervisor.py").into(),
            ])),
            ..Engine::default()
        };
        let notify: super::Listener = Arc::new(|_, _| {});
        engine.start_internal(&notify, request(), None)?;
        until(|| engine.snapshot().phase == Phase::Live)?;
        Ok((Fixture(engine), notify))
    }

    #[test]
    fn privacy_ack_preserves_process_and_prevents_implicit_restart() -> Result<(), String> {
        use madscreen_ipc::privacy::VideoState;
        let (fixture, notify) = fixture()?;
        let engine = &fixture.0;
        let id = engine.snapshot().session_id;
        assert!(engine.snapshot().privacy_supported);
        engine.privacy_internal(&notify, Some(true), None)?;
        assert!(engine.snapshot().privacy_pending);
        assert_eq!(engine.snapshot().privacy_video, Some(VideoState::Pausing));
        assert!(engine.privacy_internal(&notify, Some(false), None).is_err());
        until(|| !engine.snapshot().privacy_pending)?;
        assert_eq!(engine.snapshot().privacy_video, Some(VideoState::Paused));
        assert!(
            engine
                .change_internal(
                    &notify,
                    Quality {
                        height: 720,
                        fps: 30
                    },
                    None,
                    None
                )
                .is_err()
        );
        engine.privacy_internal(&notify, None, Some(true))?;
        until(|| !engine.snapshot().privacy_pending)?;
        assert!(engine.snapshot().audio_muted);
        engine.privacy_internal(&notify, Some(false), None)?;
        until(|| !engine.snapshot().privacy_pending)?;
        assert_eq!(engine.snapshot().privacy_video, Some(VideoState::Active));
        assert!(
            engine
                .change_internal(
                    &notify,
                    Quality {
                        height: 720,
                        fps: 30
                    },
                    None,
                    None
                )
                .is_err()
        );
        engine.privacy_internal(&notify, None, Some(false))?;
        until(|| !engine.snapshot().privacy_pending)?;
        assert_eq!(engine.snapshot().session_id, id);
        engine.privacy_internal(&notify, Some(true), None)?;
        engine.request_stop(&notify, true);
        until(|| engine.snapshot().phase == Phase::Idle)?;
        assert!(lock(&engine.inner).session.is_none());
        Ok(())
    }

    #[test]
    fn old_engine_does_not_receive_privacy_command() -> Result<(), String> {
        let (fixture, notify) = fixture()?;
        lock(&fixture.0.inner).state.privacy_supported = false;
        assert!(
            fixture
                .0
                .privacy_internal(&notify, Some(true), None)
                .is_err()
        );
        assert!(!fixture.0.snapshot().privacy_pending);
        Ok(())
    }

    #[test]
    fn missing_or_wrong_ack_ends_process_instead_of_resuming() -> Result<(), String> {
        use madscreen_ipc::privacy::{Operation, VideoState};
        let (fixture, notify) = fixture()?;
        let engine = &fixture.0;
        {
            let mut guard = lock(&engine.inner);
            guard
                .session
                .as_mut()
                .ok_or("sessão ausente")?
                .privacy_sequence = 98;
        }
        engine.privacy_internal(&notify, Some(true), None)?;
        let id = engine.snapshot().session_id;
        for (generation, sequence, video) in [
            (id + 1, 99, VideoState::Paused),
            (id, 98, VideoState::Paused),
            (id, 99, VideoState::Active),
        ] {
            engine.apply(
                &notify,
                id,
                madscreen_ipc::Event::Privacy {
                    operation: Operation {
                        generation,
                        sequence,
                    },
                    video,
                    audio_muted: false,
                },
            );
            assert!(engine.snapshot().privacy_pending);
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        while engine.snapshot().phase != Phase::Idle && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(engine.snapshot().phase, Phase::Idle);
        assert!(engine.snapshot().message_is_error);
        assert!(lock(&engine.inner).session.is_none());
        assert_eq!(engine.next_id.load(std::sync::atomic::Ordering::Relaxed), 1);
        Ok(())
    }

    fn until(mut ready: impl FnMut() -> bool) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready() {
            if Instant::now() >= deadline {
                return Err("timeout no fixture do supervisor".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    #[test]
    fn real_process_changes_quality_and_preserves_the_channel() -> Result<(), String> {
        let (fixture, notify) = fixture()?;
        assert!(fixture.0.change_internal(
            &notify,
            Quality {
                height: 720,
                fps: 30
            },
            None,
            None
        )?);
        let state = fixture.0.snapshot();
        assert_eq!(
            (state.width, state.height, state.fps_target),
            (1280, 720, 30.0)
        );
        assert!(!state.changing);
        let guard = lock(&fixture.0.inner);
        let current = &guard.session.as_ref().ok_or("sessao ausente")?.request;
        assert_eq!(current.cloudflare, request().cloudflare);
        assert_eq!(current.audio, request().audio);
        Ok(())
    }

    #[test]
    fn real_process_failed_source_restores_previous_request_once() -> Result<(), String> {
        let (fixture, notify) = fixture()?;
        let result = fixture.0.change_internal(
            &notify,
            Quality {
                height: 720,
                fps: 30,
            },
            Some(SourceSpec::WindowHandle { hwnd: 789 }),
            None,
        );
        assert!(result.is_err_and(|message| message.contains("anterior foi restaurada")));
        assert_eq!(fixture.0.snapshot().phase, Phase::Live);
        assert_eq!(fixture.0.snapshot().quality_height, 1080);
        assert_eq!(
            fixture.0.next_id.load(std::sync::atomic::Ordering::Relaxed),
            3
        );
        assert_eq!(
            lock(&fixture.0.inner)
                .session
                .as_ref()
                .ok_or("sessao ausente")?
                .request,
            request()
        );
        Ok(())
    }

    #[test]
    fn snapshot_exposes_verified_identity_only_for_confirmed_session() -> Result<(), String> {
        let (fixture, _) = fixture()?;
        {
            let mut guard = lock(&fixture.0.inner);
            guard
                .session
                .as_mut()
                .ok_or("sessao ausente")?
                .request
                .source = SourceSpec::VerifiedWindow {
                hwnd: 123,
                instance_id: "b".repeat(64),
            };
        }
        assert_eq!(
            fixture.0.snapshot().source_instance_id,
            Some("b".repeat(64))
        );
        lock(&fixture.0.inner).state.changing = true;
        assert_eq!(fixture.0.snapshot().source_instance_id, None);
        lock(&fixture.0.inner).state.changing = false;
        lock(&fixture.0.inner).state.phase = Phase::Stopping;
        assert_eq!(fixture.0.snapshot().source_instance_id, None);
        Ok(())
    }

    #[test]
    fn destroyed_verified_window_stops_session_notifies_and_never_restarts() -> Result<(), String> {
        let (fixture, _) = fixture()?;
        lock(&fixture.0.inner)
            .session
            .as_mut()
            .ok_or("sessao ausente")?
            .request
            .source = SourceSpec::VerifiedWindow {
            hwnd: 123,
            instance_id: "b".repeat(64),
        };
        fixture.0.on_window_destroyed(456);
        assert_eq!(fixture.0.snapshot().phase, Phase::Live);
        fixture.0.on_window_destroyed(123);
        assert_eq!(fixture.0.snapshot().phase, Phase::Idle);
        assert!(lock(&fixture.0.inner).session.is_none());
        assert_eq!(
            fixture.0.next_id.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        Ok(())
    }

    #[test]
    fn real_process_stop_during_shutdown_prevents_the_replacement() -> Result<(), String> {
        let (fixture, notify) = fixture()?;
        let operation_engine = fixture.0.clone();
        let operation_notify = notify.clone();
        let change = thread::spawn(move || {
            operation_engine.change_internal(
                &operation_notify,
                Quality {
                    height: 720,
                    fps: 30,
                },
                None,
                None,
            )
        });
        until(|| fixture.0.snapshot().phase == Phase::Stopping && fixture.0.snapshot().changing)?;
        fixture.0.request_stop(&notify, true);
        assert!(!change.join().map_err(|_| "thread de teste falhou")??);
        until(|| fixture.0.snapshot().phase == Phase::Idle)?;
        assert_eq!(
            fixture.0.next_id.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert!(lock(&fixture.0.inner).session.is_none());
        Ok(())
    }

    #[test]
    fn real_process_stop_during_new_start_never_rolls_back_after_cancel() -> Result<(), String> {
        let (fixture, notify) = fixture()?;
        let operation_engine = fixture.0.clone();
        let operation_notify = notify.clone();
        let change = thread::spawn(move || {
            operation_engine.change_internal(
                &operation_notify,
                Quality {
                    height: 720,
                    fps: 30,
                },
                Some(SourceSpec::WindowHandle { hwnd: 321 }),
                None,
            )
        });
        until(|| fixture.0.snapshot().session_id == 2 && fixture.0.snapshot().changing)?;
        fixture.0.request_stop(&notify, true);
        assert!(!change.join().map_err(|_| "thread de teste falhou")??);
        until(|| fixture.0.snapshot().phase == Phase::Idle)?;
        assert_eq!(
            fixture.0.next_id.load(std::sync::atomic::Ordering::Relaxed),
            2
        );
        assert!(lock(&fixture.0.inner).session.is_none());
        Ok(())
    }

    #[test]
    fn real_process_updater_shutdown_cancels_pending_change() -> Result<(), String> {
        let (fixture, notify) = fixture()?;
        let operation_engine = fixture.0.clone();
        let change = thread::spawn(move || {
            operation_engine.change_internal(
                &notify,
                Quality {
                    height: 720,
                    fps: 30,
                },
                None,
                None,
            )
        });
        until(|| fixture.0.snapshot().changing)?;
        fixture.0.shutdown();
        assert!(!change.join().map_err(|_| "thread de teste falhou")??);
        assert_eq!(fixture.0.snapshot().phase, Phase::Idle);
        assert!(lock(&fixture.0.inner).session.is_none());
        Ok(())
    }

    fn request() -> StartRequest {
        StartRequest {
            source: SourceSpec::WindowHandle { hwnd: 123 },
            quality: Quality {
                height: 1080,
                fps: 60,
            },
            bitrate_bps: None,
            livekit: None,
            cloudflare: Some(CloudflareTarget {
                url: Some("https://fixture.invalid".into()),
                token: Some("fixture-only".into()),
                channel_id: Some("same-channel".into()),
                profile_level_id: None,
            }),
            audio: Some(AudioRequest {
                follow_window: true,
                ..AudioRequest::default()
            }),
        }
    }

    #[test]
    fn quality_change_preserves_source_audio_channel_and_auth() -> Result<(), String> {
        let previous = request();
        let next = changed_request(
            &previous,
            Quality {
                height: 720,
                fps: 30,
            },
            None,
            None,
        )?;
        assert_eq!(next.source, previous.source);
        assert_eq!(next.audio, previous.audio);
        assert_eq!(next.cloudflare, previous.cloudflare);
        assert_eq!(
            next.quality,
            Quality {
                height: 720,
                fps: 30
            }
        );
        assert_eq!(previous.quality.height, 1080);
        Ok(())
    }

    #[test]
    fn new_monitor_uses_its_audio_and_keeps_the_destination() -> Result<(), String> {
        let previous = request();
        let audio = AudioRequest {
            system: true,
            exclude_pid: Some(456),
            ..AudioRequest::default()
        };
        let next = changed_request(
            &previous,
            previous.quality,
            Some(SourceSpec::Monitor { index: 1 }),
            Some(audio.clone()),
        )?;
        assert_eq!(next.source, SourceSpec::Monitor { index: 1 });
        assert_eq!(next.audio, Some(audio));
        assert_eq!(next.cloudflare, previous.cloudflare);
        let silent = changed_request(
            &previous,
            previous.quality,
            Some(SourceSpec::WindowHandle { hwnd: 789 }),
            None,
        )?;
        assert_eq!(silent.audio, None);
        Ok(())
    }

    #[test]
    fn invalid_change_is_rejected_before_touching_the_live_request() {
        let mut previous = request();
        assert!(
            changed_request(
                &previous,
                Quality {
                    height: 900,
                    fps: 60
                },
                None,
                None
            )
            .is_err()
        );
        previous.source = SourceSpec::Picker;
        assert!(changed_request(&previous, previous.quality, None, None).is_err());
        assert!(
            changed_request(
                &previous,
                previous.quality,
                Some(SourceSpec::Monitor { index: 0 }),
                None
            )
            .is_ok()
        );
    }

    #[test]
    fn stop_invalidates_an_in_flight_change_even_between_children() {
        let mut inner = Inner::default();
        inner.state.changing = true;
        inner.operation = 12;
        let pending = inner.operation;
        inner.cancel_change();
        assert_ne!(inner.operation, pending);
        assert!(!inner.state.changing);
        assert!(inner.session.is_none());
    }

    #[test]
    fn shutdown_clears_publisher_state_and_can_be_repeated_before_install_retry() {
        let engine = Engine::default();
        {
            let mut guard = lock(&engine.inner);
            guard.state.phase = Phase::Live;
            guard.state.publishing = true;
            guard.state.publishing_connected = Some(true);
            guard.state.width = 1920;
            guard.state.audio_active = Some(1);
            guard.state.changing = true;
        }
        for _ in 0..2 {
            engine.shutdown();
            let state = engine.snapshot();
            assert_eq!(state.phase, Phase::Idle);
            assert!(!state.publishing);
            assert_eq!(state.publishing_connected, None);
            assert_eq!(state.width, 0);
            assert_eq!(state.audio_active, None);
            assert!(!state.changing);
        }
    }
}
