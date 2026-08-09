use serde_json::{json, Value};

use crate::config::{parse_shortcut, shortcut_parts, Method, Placement, Settings};
use crate::{autostart, state, update};

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

/// One payload serves every surface. A surface ignores what it does not use,
/// which costs a little redundant JSON and removes the chance of two payloads
/// drifting apart.
pub fn state_json(s: &Settings, current_app: Option<&str>, installed_apps: &[String]) -> String {
    let macros: Vec<Value> = s
        .macros
        .iter()
        .map(|(trigger, expansion)| json!({ "trigger": trigger, "expansion": expansion }))
        .collect();

    let status = update::status();

    json!({
        "platform": super::platform(),
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
        "secure_input": crate::platform::secure_input_active(),
        "vietnamese_here": s.vietnamese_on(current_app),
        "excluded_here": s.excluded_for(current_app),
        "excluded_apps": s.disabled_apps,
        "suggestions": suggestions(installed_apps, current_app),
        "macros": macros,
        "slow_apps": s.slow_apps,
        "autocomplete_fix_apps": s.autocomplete_fix_apps,
        "update_state": status.state(),
        "update_detail": status.detail(),
        "update_version": status.version(),
        "notice": super::ipc::take_notice(),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> Value {
        serde_json::from_str(&state_json(&Settings::default(), None, &[])).unwrap()
    }

    #[test]
    fn the_payload_names_the_platform_it_is_drawn_for() {
        let expected = if cfg!(target_os = "macos") {
            "macos"
        } else {
            "windows"
        };
        assert_eq!(payload()["platform"], expected);
    }

    #[test]
    fn the_default_shortcut_arrives_as_keycaps() {
        assert_eq!(payload()["shortcut_parts"], json!(["⌃", "⇧", "V"]));
        assert_eq!(payload()["shortcut_valid"], json!(true));
    }

    #[test]
    fn suggestions_merge_without_repeating_a_name() {
        let names = suggestions(&["Safari".to_string(), "Notes".to_string()], Some("safari"));
        assert_eq!(names, ["Notes", "Safari"]);
    }
}
