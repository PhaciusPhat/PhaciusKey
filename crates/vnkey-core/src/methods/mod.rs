pub mod telex;
pub mod vni;

pub use telex::TelexMethod;
pub use vni::VniMethod;

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
    /// Per-character uppercase mask for `bare` (and `literal`).
    pub case_mask: Vec<bool>,
}

/// Re-apply a typed-case mask to a composed (lowercase) string.
pub fn apply_case_mask(text: &str, mask: &[bool]) -> String {
    text.chars()
        .enumerate()
        .flat_map(|(i, c)| {
            let upper = mask.get(i).copied().unwrap_or(false);
            let mut out = Vec::new();
            if upper {
                out.extend(c.to_uppercase());
            } else {
                out.push(c);
            }
            out
        })
        .collect()
}

pub trait InputMethodProcessor {
    /// Process the full raw buffer; `None` if it is empty.
    fn process(&self, raw: &str, config: &Config) -> Option<MethodResult>;
}
