//! Ensaio opt-in do supervisor com o sidecar real e duas fontes geradas/preflightadas.
use super::*;
use std::{fs::File, io::Write, sync::atomic::AtomicBool};

struct Cleanup(Engine);
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

#[test]
#[ignore = "requer duas janelas geradas, preflight de marcador e credenciais próprias via harness"]
fn real_sfu_quality_source_and_rollback() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::var("MADSCREEN_CHANGE_SMOKE_REQUEST")?);
    let input: serde_json::Value = serde_json::from_reader(File::open(&path)?)?;
    if input["preflightPassed"] != true {
        return Err("preflight não confirmado".into());
    }
    let request: StartRequest = serde_json::from_value(input["request"].clone())?;
    let alternate: SourceSpec = serde_json::from_value(input["alternate"].clone())?;
    let source_is_owned = match &request.source {
        SourceSpec::Window { title } => title.starts_with("madscreen-lab-source-"),
        SourceSpec::VerifiedWindow { hwnd, instance_id } => crate::sources::list_blocking(false)?
            .windows
            .iter()
            .any(|window| {
                window.hwnd == *hwnd
                    && window.instance_id.as_ref() == Some(instance_id)
                    && window.title.starts_with("madscreen-lab-source-")
            }),
        _ => false,
    };
    let alternate_is_owned = match &alternate {
        SourceSpec::Window { title } => title.starts_with("madscreen-lab-receiver-"),
        SourceSpec::VerifiedWindow { hwnd, instance_id } => crate::sources::list_blocking(false)?
            .windows
            .iter()
            .any(|window| {
                window.hwnd == *hwnd
                    && window.instance_id.as_ref() == Some(instance_id)
                    && window.title.starts_with("madscreen-lab-receiver-")
            }),
        _ => false,
    };
    if !source_is_owned || !alternate_is_owned {
        return Err("somente fontes próprias do laboratório".into());
    }
    let directory = path.parent().ok_or("pasta do ensaio ausente")?;
    let output = Arc::new(Mutex::new(File::create(
        directory.join("supervisor.jsonl"),
    )?));
    let failed = Arc::new(AtomicBool::new(false));
    let writer = output.clone();
    let write_failed = failed.clone();
    let notify: Listener = Arc::new(move |engine, _| {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        if let Ok(mut writer) = writer.lock() {
            let value = serde_json::json!({"atUnixMs": at, "state": engine.snapshot()});
            if serde_json::to_writer(&mut *writer, &value).is_err()
                || writer.write_all(b"\n").is_err()
            {
                write_failed.store(true, Ordering::Relaxed);
            }
        } else {
            write_failed.store(true, Ordering::Relaxed);
        }
    });
    let fixture = Cleanup(Engine::default());
    let engine = &fixture.0;
    engine.start_internal(&notify, request.clone(), None)?;
    let deadline = Instant::now() + START_GRACE;
    while engine.snapshot().phase != Phase::Live {
        if Instant::now() >= deadline {
            return Err("partida real não confirmada".into());
        }
        if engine.snapshot().phase == Phase::Idle {
            return Err("sidecar real encerrou antes de iniciar".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    let mut steps = vec![serde_json::json!({"name":"initial", "state":engine.snapshot()})];
    if let SourceSpec::VerifiedWindow { instance_id, .. } = &request.source {
        assert_eq!(
            engine.snapshot().source_instance_id.as_ref(),
            Some(instance_id)
        );
    }
    thread::sleep(Duration::from_secs(5));
    if std::env::var("MADSCREEN_POPOUT_SMOKE").is_ok_and(|value| value == "1") {
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut result = None;
        while Instant::now() < deadline {
            if let Ok(bytes) = std::fs::read(directory.join("popout-result.json")) {
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    result = Some(value);
                    break;
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        if result.as_ref().is_none_or(|value| value["ok"] != true) {
            return Err("Prova de sessão única do mini-player não confirmou".into());
        }
        engine.request_stop(&notify, true);
        let deadline = Instant::now() + STOP_GRACE + Duration::from_secs(3);
        while engine.snapshot().phase != Phase::Idle && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(engine.snapshot().phase, Phase::Idle);
        serde_json::to_writer_pretty(
            File::create(directory.join("native-summary.json"))?,
            &serde_json::json!({"ok":true,"steps":steps,"stopped":true}),
        )?;
        return Ok(());
    }
    if std::env::var("MADSCREEN_PRIVACY_SMOKE").is_ok_and(|value| value == "1") {
        use madscreen_ipc::privacy::VideoState;
        let initial_id = engine.snapshot().session_id;
        if std::env::var("MADSCREEN_PRIVACY_STATIC").is_ok_and(|value| value == "1") {
            std::fs::write(directory.join("freeze-source"), "freeze")?;
            thread::sleep(Duration::from_secs(2));
        }
        let network_mode =
            std::env::var("MADSCREEN_PRIVACY_NETWORK").is_ok_and(|value| value == "1");
        let drop_every: u32 = std::env::var("MADSCREEN_PRIVACY_DROP_EVERY")
            .ok()
            .map(|value| value.parse())
            .transpose()?
            .unwrap_or(1);
        if !(1..=100).contains(&drop_every) {
            return Err("Perda de laboratorio invalida".into());
        }
        if network_mode {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !directory.join("network-ready").exists() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(50));
            }
            if !directory.join("network-ready").exists() {
                return Err("Rota UDP não qualificada".into());
            }
        }
        for cycle in 0..if network_mode { 5 } else { 3 } {
            if network_mode {
                network_condition(directory, cycle * 2 + 1, drop_every)?;
            }
            let requested = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
            engine.privacy_internal(&notify, Some(true), None)?;
            wait_privacy(engine)?;
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
            steps.push(serde_json::json!({"name":"paused","cycle":cycle,"requestedAtUnixMs":requested,"ackAtUnixMs":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),"state":engine.snapshot()}));
            if network_mode && drop_every == 1 {
                thread::sleep(Duration::from_secs(1));
                network_condition(directory, cycle * 2 + 2, 0)?;
            }
            thread::sleep(Duration::from_secs(5));
            let audio_mode =
                std::env::var("MADSCREEN_PRIVACY_AUDIO").is_ok_and(|value| value == "1");
            if audio_mode {
                assert!(engine.snapshot().audio_active.is_some_and(|n| n > 0));
            }
            let mute_requested = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
            engine.privacy_internal(&notify, None, Some(true))?;
            wait_privacy(engine)?;
            assert!(engine.snapshot().audio_muted);
            if audio_mode {
                steps.push(serde_json::json!({"name":"audio-muted","cycle":cycle,"requestedAtUnixMs":mute_requested,"ackAtUnixMs":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),"state":engine.snapshot()}));
                thread::sleep(Duration::from_secs(2));
            }
            let unmute_requested = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
            engine.privacy_internal(&notify, None, Some(false))?;
            wait_privacy(engine)?;
            if audio_mode {
                steps.push(serde_json::json!({"name":"audio-unmuted","cycle":cycle,"requestedAtUnixMs":unmute_requested,"ackAtUnixMs":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),"state":engine.snapshot()}));
                thread::sleep(Duration::from_secs(2));
            }
            // Partial loss/delay/congestion stay active throughout paused
            // playback and both audio transitions; a complete cut restores
            // earlier so the receiver can observe any neutral/audio at all.
            if network_mode && drop_every > 1 {
                network_condition(directory, cycle * 2 + 2, 0)?;
            }
            let requested = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
            engine.privacy_internal(&notify, Some(false), None)?;
            wait_privacy(engine)?;
            assert_eq!(engine.snapshot().privacy_video, Some(VideoState::Active));
            assert_eq!(engine.snapshot().session_id, initial_id);
            steps.push(serde_json::json!({"name":"resumed","cycle":cycle,"requestedAtUnixMs":requested,"ackAtUnixMs":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),"state":engine.snapshot()}));
            thread::sleep(Duration::from_secs(3));
        }
        engine.privacy_internal(&notify, Some(true), None)?;
        engine.request_stop(&notify, true);
        let deadline = Instant::now() + STOP_GRACE + Duration::from_secs(3);
        while engine.snapshot().phase != Phase::Idle && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(engine.snapshot().phase, Phase::Idle);
        serde_json::to_writer_pretty(
            File::create(directory.join("native-summary.json"))?,
            &serde_json::json!({"ok":!failed.load(Ordering::Relaxed),"steps":steps,"stopped":true}),
        )?;
        return Ok(());
    }
    if !engine.change_internal(
        &notify,
        Quality {
            height: 1080,
            fps: 30,
        },
        None,
        None,
    )? {
        return Err("troca de qualidade cancelada".into());
    }
    steps.push(serde_json::json!({"name":"quality", "state":engine.snapshot()}));
    if let SourceSpec::VerifiedWindow { instance_id, .. } = &request.source {
        assert_eq!(
            engine.snapshot().source_instance_id.as_ref(),
            Some(instance_id)
        );
    }
    thread::sleep(Duration::from_secs(5));
    if !engine.change_internal(
        &notify,
        Quality { height: 0, fps: 60 },
        Some(alternate.clone()),
        None,
    )? {
        return Err("troca de fonte cancelada".into());
    }
    steps.push(serde_json::json!({"name":"source", "state":engine.snapshot()}));
    if let SourceSpec::VerifiedWindow { instance_id, .. } = &alternate {
        assert_eq!(
            engine.snapshot().source_instance_id.as_ref(),
            Some(instance_id)
        );
    }
    thread::sleep(Duration::from_secs(5));
    let nonce = request.source.clone();
    let missing = match nonce {
        SourceSpec::Window { title } => SourceSpec::Window {
            title: format!("{title}-missing"),
        },
        SourceSpec::VerifiedWindow { hwnd, .. } => SourceSpec::VerifiedWindow {
            hwnd,
            instance_id: "0".repeat(64),
        },
        _ => return Err("fonte inválida".into()),
    };
    let rollback = engine.change_internal(
        &notify,
        Quality {
            height: 720,
            fps: 30,
        },
        Some(missing),
        None,
    );
    if !rollback.is_err_and(|message| message.contains("anterior foi restaurada")) {
        return Err("restauração real não confirmada".into());
    }
    steps.push(serde_json::json!({"name":"rollback", "state":engine.snapshot()}));
    if let SourceSpec::VerifiedWindow { instance_id, .. } = &alternate {
        assert_eq!(
            engine.snapshot().source_instance_id.as_ref(),
            Some(instance_id)
        );
    }
    thread::sleep(Duration::from_secs(5));
    engine.request_stop(&notify, true);
    let deadline = Instant::now() + STOP_GRACE + Duration::from_secs(3);
    while engine.snapshot().phase != Phase::Idle {
        if Instant::now() >= deadline {
            return Err("parada real não confirmou".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    if failed.load(Ordering::Relaxed) {
        return Err("falha ao registrar estados".into());
    }
    serde_json::to_writer_pretty(
        File::create(directory.join("native-summary.json"))?,
        &serde_json::json!({"ok":true,"steps":steps,"stopped":true}),
    )?;
    Ok(())
}

use std::time::{SystemTime, UNIX_EPOCH};

fn wait_privacy(engine: &Engine) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(7);
    while engine.snapshot().privacy_pending && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if engine.snapshot().phase != Phase::Live || engine.snapshot().privacy_pending {
        return Err(format!(
            "Privacidade não confirmou: {:?}",
            engine.snapshot()
        ));
    }
    Ok(())
}

fn network_condition(
    directory: &std::path::Path,
    sequence: u32,
    drop_every: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    serde_json::to_writer(
        File::create(directory.join("network-condition.json"))?,
        &serde_json::json!({"sequence":sequence,"dropEvery":drop_every}),
    )?;
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Ok(file) = File::open(directory.join("network-applied.json")) {
            if let Ok(value) = serde_json::from_reader::<_, serde_json::Value>(file) {
                if value["sequence"] == sequence {
                    return Ok(());
                }
            }
        }
        if Instant::now() >= deadline {
            return Err("Condicionador UDP não confirmou operação".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}
