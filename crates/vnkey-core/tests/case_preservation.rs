use vnkey_core::{Config, Engine, InputMethod, Keystroke};

fn displayed_after_with(method: InputMethod, s: &str) -> String {
    let mut e = Engine::new(Config {
        method,
        ..Default::default()
    });
    for ch in s.chars() {
        e.process(Keystroke::char(ch));
    }
    e.current_displayed()
}

fn displayed_after(s: &str) -> String {
    displayed_after_with(InputMethod::Telex, s)
}

#[test]
fn midword_capital_is_kept() {
    assert_eq!(displayed_after("BaN"), "BaN");
    assert_eq!(displayed_after("TruNg"), "TruNg");
    assert_eq!(displayed_after("LaM"), "LaM");
}

#[test]
fn midword_capital_is_kept_in_transient_states() {
    assert_eq!(displayed_after("NoT"), "NoT");
}

#[test]
fn midword_capital_survives_a_tone_key() {
    assert_eq!(displayed_after("BaNs"), "BáN");
    assert_eq!(displayed_after("LaMf"), "LàM");
}

#[test]
fn diacritic_pair_keeps_the_first_keys_case() {
    assert_eq!(displayed_after("Aa"), "Â");
    assert_eq!(displayed_after("aA"), "â");
    assert_eq!(displayed_after("Dda"), "Đa");
}

#[test]
fn cancelled_tone_key_keeps_its_typed_case() {
    assert_eq!(displayed_after("HaSS"), "HaSS");
}

#[test]
fn leading_capital_and_all_caps_still_work() {
    assert_eq!(displayed_after("Has"), "Há");
    assert_eq!(displayed_after("VIEETS"), "VIẾT");
    assert_eq!(displayed_after("vieets"), "viết");
}

#[test]
fn vni_midword_capital_is_kept() {
    assert_eq!(displayed_after_with(InputMethod::Vni, "BaN"), "BaN");
    assert_eq!(displayed_after_with(InputMethod::Vni, "BaN1"), "BáN");
}

#[test]
fn vni_all_caps_still_works() {
    assert_eq!(displayed_after_with(InputMethod::Vni, "VIET61"), "VIẾT");
}

mod esc_restore {
    use vnkey_core::{Config, EditAction, Engine, Keystroke};

    fn typed(s: &str) -> Engine {
        let mut e = Engine::new(Config::default());
        for ch in s.chars() {
            e.process(Keystroke::char(ch));
        }
        e
    }

    #[test]
    fn restores_the_raw_keystrokes() {
        let mut e = typed("ddaays");
        assert_eq!(e.current_displayed(), "đấy");
        let actions = e.restore_raw();
        assert!(!actions.is_empty());
        assert_eq!(e.current_displayed(), "ddaays");
    }

    #[test]
    fn typing_after_restore_stays_literal() {
        let mut e = typed("ddaays");
        e.restore_raw();
        e.process(Keystroke::char('s'));
        assert_eq!(e.current_displayed(), "ddaayss");
    }

    #[test]
    fn nothing_to_restore_returns_empty() {
        assert!(typed("").restore_raw().is_empty());
        assert!(typed("xin").restore_raw().is_empty());
    }

    #[test]
    fn restore_keeps_typed_case() {
        let mut e = typed("DDaays");
        assert_eq!(e.current_displayed(), "Đấy");
        e.restore_raw();
        assert_eq!(e.current_displayed(), "DDaays");
    }

    #[test]
    fn restore_emits_a_minimal_diff() {
        let mut e = typed("as");
        assert_eq!(e.current_displayed(), "á");
        assert_eq!(
            e.restore_raw(),
            vec![EditAction::Backspace(1), EditAction::Insert("as".into())]
        );
    }
}
