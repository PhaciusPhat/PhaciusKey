//! Cross-platform settings, persisted as a small TOML file.
//!
//! Replaces the macOS-only `UserDefaults`/`AppSettings.swift` with a plain
//! config file that works identically on every OS.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use vnkey_core::{Config as CoreConfig, InputMethod, TonePlacementMode};

/// Input method convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Telex,
    Vni,
}

/// Tone-mark placement strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Modern,
    Classic,
}

/// User-facing settings. Owned by the shell, mapped into [`CoreConfig`] for the
/// engine on every change.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Master on/off for Vietnamese typing.
    pub enabled: bool,
    /// Telex or VNI.
    pub method: Method,
    /// Modern (`hòa`) or Classic (`hoà`) tone placement.
    pub placement: Placement,
    /// Restore raw keystrokes for non-Vietnamese (e.g. English) words.
    pub auto_restore: bool,
    /// Download and install new releases automatically. Set false to go back to
    /// being notified only.
    pub auto_update: bool,
    /// Global shortcut that flips `enabled`, e.g. "ctrl+shift+v". Parsed with
    /// [`parse_shortcut`]; an unparseable string simply disables the shortcut.
    pub toggle_shortcut: String,
    /// Register the app to launch at login (macOS LaunchAgent). Opt-in: the
    /// user turns it on from the tray menu, it is never assumed.
    pub start_at_login: bool,
    /// Apps (by name, case-insensitive) in which Vietnamese typing is off —
    /// keystrokes pass through untouched while one of these is focused.
    pub disabled_apps: Vec<String>,
    /// Version that last ran. Compared against the running build at startup to
    /// notice that a self-update happened, so the user can be told.
    pub last_seen_version: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            method: Method::Telex,
            placement: Placement::Modern,
            auto_restore: true,
            auto_update: true,
            toggle_shortcut: "ctrl+shift+v".into(),
            start_at_login: false,
            disabled_apps: Vec::new(),
            last_seen_version: None,
        }
    }
}

impl Settings {
    /// `~/.config/vnkey/config.toml` (or the OS equivalent).
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vnkey")
            .join("config.toml")
    }

    /// Load settings from disk, falling back to defaults on any error.
    pub fn load() -> Self {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist settings to disk. Errors are logged, never fatal.
    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match toml::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    eprintln!("[vnkey] failed to save settings to {}: {e}", path.display());
                }
            }
            Err(e) => eprintln!("[vnkey] failed to serialize settings: {e}"),
        }
    }

    /// Whether Vietnamese typing is turned off for `app` (case-insensitive).
    pub fn disabled_for(&self, app: Option<&str>) -> bool {
        match app {
            Some(app) => self
                .disabled_apps
                .iter()
                .any(|d| d.eq_ignore_ascii_case(app)),
            None => false,
        }
    }

    /// Map the shell settings onto the engine's config type.
    pub fn to_core(&self) -> CoreConfig {
        CoreConfig {
            method: match self.method {
                Method::Telex => InputMethod::Telex,
                Method::Vni => InputMethod::Vni,
            },
            placement: match self.placement {
                Placement::Modern => TonePlacementMode::Modern,
                Placement::Classic => TonePlacementMode::Classic,
            },
            enabled: self.enabled,
            auto_restore: self.auto_restore,
        }
    }
}

/// A parsed toggle shortcut: a set of modifiers plus one main key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub cmd: bool,
    /// Lowercase ASCII letter/digit, or ' ' for the space bar.
    pub key: char,
}

/// Parse a "ctrl+shift+v"-style shortcut string. `None` when the string is not
/// a recognizable modifiers+key combination — the shortcut is then simply off,
/// never a startup error, since the string comes from a hand-editable file.
pub fn parse_shortcut(s: &str) -> Option<Shortcut> {
    let mut sc = Shortcut { ctrl: false, shift: false, alt: false, cmd: false, key: '\0' };
    let mut tokens = s.split('+').map(|t| t.trim().to_ascii_lowercase()).peekable();

    while let Some(token) = tokens.next() {
        let is_last = tokens.peek().is_none();
        match token.as_str() {
            "ctrl" | "control" => sc.ctrl = true,
            "shift" => sc.shift = true,
            "alt" | "option" | "opt" => sc.alt = true,
            "cmd" | "command" | "super" | "meta" => sc.cmd = true,
            "space" if is_last => sc.key = ' ',
            key if is_last && key.len() == 1 => {
                let c = key.chars().next()?;
                if !c.is_ascii_alphanumeric() {
                    return None;
                }
                sc.key = c;
            }
            _ => return None,
        }
    }

    // A bare letter with no modifier would fire on ordinary typing; require at
    // least one modifier and a main key.
    let has_modifier = sc.ctrl || sc.shift || sc.alt || sc.cmd;
    (sc.key != '\0' && has_modifier).then_some(sc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_default_shortcut() {
        assert_eq!(
            parse_shortcut("ctrl+shift+v"),
            Some(Shortcut { ctrl: true, shift: true, alt: false, cmd: false, key: 'v' })
        );
    }

    #[test]
    fn accepts_modifier_synonyms_whitespace_and_case() {
        assert_eq!(
            parse_shortcut(" Control + Option + Space "),
            Some(Shortcut { ctrl: true, shift: false, alt: true, cmd: false, key: ' ' })
        );
        assert_eq!(
            parse_shortcut("CMD+2"),
            Some(Shortcut { ctrl: false, shift: false, alt: false, cmd: true, key: '2' })
        );
    }

    #[test]
    fn rejects_garbage_and_modifierless_keys() {
        assert_eq!(parse_shortcut(""), None);
        assert_eq!(parse_shortcut("v"), None); // would fire on plain typing
        assert_eq!(parse_shortcut("ctrl+shift"), None); // no main key
        assert_eq!(parse_shortcut("ctrl+vv"), None);
        assert_eq!(parse_shortcut("ctrl+ß"), None);
        assert_eq!(parse_shortcut("hyper+v"), None);
    }

    #[test]
    fn disabled_for_is_case_insensitive() {
        let settings = Settings { disabled_apps: vec!["Terminal".into()], ..Default::default() };
        assert!(settings.disabled_for(Some("terminal")));
        assert!(!settings.disabled_for(Some("Safari")));
        assert!(!settings.disabled_for(None));
    }
}
