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
fn z_onset_takes_a_tone() {
    assert_eq!(displayed_after("zi5"), "zị");
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

#[test]
fn uu_takes_the_horn_on_the_first_u() {
    assert_eq!(displayed_after("cuu71"), "cứu");
    assert_eq!(displayed_after("luu7"), "lưu");
}

#[test]
fn the_u_of_a_qu_onset_takes_no_horn() {
    assert_eq!(displayed_after("quo73"), "quở");
}

#[test]
fn a_second_horn_key_gives_back_a_mark_made_on_a_vowel_cluster() {
    assert_eq!(displayed_after("trua77"), "trua7");
    assert_eq!(displayed_after("thuo77"), "thuo7");
}

#[test]
fn a_horn_skips_a_glide_the_horn_cannot_mark() {
    assert_eq!(displayed_after("voi71"), "với");
    assert_eq!(displayed_after("toi7"), "tơi");
    assert_eq!(displayed_after("tui7"), "tưi");
}

#[test]
fn a_second_horn_key_gives_back_a_mark_made_before_a_glide() {
    assert_eq!(displayed_after("voi77"), "voi7");
}
