use crate::buffer::CompositionBuffer;
use crate::methods::{apply_case_mask, InputMethodProcessor, TelexMethod, VniMethod};
use crate::tone_placement::apply_tone;
use crate::types::{Config, EditAction, InputMethod, Keystroke, Tone};
use crate::validator::{base_vowel, coda_of, is_valid_prefix, tone_allowed_with_coda};

pub struct Engine {
    buffer: CompositionBuffer,
    config: Config,
    /// Set once the current word has proved it cannot be Vietnamese. Cleared at
    /// the next word boundary. While set, keystrokes are echoed verbatim.
    passthrough: bool,
    /// Committed words still sitting left of the cursor, oldest first — the
    /// OpenKey/XKey model. A Backspace arriving while nothing is composing is
    /// deleting boundary characters; once it consumes the last one before a
    /// word, that word's snapshot is popped back into the buffer so composing
    /// resumes (a repeated tone key can still reverse its tone). Cleared
    /// whenever the screen may no longer match what was committed: reset
    /// (mouse click, focus change, navigation), a macro expansion, or a
    /// commit that left no boundary character on screen (Enter/Tab).
    history: Vec<CommittedWord>,
    /// A sentence-ending mark has been typed and we are waiting to see whether
    /// whitespace follows. "a.b" and "1.5" are not sentence breaks, so the
    /// capital is only armed once the space actually arrives.
    pending_sentence_end: bool,
    /// The word being composed starts a sentence, so its first character is
    /// shown uppercase. Held for the whole word — later diacritic and tone
    /// keys rewrite that character, and the capital has to survive them.
    capitalize_next: bool,
}

/// Snapshot of a committed word, taken as the boundary key landed.
struct CommittedWord {
    raw: String,
    displayed: String,
    passthrough: bool,
    /// Boundary characters currently between this word and the cursor (or
    /// the next word). Grows as consecutive spaces/punctuation are typed,
    /// shrinks as Backspace deletes them; the word resumes at zero.
    boundaries: usize,
}

/// Committed words remembered for Backspace resume. OpenKey keeps a similar
/// small stack; beyond this depth the oldest word is simply forgotten and
/// Backspace falls back to plain native deletes.
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
        // Snapshots hold method-specific raw keys ("d9oan1" vs "ddoans") —
        // resuming one under the other method would compose garbage.
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

        // Word boundary characters — commit current word, then pass the char through.
        if is_word_boundary(ch) {
            return self.commit_and_reset(Some(ch), true);
        }

        // An ordinary character after a full stop means it was not a sentence
        // break after all ("vnkey.exe", "1.5").
        self.pending_sentence_end = false;

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
        if !self.config.enabled {
            return vec![];
        }

        if self.buffer.displayed.is_empty() {
            // Nothing composing — this Backspace deletes a boundary character
            // (natively; we return no actions). Walk the count down on the
            // most recent committed word; consuming its last boundary
            // character leaves that word at the cursor, so pop its snapshot
            // back into the buffer and composing resumes — a repeated tone
            // key can still reverse the tone.
            if let Some(last) = self.history.last_mut() {
                last.boundaries -= 1;
                if last.boundaries == 0 {
                    let word = self.history.pop().unwrap();
                    self.buffer.raw = word.raw;
                    self.buffer.displayed = word.displayed;
                    self.passthrough = word.passthrough;
                }
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

    /// Recompute the target Vietnamese word from the current raw buffer,
    /// and return the diff actions needed to update the on-screen text.
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

        // Can this still become a Vietnamese syllable? Three ways it cannot:
        // the letters don't fit Vietnamese structure, the input method saw an
        // illegal sequence (a vowel after a tone key), or the tone is impossible
        // on this coda (only sắc/nặng ride a stop consonant).
        // The tone/coda rule only applies once a tone exists: mid-word "viêt" is a
        // legitimate intermediate state (the tone key comes last in Telex), and a
        // bare onset like "t" must not be read as a coda.
        let impossible = !self.syllable_possible(bare)
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
            return self.show(&text);
        }

        // Still a possible Vietnamese syllable, but a key undid its own diacritic,
        // so show that text verbatim rather than re-applying anything: "aaa" → "aa".
        if let Some(literal) = &method_result.literal {
            let literal = apply_case_mask(literal, &method_result.case_mask);
            return self.show(&literal);
        }

        // Apply tone placement to form the target word.
        let target = apply_tone(bare, tone, self.config.placement);
        // The methods lowercase letters for matching; the mask restores the
        // case the user actually typed, per character, so mid-word capitals
        // survive ("BaN" → "BaN"). Tone placement is 1:1 on characters, so the
        // mask still lines up after it.
        let target = apply_case_mask(&target, &method_result.case_mask);

        self.show(&target)
    }

    /// Could `bare` still become a Vietnamese syllable?
    ///
    /// With a consonant shorthand switched on, the word on the way there is
    /// not spellable yet — "hag" only becomes "hang" at the boundary. It still
    /// has to count as composable, or the tone key would find a literal word
    /// and "hagf" would end up "hagf" instead of "hàng". OpenKey loosens its
    /// spell-check tables for exactly this reason.
    fn syllable_possible(&self, bare: &str) -> bool {
        is_valid_prefix(bare)
            || self.expand_quick_consonants(bare).is_some_and(|e| is_valid_prefix(&e))
    }

    /// Expand the quick start/end consonant shortcuts on the finished word,
    /// or `None` when neither applies or the result could not be Vietnamese.
    ///
    /// OpenKey gates these on its spell checker; the same job is done here by
    /// asking whether the expansion is a possible syllable at all — without
    /// it, English "for" would come out as "phor".
    fn quick_consonant_expansion(&self) -> Option<String> {
        let out = self.expand_quick_consonants(&self.buffer.displayed)?;
        (out != self.buffer.displayed && is_valid_prefix(&out)).then_some(out)
    }

    /// Rewrite `word`'s shorthand consonants, with no judgement about whether
    /// the result is a real syllable — both callers decide that for themselves.
    fn expand_quick_consonants(&self, word: &str) -> Option<String> {
        // Both shortcuts spend Telex tone and horn keys; VNI needs its digits
        // for tones and has no equivalent convention. Checked before anything
        // is allocated — `syllable_possible` calls this on every keystroke.
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
                // OpenKey adds the second letter in capitals only when the
                // word was already being typed that way: "Fanh" → "Phanh",
                // "FANH" → "PHANH".
                let first = chars[0].is_uppercase();
                [cased(a, first), cased(b, first && chars[1].is_uppercase())]
            });

        let last = chars[chars.len() - 1];
        let tail = self
            .config
            .quick_end_consonant
            // A vowel must come first, which is what keeps real codas typable:
            // the 'h' of "anh" and the 'g' of "ong" follow consonants.
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

    /// Put `text` on screen, capitalized if it opens a sentence.
    ///
    /// The capital is applied here, to the finished word, rather than to the
    /// first keystroke — diacritic and tone keys rewrite that character
    /// ("toois" → "tối"), and a capital stamped on the raw 't' would be lost.
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

    /// Commit the current word without a boundary character. The shell calls
    /// this for keys it passes through itself (Tab), so macros still expand
    /// there; the returned actions land before the passed-through key.
    pub fn commit_word(&mut self) -> Vec<EditAction> {
        if !self.config.enabled {
            return vec![];
        }
        self.commit_and_reset(None, false)
    }

    /// Commit the current word and start a new sentence — the shell calls this
    /// for Enter, which it passes through itself. Separate from
    /// [`commit_word`](Self::commit_word) because Tab commits a word without
    /// ending a line, so only Enter arms the sentence capital.
    pub fn commit_line(&mut self) -> Vec<EditAction> {
        if !self.config.enabled {
            return vec![];
        }
        let actions = self.commit_and_reset(None, true);
        self.pending_sentence_end = false;
        self.capitalize_next = true;
        actions
    }

    /// Commit the current buffer (expand a macro, then clear) and return diff
    /// actions. `extra_char` is inserted literally after the commit (e.g. a
    /// space).
    fn commit_and_reset(
        &mut self,
        extra_char: Option<char>,
        expand_shortcuts: bool,
    ) -> Vec<EditAction> {
        let mut actions = vec![];

        // Text expansion: the committed word — exactly as displayed — matches
        // a macro trigger. u8::MAX guards the Backspace count; triggers are
        // short in practice.
        let mut expanded = false;
        if let Some(expansion) = self.config.macros.get(&self.buffer.displayed) {
            let shown = self.buffer.displayed.chars().count();
            if shown > 0 && shown <= u8::MAX as usize && *expansion != self.buffer.displayed {
                actions.push(EditAction::Backspace(shown as u8));
                actions.push(EditAction::Insert(expansion.clone()));
                expanded = true;
            }
        }

        // Quick consonant shortcuts rewrite the finished word ("fag" →
        // "phang"). Deferred to the boundary the way OpenKey does it, so f/j/w
        // and g/h/k keep their ordinary Telex meaning for as long as the word
        // is still being typed. A macro already replaced the word outright, so
        // the two never both fire.
        if !expanded && expand_shortcuts {
            if let Some(text) = self.quick_consonant_expansion() {
                actions.extend(self.buffer.diff_to(&text));
                // The typed keys no longer describe the screen, so re-derive
                // them — the resume history below snapshots both.
                self.buffer.raw = match self.config.method {
                    InputMethod::Telex => crate::methods::telex::encode_telex(&text),
                    InputMethod::Vni => crate::methods::vni::encode_vni(&text),
                };
            }
        }

        // Keep the Backspace-resume history in step with the screen (see
        // `history`). A boundary character landing after a word snapshots the
        // word; one landing with nothing composing extends the run counted on
        // the previous word. A macro expansion is arbitrary text between the
        // cursor and every earlier word, and a commit without a char on
        // screen (Enter/Tab pass through the shell and may leave nothing,
        // e.g. form submit) loses track of the screen — both drop the lot.
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

        // Track where sentences begin, for auto-capitalization. `None` is a
        // key the shell passes through itself (Tab): it tells us nothing about
        // sentence structure, so the current state simply carries over.
        match extra_char {
            Some('\n' | '\r') => {
                self.pending_sentence_end = false;
                self.capitalize_next = true;
            }
            Some('.' | '!' | '?') => {
                self.pending_sentence_end = true;
                self.capitalize_next = false;
            }
            // Whitespace confirms a pending sentence end; further spaces keep
            // the capital armed rather than consuming it.
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
        self.history.clear();
        // After a click the cursor may sit mid-sentence; a capital armed
        // before the jump would land on the wrong word.
        self.pending_sentence_end = false;
        self.capitalize_next = false;
    }

    /// Returns the string currently displayed on-screen for the active word.
    /// Primarily for testing.
    pub fn current_displayed(&self) -> String {
        self.buffer.displayed.clone()
    }
}

/// Telex shorthands for a leading consonant cluster (OpenKey's
/// `_quickStartConsonant`). Note `w` is only reachable with `standalone_w`
/// off — otherwise a leading `w` has already become `ư`.
fn start_consonant(ch: char) -> Option<(char, char)> {
    match ch {
        'f' => Some(('p', 'h')),
        'j' => Some(('g', 'i')),
        'w' => Some(('q', 'u')),
        _ => None,
    }
}

/// Telex shorthands for a final consonant cluster (OpenKey's
/// `_quickEndConsonant`).
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

/// Is `ch` a vowel, ignoring case, tone mark and diacritic?
fn is_vowel(ch: char) -> bool {
    let lower = lower(ch);
    let base = base_vowel(lower).unwrap_or(lower);
    matches!(base, 'a' | 'â' | 'ă' | 'e' | 'ê' | 'i' | 'o' | 'ô' | 'ơ' | 'u' | 'ư' | 'y')
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
    fn backspace_after_space_resumes_the_word() {
        // "đoán" + space commits; Backspace deletes the space (natively — empty
        // actions) and the word composes again, so a repeated tone key still
        // reverses the tone instead of typing on top of it.
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
        // "đoán␣␣": the first Backspace deletes a space but the cursor is
        // still one space away from the word — no resume yet. The second
        // Backspace reaches the word and resumes it.
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
        // The expansion is arbitrary text (possibly several words) — it cannot
        // be taken back into the composition buffer.
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
        // Enter/Tab pass through the shell and may not leave a character on
        // screen at all (form submit) — resuming would desync.
        let mut e = engine();
        type_str(&mut e, "as");
        e.commit_word();
        assert!(e.backspace().is_empty());
        assert!(e.buffer.displayed.is_empty());
    }

    #[test]
    fn deleting_a_new_word_walks_back_into_the_previous_one() {
        // "á␣b" then Backspace ×2: the first deletes 'b', the second deletes
        // the space and resumes "á" — history survives typing a new word.
        let mut e = engine();
        type_str(&mut e, "as");
        e.process(Keystroke::char(' '));
        type_str(&mut e, "b");
        e.backspace(); // deletes the 'b'
        assert!(e.backspace().is_empty()); // deletes the space...
        assert_eq!(e.buffer.displayed, "á"); // ...and resumes the word
    }

    #[test]
    fn deleting_a_restored_word_resumes_the_one_before_it() {
        // "há␣cá␣": Backspace resumes "cá"; deleting it letter by letter and
        // backspacing over the next space resumes "há" — a history stack,
        // not a single remembered word.
        let mut e = engine();
        type_str(&mut e, "has");
        e.process(Keystroke::char(' '));
        type_str(&mut e, "cas");
        e.process(Keystroke::char(' '));

        assert!(e.backspace().is_empty()); // space deleted, "cá" resumes
        assert_eq!(e.buffer.displayed, "cá");
        e.backspace(); // 'á'
        e.backspace(); // 'c'
        assert!(e.buffer.displayed.is_empty());

        assert!(e.backspace().is_empty()); // space deleted, "há" resumes
        assert_eq!(e.buffer.displayed, "há");
        type_str(&mut e, "s");
        assert_eq!(e.buffer.displayed, "has"); // repeated tone key reverses
    }

    #[test]
    fn resume_restores_passthrough_of_an_escaped_word() {
        // Esc made the word literal ("đấy" → "ddaays"); committing and
        // resuming it must not silently re-enter composing, or the next key
        // would rewrite the literal text the user explicitly asked for.
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
        // The expansion is arbitrary text sitting between the cursor and any
        // earlier word — walking back into "á" over it would desync.
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
        // Mouse click / focus change: the screen may no longer match what
        // was committed — resuming would desync (XKey's cursorMoved guard).
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
        // "fa" stays literal while typing — the expansion lands only once the
        // word is finished, as in OpenKey.
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
        // The 'h' of "anh" follows a consonant, so it is a real coda, not a
        // shorthand — this guard is what keeps "anh"/"ach"/"ong" typable.
        let mut e = quick_engine(false, true);
        type_str(&mut e, "anh");
        let actions = e.process(Keystroke::char(' '));
        assert_eq!(actions, vec![EditAction::Insert(" ".into())]);
    }

    #[test]
    fn the_expansion_keeps_a_tone_already_placed() {
        // "hags" shows "hág" while typing; the expansion must not disturb the
        // tone mark already sitting on the nucleus.
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
        // English "for" would expand to "phor", which no Vietnamese syllable
        // can be — the word is handed back untouched instead.
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
        // OpenKey expands on space and punctuation, never on Tab — Tab often
        // moves focus, and a rewrite would chase the cursor into the next field.
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
        // The resume history has to remember what actually landed on screen,
        // not the keys that were typed to get there.
        let mut e = quick_engine(true, true);
        type_str(&mut e, "fag");
        e.process(Keystroke::char(' '));
        assert!(e.backspace().is_empty());
        assert_eq!(e.buffer.displayed, "phang");
    }

    fn capitalizing_engine() -> Engine {
        Engine::new(Config { auto_capitalize: true, ..Default::default() })
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
        // The capital is applied to the finished word, so keys that rewrite
        // the first character must not lose it: "toois" → "Tối", not "tối".
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
        // "vnkey.exe", "1.5", "a.b" — a period glued to the next letter is not
        // a sentence break.
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
        // After a click the cursor may be anywhere — mid-sentence, even — so
        // the pending capital must not carry across.
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
