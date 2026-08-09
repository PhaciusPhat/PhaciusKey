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
        let mut actions = Vec::new();

        let common: usize = self
            .displayed
            .chars()
            .zip(target.chars())
            .take_while(|(a, b)| a == b)
            .count();

        let target_tail: String = target.chars().skip(common).collect();

        let to_delete = self.displayed.chars().count() - common;
        if to_delete > 0 {
            actions.push(EditAction::Backspace(to_delete as u8));
        }
        if !target_tail.is_empty() {
            actions.push(EditAction::Insert(target_tail));
        }

        self.displayed = target.to_string();
        actions
    }

    /// Actions to clear everything currently displayed, then reset.
    pub fn clear_actions(&mut self) -> Vec<EditAction> {
        let len = self.displayed.chars().count();
        let mut actions = Vec::new();
        if len > 0 {
            actions.push(EditAction::Backspace(len as u8));
        }
        self.reset();
        actions
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
