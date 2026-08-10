use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use vnkey_core::{EditAction, Engine, Keystroke};

use crate::config::{Settings, Shortcut};

struct Shell {
    engine: Engine,
    settings: Settings,
    current_app: Option<String>,
    seen_apps: Vec<String>,
    /// What the toggle shortcut asked for in the application in front, when the
    /// saved settings cannot express it. Dropped when another application comes
    /// forward, so the app list the user curated is what persists.
    app_override: Option<bool>,
}

impl Shell {
    fn vietnamese_here(&self) -> bool {
        self.app_override
            .unwrap_or_else(|| self.settings.vietnamese_on(self.current_app.as_deref()))
    }

    fn apply_config(&mut self) {
        let mut config = self.settings.to_core(self.current_app.as_deref());
        config.enabled = self.vietnamese_here();
        self.engine.set_config(config);
    }
}

static SHELL: OnceLock<Mutex<Shell>> = OnceLock::new();

static ON_CHANGE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

static RECORDING_SHORTCUT: AtomicBool = AtomicBool::new(false);

pub fn set_shortcut_recording(on: bool) {
    RECORDING_SHORTCUT.store(on, Ordering::Relaxed);
}

pub fn shortcut_recording() -> bool {
    RECORDING_SHORTCUT.load(Ordering::Relaxed)
}

/// The parsed toggle shortcut, read by the keyboard hooks on every event. A packed atomic
/// rather than the `Settings` behind the mutex: reading it must not lock, clone or parse.
///
/// Layout: bits 0–3 the modifiers, bit 4 set when a key is present, bits 8+ that key.
/// Zero means no usable shortcut, which is unambiguous because every valid shortcut holds
/// at least one modifier.
static TOGGLE: AtomicU32 = AtomicU32::new(0);

fn pack(shortcut: Option<Shortcut>) -> u32 {
    let Some(sc) = shortcut else { return 0 };
    let mut bits = u32::from(sc.modifier_mask()) & 0xF;
    if let Some(key) = sc.key {
        bits |= 1 << 4;
        bits |= (key as u32) << 8;
    }
    bits
}

fn unpack(bits: u32) -> Option<Shortcut> {
    if bits == 0 {
        return None;
    }
    Some(Shortcut {
        ctrl: bits & u32::from(crate::config::MOD_CTRL) != 0,
        shift: bits & u32::from(crate::config::MOD_SHIFT) != 0,
        alt: bits & u32::from(crate::config::MOD_ALT) != 0,
        cmd: bits & u32::from(crate::config::MOD_CMD) != 0,
        key: (bits & (1 << 4) != 0).then(|| char::from_u32(bits >> 8).unwrap_or('\0')),
    })
}

fn cache_toggle(settings: &Settings) {
    TOGGLE.store(
        pack(crate::config::parse_shortcut(&settings.toggle_shortcut)),
        Ordering::Relaxed,
    );
}

pub fn toggle_shortcut() -> Option<Shortcut> {
    unpack(TOGGLE.load(Ordering::Relaxed))
}

pub fn set_on_change(f: Box<dyn Fn() + Send + Sync>) {
    let _ = ON_CHANGE.set(f);
}

fn notify() {
    if let Some(f) = ON_CHANGE.get() {
        f();
    }
}

pub fn init(settings: Settings) {
    let engine = Engine::new(settings.to_core(None));
    cache_toggle(&settings);
    let _ = SHELL.set(Mutex::new(Shell {
        engine,
        settings,
        current_app: None,
        seen_apps: Vec::new(),
        app_override: None,
    }));
}

fn with<R>(f: impl FnOnce(&mut Shell) -> R) -> Option<R> {
    let mutex = SHELL.get()?;
    let mut guard = mutex.lock().ok()?;
    Some(f(&mut guard))
}

pub fn process_char(ch: char) -> Vec<EditAction> {
    with(|s| s.engine.process(Keystroke::char(ch))).unwrap_or_default()
}

pub fn backspace() -> Vec<EditAction> {
    with(|s| s.engine.backspace()).unwrap_or_default()
}

pub fn restore_raw() -> Vec<EditAction> {
    with(|s| s.engine.restore_raw()).unwrap_or_default()
}

pub fn commit_word() -> Vec<EditAction> {
    with(|s| s.engine.commit_word()).unwrap_or_default()
}

pub fn commit_line() -> Vec<EditAction> {
    with(|s| s.engine.commit_line()).unwrap_or_default()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn autocomplete_fix_here() -> bool {
    with(|s| match &s.current_app {
        Some(app) => s
            .settings
            .autocomplete_fix_apps
            .iter()
            .any(|a| a.eq_ignore_ascii_case(app)),
        None => false,
    })
    .unwrap_or(false)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn slow_typing_here() -> bool {
    with(|s| match &s.current_app {
        Some(app) => s
            .settings
            .slow_apps
            .iter()
            .any(|a| a.eq_ignore_ascii_case(app)),
        None => false,
    })
    .unwrap_or(false)
}

pub fn reset() {
    with(|s| s.engine.reset());
}

pub fn settings() -> Settings {
    with(|s| s.settings.clone()).unwrap_or_default()
}

/// Bumped under the shell lock, so a snapshot's version orders it against every other.
static SETTINGS_VERSION: AtomicU64 = AtomicU64::new(0);

/// The highest version written to disk, guarding the write itself.
static SAVED_VERSION: Mutex<u64> = Mutex::new(0);

/// Persist a snapshot taken under the shell lock, without holding that lock: the
/// keyboard hook acquires it on every keystroke and `save` blocks on the filesystem.
/// An older snapshot losing a race is dropped rather than written over a newer one.
fn save_settings(settings: &Settings, version: u64) {
    let Ok(mut saved) = SAVED_VERSION.lock() else {
        return;
    };
    if version <= *saved {
        return;
    }
    settings.save();
    *saved = version;
}

/// Mutate the shell under its lock, then save and notify outside it.
fn mutate(f: impl FnOnce(&mut Shell)) -> Settings {
    let updated = with(|s| {
        f(s);
        let version = SETTINGS_VERSION.fetch_add(1, Ordering::Relaxed) + 1;
        (s.settings.clone(), version)
    });

    if let Some((settings, version)) = &updated {
        save_settings(settings, *version);
    }
    notify();
    updated.map(|(settings, _)| settings).unwrap_or_default()
}

pub fn update(f: impl FnOnce(&mut Settings)) -> Settings {
    mutate(|s| {
        f(&mut s.settings);
        cache_toggle(&s.settings);
        s.app_override = None;
        s.apply_config();
    })
}

/// The state the toggle shortcut leaves behind.
#[derive(Debug, PartialEq, Eq)]
struct Toggled {
    enabled: bool,
    app_override: Option<bool>,
}

/// The machine-wide switch cannot turn Vietnamese on in an application the
/// settings leave in English, so pressing the shortcut there holds the choice
/// for that application instead of editing the list the user curated.
fn toggled(enabled: bool, excluded_here: bool, app_override: Option<bool>) -> Toggled {
    let want = !app_override.unwrap_or(!excluded_here && enabled);
    if excluded_here {
        Toggled {
            enabled,
            app_override: Some(want),
        }
    } else {
        Toggled {
            enabled: want,
            app_override: None,
        }
    }
}

pub fn toggle_vietnamese() -> Settings {
    mutate(|s| {
        let next = toggled(
            s.settings.enabled,
            s.settings.excluded_for(s.current_app.as_deref()),
            s.app_override,
        );
        s.settings.enabled = next.enabled;
        s.app_override = next.app_override;
        s.apply_config();
        s.engine.reset();
    })
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn set_current_app(name: &str) {
    let changed = with(|s| {
        if s.current_app.as_deref() == Some(name) {
            return false;
        }
        if !s.seen_apps.iter().any(|a| a.eq_ignore_ascii_case(name)) {
            s.seen_apps.push(name.to_string());
        }
        s.current_app = Some(name.to_string());
        s.app_override = None;
        s.apply_config();
        s.engine.reset();
        true
    })
    .unwrap_or(false);
    if changed {
        notify();
    }
}

pub fn current_app() -> Option<String> {
    with(|s| s.current_app.clone()).flatten()
}

pub fn seen_apps() -> Vec<String> {
    with(|s| s.seen_apps.clone()).unwrap_or_default()
}

pub fn vietnamese_active() -> bool {
    with(|s| s.vietnamese_here()).unwrap_or(false)
}

/// Whether the application in front is being left in English, which the toggle
/// shortcut can suspend for as long as that application stays in front.
pub fn exclusion_in_effect() -> bool {
    with(|s| s.settings.excluded_for(s.current_app.as_deref()) && !s.vietnamese_here())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_valid_shortcut_survives_the_round_trip() {
        for text in [
            "ctrl+v",
            "ctrl+shift+v",
            "cmd+space",
            "alt+z",
            "cmd+2",
            "ctrl+shift",
            "ctrl+alt+shift",
            "alt+cmd",
        ] {
            let parsed = crate::config::parse_shortcut(text);
            assert!(parsed.is_some(), "{text} should parse");
            assert_eq!(unpack(pack(parsed)), parsed, "{text} should round-trip");
        }
    }

    #[test]
    fn an_unset_shortcut_packs_to_zero() {
        assert_eq!(pack(None), 0);
        assert_eq!(unpack(0), None);
    }

    #[test]
    fn the_shortcut_flips_the_machine_wide_switch() {
        assert_eq!(
            toggled(true, false, None),
            Toggled {
                enabled: false,
                app_override: None,
            }
        );
        assert_eq!(
            toggled(false, false, None),
            Toggled {
                enabled: true,
                app_override: None,
            }
        );
    }

    #[test]
    fn the_shortcut_turns_an_english_only_app_on_without_editing_the_list() {
        assert_eq!(
            toggled(true, true, None),
            Toggled {
                enabled: true,
                app_override: Some(true),
            }
        );
        assert_eq!(
            toggled(false, true, None),
            Toggled {
                enabled: false,
                app_override: Some(true),
            }
        );
    }

    #[test]
    fn the_shortcut_takes_an_english_only_app_back_off() {
        assert_eq!(
            toggled(true, true, Some(true)),
            Toggled {
                enabled: true,
                app_override: Some(false),
            }
        );
    }

    #[test]
    fn the_shortcut_drops_an_override_the_settings_no_longer_need() {
        assert_eq!(
            toggled(true, false, Some(false)),
            Toggled {
                enabled: true,
                app_override: None,
            }
        );
    }
}
