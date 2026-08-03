use crate::buffer::CompositionBuffer;
use crate::methods::{InputMethodProcessor, TelexMethod, VniMethod};
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
        self.recompute()
    }

    /// Handle a Backspace/Delete keystroke.
    ///
    /// Vietnamese diacritics can fold several raw keystrokes into one displayed
    /// character (e.g. telex "as" → "á"), so a single Backspace must undo the
    /// last *raw* keystroke and recompute the word, not just drop one on-screen
    /// character. Returns actions to reconcile the display; an empty vec means
    /// "we're not composing — let the native Backspace pass through untouched."
    pub fn backspace(&mut self) -> Vec<EditAction> {
        if !self.config.enabled || self.buffer.raw.is_empty() {
            return vec![];
        }

        self.buffer.pop();
        if self.buffer.raw.is_empty() {
            // Nothing left to compose; clear the (now stale) displayed word.
            return self.buffer.clear_actions();
        }
        self.recompute()
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
            let raw_str = raw.clone();
            return self.buffer.diff_to(&raw_str);
        }

        // Apply tone placement to form the target word.
        let target = apply_tone(bare, tone, self.config.placement);
        // The methods lowercase everything for processing; restore the case the
        // user actually typed (Shift). Handles the common patterns: a leading
        // capital ("Xin" → "Xin") and all-caps ("VIET" → "VIẾT").
        let target = apply_case(&target, &raw);

        self.buffer.diff_to(&target)
    }

    /// Commit the current buffer (validate, then clear) and return diff actions.
    /// `extra_char` is inserted literally after the commit (e.g. a space).
    fn commit_and_reset(&mut self, extra_char: Option<char>) -> Vec<EditAction> {
        // On boundary, we just need to clear internal state; the text already on
        // screen was kept in sync by recompute(). No extra backspaces needed.
        self.buffer.reset();
        self.passthrough = false;

        let mut actions = vec![];

        if let Some(ch) = extra_char {
            if ch != '\0' {
                actions.push(EditAction::Insert(ch.to_string()));
            }
        }

        actions
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

/// Re-apply the case the user typed to a converted (lowercase) syllable.
///
/// - All typed letters uppercase → uppercase the whole result (`VIET` → `VIẾT`).
/// - Otherwise, if the first typed letter was uppercase → capitalize the first
///   letter of the result (`Xin` → `Xin`, `Has` → `Há`).
/// - Otherwise leave it lowercase.
fn apply_case(target: &str, raw: &str) -> String {
    let letters: Vec<char> = raw.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return target.to_string();
    }

    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return target.to_uppercase();
    }

    if letters[0].is_uppercase() {
        let mut chars = target.chars();
        if let Some(first) = chars.next() {
            return first.to_uppercase().collect::<String>() + chars.as_str();
        }
    }

    target.to_string()
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
    fn backspace_undoes_last_raw_keystroke_not_last_char() {
        // Telex "as" folds 2 raw keystrokes into 1 displayed char: "á".
        let mut e = engine();
        type_str(&mut e, "as");
        assert_eq!(e.buffer.displayed, "á");

        // Backspace must undo the "s" (raw), leaving "a" composing — not just
        // blindly delete the on-screen character and stop.
        let actions = e.backspace();
        assert_eq!(e.buffer.raw, "a");
        assert_eq!(e.buffer.displayed, "a");
        assert_eq!(actions, vec![EditAction::Backspace(1), EditAction::Insert("a".into())]);
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
    fn backspace_then_retype_reproduces_tone() {
        // Regression: after backspacing off a tone key, re-typing it should
        // reconstruct the same word rather than leaving stale state behind.
        let mut e = engine();
        type_str(&mut e, "as");
        e.backspace();
        type_str(&mut e, "s");
        assert_eq!(e.buffer.displayed, "á");
    }
}
