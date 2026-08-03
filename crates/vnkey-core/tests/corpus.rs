//! End-to-end corpus: type a key sequence through the engine and check what
//! lands on screen.
//!
//! These are the cases that were broken before the composition rewrite — the
//! tone-placement families (gi/qu onsets, classic vs modern, ươ clusters) and
//! the auto-restore path that used to mangle English words ("jira" → "jỉa",
//! "student" → "ent").

use vnkey_core::{Config, Engine, InputMethod, Keystroke, TonePlacementMode};

fn typed(seq: &str, method: InputMethod, placement: TonePlacementMode) -> String {
    let mut engine = Engine::new(Config {
        method,
        placement,
        enabled: true,
        auto_restore: true,
    });
    for ch in seq.chars() {
        engine.process(Keystroke::char(ch));
    }
    engine.current_displayed()
}

fn telex(seq: &str) -> String {
    typed(seq, InputMethod::Telex, TonePlacementMode::Modern)
}

fn telex_classic(seq: &str) -> String {
    typed(seq, InputMethod::Telex, TonePlacementMode::Classic)
}

fn vni(seq: &str) -> String {
    typed(seq, InputMethod::Vni, TonePlacementMode::Modern)
}

#[test]
fn telex_diacritics_and_tones() {
    for (seq, want) in [
        ("as", "á"),
        ("haf", "hà"),
        ("aa", "â"),
        ("aw", "ă"),
        ("dd", "đ"),
        ("vieetj", "việt"),
        ("tieengs", "tiếng"),
        ("ddaaus", "đấu"),
        ("khoongr", "khổng"),
        ("muoons", "muốn"),
    ] {
        assert_eq!(telex(seq), want, "telex {seq:?}");
    }
}

#[test]
fn telex_horn_clusters() {
    // "uo" takes both horns at once; 'w' also applies to an earlier cluster.
    for (seq, want) in [("thuowngr", "thưởng"), ("nguoiwf", "người"), ("uw", "ư")] {
        assert_eq!(telex(seq), want, "telex {seq:?}");
    }
}

#[test]
fn gi_and_qu_are_onsets_not_nuclei() {
    for (seq, want) in [("gias", "giá"), ("quaf", "quà"), ("quyeens", "quyến")] {
        assert_eq!(telex(seq), want, "telex {seq:?}");
    }
    assert_eq!(vni("gia1"), "giá");
    assert_eq!(vni("qua2"), "quà");
}

#[test]
fn modern_and_classic_placement_differ() {
    assert_eq!(telex("hoaf"), "hòa");
    assert_eq!(telex_classic("hoaf"), "hoà");
    assert_eq!(telex("thuys"), "thúy");
    assert_eq!(telex_classic("thuys"), "thuý");
    // Closed syllables agree.
    assert_eq!(telex("toans"), "toán");
    assert_eq!(telex_classic("toans"), "toán");
}

#[test]
fn vni_diacritics_and_tones() {
    for (seq, want) in [
        ("a1", "á"),
        ("a2", "à"),
        ("a6", "â"),
        ("a8", "ă"),
        ("d9", "đ"),
        ("vie6t5", "việt"),
    ] {
        assert_eq!(vni(seq), want, "vni {seq:?}");
    }
}

/// The reported bug: typing an English word must give back that word.
///
/// Each of these used to be mangled — the engine gave up on the first
/// impossible letter but then *re-composed* the remainder, so "jira" came out
/// as "jỉa" with the 'r' eaten as a hỏi tone.
#[test]
fn english_words_are_returned_verbatim() {
    for word in [
        "jira", "student", "the", "file", "first", "result", "server", "java", "press", "sort",
        "fix", "text", "react", "script", "string", "user", "error", "start", "stop", "reset",
        "forest", "just",
    ] {
        assert_eq!(telex(word), word, "expected {word:?} to survive untouched");
    }
}

/// Words that are simultaneously valid Vietnamese syllables cannot be told
/// apart from Vietnamese without a dictionary, so they still convert. Pinned
/// here so the limitation is visible rather than surprising.
#[test]
fn known_ambiguous_english_words_still_convert() {
    for (word, becomes) in [("rust", "rút"), ("cost", "cót"), ("last", "lát"), ("test", "tét")] {
        assert_eq!(telex(word), becomes, "{word:?} is ambiguous with Vietnamese");
    }
}

#[test]
fn passthrough_resets_at_a_word_boundary() {
    // A foreign word must not poison the next word.
    let mut engine = Engine::new(Config {
        method: InputMethod::Telex,
        placement: TonePlacementMode::Modern,
        enabled: true,
        auto_restore: true,
    });
    for ch in "jira ".chars() {
        engine.process(Keystroke::char(ch));
    }
    for ch in "vieetj".chars() {
        engine.process(Keystroke::char(ch));
    }
    assert_eq!(engine.current_displayed(), "việt");
}

#[test]
fn case_is_preserved() {
    assert_eq!(telex("Vieetj"), "Việt");
    assert_eq!(telex("VIEETJ"), "VIỆT");
    assert_eq!(telex("Jira"), "Jira");
}
