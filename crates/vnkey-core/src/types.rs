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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMethod {
    Telex,
    Vni,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TonePlacementMode {
    /// Standard modern Vietnamese orthography (default).
    Modern,
    /// Classic placement (e.g. "hoà" vs "hòa").
    Classic,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub method: InputMethod,
    pub placement: TonePlacementMode,
    /// When false the engine is a no-op and all keys pass through.
    pub enabled: bool,
    /// When false, invalid sequences are passed through literally.
    pub auto_restore: bool,
    /// An on-screen word matching a key exactly (case-sensitive) is replaced when it commits.
    pub macros: std::collections::HashMap<String, String>,
    /// Telex: a `w` with no vowel to put a horn on types `ư` ("thw" → "thư").
    pub standalone_w: bool,
    /// Quick Telex: a doubled consonant expands to the digraph it stands for ("cc" → "ch").
    pub quick_telex: bool,
    /// Quick start consonants: `f` → "ph", `j` → "gi", `w` → "qu" at the start of a word.
    pub quick_start_consonant: bool,
    /// Quick end consonants: after a vowel, `g` → "ng", `h` → "nh", `k` → "ch".
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
    /// The Unicode character produced by the key.
    pub ch: char,
    /// True for space, punctuation, navigation keys, or a focus-change signal.
    pub is_boundary: bool,
}

impl Keystroke {
    pub fn char(ch: char) -> Self {
        Self {
            ch,
            is_boundary: false,
        }
    }
    pub fn boundary() -> Self {
        Self {
            ch: ' ',
            is_boundary: true,
        }
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
