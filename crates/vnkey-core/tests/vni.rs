use vnkey_core::{Config, Engine, InputMethod, Keystroke};

fn engine() -> Engine {
    Engine::new(Config {
        method: InputMethod::Vni,
        ..Default::default()
    })
}

fn displayed_after(s: &str) -> String {
    let mut e = engine();
    for ch in s.chars() {
        e.process(Keystroke::char(ch));
    }
    e.current_displayed()
}

#[test]
fn sharp_tone() {
    assert_eq!(displayed_after("ha1"), "há");
}

#[test]
fn grave_tone() {
    assert_eq!(displayed_after("ha2"), "hà");
}

#[test]
fn hook_tone() {
    assert_eq!(displayed_after("ha3"), "hả");
}

#[test]
fn tilde_tone() {
    assert_eq!(displayed_after("ha4"), "hã");
}

#[test]
fn dot_tone() {
    assert_eq!(displayed_after("ha5"), "hạ");
}

#[test]
fn remove_tone() {
    assert_eq!(displayed_after("ha10"), "ha");
}

#[test]
fn circumflex_a() {
    assert_eq!(displayed_after("a6"), "â");
}

#[test]
fn circumflex_e() {
    assert_eq!(displayed_after("e6"), "ê");
}

#[test]
fn circumflex_o() {
    assert_eq!(displayed_after("o6"), "ô");
}

#[test]
fn horn_o() {
    assert_eq!(displayed_after("o7"), "ơ");
}

#[test]
fn horn_u() {
    assert_eq!(displayed_after("u7"), "ư");
}

#[test]
fn breve_a() {
    assert_eq!(displayed_after("a8"), "ă");
}

#[test]
fn stroke_d() {
    assert_eq!(displayed_after("d9a"), "đa");
}

#[test]
fn combined_horn_and_tone() {
    assert_eq!(displayed_after("duong71"), "dướng");
}

#[test]
fn viet() {
    assert_eq!(displayed_after("vie6t5"), "việt");
}

#[test]
fn tone_reverses_after_space_and_backspace() {
    let mut e = engine();
    for ch in "d9oan1".chars() {
        e.process(Keystroke::char(ch));
    }
    e.process(Keystroke::char(' '));
    e.backspace();
    e.process(Keystroke::char('1'));
    assert_eq!(e.current_displayed(), "đoan1");
}
