mod engine;
mod foreground;
#[cfg(windows)]
mod foreground_sys;
#[cfg(any(windows, test))]
mod migration;
mod miniplayer;
mod renderer_recovery;
#[cfg(windows)]
#[allow(unsafe_code)]
mod renderer_recovery_sys;
mod settings;
mod sources;
mod tray;
#[cfg(windows)]
mod video_playout;
mod window_guard;

use engine::{Engine, ShareState};
use madscreen_audio::AudioApp;
use madscreen_ipc::{
    AudioRequest, CloudflareTarget, EncoderLimits, OutputOption, Quality, SourceList, SourceSpec,
    StartRequest, resolution,
};
use settings::Settings;
use tauri::{AppHandle, Manager, State, WindowEvent};

#[tauri::command]
fn take_renderer_recovery(
    window: tauri::WebviewWindow,
    app: AppHandle,
    recovery: State<'_, renderer_recovery::RendererRecovery>,
) -> bool {
    if window.label() != "main" || !recovery.take() {
        return false;
    }
    miniplayer::destroy_all(&app);
    true
}

#[tauri::command]
fn get_state(engine: State<'_, Engine>) -> ShareState {
    engine.snapshot()
}

#[tauri::command]
fn get_publisher_diagnostics(
    app: AppHandle,
    engine: State<'_, Engine>,
) -> madscreen_ipc::diagnostics::PublisherReport {
    engine.publisher_diagnostics(app.package_info().version.to_string())
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    settings::load(&app)
}

#[tauri::command]
fn hide_to_tray(app: AppHandle) -> Result<(), String> {
    let mut current = settings::load(&app);
    current.tray_notice_seen = true;
    settings::save(&app, &current)?;
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn set_software_video_decode(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut current = settings::load(&app);
    current.software_video_decode = enabled;
    settings::save(&app, &current)
}

/// O updater encerra o processo diretamente no Windows; libere o sidecar antes.
#[tauri::command]
async fn prepare_update_install(app: AppHandle, engine: State<'_, Engine>) -> Result<(), String> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || engine.prepare_update(&app))
        .await
        .map_err(|error| format!("falha ao encerrar o compartilhamento: {error}"))
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let mut settings = settings;
    settings.tray_notice_seen = settings::load(&app).tray_notice_seen;
    settings::save(&app, &settings)
}

/// Compartilha pelo SFU da Cloudflare (Worker de sinalizaÃ§Ã£o). `url`/`token` vÃªm da sessÃ£o do
/// app; sem eles o engine lÃª `MADSCREEN_CF_URL`/`MADSCREEN_CF_TOKEN` do ambiente ou do
/// `server.env`. O token nunca Ã© gravado nem logado aqui.
#[tauri::command]
// Cada argumento é um campo nomeado da chamada vinda do frontend.
#[allow(clippy::too_many_arguments)]
fn start_share_cloudflare(
    app: AppHandle,
    engine: State<'_, Engine>,
    height: u32,
    fps: u32,
    source: Option<SourceSpec>,
    url: Option<String>,
    token: Option<String>,
    audio: Option<AudioRequest>,
    channel_id: Option<String>,
    direct_session_id: Option<String>,
    selection_id: Option<u64>,
    fps_chosen: Option<bool>,
) -> Result<(), String> {
    window_guard::validate(&app, &source, selection_id)?;
    let mut saved = settings::load(&app);
    saved.quality_height = height;
    saved.quality_fps = fps;
    saved.quality_fps_chosen |= fps_chosen == Some(true);
    let _ = settings::save(&app, &saved);
    engine.start(
        &app,
        StartRequest {
            source: source.unwrap_or(SourceSpec::Picker),
            quality: Quality { height, fps },
            bitrate_bps: None,
            livekit: None,
            cloudflare: Some(CloudflareTarget {
                url,
                token,
                channel_id,
                direct_session_id,
                profile_level_id: None,
            }),
            audio: audio.map(with_own_process),
        },
    )
}

/// Aplica qualidade/fonte no canal da sessão atual; cancelar ou sair impede reinício tardio.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn change_share(
    app: AppHandle,
    engine: State<'_, Engine>,
    height: u32,
    fps: u32,
    source: Option<SourceSpec>,
    audio: Option<AudioRequest>,
    selection_id: Option<u64>,
    fps_chosen: Option<bool>,
) -> Result<(), String> {
    window_guard::validate(&app, &source, selection_id)?;
    let engine = engine.inner().clone();
    let operation_app = app.clone();
    let applied = tauri::async_runtime::spawn_blocking(move || {
        engine.change(
            &operation_app,
            Quality { height, fps },
            source,
            audio.map(with_own_process),
        )
    })
    .await
    .map_err(|error| format!("falha ao alterar o compartilhamento: {error}"))??;
    if applied {
        let mut saved = settings::load(&app);
        saved.quality_height = height;
        saved.quality_fps = fps;
        saved.quality_fps_chosen |= fps_chosen == Some(true);
        let _ = settings::save(&app, &saved);
    }
    Ok(())
}

/// O som do PC todo nunca inclui o prÃ³prio madscreen (evita eco com o que ele reproduz).
fn with_own_process(mut audio: AudioRequest) -> AudioRequest {
    audio.exclude_pid = Some(std::process::id());
    audio
}

/// Aplicativos que estÃ£o tocando (ou tocaram) som, sem o prÃ³prio madscreen, para o seletor de Ã¡udio.
#[tauri::command]
async fn list_audio_apps() -> Result<Vec<AudioApp>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        madscreen_audio::list_audio_apps(Some(std::process::id())).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("falha interna: {e}"))?
}

/// Janelas e monitores capturÃ¡veis, com Ã­cone e miniatura, para o seletor do app.
#[tauri::command]
async fn list_sources(app: AppHandle) -> Result<SourceList, String> {
    window_guard::synchronize(&app)?;
    let guards = app.state::<window_guard::WindowGuard>().inner().clone();
    let epoch = guards
        .0
        .lock()
        .map_err(|_| "Não foi possível conferir as janelas.")?
        .epoch();
    let mut list = sources::list().await?;
    window_guard::synchronize(&app)?;
    let mut state = guards
        .0
        .lock()
        .map_err(|_| "Não foi possível conferir as janelas.")?;
    if !state.ready {
        return Err("A lista de janelas mudou ou o observador não iniciou. Atualize a seleção ou use o seletor do Windows.".into());
    }
    list.windows.retain_mut(|window| {
        window.selection_id = state.bind(window.hwnd, epoch);
        window.selection_id.is_some()
    });
    Ok(list)
}

/// Telas e limites do encoder (sem janelas nem miniaturas), para as opÃ§Ãµes de resoluÃ§Ã£o.
#[tauri::command]
async fn list_monitors() -> Result<SourceList, String> {
    sources::list_monitors().await
}

/// OpÃ§Ãµes de resoluÃ§Ã£o para uma fonte: sÃ³ as que cabem nela (sem ampliar), com as dimensÃµes reais
/// de saÃ­da (proporÃ§Ã£o mantida, reduzidas se passarem dos limites do encoder).
#[tauri::command]
fn output_options(
    width: u32,
    height: u32,
    fps: u32,
    limits: Option<EncoderLimits>,
) -> Vec<OutputOption> {
    resolution::output_options((width, height), f64::from(fps), limits.unwrap_or_default())
}

#[tauri::command]
fn stop_share(app: AppHandle, engine: State<'_, Engine>) {
    engine.stop(&app);
}

#[tauri::command]
fn set_share_privacy(
    app: AppHandle,
    engine: State<'_, Engine>,
    pause: Option<bool>,
    muted: Option<bool>,
) -> Result<(), String> {
    engine.privacy(&app, pause, muted)
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage(Engine::default())
        .manage(foreground::Foreground::default())
        .manage(window_guard::WindowGuard::default())
        .manage(miniplayer::MiniPlayer::default())
        .manage(renderer_recovery::RendererRecovery::default())
        .invoke_handler(tauri::generate_handler![
            take_renderer_recovery,
            set_software_video_decode,
            get_state,
            get_publisher_diagnostics,
            get_settings,
            hide_to_tray,
            prepare_update_install,
            save_settings,
            start_share_cloudflare,
            change_share,
            list_sources,
            list_audio_apps,
            list_monitors,
            output_options,
            stop_share,
            set_share_privacy,
            foreground::get_foreground_suggestion,
            miniplayer::prepare_miniplayer,
            miniplayer::control_miniplayer,
        ])
        .setup(|app| {
            #[cfg(windows)]
            migration::migrate(app.handle())?;
            let config = app
                .config()
                .app
                .windows
                .first()
                .ok_or("configuracao da janela principal ausente")?;
            foreground::start(app.handle());
            let builder = tauri::WebviewWindowBuilder::from_config(app.handle(), config)?;
            let popup_app = app.handle().clone();
            let builder = builder.on_new_window(move |url, features| {
                miniplayer::new_window(&popup_app, url, features)
            });
            #[cfg(windows)]
            let builder = builder.additional_browser_args(&video_playout::browser_args(
                settings::load(app.handle()).software_video_decode,
            ));
            let window = builder.build()?;
            #[cfg(windows)]
            renderer_recovery_sys::install(
                &window,
                app.state::<renderer_recovery::RendererRecovery>()
                    .inner()
                    .clone(),
            )?;
            tray::build(app.handle())?;
            if std::env::args().any(|arg| arg == "--tray") {
                let _ = window.hide();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if miniplayer::is_miniplayer(window.label()) {
                return;
            }
            // Fechar envia para a bandeja; minimizar mantém a janela na barra de tarefas.
            // O engine (se ao vivo) segue em outro processo nos dois casos.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if settings::load(window.app_handle()).tray_notice_seen {
                    let _ = window.hide();
                } else {
                    let _ = tauri::Emitter::emit(window, "tray-close-requested", ());
                }
            }
        })
        .build(tauri::generate_context!())
        .map(|app| {
            app.run(|handle, event| {
                if let tauri::RunEvent::Exit = event {
                    handle.state::<Engine>().shutdown();
                }
            });
        })
        .unwrap_or_else(|e| {
            eprintln!("falha ao iniciar o madscreen: {e}");
            std::process::exit(1);
        });
}
