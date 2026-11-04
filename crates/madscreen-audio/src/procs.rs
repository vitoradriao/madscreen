//! Tabela de processos e regras puras para escolher o PID raiz de um aplicativo.
//!
//! `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE` captura o PID alvo e todos os filhos.
//! Navegadores e apps Electron tocam o som num processo filho (o servico de audio do Chrome),
//! entao o alvo certo e o processo raiz do mesmo executavel.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Uma linha da tabela de processos (Toolhelp32 no Windows).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub parent: u32,
    /// Nome do executavel, ex.: `chrome.exe`.
    pub exe: String,
}

/// Sessao de audio bruta, como o Windows a enumera.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionInfo {
    pub pid: u32,
    pub active: bool,
    pub peak: f32,
    pub display_name: String,
}

/// Aplicativo que esta (ou esteve) tocando som.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioApp {
    /// PID raiz da arvore a capturar.
    pub pid: u32,
    /// Executavel, ex.: `chrome.exe`.
    pub process_name: String,
    /// Nome amigavel para a interface.
    pub display_name: String,
    /// Sessao em estado ativo (esta tocando agora).
    pub active: bool,
    /// Pico recente (0..1) das sessoes do aplicativo.
    pub peak: f32,
}

const MAX_DEPTH: usize = 64;

fn index(table: &[ProcInfo]) -> HashMap<u32, &ProcInfo> {
    table.iter().map(|p| (p.pid, p)).collect()
}

/// Sobe pelos pais enquanto o pai tiver o mesmo executavel. Sem entrada na tabela devolve `pid`.
pub fn root_pid(table: &[ProcInfo], pid: u32) -> u32 {
    let by_pid = index(table);
    let mut current = pid;
    for _ in 0..MAX_DEPTH {
        let Some(info) = by_pid.get(&current) else {
            break;
        };
        match by_pid.get(&info.parent) {
            Some(parent) if parent.pid != current && parent.exe.eq_ignore_ascii_case(&info.exe) => {
                current = parent.pid;
            }
            _ => break,
        }
    }
    current
}

/// `pid` e o proprio `ancestor` ou um de seus descendentes.
pub fn in_tree(table: &[ProcInfo], pid: u32, ancestor: u32) -> bool {
    let by_pid = index(table);
    let mut current = pid;
    for _ in 0..MAX_DEPTH {
        if current == ancestor {
            return true;
        }
        match by_pid.get(&current) {
            Some(info) if info.parent != current && info.parent != 0 => current = info.parent,
            _ => return false,
        }
    }
    false
}

fn exe_stem(exe: &str) -> String {
    exe.strip_suffix(".exe")
        .or_else(|| exe.strip_suffix(".EXE"))
        .unwrap_or(exe)
        .to_owned()
}

fn usable_display_name(name: &str) -> Option<&str> {
    let name = name.trim();
    // Nomes `@%SystemRoot%\...,-123` sao referencias a recursos, nao texto.
    (!name.is_empty() && !name.starts_with('@')).then_some(name)
}

/// Agrupa sessoes por aplicativo (PID raiz). Ignora PID 0, `exclude_pid` e sua arvore.
/// Ativos primeiro, depois por nome.
pub fn group_apps(
    table: &[ProcInfo],
    sessions: &[SessionInfo],
    exclude_pid: Option<u32>,
) -> Vec<AudioApp> {
    let by_pid = index(table);
    let mut apps: Vec<AudioApp> = Vec::new();
    for session in sessions {
        if session.pid == 0 {
            continue;
        }
        if exclude_pid.is_some_and(|excluded| in_tree(table, session.pid, excluded)) {
            continue;
        }
        let Some(info) = by_pid.get(&session.pid) else {
            // Processo ja encerrado: nao ha o que capturar.
            continue;
        };
        let root = root_pid(table, session.pid);
        let exe = by_pid
            .get(&root)
            .map_or_else(|| info.exe.clone(), |p| p.exe.clone());
        if let Some(app) = apps.iter_mut().find(|app| app.pid == root) {
            app.active |= session.active;
            app.peak = app.peak.max(session.peak);
            continue;
        }
        let display_name = usable_display_name(&session.display_name)
            .map_or_else(|| exe_stem(&exe), str::to_owned);
        apps.push(AudioApp {
            pid: root,
            process_name: exe,
            display_name,
            active: session.active,
            peak: session.peak,
        });
    }
    apps.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then_with(|| {
                a.display_name
                    .to_lowercase()
                    .cmp(&b.display_name.to_lowercase())
            })
            .then(a.pid.cmp(&b.pid))
    });
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, parent: u32, exe: &str) -> ProcInfo {
        ProcInfo {
            pid,
            parent,
            exe: exe.into(),
        }
    }

    fn table() -> Vec<ProcInfo> {
        vec![
            p(4, 0, "System"),
            p(100, 4, "explorer.exe"),
            p(200, 100, "chrome.exe"),
            p(201, 200, "chrome.exe"),
            p(202, 201, "chrome.exe"),
            p(300, 100, "madscreen-app.exe"),
            p(301, 300, "msedgewebview2.exe"),
            p(302, 300, "madscreen-encode-test.exe"),
            p(400, 100, "spotify.exe"),
            p(401, 400, "Spotify.exe"),
        ]
    }

    fn s(pid: u32, active: bool, peak: f32, name: &str) -> SessionInfo {
        SessionInfo {
            pid,
            active,
            peak,
            display_name: name.into(),
        }
    }

    #[test]
    fn root_stops_at_different_executable() {
        let t = table();
        assert_eq!(root_pid(&t, 202), 200);
        assert_eq!(root_pid(&t, 200), 200);
        assert_eq!(root_pid(&t, 401), 400);
        assert_eq!(root_pid(&t, 301), 301);
        assert_eq!(root_pid(&t, 999), 999);
    }

    #[test]
    fn root_survives_cycles() {
        let t = vec![p(1, 2, "a.exe"), p(2, 1, "a.exe")];
        let root = root_pid(&t, 1);
        assert!(root == 1 || root == 2);
    }

    #[test]
    fn tree_membership() {
        let t = table();
        assert!(in_tree(&t, 302, 300));
        assert!(in_tree(&t, 300, 300));
        assert!(!in_tree(&t, 200, 300));
    }

    #[test]
    fn groups_by_root_and_merges() {
        let t = table();
        let sessions = [
            s(202, false, 0.0, ""),
            s(200, true, 0.4, ""),
            s(401, true, 0.1, "Spotify Premium"),
        ];
        let apps = group_apps(&t, &sessions, None);
        assert_eq!(apps.len(), 2);
        let chrome = apps.iter().find(|a| a.pid == 200).expect("chrome");
        assert!(chrome.active);
        assert!((chrome.peak - 0.4).abs() < 1e-6);
        assert_eq!(chrome.display_name, "chrome");
        let spotify = apps.iter().find(|a| a.pid == 400).expect("spotify");
        assert_eq!(spotify.display_name, "Spotify Premium");
    }

    #[test]
    fn skips_system_excluded_tree_and_dead_processes() {
        let t = table();
        let sessions = [
            s(0, true, 1.0, "Sons do sistema"),
            s(301, true, 0.5, ""),
            s(302, true, 0.5, ""),
            s(999, true, 0.5, ""),
            s(200, false, 0.0, "@%SystemRoot%\\foo.dll,-100"),
        ];
        let apps = group_apps(&t, &sessions, Some(300));
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].pid, 200);
        assert_eq!(apps[0].display_name, "chrome");
    }

    #[test]
    fn active_first_then_alphabetical() {
        let t = table();
        let sessions = [
            s(400, false, 0.0, "Zeta"),
            s(200, false, 0.0, "Alfa"),
            s(300, true, 0.2, "Mid"),
        ];
        let names: Vec<_> = group_apps(&t, &sessions, None)
            .into_iter()
            .map(|a| a.display_name)
            .collect();
        assert_eq!(names, ["Mid", "Alfa", "Zeta"]);
    }
}
