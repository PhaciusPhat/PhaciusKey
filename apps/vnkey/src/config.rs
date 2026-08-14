use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use vnkey_core::{Config as CoreConfig, InputMethod, TonePlacementMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Telex,
    Vni,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Modern,
    Classic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub method: Method,
    pub placement: Placement,
    pub auto_restore: bool,
    pub standalone_w: bool,
    pub quick_telex: bool,
    pub quick_start_consonant: bool,
    pub quick_end_consonant: bool,
    pub auto_capitalize: bool,
    pub auto_update: bool,
    pub toggle_shortcut: String,
    pub start_at_login: bool,
    pub show_in_dock: bool,
    pub disabled_apps: Vec<String>,
    pub macros_enabled: bool,
    pub macros: BTreeMap<String, String>,
    pub slow_apps: Vec<String>,
    pub autocomplete_fix_apps: Vec<String>,
    pub last_seen_version: Option<String>,
    /// Written by versions up to 0.0.23, drained by `load` into `disabled_apps`.
    #[serde(rename = "app_modes", skip_serializing)]
    legacy_app_modes: BTreeMap<String, bool>,
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
            show_in_dock: true,
            disabled_apps: Vec::new(),
            macros_enabled: true,
            macros: BTreeMap::new(),
            slow_apps: Vec::new(),
            autocomplete_fix_apps: Vec::new(),
            last_seen_version: None,
            legacy_app_modes: BTreeMap::new(),
        }
    }
}

/// `CFBundleName` in `apps/vnkey/Info.plist`.
const BUNDLE_NAME: &str = "PhaciusKey";

impl Settings {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vnkey")
            .join("config.toml")
    }

    pub fn load(own_name: Option<&str>) -> Self {
        let path = Self::config_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };

        let settings = toml::from_str::<Self>(&text)
            .unwrap_or_default()
            .migrated(own_name);

        // Retired keys survive in the file until something rewrites it, and
        // nothing need ever change in a session. Settle it at startup instead.
        if toml::to_string_pretty(&settings).is_ok_and(|current| current != text) {
            settings.save();
        }
        settings
    }

    /// Our own name is dropped rather than carried over: until 0.0.23 the
    /// event tap credited keystrokes in our own settings window to PhaciusKey,
    /// so configs in the wild remember an app the user never chose.
    ///
    /// `CFBundleName` is matched as well as `own_name`, because the artefact
    /// was written by the installed bundle and a build run straight from
    /// `target/` reports the executable name instead.
    fn migrated(mut self, own_name: Option<&str>) -> Self {
        let ours = |app: &str| {
            app.eq_ignore_ascii_case(BUNDLE_NAME)
                || own_name.is_some_and(|own| own.eq_ignore_ascii_case(app))
        };

        for (app, on) in std::mem::take(&mut self.legacy_app_modes) {
            if !on && !ours(&app) && !self.excluded_for(Some(&app)) {
                self.disabled_apps.push(app);
            }
        }
        self.disabled_apps.retain(|app| !ours(app));
        self.disabled_apps.sort_by_key(|a| a.to_ascii_lowercase());
        self
    }

    pub fn save(&self) {
        let path = Self::config_path();
        let text = match toml::to_string_pretty(self) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("[vnkey] failed to serialize settings: {e}");
                return;
            }
        };

        if std::fs::write(&path, &text).is_ok() {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, &text) {
            eprintln!("[vnkey] failed to save settings to {}: {e}", path.display());
        }
    }

    pub fn excluded_for(&self, app: Option<&str>) -> bool {
        match app {
            Some(app) => self
                .disabled_apps
                .iter()
                .any(|d| d.eq_ignore_ascii_case(app)),
            None => false,
        }
    }

    pub fn vietnamese_on(&self, app: Option<&str>) -> bool {
        !self.excluded_for(app) && self.enabled
    }

    pub fn set_excluded(&mut self, app: &str, excluded: bool) {
        self.disabled_apps.retain(|d| !d.eq_ignore_ascii_case(app));
        if excluded {
            self.disabled_apps.push(app.to_string());
            self.disabled_apps.sort_by_key(|a| a.to_ascii_lowercase());
        }
    }

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
            macros: if self.macros_enabled {
                self.macros
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            } else {
                HashMap::new()
            },
            standalone_w: self.standalone_w,
            quick_telex: self.quick_telex,
            quick_start_consonant: self.quick_start_consonant,
            quick_end_consonant: self.quick_end_consonant,
            auto_capitalize: self.auto_capitalize,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub cmd: bool,
    pub key: Option<char>,
}

/// A key needs one or two modifiers to escape ordinary typing. Modifiers alone need two,
/// because a single held modifier occurs constantly while typing.
const MODIFIERS_WITH_KEY: std::ops::RangeInclusive<usize> = 1..=2;
const MODIFIERS_ALONE: std::ops::RangeInclusive<usize> = 2..=3;

impl Shortcut {
    fn modifier_count(&self) -> usize {
        [self.ctrl, self.shift, self.alt, self.cmd]
            .into_iter()
            .filter(|held| *held)
            .count()
    }
}

fn modifiers_fit(count: usize, key: Option<char>) -> bool {
    match key {
        Some(_) => MODIFIERS_WITH_KEY.contains(&count),
        None => MODIFIERS_ALONE.contains(&count),
    }
}

pub fn parse_shortcut(s: &str) -> Option<Shortcut> {
    let mut sc = Shortcut {
        ctrl: false,
        shift: false,
        alt: false,
        cmd: false,
        key: None,
    };
    let mut tokens = s
        .split('+')
        .map(|t| t.trim().to_ascii_lowercase())
        .peekable();

    while let Some(token) = tokens.next() {
        let is_last = tokens.peek().is_none();
        match token.as_str() {
            "ctrl" | "control" => sc.ctrl = true,
            "shift" => sc.shift = true,
            "alt" | "option" | "opt" => sc.alt = true,
            "cmd" | "command" | "super" | "meta" => sc.cmd = true,
            "space" if is_last => sc.key = Some(' '),
            key if is_last && key.len() == 1 => {
                let c = key.chars().next()?;
                if !c.is_ascii_alphanumeric() {
                    return None;
                }
                sc.key = Some(c);
            }
            _ => return None,
        }
    }

    modifiers_fit(sc.modifier_count(), sc.key).then_some(sc)
}

pub fn valid_macro_trigger(trigger: &str) -> bool {
    !trigger.is_empty() && !trigger.contains(char::is_whitespace)
}

pub fn macro_export_json(macros: &BTreeMap<String, String>) -> String {
    serde_json::to_string_pretty(macros).unwrap_or_else(|_| "{}".to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportOutcome {
    pub added: usize,
    pub updated: usize,
}

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

pub fn merge_macros(
    into: &mut BTreeMap<String, String>,
    incoming: BTreeMap<String, String>,
) -> ImportOutcome {
    let mut outcome = ImportOutcome::default();
    for (trigger, expansion) in incoming {
        match into.insert(trigger, expansion.clone()) {
            None => outcome.added += 1,
            Some(previous) if previous != expansion => outcome.updated += 1,
            Some(_) => {}
        }
    }
    outcome
}

/// The modifier keys held down when a shortcut is recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
}

pub fn shortcut_from_event(mods: Modifiers, code: Option<&str>) -> Option<String> {
    let key = match code {
        Some(code) => Some(match code.as_bytes() {
            [b'K', b'e', b'y', c] if c.is_ascii_uppercase() => c.to_ascii_lowercase() as char,
            [b'D', b'i', b'g', b'i', b't', d] if d.is_ascii_digit() => *d as char,
            _ if code == "Space" => ' ',
            _ => return None,
        }),
        None => None,
    };

    let named = [
        (mods.ctrl, "ctrl"),
        (mods.alt, "alt"),
        (mods.shift, "shift"),
        (mods.cmd, "cmd"),
    ];

    let held = named.iter().filter(|(held, _)| *held).count();
    if !modifiers_fit(held, key) {
        return None;
    }

    let mut parts: Vec<&str> = Vec::new();
    for (held, name) in named {
        if held {
            parts.push(name);
        }
    }
    let key = key.map(|key| {
        if key == ' ' {
            "space".to_string()
        } else {
            key.to_string()
        }
    });
    if let Some(key) = &key {
        parts.push(key);
    }
    Some(parts.join("+"))
}

/// One caption per keycap. Modifier order follows the macOS convention
/// (⌃⌥⇧⌘), which is the order the glyphs are printed in on the keyboard.
pub fn shortcut_parts(shortcut: &str) -> Vec<String> {
    let Some(sc) = parse_shortcut(shortcut) else {
        return vec![shortcut.to_string()];
    };
    let mut parts: Vec<String> = [
        (sc.ctrl, '⌃'),
        (sc.alt, '⌥'),
        (sc.shift, '⇧'),
        (sc.cmd, '⌘'),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .map(|(_, glyph)| glyph.to_string())
    .collect();

    if let Some(key) = sc.key {
        parts.push(match key {
            ' ' => "Space".to_string(),
            key => key.to_uppercase().to_string(),
        });
    }
    parts
}

pub const MOD_CTRL: u8 = 1;
pub const MOD_SHIFT: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_CMD: u8 = 8;

impl Shortcut {
    pub fn modifier_mask(&self) -> u8 {
        (if self.ctrl { MOD_CTRL } else { 0 })
            | (if self.shift { MOD_SHIFT } else { 0 })
            | (if self.alt { MOD_ALT } else { 0 })
            | (if self.cmd { MOD_CMD } else { 0 })
    }
}

/// Tracks a modifier-only shortcut across events, which cannot fire on press: the
/// combination is a prefix of every `⌃⇧X` in every application.
///
/// `poisoned` outlives `armed` deliberately. Releasing a third modifier returns the held
/// set to the target, and without it the gesture would re-arm mid-flight.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChordWatch {
    armed: bool,
    poisoned: bool,
}

impl ChordWatch {
    pub const fn new() -> Self {
        Self {
            armed: false,
            poisoned: false,
        }
    }

    /// Returns true exactly once per clean gesture, on the release that empties the set.
    pub fn modifiers(&mut self, held: u8, target: u8) -> bool {
        if held == 0 {
            let fired = self.armed;
            self.armed = false;
            self.poisoned = false;
            return fired;
        }
        if held & !target != 0 {
            self.armed = false;
            self.poisoned = true;
            return false;
        }
        if held == target && !self.poisoned {
            self.armed = true;
        }
        false
    }

    pub fn interrupted(&mut self, held: u8) {
        self.armed = false;
        self.poisoned = held != 0;
    }
}

/// The Windows virtual-key code a shortcut's key is delivered as.
///
/// Here rather than beside the hook that uses it because CI lints the Windows
/// target but never runs its tests, so a table living in `platform::windows`
/// would be compiled everywhere and checked nowhere.
#[cfg(any(target_os = "windows", test))]
pub fn windows_vk(key: char) -> Option<u16> {
    Some(match key {
        'a'..='z' => key.to_ascii_uppercase() as u16,
        '0'..='9' => key as u16,
        ' ' => 0x20,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_default_shortcut() {
        assert_eq!(
            parse_shortcut("ctrl+shift+v"),
            Some(Shortcut {
                ctrl: true,
                shift: true,
                alt: false,
                cmd: false,
                key: Some('v')
            })
        );
    }

    #[test]
    fn accepts_modifier_synonyms_whitespace_and_case() {
        assert_eq!(
            parse_shortcut(" Control + Option + Space "),
            Some(Shortcut {
                ctrl: true,
                shift: false,
                alt: true,
                cmd: false,
                key: Some(' ')
            })
        );
        assert_eq!(
            parse_shortcut("CMD+2"),
            Some(Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                cmd: true,
                key: Some('2')
            })
        );
    }

    #[test]
    fn rejects_garbage_and_modifierless_keys() {
        assert_eq!(parse_shortcut(""), None);
        assert_eq!(parse_shortcut("v"), None);
        assert_eq!(parse_shortcut("ctrl+vv"), None);
        assert_eq!(parse_shortcut("ctrl+ß"), None);
        assert_eq!(parse_shortcut("hyper+v"), None);
    }

    #[test]
    fn a_combination_holds_two_or_three_keys() {
        assert!(parse_shortcut("ctrl+v").is_some());
        assert!(parse_shortcut("ctrl+shift+v").is_some());
        assert!(parse_shortcut("ctrl+alt+shift+v").is_none());
        assert!(parse_shortcut("ctrl+alt+shift+cmd+v").is_none());
    }

    #[test]
    fn modifiers_alone_need_two_of_them() {
        assert!(parse_shortcut("ctrl+shift").is_some());
        assert!(parse_shortcut("ctrl+alt+shift").is_some());
        assert_eq!(parse_shortcut("shift"), None);
        assert_eq!(parse_shortcut("ctrl"), None);
        assert_eq!(parse_shortcut("ctrl+alt+shift+cmd"), None);
    }

    #[test]
    fn modifiers_alone_render_as_keycaps() {
        assert_eq!(shortcut_parts("ctrl+shift"), ["⌃", "⇧"]);
    }

    #[test]
    fn a_recorded_release_becomes_a_modifier_only_shortcut() {
        assert_eq!(
            shortcut_from_event(
                Modifiers {
                    ctrl: true,
                    shift: true,
                    ..Modifiers::default()
                },
                None
            )
            .as_deref(),
            Some("ctrl+shift")
        );
        assert_eq!(
            shortcut_from_event(
                Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                None
            ),
            None
        );
        assert_eq!(
            shortcut_from_event(
                Modifiers {
                    ctrl: true,
                    alt: true,
                    shift: true,
                    cmd: true
                },
                None
            ),
            None
        );
    }

    #[test]
    fn excluded_for_is_case_insensitive() {
        let settings = Settings {
            disabled_apps: vec!["Terminal".into()],
            ..Default::default()
        };
        assert!(settings.excluded_for(Some("terminal")));
        assert!(!settings.excluded_for(Some("Safari")));
        assert!(!settings.excluded_for(None));
    }

    #[test]
    fn exclusion_beats_the_global_switch() {
        let mut settings = Settings::default();
        assert!(settings.vietnamese_on(Some("Safari")));

        settings.set_excluded("Safari", true);
        assert!(!settings.vietnamese_on(Some("safari")));
        assert!(settings.vietnamese_on(Some("Terminal")));

        settings.enabled = false;
        assert!(!settings.vietnamese_on(Some("Terminal")));

        settings.enabled = true;
        settings.set_excluded("SAFARI", false);
        assert!(settings.vietnamese_on(Some("Safari")));
        assert!(settings.disabled_apps.is_empty());
    }

    #[test]
    fn excluding_the_same_app_twice_lists_it_once() {
        let mut settings = Settings::default();
        settings.set_excluded("Safari", true);
        settings.set_excluded("safari", true);
        assert_eq!(settings.disabled_apps, ["safari"]);
    }

    #[test]
    fn apps_remembered_as_off_become_exclusions() {
        let settings = toml::from_str::<Settings>(
            r#"
            enabled = true
            per_app_mode = true
            disabled_apps = ["Terminal"]

            [app_modes]
            safari = false
            notes = true
            terminal = false
            "#,
        )
        .unwrap()
        .migrated(None);

        assert_eq!(settings.disabled_apps, ["safari", "Terminal"]);
        assert!(settings.legacy_app_modes.is_empty());
    }

    #[test]
    fn migration_drops_our_own_name_from_both_lists() {
        let settings = toml::from_str::<Settings>(
            r#"
            disabled_apps = ["PhaciusKey", "Terminal"]

            [app_modes]
            phaciuskey = false
            safari = false
            "#,
        )
        .unwrap()
        .migrated(Some("PhaciusKey"));

        assert_eq!(settings.disabled_apps, ["safari", "Terminal"]);
    }

    /// A build run from `target/` reports the executable name, so matching on
    /// that alone would leave the installed bundle's name behind for good.
    #[test]
    fn migration_drops_the_bundle_name_whatever_this_build_is_called() {
        let settings = toml::from_str::<Settings>(r#"disabled_apps = ["phaciuskey", "Notes"]"#)
            .unwrap()
            .migrated(Some("vnkey"));

        assert_eq!(settings.disabled_apps, ["Notes"]);
    }

    #[test]
    fn the_retired_keys_are_not_written_back() {
        let settings =
            toml::from_str::<Settings>("per_app_mode = true\n[app_modes]\nsafari = false\n")
                .unwrap()
                .migrated(None);
        let written = toml::to_string_pretty(&settings).unwrap();

        assert!(!written.contains("app_modes"), "{written}");
        assert!(!written.contains("per_app_mode"), "{written}");
        assert!(written.contains("safari"), "{written}");
    }

    fn macros(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
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
        let (parsed, _) = parse_macro_export(r#"{"macros": {"vd": "ví dụ"}}"#).unwrap();
        assert_eq!(parsed, macros(&[("vd", "ví dụ")]));
    }

    #[test]
    fn unusable_entries_are_skipped_rather_than_failing_the_import() {
        let (parsed, skipped) =
            parse_macro_export(r#"{"vd": "ví dụ", "two words": "x", "": "y", "n": 5, "  ": "z"}"#)
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
        let mut existing = macros(&[("vd", "ví dụ"), ("keep", "kept")]);
        let outcome = merge_macros(
            &mut existing,
            macros(&[("vd", "changed"), ("new", "added")]),
        );

        assert_eq!(
            existing,
            macros(&[("vd", "changed"), ("keep", "kept"), ("new", "added")])
        );
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
            shortcut_from_event(
                Modifiers {
                    ctrl: true,
                    shift: true,
                    ..Modifiers::default()
                },
                Some("KeyV")
            )
            .as_deref(),
            Some("ctrl+shift+v")
        );
        assert_eq!(
            shortcut_from_event(
                Modifiers {
                    cmd: true,
                    ..Modifiers::default()
                },
                Some("Space")
            )
            .as_deref(),
            Some("cmd+space")
        );
        assert_eq!(
            shortcut_from_event(
                Modifiers {
                    alt: true,
                    ..Modifiers::default()
                },
                Some("Digit1")
            )
            .as_deref(),
            Some("alt+1")
        );
    }

    #[test]
    fn a_recorded_combination_needs_a_modifier() {
        assert_eq!(
            shortcut_from_event(Modifiers::default(), Some("KeyV")),
            None
        );
    }

    #[test]
    fn a_recorded_combination_stops_at_three_keys() {
        assert_eq!(
            shortcut_from_event(
                Modifiers {
                    ctrl: true,
                    alt: true,
                    ..Modifiers::default()
                },
                Some("KeyV")
            )
            .as_deref(),
            Some("ctrl+alt+v")
        );
        assert_eq!(
            shortcut_from_event(
                Modifiers {
                    ctrl: true,
                    alt: true,
                    shift: true,
                    ..Modifiers::default()
                },
                Some("KeyV")
            ),
            None
        );
        assert_eq!(
            shortcut_from_event(
                Modifiers {
                    ctrl: true,
                    alt: true,
                    shift: true,
                    cmd: true
                },
                Some("KeyV")
            ),
            None
        );
    }

    #[test]
    fn keys_the_hook_cannot_match_are_refused() {
        assert_eq!(
            shortcut_from_event(
                Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                Some("F5")
            ),
            None
        );
        assert_eq!(
            shortcut_from_event(
                Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                Some("Slash")
            ),
            None
        );
        assert_eq!(
            shortcut_from_event(
                Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                Some("Enter")
            ),
            None
        );
    }

    #[test]
    fn everything_the_recorder_produces_parses_back() {
        let combinations = [
            (true, false, false, false),
            (false, true, false, false),
            (false, false, true, false),
            (false, false, false, true),
            (true, false, true, false),
            (false, true, false, true),
        ];
        for code in ["KeyA", "KeyZ", "Digit0", "Digit9", "Space"] {
            for (ctrl, alt, shift, cmd) in combinations {
                let recorded = shortcut_from_event(
                    Modifiers {
                        ctrl,
                        alt,
                        shift,
                        cmd,
                    },
                    Some(code),
                )
                .unwrap_or_else(|| panic!("{code} should record"));
                assert!(
                    parse_shortcut(&recorded).is_some(),
                    "{recorded} should parse"
                );
            }
        }
    }

    #[test]
    fn a_shortcut_becomes_one_caption_per_keycap() {
        assert_eq!(shortcut_parts("ctrl+shift+v"), ["⌃", "⇧", "V"]);
        assert_eq!(shortcut_parts("cmd+space"), ["⌘", "Space"]);
        assert_eq!(shortcut_parts("cmd+alt+k"), ["⌥", "⌘", "K"]);
    }

    #[test]
    fn an_unparseable_shortcut_is_shown_as_written() {
        assert_eq!(shortcut_parts("hyper+v"), ["hyper+v"]);
        assert_eq!(shortcut_parts("ctrl+alt+shift+v"), ["ctrl+alt+shift+v"]);
    }

    #[test]
    fn a_shortcut_key_maps_to_its_windows_virtual_key() {
        assert_eq!(windows_vk('v'), Some(0x56));
        assert_eq!(windows_vk('a'), Some(0x41));
        assert_eq!(windows_vk('z'), Some(0x5A));
        assert_eq!(windows_vk('0'), Some(0x30));
        assert_eq!(windows_vk('9'), Some(0x39));
        assert_eq!(windows_vk(' '), Some(0x20));
    }

    /// The hook can only match a shortcut whose key it can name, so anything
    /// `parse_shortcut` accepts has to have a virtual-key code.
    #[test]
    fn every_key_a_shortcut_may_hold_has_a_virtual_key() {
        for key in ('a'..='z').chain('0'..='9').chain([' ']) {
            let shortcut = match key {
                ' ' => "ctrl+space".to_string(),
                key => format!("ctrl+{key}"),
            };
            let parsed = parse_shortcut(&shortcut).map(|sc| sc.key);
            assert_eq!(parsed, Some(Some(key)), "{shortcut} should parse");
            assert!(windows_vk(key).is_some(), "{key} has no virtual-key code");
        }
    }

    #[test]
    fn a_key_no_shortcut_can_hold_has_no_virtual_key() {
        assert_eq!(windows_vk('!'), None);
        assert_eq!(windows_vk('ư'), None);
        assert_eq!(windows_vk('A'), None);
    }

    #[test]
    fn turning_macros_off_hides_them_from_the_engine() {
        let mut settings = Settings::default();
        settings.macros.insert("vd".into(), "ví dụ".into());
        assert_eq!(settings.to_core(None).macros.len(), 1);

        settings.macros_enabled = false;
        assert!(settings.to_core(None).macros.is_empty());
        assert_eq!(settings.macros.len(), 1);
    }

    fn watch_sequence(target: u8, steps: &[u8]) -> usize {
        let mut watch = ChordWatch::default();
        steps
            .iter()
            .filter(|held| watch.modifiers(**held, target))
            .count()
    }

    const CS: u8 = MOD_CTRL | MOD_SHIFT;

    #[test]
    fn a_clean_hold_and_release_fires_once() {
        assert_eq!(watch_sequence(CS, &[MOD_CTRL, CS, MOD_CTRL, 0]), 1);
    }

    #[test]
    fn either_modifier_may_be_released_first() {
        assert_eq!(watch_sequence(CS, &[MOD_SHIFT, CS, MOD_SHIFT, 0]), 1);
    }

    #[test]
    fn a_third_modifier_spoils_the_gesture_even_once_released() {
        assert_eq!(
            watch_sequence(CS, &[MOD_CTRL, CS, CS | MOD_ALT, CS, MOD_CTRL, 0]),
            0
        );
    }

    #[test]
    fn a_key_pressed_between_spoils_the_gesture() {
        let mut watch = ChordWatch::default();
        assert!(!watch.modifiers(MOD_CTRL, CS));
        assert!(!watch.modifiers(CS, CS));
        watch.interrupted(CS);
        assert!(!watch.modifiers(MOD_CTRL, CS));
        assert!(!watch.modifiers(0, CS));
    }

    /// Typing a plain letter produces no modifier event, so nothing would clear a
    /// poison set with no modifiers held — and the next gesture would be swallowed.
    #[test]
    fn typing_before_a_gesture_does_not_swallow_it() {
        let mut watch = ChordWatch::default();
        watch.interrupted(0);
        assert!(!watch.modifiers(MOD_CTRL, CS));
        assert!(!watch.modifiers(CS, CS));
        assert!(!watch.modifiers(MOD_CTRL, CS));
        assert!(
            watch.modifiers(0, CS),
            "first gesture after typing should fire"
        );
    }

    #[test]
    fn a_click_while_the_modifiers_are_held_still_spoils_it() {
        let mut watch = ChordWatch::default();
        assert!(!watch.modifiers(MOD_CTRL, CS));
        assert!(!watch.modifiers(CS, CS));
        watch.interrupted(CS);
        assert!(!watch.modifiers(MOD_CTRL, CS));
        assert!(!watch.modifiers(0, CS), "a click mid-gesture must spoil it");
    }

    #[test]
    fn a_partial_hold_never_fires() {
        assert_eq!(watch_sequence(CS, &[MOD_CTRL, 0]), 0);
    }

    #[test]
    fn the_gesture_can_be_repeated() {
        let mut watch = ChordWatch::default();
        for _ in 0..2 {
            assert!(!watch.modifiers(MOD_CTRL, CS));
            assert!(!watch.modifiers(CS, CS));
            assert!(watch.modifiers(0, CS));
        }
    }
}
