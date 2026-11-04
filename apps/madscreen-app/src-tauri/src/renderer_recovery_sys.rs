//! Windows COM registration, on the WebView's own apartment.
use super::renderer_recovery::RendererRecovery;
use webview2_com::{Microsoft::Web::WebView2::Win32::*, ProcessFailedEventHandler};

pub fn install(window: &tauri::WebviewWindow, recovery: RendererRecovery) -> tauri::Result<()> {
    window.with_webview(move |platform| {
        // SAFETY: controller is alive, accessed on its WebView UI apartment.
        let core = match unsafe { platform.controller().CoreWebView2() } {
            Ok(core) => core,
            Err(error) => {
                tracing::error!(%error, "não foi possível registrar recuperação da interface");
                return;
            }
        };
        let handler = ProcessFailedEventHandler::create(Box::new(move |sender, args| {
            if let (Some(sender), Some(args)) = (sender, args) {
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                // SAFETY: event args live for this callback; writable output on the same apartment.
                unsafe {
                    args.ProcessFailedKind(&mut kind)?;
                }
                if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
                    if recovery.request(std::time::Instant::now()) {
                        tracing::warn!(
                            "renderer encerrado; recuperando interface com reprodução interrompida"
                        );
                        // SAFETY: sender remains a live WebView COM control after its renderer exits.
                        if let Err(error) = unsafe { sender.Reload() } {
                            tracing::error!(%error, "falha ao recarregar interface");
                        }
                    } else {
                        tracing::error!(
                            "renderer falhou repetidamente; recuperação automática suspensa"
                        );
                    }
                }
            }
            Ok(())
        }));
        let mut token = 0;
        // SAFETY: valid COM handler and output token. The control owns this single
        // registration for its lifetime; the closure holds no control/app reference,
        // so there is no COM cycle. Renderer reloads do not register another handler.
        if let Err(error) = unsafe { core.add_ProcessFailed(&handler, &mut token) } {
            tracing::error!(%error, "falha ao registrar recuperação da interface");
        }
    })
}
