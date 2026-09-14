pub mod telex;
pub mod vni;

pub use telex::TelexMethod;
pub use vni::VniMethod;

use std::borrow::Cow;

use crate::types::{Config, InputMethod, Tone};
use crate::validator::{base_vowel, coda_of, is_valid_prefix, tone_allowed_with_coda};

/// Result of processing the raw buffer through an input method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodResult {
    /// The bare syllable: diacritics applied, no tone mark yet.
    pub bare: String,
    /// The tone extracted from the key sequence.
    pub tone: Tone,
    /// True if the sequence is unambiguously non-Vietnamese.
    pub is_foreign: bool,
    /// Exact text when a key undid its own diacritic or tone ("aaa" → "aa"); `None` echoes raw.
    pub literal: Option<String>,
    /// Per-character uppercase mask for `bare` (and `literal`).
    pub case_mask: Vec<bool>,
}

/// Re-apply a typed-case mask to a composed (lowercase) string.
pub fn apply_case_mask<'a>(text: &'a str, mask: &[bool]) -> Cow<'a, str> {
    if !mask.iter().any(|&upper| upper) {
        return Cow::Borrowed(text);
    }

    let mut out = String::with_capacity(text.len());
    for (i, c) in text.chars().enumerate() {
        if mask.get(i).copied().unwrap_or(false) {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Whether a tone key may take effect on the syllable typed so far, rather than
/// standing for the digit or letter it was typed as.
pub fn tone_applies(syllable: &str, tone: Tone, config: &Config) -> bool {
    syllable.chars().any(is_vowel)
        && syllable_possible(syllable, config)
        && tone_allowed_with_coda(tone, coda_of(syllable))
}

/// Whether `bare` can still grow into a Vietnamese syllable, counting the Telex
/// consonant shorthands as the syllables they stand for.
pub fn syllable_possible(bare: &str, config: &Config) -> bool {
    is_valid_prefix(bare)
        || expand_quick_consonants(bare, config).is_some_and(|e| is_valid_prefix(&e))
}

pub fn expand_quick_consonants(word: &str, config: &Config) -> Option<String> {
    if config.method != InputMethod::Telex
        || !(config.quick_start_consonant || config.quick_end_consonant)
    {
        return None;
    }
    let chars: Vec<char> = word.chars().collect();
    if chars.len() < 2 {
        return None;
    }

    let head = config
        .quick_start_consonant
        .then(|| start_consonant(lower(chars[0])))
        .flatten()
        .map(|(a, b)| {
            let first = chars[0].is_uppercase();
            [cased(a, first), cased(b, first && chars[1].is_uppercase())]
        });

    let last = chars[chars.len() - 1];
    let tail = config
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

/// Take the mark off the last vowel when it is one this key puts there; `None`
/// when that vowel is unmarked or carries a mark from another key.
pub fn undo_last_vowel_mark(syllable: &str, marked: &[char]) -> Option<String> {
    let chars: Vec<char> = syllable.chars().collect();
    let i = chars.iter().rposition(|&c| is_vowel(c))?;
    if !marked.contains(&chars[i]) {
        return None;
    }
    let mut out: String = chars[..i].iter().collect();
    out.push(unmarked(chars[i]));
    out.extend(chars[i + 1..].iter());
    Some(out)
}

fn unmarked(ch: char) -> char {
    match ch {
        'â' | 'ă' => 'a',
        'ê' => 'e',
        'ô' | 'ơ' => 'o',
        'ư' => 'u',
        other => other,
    }
}

pub trait InputMethodProcessor {
    /// Process the full raw buffer; `None` if it is empty.
    fn process(&self, raw: &str, config: &Config) -> Option<MethodResult>;
}
