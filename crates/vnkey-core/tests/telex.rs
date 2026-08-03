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
    assert_eq!(displayed_after("haz"), "ha");
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
