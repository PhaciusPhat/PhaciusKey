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
    // "duong7" → "dương", then "1" → "dướng"... wait, no tone yet, then "71" → ư + sắc
    assert_eq!(displayed_after("duong71"), "dướng");
}

#[test]
fn viet() {
    // vi + e6 + t + 5 → "việt" (circumflex e, nặng tone). Note the tone digit is
    // pressed once: pressing '5' twice now cancels it and types the digit.
    assert_eq!(displayed_after("vie6t5"), "việt");
}

#[test]
fn tone_reverses_after_space_and_backspace() {
    // The reported bug: "đoán" → space → Backspace → '1' typed "đoán1"
    // instead of reversing the tone. Backspace over the space must resume
    // composing the word, so the repeated tone key cancels sắc: "đoan1".
    let mut e = engine();
    for ch in "d9oan1".chars() {
        e.process(Keystroke::char(ch));
    }
    e.process(Keystroke::char(' '));
    e.backspace();
    e.process(Keystroke::char('1'));
    assert_eq!(e.current_displayed(), "đoan1");
}
