use super::{InputMethodProcessor, MethodResult};
use crate::types::{Config, Tone};

pub struct VniMethod;

impl InputMethodProcessor for VniMethod {
    fn process(&self, raw: &str, _config: &Config) -> Option<MethodResult> {
        if raw.is_empty() {
            return None;
        }
        Some(process_vni(raw))
    }
}

/// Process a raw VNI keystroke sequence.
pub fn process_vni(raw: &str) -> MethodResult {
    let mut syllable = String::new();
    let mut mask: Vec<bool> = Vec::new();
    let mut tone = Tone::Flat;
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
            let acts = if new_tone == Tone::Flat {
                tone != Tone::Flat
            } else {
                has_vowel(&syllable)
            };
            if !acts {
                syllable.push(ch);
                mask.push(false);
            } else if tone == new_tone && new_tone != Tone::Flat {
                tone = Tone::Flat;
                cancelled = true;
                syllable.push(ch);
                mask.push(false);
            } else {
                tone = new_tone;
            }
            continue;
        }

        match ch {
            '6' => {
                if undo_diacritic(&mut syllable, &['â', 'ê', 'ô']) {
                    cancelled = true;
                    syllable.push(ch);
                    mask.push(false);
                } else if !apply_circumflex(&mut syllable) {
                    syllable.push(ch);
                    mask.push(false);
                }
            }
            '7' => {
                if undo_diacritic(&mut syllable, &['ư', 'ơ']) {
                    cancelled = true;
                    syllable.push(ch);
                    mask.push(false);
                } else if !apply_horn(&mut syllable) {
                    syllable.push(ch);
                    mask.push(false);
                }
            }
            '8' => {
                if undo_diacritic(&mut syllable, &['ă']) {
                    cancelled = true;
                    syllable.push(ch);
                    mask.push(false);
                } else if !apply_breve(&mut syllable) {
                    syllable.push(ch);
                    mask.push(false);
                }
            }
            '9' => {
                if syllable.starts_with('đ') {
                    syllable.replace_range(..'đ'.len_utf8(), "d");
                    cancelled = true;
                    syllable.push(ch);
                    mask.push(false);
                } else if !apply_stroke_d(&mut syllable) {
                    syllable.push(ch);
                    mask.push(false);
                }
            }
            _ => {
                syllable.push(ch.to_lowercase().next().unwrap_or(ch));
                mask.push(ch.is_uppercase());
            }
        }
    }

    let literal = cancelled.then(|| syllable.clone());
    MethodResult {
        bare: syllable.replace("ưo", "ươ"),
        tone,
        is_foreign: false,
        literal,
        case_mask: mask,
    }
}

/// Turn displayed text back into the canonical VNI keys that produce it: "rượ" → "ru7o75".
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
            tone = t;
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
        out.push(if upper {
            letter.to_uppercase().next().unwrap_or(letter)
        } else {
            letter
        });
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

fn undo_diacritic(s: &mut String, marked: &[char]) -> bool {
    let chars: Vec<char> = s.chars().collect();
    for i in (0..chars.len()).rev() {
        if !is_vowel(chars[i]) {
            continue;
        }
        if !marked.contains(&chars[i]) {
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
    matches!(
        c,
        'a' | 'â' | 'ă' | 'e' | 'ê' | 'i' | 'o' | 'ô' | 'ơ' | 'u' | 'ư' | 'y'
    )
}

fn has_vowel(s: &str) -> bool {
    s.chars().any(is_vowel)
}

fn apply_circumflex(s: &mut String) -> bool {
    replace_last_vowel(s, |c| match c {
        'a' => Some('â'),
        'e' => Some('ê'),
        'o' => Some('ô'),
        _ => None,
    })
}

fn apply_horn(s: &mut String) -> bool {
    if let Some(pos) = s.find("uo") {
        let mut new = s[..pos].to_string();
        new.push('ư');
        new.push('ơ');
        new.push_str(&s[pos + 2..]);
        *s = new;
        return true;
    }
    if replace_last_vowel(s, |c| if c == 'u' { Some('ư') } else { None }) {
        return true;
    }
    replace_last_vowel(s, |c| if c == 'o' { Some('ơ') } else { None })
}

fn apply_breve(s: &mut String) -> bool {
    replace_last_vowel(s, |c| match c {
        'a' => Some('ă'),
        _ => None,
    })
}

fn apply_stroke_d(s: &mut String) -> bool {
    if s.starts_with('d') {
        s.replace_range(..1, "đ");
        true
    } else {
        false
    }
}

fn replace_last_vowel(s: &mut String, f: impl Fn(char) -> Option<char>) -> bool {
    let chars: Vec<char> = s.chars().collect();
    for i in (0..chars.len()).rev() {
        if let Some(replacement) = f(chars[i]) {
            let mut new = String::new();
            for (j, &c) in chars.iter().enumerate() {
                if j == i {
                    new.push(replacement);
                } else {
                    new.push(c);
                }
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
        assert_eq!(vni("d9oan11").0, "đoan1");
        assert_eq!(vni("a11").0, "a1");
        assert_eq!(vni("a66").0, "a6");
        assert_eq!(vni("o77").0, "o7");
        assert_eq!(vni("a88").0, "a8");
        assert_eq!(vni("d99").0, "d9");
        assert_eq!(vni("a12"), ("a".into(), Tone::Grave));
    }

    #[test]
    fn combined() {
        let (bare, tone) = vni("duong7");
        assert_eq!(bare, "dương");
        assert_eq!(tone, Tone::Flat);

        let (bare, tone) = vni("duong71");
        assert_eq!(bare, "dương");
        assert_eq!(tone, Tone::Sharp);
    }
}
