//! Eventos oficiais de acessibilidade fora de contexto; sem DLL/injeção ou captura.
#![allow(unsafe_code)]
use madscreen_ipc::source_identity::{application_id, window_instance};
use std::{
    cell::RefCell,
    sync::{Mutex, Weak},
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::{
    Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::*,
        UI::WindowsAndMessaging::*,
    },
    core::PWSTR,
};
struct Context {
    foreground: Weak<Mutex<Option<super::foreground::ForegroundWindow>>>,
    guards: Weak<Mutex<super::window_guard::Registry>>,
    engine: super::engine::Engine,
}
thread_local! { static CONTEXT:RefCell<Option<Context>>=const {RefCell::new(None)}; }

unsafe extern "system" fn event_callback(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: windows::Win32::Foundation::HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    CONTEXT.with(|context| {
        let context = context.borrow();
        let Some(context) = context.as_ref() else {
            return;
        };
        if event == EVENT_OBJECT_DESTROY && object == OBJID_WINDOW.0 && child == 0 {
            if let Some(state) = context.foreground.upgrade()
                && let Ok(mut slot) = state.lock()
                && slot
                    .as_ref()
                    .is_some_and(|window| window.hwnd == hwnd.0 as i64)
            {
                *slot = None;
            }
            if let Some(guards) = context.guards.upgrade()
                && let Ok(mut guards) = guards.lock()
            {
                guards.destroyed(hwnd.0 as i64);
            }
            context.engine.on_window_destroyed(hwnd.0 as i64);
        } else if event == EVENT_SYSTEM_FOREGROUND {
            let observation = observe_window(hwnd);
            if let Some(state) = context.foreground.upgrade()
                && let Ok(mut slot) = state.lock()
            {
                super::foreground::observe(&mut slot, observation);
            }
        }
    });
}

pub fn watch(
    foreground: Weak<Mutex<Option<super::foreground::ForegroundWindow>>>,
    guards: Weak<Mutex<super::window_guard::Registry>>,
    engine: super::engine::Engine,
) {
    CONTEXT.with(|context| {
        *context.borrow_mut() = Some(Context {
            foreground: foreground.clone(),
            guards: guards.clone(),
            engine,
        })
    });
    // SAFETY: callbacks estáticos executam só nesta thread com message pump. Sem módulo
    // DLL e OUTOFCONTEXT: nenhum código é carregado no processo observado.
    unsafe {
        let focus = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(event_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        let destroy = SetWinEventHook(
            EVENT_OBJECT_DESTROY,
            EVENT_OBJECT_DESTROY,
            None,
            Some(event_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        let ready = !focus.is_invalid() && !destroy.is_invalid();
        let (sync_tx, sync_rx) = std::sync::mpsc::sync_channel::<std::sync::mpsc::Sender<()>>(8);
        if let Some(state) = guards.upgrade()
            && let Ok(mut state) = state.lock()
        {
            state.ready = ready;
            if ready {
                state.synchronizer = Some(sync_tx);
            }
        }
        if ready {
            if let Some(state) = foreground.upgrade()
                && let Ok(mut state) = state.lock()
            {
                super::foreground::observe(&mut state, current());
            }
            while foreground.strong_count() > 0 && guards.strong_count() > 0 {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                while let Ok(ack) = sync_rx.try_recv() {
                    let _ = ack.send(());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        if !focus.is_invalid() {
            let _ = UnhookWinEvent(focus);
        }
        if !destroy.is_invalid() {
            let _ = UnhookWinEvent(destroy);
        }
        if let Some(state) = guards.upgrade()
            && let Ok(mut state) = state.lock()
        {
            state.ready = false;
            state.synchronizer = None;
        }
    }
    CONTEXT.with(|context| *context.borrow_mut() = None);
}

pub fn current() -> super::foreground::Observation {
    // SAFETY: consulta de HWND do primeiro plano.
    observe_window(unsafe { GetForegroundWindow() })
}
fn observe_window(hwnd: windows::Win32::Foundation::HWND) -> super::foreground::Observation {
    use super::foreground::Observation;
    // SAFETY: consulta somente leitura de HWND e PID atuais.
    let (hwnd, pid) = unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (hwnd, pid)
    };
    if pid == std::process::id() {
        return Observation::Own;
    }
    Observation::External(snapshot(hwnd, pid))
}

fn snapshot(
    hwnd: windows::Win32::Foundation::HWND,
    pid: u32,
) -> Option<super::foreground::ForegroundWindow> {
    if hwnd.is_invalid() || pid == 0 {
        return None;
    }
    // SAFETY: buffers delimitados e processo somente consultado, fechado em todos os caminhos.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path = [0u16; 1024];
        let mut size = path.len() as u32;
        let queried = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_FORMAT(0),
            PWSTR(path.as_mut_ptr()),
            &mut size,
        );
        let mut created = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let times = GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user);
        let _ = CloseHandle(process);
        queried.ok()?;
        times.ok()?;
        let app = application_id(&String::from_utf16_lossy(&path[..size as usize]));
        let mut title = [0u16; 1024];
        let count = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        let mut class = [0u16; 128];
        let class_count = GetClassNameW(hwnd, &mut class).max(0) as usize;
        if count == 0 || count == title.len() - 1 {
            return None;
        }
        Some(super::foreground::ForegroundWindow {
            hwnd: hwnd.0 as i64,
            instance_id: window_instance(
                &app,
                pid,
                (created.dwHighDateTime, created.dwLowDateTime),
                hwnd.0 as i64,
                &String::from_utf16_lossy(&class[..class_count]),
                &String::from_utf16_lossy(&title[..count]),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use std::{
        process::{Child, Command},
        time::{Duration, Instant},
    };

    struct OwnWindow(Child);
    impl Drop for OwnWindow {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    #[ignore = "subprocesso com janela Win32 própria para interação de bancada"]
    fn native_focus_child() -> Result<(), Box<dyn std::error::Error>> {
        let Some(directory) = std::env::var_os("MADSCREEN_FOCUS_CHILD") else {
            return Ok(());
        };
        let directory = std::path::PathBuf::from(directory);
        let name = std::env::var("MADSCREEN_FOCUS_INDEX")?;
        let title: Vec<u16> = format!("madscreen-focus-proof-{name}\0")
            .encode_utf16()
            .collect();
        // SAFETY: janelas pertencem exclusivamente a este subprocesso de teste;
        // strings/buffers válidos durante as chamadas e DestroyWindow ao encerrar.
        let left = 80 + name.parse::<i32>()? * 510;
        unsafe {
            let create = || -> windows::core::Result<windows::Win32::Foundation::HWND> {
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    windows::core::w!("STATIC"),
                    windows::core::PCWSTR(title.as_ptr()),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    left,
                    80,
                    480,
                    180,
                    None,
                    None,
                    None,
                    None,
                )?;
                let _button = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    windows::core::w!("BUTTON"),
                    windows::core::w!("Confirmar foco da bancada"),
                    WS_CHILD | WS_VISIBLE,
                    20,
                    30,
                    360,
                    60,
                    Some(hwnd),
                    None,
                    None,
                    None,
                )?;
                Ok(hwnd)
            };
            let mut hwnd = create()?;
            let mut recreated = false;
            std::fs::write(
                directory.join(format!("ready-{name}")),
                (hwnd.0 as i64).to_string(),
            )?;
            let deadline = Instant::now() + Duration::from_secs(240);
            while Instant::now() < deadline && !directory.join("stop").exists() {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                if directory.join(format!("recreate-{name}")).exists() && !recreated {
                    DestroyWindow(hwnd)?;
                    hwnd = create()?;
                    recreated = true;
                    std::fs::write(
                        directory.join(format!("recreated-{name}")),
                        (hwnd.0 as i64).to_string(),
                    )?;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            DestroyWindow(hwnd)?;
        }
        Ok(())
    }

    #[test]
    #[ignore = "requer cliques via computer-use nas duas janelas geradas"]
    fn real_foreground_events_and_window_lifetime() -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::Arc;
        let directory = std::path::PathBuf::from(std::env::var("MADSCREEN_FOCUS_PROOF")?);
        std::fs::create_dir_all(&directory)?;
        let state = Arc::new(Mutex::new(None));
        let guards = Arc::new(Mutex::new(super::super::window_guard::Registry::default()));
        let weak = Arc::downgrade(&state);
        let weak_guards = Arc::downgrade(&guards);
        let observer = std::thread::spawn(move || {
            watch(weak, weak_guards, super::super::engine::Engine::default())
        });
        let mut children = Vec::new();
        let mut handles = Vec::new();
        let executable = std::env::current_exe()?;
        let second_executable = directory.join("second-application.exe");
        std::fs::copy(&executable, &second_executable)?;
        for index in 0..2 {
            children.push(OwnWindow(
                Command::new(if index == 0 {
                    &executable
                } else {
                    &second_executable
                })
                .args([
                    "foreground_sys::tests::native_focus_child",
                    "--exact",
                    "--ignored",
                ])
                .env("MADSCREEN_FOCUS_CHILD", &directory)
                .env("MADSCREEN_FOCUS_INDEX", index.to_string())
                .spawn()?,
            ));
            let deadline = Instant::now() + Duration::from_secs(15);
            let hwnd = loop {
                if let Ok(value) = std::fs::read_to_string(directory.join(format!("ready-{index}")))
                    && let Ok(value) = value.parse::<i64>()
                {
                    break value;
                }
                if Instant::now() >= deadline {
                    return Err("janela de bancada não abriu".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            handles.push(hwnd);
        }
        let listed = crate::sources::list_blocking(false)?;
        let first = listed
            .windows
            .iter()
            .find(|window| window.hwnd == handles[0])
            .ok_or("primeira janela não enumerada")?;
        let second = listed
            .windows
            .iter()
            .find(|window| window.hwnd == handles[1])
            .ok_or("segundo executável não enumerado")?;
        assert_eq!(
            first.app_id,
            Some(application_id(&executable.to_string_lossy()))
        );
        assert_eq!(
            second.app_id,
            Some(application_id(&second_executable.to_string_lossy()))
        );
        assert_ne!(first.app_id, second.app_id);
        // Primeiro plano deve vir de entrada real, não de evento artificial.
        for index in [0, 1, 0] {
            std::fs::write(directory.join("expected"), index.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(100);
            loop {
                if state
                    .lock()
                    .map_err(|_| "lock")?
                    .as_ref()
                    .is_some_and(|window| window.hwnd == handles[index])
                {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("foco real não observado".into());
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            std::fs::write(
                directory.join(format!(
                    "observed-{index}-{}",
                    if index == 0 {
                        "first-or-return"
                    } else {
                        "second"
                    }
                )),
                "ok",
            )?;
            // Exige sair da primeira janela antes de aceitar seu retorno.
            if index == 0 {
                std::thread::sleep(Duration::from_millis(150));
            }
        }
        let ticket = {
            let mut registry = guards.lock().map_err(|_| "lock")?;
            let epoch = registry.epoch();
            registry
                .bind(handles[0], epoch)
                .ok_or("observador não está pronto")?
        };
        std::fs::write(directory.join("recreate-0"), "recreate")?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while guards.lock().map_err(|_| "lock")?.valid(handles[0], ticket) {
            if Instant::now() >= deadline {
                return Err("destruição não invalidou ticket".into());
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let recreated = loop {
            if let Ok(value) = std::fs::read_to_string(directory.join("recreated-0"))
                && let Ok(value) = value.parse::<i64>()
            {
                break value;
            }
            if Instant::now() >= deadline {
                return Err("janela não foi recriada no mesmo processo".into());
            }
            std::thread::sleep(Duration::from_millis(30));
        };
        let recreated_window = snapshot(
            windows::Win32::Foundation::HWND(recreated as *mut std::ffi::c_void),
            children[0].0.id(),
        )
        .ok_or("janela recriada inválida")?;
        assert_eq!(recreated_window.hwnd, recreated);
        let second_ticket = {
            let mut registry = guards.lock().map_err(|_| "lock")?;
            let epoch = registry.epoch();
            registry
                .bind(recreated, epoch)
                .ok_or("ticket recriado ausente")?
        };
        assert_ne!(ticket, second_ticket);
        std::fs::write(directory.join("stop"), "stop")?;
        drop(children);
        drop(state);
        drop(guards);
        observer.join().map_err(|_| "observador falhou")?;
        std::fs::write(
            directory.join("result.json"),
            serde_json::to_string(
                &serde_json::json!({"focusSequence":[0,1,0],"realInput":true,"distinctExecutablePaths":true,"enumerationMatched":true,"destroyInvalidated":true,"recreatedSameProcessClassTitle":true,"actualHandleReused":recreated==handles[0],"freshTicket":true,"ok":true}),
            )?,
        )?;
        Ok(())
    }

    #[test]
    #[ignore = "abre somente duas janelas próprias e consulta identidade sem exigir foco"]
    fn inspects_two_real_external_windows_and_rejects_closed_window()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let mut children = Vec::new();
        let mut handles = Vec::new();
        for index in 0..2 {
            let path = directory.path().join(format!("window-{index}.txt"));
            let escaped = path.display().to_string().replace('\'', "''");
            let script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; $form = New-Object System.Windows.Forms.Form; $form.Text = 'madscreen-foreground-lab-{index}'; $form.Width = 320; $form.Height = 160; $form.Add_Shown({{[IO.File]::WriteAllText('{escaped}', $form.Handle.ToInt64().ToString()); $form.Activate()}}); [void]$form.ShowDialog()"
            );
            let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            children.push(OwnWindow(
                Command::new("powershell.exe")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .args([
                        "-NoProfile",
                        "-STA",
                        "-WindowStyle",
                        "Hidden",
                        "-EncodedCommand",
                        &encoded,
                    ])
                    .spawn()?,
            ));
            let deadline = Instant::now() + Duration::from_secs(15);
            let hwnd = loop {
                if let Ok(value) = std::fs::read_to_string(&path)
                    && let Ok(value) = value.parse::<i64>()
                {
                    break value;
                }
                if Instant::now() >= deadline {
                    return Err("janela própria não abriu".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            handles.push(hwnd);
        }
        let mut identities = Vec::new();
        for (hwnd, child) in handles.iter().zip(&children) {
            let handle = windows::Win32::Foundation::HWND(*hwnd as *mut std::ffi::c_void);
            let window =
                snapshot(handle, child.0.id()).ok_or("identidade de janela própria ausente")?;
            assert_eq!(window.hwnd, *hwnd);
            assert_eq!(window.instance_id.len(), 64);
            identities.push(window.instance_id);
        }
        assert_ne!(identities[0], identities[1]);
        let mut child = children.remove(0);
        child.0.kill()?;
        child.0.wait()?;
        let handle = windows::Win32::Foundation::HWND(handles[0] as *mut std::ffi::c_void);
        assert!(snapshot(handle, child.0.id()).is_none());
        Ok(())
    }
}
