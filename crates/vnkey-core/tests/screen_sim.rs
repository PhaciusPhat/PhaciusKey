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
        Self {
            engine: Engine::new(Config {
                method,
                ..Default::default()
            }),
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
