//! Janelas relacionadas ao opener; a sessão de mídia continua no frontend principal.
//! Uma por transmissão assistida (até [`MAX_WINDOWS`]), cada uma com seu token e posição.
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{
    AppHandle, Emitter, Manager, WindowEvent,
    webview::{NewWindowFeatures, NewWindowResponse},
};

/// O layout de Assistir mostra no máximo duas lives; cada uma pode ir para o mini-player.
pub const MAX_WINDOWS: usize = 2;
const LABEL_PREFIX: &str = "miniplayer-";

#[derive(Default)]
pub struct MiniPlayer {
    /// Token → vaga (índice da posição lembrada). Reservado no prepare, liberado no fechamento.
    tokens: Mutex<HashMap<String, usize>>,
    geometry: Mutex<[Option<Geometry>; MAX_WINDOWS]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn label(token: &str) -> String {
    format!("{LABEL_PREFIX}{token}")
}

pub fn is_miniplayer(label: &str) -> bool {
    label.starts_with(LABEL_PREFIX)
}

/// Qualquer mini-player aberto (o laboratório fecha um por vez).
#[allow(dead_code)] // só o binário popout-lab usa; o app fecha por token.
pub fn any_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.webview_windows()
        .into_iter()
        .find(|(label, _)| is_miniplayer(label))
        .map(|(_, window)| window)
}

/// Recuperação do renderer: nenhum mini-player sobrevive órfão do palco que sumiu.
pub fn destroy_all(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if is_miniplayer(&label) {
            let _ = window.destroy();
        }
    }
    if let Ok(mut tokens) = app.state::<MiniPlayer>().tokens.lock() {
        tokens.clear();
    }
}

fn release(app: &AppHandle, token: &str) {
    if let Ok(mut tokens) = app.state::<MiniPlayer>().tokens.lock() {
        tokens.remove(token);
    }
}

fn slot_of(app: &AppHandle, token: &str) -> Option<usize> {
    app.state::<MiniPlayer>()
        .tokens
        .lock()
        .ok()
        .and_then(|tokens| tokens.get(token).copied())
}

fn fit_geometry(saved: Option<Geometry>, areas: &[Geometry], slot: usize) -> Geometry {
    // Sem posição lembrada, a segunda janela nasce abaixo da primeira em vez de cobri-la.
    let default = Geometry {
        x: 0.,
        y: 380. * slot as f64,
        width: 640.,
        height: 360.,
    };
    let saved = saved
        .filter(|g| [g.x, g.y, g.width, g.height].iter().all(|v| v.is_finite()))
        .unwrap_or(default);
    let area = areas
        .iter()
        .find(|area| {
            saved.x + saved.width / 2. >= area.x
                && saved.y + saved.height / 2. >= area.y
                && saved.x + saved.width / 2. < area.x + area.width
                && saved.y + saved.height / 2. < area.y + area.height
        })
        .or_else(|| areas.first())
        .copied()
        .unwrap_or(Geometry {
            x: 0.,
            y: 0.,
            width: 800.,
            height: 600.,
        });
    let width = saved.width.max(320.).min(area.width.max(1.));
    let height = saved.height.max(200.).min(area.height.max(1.));
    Geometry {
        x: saved.x.clamp(area.x, area.x + (area.width - width).max(0.)),
        y: saved
            .y
            .clamp(area.y, area.y + (area.height - height).max(0.)),
        width,
        height,
    }
}

fn remember(app: &AppHandle, window: &tauri::WebviewWindow, slot: usize) {
    if slot >= MAX_WINDOWS
        || window.is_fullscreen().unwrap_or(true)
        || window.is_minimized().unwrap_or(true)
    {
        return;
    }
    if let (Ok(position), Ok(size), Ok(scale)) = (
        window.outer_position(),
        window.inner_size(),
        window.scale_factor(),
    ) && let Ok(mut saved) = app.state::<MiniPlayer>().geometry.lock()
    {
        saved[slot] = Some(Geometry {
            x: f64::from(position.x) / scale,
            y: f64::from(position.y) / scale,
            width: f64::from(size.width) / scale,
            height: f64::from(size.height) / scale,
        });
    }
}

#[tauri::command]
pub fn prepare_miniplayer(app: AppHandle, token: String) -> Result<(), String> {
    if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Identificador de janela inválido.".into());
    }
    let state = app.state::<MiniPlayer>();
    let mut tokens = state
        .tokens
        .lock()
        .map_err(|_| "Controle da janela indisponível")?;
    if tokens.contains_key(&token) {
        return Ok(());
    }
    let slot = (0..MAX_WINDOWS)
        .find(|slot| !tokens.values().any(|used| used == slot))
        .ok_or("Já existem dois mini-players abertos.")?;
    tokens.insert(token, slot);
    Ok(())
}

#[tauri::command]
pub fn control_miniplayer(
    app: AppHandle,
    action: String,
    value: Option<bool>,
    token: String,
) -> Result<(), String> {
    let Some(slot) = slot_of(&app, &token) else {
        return if matches!(action.as_str(), "close" | "return") {
            Ok(())
        } else {
            Err("Operação de janela antiga.".into())
        };
    };
    let window = app.get_webview_window(&label(&token));
    if action == "return" {
        if let Some(window) = window {
            remember(&app, &window, slot);
            window.destroy().map_err(|error| error.to_string())?;
        }
        release(&app, &token);
        if let Some(window) = app.get_webview_window("main") {
            window
                .unminimize()
                .and_then(|()| window.show())
                .and_then(|()| window.set_focus())
                .map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    let Some(window) = window else {
        return if action == "close" {
            release(&app, &token);
            Ok(())
        } else {
            Err("Mini-player encerrado.".into())
        };
    };
    match action.as_str() {
        "close" => {
            remember(&app, &window, slot);
            release(&app, &token);
            window.destroy()
        }
        "show" => window.show().and_then(|()| window.set_focus()),
        "top" => {
            let wanted = value.ok_or("Informe o estado de Sempre no topo.")?;
            window
                .set_always_on_top(wanted)
                .map_err(|e| e.to_string())?;
            if window.is_always_on_top().map_err(|e| e.to_string())? != wanted {
                return Err("Sempre no topo não confirmou.".into());
            }
            Ok(())
        }
        "fullscreen" => window.set_fullscreen(value.ok_or("Informe o estado de tela cheia.")?),
        _ => return Err("Ação de janela inválida.".into()),
    }
    .map_err(|error| format!("Não foi possível controlar o mini-player: {error}"))
}

pub fn new_window(
    app: &AppHandle,
    url: tauri::Url,
    features: NewWindowFeatures,
) -> NewWindowResponse<tauri::Wry> {
    if url.scheme() != "about" || url.path() != "blank" {
        return NewWindowResponse::Deny;
    }
    let Some(token) = url.fragment().map(str::to_owned) else {
        return NewWindowResponse::Deny;
    };
    // Só abre token reservado por prepare_miniplayer e ainda sem janela.
    let Some(slot) = slot_of(app, &token) else {
        return NewWindowResponse::Deny;
    };
    if app.get_webview_window(&label(&token)).is_some() {
        return NewWindowResponse::Deny;
    }
    let owner = app.clone();
    let event_token = token.clone();
    let mut monitors = app.available_monitors().unwrap_or_default();
    // Default/retorno de monitor ausente prefere a tela onde está a janela principal.
    if let Some(main_monitor) = app
        .get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
        && let Some(index) = monitors
            .iter()
            .position(|m| m.position() == main_monitor.position())
    {
        monitors.swap(0, index);
    }
    let areas: Vec<_> = monitors
        .iter()
        .map(|monitor| {
            let area = monitor.work_area();
            let scale = monitor.scale_factor();
            Geometry {
                x: f64::from(area.position.x) / scale,
                y: f64::from(area.position.y) / scale,
                width: f64::from(area.size.width) / scale,
                height: f64::from(area.size.height) / scale,
            }
        })
        .collect();
    let saved = app
        .state::<MiniPlayer>()
        .geometry
        .lock()
        .ok()
        .and_then(|g| g[slot]);
    let geometry = fit_geometry(saved, &areas, slot);
    let builder =
        tauri::WebviewWindowBuilder::new(app, label(&token), tauri::WebviewUrl::External(url))
            .window_features(features)
            .title("MadScreen — mini-player")
            .inner_size(geometry.width, geometry.height)
            .position(geometry.x, geometry.y)
            .min_inner_size(320., 200.)
            .always_on_top(true)
            .visible(false)
            .on_navigation(|url| url.scheme() == "about" && url.path() == "blank")
            .on_new_window(|_, _| NewWindowResponse::Deny);
    match builder.build() {
        Ok(window) => {
            let event_window = window.clone();
            window.on_window_event(move |event| match event {
                WindowEvent::CloseRequested { api, .. } => {
                    remember(&owner, &event_window, slot);
                    api.prevent_close();
                    let _ = owner.emit_to("main", "miniplayer-close-requested", &event_token);
                    // Se o frontend não responder, destruir não deixa janela órfã.
                    let window = event_window.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        let _ = window.destroy();
                    });
                }
                WindowEvent::Destroyed => {
                    release(&owner, &event_token);
                    let _ = owner.emit_to("main", "miniplayer-closed", &event_token);
                }
                WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                    remember(&owner, &event_window, slot)
                }
                _ => {}
            });
            NewWindowResponse::Create { window }
        }
        Err(error) => {
            release(app, &token);
            tracing::warn!(%error, "falha ao criar mini-player");
            NewWindowResponse::Deny
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_returns_to_visible_area_after_monitor_removal_or_dpi_change() {
        let area = Geometry {
            x: 0.,
            y: 0.,
            width: 1280.,
            height: 720.,
        };
        let fitted = fit_geometry(
            Some(Geometry {
                x: -2500.,
                y: 3000.,
                width: 4000.,
                height: 2000.,
            }),
            &[area],
            0,
        );
        assert_eq!(fitted, area);
        let fitted = fit_geometry(
            Some(Geometry {
                x: 1200.,
                y: 700.,
                width: 100.,
                height: 100.,
            }),
            &[area],
            0,
        );
        assert_eq!(
            fitted,
            Geometry {
                x: 960.,
                y: 520.,
                width: 320.,
                height: 200.
            }
        );
        let second = Geometry {
            x: -1280.,
            y: 0.,
            width: 1280.,
            height: 720.,
        };
        let saved = Geometry {
            x: -800.,
            y: 100.,
            width: 480.,
            height: 300.,
        };
        assert_eq!(fit_geometry(Some(saved), &[area, second], 0), saved);
        assert_eq!(
            fit_geometry(
                Some(Geometry {
                    x: f64::NAN,
                    ..saved
                }),
                &[area],
                0
            )
            .width,
            640.
        );
    }

    #[test]
    fn second_slot_does_not_open_on_top_of_the_first() {
        let area = Geometry {
            x: 0.,
            y: 0.,
            width: 1280.,
            height: 720.,
        };
        let first = fit_geometry(None, &[area], 0);
        let second = fit_geometry(None, &[area], 1);
        assert_eq!(first.y, 0.);
        assert_eq!(second.y, 360.);
        assert!(second.y >= first.y + first.height);
    }
}
