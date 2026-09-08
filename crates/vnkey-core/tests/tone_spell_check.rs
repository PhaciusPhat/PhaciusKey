use vnkey_core::{Config, Engine, InputMethod, Keystroke};

fn displayed_after(method: InputMethod, s: &str) -> String {
    let mut e = Engine::new(Config {
        method,
        auto_restore: false,
        ..Default::default()
    });
    for ch in s.chars() {
        e.process(Keystroke::char(ch));
    }
    e.current_displayed()
}

fn vni(s: &str) -> String {
    displayed_after(InputMethod::Vni, s)
}

fn telex(s: &str) -> String {
    displayed_after(InputMethod::Telex, s)
}

#[test]
fn tone_key_is_literal_after_an_impossible_syllable() {
    assert_eq!(vni("phacius2"), "phacius2");
    assert_eq!(vni("test2"), "test2");
    assert_eq!(vni("abc123"), "abc123");
    assert_eq!(telex("hellof"), "hellof");
    assert_eq!(telex("press"), "press");
}

#[test]
fn tone_key_is_literal_when_the_coda_forbids_the_tone() {
    assert_eq!(vni("bat2"), "bat2");
    assert_eq!(telex("batf"), "batf");
}

#[test]
fn a_stop_coda_still_takes_sac_and_nang() {
    assert_eq!(vni("bat1"), "bát");
    assert_eq!(vni("bat5"), "bạt");
    assert_eq!(telex("bats"), "bát");
    assert_eq!(telex("batj"), "bạt");
}

#[test]
fn tones_still_apply_to_possible_syllables() {
    assert_eq!(vni("nguyen64"), "nguyễn");
    assert_eq!(vni("viet65"), "việt");
    assert_eq!(vni("quoc61"), "quốc");
    assert_eq!(telex("nguyeenx"), "nguyễn");
    assert_eq!(telex("ddaays"), "đấy");
    assert_eq!(telex("quoocs"), "quốc");
    assert_eq!(telex("toanf"), "toàn");
    assert_eq!(telex("toafn"), "toàn");
}
