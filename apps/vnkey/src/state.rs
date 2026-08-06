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
    let engine = Engine::new(settings.to_core());
    let _ = SHELL.set(Mutex::new(Shell { engine, settings, current_app: None }));
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
        s.engine.set_config(s.settings.to_core());
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
        s.current_app = Some(name.to_string());
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

/// Whether Vietnamese typing is turned off for the current app.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn current_app_disabled() -> bool {
    with(|s| s.settings.disabled_for(s.current_app.as_deref())).unwrap_or(false)
}
