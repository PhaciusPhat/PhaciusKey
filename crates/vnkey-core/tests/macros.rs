//! Macros / text expansion: a word matching a user-defined trigger is
//! replaced with its expansion when the word is committed by a boundary.

use std::collections::HashMap;

use vnkey_core::{Config, EditAction, Engine, Keystroke};

fn engine_with_macros(pairs: &[(&str, &str)]) -> Engine {
    let macros: HashMap<String, String> =
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Engine::new(Config { macros, ..Default::default() })
}

/// Type `s` and apply the actions to a screen buffer, like the platform layer.
fn type_through(e: &mut Engine, s: &str) -> String {
    let mut screen = String::new();
    for ch in s.chars() {
        let actions = e.process(Keystroke::char(ch));
        if actions.is_empty() {
            screen.push(ch);
            continue;
        }
        for action in actions {
            match action {
                EditAction::Backspace(n) => {
                    for _ in 0..n {
                        screen.pop();
                    }
                }
                EditAction::Insert(text) => screen.push_str(&text),
            }
        }
    }
    screen
}

#[test]
fn space_expands_a_trigger() {
    let mut e = engine_with_macros(&[("vd", "ví dụ")]);
    assert_eq!(type_through(&mut e, "vd "), "ví dụ ");
}

#[test]
fn punctuation_expands_too() {
    let mut e = engine_with_macros(&[("ko", "không")]);
    assert_eq!(type_through(&mut e, "ko,"), "không,");
}

#[test]
fn explicit_commit_expands_without_a_boundary_char() {
    // Enter/Tab reach the engine as a plain commit (the shell passes the key
    // itself through to the app).
    let mut e = engine_with_macros(&[("vd", "ví dụ")]);
    let mut screen = type_through(&mut e, "vd");
    for action in e.commit_word() {
        match action {
            EditAction::Backspace(n) => {
                for _ in 0..n {
                    screen.pop();
                }
            }
            EditAction::Insert(text) => screen.push_str(&text),
        }
    }
    assert_eq!(screen, "ví dụ");
}

#[test]
fn non_triggers_are_left_alone() {
    let mut e = engine_with_macros(&[("vd", "ví dụ")]);
    assert_eq!(type_through(&mut e, "vdx xin "), "vdx xin ");
}

#[test]
fn trigger_matches_the_displayed_word() {
    // The trigger is what the user *sees*: with Telex on, typing "email" would
    // show "email" (auto-restored), so a trigger can be any on-screen word.
    let mut e = engine_with_macros(&[("email", "phat.le@example.com")]);
    assert_eq!(type_through(&mut e, "email "), "phat.le@example.com ");
}

#[test]
fn triggers_are_case_sensitive() {
    let mut e = engine_with_macros(&[("vd", "ví dụ")]);
    assert_eq!(type_through(&mut e, "Vd "), "Vd ");
}

#[test]
fn expansion_can_contain_composed_vietnamese_and_spaces() {
    let mut e = engine_with_macros(&[("xph", "xin phép được vắng mặt")]);
    assert_eq!(type_through(&mut e, "xph "), "xin phép được vắng mặt ");
}

#[test]
fn no_macros_configured_changes_nothing() {
    let mut e = engine_with_macros(&[]);
    assert_eq!(type_through(&mut e, "vd "), "vd ");
}
