use vnkey_core::{Config, Engine, InputMethod, Keystroke};

fn engine() -> Engine {
    Engine::new(Config {
        method: InputMethod::Telex,
        ..Default::default()
    })
}

/// Type a word character by character and return what the engine has displayed after all chars.
fn displayed_after(s: &str) -> String {
    let mut e = engine();
    for ch in s.chars() {
        e.process(Keystroke::char(ch));
    }
    e.buffer_displayed()
}

trait EngineExt {
    fn buffer_displayed(&self) -> String;
}
impl EngineExt for Engine {
    fn buffer_displayed(&self) -> String {
        // Access via public method added to Engine for testing.
        self.current_displayed()
    }
}

// ── Tone tests ───────────────────────────────────────────────────────────────

#[test]
fn sharp_tone() {
    assert_eq!(displayed_after("has"), "há");
    assert_eq!(displayed_after("mes"), "mé");
    assert_eq!(displayed_after("bis"), "bí");
}

#[test]
fn grave_tone() {
    assert_eq!(displayed_after("haf"), "hà");
    assert_eq!(displayed_after("mef"), "mè");
}

#[test]
fn hook_tone() {
    assert_eq!(displayed_after("har"), "hả");
}

#[test]
fn tilde_tone() {
    assert_eq!(displayed_after("hax"), "hã");
}

#[test]
fn dot_tone() {
    assert_eq!(displayed_after("haj"), "hạ");
}

#[test]
fn flat_tone_z() {
    // z removes a tone that exists; with no tone it is the letter z.
    assert_eq!(displayed_after("hasz"), "ha");
    assert_eq!(displayed_after("haz"), "haz");
}

// ── Vowel diacritic tests ─────────────────────────────────────────────────────

#[test]
fn circumflex_a() {
    assert_eq!(displayed_after("haa"), "hâ");
}

#[test]
fn breve_a() {
    assert_eq!(displayed_after("haw"), "hă");
}

#[test]
fn circumflex_e() {
    assert_eq!(displayed_after("hee"), "hê");
}

#[test]
fn circumflex_o() {
    assert_eq!(displayed_after("hoo"), "hô");
}

#[test]
fn horn_o() {
    assert_eq!(displayed_after("how"), "hơ");
}

#[test]
fn horn_u() {
    assert_eq!(displayed_after("huw"), "hư");
}

#[test]
fn stroke_d() {
    assert_eq!(displayed_after("dda"), "đa");
}

// ── Combined diacritic + tone ──────────────────────────────────────────────────

#[test]
fn circumflex_a_sharp() {
    assert_eq!(displayed_after("haas"), "hấ");
}

#[test]
fn circumflex_a_grave() {
    assert_eq!(displayed_after("haaf"), "hầ");
}

#[test]
fn breve_a_sharp() {
    assert_eq!(displayed_after("haws"), "hắ");
}

#[test]
fn horn_o_dot() {
    assert_eq!(displayed_after("howj"), "hợ");
}

// ── Common words ──────────────────────────────────────────────────────────────

#[test]
fn viet_sharp() {
    // "viets" → "viét" (plain e + sắc; use "vieets" for ê)
    assert_eq!(displayed_after("viets"), "viét");
}

#[test]
fn viet_dot() {
    // "vieetj" → "việt" (ê from ee + nặng)
    assert_eq!(displayed_after("vieetj"), "việt");
}

#[test]
fn nam() {
    assert_eq!(displayed_after("namf"), "nàm");
}

#[test]
fn chao() {
    assert_eq!(displayed_after("chaof"), "chào");
}

#[test]
fn nguoi() {
    // nguwowif → người (ư from uw, ơ from ow, hỏi from r... wait ow→ơ, i at end)
    // "nguwowif" = ng+uw(→ư)+ow(→ơ)+i+f(→huyền) = "người" with huyền
    assert_eq!(displayed_after("nguwowif"), "người");
}

// ── Restore ───────────────────────────────────────────────────────────────────

#[test]
fn triple_a_restores() {
    // Pressing 'a' a third time undoes â and types itself: "aa".
    assert_eq!(displayed_after("aaa"), "aa");
}

#[test]
fn triple_e_restores() {
    assert_eq!(displayed_after("eee"), "ee");
}

// ── Standalone w ──────────────────────────────────────────────────────────────

/// Type into an engine built from `config`.
fn displayed_with(config: Config, s: &str) -> String {
    let mut e = Engine::new(config);
    for ch in s.chars() {
        e.process(Keystroke::char(ch));
    }
    e.current_displayed()
}

#[test]
fn standalone_w_types_u_horn() {
    // The Unikey/OpenKey shorthand: with no vowel to put a horn on, 'w' is 'ư'.
    assert_eq!(displayed_after("w"), "ư");
    assert_eq!(displayed_after("thw"), "thư");
    assert_eq!(displayed_after("ngw"), "ngư");
    assert_eq!(displayed_after("chw"), "chư");
}

#[test]
fn standalone_w_stays_a_letter_where_u_horn_cannot_follow() {
    // "kư"/"zư"/"nghư" are not syllables, so the word is handed back as typed.
    assert_eq!(displayed_after("kw"), "kw");
    assert_eq!(displayed_after("zw"), "zw");
    assert_eq!(displayed_after("nghw"), "nghw");
}

#[test]
fn a_second_w_gives_the_letter_back() {
    // "ww" is the escape hatch for typing a literal 'w' — one keypress made
    // 'ư', the next takes it back.
    assert_eq!(displayed_after("ww"), "w");
    // The 'ư' of "uw" was asked for by two keys, so it owes both of them back.
    assert_eq!(displayed_after("uww"), "uw");
}

#[test]
fn standalone_w_keeps_the_typed_case() {
    assert_eq!(displayed_after("W"), "Ư");
    assert_eq!(displayed_after("Thw"), "Thư");
    assert_eq!(displayed_after("THW"), "THƯ");
    // The surviving letter of an undo is the key that was just pressed, so it
    // is that keypress — not the earlier 'W' — whose case shows.
    assert_eq!(displayed_after("Ww"), "w");
}

#[test]
fn standalone_w_can_be_switched_off() {
    let plain = Config { standalone_w: false, ..Default::default() };
    assert_eq!(displayed_with(plain.clone(), "w"), "w");
    assert_eq!(displayed_with(plain.clone(), "thw"), "thw");
    // The horn keys proper are a separate rule and keep working.
    assert_eq!(displayed_with(plain, "thuowng"), "thương");
}

// ── Quick Telex ───────────────────────────────────────────────────────────────

fn quick_telex_engine_config() -> Config {
    Config { quick_telex: true, ..Default::default() }
}

#[test]
fn quick_telex_expands_the_seven_doubled_consonants() {
    for (seq, want) in [
        ("cc", "ch"), ("gg", "gi"), ("kk", "kh"), ("nn", "ng"),
        ("qq", "qu"), ("pp", "ph"), ("tt", "th"),
    ] {
        assert_eq!(displayed_with(quick_telex_engine_config(), seq), want, "telex {seq:?}");
    }
}

#[test]
fn quick_telex_keeps_the_case_of_the_letter_on_screen() {
    // The expansion appends to the letter already typed, so that letter — and
    // its capital — survives: "Cc" is "Ch", not "ch".
    assert_eq!(displayed_with(quick_telex_engine_config(), "Cc"), "Ch");
    assert_eq!(displayed_with(quick_telex_engine_config(), "CC"), "CH");
}

#[test]
fn quick_telex_is_off_by_default() {
    assert_eq!(displayed_after("cc"), "cc");
    assert_eq!(displayed_after("tt"), "tt");
}

#[test]
fn the_horn_rules_still_win_over_the_standalone_one() {
    // Every one of these has a vowel for the 'w' to mark, so the standalone
    // rule must never see the key.
    for (seq, want) in [
        ("truaw", "trưa"),
        ("muaw", "mưa"),
        ("chuaw", "chưa"),
        ("quawng", "quăng"),
        ("thuowng", "thương"),
        ("ruwou", "rươu"),
        ("nguoiw", "ngươi"),
    ] {
        assert_eq!(displayed_after(seq), want, "telex {seq:?}");
    }
}
