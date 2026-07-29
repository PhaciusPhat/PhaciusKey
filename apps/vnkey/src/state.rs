//! Process-wide shell state: the engine plus the live settings.
//!
//! The engine is thread-unsafe by design (single composition buffer), so it
//! lives behind a `Mutex`. On macOS the keyboard-tap callback and the tray menu
//! both run on the main thread, so the lock is effectively uncontended; the
//! `Mutex` exists only to satisfy the `Send` bound shared state requires.

use std::sync::{Mutex, OnceLock};

use vnkey_core::{EditAction, Engine, Keystroke};

use crate::config::Settings;

struct Shell {
    engine: Engine,
    settings: Settings,
}

static SHELL: OnceLock<Mutex<Shell>> = OnceLock::new();

/// Initialize the global shell state. Call once at startup.
pub fn init(settings: Settings) {
    let engine = Engine::new(settings.to_core());
    let _ = SHELL.set(Mutex::new(Shell { engine, settings }));
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
    with(|s| {
        f(&mut s.settings);
        s.engine.set_config(s.settings.to_core());
        s.settings.save();
        s.settings.clone()
    })
    .unwrap_or_default()
}
