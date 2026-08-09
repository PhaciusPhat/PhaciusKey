use crate::types::{Config, Tone};
use crate::validator::is_valid_prefix;
use super::{InputMethodProcessor, MethodResult};

pub struct TelexMethod;

impl InputMethodProcessor for TelexMethod {
    fn process(&self, raw: &str, config: &Config) -> Option<MethodResult> {
        if raw.is_empty() {
            return None;
        }
        Some(process_telex(raw, config))
    }
}

/// Process a raw Telex keystroke sequence into a bare syllable + tone.
///
/// Telex rules:
///   Vowel diacritics (by doubling or special key):
///     aa → â,  aw → ă,  ee → ê,  oo → ô,  ow → ơ,  uw → ư,  dd → đ
///   Tone suffix (last unambiguous tone key wins):
///     s → sắc, f → huyền, r → hỏi, x → ngã, j → nặng, z → ngang (remove)
///   Undo: pressing a diacritic or tone key again undoes it and types the key
///     literally — "aaa" → "aa", "ass" → "as".
pub fn process_telex(raw: &str, config: &Config) -> MethodResult {
    // Work character by character, maintaining a mutable state.
    let mut state = TelexState { config: config.clone(), ..Default::default() };
    for ch in raw.chars() {
        state.push(ch);
    }
    state.finish()
}

#[derive(Default)]
struct TelexState {
    /// Which optional typing conventions are switched on.
    config: Config,
    /// Accumulated bare consonants + vowels (no tone mark).
    syllable: String,
    /// Current tone.
    tone: Tone,
    /// Whether this is unambiguously a foreign (non-Vietnamese) word.
    is_foreign: bool,
    /// Whether a real (non-Flat) tone key has been consumed.
    tone_applied: bool,
    /// Whether a key undid its own diacritic/tone, making this literal text.
    cancelled: bool,
    /// Raw character buffer (for triple-press detection).
    raw: String,
    /// Uppercase flag per `syllable` character. Diacritic replacements keep
    /// the replaced character's flag (the doubling key only enriches what is
    /// already on screen), so the mask always stays in step with `syllable`.
    mask: Vec<bool>,
    /// Which `syllable` position, if any, holds an `ư` the standalone rule
    /// produced (OpenKey's `STANDALONE_MASK`). The two ways to get an `ư` undo
    /// differently — a typed "uw" goes back to "uw", a lone 'w' goes back to
    /// 'w' — and the character on screen cannot tell them apart.
    standalone_w: Option<usize>,
}

impl TelexState {
    fn push(&mut self, ch: char) {
        self.push_key(ch);
        // The marker names a position, and nearly every rule in `push_key` may
        // rewrite the syllable underneath it — a later horn key turning "ưo"
        // into "ươ", say. Re-reading the position once, here, is cheaper than
        // threading invalidation through each of those paths and cannot miss
        // one of them.
        if let Some(pos) = self.standalone_w {
            if self.syllable.chars().nth(pos) != Some('ư') {
                self.standalone_w = None;
            }
        }
    }

    fn push_key(&mut self, ch: char) {
        self.raw.push(ch);
        let lower = ch.to_lowercase().next().unwrap_or(ch);

        // --- Quick Telex ---
        // A doubled consonant stands for the digraph it opens: "cc" is "ch",
        // "nn" is "ng". First of all the rules because none of c/g/k/n/q/p/t is
        // a tone key or half of a diacritic pair, so nothing below has a claim
        // on these keystrokes — and because the rule reads the character the
        // key lands on, which the paths below are free to have rewritten.
        if self.config.quick_telex {
            if let Some(completion) = quick_telex_completion(&self.syllable, lower) {
                self.syllable.push(completion);
                self.mask.push(ch.is_uppercase());
                return;
            }
        }

        // --- Tone key? ---
        if let Some(tone) = tone_key(lower) {
            // A tone key needs a vowel to carry the mark, and 'z' (remove tone)
            // additionally needs a tone to remove — with nothing to undo it is
            // the letter z. It used to be consumed unconditionally, so a
            // leading 'z' vanished ("zoo" showed "ô") and "size" lost its z.
            let acts = if tone == Tone::Flat {
                self.tone != Tone::Flat
            } else {
                has_vowel(&self.syllable)
            };
            if acts {
                if self.tone == tone && tone != Tone::Flat {
                    // Same tone key twice cancels it and types the letter, so
                    // "ass" is "as" rather than "á" with the second 's' eaten.
                    self.tone = Tone::Flat;
                    self.tone_applied = false;
                    self.cancelled = true;
                    self.syllable.push(lower);
                    self.mask.push(ch.is_uppercase());
                } else {
                    self.tone = tone;
                    self.tone_applied = tone != Tone::Flat;
                }
                return;
            }
            // Otherwise fall through and treat as a literal character.
        }

        // A vowel arriving after a tone key is *usually* not Vietnamese: in
        // Telex the tone comes after the rime. This is what marks "reset",
        // "user" and "server" as foreign instead of composing "rết", "ủe",
        // "sẻver". But the closing vowels i/u/y/o legitimately extend the rime
        // after an early tone key — "loaji" → "loại", "ddoori" → "đổi",
        // "sasu" → "sáu" — so they keep composing and the validity check in the
        // engine decides. Only a/e stay a foreign marker, which is what keeps
        // "user" and "reset" as themselves.
        if self.tone_applied && is_vowel(lower) && !matches!(lower, 'i' | 'u' | 'y' | 'o') {
            self.is_foreign = true;
        }

        // --- "uo"/"ua" + w takes the cluster horn ---
        // Checked before the single-vowel pairs below, which would otherwise put
        // a horn on the 'o' alone ("thuơng" instead of "thương") or a breve on
        // the trailing 'a' ("truă" instead of "trưa").
        if lower == 'w' && (self.syllable.ends_with("uo") || self.syllable.ends_with("ua")) {
            if let Some(new_syl) = apply_horn_cluster(&self.syllable) {
                self.syllable = new_syl;
                return;
            }
        }

        // --- A second 'w' takes back a standalone one ---
        // Must come before the pairs below, where ('ư', 'w') restores "uw" —
        // right for an 'ư' the user built out of "uw", wrong for one the 'w'
        // key produced on its own, which owes the user back a single 'w'.
        if lower == 'w'
            && self.standalone_w.is_some_and(|pos| pos + 1 == self.syllable.chars().count())
        {
            self.syllable.pop();
            self.syllable.push('w');
            // One character became one character, so the mask keeps its length.
            // The 'w' now on screen is the key just pressed, and takes that
            // key's case rather than the case of the 'ư' it replaced.
            if let Some(flag) = self.mask.last_mut() {
                *flag = ch.is_uppercase();
            }
            self.cancelled = true;
            return;
        }

        // --- Diacritic pair? ---
        if let Some(replacement) = diacritic_pair(&self.syllable, lower) {
            match replacement {
                PairResult::Replace(new_syl) => {
                    self.syllable = new_syl;
                    return;
                }
                PairResult::Restore(new_syl) => {
                    // One character became two ("â" → "aa"): the restored base
                    // keeps its flag, the newly typed key adds its own.
                    self.syllable = new_syl;
                    self.mask.push(ch.is_uppercase());
                    self.cancelled = true;
                    return;
                }
            }
        }

        // --- 'w' on a vowel cluster ---
        // Telex writes ư/ơ with 'w', and the "uo" cluster takes both horns at
        // once ("thuowng" → "thương", "nguoiw" → "ngươi"). Without this, 'w'
        // only paired with the vowel directly before it.
        if lower == 'w' {
            if let Some(new_syl) = apply_horn_cluster(&self.syllable) {
                self.syllable = new_syl;
                return;
            }
        }

        // --- Standalone 'w' → 'ư' ---
        // A 'w' with no vowel to horn is the Unikey/OpenKey shorthand for 'ư'
        // itself, so "w" is "ư" and "thw" is "thư". Only reachable once the
        // horn logic above has declined the key.
        if lower == 'w' && self.config.standalone_w && standalone_w_applies(&self.syllable) {
            self.standalone_w = Some(self.syllable.chars().count());
            self.syllable.push('ư');
            self.mask.push(ch.is_uppercase());
            return;
        }

        // --- Doubling vowel after the coda ---
        // "viet" + 'e' is the user finishing the word before enriching it:
        // the 'e' reaches back across the coda to the matching vowel, giving
        // "viêt" ("vietej" → "việt"). Only fires when typing the key literally
        // could no longer be Vietnamese and the enriched form still can, and
        // only into a multi-vowel nucleus — the last guard is what keeps
        // English "data"/"photo" (one vowel before the coda) restored verbatim
        // instead of composing "dât"/"phôt".
        if !self.is_foreign
            && matches!(lower, 'a' | 'e' | 'o')
            && self.syllable.chars().filter(|&c| is_vowel(c)).count() >= 2
        {
            if let Some(new_syl) = double_distant_vowel(&self.syllable, lower) {
                let plain: String = format!("{}{}", self.syllable, lower);
                if !is_valid_prefix(&plain) && is_valid_prefix(&new_syl) {
                    self.syllable = new_syl;
                    return;
                }
            }
        }

        // --- Regular character ---
        self.syllable.push(lower);
        self.mask.push(ch.is_uppercase());
    }

    fn finish(self) -> MethodResult {
        let literal = self.cancelled.then(|| self.syllable.clone());
        MethodResult {
            // "ưo" never occurs in Vietnamese — the horn always spans the pair.
            // Normalizing here is what lets "ruwou" (horn typed before the 'o')
            // come out as "rươu" instead of going foreign.
            bare: self.syllable.replace("ưo", "ươ"),
            tone: self.tone,
            is_foreign: self.is_foreign,
            literal,
            case_mask: self.mask,
        }
    }
}

/// Reach back across the coda for the vowel `ch` doubles: "viet" + 'e' → "viêt".
/// Returns `None` when the syllable has no plain occurrence of `ch` to enrich.
fn double_distant_vowel(syllable: &str, ch: char) -> Option<String> {
    let enriched = match ch {
        'a' => 'â',
        'e' => 'ê',
        'o' => 'ô',
        _ => return None,
    };
    let chars: Vec<char> = syllable.chars().collect();
    let idx = chars.iter().rposition(|&c| c == ch)?;
    let mut out: String = chars[..idx].iter().collect();
    out.push(enriched);
    out.extend(chars[idx + 1..].iter());
    Some(out)
}

/// Turn displayed text back into the canonical Telex keys that produce it:
/// "rượ" → "ruwowj". Used by Backspace so composing can continue after a
/// character is deleted — the typed keys no longer describe the screen, but a
/// re-derived sequence does.
pub fn encode_telex(text: &str) -> String {
    use crate::tone_placement::char_tone;
    use crate::validator::base_vowel;

    let mut out = String::new();
    let mut tone = Tone::Flat;
    for ch in text.chars() {
        let upper = ch.is_uppercase();
        let lower = ch.to_lowercase().next().unwrap_or(ch);
        let t = char_tone(lower);
        if t != Tone::Flat {
            tone = t; // a syllable carries at most one tone; last one wins
        }
        let base = base_vowel(lower).unwrap_or(lower);
        let keys: &str = match base {
            'â' => "aa",
            'ă' => "aw",
            'ê' => "ee",
            'ô' => "oo",
            'ơ' => "ow",
            'ư' => "uw",
            'đ' => "dd",
            _ => {
                out.push(if upper { base.to_uppercase().next().unwrap_or(base) } else { base });
                continue;
            }
        };
        if upper {
            out.push_str(&keys.to_uppercase());
        } else {
            out.push_str(keys);
        }
    }

    if let Some(key) = tone_char(tone) {
        // Match the case convention `apply_case` reads back: an all-caps word
        // needs an all-caps raw ("VIỆT" → "VIEETJ"), otherwise lowercase.
        let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
        let all_caps = letters.len() > 1 && letters.iter().all(|c| c.is_uppercase());
        out.push(if all_caps { key.to_ascii_uppercase() } else { key });
    }
    out
}

fn tone_char(tone: Tone) -> Option<char> {
    match tone {
        Tone::Sharp => Some('s'),
        Tone::Grave => Some('f'),
        Tone::Hook => Some('r'),
        Tone::Tilde => Some('x'),
        Tone::Dot => Some('j'),
        Tone::Flat => None,
    }
}

// ── Tone keys ────────────────────────────────────────────────────────────────

fn tone_key(ch: char) -> Option<Tone> {
    match ch {
        's' => Some(Tone::Sharp),
        'f' => Some(Tone::Grave),
        'r' => Some(Tone::Hook),
        'x' => Some(Tone::Tilde),
        'j' => Some(Tone::Dot),
        'z' => Some(Tone::Flat),
        _ => None,
    }
}

// ── Diacritic pair detection ─────────────────────────────────────────────────

enum PairResult {
    Replace(String),
    /// The key undid its own diacritic, so the base letter is back *and* the key
    /// is typed literally: "aa" gives â, a third 'a' gives "aa".
    Restore(String),
}

/// Check whether the new character `ch` forms a diacritic pair with the end
/// of `current_syllable`. Returns the new syllable on match, or None.
fn diacritic_pair(syllable: &str, ch: char) -> Option<PairResult> {
    let last = syllable.chars().last()?;
    let prefix = &syllable[..syllable.len() - last.len_utf8()];

    match (last, ch) {
        // aa → â  (if already ends with â → restore to a)
        ('a', 'a') => Some(PairResult::Replace(format!("{prefix}â"))),
        ('â', 'a') => Some(PairResult::Restore(format!("{prefix}aa"))),

        // aw → ă
        ('a', 'w') => Some(PairResult::Replace(format!("{prefix}ă"))),
        ('ă', 'w') => Some(PairResult::Restore(format!("{prefix}aw"))),

        // ee → ê
        ('e', 'e') => Some(PairResult::Replace(format!("{prefix}ê"))),
        ('ê', 'e') => Some(PairResult::Restore(format!("{prefix}ee"))),

        // oo → ô
        ('o', 'o') => Some(PairResult::Replace(format!("{prefix}ô"))),
        ('ô', 'o') => Some(PairResult::Restore(format!("{prefix}oo"))),

        // ow → ơ
        ('o', 'w') => Some(PairResult::Replace(format!("{prefix}ơ"))),
        ('ơ', 'w') => Some(PairResult::Restore(format!("{prefix}ow"))),

        // uw → ư
        ('u', 'w') => Some(PairResult::Replace(format!("{prefix}ư"))),
        ('ư', 'w') => Some(PairResult::Restore(format!("{prefix}uw"))),

        // dd → đ
        ('d', 'd') => Some(PairResult::Replace(format!("{prefix}đ"))),
        ('đ', 'd') => Some(PairResult::Restore(format!("{prefix}dd"))),

        _ => None,
    }
}

// ── Quick Telex ──────────────────────────────────────────────────────────────

/// The letter that finishes the digraph a doubled consonant stands for:
/// "c" + 'c' is "ch", so the answer is 'h'. `None` when `ch` is not doubling
/// one of OpenKey's seven quick-Telex consonants.
///
/// Every row of OpenKey's table repeats the doubled letter as the first letter
/// of its digraph, so the expansion only ever *appends*. That is what keeps the
/// letter already on screen — and its entry in the case mask — exactly as the
/// user typed it: "Cc" gives "Ch", never "ch". The appended letter takes the
/// case of the keypress that asked for it.
fn quick_telex_completion(syllable: &str, ch: char) -> Option<char> {
    // `syllable` is stored lowercased, and `ch` arrives lowercased, so this
    // comparison is the case-insensitive match OpenKey does.
    if syllable.chars().last()? != ch {
        return None;
    }
    match ch {
        'c' => Some('h'),
        'g' => Some('i'),
        'k' => Some('h'),
        'n' => Some('g'),
        'q' => Some('u'),
        'p' => Some('h'),
        't' => Some('h'),
        // OpenKey's table has a "uu" → "ươ" row, but `IS_QUICK_TELEX_KEY` never
        // lets a 'u' reach it — the row is dead code, so it is not copied here.
        _ => None,
    }
}

// ── Standalone 'w' ───────────────────────────────────────────────────────────

/// The letter under any mark: 'ê' → 'e'. The syllable carries no tone (that
/// lives in `TelexState::tone`), so only the seven mark-bearing letters need
/// folding — which is what OpenKey's `CHR()` masking amounts to here.
fn base_letter(c: char) -> char {
    match c {
        'â' | 'ă' => 'a',
        'ê' => 'e',
        'ô' | 'ơ' => 'o',
        'ư' => 'u',
        'đ' => 'd',
        other => other,
    }
}

/// Whether a `w` that found no vowel to horn should type `ư` (OpenKey's
/// `checkForStandaloneChar`). `syllable` is everything composed so far.
fn standalone_w_applies(syllable: &str) -> bool {
    let so_far: Vec<char> = syllable.chars().map(base_letter).collect();
    match so_far.len() {
        // Opening the syllable: 'ư' is the only thing a lone 'w' can mean.
        0 => true,
        // One onset letter in front — but 'e'/'y' are vowels that never sit
        // before 'ư', and 'k' has no "kư" (that word is spelt "cư"). The
        // remaining four are the Telex tone keys, which reached the syllable
        // as literal letters, so "fw"/"jw"/"zw" are somebody typing English.
        1 => !matches!(so_far[0], 'w' | 'e' | 'y' | 'f' | 'j' | 'k' | 'z'),
        // Two letters in, only a real Vietnamese digraph onset can still be
        // waiting for its nucleus ("thư", "ngư"); "plw"/"stw" are English.
        2 => {
            let onset: String = so_far.iter().collect();
            matches!(
                onset.as_str(),
                "tr" | "th" | "ch" | "nh" | "ng" | "kh" | "gi" | "ph" | "gh"
            )
        }
        // Three letters is already a full onset plus something, and "ngh" —
        // the only three-letter onset — has no "nghư". OpenKey stops here too.
        _ => false,
    }
}

fn has_vowel(s: &str) -> bool {
    s.chars().any(is_vowel)
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a'|'â'|'ă'|'e'|'ê'|'i'|'o'|'ô'|'ơ'|'u'|'ư'|'y')
}

/// Apply the Telex horn key to the trailing vowel cluster.
///
/// "uo" takes both horns together ("thuo" + w → "thươ"); otherwise the last
/// eligible vowel takes one (u → ư, o → ơ, a → ă). Returns `None` when there is
/// nothing for 'w' to modify, in which case it is a literal letter.
fn apply_horn_cluster(syllable: &str) -> Option<String> {
    // Longest first: the "uo" pair beats the single vowels.
    if let Some(pos) = syllable.rfind("uo") {
        let mut out = syllable[..pos].to_string();
        out.push('ư');
        out.push('ơ');
        out.push_str(&syllable[pos + 2..]);
        return Some(out);
    }

    // In the "ua" cluster the horn belongs to the u ("truaw" → "trưa", like
    // "mưa"/"chưa") — the plain reverse scan below would breve the 'a' into
    // "truă". After a "qu" onset the u is not part of the nucleus, so 'w'
    // falls through to the scan and marks the 'a' ("quawng" → "quăng").
    if let Some(pos) = syllable.rfind("ua") {
        let after_q = syllable[..pos].ends_with('q');
        if !after_q {
            let mut out = syllable[..pos].to_string();
            out.push('ư');
            out.push_str(&syllable[pos + 1..]);
            return Some(out);
        }
    }

    let chars: Vec<char> = syllable.chars().collect();
    for i in (0..chars.len()).rev() {
        let replacement = match chars[i] {
            'u' => Some('ư'),
            'o' => Some('ơ'),
            'a' => Some('ă'),
            _ => None,
        };
        if let Some(r) = replacement {
            let mut out: String = chars[..i].iter().collect();
            out.push(r);
            out.extend(chars[i + 1..].iter());
            return Some(out);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Tone;

    fn telex(s: &str) -> (String, Tone) {
        let r = process_telex(s, &Config::default());
        (r.bare, r.tone)
    }

    fn telex_with(config: &Config, s: &str) -> String {
        process_telex(s, config).bare
    }

    #[test]
    fn standalone_w_with_nothing_typed_yet() {
        assert_eq!(telex("w").0, "ư");
    }

    #[test]
    fn standalone_w_after_one_character() {
        // A single onset letter takes the 'ư'...
        assert_eq!(telex("tw").0, "tư");
        assert_eq!(telex("nw").0, "nư");
        assert_eq!(telex("mw").0, "mư");
        // ...unless it is one of OpenKey's `_standaloneWbad` letters, none of
        // which opens a syllable in front of 'ư'.
        assert_eq!(telex("kw").0, "kw");
        assert_eq!(telex("zw").0, "zw");
        assert_eq!(telex("fw").0, "fw");
        assert_eq!(telex("jw").0, "jw");
        assert_eq!(telex("ew").0, "ew");
        assert_eq!(telex("yw").0, "yw");
    }

    #[test]
    fn standalone_w_after_a_two_letter_onset() {
        // Only the nine digraphs that really are Vietnamese onsets take it.
        for (seq, want) in [
            ("thw", "thư"), ("trw", "trư"), ("chw", "chư"), ("nhw", "như"),
            ("ngw", "ngư"), ("khw", "khư"), ("giw", "giư"), ("phw", "phư"),
            ("ghw", "ghư"),
        ] {
            assert_eq!(telex(seq).0, want, "telex {seq:?}");
        }
        // Anything else is two letters of a foreign word.
        assert_eq!(telex("plw").0, "plw");
        assert_eq!(telex("stw").0, "stw");
        // Three letters in: OpenKey has no allow-list at all, so "nghư" — a
        // syllable that does not exist — is never composed.
        assert_eq!(telex("nghw").0, "nghw");
    }

    #[test]
    fn standalone_w_ignores_marks_already_applied() {
        // "eew" is 'ê' + 'w'. OpenKey compares the base letter, so the 'ê'
        // still counts as the 'e' of the bad list and the word stays "êw" —
        // reading it as an unlisted letter would compose the impossible "êư".
        assert_eq!(telex("eew").0, "êw");
    }

    #[test]
    fn a_second_w_undoes_a_standalone_one() {
        // The 'ư' came from the 'w' key alone, so undoing it leaves the key
        // itself — not the "uw" the user never typed.
        let r = process_telex("ww", &Config::default());
        assert_eq!(r.bare, "w");
        assert_eq!(r.literal.as_deref(), Some("w"));
        assert_eq!(r.case_mask.len(), 1);
    }

    #[test]
    fn a_second_w_still_restores_a_typed_uw() {
        // Same 'ư' on screen, different origin: here the user really did type
        // "uw", so the undo has to give both letters back.
        let r = process_telex("uww", &Config::default());
        assert_eq!(r.bare, "uw");
        assert_eq!(r.literal.as_deref(), Some("uw"));
    }

    #[test]
    fn standalone_w_can_be_switched_off() {
        let plain = Config { standalone_w: false, ..Default::default() };
        assert_eq!(telex_with(&plain, "w"), "w");
        assert_eq!(telex_with(&plain, "thw"), "thw");
        // The horn keys themselves are untouched by the switch.
        assert_eq!(telex_with(&plain, "thuowng"), "thương");
    }

    #[test]
    fn basic_tones() {
        assert_eq!(telex("has"), ("ha".into(), Tone::Sharp));
        assert_eq!(telex("haf"), ("ha".into(), Tone::Grave));
        assert_eq!(telex("har"), ("ha".into(), Tone::Hook));
        assert_eq!(telex("hax"), ("ha".into(), Tone::Tilde));
        assert_eq!(telex("haj"), ("ha".into(), Tone::Dot));
        // z removes an existing tone; with no tone it is the letter z.
        assert_eq!(telex("hasz"), ("ha".into(), Tone::Flat));
        assert_eq!(telex("haz"), ("haz".into(), Tone::Flat));
    }

    #[test]
    fn vowel_diacritics() {
        assert_eq!(telex("aa").0, "â");
        assert_eq!(telex("aw").0, "ă");
        assert_eq!(telex("ee").0, "ê");
        assert_eq!(telex("oo").0, "ô");
        assert_eq!(telex("ow").0, "ơ");
        assert_eq!(telex("uw").0, "ư");
        assert_eq!(telex("dd").0, "đ");
    }

    #[test]
    fn combined_diacritic_and_tone() {
        let (bare, tone) = telex("haas");
        assert_eq!(bare, "hâ");
        assert_eq!(tone, Tone::Sharp);

        let (bare, tone) = telex("haws");
        assert_eq!(bare, "hă");
        assert_eq!(tone, Tone::Sharp);
    }

    #[test]
    fn restore_on_triple() {
        // "aaa" → 'aa' gives â, the third 'a' undoes it and types itself: "aa".
        assert_eq!(telex("aaa").0, "aa");
        assert_eq!(telex("eee").0, "ee");
        assert_eq!(telex("ddd").0, "dd");
        assert_eq!(telex("oww").0, "ow");
    }

    #[test]
    fn horn_key_after_ua_horns_the_u() {
        // The beta-reported bug: "truaw" gave "truă". In the "ua" cluster the
        // horn belongs to the u ("trưa", "mưa", "chưa"), like the "uo" pair.
        assert_eq!(telex("truaw").0, "trưa");
        assert_eq!(telex("muaw").0, "mưa");
        assert_eq!(telex("chuaw").0, "chưa");
    }

    #[test]
    fn horn_key_after_qu_onset_still_gives_breve() {
        // After the "qu" onset the 'u' is not part of the nucleus, so 'w'
        // falls to the 'a': "quawng" → "quăng", never "qưang".
        assert_eq!(telex("quawng").0, "quăng");
    }

    #[test]
    fn quick_telex_expands_a_doubled_consonant() {
        let quick = Config { quick_telex: true, ..Default::default() };
        for (seq, want) in [
            ("cc", "ch"), ("gg", "gi"), ("kk", "kh"), ("nn", "ng"),
            ("qq", "qu"), ("pp", "ph"), ("tt", "th"),
        ] {
            assert_eq!(telex_with(&quick, seq), want, "telex {seq:?}");
        }
    }

    #[test]
    fn quick_telex_is_not_limited_to_the_word_start() {
        let quick = Config { quick_telex: true, ..Default::default() };
        // OpenKey looks only at the character in front of the key, wherever
        // in the word it sits.
        assert_eq!(telex_with(&quick, "acc"), "ach");
        // There is no undo path: the third 'c' finds an 'h' in front of it and
        // is simply typed.
        assert_eq!(telex_with(&quick, "ccc"), "chc");
        // And the same literal reading gives "ngg" → "ngi", because the key
        // doubles the 'g' it landed on. OpenKey behaves this way too.
        assert_eq!(telex_with(&quick, "ngg"), "ngi");
    }

    #[test]
    fn quick_telex_is_off_by_default() {
        // It costs the ability to type a doubled consonant at all, so it is
        // opt-in — "cc" has to stay "cc".
        assert_eq!(telex("cc").0, "cc");
        assert_eq!(telex("tt").0, "tt");
        assert_eq!(telex("ngg").0, "ngg");
    }

    #[test]
    fn viet() {
        // "viet" in Telex — 'i' is a vowel, 't' is a coda — no diacritic keys
        let (bare, tone) = telex("viet");
        assert_eq!(bare, "viet");
        assert_eq!(tone, Tone::Flat);
    }

    #[test]
    fn vief_t() {
        // "viets" → sắc on nucleus
        let (bare, tone) = telex("viets");
        assert_eq!(bare, "viet");
        assert_eq!(tone, Tone::Sharp);
    }
}
