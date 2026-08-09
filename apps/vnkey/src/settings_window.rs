use std::cell::RefCell;
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{json, Value};
use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use wry::WebView;

use crate::config::{
    macro_export_json, merge_macros, parse_macro_export, parse_shortcut, shortcut_from_event,
    shortcut_parts, valid_macro_trigger, Method, Placement, Settings,
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

/// Everything the app-name fields offer as suggestions: what is installed,
/// plus anything typed in during this session, which catches binaries that
/// live outside the usual application folders.
fn suggestions(installed_apps: &[String], current_app: Option<&str>) -> Vec<String> {
    let mut names = installed_apps.to_vec();
    names.extend(state::seen_apps());
    names.extend(current_app.map(str::to_string));
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

fn state_json(s: &Settings, current_app: Option<&str>, installed_apps: &[String]) -> String {
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
        "toggle_shortcut": s.toggle_shortcut,
        "shortcut_parts": shortcut_parts(&s.toggle_shortcut),
        "shortcut_valid": parse_shortcut(&s.toggle_shortcut).is_some(),
        "macros_enabled": s.macros_enabled,
        "current_app": current_app,
        "excluded_apps": s.disabled_apps,
        "suggestions": suggestions(installed_apps, current_app),
        "macros": macros,
        "slow_apps": s.slow_apps,
        "autocomplete_fix_apps": s.autocomplete_fix_apps,
        "notice": take_notice(),
    })
    .to_string()
}

/// The wire format the page speaks. Deserialising into a tagged enum, rather
/// than reading fields off a `Value` by hand, is what makes a mistyped or
/// shadowed field an error instead of a silently ignored message.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Cmd {
    Init,
    Set {
        key: String,
        value: Value,
    },
    Exclude {
        name: String,
        on: bool,
    },
    ForgetApps,
    MacroSet {
        trigger: String,
        expansion: String,
    },
    MacroRemove {
        trigger: String,
    },
    SlowApp {
        name: String,
        on: bool,
    },
    AutocompleteApp {
        name: String,
        on: bool,
    },
    ShortcutRecord {
        on: bool,
    },
    ShortcutCapture {
        code: String,
        ctrl: bool,
        alt: bool,
        shift: bool,
        meta: bool,
    },
    MacrosExport,
    MacrosImport {
        text: String,
    },
    OpenConfig,
}

pub fn apply_ipc(msg: &str) {
    let cmd = match serde_json::from_str::<Cmd>(msg) {
        Ok(cmd) => cmd,
        Err(e) => return eprintln!("[vnkey] ignoring settings message ({e}): {msg}"),
    };

    match cmd {
        Cmd::Init => {}
        Cmd::Set { key, value } => apply_set(&key, &value),
        Cmd::Exclude { name, on } => {
            state::update(move |s| s.set_excluded(&name, on));
        }
        Cmd::ForgetApps => {
            state::update(|s| s.disabled_apps.clear());
        }
        Cmd::MacroSet { trigger, expansion } => {
            let trigger = trigger.trim().to_string();
            if valid_macro_trigger(&trigger) {
                state::update(move |s| {
                    s.macros.insert(trigger, expansion);
                });
            }
        }
        Cmd::MacroRemove { trigger } => {
            state::update(move |s| {
                s.macros.remove(&trigger);
            });
        }
        Cmd::SlowApp { name, on } => {
            state::update(move |s| set_listed(&mut s.slow_apps, &name, on));
        }
        Cmd::AutocompleteApp { name, on } => {
            state::update(move |s| set_listed(&mut s.autocomplete_fix_apps, &name, on));
        }
        Cmd::ShortcutRecord { on } => state::set_shortcut_recording(on),
        Cmd::ShortcutCapture {
            code,
            ctrl,
            alt,
            shift,
            meta,
        } => {
            let Some(shortcut) = shortcut_from_event(ctrl, alt, shift, meta, &code) else {
                return;
            };
            state::update(move |s| s.toggle_shortcut = shortcut);
        }
        Cmd::MacrosExport => export_macros(),
        Cmd::MacrosImport { text } => import_macros(&text),
        Cmd::OpenConfig => crate::open_config_file(),
    }
}

fn set_listed(list: &mut Vec<String>, name: &str, on: bool) {
    list.retain(|a| !a.eq_ignore_ascii_case(name));
    if on {
        list.push(name.to_string());
        list.sort_by_key(|a| a.to_ascii_lowercase());
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

fn apply_set(key: &str, value: &Value) {
    let toggle: Option<fn(&mut Settings, bool)> = match key {
        "enabled" => Some(|s, on| s.enabled = on),
        "auto_restore" => Some(|s, on| s.auto_restore = on),
        "auto_update" => Some(|s, on| s.auto_update = on),
        "start_at_login" => Some(|s, on| s.start_at_login = on),
        "standalone_w" => Some(|s, on| s.standalone_w = on),
        "quick_telex" => Some(|s, on| s.quick_telex = on),
        "quick_start_consonant" => Some(|s, on| s.quick_start_consonant = on),
        "quick_end_consonant" => Some(|s, on| s.quick_end_consonant = on),
        "auto_capitalize" => Some(|s, on| s.auto_capitalize = on),
        "macros_enabled" => Some(|s, on| s.macros_enabled = on),
        _ => None,
    };

    if let Some(set) = toggle {
        let Some(on) = value.as_bool() else { return };
        let updated = state::update(|s| set(s, on));
        if key == "start_at_login" {
            autostart::apply(updated.start_at_login);
        }
        return;
    }

    match (key, value.as_str()) {
        ("method", Some("telex")) => state::update(|s| s.method = Method::Telex),
        ("method", Some("vni")) => state::update(|s| s.method = Method::Vni),
        ("placement", Some("modern")) => state::update(|s| s.placement = Placement::Modern),
        ("placement", Some("classic")) => state::update(|s| s.placement = Placement::Classic),
        _ => return,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(msg: &str) -> Cmd {
        serde_json::from_str(msg).unwrap()
    }

    #[test]
    fn a_recorded_combination_carries_every_modifier() {
        assert_eq!(
            parse(
                r#"{"cmd":"shortcut_capture","code":"KeyV","ctrl":true,
                    "alt":false,"shift":true,"meta":false}"#
            ),
            Cmd::ShortcutCapture {
                code: "KeyV".to_string(),
                ctrl: true,
                alt: false,
                shift: true,
                meta: false,
            }
        );
    }

    /// The command name used to share a key with the ⌘ modifier, so the page
    /// sent `{"cmd":false,…}` and every capture was discarded unread.
    #[test]
    fn a_modifier_cannot_shadow_the_command_name() {
        let shadowed = r#"{"cmd":"shortcut_capture","code":"KeyV","ctrl":true,
                           "alt":false,"shift":true,"cmd":false}"#;
        assert!(serde_json::from_str::<Cmd>(shadowed).is_err());
    }

    #[test]
    fn every_command_the_page_sends_is_understood() {
        for msg in [
            r#"{"cmd":"init"}"#,
            r#"{"cmd":"set","key":"enabled","value":true}"#,
            r#"{"cmd":"set","key":"method","value":"telex"}"#,
            r#"{"cmd":"exclude","name":"Safari","on":true}"#,
            r#"{"cmd":"forget_apps"}"#,
            r#"{"cmd":"macro_set","trigger":"vd","expansion":"ví dụ"}"#,
            r#"{"cmd":"macro_remove","trigger":"vd"}"#,
            r#"{"cmd":"slow_app","name":"IntelliJ IDEA","on":true}"#,
            r#"{"cmd":"autocomplete_app","name":"Safari","on":false}"#,
            r#"{"cmd":"shortcut_record","on":true}"#,
            r#"{"cmd":"macros_export"}"#,
            r#"{"cmd":"macros_import","text":"{}"}"#,
            r#"{"cmd":"open_config"}"#,
        ] {
            assert!(
                serde_json::from_str::<Cmd>(msg).is_ok(),
                "should parse: {msg}"
            );
        }
    }

    #[test]
    fn a_message_that_is_not_a_command_is_refused() {
        assert!(serde_json::from_str::<Cmd>(r#"{"cmd":"nonsense"}"#).is_err());
        assert!(serde_json::from_str::<Cmd>(r#"{"cmd":"exclude"}"#).is_err());
        assert!(serde_json::from_str::<Cmd>("not json").is_err());
    }

    /// `$("missing")` returns null and the next property access throws, which
    /// aborts the whole script and leaves every control on the page inert.
    #[test]
    fn every_element_the_page_looks_up_exists_in_the_markup() {
        let ids: Vec<&str> = HTML
            .match_indices("id=\"")
            .filter_map(|(at, _)| HTML[at + 4..].split('"').next())
            .collect();

        let looked_up = HTML
            .match_indices("$(\"")
            .filter_map(|(at, _)| HTML[at + 3..].split('"').next());

        for id in looked_up {
            assert!(ids.contains(&id), "the page looks up #{id}, which is gone");
        }
    }

    /// Every switch reports a settings key the state payload also carries.
    #[test]
    fn every_switch_binds_to_a_key_the_payload_sends() {
        let payload = state_json(&Settings::default(), None, &[]);
        let payload: Value = serde_json::from_str(&payload).unwrap();

        let keys = HTML
            .match_indices("data-key=\"")
            .filter_map(|(at, _)| HTML[at + 10..].split('"').next());

        for key in keys {
            assert!(payload.get(key).is_some(), "no state is pushed for {key}");
        }
    }

    #[test]
    fn listing_an_app_is_idempotent_and_case_insensitive() {
        let mut list = vec!["Safari".to_string()];
        set_listed(&mut list, "safari", true);
        assert_eq!(list, ["safari"]);

        set_listed(&mut list, "Notes", true);
        assert_eq!(list, ["Notes", "safari"]);

        set_listed(&mut list, "SAFARI", false);
        assert_eq!(list, ["Notes"]);
    }
}
