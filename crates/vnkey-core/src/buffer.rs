use crate::types::EditAction;

/// The raw keystrokes since the last word boundary, and the word currently displayed on-screen.
#[derive(Debug, Default, Clone)]
pub struct CompositionBuffer {
    pub raw: String,
    pub displayed: String,
}

impl CompositionBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, ch: char) {
        self.raw.push(ch);
    }

    /// Remove the last raw keystroke. `false` if the buffer was already empty.
    pub fn pop(&mut self) -> bool {
        self.raw.pop().is_some()
    }

    pub fn reset(&mut self) {
        self.raw.clear();
        self.displayed.clear();
    }

    /// Minimal edit actions from `self.displayed` to `target`, which then becomes `self.displayed`.
    pub fn diff_to(&mut self, target: &str) -> Vec<EditAction> {
        let mut common_bytes = 0;
        let mut common_chars = 0;
        for (a, b) in self.displayed.chars().zip(target.chars()) {
            if a != b {
                break;
            }
            common_bytes += a.len_utf8();
            common_chars += 1;
        }

        let mut actions = Vec::new();
        push_backspaces(&mut actions, self.displayed.chars().count() - common_chars);

        let tail = &target[common_bytes..];
        if !tail.is_empty() {
            actions.push(EditAction::Insert(tail.to_string()));
        }

        self.displayed.clear();
        self.displayed.push_str(target);
        actions
    }

    /// Actions to clear everything currently displayed, then reset.
    pub fn clear_actions(&mut self) -> Vec<EditAction> {
        let mut actions = Vec::new();
        push_backspaces(&mut actions, self.displayed.chars().count());
        self.reset();
        actions
    }
}

fn push_backspaces(actions: &mut Vec<EditAction>, mut count: usize) {
    while count > 0 {
        let chunk = count.min(usize::from(u8::MAX));
        actions.push(EditAction::Backspace(chunk as u8));
        count -= chunk;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_no_change() {
        let mut buf = CompositionBuffer {
            raw: String::new(),
            displayed: "ha".into(),
        };
        let actions = buf.diff_to("ha");
        assert!(actions.is_empty());
    }

    #[test]
    fn diff_extend() {
        let mut buf = CompositionBuffer {
            raw: String::new(),
            displayed: "h".into(),
        };
        let actions = buf.diff_to("hà");
        assert_eq!(actions, vec![EditAction::Insert("à".into())]);
    }

    #[test]
    fn a_diff_past_the_backspace_ceiling_still_deletes_every_character() {
        let mut buf = CompositionBuffer {
            raw: String::new(),
            displayed: "a".repeat(300),
        };
        let actions = buf.diff_to("b");
        let deleted: usize = actions
            .iter()
            .map(|a| match a {
                EditAction::Backspace(n) => usize::from(*n),
                EditAction::Insert(_) => 0,
            })
            .sum();
        assert_eq!(deleted, 300);
    }

    #[test]
    fn diff_replace() {
        let mut buf = CompositionBuffer {
            raw: String::new(),
            displayed: "ha".into(),
        };
        let actions = buf.diff_to("há");
        assert_eq!(
            actions,
            vec![EditAction::Backspace(1), EditAction::Insert("á".into())]
        );
    }
}
