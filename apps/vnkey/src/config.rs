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
    /// Telex: a `w` that has no vowel to put a horn on types `ư` ("thw" →
    /// "thư"). Standard in Unikey/OpenKey, so on by default.
    pub standalone_w: bool,
    /// Quick Telex: doubled consonants at word start expand ("cc" → "ch").
    pub quick_telex: bool,
    /// Quick start consonants: `f` → "ph", `j` → "gi", `w` → "qu" at word start.
    pub quick_start_consonant: bool,
    /// Quick end consonants: after a vowel, `g` → "ng", `h` → "nh", `k` → "ch".
    pub quick_end_consonant: bool,
    /// Capitalize the first letter of each sentence as it is typed.
    pub auto_capitalize: bool,
    /// Download and install new releases automatically. Set false to go back to
    /// being notified only.
    pub auto_update: bool,
    /// Global shortcut that flips `enabled`, e.g. "ctrl+shift+v". Parsed with
    /// [`parse_shortcut`]; an unparseable string simply disables the shortcut.
    pub toggle_shortcut: String,
    /// Register the app to launch at login. Opt-in: the user turns it on from
    /// the tray menu, it is never assumed. macOS owns the real state — this is
    /// the user's request, reconciled against `SMAppService` at startup.
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
    /// Master switch for text expansion. Off hands the engine no macros at
    /// all while keeping the list intact, so the feature can be parked without
    /// losing what the user defined.
    pub macros_enabled: bool,
    /// Text-expansion macros: word typed → text it becomes at the word
    /// boundary ("vd" → "ví dụ"). Matched against the on-screen word,
    /// case-sensitively.
    pub macros: BTreeMap<String, String>,
    /// Apps (by name, case-insensitive) that drop rapid synthetic keystrokes —
    /// injection pauses briefly between events while one of these is focused.
    pub slow_apps: Vec<String>,
    /// Apps (by name, case-insensitive) whose inline autocomplete eats the
    /// first injected Backspace, doubling letters ("dđ" instead of "đ").
    /// Injection sends an invisible sentinel character first while one of
    /// these is focused. Opt-in — see `with_autocomplete_fix` in the macOS
    /// backend for why it is not simply on everywhere.
    pub autocomplete_fix_apps: Vec<String>,
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
            standalone_w: true,
            quick_telex: false,
            quick_start_consonant: false,
            quick_end_consonant: false,
            auto_capitalize: false,
            auto_update: true,
            toggle_shortcut: "ctrl+shift+v".into(),
            start_at_login: false,
            disabled_apps: Vec::new(),
            per_app_mode: false,
            app_modes: BTreeMap::new(),
            macros_enabled: true,
            macros: BTreeMap::new(),
            slow_apps: Vec::new(),
            autocomplete_fix_apps: Vec::new(),
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
            macros: match self.macros_enabled {
                true => self.macros.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                false => Default::default(),
            },
            standalone_w: self.standalone_w,
            quick_telex: self.quick_telex,
            quick_start_consonant: self.quick_start_consonant,
            quick_end_consonant: self.quick_end_consonant,
            auto_capitalize: self.auto_capitalize,
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

/// Whether `trigger` can actually work as a macro trigger.
///
/// Macros are matched against one committed word, so a trigger containing
/// whitespace could never fire — it would already have been committed at the
/// space.
pub fn valid_macro_trigger(trigger: &str) -> bool {
    !trigger.is_empty() && !trigger.contains(char::is_whitespace)
}

/// Serialize macros for export: a plain JSON object of trigger → expansion.
///
/// JSON rather than the app's own TOML because an expansion may contain
/// newlines, quotes and tabs, and this way their escaping is somebody else's
/// solved problem. Pretty-printed so the file stays hand-editable.
pub fn macro_export_json(macros: &BTreeMap<String, String>) -> String {
    // A BTreeMap serializes in key order, so re-exporting an unchanged list
    // produces an identical file rather than a spurious diff.
    serde_json::to_string_pretty(macros).unwrap_or_else(|_| "{}".to_string())
}

/// How an import changed the existing list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportOutcome {
    pub added: usize,
    pub updated: usize,
}

/// Parse an exported macro file into the entries worth keeping, along with how
/// many rows were dropped as unusable.
///
/// Accepts a flat `{"vd": "ví dụ"}` object, or the same wrapped in a `macros`
/// key. Individual bad rows are skipped rather than failing the whole import —
/// one malformed line should not cost the user the rest of the file.
pub fn parse_macro_export(text: &str) -> Result<(BTreeMap<String, String>, usize), String> {
    let parsed: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not a valid JSON file: {e}"))?;

    let table = match parsed.get("macros").filter(|m| m.is_object()) {
        Some(inner) => inner,
        None => &parsed,
    };
    let table = table
        .as_object()
        .ok_or_else(|| "expected a list of macros, e.g. {\"vd\": \"ví dụ\"}".to_string())?;

    let mut macros = BTreeMap::new();
    let mut skipped = 0;
    for (trigger, expansion) in table {
        let trigger = trigger.trim();
        match expansion.as_str() {
            Some(expansion) if valid_macro_trigger(trigger) => {
                macros.insert(trigger.to_string(), expansion.to_string());
            }
            _ => skipped += 1,
        }
    }
    Ok((macros, skipped))
}

/// Merge imported macros into the existing list, reporting what changed.
///
/// A merge and not a replacement: a file that simply does not mention one of
/// the user's macros must not delete it.
pub fn merge_macros(
    into: &mut BTreeMap<String, String>,
    incoming: BTreeMap<String, String>,
) -> ImportOutcome {
    let mut outcome = ImportOutcome::default();
    for (trigger, expansion) in incoming {
        match into.insert(trigger, expansion.clone()) {
            None => outcome.added += 1,
            // Re-importing an unchanged file should report nothing happened.
            Some(previous) if previous != expansion => outcome.updated += 1,
            Some(_) => {}
        }
    }
    outcome
}

/// Turn a recorded key press into the canonical shortcut string
/// (`"ctrl+shift+v"`), or `None` when the combination cannot be used.
///
/// `code` is the browser's `KeyboardEvent.code` — the *physical* key, so the
/// recorded shortcut is the one the user's fingers pressed regardless of
/// layout or of what the modifiers turn the character into (Option+V is `√`,
/// Shift+V is `V`). That matches the hook, which compares physical key codes.
///
/// Refused: a combination with no modifier, which would fire on ordinary
/// typing, and any key the hook's `keycode_for` cannot map — storing one would
/// give the user a shortcut that silently never fires.
pub fn shortcut_from_event(ctrl: bool, alt: bool, shift: bool, cmd: bool, code: &str) -> Option<String> {
    if !(ctrl || alt || shift || cmd) {
        return None;
    }
    let key = match code.as_bytes() {
        [b'K', b'e', b'y', c] if c.is_ascii_uppercase() => c.to_ascii_lowercase() as char,
        [b'D', b'i', b'g', b'i', b't', d] if d.is_ascii_digit() => *d as char,
        _ if code == "Space" => ' ',
        _ => return None,
    };

    let mut parts: Vec<&str> = Vec::new();
    for (held, name) in [(ctrl, "ctrl"), (alt, "alt"), (shift, "shift"), (cmd, "cmd")] {
        if held {
            parts.push(name);
        }
    }
    let key = if key == ' ' { "space".to_string() } else { key.to_string() };
    parts.push(&key);
    Some(parts.join("+"))
}

/// Render a shortcut for display as macOS glyphs: `"ctrl+shift+v"` → `"⌃⇧V"`.
///
/// Anything [`parse_shortcut`] rejects is handed back unchanged — `config.toml`
/// is hand-editable, and a bad value has to stay readable to be fixable.
pub fn shortcut_display(shortcut: &str) -> String {
    let Some(sc) = parse_shortcut(shortcut) else {
        return shortcut.to_string();
    };
    let mut out = String::new();
    // The conventional macOS order, which is not the order they were typed in.
    for (held, glyph) in [(sc.ctrl, '⌃'), (sc.alt, '⌥'), (sc.shift, '⇧'), (sc.cmd, '⌘')] {
        if held {
            out.push(glyph);
        }
    }
    if sc.key == ' ' {
        out.push_str("Space");
    } else {
        out.extend(sc.key.to_uppercase());
    }
    out
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

    fn macros(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn exported_macros_come_back_unchanged() {
        let original = macros(&[("vd", "ví dụ"), ("btw", "by the way")]);
        let (parsed, skipped) = parse_macro_export(&macro_export_json(&original)).unwrap();
        assert_eq!(parsed, original);
        assert_eq!(skipped, 0);
    }

    #[test]
    fn an_expansion_with_newlines_and_quotes_survives_the_round_trip() {
        // JSON is the format precisely so these do not need escaping rules of
        // our own — but that only holds if it actually round-trips.
        let original = macros(&[("sig", "Regards,\n\"Phát\"\tPhacius")]);
        let (parsed, _) = parse_macro_export(&macro_export_json(&original)).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn a_file_that_is_not_a_macro_list_is_an_error() {
        assert!(parse_macro_export("not json at all").is_err());
        assert!(parse_macro_export("[1, 2, 3]").is_err());
        assert!(parse_macro_export("\"just a string\"").is_err());
    }

    #[test]
    fn a_config_style_wrapper_is_accepted() {
        // Someone pasting the `[macros]` table out of config.toml, converted,
        // gets a nested object — worth accepting rather than rejecting.
        let (parsed, _) = parse_macro_export(r#"{"macros": {"vd": "ví dụ"}}"#).unwrap();
        assert_eq!(parsed, macros(&[("vd", "ví dụ")]));
    }

    #[test]
    fn unusable_entries_are_skipped_rather_than_failing_the_import() {
        // A trigger with whitespace can never commit as one word, and a
        // non-string value is not an expansion. One bad row must not cost the
        // user the rest of the file.
        let (parsed, skipped) = parse_macro_export(
            r#"{"vd": "ví dụ", "two words": "x", "": "y", "n": 5, "  ": "z"}"#,
        )
        .unwrap();
        assert_eq!(parsed, macros(&[("vd", "ví dụ")]));
        assert_eq!(skipped, 4);
    }

    #[test]
    fn a_trigger_is_trimmed_before_it_is_stored() {
        let (parsed, skipped) = parse_macro_export(r#"{" vd ": "ví dụ"}"#).unwrap();
        assert_eq!(parsed, macros(&[("vd", "ví dụ")]));
        assert_eq!(skipped, 0);
    }

    #[test]
    fn importing_adds_and_overwrites_without_dropping_what_is_there() {
        // Import is a merge, not a replacement: nothing the user already had
        // disappears because a file did not mention it.
        let mut existing = macros(&[("vd", "ví dụ"), ("keep", "kept")]);
        let outcome = merge_macros(&mut existing, macros(&[("vd", "changed"), ("new", "added")]));

        assert_eq!(existing, macros(&[("vd", "changed"), ("keep", "kept"), ("new", "added")]));
        assert_eq!((outcome.added, outcome.updated), (1, 1));
    }

    #[test]
    fn reimporting_the_same_file_changes_nothing() {
        let mut existing = macros(&[("vd", "ví dụ")]);
        let outcome = merge_macros(&mut existing, macros(&[("vd", "ví dụ")]));
        assert_eq!(existing, macros(&[("vd", "ví dụ")]));
        assert_eq!((outcome.added, outcome.updated), (0, 0));
    }

    #[test]
    fn a_recorded_key_press_becomes_a_canonical_shortcut() {
        assert_eq!(
            shortcut_from_event(true, false, true, false, "KeyV").as_deref(),
            Some("ctrl+shift+v")
        );
        assert_eq!(
            shortcut_from_event(false, false, false, true, "Space").as_deref(),
            Some("cmd+space")
        );
        assert_eq!(shortcut_from_event(false, true, false, false, "Digit1").as_deref(), Some("alt+1"));
    }

    #[test]
    fn a_recorded_combination_needs_a_modifier() {
        // A bare letter would fire on ordinary typing, so the recorder must
        // refuse it rather than store a shortcut that eats the keyboard.
        assert_eq!(shortcut_from_event(false, false, false, false, "KeyV"), None);
    }

    #[test]
    fn keys_the_hook_cannot_match_are_refused() {
        // `keycode_for` in the macOS hook maps letters, digits and space only;
        // anything else would be stored and then silently never fire.
        assert_eq!(shortcut_from_event(true, false, false, false, "F5"), None);
        assert_eq!(shortcut_from_event(true, false, false, false, "Slash"), None);
        assert_eq!(shortcut_from_event(true, false, false, false, "Enter"), None);
    }

    #[test]
    fn everything_the_recorder_produces_parses_back() {
        // The recorder and the reader must not drift apart — whatever is
        // recorded has to survive a round trip through `parse_shortcut`.
        for code in ["KeyA", "KeyZ", "Digit0", "Digit9", "Space"] {
            let recorded = shortcut_from_event(true, true, true, true, code)
                .unwrap_or_else(|| panic!("{code} should record"));
            assert!(parse_shortcut(&recorded).is_some(), "{recorded} should parse");
        }
    }

    #[test]
    fn shortcuts_are_shown_as_mac_glyphs() {
        assert_eq!(shortcut_display("ctrl+shift+v"), "⌃⇧V");
        assert_eq!(shortcut_display("cmd+space"), "⌘Space");
        // Glyph order is the macOS convention, not the order they were typed.
        assert_eq!(shortcut_display("cmd+ctrl+alt+shift+k"), "⌃⌥⇧⌘K");
    }

    #[test]
    fn an_unparseable_shortcut_is_shown_as_written() {
        // `config.toml` is hand-editable; a broken value must stay visible so
        // the user can see what to fix rather than an empty box.
        assert_eq!(shortcut_display("hyper+v"), "hyper+v");
    }

    #[test]
    fn turning_macros_off_hides_them_from_the_engine() {
        let mut settings = Settings::default();
        settings.macros.insert("vd".into(), "ví dụ".into());
        assert_eq!(settings.to_core(None).macros.len(), 1);

        settings.macros_enabled = false;
        // The engine is handed none, but the list itself survives — switching
        // the feature back on must not cost the user their macros.
        assert!(settings.to_core(None).macros.is_empty());
        assert_eq!(settings.macros.len(), 1);
    }

    #[test]
    fn memory_is_inert_while_per_app_mode_is_off() {
        let mut settings = Settings::default();
        settings.set_app_mode("Safari", false);
        assert!(settings.vietnamese_on(Some("Safari"))); // global ON wins
    }
}
