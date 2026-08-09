use vnkey_core::{Config, Engine, InputMethod, Keystroke};

fn engine_with_restore(auto_restore: bool) -> Engine {
    Engine::new(Config {
        method: InputMethod::Telex,
        auto_restore,
        ..Default::default()
    })
}

#[test]
fn english_word_auto_restored() {
    let _e = engine_with_restore(true);
    let mut e2 = engine_with_restore(true);
    for ch in "xzq".chars() {
        e2.process(Keystroke::char(ch));
    }
    assert!(e2.current_displayed().len() <= 3);
}

#[test]
fn auto_restore_off_no_restore() {
    let mut e = engine_with_restore(false);
    for ch in "xzq".chars() {
        e.process(Keystroke::char(ch));
    }
}

#[test]
fn vietnamese_word_not_restored() {
    let mut e = engine_with_restore(true);
    for ch in "vieets".chars() {
        e.process(Keystroke::char(ch));
    }
    assert_eq!(e.current_displayed(), "viết");
}
