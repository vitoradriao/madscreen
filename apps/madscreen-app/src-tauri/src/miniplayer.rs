//! Janela relacionada ao opener; a sessão de mídia continua no frontend principal.
use std::sync::Mutex;
use tauri::{
    AppHandle, Emitter, Manager, WindowEvent,
    webview::{NewWindowFeatures, NewWindowResponse},
};

#[derive(Default)]
pub struct MiniPlayer {
    pending: Mutex<Option<String>>,
    geometry: Mutex<Option<Geometry>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn fit_geometry(saved: Option<Geometry>, areas: &[Geometry]) -> Geometry {
    let default = Geometry {
        x: 0.,
        y: 0.,
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

fn remember(app: &AppHandle, window: &tauri::WebviewWindow) {
    if window.is_fullscreen().unwrap_or(true) || window.is_minimized().unwrap_or(true) {
        return;
    }
    if let (Ok(position), Ok(size), Ok(scale)) = (
        window.outer_position(),
        window.inner_size(),
        window.scale_factor(),
    ) && let Ok(mut saved) = app.state::<MiniPlayer>().geometry.lock()
    {
        *saved = Some(Geometry {
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
    if app.get_webview_window("miniplayer").is_some() {
        return Err("Já existe um mini-player aberto.".into());
    }
    *app.state::<MiniPlayer>()
        .pending
        .lock()
        .map_err(|_| "Controle da janela indisponível")? = Some(token);
    Ok(())
}

#[tauri::command]
pub fn control_miniplayer(
    app: AppHandle,
    action: String,
    value: Option<bool>,
    token: String,
) -> Result<(), String> {
    let current = app.state::<MiniPlayer>();
    let expected = current
        .pending
        .lock()
        .map_err(|_| "Controle da janela indisponível")?;
    if expected.as_deref() != Some(token.as_str()) {
        return Err("Operação de janela antiga.".into());
    }
    drop(expected);
    if action == "return" {
        if let Some(window) = app.get_webview_window("miniplayer") {
            remember(&app, &window);
            window.destroy().map_err(|error| error.to_string())?;
        }
        if let Some(window) = app.get_webview_window("main") {
            window
                .unminimize()
                .and_then(|()| window.show())
                .and_then(|()| window.set_focus())
                .map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    let Some(window) = app.get_webview_window("miniplayer") else {
        return if action == "close" {
            Ok(())
        } else {
            Err("Mini-player encerrado.".into())
        };
    };
    match action.as_str() {
        "close" => {
            remember(&app, &window);
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
    if url.scheme() != "about"
        || url.path() != "blank"
        || app.get_webview_window("miniplayer").is_some()
    {
        return NewWindowResponse::Deny;
    }
    let token = {
        let state = app.state::<MiniPlayer>();
        let Ok(pending) = state.pending.lock() else {
            return NewWindowResponse::Deny;
        };
        if pending.as_deref() != url.fragment() {
            return NewWindowResponse::Deny;
        }
        pending.clone()
    };
    let Some(token) = token else {
        return NewWindowResponse::Deny;
    };
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
        .and_then(|g| *g);
    let geometry = fit_geometry(saved, &areas);
    let builder =
        tauri::WebviewWindowBuilder::new(app, "miniplayer", tauri::WebviewUrl::External(url))
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
                    remember(&owner, &event_window);
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
                    let _ = owner.emit_to("main", "miniplayer-closed", &event_token);
                }
                WindowEvent::Moved(_) | WindowEvent::Resized(_) => remember(&owner, &event_window),
                _ => {}
            });
            NewWindowResponse::Create { window }
        }
        Err(error) => {
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
        assert_eq!(fit_geometry(Some(saved), &[area, second]), saved);
        assert_eq!(
            fit_geometry(
                Some(Geometry {
                    x: f64::NAN,
                    ..saved
                }),
                &[area]
            )
            .width,
            640.
        );
    }
}
