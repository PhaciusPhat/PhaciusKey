//! Regression corpus for the v0.0.12 fixes: modifier keys arriving *after* the
//! word ("loaji", "ddoori", "vietej"), the ưo → ươ auto-correction, and
//! Backspace keeping the composition alive so deleting and re-typing inside a
//! word recomposes it instead of breaking it ("rượu" → "rượ" → "rượu").

use vnkey_core::{Config, EditAction, Engine, InputMethod, Keystroke, TonePlacementMode};

fn engine(method: InputMethod) -> Engine {
    Engine::new(Config {
        method,
        placement: TonePlacementMode::Modern,
        enabled: true,
        auto_restore: true,
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

// ── Late modifier keys ────────────────────────────────────────────────────────

/// A tone key typed before the closing vowel of the rime, or a doubling vowel
/// typed after the coda, still applies to the word: "loaji" → "loại", not raw
/// keystrokes handed back as foreign.
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

// ── ưo → ươ auto-correction ──────────────────────────────────────────────────

/// "ưo" never occurs in Vietnamese orthography — a plain 'o' after 'ư' is
/// always "ươ", so the engine corrects it without waiting for a horn key.
#[test]
fn uo_after_horn_is_corrected_to_uo_horn() {
    assert_eq!(telex("ruwou"), "rươu");
    assert_eq!(telex("ruwouj"), "rượu");
    assert_eq!(vni("ru7ou"), "rươu");
    assert_eq!(vni("ru7ou5"), "rượu");
}

// ── English still survives the relaxed rules ─────────────────────────────────

/// The rules were only *relaxed* for closing vowels; English words that used to
/// come back verbatim must still come back verbatim.
#[test]
fn english_words_still_restored_verbatim() {
    for word in ["user", "reset", "there", "case", "photo", "note", "date"] {
        assert_eq!(telex(word), word, "expected {word:?} to survive untouched");
    }
}

// ── Backspace keeps composing ────────────────────────────────────────────────

/// Drives an engine the way the platform layer does, applying `EditAction`s to
/// a screen string. An empty action list means the key passes through natively:
/// the char is typed as-is, or the native Backspace removes one char.
struct Screen {
    engine: Engine,
    text: String,
}

impl Screen {
    fn new(method: InputMethod) -> Self {
        Self { engine: engine(method), text: String::new() }
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

/// The reported bug: "rượu" → Backspace → re-type must give "rượu" again, not
/// "rưoự" (the old engine forgot the word and composed "uj" from scratch).
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

/// Deleting deeper and re-typing the tail also auto-corrects: "rư" + "ou" must
/// become "rươu" (ưo → ươ), then the tone key finishes "rượu".
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

/// A foreign (passthrough) word must survive delete + re-type untouched rather
/// than being re-composed into diacritics.
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
