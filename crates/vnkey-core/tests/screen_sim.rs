//! Mirrors what the platform hooks do with the actions the engine returns, so
//! a keystroke that never reaches the screen shows up as a test failure rather
//! than as a report that typing "sometimes" stops working.

use vnkey_core::{Config, EditAction, Engine, InputMethod, Keystroke};

struct Screen {
    engine: Engine,
    text: String,
}

impl Screen {
    fn new(method: InputMethod) -> Self {
        Self::with(Config {
            method,
            ..Default::default()
        })
    }

    fn with(config: Config) -> Self {
        Self {
            engine: Engine::new(config),
            text: String::new(),
        }
    }

    /// The hook's rule: no actions means the keystroke was not consumed and
    /// reaches the application unchanged; any actions replace it entirely.
    fn press(&mut self, ch: char) {
        let actions = self.engine.process(Keystroke::char(ch));
        if actions.is_empty() {
            self.text.push(ch);
            return;
        }
        for action in actions {
            match action {
                EditAction::Backspace(n) => {
                    for _ in 0..n {
                        self.text.pop();
                    }
                }
                EditAction::Insert(s) => self.text.push_str(&s),
            }
        }
    }

    fn type_str(&mut self, s: &str) -> &str {
        for ch in s.chars() {
            self.press(ch);
        }
        &self.text
    }
}

fn typed(method: InputMethod, s: &str) -> String {
    Screen::new(method).type_str(s).to_string()
}

fn typed_with(config: Config, s: &str) -> String {
    Screen::with(config).type_str(s).to_string()
}

#[test]
fn an_opening_bracket_reaches_the_screen() {
    assert_eq!(typed(InputMethod::Vni, "("), "(");
    assert_eq!(typed(InputMethod::Telex, "("), "(");
}

#[test]
fn typing_continues_after_an_opening_bracket() {
    assert_eq!(typed(InputMethod::Vni, "(tieng"), "(tieng");
    assert_eq!(typed(InputMethod::Vni, "(tieng1"), "(tiéng");
    assert_eq!(typed(InputMethod::Telex, "(tieengs"), "(tiếng");
}

/// Sweeps a word, then a boundary, then another key: the shape of real typing
/// rather than a boundary in isolation.
#[test]
fn no_keystroke_disappears_mid_sentence() {
    let alphabet: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();

    for method in [InputMethod::Vni, InputMethod::Telex] {
        for lead in ["", "a", "ti", "tieng", "vieet", "ddoa", "xin", "chao2"] {
            for boundary in ['(', ')', ' ', '.', '-', '"'] {
                for &ch in &alphabet {
                    let mut screen = Screen::new(method);
                    screen.type_str(lead);
                    screen.press(boundary);
                    let before = screen.text.clone();
                    screen.press(ch);

                    assert_ne!(
                        screen.text, before,
                        "{method:?}: {lead:?} then {boundary:?} then {ch:?} put nothing on screen"
                    );
                }
            }
        }
    }
}

/// A word the engine has given up on reads back exactly as it was typed, which
/// is what lets an English word through whole.
#[test]
fn a_restored_word_reads_back_key_for_key() {
    for sequence in [
        "error", "hurry", "sorry", "carry", "arrow", "mirror", "possible", "address", "dorrs",
        "hassf", "toanssf",
    ] {
        assert_eq!(typed(InputMethod::Telex, sequence), sequence);
    }
}

/// Pressing a tone key twice removes the tone and types the key. A third press
/// types one more character, rather than also putting back the press the tone
/// rule spent.
#[test]
fn a_third_tone_key_types_one_character() {
    assert_eq!(typed(InputMethod::Telex, "dorr"), "dor");
    assert_eq!(typed(InputMethod::Telex, "dorrr"), "dorr");
    assert_eq!(typed(InputMethod::Telex, "dorrrr"), "dorrr");
    assert_eq!(typed(InputMethod::Vni, "do33"), "do3");
    assert_eq!(typed(InputMethod::Vni, "do333"), "do33");
}

/// A key that follows an undone diacritic still reaches the screen: the word is
/// literal from then on, so the key types itself.
#[test]
fn a_key_after_an_undone_diacritic_is_not_swallowed() {
    assert_eq!(typed(InputMethod::Telex, "aaas"), "aas");
    assert_eq!(typed(InputMethod::Telex, "aaaf"), "aaf");
}

const ADDRESSES: &[&str] = &[
    "phacius2001@gmail.com",
    "hoa2@gmail.com",
    "an1@x.com",
    "toan5@gmail.com",
    "nguyen1998@gmail.com",
    "phat.le@mesoneer.io",
    "user+tag@sub.domain.org",
];

#[test]
fn an_email_address_survives_being_typed() {
    for method in [InputMethod::Vni, InputMethod::Telex] {
        for address in ADDRESSES {
            assert_eq!(typed(method, address), *address, "{method:?}: {address:?}");
        }
    }
}

/// Auto-restore is a guess about English words, so it can be switched off. An
/// `@` is not a guess: a VNI tone digit typed before it belongs to an address,
/// and no tone reaches the domain that follows.
#[test]
fn a_tone_digit_never_crosses_an_at_sign() {
    for address in ADDRESSES {
        let config = Config {
            method: InputMethod::Vni,
            auto_restore: false,
            ..Default::default()
        };
        assert_eq!(typed_with(config, address), *address, "{address:?}");
    }
}

/// The tone a word already earned is not taken away by the punctuation that
/// ends it.
#[test]
fn punctuation_keeps_the_tone_it_follows() {
    assert_eq!(typed(InputMethod::Telex, "chaof."), "chào.");
    assert_eq!(typed(InputMethod::Telex, "chaof,"), "chào,");
    assert_eq!(typed(InputMethod::Telex, "chaof-em"), "chào-em");
    assert_eq!(typed(InputMethod::Vni, "chao2."), "chào.");
}

/// Every printable key must leave something behind, whatever precedes it.
#[test]
fn no_keystroke_disappears_after_a_boundary() {
    let boundaries = "()[]{}.,!?;:\"'`/\\|-_ \t";
    let alphabet: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();

    for method in [InputMethod::Vni, InputMethod::Telex] {
        for boundary in boundaries.chars() {
            for &ch in &alphabet {
                let mut screen = Screen::new(method);
                screen.press(boundary);
                let before = screen.text.clone();
                screen.press(ch);

                assert_ne!(
                    screen.text, before,
                    "{method:?}: {boundary:?} then {ch:?} put nothing on screen"
                );
            }
        }
    }
}
