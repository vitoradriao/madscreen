//! Janelas isoladas de ensaio. Nao carrega supervisor, updater, plugins ou contas do app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "../video_playout.rs"]
mod video_playout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("MADSCREEN_LAB_BASE")?;
    let url: tauri::Url = base.parse()?;
    if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") || url.path() != "/" {
        return Err("laboratório exige servidor HTTP loopback".into());
    }
    let nonce = std::env::var("MADSCREEN_LAB_NONCE")?;
    if nonce.len() != 16 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("identificador de ensaio inválido".into());
    }
    let profile = std::path::PathBuf::from(std::env::var("MADSCREEN_LAB_PROFILE")?);
    if !profile.is_absolute() || !profile.is_dir() {
        return Err("perfil do laboratório deve ser uma pasta absoluta existente".into());
    }
    // Opt-in de ensaio: permite fonte 1080p sem alterar as janelas do produto.
    let source_size = match std::env::var("MADSCREEN_LAB_SOURCE_SIZE") {
        Ok(value) => {
            let (width, height) = value.split_once('x').ok_or("tamanho da fonte inválido")?;
            let width: u32 = width.parse()?;
            let height: u32 = height.parse()?;
            if !(640..=2560).contains(&width) || !(480..=1440).contains(&height) {
                return Err("tamanho da fonte fora dos limites do laboratório".into());
            }
            (f64::from(width), f64::from(height))
        }
        Err(_) => (1280., 720.),
    };
    let generator_pair =
        std::env::var("MADSCREEN_LAB_GENERATOR_PAIR").is_ok_and(|value| value == "1");
    let software_decode = std::env::var("MADSCREEN_LAB_SOFTWARE_DECODE").is_ok_and(|v| v == "1");
    let no_throttle = std::env::var("MADSCREEN_LAB_NO_THROTTLE").is_ok_and(|v| v == "1");
    let mut browser_args = if no_throttle {
        "--autoplay-policy=no-user-gesture-required --disable-features=WebRtcHideLocalIpsWithMdns --disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling"
    } else if software_decode {
        "--autoplay-policy=no-user-gesture-required --disable-features=WebRtcHideLocalIpsWithMdns --disable-accelerated-video-decode"
    } else {
        "--autoplay-policy=no-user-gesture-required --disable-features=WebRtcHideLocalIpsWithMdns"
    }.to_owned();
    if std::env::var("MADSCREEN_LAB_PACED_DECODE").is_ok_and(|v| v == "1") {
        // Ensaio causal: caminho de baixa latencia com cadencia mesmo quando
        // ha backlog; mantem o decoder acelerado e nao altera o publicador.
        browser_args.push(' ');
        browser_args.push_str(&video_playout::browser_args(false));
    }
    if let Ok(port) = std::env::var("MADSCREEN_LAB_CDP_PORT") {
        let port: u16 = port.parse()?;
        if port < 1024 {
            return Err("porta de diagnóstico inválida".into());
        }
        browser_args.push_str(&format!(
            " --remote-debugging-port={port} --remote-debugging-address=127.0.0.1 --enable-logging --log-file=\"{}\" --vmodule=rtc_video_decoder_adapter=3,d3d11_video_decoder=3,mojo_video_decoder=3 --webrtc-event-logging=\"{}\"",
            profile.join("browser-debug.private.log").display(),
            profile.join("rtc-events").display()
        ));
    }
    if std::env::var("MADSCREEN_LAB_NETLOG").is_ok_and(|v| v == "1") {
        browser_args.push_str(&format!(
            " --log-net-log=\"{}\" --net-log-capture-mode=Everything",
            profile.join("network.private.json").display()
        ));
    }
    tauri::Builder::default()
        .setup(move |app| {
            let primary = app.primary_monitor()?;
            let monitor = app
                .available_monitors()?
                .into_iter()
                .find(|monitor| {
                    primary
                        .as_ref()
                        .is_some_and(|p| p.position() != monitor.position())
                })
                .ok_or("laboratório exige segunda tela conectada")?;
            let scale = monitor.scale_factor();
            let origin_x = f64::from(monitor.position().x) / scale;
            let origin_y = f64::from(monitor.position().y) / scale;
            for (label, route, width, height, x) in [
                ("source", "source", source_size.0, source_size.1, 0.),
                (
                    "receiver",
                    "receiver",
                    if generator_pair { source_size.0 } else { 960. },
                    if generator_pair { source_size.1 } else { 650. },
                    960.,
                ),
            ] {
                tauri::WebviewWindowBuilder::new(
                    app,
                    label,
                    tauri::WebviewUrl::External(url.join(route)?),
                )
                .title(format!("madscreen-lab-{label}-{nonce}"))
                .inner_size(width, height)
                .position(origin_x + x, origin_y)
                .decorations(label != "source" && !generator_pair)
                .always_on_top(true)
                .focused(label == "source")
                .data_directory(profile.clone())
                .additional_browser_args(&browser_args)
                .build()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}
