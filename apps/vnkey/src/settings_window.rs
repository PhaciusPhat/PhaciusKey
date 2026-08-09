use std::cell::RefCell;
use std::sync::Mutex;

use serde_json::{json, Value};
use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use wry::WebView;

use crate::config::{
    macro_export_json, merge_macros, parse_macro_export, parse_shortcut, shortcut_display,
    shortcut_from_event, valid_macro_trigger, Method, Placement, Settings,
};
use crate::{autostart, platform, state, update, UserEvent};

const HTML: &str = include_str!("settings.html");

static NOTICE: Mutex<Option<String>> = Mutex::new(None);

fn set_notice(text: impl Into<String>) {
    if let Ok(mut notice) = NOTICE.lock() {
        *notice = Some(text.into());
    }
}

fn take_notice() -> Option<String> {
    NOTICE.lock().ok().and_then(|mut notice| notice.take())
}

pub struct SettingsWindow {
    window: Window,
    webview: WebView,
    installed_apps: RefCell<Vec<String>>,
}

impl SettingsWindow {
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

    pub fn show(&self) {
        *self.installed_apps.borrow_mut() = platform::installed_apps();
        self.window.set_visible(true);
        self.window.set_focus();
    }

    pub fn hide(&self) {
        state::set_shortcut_recording(false);
        self.window.set_visible(false);
    }

    pub fn push_state(&self) {
        let state = state_json(
            &state::settings(),
            state::current_app().as_deref(),
            &self.installed_apps.borrow(),
        );
        let _ = self
            .webview
            .evaluate_script(&format!("window.__setState({state})"));
    }
}

fn state_json(s: &Settings, current_app: Option<&str>, installed_apps: &[String]) -> String {
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

    let macros: Vec<Value> = s
        .macros
        .iter()
        .map(|(trigger, expansion)| json!({ "trigger": trigger, "expansion": expansion }))
        .collect();

    json!({
        "version": update::CURRENT,
        "enabled": s.enabled,
        "method": match s.method { Method::Telex => "telex", Method::Vni => "vni" },
        "placement": match s.placement { Placement::Modern => "modern", Placement::Classic => "classic" },
        "auto_restore": s.auto_restore,
        "standalone_w": s.standalone_w,
        "quick_telex": s.quick_telex,
        "quick_start_consonant": s.quick_start_consonant,
        "quick_end_consonant": s.quick_end_consonant,
        "auto_capitalize": s.auto_capitalize,
        "auto_update": s.auto_update,
        "start_at_login": autostart::effective(s.start_at_login),
        "per_app_mode": s.per_app_mode,
        "toggle_shortcut": s.toggle_shortcut,
        "shortcut_display": shortcut_display(&s.toggle_shortcut),
        "shortcut_valid": parse_shortcut(&s.toggle_shortcut).is_some(),
        "macros_enabled": s.macros_enabled,
        "current_app": current_app,
        "apps": apps,
        "installed_apps": installed_apps,
        "macros": macros,
        "slow_apps": s.slow_apps,
        "autocomplete_fix_apps": s.autocomplete_fix_apps,
        "notice": take_notice(),
    })
    .to_string()
}

pub fn apply_ipc(msg: &str) {
    let Ok(v) = serde_json::from_str::<Value>(msg) else {
        return;
    };
    match v["cmd"].as_str() {
        Some("init") => {}
        Some("set") => apply_set(&v),
        Some("app") => {
            let (Some(name), Some(on)) = (v["name"].as_str(), v["on"].as_bool()) else {
                return;
            };
            let name = name.to_string();
            state::update(move |s| set_app_on(s, &name, on));
        }
        Some("app_remove") => {
            let Some(name) = v["name"].as_str() else {
                return;
            };
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
        Some("macro_set") => {
            let (Some(trigger), Some(expansion)) = (v["trigger"].as_str(), v["expansion"].as_str())
            else {
                return;
            };
            let trigger = trigger.trim().to_string();
            if !valid_macro_trigger(&trigger) {
                return;
            }
            let expansion = expansion.to_string();
            state::update(move |s| {
                s.macros.insert(trigger, expansion);
            });
        }
        Some("macro_remove") => {
            let Some(trigger) = v["trigger"].as_str() else {
                return;
            };
            let trigger = trigger.to_string();
            state::update(move |s| {
                s.macros.remove(&trigger);
            });
        }
        Some("slow_app") => {
            let (Some(name), Some(on)) = (v["name"].as_str(), v["on"].as_bool()) else {
                return;
            };
            let name = name.to_string();
            state::update(move |s| {
                s.slow_apps.retain(|a| !a.eq_ignore_ascii_case(&name));
                if on {
                    s.slow_apps.push(name.clone());
                    s.slow_apps.sort_by_key(|a| a.to_ascii_lowercase());
                }
            });
        }
        Some("autocomplete_app") => {
            let (Some(name), Some(on)) = (v["name"].as_str(), v["on"].as_bool()) else {
                return;
            };
            let name = name.to_string();
            state::update(move |s| {
                s.autocomplete_fix_apps
                    .retain(|a| !a.eq_ignore_ascii_case(&name));
                if on {
                    s.autocomplete_fix_apps.push(name.clone());
                    s.autocomplete_fix_apps
                        .sort_by_key(|a| a.to_ascii_lowercase());
                }
            });
        }
        Some("shortcut_record") => {
            let Some(on) = v["on"].as_bool() else { return };
            state::set_shortcut_recording(on);
        }
        Some("shortcut_capture") => {
            let code = v["code"].as_str().unwrap_or_default();
            let held = |name: &str| v[name].as_bool().unwrap_or(false);
            let Some(shortcut) =
                shortcut_from_event(held("ctrl"), held("alt"), held("shift"), held("cmd"), code)
            else {
                return;
            };
            state::update(move |s| s.toggle_shortcut = shortcut);
        }
        Some("macros_export") => export_macros(),
        Some("macros_import") => {
            let Some(text) = v["text"].as_str() else {
                return;
            };
            import_macros(text);
        }
        Some("open_config") => crate::open_config_file(),
        _ => {}
    }
}

fn export_macros() {
    let settings = state::settings();
    if settings.macros.is_empty() {
        set_notice("There are no macros to export yet.");
        return;
    }

    let Some(dir) = dirs::download_dir().or_else(dirs::home_dir) else {
        set_notice("Could not work out where to save the file.");
        return;
    };
    let path = dir.join("vnkey-macros.json");
    if let Err(e) = std::fs::write(&path, macro_export_json(&settings.macros)) {
        set_notice(format!("Could not write {}: {e}", path.display()));
        return;
    }

    let count = settings.macros.len();
    set_notice(format!(
        "Exported {count} macro{} to {}",
        plural(count),
        path.display()
    ));
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn();
}

fn import_macros(text: &str) {
    let (incoming, skipped) = match parse_macro_export(text) {
        Ok(result) => result,
        Err(e) => return set_notice(format!("Could not read that file — {e}")),
    };
    if incoming.is_empty() {
        set_notice(match skipped {
            0 => "That file had no macros in it.".to_string(),
            n => format!(
                "Nothing usable in that file — {n} entr{} skipped",
                plural_y(n)
            ),
        });
        return;
    }

    let mut outcome = crate::config::ImportOutcome::default();
    state::update(|s| outcome = merge_macros(&mut s.macros, incoming));

    let mut parts = vec![format!("Added {}", outcome.added)];
    if outcome.updated > 0 {
        parts.push(format!("{} updated", outcome.updated));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} skipped"));
    }
    set_notice(format!("Imported macros — {}", parts.join(", ")));
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn plural_y(n: usize) -> &'static str {
    if n == 1 {
        "y"
    } else {
        "ies"
    }
}

fn apply_set(v: &Value) {
    let Some(key) = v["key"].as_str() else { return };
    match key {
        "enabled"
        | "auto_restore"
        | "auto_update"
        | "per_app_mode"
        | "start_at_login"
        | "standalone_w"
        | "quick_telex"
        | "quick_start_consonant"
        | "quick_end_consonant"
        | "auto_capitalize"
        | "macros_enabled" => {
            let Some(on) = v["value"].as_bool() else {
                return;
            };
            let key = key.to_string();
            let updated = state::update(|s| match key.as_str() {
                "enabled" => s.enabled = on,
                "auto_restore" => s.auto_restore = on,
                "auto_update" => s.auto_update = on,
                "per_app_mode" => s.per_app_mode = on,
                "start_at_login" => s.start_at_login = on,
                "standalone_w" => s.standalone_w = on,
                "quick_telex" => s.quick_telex = on,
                "quick_start_consonant" => s.quick_start_consonant = on,
                "quick_end_consonant" => s.quick_end_consonant = on,
                "auto_capitalize" => s.auto_capitalize = on,
                "macros_enabled" => s.macros_enabled = on,
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

        _ => {}
    }
}

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
