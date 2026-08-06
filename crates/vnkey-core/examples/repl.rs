//! Interactive engine harness — type keystroke sequences, see what lands on
//! screen. Pure engine, no keyboard hook: needs **no Accessibility
//! permission** and works in any terminal (CI, SSH, containers).
//!
//! ```sh
//! cargo run -p vnkey-core --example repl
//! ```
//!
//! Each input line is typed through a fresh engine, character by character,
//! and the resulting on-screen text is printed. Spaces and punctuation act as
//! word boundaries exactly as they do live.
//!
//! Commands:
//!   :telex / :vni        switch input method
//!   :modern / :classic   switch tone placement
//!   :restore on|off      toggle auto-restore of non-Vietnamese words
//!   :quit                exit

use std::io::{self, BufRead, Write};

use vnkey_core::{Config, EditAction, Engine, InputMethod, Keystroke, TonePlacementMode};

fn main() {
    let mut config = Config::default();
    let stdin = io::stdin();

    println!("vnkey-core REPL — type a keystroke sequence, get the screen text.");
    println!("Commands: :telex :vni :modern :classic :restore on|off :quit");
    prompt(&config);

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        match line.trim() {
            "" => {}
            ":quit" | ":q" => break,
            ":telex" => config.method = InputMethod::Telex,
            ":vni" => config.method = InputMethod::Vni,
            ":modern" => config.placement = TonePlacementMode::Modern,
            ":classic" => config.placement = TonePlacementMode::Classic,
            ":restore on" => config.auto_restore = true,
            ":restore off" => config.auto_restore = false,
            cmd if cmd.starts_with(':') => println!("unknown command: {cmd}"),
            seq => println!("{seq:?} → {:?}", type_through(seq, &config)),
        }
        prompt(&config);
    }
}

fn prompt(config: &Config) {
    let method = match config.method {
        InputMethod::Telex => "telex",
        InputMethod::Vni => "vni",
    };
    let placement = match config.placement {
        TonePlacementMode::Modern => "modern",
        TonePlacementMode::Classic => "classic",
    };
    let restore = if config.auto_restore { "on" } else { "off" };
    print!("[{method}/{placement}/restore:{restore}] > ");
    let _ = io::stdout().flush();
}

/// Feed `seq` through a fresh engine and apply the edit actions to a screen
/// buffer, the way the platform layer applies them to the focused text field.
fn type_through(seq: &str, config: &Config) -> String {
    let mut engine = Engine::new(config.clone());
    let mut screen = String::new();

    for ch in seq.chars() {
        let actions = engine.process(Keystroke::char(ch));
        if actions.is_empty() {
            // Engine passed the key through; the app would print it itself.
            screen.push(ch);
            continue;
        }
        for action in actions {
            match action {
                EditAction::Backspace(n) => {
                    for _ in 0..n {
                        screen.pop();
                    }
                }
                EditAction::Insert(text) => screen.push_str(&text),
            }
        }
    }
    screen
}
