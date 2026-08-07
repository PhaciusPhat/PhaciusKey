//! Glass settings window: a `tao` window hosting a `wry` WebView whose UI is
//! the embedded `settings.html` (same "lacquer night" theme as the website).
//!
//! Data flow is one-directional in each direction: the page posts JSON
//! commands over wry's IPC channel, which are forwarded to the main event
//! loop as [`UserEvent::Ipc`] and applied here through `state::update`; after
//! any change (from the page, the tray, or the toggle shortcut) the full
//! settings snapshot is pushed back into the page via `evaluate_script`, so
//! the window always mirrors reality instead of tracking its own copy.

use std::cell::RefCell;

use serde_json::{json, Value};
use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use wry::WebView;

use crate::config::{parse_shortcut, Method, Placement, Settings};
use crate::{autostart, platform, state, update, UserEvent};

const HTML: &str = include_str!("settings.html");

pub struct SettingsWindow {
    window: Window,
    webview: WebView,
    /// Installed applications feeding the page's app-search suggestions.
    /// Scanned when the window is created and again on every `show`, so a
    /// fresh install appears the next time the window opens — while the
    /// frequent `push_state` calls never touch the filesystem.
    installed_apps: RefCell<Vec<String>>,
}

impl SettingsWindow {
    /// Create the window and its webview. Must run on the main thread (both
    /// tao windows and WKWebView require it).
    pub fn new(
        target: &EventLoopWindowTarget<UserEvent>,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, String> {
        let window = WindowBuilder::new()
            .with_title("PhaciusKey Settings")
            .with_inner_size(LogicalSize::new(680.0, 720.0))
            .with_min_inner_size(LogicalSize::new(560.0, 480.0))
            .build(target)
            .map_err(|e| e.to_string())?;

        let webview = wry::WebViewBuilder::new()
            .with_html(HTML)
            // The page can't touch settings itself — it only posts commands,
            // which the main loop applies via `apply_ipc`.
            .with_ipc_handler(move |request| {
                let _ = proxy.send_event(UserEvent::Ipc(request.body().clone()));
            })
            .build(&window)
            .map_err(|e| e.to_string())?;

        Ok(Self {
            window,
            webview,
            installed_apps: RefCell::new(platform::installed_apps()),
        })
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// Bring the (possibly hidden) window back in front of the user.
    pub fn show(&self) {
        *self.installed_apps.borrow_mut() = platform::installed_apps();
        self.window.set_visible(true);
        self.window.set_focus();
    }

    /// Closing the window only hides it — the app lives in the menu bar.
    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    /// Push the current settings snapshot into the page.
    pub fn push_state(&self) {
        let state = state_json(
            &state::settings(),
            state::current_app().as_deref(),
            &self.installed_apps.borrow(),
        );
        let _ = self.webview.evaluate_script(&format!("window.__setState({state})"));
    }
}

/// The page's state snapshot: settings, the derived per-app rows, and the
/// installed apps offered as search suggestions.
fn state_json(s: &Settings, current_app: Option<&str>, installed_apps: &[String]) -> String {
    // Union of every app the settings or this session knows about, keyed
    // case-insensitively, preferring the real-cased name for display.
    let mut names: Vec<String> = Vec::new();
    let mut add = |name: &str| {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            names.push(name.to_string());
        }
    };
    for app in state::seen_apps() {
        add(&app);
    }
    if let Some(app) = current_app {
        add(app);
    }
    for app in &s.disabled_apps {
        add(app);
    }
    for app in s.app_modes.keys() {
        add(app);
    }
    names.sort_by_key(|n| n.to_ascii_lowercase());

    // Row state mirrors the tray's per-app checkbox: effective state in
    // per-app mode, exclusion *intent* otherwise. Rendering effective state
    // outside per-app mode would make every row a dead control while the
    // master toggle is off (flipping one would snap straight back).
    let apps: Vec<Value> = names
        .iter()
        .map(|name| {
            let on = if s.per_app_mode {
                s.vietnamese_on(Some(name))
            } else {
                !s.disabled_for(Some(name))
            };
            json!({ "name": name, "on": on })
        })
        .collect();

    json!({
        "version": update::CURRENT,
        "enabled": s.enabled,
        "method": match s.method { Method::Telex => "telex", Method::Vni => "vni" },
        "placement": match s.placement { Placement::Modern => "modern", Placement::Classic => "classic" },
        "auto_restore": s.auto_restore,
        "auto_update": s.auto_update,
        "start_at_login": s.start_at_login,
        "per_app_mode": s.per_app_mode,
        "toggle_shortcut": s.toggle_shortcut,
        "shortcut_valid": parse_shortcut(&s.toggle_shortcut).is_some(),
        "current_app": current_app,
        "apps": apps,
        "installed_apps": installed_apps,
    })
    .to_string()
}

/// Apply one JSON command posted by the page. Malformed input is ignored —
/// the page is trusted, but never load-bearing.
pub fn apply_ipc(msg: &str) {
    let Ok(v) = serde_json::from_str::<Value>(msg) else { return };
    match v["cmd"].as_str() {
        Some("init") => {} // no change; the caller pushes state after every command
        Some("set") => apply_set(&v),
        Some("app") => {
            let (Some(name), Some(on)) = (v["name"].as_str(), v["on"].as_bool()) else { return };
            let name = name.to_string();
            state::update(move |s| set_app_on(s, &name, on));
        }
        Some("app_remove") => {
            let Some(name) = v["name"].as_str() else { return };
            let name = name.to_string();
            state::update(move |s| {
                s.app_modes.remove(&name.to_ascii_lowercase());
                s.disabled_apps.retain(|d| !d.eq_ignore_ascii_case(&name));
            });
        }
        Some("forget_apps") => {
            state::update(|s| {
                s.app_modes.clear();
                s.disabled_apps.clear();
            });
        }
        Some("open_config") => crate::open_config_file(),
        _ => {}
    }
}

fn apply_set(v: &Value) {
    let Some(key) = v["key"].as_str() else { return };
    match key {
        "enabled" | "auto_restore" | "auto_update" | "per_app_mode" | "start_at_login" => {
            let Some(on) = v["value"].as_bool() else { return };
            let key = key.to_string();
            let updated = state::update(|s| match key.as_str() {
                "enabled" => s.enabled = on,
                "auto_restore" => s.auto_restore = on,
                "auto_update" => s.auto_update = on,
                "per_app_mode" => s.per_app_mode = on,
                "start_at_login" => s.start_at_login = on,
                _ => unreachable!(),
            });
            if key == "start_at_login" {
                autostart::apply(updated.start_at_login);
            }
        }
        "method" => {
            let method = match v["value"].as_str() {
                Some("telex") => Method::Telex,
                Some("vni") => Method::Vni,
                _ => return,
            };
            state::update(move |s| s.method = method);
        }
        "placement" => {
            let placement = match v["value"].as_str() {
                Some("modern") => Placement::Modern,
                Some("classic") => Placement::Classic,
                _ => return,
            };
            state::update(move |s| s.placement = placement);
        }
        "toggle_shortcut" => {
            let Some(shortcut) = v["value"].as_str() else { return };
            let shortcut = shortcut.to_string();
            state::update(move |s| s.toggle_shortcut = shortcut);
        }
        _ => {}
    }
}

/// Set `app`'s Vietnamese state, using the same precedence the rest of the
/// app does: turning ON lifts the hard exclusion and remembers "on"; turning
/// OFF remembers "off" and adds the hard exclusion. The exclusion is written
/// in both modes — it keeps the switch sticky-off either way, and
/// `disabled_apps` preserves the display casing that `app_modes`' lowercased
/// keys lose.
fn set_app_on(s: &mut Settings, app: &str, on: bool) {
    if on {
        s.set_app_mode(app, true);
    } else {
        s.app_modes.insert(app.to_ascii_lowercase(), false);
        if !s.disabled_for(Some(app)) {
            s.disabled_apps.push(app.to_string());
        }
    }
}
