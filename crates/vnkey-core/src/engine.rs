use crate::buffer::CompositionBuffer;
use crate::methods::{InputMethodProcessor, TelexMethod, VniMethod};
use crate::tone_placement::apply_tone;
use crate::types::{Config, EditAction, InputMethod, Keystroke};
use crate::validator::is_valid_prefix;

pub struct Engine {
    buffer: CompositionBuffer,
    config: Config,
}

impl Engine {
    pub fn new(config: Config) -> Self {
        Self { buffer: CompositionBuffer::new(), config }
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

        // Check whether the current buffer could still form a valid Vietnamese syllable.
        // If definitely not, and auto-restore is on, output raw keystrokes.
        if self.config.auto_restore && !is_valid_prefix(bare) {
            // Auto-restore: emit raw characters and stop buffering.
            let raw_str = raw.clone();
            let mut actions = self.buffer.clear_actions();
            actions.push(EditAction::Insert(raw_str));
            return actions;
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
}
