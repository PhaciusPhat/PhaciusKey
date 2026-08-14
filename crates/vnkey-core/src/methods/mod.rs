pub mod telex;
pub mod vni;

pub use telex::TelexMethod;
pub use vni::VniMethod;

use std::borrow::Cow;

use crate::types::{Config, Tone};

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
    /// True when a key put back the two that spell a diacritic — `ddd` → `dd`,
    /// `oww` → `ow`. No English word triples a letter that way, so unlike a
    /// cancelled tone this is never a word the typist meant to write.
    pub restored_spelling: bool,
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

pub trait InputMethodProcessor {
    /// Process the full raw buffer; `None` if it is empty.
    fn process(&self, raw: &str, config: &Config) -> Option<MethodResult>;
}
