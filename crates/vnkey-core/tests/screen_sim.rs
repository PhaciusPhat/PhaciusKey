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

/// A word the engine gives up on without any key having undone itself reads
/// back exactly as typed, which is what lets an English word through whole.
#[test]
fn a_restored_word_reads_back_key_for_key() {
    for sequence in ["address", "moods", "goods", "food", "settings", "toppings"] {
        assert_eq!(typed(InputMethod::Telex, sequence), sequence);
    }
}

/// Every press inside a word an undo has made literal is worth exactly one
/// character, the undo's own press included.
#[test]
fn a_press_after_an_undone_tone_types_one_character() {
    let mut screen = Screen::new(InputMethod::Telex);
    assert_eq!(screen.type_str("phas"), "phá");
    assert_eq!(screen.type_str("s"), "phas");
    assert_eq!(screen.type_str("r"), "phasr");
}

#[test]
fn an_undo_keeps_a_letter_the_keys_composed() {
    for (method, seq, undo, word, undone) in [
        (InputMethod::Telex, "ddoans", 's', "đoán", "đoans"),
        (InputMethod::Telex, "ddoasn", 's', "đoán", "đoans"),
        (InputMethod::Telex, "vieetj", 'j', "việt", "viêtj"),
        (InputMethod::Telex, "vietej", 'j', "việt", "viêtj"),
        (InputMethod::Telex, "tieengs", 's', "tiếng", "tiêngs"),
        (InputMethod::Telex, "tieesng", 's', "tiếng", "tiêngs"),
        (InputMethod::Telex, "tienges", 's', "tiếng", "tiêngs"),
        (InputMethod::Telex, "muonos", 's', "muốn", "muôns"),
        (InputMethod::Telex, "dduwowcj", 'j', "được", "đươcj"),
        (InputMethod::Telex, "cuwar", 'r', "cửa", "cưar"),
        (InputMethod::Telex, "thuwowngf", 'f', "thường", "thươngf"),
        (InputMethod::Vni, "d9oan1", '1', "đoán", "đoan1"),
        (InputMethod::Vni, "vie6t5", '5', "việt", "viêt5"),
        (InputMethod::Vni, "tie6ng1", '1', "tiếng", "tiêng1"),
    ] {
        let mut screen = Screen::new(method);
        assert_eq!(screen.type_str(seq), word, "{method:?} {seq:?}");
        screen.press(undo);
        assert_eq!(screen.text, undone, "{method:?} {seq:?} then {undo:?}");
    }
}

#[test]
fn a_press_after_an_undo_that_kept_a_composed_letter_types_one_character() {
    assert_eq!(typed(InputMethod::Telex, "ddoanssn"), "đoansn");
    assert_eq!(typed(InputMethod::Telex, "ddoanssnn"), "đoansnn");
    assert_eq!(typed(InputMethod::Telex, "vieetjjt"), "viêtjt");
    assert_eq!(typed(InputMethod::Vni, "d9oan11n"), "đoan1n");
}

#[test]
fn an_undo_types_its_key_once_whatever_it_leaves() {
    for (method, seq, undo, word, undone) in [
        (InputMethod::Telex, "w", 'w', "ư", "w"),
        (InputMethod::Telex, "phas", 's', "phá", "phas"),
        (InputMethod::Telex, "toans", 's', "toán", "toans"),
        (InputMethod::Telex, "dor", 'r', "dỏ", "dor"),
        (InputMethod::Telex, "does", 's', "dóe", "does"),
        (InputMethod::Telex, "bos", 's', "bó", "bos"),
        (InputMethod::Vni, "pha1", '1', "phá", "pha1"),
        (InputMethod::Vni, "do1", '1', "dó", "do1"),
    ] {
        let mut screen = Screen::new(method);
        assert_eq!(screen.type_str(seq), word, "{method:?} {seq:?}");
        screen.press(undo);
        assert_eq!(screen.text, undone, "{method:?} {seq:?} then {undo:?}");
    }
}

#[test]
fn a_restored_spelling_gives_both_keys_back() {
    for (method, seq, undo, word, undone) in [
        (InputMethod::Telex, "dd", 'd', "đ", "dd"),
        (InputMethod::Telex, "ow", 'w', "ơ", "ow"),
        (InputMethod::Telex, "uw", 'w', "ư", "uw"),
        (InputMethod::Vni, "d9", '9', "đ", "d9"),
        (InputMethod::Vni, "a6", '6', "â", "a6"),
    ] {
        let mut screen = Screen::new(method);
        assert_eq!(screen.type_str(seq), word, "{method:?} {seq:?}");
        screen.press(undo);
        assert_eq!(screen.text, undone, "{method:?} {seq:?} then {undo:?}");
    }
}

/// The key after a restored spelling types itself, rather than the display
/// jumping back to the keys the restore already spent.
#[test]
fn a_key_after_a_restored_spelling_types_one_character() {
    assert_eq!(typed(InputMethod::Telex, "aaas"), "aas");
    assert_eq!(typed(InputMethod::Telex, "aaaf"), "aaf");
    assert_eq!(typed(InputMethod::Telex, "dddo"), "ddo");
}

#[test]
fn an_undo_keeps_the_case_the_word_was_typed_in() {
    assert_eq!(typed(InputMethod::Telex, "Ddoanss"), "Đoans");
    assert_eq!(typed(InputMethod::Telex, "DDOANSS"), "ĐOANS");
    assert_eq!(typed(InputMethod::Telex, "VIEETJJ"), "VIÊTJ");
    assert_eq!(typed(InputMethod::Telex, "HaSS"), "HaS");
    assert_eq!(typed(InputMethod::Telex, "Ww"), "w");
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
