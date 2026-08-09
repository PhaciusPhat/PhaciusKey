/// The six Vietnamese tones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tone {
    /// Ngang — level, no mark
    #[default]
    Flat,
    /// Sắc — acute ´
    Sharp,
    /// Huyền — grave `
    Grave,
    /// Hỏi — hook above
    Hook,
    /// Ngã — tilde ~
    Tilde,
    /// Nặng — dot below
    Dot,
}

/// Input method convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMethod {
    Telex,
    Vni,
}

/// Tone-mark placement strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TonePlacementMode {
    /// Standard modern Vietnamese orthography (default).
    Modern,
    /// Classic placement (e.g. "hoà" vs "hòa").
    Classic,
}

/// Engine configuration — owned by the shell, passed into every call.
#[derive(Debug, Clone)]
pub struct Config {
    pub method: InputMethod,
    pub placement: TonePlacementMode,
    /// When false the engine is a no-op and all keys pass through.
    pub enabled: bool,
    /// When false, invalid sequences are passed through literally (no diacritics forced).
    pub auto_restore: bool,
    /// Text-expansion macros: an on-screen word matching a key exactly
    /// (case-sensitive) is replaced with its value when the word commits.
    pub macros: std::collections::HashMap<String, String>,
    /// Telex: a `w` with no vowel to put a horn on types `ư` ("thw" → "thư",
    /// "w" → "ư"). Standard in Unikey and OpenKey, hence on by default; the
    /// key still types a literal `w` where `ư` cannot start a syllable.
    pub standalone_w: bool,
    /// Quick Telex: a doubled consonant at the start of a word expands to the
    /// digraph it stands for ("cc" → "ch", "nn" → "ng"). Off by default, as in
    /// OpenKey — it costs the ability to type a literal doubled consonant.
    pub quick_telex: bool,
    /// Quick start consonants: `f` → "ph", `j` → "gi", `w` → "qu" as the first
    /// letter of a word. Off by default: these letters are Telex tone and horn
    /// keys everywhere else, so the shortcut is a deliberate trade.
    pub quick_start_consonant: bool,
    /// Quick end consonants: after a vowel, a final `g` → "ng", `h` → "nh",
    /// `k` → "ch". Off by default for the same reason.
    pub quick_end_consonant: bool,
    /// Capitalize the first letter of a sentence as it is typed.
    pub auto_capitalize: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            method: InputMethod::Telex,
            placement: TonePlacementMode::Modern,
            enabled: true,
            auto_restore: true,
            macros: std::collections::HashMap::new(),
            standalone_w: true,
            quick_telex: false,
            quick_start_consonant: false,
            quick_end_consonant: false,
            auto_capitalize: false,
        }
    }
}

/// A single keystroke delivered to the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keystroke {
    /// The Unicode character produced by the key (after basic keyboard mapping).
    pub ch: char,
    /// True for space, punctuation, navigation keys, or a shell-supplied focus-change signal.
    pub is_boundary: bool,
}

impl Keystroke {
    pub fn char(ch: char) -> Self {
        Self { ch, is_boundary: false }
    }
    pub fn boundary() -> Self {
        Self { ch: ' ', is_boundary: true }
    }
}

/// Actions the shell must execute to keep the on-screen text in sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditAction {
    /// Delete this many characters to the left of the cursor.
    Backspace(u8),
    /// Insert this Unicode string at the cursor.
    Insert(String),
}
