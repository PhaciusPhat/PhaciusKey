use crate::types::Tone;
use super::{InputMethodProcessor, MethodResult};

pub struct TelexMethod;

impl InputMethodProcessor for TelexMethod {
    fn process(&self, raw: &str) -> Option<MethodResult> {
        if raw.is_empty() {
            return None;
        }
        Some(process_telex(raw))
    }
}

/// Process a raw Telex keystroke sequence into a bare syllable + tone.
///
/// Telex rules:
///   Vowel diacritics (by doubling or special key):
///     aa → â,  aw → ă,  ee → ê,  oo → ô,  ow → ơ,  uw → ư,  dd → đ
///   Tone suffix (last unambiguous tone key wins):
///     s → sắc, f → huyền, r → hỏi, x → ngã, j → nặng, z → ngang (remove)
///   Restore: typing the base letter a third time restores the base letter
///     (e.g. "aaa" → "a").
pub fn process_telex(raw: &str) -> MethodResult {
    // Work character by character, maintaining a mutable state.
    let mut state = TelexState::default();
    for ch in raw.chars() {
        state.push(ch);
    }
    state.finish()
}

#[derive(Default)]
struct TelexState {
    /// Accumulated bare consonants + vowels (no tone mark).
    syllable: String,
    /// Current tone.
    tone: Tone,
    /// Whether this is unambiguously a foreign (non-Vietnamese) word.
    is_foreign: bool,
    /// Whether a real (non-Flat) tone key has been consumed.
    tone_applied: bool,
    /// Raw character buffer (for triple-press detection).
    raw: String,
}

impl TelexState {
    fn push(&mut self, ch: char) {
        self.raw.push(ch);
        let lower = ch.to_lowercase().next().unwrap_or(ch);

        // --- Tone key? ---
        if let Some(tone) = tone_key(lower) {
            // Tone keys are applied if the syllable has at least one vowel.
            // 'z' explicitly resets tone to Flat.
            if has_vowel(&self.syllable) || tone == Tone::Flat {
                if self.tone == tone && tone != Tone::Flat {
                    // Same tone key twice cancels it and types the letter, so
                    // "ass" is "as" rather than "á" with the second 's' eaten.
                    self.tone = Tone::Flat;
                    self.tone_applied = false;
                    self.syllable.push(lower);
                } else {
                    self.tone = tone;
                    self.tone_applied = tone != Tone::Flat;
                }
                return;
            }
            // Otherwise fall through and treat as a literal character.
        }

        // A vowel arriving after a tone key cannot be Vietnamese: in Telex the
        // tone comes after the rime. This is what marks "reset", "user" and
        // "server" as foreign instead of composing "rết", "ủe", "sẻver".
        if self.tone_applied && is_vowel(lower) {
            self.is_foreign = true;
        }

        // --- "uo" + w takes both horns at once ---
        // Checked before the single-vowel pairs below, which would otherwise put
        // a horn on the 'o' alone and give "thuơng" instead of "thương".
        if lower == 'w' && self.syllable.ends_with("uo") {
            if let Some(new_syl) = apply_horn_cluster(&self.syllable) {
                self.syllable = new_syl;
                return;
            }
        }

        // --- Diacritic pair? ---
        if let Some(replacement) = diacritic_pair(&self.syllable, lower) {
            match replacement {
                PairResult::Replace(new_syl) => {
                    self.syllable = new_syl;
                    return;
                }
                PairResult::Restore(new_syl) => {
                    self.syllable = new_syl;
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

        // --- Regular character ---
        self.syllable.push(lower);
    }

    fn finish(self) -> MethodResult {
        MethodResult {
            bare: self.syllable,
            tone: self.tone,
            is_foreign: self.is_foreign,
        }
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
        ('â', 'a') => Some(PairResult::Restore(format!("{prefix}a"))),

        // aw → ă
        ('a', 'w') => Some(PairResult::Replace(format!("{prefix}ă"))),
        ('ă', 'w') => Some(PairResult::Restore(format!("{prefix}a"))),

        // ee → ê
        ('e', 'e') => Some(PairResult::Replace(format!("{prefix}ê"))),
        ('ê', 'e') => Some(PairResult::Restore(format!("{prefix}e"))),

        // oo → ô
        ('o', 'o') => Some(PairResult::Replace(format!("{prefix}ô"))),
        ('ô', 'o') => Some(PairResult::Restore(format!("{prefix}o"))),

        // ow → ơ
        ('o', 'w') => Some(PairResult::Replace(format!("{prefix}ơ"))),
        ('ơ', 'w') => Some(PairResult::Restore(format!("{prefix}o"))),

        // uw → ư
        ('u', 'w') => Some(PairResult::Replace(format!("{prefix}ư"))),
        ('ư', 'w') => Some(PairResult::Restore(format!("{prefix}u"))),

        // dd → đ
        ('d', 'd') => Some(PairResult::Replace(format!("{prefix}đ"))),
        ('đ', 'd') => Some(PairResult::Restore(format!("{prefix}d"))),

        _ => None,
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
        let r = process_telex(s);
        (r.bare, r.tone)
    }

    #[test]
    fn basic_tones() {
        assert_eq!(telex("has"), ("ha".into(), Tone::Sharp));
        assert_eq!(telex("haf"), ("ha".into(), Tone::Grave));
        assert_eq!(telex("har"), ("ha".into(), Tone::Hook));
        assert_eq!(telex("hax"), ("ha".into(), Tone::Tilde));
        assert_eq!(telex("haj"), ("ha".into(), Tone::Dot));
        assert_eq!(telex("haz"), ("ha".into(), Tone::Flat));
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
        // "aaa" → first 'aa' → â, then 'a' → restore to 'a'
        assert_eq!(telex("aaa").0, "a");
        assert_eq!(telex("eee").0, "e");
        assert_eq!(telex("ddd").0, "d");
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
