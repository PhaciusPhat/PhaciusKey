use vnkey_core::{Config, Engine, InputMethod, Keystroke, TonePlacementMode};

fn typed(seq: &str, method: InputMethod, placement: TonePlacementMode) -> String {
    let mut engine = Engine::new(Config {
        method,
        placement,
        enabled: true,
        auto_restore: true,
        ..Default::default()
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
fn g_plus_i_plus_coda_words_compose() {
    assert_eq!(telex("ginf"), "gìn");
    assert_eq!(vni("gin2"), "gìn");
    assert_eq!(telex("gias"), "giá");
    assert_eq!(telex("giuwx"), "giữ");
}

#[test]
fn modern_and_classic_placement_differ() {
    assert_eq!(telex("hoaf"), "hòa");
    assert_eq!(telex_classic("hoaf"), "hoà");
    assert_eq!(telex("thuys"), "thúy");
    assert_eq!(telex_classic("thuys"), "thuý");
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

#[test]
fn known_ambiguous_english_words_still_convert() {
    for (word, becomes) in [
        ("rust", "rút"),
        ("cost", "cót"),
        ("last", "lát"),
        ("test", "tét"),
    ] {
        assert_eq!(
            telex(word),
            becomes,
            "{word:?} is ambiguous with Vietnamese"
        );
    }
}

#[test]
fn tone_removal_key_with_nothing_to_remove_is_literal() {
    for word in ["z", "zoo", "zalo", "size", "haz"] {
        assert_eq!(telex(word), word, "expected {word:?} to survive untouched");
    }
    assert_eq!(telex("hasz"), "ha");
    assert_eq!(vni("ha0"), "ha0");
    assert_eq!(vni("ha10"), "ha");
}

#[test]
fn passthrough_resets_at_a_word_boundary() {
    let mut engine = Engine::new(Config {
        method: InputMethod::Telex,
        placement: TonePlacementMode::Modern,
        enabled: true,
        auto_restore: true,
        ..Default::default()
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

fn screen(seq: &str, backspaces_at_end: usize) -> String {
    let mut engine = Engine::new(Config {
        method: InputMethod::Telex,
        placement: TonePlacementMode::Modern,
        enabled: true,
        auto_restore: true,
        ..Default::default()
    });
    let mut out = String::new();

    let apply = |actions: Vec<vnkey_core::EditAction>, out: &mut String| {
        for action in actions {
            match action {
                vnkey_core::EditAction::Backspace(n) => {
                    for _ in 0..n {
                        out.pop();
                    }
                }
                vnkey_core::EditAction::Insert(text) => out.push_str(&text),
            }
        }
    };

    for ch in seq.chars() {
        let actions = engine.process(Keystroke::char(ch));
        if actions.is_empty() {
            out.push(ch);
        } else {
            apply(actions, &mut out);
        }
    }
    for _ in 0..backspaces_at_end {
        let actions = engine.backspace();
        if actions.is_empty() {
            out.pop();
        } else {
            apply(actions, &mut out);
        }
    }
    out
}

#[test]
fn backspace_deletes_a_letter_not_the_tone_mark() {
    assert_eq!(screen("ddoans", 0), "đoán");
    assert_eq!(screen("ddoans", 1), "đoá");
    assert_eq!(screen("ddoans", 2), "đo");
    assert_eq!(screen("ddoans", 3), "đ");
    assert_eq!(screen("ddoans", 4), "");
}

#[test]
fn backspace_on_a_composed_vowel_removes_the_whole_character() {
    assert_eq!(screen("tieengs", 0), "tiếng");
    assert_eq!(screen("tieengs", 1), "tiến");
    assert_eq!(screen("tieengs", 2), "tiế");
    assert_eq!(screen("as", 1), "");
}

#[test]
fn screen_text_matches_engine_for_plain_typing() {
    assert_eq!(screen("vieetj", 0), "việt");
    assert_eq!(screen("jira", 0), "jira");
    assert_eq!(screen("hoaf", 0), "hòa");
}

fn screen_vni(seq: &str) -> String {
    let mut engine = Engine::new(Config {
        method: InputMethod::Vni,
        placement: TonePlacementMode::Modern,
        enabled: true,
        auto_restore: true,
        ..Default::default()
    });
    let mut out = String::new();
    for ch in seq.chars() {
        let actions = engine.process(Keystroke::char(ch));
        if actions.is_empty() {
            out.push(ch);
            continue;
        }
        for action in actions {
            match action {
                vnkey_core::EditAction::Backspace(n) => {
                    for _ in 0..n {
                        out.pop();
                    }
                }
                vnkey_core::EditAction::Insert(text) => out.push_str(&text),
            }
        }
    }
    out
}

#[test]
fn repeating_a_key_undoes_it_and_types_it() {
    assert_eq!(screen_vni("d9oan1"), "đoán");
    assert_eq!(screen_vni("d9oan11"), "đoan1");
    assert_eq!(screen("aa", 0), "â");
    assert_eq!(screen("aaa", 0), "aa");
    assert_eq!(screen("eee", 0), "ee");
    assert_eq!(screen("ooo", 0), "oo");
    assert_eq!(screen("ddd", 0), "dd");
    assert_eq!(screen("oww", 0), "ow");
    assert_eq!(screen("ass", 0), "as");
    assert_eq!(screen_vni("a66"), "a6");
    assert_eq!(screen_vni("d99"), "d9");
}

#[test]
fn a_different_key_changes_rather_than_undoes() {
    assert_eq!(screen("asf", 0), "à");
    assert_eq!(screen_vni("a12"), "à");
}
