//! Process-wide shell state: the engine plus the live settings.
//!
//! The engine is thread-unsafe by design (single composition buffer), so it
//! lives behind a `Mutex`. On macOS the keyboard-tap callback runs on its own
//! dedicated thread (see `platform::macos::Hook`) while the tray menu runs on
//! the main thread, so this lock mediates genuine cross-thread access — not
//! just a `Send`-bound formality.

use std::sync::{Mutex, OnceLock};

use vnkey_core::{EditAction, Engine, Keystroke};

use crate::config::Settings;

struct Shell {
    engine: Engine,
    settings: Settings,
    /// Name of the app currently receiving keystrokes (from the platform hook).
    current_app: Option<String>,
    /// Every app that has received keystrokes this session, in the order first
    /// seen. Feeds the settings window's per-app list, so apps show up there
    /// as the user switches around — not persisted.
    seen_apps: Vec<String>,
}

static SHELL: OnceLock<Mutex<Shell>> = OnceLock::new();

/// Called after any state change that the tray must reflect. Set once by the
/// main loop; invoked from whichever thread made the change (the tap thread for
/// the toggle shortcut and app switches), so it must only *signal* the main
/// loop, never touch UI itself.
static ON_CHANGE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn set_on_change(f: Box<dyn Fn() + Send + Sync>) {
    let _ = ON_CHANGE.set(f);
}

fn notify() {
    if let Some(f) = ON_CHANGE.get() {
        f();
    }
}

/// Initialize the global shell state. Call once at startup.
pub fn init(settings: Settings) {
    let engine = Engine::new(settings.to_core(None));
    let _ = SHELL.set(Mutex::new(Shell {
        engine,
        settings,
        current_app: None,
        seen_apps: Vec::new(),
    }));
}

fn with<R>(f: impl FnOnce(&mut Shell) -> R) -> Option<R> {
    let mutex = SHELL.get()?;
    let mut guard = mutex.lock().ok()?;
    Some(f(&mut guard))
}

/// Feed one typed character to the engine and get back the edit actions the
/// shell must perform. Empty means "pass the keystroke through untouched".
pub fn process_char(ch: char) -> Vec<EditAction> {
    with(|s| s.engine.process(Keystroke::char(ch))).unwrap_or_default()
}

/// Feed a Backspace/Delete keystroke to the engine. Empty means "not
/// currently composing — pass the native Backspace through untouched".
pub fn backspace() -> Vec<EditAction> {
    with(|s| s.engine.backspace()).unwrap_or_default()
}

/// Reset the composition buffer (mouse click / focus change).
// Only wired up on macOS today; the Windows scaffold will call it once it grows
// a mouse hook.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn reset() {
    with(|s| s.engine.reset());
}

/// A snapshot of the current settings.
pub fn settings() -> Settings {
    with(|s| s.settings.clone()).unwrap_or_default()
}

/// Mutate the settings, push them into the engine, persist to disk, and return
/// the updated snapshot.
pub fn update(f: impl FnOnce(&mut Settings)) -> Settings {
    let updated = with(|s| {
        f(&mut s.settings);
        s.engine.set_config(s.settings.to_core(s.current_app.as_deref()));
        s.settings.save();
        s.settings.clone()
    })
    .unwrap_or_default();
    notify();
    updated
}

/// Flip Vietnamese typing. In per-app mode this toggles (and remembers) the
/// state of the app currently receiving keystrokes — EVKey-style — falling back
/// to the global switch when no app is known yet; otherwise it is the global
/// switch. Shared by the keyboard shortcut and the tray menu item.
pub fn toggle_vietnamese() -> Settings {
    let updated = with(|s| {
        let app = s.current_app.clone();
        match app.filter(|_| s.settings.per_app_mode) {
            Some(app) => {
                let now = s.settings.vietnamese_on(Some(&app));
                s.settings.set_app_mode(&app, !now);
            }
            None => s.settings.enabled = !s.settings.enabled,
        }
        s.engine.set_config(s.settings.to_core(s.current_app.as_deref()));
        // Mid-word state is meaningless across an on/off flip.
        s.engine.reset();
        s.settings.save();
        s.settings.clone()
    })
    .unwrap_or_default();
    notify();
    updated
}

/// Record which app is receiving keystrokes. On a change of app the
/// composition buffer is stale (different text field), so it is reset.
// Only fed by the macOS hook today, like `reset`.
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
        // The effective on/off state can differ per app, so the engine config
        // must follow the focus, not just the settings.
        s.engine.set_config(s.settings.to_core(Some(name)));
        s.engine.reset();
        true
    })
    .unwrap_or(false);
    if changed {
        notify();
    }
}

/// Name of the app currently receiving keystrokes, if known.
pub fn current_app() -> Option<String> {
    with(|s| s.current_app.clone()).flatten()
}

/// Apps that have received keystrokes this session, in first-seen order.
pub fn seen_apps() -> Vec<String> {
    with(|s| s.seen_apps.clone()).unwrap_or_default()
}

/// Whether Vietnamese typing is effectively on right now (global switch,
/// per-app memory and the exclusion list all considered).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn vietnamese_active() -> bool {
    with(|s| s.settings.vietnamese_on(s.current_app.as_deref())).unwrap_or(false)
}
