use super::{InputMethodProcessor, MethodResult};
use crate::types::{Config, Tone};
use crate::validator::is_valid_prefix;

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
pub fn process_telex(raw: &str, config: &Config) -> MethodResult {
    let mut state = TelexState {
        standalone_w_enabled: config.standalone_w,
        quick_telex_enabled: config.quick_telex,
        ..Default::default()
    };
    for ch in raw.chars() {
        state.push(ch);
    }
    state.finish()
}

#[derive(Default)]
struct TelexState {
    standalone_w_enabled: bool,
    quick_telex_enabled: bool,
    syllable: String,
    tone: Tone,
    is_foreign: bool,
    tone_applied: bool,
    cancelled: bool,
    mask: Vec<bool>,
    // OpenKey STANDALONE_MASK.
    standalone_w: Option<usize>,
}

impl TelexState {
    fn push(&mut self, ch: char) {
        self.push_key(ch);
        if let Some(pos) = self.standalone_w {
            if self.syllable.chars().nth(pos) != Some('ư') {
                self.standalone_w = None;
            }
        }
    }

    fn push_key(&mut self, ch: char) {
        let lower = ch.to_lowercase().next().unwrap_or(ch);

        if self.quick_telex_enabled {
            if let Some(completion) = quick_telex_completion(&self.syllable, lower) {
                self.syllable.push(completion);
                self.mask.push(ch.is_uppercase());
                return;
            }
        }

        if let Some(tone) = tone_key(lower) {
            let acts = if tone == Tone::Flat {
                self.tone != Tone::Flat
            } else {
                has_vowel(&self.syllable)
            };
            if acts && !self.cancelled {
                if self.tone == tone && tone != Tone::Flat {
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
        }

        if self.tone_applied && is_vowel(lower) && !matches!(lower, 'i' | 'u' | 'y' | 'o') {
            self.is_foreign = true;
        }

        if lower == 'w' && (self.syllable.ends_with("uo") || self.syllable.ends_with("ua")) {
            if let Some(new_syl) = apply_horn_cluster(&self.syllable) {
                self.syllable = new_syl;
                return;
            }
        }

        if lower == 'w'
            && self
                .standalone_w
                .is_some_and(|pos| pos + 1 == self.syllable.chars().count())
        {
            self.syllable.pop();
            self.syllable.push('w');
            if let Some(flag) = self.mask.last_mut() {
                *flag = ch.is_uppercase();
            }
            self.cancelled = true;
            return;
        }

        if let Some(replacement) = diacritic_pair(&self.syllable, lower) {
            match replacement {
                PairResult::Replace(new_syl) => {
                    self.syllable = new_syl;
                    return;
                }
                PairResult::Restore(new_syl) => {
                    self.syllable = new_syl;
                    self.mask.push(ch.is_uppercase());
                    self.cancelled = true;
                    return;
                }
            }
        }

        if lower == 'w' {
            if let Some(new_syl) = apply_horn_cluster(&self.syllable) {
                self.syllable = new_syl;
                return;
            }
        }

        if lower == 'w' && self.standalone_w_enabled && standalone_w_applies(&self.syllable) {
            self.standalone_w = Some(self.syllable.chars().count());
            self.syllable.push('ư');
            self.mask.push(ch.is_uppercase());
            return;
        }

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

        self.syllable.push(lower);
        self.mask.push(ch.is_uppercase());
    }

    fn finish(self) -> MethodResult {
        let literal = self.cancelled.then(|| self.syllable.clone());
        MethodResult {
            bare: match self.syllable.find("ưo") {
                Some(_) => self.syllable.replace("ưo", "ươ"),
                None => self.syllable,
            },
            tone: self.tone,
            is_foreign: self.is_foreign,
            literal,
            case_mask: self.mask,
        }
    }
}

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

/// Turn displayed text back into the canonical Telex keys that produce it: "rượ" → "ruwowj".
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
            tone = t;
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
                out.push(if upper {
                    base.to_uppercase().next().unwrap_or(base)
                } else {
                    base
                });
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
        let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
        let all_caps = letters.len() > 1 && letters.iter().all(|c| c.is_uppercase());
        out.push(if all_caps {
            key.to_ascii_uppercase()
        } else {
            key
        });
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

enum PairResult {
    Replace(String),
    Restore(String),
}

fn diacritic_pair(syllable: &str, ch: char) -> Option<PairResult> {
    let last = syllable.chars().last()?;
    let prefix = &syllable[..syllable.len() - last.len_utf8()];

    match (last, ch) {
        ('a', 'a') => Some(PairResult::Replace(format!("{prefix}â"))),
        ('â', 'a') => Some(PairResult::Restore(format!("{prefix}aa"))),

        ('a', 'w') => Some(PairResult::Replace(format!("{prefix}ă"))),
        ('ă', 'w') => Some(PairResult::Restore(format!("{prefix}aw"))),

        ('e', 'e') => Some(PairResult::Replace(format!("{prefix}ê"))),
        ('ê', 'e') => Some(PairResult::Restore(format!("{prefix}ee"))),

        ('o', 'o') => Some(PairResult::Replace(format!("{prefix}ô"))),
        ('ô', 'o') => Some(PairResult::Restore(format!("{prefix}oo"))),

        ('o', 'w') => Some(PairResult::Replace(format!("{prefix}ơ"))),
        ('ơ', 'w') => Some(PairResult::Restore(format!("{prefix}ow"))),

        ('u', 'w') => Some(PairResult::Replace(format!("{prefix}ư"))),
        ('ư', 'w') => Some(PairResult::Restore(format!("{prefix}uw"))),

        ('d', 'd') => Some(PairResult::Replace(format!("{prefix}đ"))),
        ('đ', 'd') => Some(PairResult::Restore(format!("{prefix}dd"))),

        _ => None,
    }
}

// OpenKey's seven quick-Telex consonants.
fn quick_telex_completion(syllable: &str, ch: char) -> Option<char> {
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
        // OpenKey IS_QUICK_TELEX_KEY.
        _ => None,
    }
}

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

// OpenKey checkForStandaloneChar.
fn standalone_w_applies(syllable: &str) -> bool {
    let so_far: Vec<char> = syllable.chars().map(base_letter).collect();
    match so_far.len() {
        0 => true,
        // OpenKey _standaloneWbad.
        1 => !matches!(so_far[0], 'w' | 'e' | 'y' | 'f' | 'j' | 'k' | 'z'),
        2 => {
            let onset: String = so_far.iter().collect();
            matches!(
                onset.as_str(),
                "tr" | "th" | "ch" | "nh" | "ng" | "kh" | "gi" | "ph" | "gh"
            )
        }
        _ => false,
    }
}

fn has_vowel(s: &str) -> bool {
    s.chars().any(is_vowel)
}

fn is_vowel(c: char) -> bool {
    matches!(
        c,
        'a' | 'â' | 'ă' | 'e' | 'ê' | 'i' | 'o' | 'ô' | 'ơ' | 'u' | 'ư' | 'y'
    )
}

fn apply_horn_cluster(syllable: &str) -> Option<String> {
    if let Some(pos) = syllable.rfind("uo") {
        let mut out = syllable[..pos].to_string();
        out.push('ư');
        out.push('ơ');
        out.push_str(&syllable[pos + 2..]);
        return Some(out);
    }

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
        assert_eq!(telex("tw").0, "tư");
        assert_eq!(telex("nw").0, "nư");
        assert_eq!(telex("mw").0, "mư");
        assert_eq!(telex("kw").0, "kw");
        assert_eq!(telex("zw").0, "zw");
        assert_eq!(telex("fw").0, "fw");
        assert_eq!(telex("jw").0, "jw");
        assert_eq!(telex("ew").0, "ew");
        assert_eq!(telex("yw").0, "yw");
    }

    #[test]
    fn standalone_w_after_a_two_letter_onset() {
        for (seq, want) in [
            ("thw", "thư"),
            ("trw", "trư"),
            ("chw", "chư"),
            ("nhw", "như"),
            ("ngw", "ngư"),
            ("khw", "khư"),
            ("giw", "giư"),
            ("phw", "phư"),
            ("ghw", "ghư"),
        ] {
            assert_eq!(telex(seq).0, want, "telex {seq:?}");
        }
        assert_eq!(telex("plw").0, "plw");
        assert_eq!(telex("stw").0, "stw");
        assert_eq!(telex("nghw").0, "nghw");
    }

    #[test]
    fn standalone_w_ignores_marks_already_applied() {
        assert_eq!(telex("eew").0, "êw");
    }

    #[test]
    fn a_second_w_undoes_a_standalone_one() {
        let r = process_telex("ww", &Config::default());
        assert_eq!(r.bare, "w");
        assert_eq!(r.literal.as_deref(), Some("w"));
        assert_eq!(r.case_mask.len(), 1);
    }

    #[test]
    fn a_second_w_still_restores_a_typed_uw() {
        let r = process_telex("uww", &Config::default());
        assert_eq!(r.bare, "uw");
        assert_eq!(r.literal.as_deref(), Some("uw"));
    }

    #[test]
    fn standalone_w_can_be_switched_off() {
        let plain = Config {
            standalone_w: false,
            ..Default::default()
        };
        assert_eq!(telex_with(&plain, "w"), "w");
        assert_eq!(telex_with(&plain, "thw"), "thw");
        assert_eq!(telex_with(&plain, "thuowng"), "thương");
    }

    #[test]
    fn basic_tones() {
        assert_eq!(telex("has"), ("ha".into(), Tone::Sharp));
        assert_eq!(telex("haf"), ("ha".into(), Tone::Grave));
        assert_eq!(telex("har"), ("ha".into(), Tone::Hook));
        assert_eq!(telex("hax"), ("ha".into(), Tone::Tilde));
        assert_eq!(telex("haj"), ("ha".into(), Tone::Dot));
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
        assert_eq!(telex("aaa").0, "aa");
        assert_eq!(telex("eee").0, "ee");
        assert_eq!(telex("ddd").0, "dd");
        assert_eq!(telex("oww").0, "ow");
    }

    #[test]
    fn horn_key_after_ua_horns_the_u() {
        assert_eq!(telex("truaw").0, "trưa");
        assert_eq!(telex("muaw").0, "mưa");
        assert_eq!(telex("chuaw").0, "chưa");
    }

    #[test]
    fn horn_key_after_qu_onset_still_gives_breve() {
        assert_eq!(telex("quawng").0, "quăng");
    }

    #[test]
    fn quick_telex_expands_a_doubled_consonant() {
        let quick = Config {
            quick_telex: true,
            ..Default::default()
        };
        for (seq, want) in [
            ("cc", "ch"),
            ("gg", "gi"),
            ("kk", "kh"),
            ("nn", "ng"),
            ("qq", "qu"),
            ("pp", "ph"),
            ("tt", "th"),
        ] {
            assert_eq!(telex_with(&quick, seq), want, "telex {seq:?}");
        }
    }

    #[test]
    fn quick_telex_is_not_limited_to_the_word_start() {
        let quick = Config {
            quick_telex: true,
            ..Default::default()
        };
        assert_eq!(telex_with(&quick, "acc"), "ach");
        assert_eq!(telex_with(&quick, "ccc"), "chc");
        assert_eq!(telex_with(&quick, "ngg"), "ngi");
    }

    #[test]
    fn quick_telex_is_off_by_default() {
        assert_eq!(telex("cc").0, "cc");
        assert_eq!(telex("tt").0, "tt");
        assert_eq!(telex("ngg").0, "ngg");
    }

    #[test]
    fn viet() {
        let (bare, tone) = telex("viet");
        assert_eq!(bare, "viet");
        assert_eq!(tone, Tone::Flat);
    }

    #[test]
    fn vief_t() {
        let (bare, tone) = telex("viets");
        assert_eq!(bare, "viet");
        assert_eq!(tone, Tone::Sharp);
    }
}
