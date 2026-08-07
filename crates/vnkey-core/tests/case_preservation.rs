//! Mid-word capitals must survive composition ("BaN" → "BaN", not "Ban").
//!
//! The methods lowercase letters for matching, so case is carried in a
//! per-character mask rather than reconstructed from patterns afterwards.

use vnkey_core::{Config, Engine, InputMethod, Keystroke};

fn displayed_after_with(method: InputMethod, s: &str) -> String {
    let mut e = Engine::new(Config { method, ..Default::default() });
    for ch in s.chars() {
        e.process(Keystroke::char(ch));
    }
    e.current_displayed()
}

fn displayed_after(s: &str) -> String {
    displayed_after_with(InputMethod::Telex, s)
}

// ── Mid-word capitals on words that stay valid Vietnamese ───────────────────

#[test]
fn midword_capital_is_kept() {
    assert_eq!(displayed_after("BaN"), "BaN");
    assert_eq!(displayed_after("TruNg"), "TruNg");
    assert_eq!(displayed_after("LaM"), "LaM");
}

#[test]
fn midword_capital_is_kept_in_transient_states() {
    // "NoT" is the state mid-way through typing "NoTa" — it must not flash
    // lowercase and get corrected later.
    assert_eq!(displayed_after("NoT"), "NoT");
}

#[test]
fn midword_capital_survives_a_tone_key() {
    // Tone lands on the vowel, the trailing capital stays capital.
    assert_eq!(displayed_after("BaNs"), "BáN");
    assert_eq!(displayed_after("LaMf"), "LàM");
}

// ── Case interaction with diacritic pairs ────────────────────────────────────

#[test]
fn diacritic_pair_keeps_the_first_keys_case() {
    // The doubling key only enriches the letter already on screen; its own
    // case is irrelevant.
    assert_eq!(displayed_after("Aa"), "Â");
    assert_eq!(displayed_after("aA"), "â");
    assert_eq!(displayed_after("Dda"), "Đa");
}

#[test]
fn cancelled_tone_key_keeps_its_typed_case() {
    // "HaSS": second 'S' undoes the tone and is typed literally — as 'S'.
    assert_eq!(displayed_after("HaSS"), "HaS");
}

// ── Existing conventions must keep working ───────────────────────────────────

#[test]
fn leading_capital_and_all_caps_still_work() {
    assert_eq!(displayed_after("Has"), "Há");
    assert_eq!(displayed_after("VIEETS"), "VIẾT");
    assert_eq!(displayed_after("vieets"), "viết");
}

// ── VNI ──────────────────────────────────────────────────────────────────────

#[test]
fn vni_midword_capital_is_kept() {
    assert_eq!(displayed_after_with(InputMethod::Vni, "BaN"), "BaN");
    assert_eq!(displayed_after_with(InputMethod::Vni, "BaN1"), "BáN");
}

#[test]
fn vni_all_caps_still_works() {
    assert_eq!(displayed_after_with(InputMethod::Vni, "VIET61"), "VIẾT");
}
