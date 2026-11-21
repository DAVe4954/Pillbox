//! A one-line text box: the text and a caret, and the edits a keyboard makes to them.
//! Pillbox draws it; this only keeps the state right (caret counted in characters, never
//! splitting one).

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextEdit {
    pub text: String,
    /// Caret position in characters (0 = before the first one).
    pub caret: usize,
}

/// Keys a text box understands, already translated from virtual-key codes by the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKey {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    /// Ctrl+Backspace.
    DeleteWordBack,
    /// Ctrl+Left / Ctrl+Right.
    WordLeft,
    WordRight,
}

impl TextEdit {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            caret: text.chars().count(),
        }
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map_or(self.text.len(), |(i, _)| i)
    }

    /// The text before the caret (for measuring where to draw it).
    pub fn before_caret(&self) -> &str {
        &self.text[..self.byte_at(self.caret)]
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
    }

    /// Insert typed or pasted text at the caret. Line breaks and tabs become spaces, other
    /// control characters are dropped, and the box holds at most `max` characters.
    pub fn insert(&mut self, s: &str, max: usize) {
        let room = max.saturating_sub(self.len());
        let clean: String = s
            .chars()
            .map(|c| {
                if matches!(c, '\n' | '\r' | '\t') {
                    ' '
                } else {
                    c
                }
            })
            .filter(|c| !c.is_control())
            .take(room)
            .collect();
        if clean.is_empty() {
            return;
        }
        let at = self.byte_at(self.caret);
        self.text.insert_str(at, &clean);
        self.caret += clean.chars().count();
    }

    fn word_start(&self) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = self.caret.min(chars.len());
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    fn word_end(&self) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = self.caret.min(chars.len());
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        i
    }

    fn remove_range(&mut self, from: usize, to: usize) {
        let (a, b) = (self.byte_at(from), self.byte_at(to));
        self.text.replace_range(a..b, "");
        self.caret = from;
    }

    pub fn key(&mut self, key: EditKey) {
        let len = self.len();
        self.caret = self.caret.min(len);
        match key {
            EditKey::Backspace if self.caret > 0 => self.remove_range(self.caret - 1, self.caret),
            EditKey::Delete if self.caret < len => self.remove_range(self.caret, self.caret + 1),
            EditKey::DeleteWordBack => {
                let start = self.word_start();
                self.remove_range(start, self.caret);
            }
            EditKey::Left => self.caret = self.caret.saturating_sub(1),
            EditKey::Right => self.caret = (self.caret + 1).min(len),
            EditKey::Home => self.caret = 0,
            EditKey::End => self.caret = len,
            EditKey::WordLeft => self.caret = self.word_start(),
            EditKey::WordRight => self.caret = self.word_end(),
            EditKey::Backspace | EditKey::Delete => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_moving() {
        let mut t = TextEdit::default();
        t.insert("helo", 100);
        t.key(EditKey::Left);
        t.insert("l", 100);
        assert_eq!((t.text.as_str(), t.caret), ("hello", 4));
        t.key(EditKey::Home);
        t.insert(">", 100);
        t.key(EditKey::End);
        t.insert("!", 100);
        assert_eq!(t.text, ">hello!");
        assert_eq!(t.before_caret(), ">hello!");
    }

    #[test]
    fn deleting() {
        let mut t = TextEdit::new("abc");
        t.key(EditKey::Backspace);
        assert_eq!(t.text, "ab");
        t.key(EditKey::Home);
        t.key(EditKey::Backspace);
        assert_eq!(t.text, "ab", "backspace at the start does nothing");
        t.key(EditKey::Delete);
        assert_eq!((t.text.as_str(), t.caret), ("b", 0));
        t.key(EditKey::End);
        t.key(EditKey::Delete);
        assert_eq!(t.text, "b");
    }

    #[test]
    fn words() {
        let mut t = TextEdit::new("call mum  at 6");
        t.key(EditKey::DeleteWordBack);
        assert_eq!(t.text, "call mum  at ");
        t.key(EditKey::DeleteWordBack);
        assert_eq!(t.text, "call mum  ");
        t.key(EditKey::WordLeft);
        assert_eq!(t.caret, 5);
        t.key(EditKey::WordRight);
        assert_eq!(t.caret, 8);
    }

    #[test]
    fn multibyte_characters_are_never_split() {
        let mut t = TextEdit::new("café 🎵");
        t.key(EditKey::Backspace);
        assert_eq!(t.text, "café ");
        t.key(EditKey::Left);
        t.key(EditKey::Backspace);
        assert_eq!(t.text, "caf ");
        t.insert("é", 100);
        assert_eq!(t.before_caret(), "café");
    }

    #[test]
    fn pasting_is_cleaned_and_limited() {
        let mut t = TextEdit::default();
        t.insert("line one\r\nline\ttwo\u{7}", 100);
        assert_eq!(t.text, "line one  line two");
        let mut t = TextEdit::default();
        t.insert("abcdef", 4);
        assert_eq!(t.text, "abcd");
        t.insert("x", 4);
        assert_eq!(t.text, "abcd", "full box takes nothing more");
    }
}
