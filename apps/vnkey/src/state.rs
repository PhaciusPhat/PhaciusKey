use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use vnkey_core::{EditAction, Engine, Keystroke};

use crate::config::Settings;

struct Shell {
    engine: Engine,
    settings: Settings,
    current_app: Option<String>,
    seen_apps: Vec<String>,
}

static SHELL: OnceLock<Mutex<Shell>> = OnceLock::new();

static ON_CHANGE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

static RECORDING_SHORTCUT: AtomicBool = AtomicBool::new(false);

pub fn set_shortcut_recording(on: bool) {
    RECORDING_SHORTCUT.store(on, Ordering::Relaxed);
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn shortcut_recording() -> bool {
    RECORDING_SHORTCUT.load(Ordering::Relaxed)
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

pub fn update(f: impl FnOnce(&mut Settings)) -> Settings {
    let updated = with(|s| {
        f(&mut s.settings);
        s.engine
            .set_config(s.settings.to_core(s.current_app.as_deref()));
        s.settings.save();
        s.settings.clone()
    })
    .unwrap_or_default();
    notify();
    updated
}

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
        s.engine
            .set_config(s.settings.to_core(s.current_app.as_deref()));
        s.engine.reset();
        s.settings.save();
        s.settings.clone()
    })
    .unwrap_or_default();
    notify();
    updated
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
        s.engine.set_config(s.settings.to_core(Some(name)));
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
    with(|s| s.settings.vietnamese_on(s.current_app.as_deref())).unwrap_or(false)
}
