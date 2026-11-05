//! Prova opt-in de sessão única entre janelas, sem usar conta/app pessoal.
#[path = "../miniplayer.rs"]
mod miniplayer;
#[path = "../renderer_recovery.rs"]
mod renderer_recovery;
#[cfg(windows)]
#[allow(unsafe_code)]
#[path = "../renderer_recovery_sys.rs"]
mod renderer_recovery_sys;
#[path = "../video_playout.rs"]
mod video_playout;
use std::{path::PathBuf, time::Duration};
use tauri::{Manager, webview::NewWindowResponse};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base: tauri::Url = std::env::var("MADSCREEN_LAB_BASE")?.parse()?;
    if base.scheme() != "http" || base.host_str() != Some("127.0.0.1") || base.path() != "/" {
        return Err("laboratório exige HTTP loopback".into());
    }
    let profile = PathBuf::from(std::env::var("MADSCREEN_LAB_PROFILE")?);
    if !profile.is_absolute() || !profile.is_dir() {
        return Err("perfil do laboratório inválido".into());
    }
    let directory = profile
        .parent()
        .ok_or("pasta do laboratório ausente")?
        .to_path_buf();
    let product_mode = std::env::var("MADSCREEN_POPOUT_PRODUCT").is_ok_and(|v| v == "1");
    tauri::Builder::default()
        .manage(miniplayer::MiniPlayer::default())
        .manage(renderer_recovery::RendererRecovery::default())
        .setup(move |app| {
            // Generated WGC sources occupy the other monitor. Keep the receiver
            // visible here; an occluded renderer is not a presentation baseline.
            let monitor = app.primary_monitor()?.ok_or("monitor principal ausente")?;
            let scale = monitor.scale_factor();
            let x = f64::from(monitor.position().x) / scale;
            let y = f64::from(monitor.position().y) / scale;
            let child_app = app.handle().clone();
            let browser_args = format!(
                "--autoplay-policy=no-user-gesture-required {}",
                video_playout::browser_args(false)
            );
            let recovery_directory = directory.clone();
            let window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(base.join("watch")?),
            )
            .title("MadScreen — prova de mini-player")
            .inner_size(900., 520.)
            .position(x, y)
            .data_directory(profile.clone())
            .additional_browser_args(&browser_args)
            .on_page_load(move |window, payload| {
                if payload.event() == tauri::webview::PageLoadEvent::Finished
                    && window.state::<renderer_recovery::RendererRecovery>().take()
                {
                    miniplayer::destroy_all(window.app_handle());
                    let _ = std::fs::write(
                        recovery_directory.join("renderer-recovered.json"),
                        "{\"nativeReloadConfirmed\":true}",
                    );
                }
            })
            .on_new_window(move |url, features| {
                if product_mode {
                    return miniplayer::new_window(&child_app, url, features);
                }
                if url.as_str() != "about:blank"
                    || child_app.get_webview_window("popout-child").is_some()
                {
                    return NewWindowResponse::Deny;
                }
                match tauri::WebviewWindowBuilder::new(
                    &child_app,
                    "popout-child",
                    tauri::WebviewUrl::External(url),
                )
                .window_features(features)
                .title("MadScreen — mini-player de teste")
                .inner_size(480., 270.)
                .min_inner_size(240., 135.)
                .position(x + 100., y + 100.)
                .always_on_top(true)
                .on_navigation(|url| url.as_str() == "about:blank")
                .build()
                {
                    Ok(window) => NewWindowResponse::Create { window },
                    Err(error) => {
                        eprintln!("Falha ao abrir janela de teste: {error}");
                        NewWindowResponse::Deny
                    }
                }
            })
            .build()?;
            #[cfg(windows)]
            renderer_recovery_sys::install(
                &window,
                app.state::<renderer_recovery::RendererRecovery>()
                    .inner()
                    .clone(),
            )?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut sequence = 0;
                loop {
                    std::thread::sleep(Duration::from_millis(50));
                    let Ok(bytes) = std::fs::read(directory.join("popout-command.json")) else {
                        continue;
                    };
                    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                        continue;
                    };
                    let Some(next) = value["sequence"].as_u64().filter(|next| *next > sequence)
                    else {
                        continue;
                    };
                    sequence = next;
                    let action = value["action"].as_str().unwrap_or("").to_owned();
                    let arguments = value["args"].clone();
                    let target = handle.clone();
                    let directory = directory.clone();
                    let _ = handle.run_on_main_thread(move || {
                        let main = target.get_webview_window("main");
                        let result: Result<(), String> = match (action.as_str(), main) {
                            ("prepare_miniplayer", _) => miniplayer::prepare_miniplayer(
                                target.clone(),
                                arguments["token"].as_str().unwrap_or("").to_owned(),
                            ),
                            ("control_miniplayer", _) => miniplayer::control_miniplayer(
                                target.clone(),
                                arguments["action"].as_str().unwrap_or("").to_owned(),
                                arguments["value"].as_bool(),
                                arguments["token"].as_str().unwrap_or("").to_owned(),
                            ),
                            ("minimize", Some(window)) => {
                                window.minimize().map_err(|e| e.to_string())
                            }
                            ("restore", Some(window)) => window
                                .unminimize()
                                .and_then(|()| window.show())
                                .and_then(|()| window.set_focus())
                                .map_err(|e| e.to_string()),
                            ("close-popup", _) => (if product_mode {
                                miniplayer::any_window(&target)
                            } else {
                                target.get_webview_window("popout-child")
                            })
                            .map_or(Ok(()), |window| window.destroy())
                            .map_err(|e| e.to_string()),
                            _ => Err("ação de laboratório inválida".into()),
                        };
                        if let Ok(bytes) = serde_json::to_vec(
                            &serde_json::json!({"sequence":next,"ok":result.is_ok()}),
                        ) {
                            let _ = std::fs::write(directory.join("popout-applied.json"), bytes);
                        }
                    });
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}
