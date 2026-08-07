use crate::buffer::CompositionBuffer;
use crate::methods::{apply_case_mask, InputMethodProcessor, TelexMethod, VniMethod};
use crate::tone_placement::apply_tone;
use crate::types::{Config, EditAction, InputMethod, Keystroke, Tone};
use crate::validator::{coda_of, is_valid_prefix, tone_allowed_with_coda};

pub struct Engine {
    buffer: CompositionBuffer,
    config: Config,
    /// Set once the current word has proved it cannot be Vietnamese. Cleared at
    /// the next word boundary. While set, keystrokes are echoed verbatim.
    passthrough: bool,
}

impl Engine {
    pub fn new(config: Config) -> Self {
        Self { buffer: CompositionBuffer::new(), config, passthrough: false }
    }

    pub fn set_config(&mut self, config: Config) {
        self.config = config;
    }

    /// Process one keystroke and return the edit actions the shell must execute.
    pub fn process(&mut self, key: Keystroke) -> Vec<EditAction> {
        if !self.config.enabled {
            return vec![];
        }

        if key.is_boundary {
            return self.commit_and_reset(Some(key.ch));
        }

        let ch = key.ch;

        // Word boundary characters — commit current word, then pass the char through.
        if is_word_boundary(ch) {
            let mut actions = self.commit_and_reset(None);
            // The boundary character itself is passed through untouched.
            actions.push(EditAction::Insert(ch.to_string()));
            return actions;
        }

        self.buffer.push(ch);
        let actions = self.recompute();
        if actions.is_empty() {
            // The key was consumed but the screen already shows the result
            // (e.g. a redundant horn key after "ưo" was auto-corrected to
            // "ươ"). An empty list means "pass the key through", which would
            // type the raw key on top of the word — return an explicit no-op
            // so the shell still swallows the keystroke.
            return vec![EditAction::Insert(String::new())];
        }
        actions
    }

    /// Handle a Backspace/Delete keystroke.
    ///
    /// Deletes one **displayed character** — the letter the user can see. It used
    /// to undo the last raw *keystroke* and recompute instead, which made
    /// Backspace strip the tone mark rather than remove a letter: "đoán" became
    /// "đoan" (undoing the `s` of "ddoans") when the user expected "đoá".
    ///
    /// Composing then **continues**. The typed keystrokes no longer describe
    /// what is on screen — deleting the 'n' of "đoán" would need raw "ddoas",
    /// i.e. removing a key from the middle — so the raw buffer is re-derived
    /// from the remaining text instead ("đoá" → "ddoas"). An earlier version
    /// gave up and reset here, which meant keys typed after a delete composed
    /// against nothing: re-completing "rượ" with a 'u' produced "rượu" typed
    /// fresh as "rưoự" or plain "rưou" instead of "rượu".
    ///
    /// An empty vec means "we're not composing — let the native Backspace pass
    /// through untouched."
    pub fn backspace(&mut self) -> Vec<EditAction> {
        if !self.config.enabled || self.buffer.displayed.is_empty() {
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

    /// Recompute the target Vietnamese word from the current raw buffer,
    /// and return the diff actions needed to update the on-screen text.
    fn recompute(&mut self) -> Vec<EditAction> {
        let raw = self.buffer.raw.clone();
        let result = match self.config.method {
            InputMethod::Telex => TelexMethod.process(&raw),
            InputMethod::Vni => VniMethod.process(&raw),
        };

        let method_result = match result {
            Some(r) => r,
            None => return vec![],
        };

        let bare = &method_result.bare;
        let tone = method_result.tone;

        // Can this still become a Vietnamese syllable? Three ways it cannot:
        // the letters don't fit Vietnamese structure, the input method saw an
        // illegal sequence (a vowel after a tone key), or the tone is impossible
        // on this coda (only sắc/nặng ride a stop consonant).
        // The tone/coda rule only applies once a tone exists: mid-word "viêt" is a
        // legitimate intermediate state (the tone key comes last in Telex), and a
        // bare onset like "t" must not be read as a coda.
        let impossible = !is_valid_prefix(bare)
            || method_result.is_foreign
            || (tone != Tone::Flat && !tone_allowed_with_coda(tone, coda_of(bare)));

        // Was this word already literal before this keystroke? If so the raw keys
        // are the only faithful rendering — a cancel inside an already-foreign word
        // ("press") has lost a consumed tone key from `bare`.
        let was_passthrough = self.passthrough;

        if self.config.auto_restore && impossible {
            // Hand back exactly what was typed, and *stay* in passthrough until
            // the next word boundary. Re-composing the rest of the word is what
            // turned "jira" into "jỉa": the 'j' was emitted, state was reset,
            // then "ira" was composed afresh with 'r' eaten as hỏi.
            self.passthrough = true;
        }

        if self.passthrough {
            // diff_to (not clear + insert) keeps `displayed` describing what is
            // actually on screen, so nothing gets eaten by a stale backspace
            // count on the following keystroke.
            let text = match &method_result.literal {
                // A key that undid its own diacritic makes the word literal text,
                // and the method already dropped the undone mark: "ddd" → "dd".
                Some(literal) if !was_passthrough && !method_result.is_foreign => {
                    apply_case_mask(literal, &method_result.case_mask)
                }
                _ => raw.clone(),
            };
            return self.buffer.diff_to(&text);
        }

        // Still a possible Vietnamese syllable, but a key undid its own diacritic,
        // so show that text verbatim rather than re-applying anything: "aaa" → "aa".
        if let Some(literal) = &method_result.literal {
            let literal = apply_case_mask(literal, &method_result.case_mask);
            return self.buffer.diff_to(&literal);
        }

        // Apply tone placement to form the target word.
        let target = apply_tone(bare, tone, self.config.placement);
        // The methods lowercase letters for matching; the mask restores the
        // case the user actually typed, per character, so mid-word capitals
        // survive ("BaN" → "BaN"). Tone placement is 1:1 on characters, so the
        // mask still lines up after it.
        let target = apply_case_mask(&target, &method_result.case_mask);

        self.buffer.diff_to(&target)
    }

    /// Commit the current word without a boundary character. The shell calls
    /// this for keys it passes through itself (Enter, Tab), so macros still
    /// expand there; the returned actions land before the passed-through key.
    pub fn commit_word(&mut self) -> Vec<EditAction> {
        if !self.config.enabled {
            return vec![];
        }
        self.commit_and_reset(None)
    }

    /// Commit the current buffer (expand a macro, then clear) and return diff
    /// actions. `extra_char` is inserted literally after the commit (e.g. a
    /// space).
    fn commit_and_reset(&mut self, extra_char: Option<char>) -> Vec<EditAction> {
        let mut actions = vec![];

        // Text expansion: the committed word — exactly as displayed — matches
        // a macro trigger. u8::MAX guards the Backspace count; triggers are
        // short in practice.
        if let Some(expansion) = self.config.macros.get(&self.buffer.displayed) {
            let shown = self.buffer.displayed.chars().count();
            if shown > 0 && shown <= u8::MAX as usize && *expansion != self.buffer.displayed {
                actions.push(EditAction::Backspace(shown as u8));
                actions.push(EditAction::Insert(expansion.clone()));
            }
        }

        // Otherwise the text already on screen was kept in sync by
        // recompute() — clearing internal state needs no backspaces.
        self.buffer.reset();
        self.passthrough = false;

        if let Some(ch) = extra_char {
            if ch != '\0' {
                actions.push(EditAction::Insert(ch.to_string()));
            }
        }

        actions
    }

    /// Put the raw keystrokes back on screen (Esc): "đấy" → "ddaays".
    ///
    /// The escape hatch for when composition guessed wrong. The word then
    /// stays literal until the next boundary — restoring it just to have the
    /// next key re-compose it would defeat the point.
    ///
    /// An empty vec means there is nothing to restore (not composing, or the
    /// screen already shows the raw keys) — the caller should treat the
    /// keystroke as not ours and pass it through.
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
    }

    /// Returns the string currently displayed on-screen for the active word.
    /// Primarily for testing.
    pub fn current_displayed(&self) -> String {
        self.buffer.displayed.clone()
    }
}

fn is_word_boundary(ch: char) -> bool {
    matches!(ch,
        ' ' | '\t' | '\n' | '\r'
        | '.' | ',' | '!' | '?' | ';' | ':'
        | '(' | ')' | '[' | ']' | '{' | '}'
        | '"' | '\'' | '`'
        | '/' | '\\' | '|' | '-' | '_'
        // Note: digits are NOT boundaries — VNI uses them for tone/diacritic input.
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
        // "vieetj" = viet with ê (ee) and nặng (j) tone → "việt"
        type_str(&mut e, "vieetj");
        assert_eq!(e.buffer.displayed, "việt");
    }

    #[test]
    fn telex_viet_sharp() {
        let mut e = engine();
        // "vieets" = viêt (ê from ee) with sắc → "viết"
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
        type_str(&mut e, "VIEETS"); // VIET with ê + sắc, all caps
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
        let mut e = Engine::new(Config { enabled: false, ..Default::default() });
        let actions = e.process(Keystroke::char('a'));
        assert!(actions.is_empty());
    }

    #[test]
    fn backspace_deletes_a_displayed_character() {
        // Telex "as" folds 2 raw keystrokes into 1 displayed char: "á".
        let mut e = engine();
        type_str(&mut e, "as");
        assert_eq!(e.buffer.displayed, "á");

        // Backspace deletes the character the user sees, and stops composing.
        let actions = e.backspace();
        assert_eq!(actions, vec![EditAction::Backspace(1)]);
        assert!(e.buffer.raw.is_empty());
        assert!(e.buffer.displayed.is_empty());
    }

    #[test]
    fn backspace_removes_a_letter_not_the_tone_mark() {
        // The reported bug: "đoán" must become "đoá", not "đoan".
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
        // Deleting the whole word leaves nothing to continue from, so the next
        // keys compose from scratch.
        let mut e = engine();
        type_str(&mut e, "as");
        e.backspace();
        let actions = type_str(&mut e, "b");
        assert_eq!(actions, vec![EditAction::Insert("b".into())]);
        assert_eq!(e.buffer.raw, "b");
    }

    #[test]
    fn typing_after_backspace_continues_the_composition() {
        // Deleting into the middle of a word re-derives the raw keys from the
        // remaining text, so the next keys keep composing: "đoá" + n → "đoán".
        let mut e = engine();
        type_str(&mut e, "ddoans");
        e.backspace();
        assert_eq!(e.buffer.displayed, "đoá");
        assert_eq!(e.buffer.raw, "ddoas");
        type_str(&mut e, "n");
        assert_eq!(e.buffer.displayed, "đoán");
    }
}
