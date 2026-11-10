//! The one-line text field every editor of text in the app is made of: the palette's
//! search, the track browser's search, the names of a version, an export, a track and a
//! macro, the pick lists' search and the settings' fields.
//!
//! It keeps what is typed, a caret, the other end of a selection, what the IME is still
//! composing, its own undo and redo, and how far the text is scrolled. Movement is by
//! grapheme cluster (a surrogate pair, a letter with its combining marks and an emoji
//! sequence are one step) or by word. The IME's side (`text_for_range`, `replace_text`,
//! `mark_text`, ...) counts in UTF-16, as the platform does. The mouse is turned into a
//! position with [`Field::index_at`], which measures with the function the caller paints
//! with, so a click lands where the text is drawn.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use gpui::Hsla;
use unicode_segmentation::UnicodeSegmentation;
use xirius_design::{FontSize, Radius, Weight};

use super::FieldId;
use crate::draw::{Align, Pen};
use crate::palette::{byte_at, printable, utf16_len};
use crate::studio::Rect;

/// How many steps of undo a field keeps.
const HISTORY: usize = 200;

/// What kind of edit was the last one: edits of one kind in a row are one step of undo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edit {
    Typing,
    Deleting,
}

#[derive(Clone, Debug, PartialEq)]
struct Snap {
    value: String,
    cursor: usize,
    anchor: usize,
}

/// What a key did to a field.
#[derive(Clone, Debug, PartialEq)]
pub enum Keyed {
    /// Not a key of the field's: the screen goes on with it.
    Pass,
    /// The field took it (it may have changed nothing, e.g. Left at the start).
    Used,
    /// Ctrl+C: this text goes to the clipboard.
    Copy(String),
    /// Ctrl+X: this text goes to the clipboard (and has left the field).
    Cut(String),
    /// Ctrl+V: the screen reads the clipboard and gives it to [`Field::paste`].
    Paste,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub id: FieldId,
    /// What is typed.
    pub value: String,
    /// What the IME is still composing, at the caret.
    pub marked: String,
    /// The caret, as a byte offset in `value`.
    cursor: usize,
    /// The other end of the selection.
    anchor: usize,
    /// Shared by the copies of a field a screen is drawn from, so the scroll outlasts them;
    /// so is the history, which a copy does not duplicate.
    scroll: Rc<Cell<f32>>,
    undo: Rc<Vec<Snap>>,
    redo: Rc<Vec<Snap>>,
    last: Option<Edit>,
}

/// How a field draws its text.
#[derive(Clone, Copy)]
pub struct Look<'a> {
    pub size: FontSize,
    pub weight: Weight,
    pub ink: Hsla,
    /// Shown, dimmed, while the field is empty.
    pub placeholder: &'a str,
    pub placeholder_ink: Hsla,
    pub focused: bool,
}

/// The band of a line of text inside `r` (a selection's face and the caret's height).
fn line_box(r: Rect, size: FontSize) -> (f32, f32) {
    let h = (size.px() * 1.5).min(r.h);
    (r.y + (r.h - h) / 2.0, h)
}

fn clamp(s: &str, at: usize) -> usize {
    let mut at = at.min(s.len());
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The grapheme boundary before `at` (0 at the start).
fn prev_boundary(s: &str, at: usize) -> usize {
    s[..at]
        .grapheme_indices(true)
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn next_boundary(s: &str, at: usize) -> usize {
    s[at..]
        .graphemes(true)
        .next()
        .map(|g| at + g.len())
        .unwrap_or(s.len())
}

fn is_word(segment: &str) -> bool {
    segment.chars().any(char::is_alphanumeric)
}

/// Where the word before `at` starts.
fn word_before(s: &str, at: usize) -> usize {
    s.split_word_bound_indices()
        .rfind(|(i, w)| *i < at && is_word(w))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Where the word at or after `at` ends.
fn word_after(s: &str, at: usize) -> usize {
    s.split_word_bound_indices()
        .find(|(i, w)| i + w.len() > at && is_word(w))
        .map(|(i, w)| i + w.len())
        .unwrap_or(s.len())
}

impl Field {
    pub fn new(id: FieldId, value: String) -> Field {
        let end = value.len();
        Field {
            id,
            value,
            marked: String::new(),
            cursor: end,
            anchor: end,
            scroll: Rc::new(Cell::new(0.0)),
            undo: Rc::new(Vec::new()),
            redo: Rc::new(Vec::new()),
            last: None,
        }
    }

    /// A field that starts with all of its text selected: a name about to be changed.
    pub fn selected(id: FieldId, value: String) -> Field {
        let mut f = Field::new(id, value);
        f.select_all();
        f
    }

    /// What the field shows: the value with what is composed at the caret.
    pub fn text(&self) -> String {
        let at = self.caret();
        format!("{}{}{}", &self.value[..at], self.marked, &self.value[at..])
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty() && self.marked.is_empty()
    }

    /// Replaces the text for good: the caret goes to the end, the history is forgotten.
    pub fn set(&mut self, value: String) {
        self.cursor = value.len();
        self.anchor = value.len();
        self.value = value;
        self.marked.clear();
        Rc::make_mut(&mut self.undo).clear();
        Rc::make_mut(&mut self.redo).clear();
        self.last = None;
        self.scroll.set(0.0);
    }

    /// Like [`Field::set`], with everything selected.
    pub fn set_selected(&mut self, value: String) {
        self.set(value);
        self.select_all();
    }

    /// The caret in `value` (a byte offset, always on a character boundary).
    pub fn caret(&self) -> usize {
        clamp(&self.value, self.cursor)
    }

    /// The selection in `value`, as byte offsets in order.
    pub fn span(&self) -> Range<usize> {
        let a = clamp(&self.value, self.cursor);
        let b = clamp(&self.value, self.anchor);
        a.min(b)..a.max(b)
    }

    pub fn has_selection(&self) -> bool {
        self.marked.is_empty() && !self.span().is_empty()
    }

    pub fn selected_text(&self) -> &str {
        &self.value[self.span()]
    }

    pub fn scroll(&self) -> f32 {
        self.scroll.get()
    }

    // --- editing ---------------------------------------------------------------

    fn snap(&self) -> Snap {
        Snap {
            value: self.value.clone(),
            cursor: self.caret(),
            anchor: clamp(&self.value, self.anchor),
        }
    }

    fn restore(&mut self, s: Snap) {
        self.value = s.value;
        self.cursor = s.cursor;
        self.anchor = s.anchor;
        self.marked.clear();
        self.last = None;
    }

    /// Keeps what is about to change for Undo. A run of one kind of edit is one step.
    fn record(&mut self, kind: Option<Edit>) {
        if kind.is_none() || kind != self.last {
            self.keep_step();
        }
        Rc::make_mut(&mut self.redo).clear();
        self.last = kind;
    }

    /// The text as it is now, kept for Undo.
    fn keep_step(&mut self) {
        let s = self.snap();
        let undo = Rc::make_mut(&mut self.undo);
        undo.push(s);
        if undo.len() > HISTORY {
            undo.remove(0);
        }
    }

    fn splice(&mut self, range: Range<usize>, text: &str) {
        self.value.replace_range(range.clone(), text);
        self.cursor = range.start + text.len();
        self.anchor = self.cursor;
    }

    /// What may be put in: no control characters, and a number field only what a number is
    /// written with.
    fn keep(&self, text: &str) -> String {
        let text = printable(text);
        match self.id {
            FieldId::Pref(_) => text
                .chars()
                .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-'))
                .collect(),
            FieldId::Search | FieldId::MacroName | FieldId::MacroPick | FieldId::Value => text,
        }
    }

    /// Folds what is composed into the value (the caret goes after it).
    fn commit_marked(&mut self) {
        if !self.marked.is_empty() {
            let at = self.caret();
            let marked = std::mem::take(&mut self.marked);
            self.value.insert_str(at, &marked);
            self.cursor = at + marked.len();
            self.anchor = self.cursor;
        }
    }

    /// A typed letter (or letters): they replace the selection.
    pub fn insert(&mut self, text: &str) -> bool {
        let text = self.keep(text);
        if text.is_empty() {
            return false;
        }
        self.commit_marked();
        let span = self.span();
        self.record((span.is_empty()).then_some(Edit::Typing));
        self.splice(span, &text);
        true
    }

    /// Text pasted: one step of undo, newlines become spaces.
    pub fn paste(&mut self, text: &str) -> bool {
        let text = self.keep(&text.replace(['\r', '\n'], " "));
        if text.is_empty() {
            return false;
        }
        self.commit_marked();
        let span = self.span();
        self.record(None);
        self.splice(span, &text);
        true
    }

    /// Backspace: the selection, or the cluster before the caret.
    pub fn backspace(&mut self) {
        self.erase(prev_boundary, true);
    }

    /// Delete: the selection, or the cluster after the caret.
    pub fn delete(&mut self) {
        self.erase(next_boundary, false);
    }

    /// Ctrl+Backspace: back to the start of the word.
    pub fn delete_word_back(&mut self) -> bool {
        self.erase_by(word_before, true, None)
    }

    /// Ctrl+Delete: on to the end of the word.
    pub fn delete_word_forward(&mut self) -> bool {
        self.erase_by(word_after, false, None)
    }

    fn erase(&mut self, to: fn(&str, usize) -> usize, back: bool) -> bool {
        self.erase_by(to, back, Some(Edit::Deleting))
    }

    fn erase_by(&mut self, to: fn(&str, usize) -> usize, back: bool, kind: Option<Edit>) -> bool {
        self.commit_marked();
        let span = self.span();
        let range = if !span.is_empty() {
            span
        } else {
            let at = self.caret();
            let other = to(&self.value, at);
            if other == at {
                return false;
            }
            if back { other..at } else { at..other }
        };
        let kind = if self.span().is_empty() { kind } else { None };
        self.record(kind);
        self.splice(range, "");
        true
    }

    /// Esc in a search: what is typed goes. False when there was nothing.
    pub fn clear(&mut self) -> bool {
        if self.is_empty() {
            return false;
        }
        self.record(None);
        self.value.clear();
        self.marked.clear();
        self.cursor = 0;
        self.anchor = 0;
        self.scroll.set(0.0);
        true
    }

    pub fn undo(&mut self) -> bool {
        self.commit_marked();
        let Some(s) = Rc::make_mut(&mut self.undo).pop() else {
            return false;
        };
        let now = self.snap();
        Rc::make_mut(&mut self.redo).push(now);
        self.restore(s);
        true
    }

    pub fn redo(&mut self) -> bool {
        self.commit_marked();
        let Some(s) = Rc::make_mut(&mut self.redo).pop() else {
            return false;
        };
        let now = self.snap();
        Rc::make_mut(&mut self.undo).push(now);
        self.restore(s);
        true
    }

    /// Ctrl+C: the selected text.
    pub fn copy(&self) -> Option<String> {
        self.has_selection()
            .then(|| self.selected_text().to_string())
    }

    /// Ctrl+X: the selected text, which leaves the field.
    pub fn cut(&mut self) -> Option<String> {
        let text = self.copy()?;
        self.record(None);
        let span = self.span();
        self.splice(span, "");
        Some(text)
    }

    // --- the caret ---------------------------------------------------------------

    fn move_to(&mut self, at: usize, extend: bool) {
        self.commit_marked();
        self.cursor = at;
        if !extend {
            self.anchor = at;
        }
        self.last = None;
    }

    /// Left: a cluster (or a word) back. With a selection and no Shift, it closes at the
    /// selection's start.
    pub fn left(&mut self, extend: bool, word: bool) {
        self.commit_marked();
        let span = self.span();
        if !extend && !span.is_empty() {
            return self.move_to(span.start, false);
        }
        let at = self.caret();
        let to = if word {
            word_before(&self.value, at)
        } else {
            prev_boundary(&self.value, at)
        };
        self.move_to(to, extend);
    }

    pub fn right(&mut self, extend: bool, word: bool) {
        self.commit_marked();
        let span = self.span();
        if !extend && !span.is_empty() {
            return self.move_to(span.end, false);
        }
        let at = self.caret();
        let to = if word {
            word_after(&self.value, at)
        } else {
            next_boundary(&self.value, at)
        };
        self.move_to(to, extend);
    }

    pub fn home(&mut self, extend: bool) {
        self.move_to(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        self.commit_marked();
        self.move_to(self.value.len(), extend);
    }

    pub fn select_all(&mut self) {
        self.commit_marked();
        self.anchor = 0;
        self.cursor = self.value.len();
        self.last = None;
    }

    /// The word around `at` (a byte offset in `value`), or the blank run it is in.
    pub fn word_at(&self, at: usize) -> Range<usize> {
        let at = clamp(&self.value, at);
        self.value
            .split_word_bound_indices()
            .find(|(i, w)| at >= *i && at < i + w.len())
            .map(|(i, w)| i..i + w.len())
            .unwrap_or(at..at)
    }

    /// A key that belongs to the field. `ctrl` is Ctrl, or Cmd on macOS.
    pub fn key(&mut self, key: &str, shift: bool, ctrl: bool) -> Keyed {
        match (key, ctrl) {
            ("left", _) => self.left(shift, ctrl),
            ("right", _) => self.right(shift, ctrl),
            ("home", _) => self.home(shift),
            ("end", _) => self.end(shift),
            ("backspace", false) => {
                self.backspace();
            }
            ("backspace", true) => {
                self.delete_word_back();
            }
            ("delete", false) => {
                self.delete();
            }
            ("delete", true) => {
                self.delete_word_forward();
            }
            ("a", true) => self.select_all(),
            ("z", true) if shift => {
                self.redo();
            }
            ("z", true) => {
                self.undo();
            }
            ("y", true) => {
                self.redo();
            }
            ("c", true) => {
                return self.copy().map_or(Keyed::Used, Keyed::Copy);
            }
            ("x", true) => {
                return self.cut().map_or(Keyed::Used, Keyed::Cut);
            }
            ("v", true) => return Keyed::Paste,
            _ => return Keyed::Pass,
        }
        Keyed::Used
    }

    // --- the mouse -----------------------------------------------------------------

    /// The offset in `value` nearest to `x`, counted from where the text starts when it is
    /// not scrolled (add [`Field::scroll`] to a position inside the field). `measure` is the
    /// width of a text as it is drawn.
    pub fn index_at(&self, x: f32, mut measure: impl FnMut(&str) -> f32) -> usize {
        if !self.marked.is_empty() {
            return self.caret();
        }
        let v = &self.value;
        let mut marks: Vec<usize> = v.grapheme_indices(true).map(|(i, _)| i).collect();
        marks.push(v.len());
        // The widths grow with the offset: find the last mark at or before x, then the nearer one.
        let (mut lo, mut hi) = (0, marks.len() - 1);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if measure(&v[..marks[mid]]) <= x {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let a = marks[lo];
        match marks.get(lo + 1) {
            Some(&b) if measure(&v[..b]) - x < x - measure(&v[..a]) => b,
            _ => a,
        }
    }

    /// A press at `at` (see [`Field::index_at`]); `clicks` counts presses in a row: one puts
    /// the caret there (Shift keeps the selection's other end), two select the word, three
    /// everything.
    pub fn press(&mut self, at: usize, clicks: usize, extend: bool) {
        self.commit_marked();
        let at = clamp(&self.value, at);
        match clicks {
            0 | 1 => self.move_to(at, extend),
            2 => {
                let w = self.word_at(at);
                self.anchor = w.start;
                self.cursor = w.end;
                self.last = None;
            }
            _ => self.select_all(),
        }
    }

    /// The mouse moves with the button down: the selection reaches `at`.
    pub fn drag_to(&mut self, at: usize) {
        self.cursor = clamp(&self.value, at);
        self.last = None;
    }

    // --- scrolling and painting ---------------------------------------------------

    /// How wide the text is up to the caret, and so where the caret is when not scrolled.
    fn caret_x(&self, mut measure: impl FnMut(&str) -> f32) -> f32 {
        let at = self.caret() + self.marked.len();
        measure(&self.text()[..at])
    }

    /// Scrolls just enough to keep the caret inside a field `w` wide.
    pub fn reveal(&self, w: f32, mut measure: impl FnMut(&str) -> f32) {
        const MARGIN: f32 = 2.0;
        let x = self.caret_x(&mut measure);
        let total = measure(&self.text());
        let mut off = self.scroll.get();
        if x - off > w - MARGIN {
            off = x - w + MARGIN;
        }
        if x < off {
            off = x;
        }
        off = off.min((total - w + MARGIN).max(0.0)).max(0.0);
        self.scroll.set(off);
    }

    /// Draws the text in `r`: the selection on its own face, the composition underlined, the
    /// caret while `focused`, and the placeholder while there is nothing.
    pub fn paint(&self, pen: &mut Pen, r: Rect, look: Look) {
        let c = pen.c;
        let (size, weight) = (look.size, look.weight);
        if self.is_empty() {
            pen.text(
                look.placeholder,
                size,
                weight,
                look.placeholder_ink,
                r,
                Align::Left,
            );
            if look.focused {
                self.caret_bar(pen, r, size, r.x);
            }
            return;
        }
        let full = self.text();
        self.reveal(r.w, |s| pen.measure(s, size, weight));
        let off = self.scroll.get();
        let total = pen.measure(&full, size, weight);
        let origin = r.x - off;
        let line = Rect::new(origin, r.y, total + 16.0, r.h);
        pen.mask(r, |pen| {
            pen.text(&full, size, weight, look.ink, line, Align::Left);
            if self.has_selection() {
                let span = self.span();
                let x0 = pen.measure(&full[..span.start], size, weight);
                let x1 = pen.measure(&full[..span.end], size, weight);
                let (y, h) = line_box(r, size);
                let face = Rect::new(origin + x0, y, x1 - x0, h);
                pen.round(face, c.text_sel.0, Radius::Xs);
                pen.mask(face, |pen| {
                    pen.text(&full, size, weight, c.text_sel.1, line, Align::Left);
                });
            }
            let at = self.caret();
            let marked = !self.marked.is_empty();
            if marked {
                let x0 = pen.measure(&full[..at], size, weight);
                let x1 = pen.measure(&full[..at + self.marked.len()], size, weight);
                let y = r.y + r.h / 2.0 + size.px() * 0.6;
                pen.fill(Rect::new(origin + x0, y, x1 - x0, 1.0), look.ink);
            }
            if look.focused && !self.has_selection() {
                let x = pen.measure(&full[..at + self.marked.len()], size, weight);
                self.caret_bar(pen, r, size, origin + x);
            }
        });
    }

    fn caret_bar(&self, pen: &mut Pen, r: Rect, size: FontSize, x: f32) {
        let c = pen.c;
        let (y, h) = line_box(r, size);
        pen.fill(Rect::new(x, y, 1.5, h), c.ink_strong);
    }

    // --- the IME (offsets in UTF-16, over what the field shows) ------------------------

    pub fn text_for_range(&self, range: Range<usize>) -> (String, Range<usize>) {
        let text = self.text();
        let a = byte_at(&text, range.start);
        let b = byte_at(&text, range.end).max(a);
        let real = utf16_len(&text[..a])..utf16_len(&text[..b]);
        (text[a..b].to_string(), real)
    }

    /// The selection; while composing, the caret at the end of what is composed.
    pub fn selected_range(&self) -> Range<usize> {
        if !self.marked.is_empty() {
            let n = utf16_len(&self.value[..self.caret()]) + utf16_len(&self.marked);
            return n..n;
        }
        let s = self.span();
        utf16_len(&self.value[..s.start])..utf16_len(&self.value[..s.end])
    }

    pub fn marked_range(&self) -> Option<Range<usize>> {
        (!self.marked.is_empty()).then(|| {
            let a = utf16_len(&self.value[..self.caret()]);
            a..a + utf16_len(&self.marked)
        })
    }

    /// The part of the shown text an IME call is about, as byte offsets in it: the range
    /// given, else what is composed, else the selection.
    fn target(&self, text: &str, range: Option<Range<usize>>) -> Range<usize> {
        let r = range
            .or_else(|| self.marked_range())
            .unwrap_or_else(|| self.selected_range());
        let a = byte_at(text, r.start);
        a..byte_at(text, r.end).max(a)
    }

    /// Text put in for good: a typed letter, or what the IME composed.
    pub fn replace_text(&mut self, range: Option<Range<usize>>, text: &str) {
        let text = self.keep(text);
        let full = self.text();
        let t = self.target(&full, range);
        let had_marked = !self.marked.is_empty();
        if text.is_empty() && t.is_empty() && !had_marked {
            return;
        }
        let whole = format!("{}{}{}", &full[..t.start], text, &full[t.end..]);
        let end = t.start + text.len();
        // What is composed was kept when the composition began; the commit is part of it.
        if !had_marked {
            let kind = (t.is_empty()).then_some(Edit::Typing);
            if kind.is_none() || kind != self.last {
                self.keep_step();
            }
            self.last = kind;
        } else {
            self.last = None;
        }
        Rc::make_mut(&mut self.redo).clear();
        self.marked.clear();
        self.value = whole;
        self.cursor = end;
        self.anchor = end;
    }

    /// Text the IME is composing (it takes the place of the selection or of the earlier
    /// composition).
    pub fn mark_text(&mut self, range: Option<Range<usize>>, text: &str) {
        let marked = printable(text);
        let full = self.text();
        let t = self.target(&full, range);
        if self.marked.is_empty() {
            self.keep_step();
            Rc::make_mut(&mut self.redo).clear();
        }
        self.value = format!("{}{}", &full[..t.start], &full[t.end..]);
        self.cursor = t.start;
        self.anchor = t.start;
        self.marked = marked;
        self.last = None;
        if self.marked.is_empty() {
            Rc::make_mut(&mut self.undo).pop();
        }
    }

    pub fn unmark(&mut self) {
        self.commit_marked();
    }

    /// Where a range of the text is on the screen, for the IME's window: it starts where the
    /// composition starts and sits at the text's height, so the window opens below it.
    pub fn text_bounds(
        &self,
        r: Rect,
        range: Range<usize>,
        mut measure: impl FnMut(&str) -> f32,
    ) -> Rect {
        let text = self.text();
        let a = byte_at(&text, range.start);
        let b = byte_at(&text, range.end).max(a);
        let from = match self.marked_range() {
            Some(m) => byte_at(&text, m.start).min(a),
            None => a,
        };
        let x0 = measure(&text[..from]) - self.scroll.get();
        let x1 = measure(&text[..b]) - self.scroll.get();
        Rect::new(r.x + x0, r.y + 4.0, (x1 - x0).max(1.5), r.h - 8.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(text: &str) -> Field {
        Field::new(FieldId::Search, text.into())
    }

    /// Ten pixels a character, twenty for the full-width ones.
    fn wide(s: &str) -> f32 {
        s.chars()
            .map(|c| if (c as u32) > 0x2e80 { 20.0 } else { 10.0 })
            .sum()
    }

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

    #[test]
    fn typing_goes_at_the_caret_and_the_caret_moves_by_a_cluster() {
        let mut f = search("ac");
        f.left(false, false);
        f.insert("b");
        assert_eq!(f.value, "abc");
        assert_eq!(f.caret(), 2);
        f.right(false, false);
        f.right(false, false);
        assert_eq!(f.caret(), 3);
        f.home(false);
        f.delete();
        assert_eq!(f.value, "bc");
    }

    #[test]
    fn the_caret_never_stops_inside_a_surrogate_pair_or_a_combining_mark() {
        // U+1F3B5 is a surrogate pair in UTF-16; "e" and a combining acute are one cluster.
        let mut f = search("a\u{1F3B5}e\u{301}z");
        f.left(false, false);
        f.left(false, false);
        assert_eq!(f.caret(), "a\u{1F3B5}".len());
        f.left(false, false);
        assert_eq!(f.caret(), 1);
        f.right(false, false);
        f.right(false, false);
        assert_eq!(f.caret(), "a\u{1F3B5}e\u{301}".len());
        // Backspace takes the whole cluster.
        f.backspace();
        assert_eq!(f.value, "a\u{1F3B5}z");
        f.backspace();
        assert_eq!(f.value, "az");
    }

    #[test]
    fn an_emoji_sequence_and_a_flag_are_one_step() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        let mut f = search(&format!("{family}\u{1F1EF}\u{1F1F5}"));
        f.left(false, false);
        assert_eq!(f.caret(), family.len());
        f.left(false, false);
        assert_eq!(f.caret(), 0);
    }

    #[test]
    fn full_width_text_is_one_step_a_character() {
        let mut f = search("再生位置");
        f.left(false, false);
        assert_eq!(f.caret(), "再生位".len());
        f.backspace();
        assert_eq!(f.value, "再生置");
    }

    #[test]
    fn selecting_with_shift_and_typing_over_it() {
        let mut f = search("hello world");
        f.home(false);
        f.right(true, false);
        f.right(true, false);
        assert_eq!(f.selected_text(), "he");
        f.insert("J");
        assert_eq!(f.value, "Jllo world");
        f.select_all();
        assert_eq!(f.selected_text(), "Jllo world");
        f.backspace();
        assert!(f.value.is_empty());
    }

    #[test]
    fn left_and_right_with_a_selection_close_it_at_its_edge() {
        let mut f = search("abcdef");
        f.home(false);
        f.right(false, false);
        f.right(true, false);
        f.right(true, false);
        assert_eq!(f.span(), 1..3);
        f.left(false, false);
        assert_eq!((f.caret(), f.has_selection()), (1, false));
        f.right(true, false);
        f.right(true, false);
        f.right(false, false);
        assert_eq!((f.caret(), f.has_selection()), (3, false));
    }

    #[test]
    fn words_are_the_unit_of_ctrl_movement_and_ctrl_delete() {
        let mut f = search("one two  three");
        f.left(false, true);
        assert_eq!(f.caret(), "one two  ".len());
        f.left(false, true);
        assert_eq!(f.caret(), "one ".len());
        f.right(false, true);
        assert_eq!(f.caret(), "one two".len());
        f.delete_word_back();
        assert_eq!(f.value, "one   three");
        f.end(false);
        f.delete_word_back();
        assert_eq!(f.value, "one   ");
        f.home(false);
        f.delete_word_forward();
        assert_eq!(f.value, "   ");
    }

    #[test]
    fn undo_and_redo_are_the_fields_own_and_typing_runs_are_one_step() {
        let mut f = search("");
        for ch in ["a", "b", "c"] {
            f.insert(ch);
        }
        f.backspace();
        f.backspace();
        assert_eq!(f.value, "a");
        assert!(f.undo());
        assert_eq!(f.value, "abc");
        assert!(f.undo());
        assert_eq!(f.value, "");
        assert!(!f.undo());
        assert!(f.redo());
        assert_eq!(f.value, "abc");
        assert!(f.redo());
        assert_eq!(f.value, "a");
        assert!(!f.redo());
        // A new edit forgets what could be redone.
        f.undo();
        f.insert("z");
        assert!(!f.redo());
    }

    #[test]
    fn a_caret_move_ends_a_typing_run_for_undo() {
        let mut f = search("");
        f.insert("a");
        f.insert("b");
        f.left(false, false);
        f.insert("x");
        assert_eq!(f.value, "axb");
        f.undo();
        assert_eq!(f.value, "ab");
        f.undo();
        assert_eq!(f.value, "");
    }

    #[test]
    fn copy_cut_and_paste() {
        let mut f = search("alpha beta");
        assert_eq!(f.copy(), None);
        f.home(false);
        f.right(true, true);
        assert_eq!(f.copy().as_deref(), Some("alpha"));
        assert_eq!(f.cut().as_deref(), Some("alpha"));
        assert_eq!(f.value, " beta");
        f.end(false);
        assert!(f.paste("one\r\ntwo"));
        assert_eq!(f.value, " betaone  two");
        assert!(f.undo());
        assert_eq!(f.value, " beta");
        assert!(f.undo());
        assert_eq!(f.value, "alpha beta");
    }

    #[test]
    fn a_number_field_pastes_only_a_number() {
        let mut f = Field::new(FieldId::Pref("x".into()), String::new());
        f.paste("12ab.5");
        assert_eq!(f.value, "12.5");
    }

    #[test]
    fn keys_go_to_the_field_first() {
        let mut f = search("abc");
        assert_eq!(f.key("a", false, true), Keyed::Used);
        assert_eq!(f.selected_text(), "abc");
        assert_eq!(f.key("c", false, true), Keyed::Copy("abc".into()));
        assert_eq!(f.key("x", false, true), Keyed::Cut("abc".into()));
        assert!(f.value.is_empty());
        assert_eq!(f.key("z", false, true), Keyed::Used);
        assert_eq!(f.value, "abc");
        assert_eq!(f.key("z", true, true), Keyed::Used);
        assert!(f.value.is_empty());
        assert_eq!(f.key("z", false, true), Keyed::Used);
        assert_eq!(f.value, "abc");
        assert_eq!(f.key("y", false, true), Keyed::Used);
        assert!(f.value.is_empty());
        f.key("z", false, true);
        assert_eq!(f.key("v", false, true), Keyed::Paste);
        assert_eq!(f.key("enter", false, false), Keyed::Pass);
        assert_eq!(f.key("up", false, false), Keyed::Pass);
        assert_eq!(f.key("q", false, false), Keyed::Pass);
    }

    #[test]
    fn clear_is_one_undoable_step() {
        let mut f = search("abc");
        assert!(f.clear());
        assert!(!f.clear());
        assert!(f.undo());
        assert_eq!(f.value, "abc");
    }

    #[test]
    fn a_name_about_to_change_starts_fully_selected() {
        let f = Field::selected(FieldId::MacroName, "Kick".into());
        assert_eq!(f.selected_text(), "Kick");
        assert_eq!(f.selected_range(), 0..4);
    }

    #[test]
    fn utf16_ranges_count_surrogates_as_two() {
        let mut f = search("a\u{1F3B5}b");
        assert_eq!(f.selected_range(), 4..4);
        f.left(false, false);
        assert_eq!(f.selected_range(), 3..3);
        f.left(true, false);
        assert_eq!(f.selected_range(), 1..3);
        let (text, real) = f.text_for_range(0..10);
        assert_eq!((text.as_str(), real), ("a\u{1F3B5}b", 0..4));
        // A range that falls inside a pair rounds to the pair's end.
        let (text, real) = f.text_for_range(2..3);
        assert_eq!((text.as_str(), real), ("", 3..3));
    }

    #[test]
    fn a_composition_sits_at_the_caret_and_its_range_is_in_utf16() {
        let mut f = search("ab");
        f.left(false, false);
        f.mark_text(None, "さい");
        assert_eq!(f.text(), "aさいb");
        assert_eq!(f.marked_range(), Some(1..3));
        assert_eq!(f.selected_range(), 3..3);
        // The IME may change the composition by naming its range.
        f.mark_text(Some(1..3), "最\u{1F3B5}");
        assert_eq!(f.text(), "a最\u{1F3B5}b");
        assert_eq!(f.marked_range(), Some(1..4));
        f.replace_text(Some(1..4), "再生");
        assert_eq!(f.value, "a再生b");
        assert!(f.marked.is_empty());
        assert_eq!(f.selected_range(), 3..3);
    }

    #[test]
    fn a_composition_replaces_the_selection() {
        let mut f = search("hello");
        f.select_all();
        f.mark_text(None, "あ");
        assert_eq!(f.text(), "あ");
        f.replace_text(None, "亜");
        assert_eq!(f.value, "亜");
        // Undo brings back the selected text, in one step.
        f.undo();
        assert_eq!(f.value, "hello");
    }

    #[test]
    fn an_empty_composition_leaves_no_step_of_undo_behind() {
        let mut f = search("a");
        f.mark_text(None, "あ");
        f.mark_text(None, "");
        assert_eq!(f.value, "a");
        assert!(f.marked.is_empty());
        assert!(!f.undo());
    }

    #[test]
    fn a_key_during_composition_keeps_what_is_composed() {
        let mut f = search("ab");
        f.mark_text(None, "か");
        f.left(false, false);
        assert_eq!(f.value, "abか");
        assert_eq!(f.caret(), "ab".len());
    }

    #[test]
    fn the_mouse_lands_on_the_nearest_cluster() {
        let f = search("abcd");
        assert_eq!(f.index_at(-5.0, wide), 0);
        assert_eq!(f.index_at(4.0, wide), 0);
        assert_eq!(f.index_at(6.0, wide), 1);
        assert_eq!(f.index_at(24.0, wide), 2);
        assert_eq!(f.index_at(26.0, wide), 3);
        assert_eq!(f.index_at(400.0, wide), 4);
        let f = search("あい");
        assert_eq!(f.index_at(9.0, wide), 0);
        assert_eq!(f.index_at(11.0, wide), "あ".len());
        assert_eq!(f.index_at(31.0, wide), "あい".len());
        let f = search("a\u{1F3B5}e\u{301}");
        // 10 + 20 + 10 + 10 would be the chars' widths; the stops are the clusters'.
        let stops: Vec<usize> = (0..60)
            .map(|x| f.index_at(x as f32, wide))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(stops, vec![0, 1, "a\u{1F3B5}".len(), f.value.len()]);
        assert_eq!(search("").index_at(5.0, wide), 0);
    }

    #[test]
    fn click_drag_double_and_triple_click() {
        let mut f = search("one two three");
        f.press(5, 1, false);
        assert_eq!((f.caret(), f.has_selection()), (5, false));
        f.drag_to(10);
        assert_eq!(f.span(), 5..10);
        f.press(5, 2, false);
        assert_eq!(f.selected_text(), "two");
        f.press(5, 3, false);
        assert_eq!(f.selected_text(), "one two three");
        f.press(2, 1, false);
        f.press(6, 1, true);
        assert_eq!(f.span(), 2..6);
    }

    #[test]
    fn double_click_on_full_width_text_takes_the_word() {
        let mut f = search("play 再生 now");
        f.press("play 再".len() - 1, 2, false);
        assert!(!f.selected_text().is_empty());
        assert!("play 再生 now".contains(f.selected_text()));
    }

    #[test]
    fn the_text_scrolls_to_keep_the_caret_in_view() {
        let f = search("0123456789abcdefghij");
        // 200 wide text in a 100 wide field, caret at the end.
        f.reveal(100.0, wide);
        assert_eq!(f.scroll(), 102.0);
        let mut f = f;
        f.home(false);
        f.reveal(100.0, wide);
        assert_eq!(f.scroll(), 0.0);
        f.press(7, 1, false);
        f.reveal(100.0, wide);
        assert_eq!(f.scroll(), 0.0);
        f.end(false);
        f.reveal(100.0, wide);
        f.left(false, false);
        f.reveal(100.0, wide);
        assert_eq!(f.scroll(), 102.0);
        for _ in 0..12 {
            f.left(false, false);
        }
        f.reveal(100.0, wide);
        assert_eq!(f.scroll(), 70.0);
    }

    #[test]
    fn the_ime_window_opens_below_the_start_of_the_composition() {
        let mut f = search("ab");
        f.mark_text(None, "さい");
        let r = Rect::new(100.0, 10.0, 200.0, 28.0);
        let full = f.marked_range().unwrap();
        let b = f.text_bounds(r, full.clone(), wide);
        assert_eq!(b.x, 100.0 + 20.0);
        assert_eq!(b.w, 40.0);
        // A range inside the composition still starts at its head.
        let part = f.text_bounds(r, 3..4, wide);
        assert_eq!(part.x, b.x);
        assert!(b.y + b.h <= r.y + r.h);
    }

    #[test]
    fn a_value_set_from_outside_puts_the_caret_at_the_end_and_forgets_history() {
        let mut f = search("a");
        f.insert("b");
        f.set("xyz".into());
        assert_eq!(f.caret(), 3);
        assert!(!f.undo());
        f.value = "q".into();
        assert_eq!(f.caret(), 1);
        f.insert("r");
        assert_eq!(f.value, "qr");
    }
}
