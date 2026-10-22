//! The settings' text fields: a number kept in a preference, and the search over the
//! shortcuts. One line, with the caret at the end, as in the palette's field; the IME's
//! side goes through the same helpers (offsets in UTF-16).

use std::ops::Range;

use super::FieldId;
use crate::palette::{byte_at, printable, utf16_len};
use crate::studio::Rect;

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub id: FieldId,
    /// What is typed.
    pub value: String,
    /// What the IME is still composing after it.
    pub marked: String,
}

impl Field {
    pub fn new(id: FieldId, value: String) -> Field {
        Field {
            id,
            value,
            marked: String::new(),
        }
    }

    /// What the field shows: the value, then what is being composed.
    pub fn text(&self) -> String {
        format!("{}{}", self.value, self.marked)
    }

    pub fn set(&mut self, value: String) {
        self.value = value;
        self.marked.clear();
    }

    pub fn backspace(&mut self) {
        self.value.pop();
    }

    /// A number field keeps only what a number is written with.
    fn keep(&self, text: &str) -> String {
        let text = printable(text);
        match self.id {
            FieldId::Pref(_) => text
                .chars()
                .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-'))
                .collect(),
            FieldId::Search | FieldId::MacroName | FieldId::MacroPick => text,
        }
    }

    pub fn text_for_range(&self, range: Range<usize>) -> (String, Range<usize>) {
        let text = self.text();
        let a = byte_at(&text, range.start);
        let b = byte_at(&text, range.end).max(a);
        let real = utf16_len(&text[..a])..utf16_len(&text[..b]);
        (text[a..b].to_string(), real)
    }

    pub fn selected_range(&self) -> Range<usize> {
        let n = utf16_len(&self.text());
        n..n
    }

    pub fn marked_range(&self) -> Option<Range<usize>> {
        (!self.marked.is_empty()).then(|| {
            let a = utf16_len(&self.value);
            a..a + utf16_len(&self.marked)
        })
    }

    fn target(&self, text: &str, range: Option<Range<usize>>) -> (usize, usize) {
        let r = range
            .or_else(|| self.marked_range())
            .unwrap_or_else(|| self.selected_range());
        let a = byte_at(text, r.start);
        (a, byte_at(text, r.end).max(a))
    }

    /// Text put in for good: a typed letter, or what the IME composed.
    pub fn replace_text(&mut self, range: Option<Range<usize>>, text: &str) {
        let full = self.text();
        let (a, b) = self.target(&full, range);
        self.value = format!("{}{}{}", &full[..a], self.keep(text), &full[b..]);
        self.marked.clear();
    }

    /// Text the IME is composing.
    pub fn mark_text(&mut self, range: Option<Range<usize>>, text: &str) {
        let full = self.text();
        let (a, b) = self.target(&full, range);
        self.value = format!("{}{}", &full[..a], &full[b..]);
        self.marked = printable(text);
    }

    pub fn unmark(&mut self) {
        if !self.marked.is_empty() {
            self.value = self.text();
            self.marked.clear();
        }
    }

    /// Where a range of the text is on the screen, for the IME's window.
    pub fn text_bounds(
        &self,
        r: Rect,
        range: Range<usize>,
        mut measure: impl FnMut(&str) -> f32,
    ) -> Rect {
        let text = self.text();
        let a = byte_at(&text, range.start);
        let b = byte_at(&text, range.end).max(a);
        let x0 = measure(&text[..a]);
        let x1 = measure(&text[..b]);
        Rect::new(r.x + x0, r.y + 4.0, (x1 - x0).max(1.5), r.h - 8.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_field_keeps_only_a_number() {
        let mut f = Field::new(FieldId::Pref("audio.record-offset".into()), "0".into());
        f.replace_text(None, "-1.5ms");
        assert_eq!(f.value, "0-1.5");
        let mut s = Field::new(FieldId::Search, String::new());
        s.mark_text(None, "さい");
        assert_eq!(s.text(), "さい");
        s.replace_text(None, "再生");
        assert_eq!(s.value, "再生");
        assert!(s.marked.is_empty());
    }
}
