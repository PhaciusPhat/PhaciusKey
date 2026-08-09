use std::sync::Mutex;

use serde::Deserialize;
use serde_json::Value;

use crate::config::{
    macro_export_json, merge_macros, parse_macro_export, shortcut_from_event, valid_macro_trigger,
    Method, Placement, Settings,
};
use crate::{autostart, state, update};

static NOTICE: Mutex<Option<String>> = Mutex::new(None);

fn set_notice(text: impl Into<String>) {
    if let Ok(mut notice) = NOTICE.lock() {
        *notice = Some(text.into());
    }
}

pub(super) fn take_notice() -> Option<String> {
    NOTICE.lock().ok().and_then(|mut notice| notice.take())
}

/// What a page asked of the window it lives in. Everything else a message can
/// ask for is settings state, which is applied here and pushed back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Close,
    Drag,
    Quit,
    OpenSettings,
    CheckUpdates,
    /// The panel measured its content and wants the window to match.
    Resize(u32),
}

/// The wire format the pages speak. Deserialising into a tagged enum, rather
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
        code: Option<String>,
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
    ReportIssue,
    CheckUpdates,
    ToggleVietnamese,
    DragWindow,
    CloseWindow,
    OpenSettings,
    Quit,
    PanelHeight {
        height: u32,
    },
    OpenAccessibility,
    OpenReleases,
}

pub fn apply_ipc(msg: &str) -> Option<WindowAction> {
    let cmd = match serde_json::from_str::<Cmd>(msg) {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("[vnkey] ignoring interface message ({e}): {msg}");
            return None;
        }
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
            if let Some(shortcut) = shortcut_from_event(ctrl, alt, shift, meta, code.as_deref()) {
                state::update(move |s| s.toggle_shortcut = shortcut);
            }
        }
        Cmd::MacrosExport => export_macros(),
        Cmd::MacrosImport { text } => import_macros(&text),
        Cmd::OpenConfig => crate::open_config_file(),
        Cmd::ReportIssue => update::open_url(&update::new_issue_url()),
        Cmd::CheckUpdates => return Some(WindowAction::CheckUpdates),
        Cmd::ToggleVietnamese => {
            state::toggle_vietnamese();
        }
        Cmd::DragWindow => return Some(WindowAction::Drag),
        Cmd::CloseWindow => return Some(WindowAction::Close),
        Cmd::OpenSettings => return Some(WindowAction::OpenSettings),
        Cmd::Quit => return Some(WindowAction::Quit),
        Cmd::PanelHeight { height } => return Some(WindowAction::Resize(height)),
        Cmd::OpenAccessibility => crate::platform::open_accessibility_settings(),
        Cmd::OpenReleases => update::open_url(&update::releases_url()),
    }
    None
}

fn set_listed(list: &mut Vec<String>, name: &str, on: bool) {
    list.retain(|a| !a.eq_ignore_ascii_case(name));
    if on {
        list.push(name.to_string());
        list.sort_by_key(|a| a.to_ascii_lowercase());
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
                code: Some("KeyV".to_string()),
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
    fn a_capture_may_arrive_without_a_key() {
        assert_eq!(
            parse(
                r#"{"cmd":"shortcut_capture","code":null,"ctrl":true,
                    "alt":false,"shift":true,"meta":false}"#
            ),
            Cmd::ShortcutCapture {
                code: None,
                ctrl: true,
                alt: false,
                shift: true,
                meta: false,
            }
        );
    }

    #[test]
    fn every_command_the_pages_send_is_understood() {
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
            r#"{"cmd":"shortcut_capture","code":null,"ctrl":true,
                "alt":false,"shift":true,"meta":false}"#,
            r#"{"cmd":"macros_export"}"#,
            r#"{"cmd":"macros_import","text":"{}"}"#,
            r#"{"cmd":"open_config"}"#,
            r#"{"cmd":"report_issue"}"#,
            r#"{"cmd":"check_updates"}"#,
            r#"{"cmd":"toggle_vietnamese"}"#,
            r#"{"cmd":"drag_window"}"#,
            r#"{"cmd":"close_window"}"#,
            r#"{"cmd":"open_settings"}"#,
            r#"{"cmd":"quit"}"#,
            r#"{"cmd":"panel_height","height":312}"#,
            r#"{"cmd":"open_accessibility"}"#,
            r#"{"cmd":"open_releases"}"#,
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
