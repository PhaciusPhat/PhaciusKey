use vnkey_core::validator::is_valid_syllable;

const VALID: &[&str] = &[
    "ba", "me", "bà", "mẹ", "hoa", "hoà", "hòa",
    "việt", "nam", "chào", "nghĩ", "quê", "đường",
    "thương", "trường", "ghi", "nghe", "người",
    "khi", "thi", "phi", "nhi",
    "an", "em", "in", "on", "un",
    "ăn", "ân", "êm", "ôm", "ơn", "ưng",
    "bàn", "con", "đen", "giờ", "học", "lớp",
    "mình", "nhà", "ông", "phố", "quán", "rừng",
    "sông", "tôi", "vui", "xanh",
    "gìn", "gin", // onset g + nucleus i + coda: greedy "gi" onset must backtrack
];

const INVALID: &[&str] = &[
    "test", "hello", "abc", "xzq", "bbb",
    "gha", "ngha", // gh/ngh + non-front vowel
    "pt", "st", "xk",
    "",
];

#[test]
fn valid_syllables() {
    for word in VALID {
        assert!(is_valid_syllable(word), "expected valid: {word}");
    }
}

#[test]
fn invalid_syllables() {
    for word in INVALID {
        assert!(!is_valid_syllable(word), "expected invalid: {word}");
    }
}
