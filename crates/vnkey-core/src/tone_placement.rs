use std::borrow::Cow;

use crate::types::{Tone, TonePlacementMode};
use crate::validator::base_vowel;

/// Apply `tone` to `word` (a bare syllable, no existing tone marks).
pub fn apply_tone(word: &str, tone: Tone, mode: TonePlacementMode) -> Cow<'_, str> {
    if tone == Tone::Flat {
        return Cow::Borrowed(word);
    }

    let chars: Vec<char> = word.chars().collect();

    match tone_position(&chars, mode) {
        Some(idx) => Cow::Owned(
            chars
                .iter()
                .enumerate()
                .map(|(i, &c)| if i == idx { toned_vowel(c, tone) } else { c })
                .collect(),
        ),
        None => Cow::Borrowed(word),
    }
}

fn vowel_indices(
    chars: &[char],
    skip: Option<usize>,
) -> impl DoubleEndedIterator<Item = usize> + Clone + '_ {
    chars
        .iter()
        .enumerate()
        .filter(|&(i, &c)| is_vowel(c) && (i == 0 || chars[i - 1] != c))
        .map(|(i, _)| i)
        .filter(move |&i| Some(i) != skip)
}

fn tone_position(chars: &[char], mode: TonePlacementMode) -> Option<usize> {
    vowel_indices(chars, None).next()?;

    let glide_onset = matches!(
        (chars.first(), chars.get(1)),
        (Some('g'), Some('i')) | (Some('q'), Some('u'))
    );
    let skip = (glide_onset && vowel_indices(chars, None).nth(1).is_some()).then_some(1);
    let vowels = vowel_indices(chars, skip);

    let horn_cluster = chars
        .windows(2)
        .any(|w| w[0] == 'ư' && matches!(w[1], 'ơ' | 'o'));
    if horn_cluster {
        if let Some(vi) = vowels.clone().find(|&vi| matches!(chars[vi], 'ơ' | 'ô')) {
            return Some(vi);
        }
    }
    if let Some(vi) = vowels
        .clone()
        .find(|&vi| matches!(chars[vi], 'â' | 'ê' | 'ô' | 'ă' | 'ơ' | 'ư'))
    {
        return Some(vi);
    }

    let coda_start = coda_start_index(chars);

    match mode {
        TonePlacementMode::Modern => modern_position(vowels, coda_start),
        TonePlacementMode::Classic => classic_position(vowels, coda_start, chars),
    }
}

fn modern_position(
    vowels: impl DoubleEndedIterator<Item = usize> + Clone,
    coda_start: Option<usize>,
) -> Option<usize> {
    if let Some(cs) = coda_start {
        return vowels.clone().rfind(|&i| i < cs).or_else(|| vowels.last());
    }

    let mut back = vowels.rev();
    let last = back.next();
    back.next().or(last)
}

fn classic_position(
    vowels: impl DoubleEndedIterator<Item = usize> + Clone,
    coda_start: Option<usize>,
    chars: &[char],
) -> Option<usize> {
    if coda_start.is_none() {
        let mut pair = vowels.clone();
        if let (Some(first), Some(second), None) = (pair.next(), pair.next(), pair.next()) {
            let glide_pair = matches!(
                (chars.get(first), chars.get(second)),
                (Some('o'), Some('a')) | (Some('o'), Some('e')) | (Some('u'), Some('y'))
            );
            if glide_pair {
                return Some(second);
            }
        }
    }
    modern_position(vowels, coda_start)
}

fn coda_start_index(chars: &[char]) -> Option<usize> {
    const CODAS: &[&str] = &["ng", "nh", "ch", "c", "m", "n", "p", "t"];
    for coda in CODAS {
        let Some(start) = chars.len().checked_sub(coda.chars().count()) else {
            continue;
        };
        if chars[start..].iter().copied().eq(coda.chars()) {
            return Some(start);
        }
    }
    None
}

fn is_vowel(c: char) -> bool {
    is_base_vowel(base_vowel(c).unwrap_or(c))
}

fn is_base_vowel(c: char) -> bool {
    matches!(
        c,
        'a' | 'ă' | 'â' | 'e' | 'ê' | 'i' | 'o' | 'ô' | 'ơ' | 'u' | 'ư' | 'y'
    )
}

/// The tone a single lowercase vowel carries; inverse of [`toned_vowel`].
pub fn char_tone(c: char) -> Tone {
    let base = base_vowel(c).unwrap_or(c);
    if base == c {
        return Tone::Flat;
    }
    for tone in [Tone::Sharp, Tone::Grave, Tone::Hook, Tone::Tilde, Tone::Dot] {
        if toned_vowel(base, tone) == c {
            return tone;
        }
    }
    Tone::Flat
}

/// Map a base vowel + tone to the precomposed Unicode character.
pub fn toned_vowel(base: char, tone: Tone) -> char {
    let b = base_vowel(base).unwrap_or(base);
    match (b, tone) {
        ('a', Tone::Sharp) => 'á',
        ('a', Tone::Grave) => 'à',
        ('a', Tone::Hook) => 'ả',
        ('a', Tone::Tilde) => 'ã',
        ('a', Tone::Dot) => 'ạ',

        ('â', Tone::Sharp) => 'ấ',
        ('â', Tone::Grave) => 'ầ',
        ('â', Tone::Hook) => 'ẩ',
        ('â', Tone::Tilde) => 'ẫ',
        ('â', Tone::Dot) => 'ậ',

        ('ă', Tone::Sharp) => 'ắ',
        ('ă', Tone::Grave) => 'ằ',
        ('ă', Tone::Hook) => 'ẳ',
        ('ă', Tone::Tilde) => 'ẵ',
        ('ă', Tone::Dot) => 'ặ',

        ('e', Tone::Sharp) => 'é',
        ('e', Tone::Grave) => 'è',
        ('e', Tone::Hook) => 'ẻ',
        ('e', Tone::Tilde) => 'ẽ',
        ('e', Tone::Dot) => 'ẹ',

        ('ê', Tone::Sharp) => 'ế',
        ('ê', Tone::Grave) => 'ề',
        ('ê', Tone::Hook) => 'ể',
        ('ê', Tone::Tilde) => 'ễ',
        ('ê', Tone::Dot) => 'ệ',

        ('i', Tone::Sharp) => 'í',
        ('i', Tone::Grave) => 'ì',
        ('i', Tone::Hook) => 'ỉ',
        ('i', Tone::Tilde) => 'ĩ',
        ('i', Tone::Dot) => 'ị',

        ('o', Tone::Sharp) => 'ó',
        ('o', Tone::Grave) => 'ò',
        ('o', Tone::Hook) => 'ỏ',
        ('o', Tone::Tilde) => 'õ',
        ('o', Tone::Dot) => 'ọ',

        ('ô', Tone::Sharp) => 'ố',
        ('ô', Tone::Grave) => 'ồ',
        ('ô', Tone::Hook) => 'ổ',
        ('ô', Tone::Tilde) => 'ỗ',
        ('ô', Tone::Dot) => 'ộ',

        ('ơ', Tone::Sharp) => 'ớ',
        ('ơ', Tone::Grave) => 'ờ',
        ('ơ', Tone::Hook) => 'ở',
        ('ơ', Tone::Tilde) => 'ỡ',
        ('ơ', Tone::Dot) => 'ợ',

        ('u', Tone::Sharp) => 'ú',
        ('u', Tone::Grave) => 'ù',
        ('u', Tone::Hook) => 'ủ',
        ('u', Tone::Tilde) => 'ũ',
        ('u', Tone::Dot) => 'ụ',

        ('ư', Tone::Sharp) => 'ứ',
        ('ư', Tone::Grave) => 'ừ',
        ('ư', Tone::Hook) => 'ử',
        ('ư', Tone::Tilde) => 'ữ',
        ('ư', Tone::Dot) => 'ự',

        ('y', Tone::Sharp) => 'ý',
        ('y', Tone::Grave) => 'ỳ',
        ('y', Tone::Hook) => 'ỷ',
        ('y', Tone::Tilde) => 'ỹ',
        ('y', Tone::Dot) => 'ỵ',

        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Tone, TonePlacementMode};

    fn modern(word: &str, tone: Tone) -> String {
        apply_tone(word, tone, TonePlacementMode::Modern).into_owned()
    }

    #[test]
    fn single_vowel() {
        assert_eq!(modern("ba", Tone::Sharp), "bá");
        assert_eq!(modern("me", Tone::Grave), "mè");
        assert_eq!(modern("ho", Tone::Dot), "họ");
    }

    #[test]
    fn circumflex_priority() {
        assert_eq!(modern("han", Tone::Sharp), "hán");
        assert_eq!(modern("hân", Tone::Sharp), "hấn");
        assert_eq!(modern("hoang", Tone::Grave), "hoàng");
    }

    #[test]
    fn flat_tone_no_change() {
        assert_eq!(modern("ba", Tone::Flat), "ba");
    }

    #[test]
    fn a_key_repeated_past_the_nucleus_does_not_move_the_tone() {
        assert_eq!(modern("bay", Tone::Grave), "bày");
        assert_eq!(modern("bayy", Tone::Grave), "bàyy");
        assert_eq!(modern("bayyyyyyyy", Tone::Grave), "bàyyyyyyyy");
        assert_eq!(modern("doiiiii", Tone::Grave), "dòiiiii");
    }

    #[test]
    fn classic_vs_modern_differ_for_glide_nuclei() {
        for (bare, tone, modern_want, classic_want) in [
            ("hoa", Tone::Grave, "hòa", "hoà"),
            ("hoe", Tone::Sharp, "hóe", "hoé"),
            ("thuy", Tone::Sharp, "thúy", "thuý"),
        ] {
            assert_eq!(
                apply_tone(bare, tone, TonePlacementMode::Modern),
                modern_want
            );
            assert_eq!(
                apply_tone(bare, tone, TonePlacementMode::Classic),
                classic_want
            );
        }
    }

    #[test]
    fn classic_matches_modern_elsewhere() {
        for (bare, tone) in [
            ("toan", Tone::Sharp),
            ("hoang", Tone::Grave),
            ("mua", Tone::Grave),
        ] {
            assert_eq!(
                apply_tone(bare, tone, TonePlacementMode::Modern),
                apply_tone(bare, tone, TonePlacementMode::Classic),
            );
        }
    }

    #[test]
    fn glide_onset_never_takes_the_tone() {
        assert_eq!(modern("gia", Tone::Sharp), "giá");
        assert_eq!(modern("qua", Tone::Grave), "quà");
        assert_eq!(modern("gi", Tone::Grave), "gì");
        assert_eq!(modern("cu", Tone::Grave), "cù");
    }
}
