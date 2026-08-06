use crate::types::Tone;
use super::{InputMethodProcessor, MethodResult};

pub struct VniMethod;

impl InputMethodProcessor for VniMethod {
    fn process(&self, raw: &str) -> Option<MethodResult> {
        if raw.is_empty() {
            return None;
        }
        Some(process_vni(raw))
    }
}

/// Process a raw VNI keystroke sequence.
///
/// VNI rules:
///   1 → sắc, 2 → huyền, 3 → hỏi, 4 → ngã, 5 → nặng, 0 → remove tone
///   6 → circumflex on preceding vowel (a→â, e→ê, o→ô)
///   7 → horn on preceding vowel (o→ơ, u→ư)
///   8 → breve on preceding vowel (a→ă)
///   9 → đ (only replaces 'd' at onset)
pub fn process_vni(raw: &str) -> MethodResult {
    let mut syllable = String::new();
    let mut tone = Tone::Flat;
    // Set when a digit undid its own tone/diacritic, which makes the word literal
    // text: "đoán" then '1' is "đoan1", not "đoán" with the digit swallowed.
    let mut cancelled = false;

    for ch in raw.chars() {
        let tone_digit = match ch {
            '1' => Some(Tone::Sharp),
            '2' => Some(Tone::Grave),
            '3' => Some(Tone::Hook),
            '4' => Some(Tone::Tilde),
            '5' => Some(Tone::Dot),
            '0' => Some(Tone::Flat),
            _ => None,
        };

        if let Some(new_tone) = tone_digit {
            // A tone digit needs a vowel to carry the mark, and '0' (remove
            // tone) additionally needs a tone to remove — with nothing to undo
            // it is the digit 0, mirroring Telex's 'z'.
            let acts = if new_tone == Tone::Flat {
                tone != Tone::Flat
            } else {
                has_vowel(&syllable)
            };
            if !acts {
                syllable.push(ch);
            } else if tone == new_tone && new_tone != Tone::Flat {
                // Same digit twice: undo the tone and type the digit.
                tone = Tone::Flat;
                cancelled = true;
                syllable.push(ch);
            } else {
                tone = new_tone;
            }
            continue;
        }

        match ch {
            // Each diacritic digit first tries to undo itself, then to apply.
            '6' => {
                if undo_diacritic(&mut syllable, &['â', 'ê', 'ô']) {
                    cancelled = true;
                    syllable.push(ch);
                } else if !apply_circumflex(&mut syllable) {
                    syllable.push(ch);
                }
            }
            '7' => {
                if undo_diacritic(&mut syllable, &['ư', 'ơ']) {
                    cancelled = true;
                    syllable.push(ch);
                } else if !apply_horn(&mut syllable) {
                    syllable.push(ch);
                }
            }
            '8' => {
                if undo_diacritic(&mut syllable, &['ă']) {
                    cancelled = true;
                    syllable.push(ch);
                } else if !apply_breve(&mut syllable) {
                    syllable.push(ch);
                }
            }
            '9' => {
                if syllable.starts_with('đ') {
                    syllable.replace_range(..'đ'.len_utf8(), "d");
                    cancelled = true;
                    syllable.push(ch);
                } else if !apply_stroke_d(&mut syllable) {
                    syllable.push(ch);
                }
            }
            _ => {
                syllable.push(ch.to_lowercase().next().unwrap_or(ch));
            }
        }
    }

    let literal = cancelled.then(|| syllable.clone());
    MethodResult {
        // "ưo" never occurs in Vietnamese — the horn always spans the pair, so
        // "ru7ou" comes out as "rươu" (see the same rule in Telex).
        bare: syllable.replace("ưo", "ươ"),
        tone,
        is_foreign: false,
        literal,
    }
}

/// Turn displayed text back into the canonical VNI keys that produce it:
/// "rượ" → "ru7o75". Used by Backspace so composing can continue after a
/// character is deleted.
pub fn encode_vni(text: &str) -> String {
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
        let (letter, digit) = match base {
            'â' => ('a', Some('6')),
            'ê' => ('e', Some('6')),
            'ô' => ('o', Some('6')),
            'ơ' => ('o', Some('7')),
            'ư' => ('u', Some('7')),
            'ă' => ('a', Some('8')),
            'đ' => ('d', Some('9')),
            other => (other, None),
        };
        out.push(if upper { letter.to_uppercase().next().unwrap_or(letter) } else { letter });
        if let Some(d) = digit {
            out.push(d);
        }
    }

    let tone_digit = match tone {
        Tone::Sharp => Some('1'),
        Tone::Grave => Some('2'),
        Tone::Hook => Some('3'),
        Tone::Tilde => Some('4'),
        Tone::Dot => Some('5'),
        Tone::Flat => None,
    };
    if let Some(d) = tone_digit {
        out.push(d);
    }
    out
}

/// If the last vowel carries one of `marked`, put it back to its base form.
/// Returns whether anything was undone.
fn undo_diacritic(s: &mut String, marked: &[char]) -> bool {
    let chars: Vec<char> = s.chars().collect();
    for i in (0..chars.len()).rev() {
        if !is_vowel(chars[i]) {
            continue;
        }
        if !marked.contains(&chars[i]) {
            // The most recent vowel does not carry this mark, so there is nothing
            // to undo — the digit should apply normally instead.
            return false;
        }
        let base = match chars[i] {
            'â' => 'a',
            'ê' => 'e',
            'ô' => 'o',
            'ơ' => 'o',
            'ư' => 'u',
            'ă' => 'a',
            other => other,
        };
        let mut out: String = chars[..i].iter().collect();
        out.push(base);
        out.extend(chars[i + 1..].iter());
        *s = out;
        return true;
    }
    false
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a'|'â'|'ă'|'e'|'ê'|'i'|'o'|'ô'|'ơ'|'u'|'ư'|'y')
}

fn has_vowel(s: &str) -> bool {
    s.chars().any(is_vowel)
}

/// Apply circumflex to the last eligible vowel (a→â, e→ê, o→ô). Returns true if applied.
fn apply_circumflex(s: &mut String) -> bool {
    replace_last_vowel(s, |c| match c {
        'a' => Some('â'),
        'e' => Some('ê'),
        'o' => Some('ô'),
        _ => None,
    })
}

/// Apply horn (o→ơ, u→ư). Returns true if applied.
/// The compound "uo" cluster → "ươ" both chars together.
fn apply_horn(s: &mut String) -> bool {
    // Handle "uo" compound: both u and o get horn together.
    if let Some(pos) = s.find("uo") {
        let mut new = s[..pos].to_string();
        new.push('ư');
        new.push('ơ');
        new.push_str(&s[pos + 2..]);
        *s = new;
        return true;
    }
    // Single vowel: try 'u' first, then 'o'.
    if replace_last_vowel(s, |c| if c == 'u' { Some('ư') } else { None }) {
        return true;
    }
    replace_last_vowel(s, |c| if c == 'o' { Some('ơ') } else { None })
}

/// Apply breve (a→ă). Returns true if applied.
fn apply_breve(s: &mut String) -> bool {
    replace_last_vowel(s, |c| match c {
        'a' => Some('ă'),
        _ => None,
    })
}

/// Replace 'd' at the start of the syllable with 'đ'. Returns true if applied.
fn apply_stroke_d(s: &mut String) -> bool {
    if s.starts_with('d') {
        s.replace_range(..1, "đ");
        true
    } else {
        false
    }
}

/// Replace the last vowel in `s` using `f`. Returns true if a replacement was made.
fn replace_last_vowel(s: &mut String, f: impl Fn(char) -> Option<char>) -> bool {
    let chars: Vec<char> = s.chars().collect();
    // Find rightmost vowel.
    for i in (0..chars.len()).rev() {
        if let Some(replacement) = f(chars[i]) {
            // Rebuild the string.
            let mut new = String::new();
            for (j, &c) in chars.iter().enumerate() {
                if j == i { new.push(replacement); } else { new.push(c); }
            }
            *s = new;
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Tone;

    fn vni(s: &str) -> (String, Tone) {
        let r = process_vni(s);
        (r.bare, r.tone)
    }

    #[test]
    fn basic_tones() {
        assert_eq!(vni("ha1"), ("ha".into(), Tone::Sharp));
        assert_eq!(vni("ha2"), ("ha".into(), Tone::Grave));
        assert_eq!(vni("ha3"), ("ha".into(), Tone::Hook));
        assert_eq!(vni("ha4"), ("ha".into(), Tone::Tilde));
        assert_eq!(vni("ha5"), ("ha".into(), Tone::Dot));
        // 0 removes an existing tone; with no tone it is the digit 0.
        assert_eq!(vni("ha10"), ("ha".into(), Tone::Flat));
        assert_eq!(vni("ha0"), ("ha0".into(), Tone::Flat));
    }

    #[test]
    fn circumflex() {
        assert_eq!(vni("a6").0, "â");
        assert_eq!(vni("e6").0, "ê");
        assert_eq!(vni("o6").0, "ô");
    }

    #[test]
    fn horn() {
        assert_eq!(vni("o7").0, "ơ");
        assert_eq!(vni("u7").0, "ư");
    }

    #[test]
    fn breve() {
        assert_eq!(vni("a8").0, "ă");
    }

    #[test]
    fn stroke_d() {
        assert_eq!(vni("d9").0, "đ");
    }

    #[test]
    fn repeating_a_digit_undoes_it_and_types_it() {
        // The reported case: "đoán" then '1' is "đoan1".
        assert_eq!(vni("d9oan11").0, "đoan1");
        assert_eq!(vni("a11").0, "a1");
        assert_eq!(vni("a66").0, "a6");
        assert_eq!(vni("o77").0, "o7");
        assert_eq!(vni("a88").0, "a8");
        assert_eq!(vni("d99").0, "d9");
        // A *different* digit still just changes the tone.
        assert_eq!(vni("a12"), ("a".into(), Tone::Grave));
    }

    #[test]
    fn combined() {
        // "duong7" → ư applied to 'u' → "dương" bare
        let (bare, tone) = vni("duong7");
        assert_eq!(bare, "dương");
        assert_eq!(tone, Tone::Flat);

        // "duong71" → ư + sắc
        let (bare, tone) = vni("duong71");
        assert_eq!(bare, "dương");
        assert_eq!(tone, Tone::Sharp);
    }
}
