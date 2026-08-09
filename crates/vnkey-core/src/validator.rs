use crate::types::Tone;

/// Whether `syllable` (lowercased, NFC) is a legal Vietnamese syllable.
pub fn is_valid_syllable(syllable: &str) -> bool {
    if syllable.is_empty() {
        return false;
    }
    let bare = strip_tone_marks(syllable);
    let s = bare.to_lowercase();

    let valid = onset_splits(&s).any(|(onset, rest)| match parse_rime(rest) {
        Some((nucleus, coda)) => validate_combination(onset, nucleus, coda),
        None => false,
    });
    valid
}

fn onset_splits(s: &str) -> impl Iterator<Item = (&'static str, &str)> {
    ONSETS
        .iter()
        .filter_map(move |&o| s.strip_prefix(o).map(|rest| (o, rest)))
        .chain(std::iter::once(("", s)))
}

/// Whether `s` could still grow into a valid Vietnamese syllable; partial onsets count.
pub fn is_valid_prefix(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    let bare = strip_tone_marks(s);
    let s = bare.to_lowercase();

    if ONSETS
        .iter()
        .any(|o| o.chars().count() > s.chars().count() && o.starts_with(&s))
    {
        return true;
    }

    let valid = onset_splits(&s).any(|(onset, rest)| {
        if onset.is_empty() && !s.chars().next().is_some_and(is_vowel_char) {
            return false;
        }
        is_valid_rime_prefix(rest)
    });
    valid
}

fn is_valid_rime_prefix(rest: &str) -> bool {
    let mut vowels = 0usize;
    let mut coda = String::new();

    for ch in rest.chars() {
        if coda.is_empty() && is_vowel_char(ch) {
            vowels += 1;
            if vowels > 3 {
                return false;
            }
        } else if is_coda_char(ch) {
            coda.push(ch);
            if !CODAS.iter().any(|c| c.starts_with(&coda)) {
                return false;
            }
        } else {
            return false;
        }
    }

    vowels != 0 || coda.is_empty()
}

/// Whether `tone` may occur on a syllable ending in `coda`; stop codas take only sắc or nặng.
pub fn tone_allowed_with_coda(tone: Tone, coda: &str) -> bool {
    let stop = matches!(coda, "p" | "t" | "c" | "ch");
    if !stop {
        return true;
    }
    matches!(tone, Tone::Sharp | Tone::Dot)
}

/// The coda of `syllable`, if it has one; tone marks are stripped first.
pub fn coda_of(syllable: &str) -> &'static str {
    let bare = strip_tone_marks(syllable).to_lowercase();
    for coda in CODAS {
        if bare.ends_with(coda) {
            return coda;
        }
    }
    ""
}

pub fn strip_tone_marks(s: &str) -> String {
    s.chars().map(|c| base_vowel(c).unwrap_or(c)).collect()
}

/// Map a toned/diacritical vowel to its base form.
pub fn base_vowel(c: char) -> Option<char> {
    match c {
        'à' | 'á' | 'ả' | 'ã' | 'ạ' => Some('a'),
        'ầ' | 'ấ' | 'ẩ' | 'ẫ' | 'ậ' | 'â' => Some('â'),
        'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'ă' => Some('ă'),
        'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' => Some('e'),
        'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' | 'ê' => Some('ê'),
        'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' => Some('i'),
        'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' => Some('o'),
        'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ô' => Some('ô'),
        'ờ' | 'ớ' | 'ở' | 'ỡ' | 'ợ' | 'ơ' => Some('ơ'),
        'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' => Some('u'),
        'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' | 'ư' => Some('ư'),
        'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => Some('y'),
        _ => None,
    }
}

const ONSETS: &[&str] = &[
    "ngh", "gh", "gi", "ng", "nh", "ph", "th", "tr", "ch", "kh", "qu", "b", "c", "d", "đ", "g",
    "h", "k", "l", "m", "n", "p", "r", "s", "t", "v", "x",
];

fn parse_rime(s: &str) -> Option<(&'static str, &'static str)> {
    let (nucleus, after_nucleus) = consume_nucleus(s)?;
    let coda = if after_nucleus.is_empty() {
        ""
    } else {
        match consume_coda(after_nucleus) {
            Some((c, "")) => c,
            _ => return None,
        }
    };
    Some((nucleus, coda))
}

const NUCLEI: &[&str] = &[
    "uôi", "ươi", "iêu", "ươu", "iê", "yê", "uô", "ươ", "ua", "ia", "oa", "oe", "uy", "oo", "ao",
    "ai", "au", "oi", "ôi", "ơi", "ui", "ưi", "eo", "êu", "iu", "ay", "ây", "â", "ă", "ê", "ô",
    "ơ", "ư", "a", "e", "i", "o", "u", "y",
];

const CODAS: &[&str] = &["ng", "nh", "ch", "c", "m", "n", "p", "t"];

fn is_vowel_char(c: char) -> bool {
    matches!(
        c,
        'a' | 'ă' | 'â' | 'e' | 'ê' | 'i' | 'o' | 'ô' | 'ơ' | 'u' | 'ư' | 'y'
    )
}

fn is_coda_char(c: char) -> bool {
    matches!(c, 'c' | 'h' | 'm' | 'n' | 'g' | 'p' | 't')
}

fn consume_nucleus(s: &str) -> Option<(&'static str, &str)> {
    for &nuc in NUCLEI {
        if let Some(rest) = s.strip_prefix(nuc) {
            return Some((nuc, rest));
        }
    }
    None
}

fn consume_coda(s: &str) -> Option<(&'static str, &str)> {
    for &coda in CODAS {
        if s == coda {
            return Some((coda, ""));
        }
    }
    None
}

fn validate_combination(onset: &str, nucleus: &str, coda: &str) -> bool {
    if matches!(onset, "gh" | "ngh") && !matches!(nucleus, "e" | "ê" | "i" | "iê" | "ia") {
        return false;
    }

    if onset == "gi" && (nucleus == "i" || nucleus == "ia" || nucleus == "iê") {
        return false;
    }

    if onset == "qu"
        && !matches!(
            nucleus,
            "a" | "â" | "e" | "ê" | "i" | "o" | "ô" | "oa" | "oe" | "uy" | "u"
        )
    {
        return false;
    }

    match coda {
        "c" | "p" => {
            if matches!(nucleus, "uôi" | "ươi" | "iêu" | "ươu") {
                return false;
            }
        }
        "ch" => {
            if !matches!(
                nucleus,
                "a" | "ă" | "â" | "e" | "ê" | "i" | "ia" | "iê" | "oa" | "u"
            ) {
                return false;
            }
        }
        "nh" if !matches!(
            nucleus,
            "a" | "ă" | "â" | "e" | "ê" | "i" | "ia" | "iê" | "oa" | "u" | "uy"
        ) =>
        {
            return false;
        }
        _ => {}
    }

    if matches!(nucleus, "uôi" | "ươi" | "iêu" | "ươu") && !coda.is_empty() {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_valid() {
        for word in &[
            "ba",
            "me",
            "bà",
            "mẹ",
            "việt",
            "nam",
            "hoa",
            "hoà",
            "hòa",
            "chào",
            "nghĩ",
            "quê",
            "đường",
            "thương",
            "trường",
        ] {
            assert!(is_valid_syllable(word), "expected valid: {word}");
        }
    }

    #[test]
    fn basic_invalid() {
        for word in &["test", "hello", "abc", "xzq", "bbb"] {
            assert!(!is_valid_syllable(word), "expected invalid: {word}");
        }
    }

    #[test]
    fn gh_ngh_only_front_vowels() {
        assert!(is_valid_syllable("ghi"));
        assert!(is_valid_syllable("nghe"));
        assert!(!is_valid_syllable("gha"));
        assert!(!is_valid_syllable("ngha"));
    }

    #[test]
    fn qu_onset() {
        assert!(is_valid_syllable("qua"));
        assert!(is_valid_syllable("quê"));
    }
}
