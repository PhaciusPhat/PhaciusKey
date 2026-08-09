use crate::buffer::CompositionBuffer;
use crate::methods::{apply_case_mask, InputMethodProcessor, TelexMethod, VniMethod};
use crate::tone_placement::apply_tone;
use crate::types::{Config, EditAction, InputMethod, Keystroke, Tone};
use crate::validator::{base_vowel, coda_of, is_valid_prefix, tone_allowed_with_coda};

pub struct Engine {
    buffer: CompositionBuffer,
    config: Config,
    passthrough: bool,
    history: Vec<CommittedWord>,
    pending_sentence_end: bool,
    capitalize_next: bool,
}

struct CommittedWord {
    raw: String,
    displayed: String,
    passthrough: bool,
    boundaries: usize,
}

const HISTORY_DEPTH: usize = 16;

impl Engine {
    pub fn new(config: Config) -> Self {
        Self {
            buffer: CompositionBuffer::new(),
            config,
            passthrough: false,
            history: Vec::new(),
            pending_sentence_end: false,
            capitalize_next: false,
        }
    }

    pub fn set_config(&mut self, config: Config) {
        if config.method != self.config.method {
            self.history.clear();
        }
        self.config = config;
    }

    /// Process one keystroke and return the edit actions the shell must execute.
    pub fn process(&mut self, key: Keystroke) -> Vec<EditAction> {
        if !self.config.enabled {
            return vec![];
        }

        if key.is_boundary {
            return self.commit_and_reset(Some(key.ch), true);
        }

        let ch = key.ch;

        if is_word_boundary(ch) {
            return self.commit_and_reset(Some(ch), true);
        }

        self.pending_sentence_end = false;

        self.buffer.push(ch);
        let actions = self.recompute();
        if actions.is_empty() {
            return vec![EditAction::Insert(String::new())];
        }
        actions
    }

    /// Delete one displayed character and keep composing; empty means pass the Backspace through.
    pub fn backspace(&mut self) -> Vec<EditAction> {
        if !self.config.enabled {
            return vec![];
        }

        if self.buffer.displayed.is_empty() {
            let Some(last) = self.history.last_mut() else {
                return vec![];
            };
            last.boundaries -= 1;
            if last.boundaries > 0 {
                return vec![];
            }
            if let Some(word) = self.history.pop() {
                self.buffer.raw = word.raw;
                self.buffer.displayed = word.displayed;
                self.passthrough = word.passthrough;
            }
            return vec![];
        }

        let mut chars: Vec<char> = self.buffer.displayed.chars().collect();
        chars.pop();
        let target: String = chars.into_iter().collect();

        let actions = self.buffer.diff_to(&target);
        self.buffer.raw = match self.config.method {
            InputMethod::Telex => crate::methods::telex::encode_telex(&target),
            InputMethod::Vni => crate::methods::vni::encode_vni(&target),
        };
        self.passthrough = false;
        actions
    }

    fn recompute(&mut self) -> Vec<EditAction> {
        let raw = self.buffer.raw.clone();
        let result = match self.config.method {
            InputMethod::Telex => TelexMethod.process(&raw, &self.config),
            InputMethod::Vni => VniMethod.process(&raw, &self.config),
        };

        let method_result = match result {
            Some(r) => r,
            None => return vec![],
        };

        let bare = &method_result.bare;
        let tone = method_result.tone;

        let impossible = !self.syllable_possible(bare)
            || method_result.is_foreign
            || (tone != Tone::Flat && !tone_allowed_with_coda(tone, coda_of(bare)));

        let was_passthrough = self.passthrough;

        if self.config.auto_restore && impossible {
            self.passthrough = true;
        }

        if self.passthrough {
            let text = match &method_result.literal {
                Some(literal) if !was_passthrough && !method_result.is_foreign => {
                    apply_case_mask(literal, &method_result.case_mask)
                }
                _ => raw,
            };
            return self.show(&text);
        }

        if let Some(literal) = &method_result.literal {
            let literal = apply_case_mask(literal, &method_result.case_mask);
            return self.show(&literal);
        }

        let target = apply_tone(bare, tone, self.config.placement);
        let target = apply_case_mask(&target, &method_result.case_mask);

        self.show(&target)
    }

    fn syllable_possible(&self, bare: &str) -> bool {
        is_valid_prefix(bare)
            || self
                .expand_quick_consonants(bare)
                .is_some_and(|e| is_valid_prefix(&e))
    }

    fn quick_consonant_expansion(&self) -> Option<String> {
        let out = self.expand_quick_consonants(&self.buffer.displayed)?;
        (out != self.buffer.displayed && is_valid_prefix(&out)).then_some(out)
    }

    fn expand_quick_consonants(&self, word: &str) -> Option<String> {
        if self.config.method != InputMethod::Telex
            || !(self.config.quick_start_consonant || self.config.quick_end_consonant)
        {
            return None;
        }
        let chars: Vec<char> = word.chars().collect();
        if chars.len() < 2 {
            return None;
        }

        let head = self
            .config
            .quick_start_consonant
            .then(|| start_consonant(lower(chars[0])))
            .flatten()
            .map(|(a, b)| {
                let first = chars[0].is_uppercase();
                [cased(a, first), cased(b, first && chars[1].is_uppercase())]
            });

        let last = chars[chars.len() - 1];
        let tail = self
            .config
            .quick_end_consonant
            .then(|| is_vowel(chars[chars.len() - 2]).then(|| end_consonant(lower(last))))
            .flatten()
            .flatten()
            .map(|(a, b)| {
                let upper = last.is_uppercase();
                [cased(a, upper), cased(b, upper)]
            });

        if head.is_none() && tail.is_none() {
            return None;
        }

        let body = &chars[usize::from(head.is_some())..chars.len() - usize::from(tail.is_some())];
        let mut out: String = head.into_iter().flatten().collect();
        out.extend(body);
        out.extend(tail.into_iter().flatten());
        Some(out)
    }

    fn show(&mut self, text: &str) -> Vec<EditAction> {
        if !self.config.auto_capitalize || !self.capitalize_next {
            return self.buffer.diff_to(text);
        }
        let mut chars = text.chars();
        let capitalized: String = match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => String::new(),
        };
        self.buffer.diff_to(&capitalized)
    }

    /// Commit the current word without a boundary character (Tab).
    pub fn commit_word(&mut self) -> Vec<EditAction> {
        if !self.config.enabled {
            return vec![];
        }
        self.commit_and_reset(None, false)
    }

    /// Commit the current word and start a new sentence (Enter).
    pub fn commit_line(&mut self) -> Vec<EditAction> {
        if !self.config.enabled {
            return vec![];
        }
        let actions = self.commit_and_reset(None, true);
        self.pending_sentence_end = false;
        self.capitalize_next = true;
        actions
    }

    fn commit_and_reset(
        &mut self,
        extra_char: Option<char>,
        expand_shortcuts: bool,
    ) -> Vec<EditAction> {
        let mut actions = vec![];

        let mut expanded = false;
        if let Some(expansion) = self.config.macros.get(&self.buffer.displayed) {
            let shown = self.buffer.displayed.chars().count();
            if shown > 0 && shown <= u8::MAX as usize && *expansion != self.buffer.displayed {
                actions.push(EditAction::Backspace(shown as u8));
                actions.push(EditAction::Insert(expansion.clone()));
                expanded = true;
            }
        }

        if !expanded && expand_shortcuts {
            if let Some(text) = self.quick_consonant_expansion() {
                actions.extend(self.buffer.diff_to(&text));
                self.buffer.raw = match self.config.method {
                    InputMethod::Telex => crate::methods::telex::encode_telex(&text),
                    InputMethod::Vni => crate::methods::vni::encode_vni(&text),
                };
            }
        }

        let boundary_on_screen = matches!(extra_char, Some(ch) if ch != '\0');
        if expanded || !boundary_on_screen {
            self.history.clear();
        } else if self.buffer.displayed.is_empty() {
            if let Some(last) = self.history.last_mut() {
                last.boundaries += 1;
            }
        } else {
            if self.history.len() == HISTORY_DEPTH {
                self.history.remove(0);
            }
            self.history.push(CommittedWord {
                raw: self.buffer.raw.clone(),
                displayed: self.buffer.displayed.clone(),
                passthrough: self.passthrough,
                boundaries: 1,
            });
        }

        match extra_char {
            Some('\n' | '\r') => {
                self.pending_sentence_end = false;
                self.capitalize_next = true;
            }
            Some('.' | '!' | '?') => {
                self.pending_sentence_end = true;
                self.capitalize_next = false;
            }
            Some(ch) if ch.is_whitespace() => {
                if self.pending_sentence_end {
                    self.pending_sentence_end = false;
                    self.capitalize_next = true;
                }
            }
            Some(_) => {
                self.pending_sentence_end = false;
                self.capitalize_next = false;
            }
            None => {}
        }

        self.buffer.reset();
        self.passthrough = false;

        if let Some(ch) = extra_char {
            if ch != '\0' {
                actions.push(EditAction::Insert(ch.to_string()));
            }
        }

        actions
    }

    /// Put the raw keystrokes back (Esc): "đấy" → "ddaays". Empty if there is nothing to restore.
    pub fn restore_raw(&mut self) -> Vec<EditAction> {
        if !self.config.enabled || self.buffer.raw.is_empty() {
            return vec![];
        }
        if self.buffer.displayed == self.buffer.raw {
            return vec![];
        }
        let raw = self.buffer.raw.clone();
        self.passthrough = true;
        self.buffer.diff_to(&raw)
    }

    /// Force-reset the buffer (e.g. on mouse click / focus change).
    pub fn reset(&mut self) {
        self.buffer.reset();
        self.passthrough = false;
        self.history.clear();
        self.pending_sentence_end = false;
        self.capitalize_next = false;
    }

    /// The string currently displayed on-screen for the active word.
    pub fn current_displayed(&self) -> String {
        self.buffer.displayed.clone()
    }
}

// OpenKey _quickStartConsonant.
fn start_consonant(ch: char) -> Option<(char, char)> {
    match ch {
        'f' => Some(('p', 'h')),
        'j' => Some(('g', 'i')),
        'w' => Some(('q', 'u')),
        _ => None,
    }
}

// OpenKey _quickEndConsonant.
fn end_consonant(ch: char) -> Option<(char, char)> {
    match ch {
        'g' => Some(('n', 'g')),
        'h' => Some(('n', 'h')),
        'k' => Some(('c', 'h')),
        _ => None,
    }
}

fn lower(ch: char) -> char {
    ch.to_lowercase().next().unwrap_or(ch)
}

fn cased(ch: char, upper: bool) -> char {
    if upper {
        ch.to_uppercase().next().unwrap_or(ch)
    } else {
        ch
    }
}

fn is_vowel(ch: char) -> bool {
    let lower = lower(ch);
    let base = base_vowel(lower).unwrap_or(lower);
    matches!(
        base,
        'a' | 'â' | 'ă' | 'e' | 'ê' | 'i' | 'o' | 'ô' | 'ơ' | 'u' | 'ư' | 'y'
    )
}

fn is_word_boundary(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t'
            | '\n'
            | '\r'
            | '.'
            | ','
            | '!'
            | '?'
            | ';'
            | ':'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '"'
            | '\''
            | '`'
            | '/'
            | '\\'
            | '|'
            | '-'
            | '_'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Config, Keystroke};

    fn engine() -> Engine {
        Engine::new(Config::default())
    }

    fn type_str(e: &mut Engine, s: &str) -> Vec<EditAction> {
        let mut all = vec![];
        for ch in s.chars() {
            all = e.process(Keystroke::char(ch));
        }
        all
    }

    #[test]
    fn telex_viet() {
        let mut e = engine();
        type_str(&mut e, "vieetj");
        assert_eq!(e.buffer.displayed, "việt");
    }

    #[test]
    fn telex_viet_sharp() {
        let mut e = engine();
        type_str(&mut e, "vieets");
        assert_eq!(e.buffer.displayed, "viết");
    }

    #[test]
    fn telex_ha_sharp() {
        let mut e = engine();
        type_str(&mut e, "has");
        assert_eq!(e.buffer.displayed, "há");
    }

    #[test]
    fn preserves_leading_capital() {
        let mut e = engine();
        type_str(&mut e, "Has");
        assert_eq!(e.buffer.displayed, "Há");
    }

    #[test]
    fn preserves_all_caps() {
        let mut e = engine();
        type_str(&mut e, "VIEETS");
        assert_eq!(e.buffer.displayed, "VIẾT");
    }

    #[test]
    fn lowercase_stays_lowercase() {
        let mut e = engine();
        type_str(&mut e, "vieets");
        assert_eq!(e.buffer.displayed, "viết");
    }

    #[test]
    fn reset_clears_buffer() {
        let mut e = engine();
        type_str(&mut e, "ha");
        e.reset();
        assert!(e.buffer.raw.is_empty());
        assert!(e.buffer.displayed.is_empty());
    }

    #[test]
    fn disabled_engine_passthrough() {
        let mut e = Engine::new(Config {
            enabled: false,
            ..Default::default()
        });
        let actions = e.process(Keystroke::char('a'));
        assert!(actions.is_empty());
    }

    #[test]
    fn backspace_deletes_a_displayed_character() {
        let mut e = engine();
        type_str(&mut e, "as");
        assert_eq!(e.buffer.displayed, "á");

        let actions = e.backspace();
        assert_eq!(actions, vec![EditAction::Backspace(1)]);
        assert!(e.buffer.raw.is_empty());
        assert!(e.buffer.displayed.is_empty());
    }

    #[test]
    fn backspace_removes_a_letter_not_the_tone_mark() {
        let mut e = engine();
        type_str(&mut e, "ddoans");
        assert_eq!(e.buffer.displayed, "đoán");

        let actions = e.backspace();
        assert_eq!(actions, vec![EditAction::Backspace(1)]);
    }

    #[test]
    fn backspace_to_empty_clears_buffer() {
        let mut e = engine();
        type_str(&mut e, "a");
        let actions = e.backspace();
        assert_eq!(actions, vec![EditAction::Backspace(1)]);
        assert!(e.buffer.raw.is_empty());
        assert!(e.buffer.displayed.is_empty());
    }

    #[test]
    fn backspace_when_not_composing_passes_through() {
        let mut e = engine();
        assert!(e.backspace().is_empty());
    }

    #[test]
    fn typing_after_backspace_to_empty_starts_fresh() {
        let mut e = engine();
        type_str(&mut e, "as");
        e.backspace();
        let actions = type_str(&mut e, "b");
        assert_eq!(actions, vec![EditAction::Insert("b".into())]);
        assert_eq!(e.buffer.raw, "b");
    }

    #[test]
    fn backspace_after_space_resumes_the_word() {
        let mut e = engine();
        type_str(&mut e, "ddoans");
        e.process(Keystroke::char(' '));
        let actions = e.backspace();
        assert!(actions.is_empty());
        assert_eq!(e.buffer.displayed, "đoán");
        type_str(&mut e, "s");
        assert_eq!(e.buffer.displayed, "đoans");
    }

    #[test]
    fn backspace_walks_back_over_consecutive_spaces() {
        let mut e = engine();
        type_str(&mut e, "ddoans");
        e.process(Keystroke::char(' '));
        e.process(Keystroke::char(' '));
        assert!(e.backspace().is_empty());
        assert!(e.buffer.displayed.is_empty());
        assert!(e.backspace().is_empty());
        assert_eq!(e.buffer.displayed, "đoán");
        type_str(&mut e, "s");
        assert_eq!(e.buffer.displayed, "đoans");
    }

    #[test]
    fn backspace_after_macro_expansion_does_not_resume() {
        let mut config = Config::default();
        config.macros.insert("btw".into(), "by the way".into());
        let mut e = Engine::new(config);
        type_str(&mut e, "btw");
        e.process(Keystroke::char(' '));
        assert!(e.backspace().is_empty());
        assert!(e.buffer.displayed.is_empty());
    }

    #[test]
    fn backspace_after_enter_commit_does_not_resume() {
        let mut e = engine();
        type_str(&mut e, "as");
        e.commit_word();
        assert!(e.backspace().is_empty());
        assert!(e.buffer.displayed.is_empty());
    }

    #[test]
    fn deleting_a_new_word_walks_back_into_the_previous_one() {
        let mut e = engine();
        type_str(&mut e, "as");
        e.process(Keystroke::char(' '));
        type_str(&mut e, "b");
        e.backspace();
        assert!(e.backspace().is_empty());
        assert_eq!(e.buffer.displayed, "á");
    }

    #[test]
    fn deleting_a_restored_word_resumes_the_one_before_it() {
        let mut e = engine();
        type_str(&mut e, "has");
        e.process(Keystroke::char(' '));
        type_str(&mut e, "cas");
        e.process(Keystroke::char(' '));

        assert!(e.backspace().is_empty());
        assert_eq!(e.buffer.displayed, "cá");
        e.backspace();
        e.backspace();
        assert!(e.buffer.displayed.is_empty());

        assert!(e.backspace().is_empty());
        assert_eq!(e.buffer.displayed, "há");
        type_str(&mut e, "s");
        assert_eq!(e.buffer.displayed, "has");
    }

    #[test]
    fn resume_restores_passthrough_of_an_escaped_word() {
        let mut e = engine();
        type_str(&mut e, "ddaays");
        e.restore_raw();
        assert_eq!(e.buffer.displayed, "ddaays");
        e.process(Keystroke::char(' '));
        assert!(e.backspace().is_empty());
        assert_eq!(e.buffer.displayed, "ddaays");
        type_str(&mut e, "x");
        assert_eq!(e.buffer.displayed, "ddaaysx");
    }

    #[test]
    fn macro_expansion_clears_the_whole_history() {
        let mut config = Config::default();
        config.macros.insert("btw".into(), "by the way".into());
        let mut e = Engine::new(config);
        type_str(&mut e, "as");
        e.process(Keystroke::char(' '));
        type_str(&mut e, "btw");
        e.process(Keystroke::char(' '));
        assert!(e.backspace().is_empty());
        assert!(e.buffer.displayed.is_empty());
        assert!(e.backspace().is_empty());
        assert!(e.buffer.displayed.is_empty());
    }

    #[test]
    fn reset_clears_the_history() {
        let mut e = engine();
        type_str(&mut e, "as");
        e.process(Keystroke::char(' '));
        e.reset();
        assert!(e.backspace().is_empty());
        assert!(e.buffer.displayed.is_empty());
    }

    fn quick_engine(start: bool, end: bool) -> Engine {
        Engine::new(Config {
            quick_start_consonant: start,
            quick_end_consonant: end,
            ..Default::default()
        })
    }

    #[test]
    fn quick_start_consonant_expands_at_the_word_boundary() {
        let mut e = quick_engine(true, false);
        type_str(&mut e, "fa");
        assert_eq!(e.buffer.displayed, "fa");

        let actions = e.process(Keystroke::char(' '));
        assert_eq!(
            actions,
            vec![
                EditAction::Backspace(2),
                EditAction::Insert("pha".into()),
                EditAction::Insert(" ".into()),
            ]
        );
    }

    #[test]
    fn quick_end_consonant_expands_after_a_vowel() {
        let mut e = quick_engine(false, true);
        type_str(&mut e, "hag");
        let actions = e.process(Keystroke::char(' '));
        assert_eq!(
            actions,
            vec![
                EditAction::Backspace(1),
                EditAction::Insert("ng".into()),
                EditAction::Insert(" ".into()),
            ]
        );
    }

    #[test]
    fn quick_end_consonant_needs_a_vowel_before_it() {
        let mut e = quick_engine(false, true);
        type_str(&mut e, "anh");
        let actions = e.process(Keystroke::char(' '));
        assert_eq!(actions, vec![EditAction::Insert(" ".into())]);
    }

    #[test]
    fn the_expansion_keeps_a_tone_already_placed() {
        let mut e = quick_engine(false, true);
        type_str(&mut e, "hags");
        assert_eq!(e.buffer.displayed, "hág");

        let actions = e.process(Keystroke::char(' '));
        assert_eq!(
            actions,
            vec![
                EditAction::Backspace(1),
                EditAction::Insert("ng".into()),
                EditAction::Insert(" ".into()),
            ]
        );
    }

    #[test]
    fn both_shortcuts_can_fire_in_one_word() {
        let mut e = quick_engine(true, true);
        type_str(&mut e, "fag");
        let actions = e.process(Keystroke::char(' '));
        assert_eq!(
            actions,
            vec![
                EditAction::Backspace(3),
                EditAction::Insert("phang".into()),
                EditAction::Insert(" ".into()),
            ]
        );
    }

    #[test]
    fn an_expansion_that_is_not_vietnamese_is_left_alone() {
        let mut e = quick_engine(true, true);
        type_str(&mut e, "for");
        let actions = e.process(Keystroke::char(' '));
        assert_eq!(actions, vec![EditAction::Insert(" ".into())]);
    }

    #[test]
    fn quick_consonants_are_off_by_default() {
        let mut e = engine();
        type_str(&mut e, "fa");
        let actions = e.process(Keystroke::char(' '));
        assert_eq!(actions, vec![EditAction::Insert(" ".into())]);
    }

    #[test]
    fn tab_commits_without_expanding() {
        let mut e = quick_engine(true, true);
        type_str(&mut e, "fag");
        assert!(e.commit_word().is_empty());
    }

    #[test]
    fn enter_commits_and_expands() {
        let mut e = quick_engine(true, true);
        type_str(&mut e, "fag");
        assert_eq!(
            e.commit_line(),
            vec![EditAction::Backspace(3), EditAction::Insert("phang".into())]
        );
    }

    #[test]
    fn backspace_after_an_expansion_resumes_the_expanded_word() {
        let mut e = quick_engine(true, true);
        type_str(&mut e, "fag");
        e.process(Keystroke::char(' '));
        assert!(e.backspace().is_empty());
        assert_eq!(e.buffer.displayed, "phang");
    }

    fn capitalizing_engine() -> Engine {
        Engine::new(Config {
            auto_capitalize: true,
            ..Default::default()
        })
    }

    #[test]
    fn sentence_start_is_capitalized_after_a_full_stop_and_space() {
        let mut e = capitalizing_engine();
        type_str(&mut e, "chaof");
        e.process(Keystroke::char('.'));
        e.process(Keystroke::char(' '));
        type_str(&mut e, "toi");
        assert_eq!(e.buffer.displayed, "Toi");
    }

    #[test]
    fn the_capital_survives_later_diacritic_and_tone_keys() {
        let mut e = capitalizing_engine();
        type_str(&mut e, "chaof");
        e.process(Keystroke::char('!'));
        e.process(Keystroke::char(' '));
        type_str(&mut e, "toois");
        assert_eq!(e.buffer.displayed, "Tối");
    }

    #[test]
    fn capitalization_is_off_by_default() {
        let mut e = engine();
        type_str(&mut e, "chaof");
        e.process(Keystroke::char('.'));
        e.process(Keystroke::char(' '));
        type_str(&mut e, "toi");
        assert_eq!(e.buffer.displayed, "toi");
    }

    #[test]
    fn a_full_stop_with_no_space_after_it_starts_no_sentence() {
        let mut e = capitalizing_engine();
        type_str(&mut e, "a");
        e.process(Keystroke::char('.'));
        type_str(&mut e, "b");
        assert_eq!(e.buffer.displayed, "b");
    }

    #[test]
    fn a_comma_does_not_start_a_sentence() {
        let mut e = capitalizing_engine();
        type_str(&mut e, "a");
        e.process(Keystroke::char(','));
        e.process(Keystroke::char(' '));
        type_str(&mut e, "toi");
        assert_eq!(e.buffer.displayed, "toi");
    }

    #[test]
    fn extra_spaces_after_the_full_stop_keep_the_sentence_pending() {
        let mut e = capitalizing_engine();
        type_str(&mut e, "a");
        e.process(Keystroke::char('.'));
        e.process(Keystroke::char(' '));
        e.process(Keystroke::char(' '));
        type_str(&mut e, "toi");
        assert_eq!(e.buffer.displayed, "Toi");
    }

    #[test]
    fn a_newline_starts_a_sentence_on_its_own() {
        let mut e = capitalizing_engine();
        type_str(&mut e, "a");
        e.process(Keystroke::char('\n'));
        type_str(&mut e, "toi");
        assert_eq!(e.buffer.displayed, "Toi");
    }

    #[test]
    fn reset_forgets_a_pending_sentence_start() {
        let mut e = capitalizing_engine();
        type_str(&mut e, "a");
        e.process(Keystroke::char('.'));
        e.process(Keystroke::char(' '));
        e.reset();
        type_str(&mut e, "toi");
        assert_eq!(e.buffer.displayed, "toi");
    }

    #[test]
    fn a_restored_foreign_word_is_capitalized_too() {
        let mut e = capitalizing_engine();
        type_str(&mut e, "a");
        e.process(Keystroke::char('.'));
        e.process(Keystroke::char(' '));
        type_str(&mut e, "reset");
        assert_eq!(e.buffer.displayed, "Reset");
    }

    #[test]
    fn typing_after_backspace_continues_the_composition() {
        let mut e = engine();
        type_str(&mut e, "ddoans");
        e.backspace();
        assert_eq!(e.buffer.displayed, "đoá");
        assert_eq!(e.buffer.raw, "ddoas");
        type_str(&mut e, "n");
        assert_eq!(e.buffer.displayed, "đoán");
    }
}
