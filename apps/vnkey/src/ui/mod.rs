mod ipc;
mod panel;
mod payload;
mod settings;

pub use ipc::{apply_ipc, WindowAction};
pub use panel::Panel;
pub use settings::SettingsWindow;

const THEME: &str = include_str!("assets/theme.css");

/// Which window a message came from. The two surfaces speak the same commands —
/// `close_window` means "hide me" to both — so the origin is what decides which
/// window a window action lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Panel,
    Settings,
}

/// Assembles one page from the shared theme and a surface's own parts. Keeping
/// the theme in a single file is what makes the surfaces look like one
/// application rather than two that were styled to resemble each other.
fn document(css: &str, body: &str, script: &str) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <style>{THEME}\n{css}</style></head><body>{body}\
         <script>{script}</script></body></html>"
    )
}

/// Which convention the window chrome should follow.
fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else {
        "windows"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_carries_the_theme_and_both_parts() {
        let page = document(".x{}", "<div id=\"body\"></div>", "var js = 1;");
        assert!(page.contains("--lacquer"), "theme is missing");
        assert!(page.contains(".x{}"), "surface css is missing");
        assert!(page.contains("id=\"body\""), "body is missing");
        assert!(page.contains("var js = 1;"), "script is missing");
    }
}
