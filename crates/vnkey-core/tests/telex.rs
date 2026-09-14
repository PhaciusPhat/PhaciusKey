use vnkey_core::{Config, Engine, InputMethod, Keystroke};

fn engine() -> Engine {
    Engine::new(Config {
        method: InputMethod::Telex,
        ..Default::default()
    })
}

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
        self.current_displayed()
    }
}

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
    assert_eq!(displayed_after("hasz"), "ha");
    assert_eq!(displayed_after("haz"), "haz");
}

#[test]
fn z_onset_takes_a_tone() {
    assert_eq!(displayed_after("zij"), "zị");
    assert_eq!(displayed_after("zas"), "zá");
}

#[test]
fn z_onset_takes_a_diacritic() {
    assert_eq!(displayed_after("zoo"), "zô");
    assert_eq!(displayed_after("zuw"), "zư");
}

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

#[test]
fn viet_sharp() {
    assert_eq!(displayed_after("viets"), "viét");
}

#[test]
fn viet_dot() {
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
    assert_eq!(displayed_after("nguwowif"), "người");
}

#[test]
fn triple_a_restores() {
    assert_eq!(displayed_after("aaa"), "aa");
}

#[test]
fn triple_e_restores() {
    assert_eq!(displayed_after("eee"), "ee");
}

fn displayed_with(config: Config, s: &str) -> String {
    let mut e = Engine::new(config);
    for ch in s.chars() {
        e.process(Keystroke::char(ch));
    }
    e.current_displayed()
}

#[test]
fn standalone_w_types_u_horn() {
    assert_eq!(displayed_after("w"), "ư");
    assert_eq!(displayed_after("thw"), "thư");
    assert_eq!(displayed_after("ngw"), "ngư");
    assert_eq!(displayed_after("chw"), "chư");
}

#[test]
fn standalone_w_stays_a_letter_where_u_horn_cannot_follow() {
    assert_eq!(displayed_after("kw"), "kw");
    assert_eq!(displayed_after("zw"), "zw");
    assert_eq!(displayed_after("nghw"), "nghw");
}

#[test]
fn a_second_w_gives_the_letter_back() {
    assert_eq!(displayed_after("ww"), "w");
    assert_eq!(displayed_after("www"), "ww");
}

#[test]
fn a_second_w_after_a_typed_uw_gives_both_keys_back() {
    assert_eq!(displayed_after("uww"), "uw");
    assert_eq!(displayed_after("oww"), "ow");
}

#[test]
fn standalone_w_keeps_the_typed_case() {
    assert_eq!(displayed_after("W"), "Ư");
    assert_eq!(displayed_after("Thw"), "Thư");
    assert_eq!(displayed_after("THW"), "THƯ");
    assert_eq!(displayed_after("Ww"), "w");
}

#[test]
fn standalone_w_can_be_switched_off() {
    let plain = Config {
        standalone_w: false,
        ..Default::default()
    };
    assert_eq!(displayed_with(plain.clone(), "w"), "w");
    assert_eq!(displayed_with(plain.clone(), "thw"), "thw");
    assert_eq!(displayed_with(plain, "thuowng"), "thương");
}

fn quick_telex_engine_config() -> Config {
    Config {
        quick_telex: true,
        ..Default::default()
    }
}

#[test]
fn quick_telex_expands_the_seven_doubled_consonants() {
    for (seq, want) in [
        ("cc", "ch"),
        ("gg", "gi"),
        ("kk", "kh"),
        ("nn", "ng"),
        ("qq", "qu"),
        ("pp", "ph"),
        ("tt", "th"),
    ] {
        assert_eq!(
            displayed_with(quick_telex_engine_config(), seq),
            want,
            "telex {seq:?}"
        );
    }
}

#[test]
fn quick_telex_keeps_the_case_of_the_letter_on_screen() {
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

#[test]
fn w_does_not_reach_past_a_vowel_it_cannot_mark() {
    assert_eq!(displayed_after("gatew"), "gatew");
    assert_eq!(displayed_after("tiew"), "tiew");
}

#[test]
fn a_second_w_gives_back_a_mark_made_before_the_coda() {
    assert_eq!(displayed_after("tanww"), "tanw");
    assert_eq!(displayed_after("quangww"), "quangw");
    assert_eq!(displayed_after("hoangww"), "hoangw");
}

#[test]
fn a_second_vowel_key_gives_back_a_mark_made_before_the_coda() {
    assert_eq!(displayed_after("loanaa"), "loana");
    assert_eq!(displayed_after("vieneee"), "vienee");
}

#[test]
fn marks_still_come_back_without_auto_restore() {
    let plain = Config {
        method: InputMethod::Telex,
        auto_restore: false,
        ..Default::default()
    };
    assert_eq!(displayed_with(plain.clone(), "gatew"), "gatew");
    assert_eq!(displayed_with(plain.clone(), "gateww"), "gateww");
    assert_eq!(displayed_with(plain, "tanww"), "tanw");
}

#[test]
fn uu_takes_the_horn_on_the_first_u() {
    assert_eq!(displayed_after("cuuws"), "cứu");
    assert_eq!(displayed_after("luuw"), "lưu");
    assert_eq!(displayed_after("nguuw"), "ngưu");
}

#[test]
fn a_horn_typed_after_the_vowel_cluster_still_lands_on_it() {
    assert_eq!(displayed_after("ruouwj"), "rượu");
    assert_eq!(displayed_after("nguoiwf"), "người");
}

#[test]
fn the_u_of_a_qu_onset_takes_no_horn() {
    assert_eq!(displayed_after("quow"), "quơ");
    assert_eq!(displayed_after("quowr"), "quở");
    assert_eq!(displayed_after("quawng"), "quăng");
}

#[test]
fn a_second_w_gives_back_a_mark_made_on_a_vowel_cluster() {
    assert_eq!(displayed_after("truaww"), "truaw");
    assert_eq!(displayed_after("thuoww"), "thuow");
    assert_eq!(displayed_after("cuuww"), "cuuw");
}

#[test]
fn a_horn_skips_a_glide_the_horn_cannot_mark() {
    assert_eq!(displayed_after("voiwf"), "vời");
    assert_eq!(displayed_after("toiw"), "tơi");
}
