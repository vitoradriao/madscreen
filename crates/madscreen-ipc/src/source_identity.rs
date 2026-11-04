//! Identidades locais opacas; caminhos/títulos não precisam sair da enumeração.
use sha2::{Digest, Sha256};
pub fn application_id(path: &str) -> String {
    format!("{:x}", Sha256::digest(path.to_lowercase().as_bytes()))
}
pub fn window_instance(
    app: &str,
    pid: u32,
    creation: (u32, u32),
    hwnd: i64,
    class: &str,
    title: &str,
) -> String {
    format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{app}:{pid}:{}:{}:{hwnd}:{class}:{title}",
                creation.0, creation.1
            )
            .as_bytes()
        )
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_changes_for_recycled_process_handle_and_changed_window() {
        let app = application_id("C:\\Apps\\Game.exe");
        assert_eq!(app, application_id("c:\\apps\\game.EXE"));
        assert_ne!(app, application_id("C:\\Other\\Game.exe"));
        let instance = window_instance(&app, 1, (2, 3), 4, "class", "private title");
        for other in [
            window_instance(&app, 2, (2, 3), 4, "class", "private title"),
            window_instance(&app, 1, (2, 4), 4, "class", "private title"),
            window_instance(&app, 1, (2, 3), 5, "class", "private title"),
            window_instance(&app, 1, (2, 3), 4, "other", "private title"),
            window_instance(&app, 1, (2, 3), 4, "class", "other title"),
        ] {
            assert_ne!(instance, other);
        }
        assert_eq!(instance.len(), 64);
        assert!(!instance.contains("private"));
    }
}
