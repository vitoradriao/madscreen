//! Ícone da bandeja: abrir, parar o compartilhamento e sair.
use crate::engine::{Phase, ShareState};
use tauri::{
    AppHandle, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const TRAY_ID: &str = "main";
const ID_OPEN: &str = "open";
const ID_STOP: &str = "stop";
const ID_QUIT: &str = "quit";

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    // O frontend pede o estado ao ganhar foco; este evento cobre o caso de janela já montada.
    let state = app.state::<crate::engine::Engine>().snapshot();
    let _ = tauri::Emitter::emit(app, "share-state", &state);
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, ID_OPEN, "Abrir madscreen", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, ID_STOP, "Parar compartilhamento", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, ID_QUIT, "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &stop, &PredefinedMenuItem::separator(app)?, &quit],
    )?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("madscreen")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            ID_OPEN => show_main(app),
            ID_STOP => app.state::<crate::engine::Engine>().stop(app),
            ID_QUIT => {
                app.state::<crate::engine::Engine>().shutdown();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    app.manage(StopItem(stop));
    Ok(())
}

struct StopItem(MenuItem<tauri::Wry>);

pub fn update(app: &AppHandle, state: &ShareState) {
    let live =
        state.changing || matches!(state.phase, Phase::Live | Phase::Starting | Phase::Picking);
    if let Some(item) = app.try_state::<StopItem>() {
        let _ = item.0.set_enabled(live);
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let tip = match state.phase {
            _ if state.changing => "madscreen — aplicando alterações".into(),
            Phase::Live if state.publishing && state.publishing_connected == Some(true) => format!(
                "madscreen — ao vivo {}x{} @ {:.0} FPS",
                state.width, state.height, state.fps
            ),
            Phase::Live if !state.publishing => "madscreen — captura local, sem transmissão".into(),
            Phase::Live if state.publishing_connected == Some(false) => {
                "madscreen — reconectando transmissão".into()
            }
            Phase::Live => "madscreen — verificando conexão".into(),
            Phase::Stopping => "madscreen — parando".into(),
            Phase::Idle => "madscreen".to_string(),
            _ => "madscreen — iniciando".to_string(),
        };
        let _ = tray.set_tooltip(Some(tip));
    }
}
