use vnkey_core::{Config, EditAction, Engine, InputMethod, Keystroke, TonePlacementMode};

fn engine(method: InputMethod) -> Engine {
    Engine::new(Config {
        method,
        placement: TonePlacementMode::Modern,
        enabled: true,
        auto_restore: true,
        ..Default::default()
    })
}

fn typed(seq: &str, method: InputMethod) -> String {
    let mut e = engine(method);
    for ch in seq.chars() {
        e.process(Keystroke::char(ch));
    }
    e.current_displayed()
}

fn telex(seq: &str) -> String {
    typed(seq, InputMethod::Telex)
}

fn vni(seq: &str) -> String {
    typed(seq, InputMethod::Vni)
}

#[test]
fn late_modifier_keys_apply_to_the_word() {
    for (seq, want) in [
        ("loaji", "loại"),
        ("loafi", "loài"),
        ("ddoori", "đổi"),
        ("vietej", "việt"),
        ("sasu", "sáu"),
        ("muono", "muôn"),
    ] {
        assert_eq!(telex(seq), want, "telex {seq:?}");
    }
}

#[test]
fn uo_after_horn_is_corrected_to_uo_horn() {
    assert_eq!(telex("ruwou"), "rươu");
    assert_eq!(telex("ruwouj"), "rượu");
    assert_eq!(vni("ru7ou"), "rươu");
    assert_eq!(vni("ru7ou5"), "rượu");
}

#[test]
fn english_words_still_restored_verbatim() {
    for word in ["user", "reset", "there", "case", "photo", "note", "date"] {
        assert_eq!(telex(word), word, "expected {word:?} to survive untouched");
    }
}

struct Screen {
    engine: Engine,
    text: String,
}

impl Screen {
    fn new(method: InputMethod) -> Self {
        Self {
            engine: engine(method),
            text: String::new(),
        }
    }

    fn apply(&mut self, actions: Vec<EditAction>) {
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

    fn type_str(&mut self, seq: &str) -> &mut Self {
        for ch in seq.chars() {
            let actions = self.engine.process(Keystroke::char(ch));
            if actions.is_empty() {
                self.text.push(ch);
            } else {
                self.apply(actions);
            }
        }
        self
    }

    fn backspace(&mut self) -> &mut Self {
        let actions = self.engine.backspace();
        if actions.is_empty() {
            self.text.pop();
        } else {
            self.apply(actions);
        }
        self
    }

    #[track_caller]
    fn expect(&mut self, want: &str) -> &mut Self {
        assert_eq!(self.text, want);
        self
    }
}

#[test]
fn backspace_then_retype_recomposes_the_word() {
    Screen::new(InputMethod::Telex)
        .type_str("ruwowuj")
        .expect("rượu")
        .backspace()
        .expect("rượ")
        .type_str("u")
        .expect("rượu");
}

#[test]
fn backspace_twice_then_retype_autocorrects() {
    Screen::new(InputMethod::Telex)
        .type_str("ruwowuj")
        .expect("rượu")
        .backspace()
        .backspace()
        .expect("rư")
        .type_str("ou")
        .expect("rươu")
        .type_str("j")
        .expect("rượu");
}

#[test]
fn backspace_then_retype_keeps_the_tone() {
    Screen::new(InputMethod::Telex)
        .type_str("ddoans")
        .expect("đoán")
        .backspace()
        .expect("đoá")
        .type_str("n")
        .expect("đoán");
}

#[test]
fn backspace_then_retype_keeps_the_diacritic() {
    Screen::new(InputMethod::Telex)
        .type_str("vieetj")
        .expect("việt")
        .backspace()
        .expect("việ")
        .type_str("t")
        .expect("việt");
}

#[test]
fn backspace_in_a_foreign_word_stays_verbatim() {
    Screen::new(InputMethod::Telex)
        .type_str("jira")
        .expect("jira")
        .backspace()
        .expect("jir")
        .type_str("a")
        .expect("jira");
}

#[test]
fn vni_backspace_then_retype_recomposes() {
    Screen::new(InputMethod::Vni)
        .type_str("d9o6i3")
        .expect("đổi")
        .backspace()
        .expect("đổ")
        .type_str("i")
        .expect("đổi");
}
