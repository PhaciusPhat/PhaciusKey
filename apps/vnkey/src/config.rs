//! Cross-platform settings, persisted as a small TOML file.
//!
//! Replaces the macOS-only `UserDefaults`/`AppSettings.swift` with a plain
//! config file that works identically on every OS.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    /// EVKey-style per-app memory: when on, the Vietnamese toggle applies to
    /// the focused app only, and switching apps restores each app's last state.
    pub per_app_mode: bool,
    /// Remembered on/off state per app (lowercased name), written by the toggle
    /// while `per_app_mode` is on. Apps never toggled fall back to [`enabled`].
    ///
    /// [`enabled`]: Settings::enabled
    pub app_modes: BTreeMap<String, bool>,
    /// Text-expansion macros: word typed → text it becomes at the word
    /// boundary ("vd" → "ví dụ"). Matched against the on-screen word,
    /// case-sensitively.
    pub macros: BTreeMap<String, String>,
    /// Apps (by name, case-insensitive) that drop rapid synthetic keystrokes —
    /// injection pauses briefly between events while one of these is focused.
    pub slow_apps: Vec<String>,
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
            per_app_mode: false,
            app_modes: BTreeMap::new(),
            macros: BTreeMap::new(),
            slow_apps: Vec::new(),
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

    /// Whether Vietnamese typing is effectively on while `app` is focused.
    ///
    /// Precedence: the exclusion list is a hard off; then, in per-app mode, the
    /// app's remembered state; finally the global toggle as the default.
    pub fn vietnamese_on(&self, app: Option<&str>) -> bool {
        if self.disabled_for(app) {
            return false;
        }
        if self.per_app_mode {
            if let Some(&remembered) =
                app.and_then(|a| self.app_modes.get(&a.to_ascii_lowercase()))
            {
                return remembered;
            }
        }
        self.enabled
    }

    /// Remember `on` as `app`'s Vietnamese state. Turning an app *on* also
    /// lifts a hard exclusion — otherwise the toggle would appear dead there.
    pub fn set_app_mode(&mut self, app: &str, on: bool) {
        if on {
            self.disabled_apps.retain(|d| !d.eq_ignore_ascii_case(app));
        }
        self.app_modes.insert(app.to_ascii_lowercase(), on);
    }

    /// Map the shell settings onto the engine's config type, resolving the
    /// effective on/off state for the app currently receiving keystrokes.
    pub fn to_core(&self, app: Option<&str>) -> CoreConfig {
        CoreConfig {
            method: match self.method {
                Method::Telex => InputMethod::Telex,
                Method::Vni => InputMethod::Vni,
            },
            placement: match self.placement {
                Placement::Modern => TonePlacementMode::Modern,
                Placement::Classic => TonePlacementMode::Classic,
            },
            enabled: self.vietnamese_on(app),
            auto_restore: self.auto_restore,
            macros: self.macros.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
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

    #[test]
    fn per_app_memory_beats_global_and_exclusion_beats_memory() {
        let mut settings = Settings { per_app_mode: true, ..Default::default() };
        // No memory yet: falls back to the global toggle.
        assert!(settings.vietnamese_on(Some("Safari")));
        settings.enabled = false;
        assert!(!settings.vietnamese_on(Some("Safari")));

        // Remembered state wins over the global default, case-insensitively.
        settings.set_app_mode("Safari", true);
        assert!(settings.vietnamese_on(Some("safari")));
        assert!(!settings.vietnamese_on(Some("Terminal")));

        // A hard exclusion outranks the memory…
        settings.disabled_apps.push("Safari".into());
        assert!(!settings.vietnamese_on(Some("Safari")));
        // …until an explicit "turn on" lifts it.
        settings.set_app_mode("safari", true);
        assert!(settings.vietnamese_on(Some("Safari")));
        assert!(settings.disabled_apps.is_empty());
    }

    #[test]
    fn memory_is_inert_while_per_app_mode_is_off() {
        let mut settings = Settings::default();
        settings.set_app_mode("Safari", false);
        assert!(settings.vietnamese_on(Some("Safari"))); // global ON wins
    }
}
