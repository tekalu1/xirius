//! The command palette (UI-087, OPS-005, PRJ-020, CTX-001): every operation
//! by its name, as in § 05 of the mock (`#palVeil`).
//!
//! The rows are read from core's one registry, the list the CLI and MCP use
//! too, and a row's name is the registry's short name of the command
//! ([`CommandDef::label`]); there is no list of the palette's own. A row runs
//! the way its key or menu does ([`Way`]), so one undo takes it back as it
//! would after the key. Values are entered in the same palette, using the
//! registry's argument definitions and validation.
//!
//! The search field takes its text from the window's IME (the screen hands
//! [`Palette::replace_text`] and [`Palette::mark_text`] to GPUI), so Japanese
//! is composed in place; the keys only move, run and close.

use std::ops::Range;

use gpui::{Bounds, Corners, point, px, size};
use serde_json::{Map, Value, json};
use xirius_core::command::{ArgType, CommandDef, Handler, Risk};
use xirius_core::error::ErrorCode;
use xirius_core::id::{IdKind, Node};
use xirius_core::model::ClipBody;
use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::popup::Action;
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tools::Mods;
use crate::topbar::BarButton;
use crate::tree::RowKind;

/// Where a key acts: the whole window, or one part of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    /// The piano roll in the drawer.
    Roll,
    /// The arrangement (it has the focus).
    Arrange,
    /// The transport, while no pop-over is open.
    Transport,
}

/// Which selection an operation of the palette needs. Keys come from Settings.
pub const SCOPES: &[(&str, Scope)] = &[
    ("history.undo", Scope::Global),
    ("history.redo", Scope::Global),
    ("project.save", Scope::Global),
    ("project.save-as", Scope::Global),
    ("project.open", Scope::Global),
    ("edit.copy", Scope::Global),
    ("edit.cut", Scope::Global),
    ("edit.paste", Scope::Global),
    ("select.all", Scope::Global),
    ("select.clear", Scope::Global),
    ("select.invert", Scope::Global),
    ("select.on-tracks", Scope::Global),
    ("select.in-loop", Scope::Global),
    ("note.delete", Scope::Roll),
    ("note.transpose", Scope::Roll),
    ("note.move", Scope::Roll),
    ("note.quantize", Scope::Roll),
    ("note.duplicate", Scope::Roll),
    ("note.mute", Scope::Roll),
    ("clip.duplicate", Scope::Arrange),
    ("clip.delete", Scope::Arrange),
    ("transport.play", Scope::Transport),
    ("transport.stop", Scope::Transport),
    ("transport.seek", Scope::Transport),
    ("transport.goto", Scope::Transport),
    ("transport.loop", Scope::Transport),
    ("record.start", Scope::Transport),
    ("record.stop", Scope::Transport),
];

pub fn scope_of(name: &str) -> Option<Scope> {
    SCOPES.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// The command the settings screen opens by (設定を表示), and its key: the screen takes
/// Ctrl+, itself ([`Studio::settings_key`]).
pub const SETTINGS: (&str, &str) = ("prefs.get", "Ctrl+,");

/// The operations the list starts with, as § 05 of the mock orders them:
/// the transport, then the edits of what is picked, adding, the mixer's
/// switches, export, history and files. The rest follow in the registry's
/// order.
pub const FIRST: &[&str] = &[
    "transport.play",
    "transport.stop",
    "record.start",
    "record.stop",
    "transport.loop",
    "transport.seek",
    "clip.split",
    "clip.duplicate",
    "clip.delete",
    "note.quantize",
    "note.transpose",
    "track.add.audio",
    "track.add.instrument",
    "track.add.bus",
    "folder.add",
    "track.mute",
    "track.solo",
    "track.arm",
    "export.mixdown",
    "export.stems",
    "history.undo",
    "history.redo",
    "history.list",
    "project.save",
    "project.save-as",
    "project.open",
    "version.save",
    "version.list",
    "view.snap",
    "view.link",
    "view.scale",
    // Not in the mock: the roll's other edits.
    "note.duplicate",
    "note.mute",
    "note.delete",
];

/// Where an operation comes in the list with nothing typed.
fn place_of(name: &str) -> usize {
    FIRST.iter().position(|n| *n == name).unwrap_or(FIRST.len())
}

/// How a row runs: by the same path as its key or its menu.
#[derive(Clone, Debug, PartialEq)]
pub enum Way {
    Arguments(Value),
    Binding(String),
    Key(Scope),
    /// Presses this (a top-bar button, the column's +) and picks the item.
    Menu(Hit, Option<Action>),
    /// Starts the plugins' processes again, as the track's bolt does.
    Restart,
    /// Opens the settings on 一般, as Ctrl+, does (SET-001).
    Settings,
    /// Runs the command with these arguments, from what is selected.
    Plain(Value),
}

/// One operation in the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The registry's name: what the CLI and MCP call it.
    pub name: &'static str,
    /// The registry's short name, as a menu writes it.
    pub label: String,
    pub key: Option<String>,
    pub icon: &'static str,
    /// Whether it can run here and now.
    pub enabled: bool,
    pub reason: String,
    pub way: Way,
}

/// The open palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub form: Option<crate::palette_args::Form>,
    result_layout: std::cell::RefCell<Option<std::rc::Rc<crate::palette_args::ResultLayout>>>,
    pub result_all: bool,
    pub result_selected: bool,
    /// What is typed: the list is filtered by it.
    pub query: String,
    /// What the IME is still composing after the query: shown in the field,
    /// not searched for until it is put in.
    pub marked: String,
    /// The picked row, an index into [`Palette::hits`]; only a row that can
    /// run is picked.
    pub selected: usize,
    /// How far the list is scrolled, in px.
    pub offset: f32,
    /// The row under the pointer, an index into [`Palette::hits`].
    pub hover: Option<usize>,
    /// The pointer is on a field's folder button (BRW-004).
    pub folder_hover: bool,
    pub rows: Vec<Row>,
}

/// Where the palette's parts are, in body coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteLayout {
    pub rect: Rect,
    pub search: Rect,
    pub list: Rect,
    /// The rows that can be seen: (index into [`Palette::hits`], rect).
    pub rows: Vec<(usize, Rect)>,
    /// The heading over the rows that cannot run, when it can be seen.
    pub head: Option<Rect>,
    /// The band the heading and the rows that cannot run sit on, inside
    /// the list, when any of it can be seen.
    pub off: Option<Rect>,
}

impl Palette {
    /// The width at most, the search row, a list row and the list at most (the mock's numbers).
    pub const W: f32 = 600.0;
    pub const SEARCH_H: f32 = 48.0;
    pub const ROW_H: f32 = 36.0;
    pub const LIST_MAX_H: f32 = 340.0;
    /// The heading 「今は使えない操作」 over the rows that cannot run.
    pub const HEAD_H: f32 = 28.0;
    pub const LIST_PAD: f32 = 4.0;

    pub fn new(rows: Vec<Row>) -> Palette {
        Palette {
            form: None,
            result_layout: Default::default(),
            result_all: false,
            result_selected: false,
            query: String::new(),
            marked: String::new(),
            selected: 0,
            offset: 0.0,
            hover: None,
            folder_hover: false,
            rows,
        }
    }

    /// The rows the query finds, in the list's order: what can run, then
    /// what cannot. Inside each, a name that starts with the query comes
    /// before one that has it, and that before one whose letters come in
    /// order; ties keep the order of [`FIRST`] and the registry.
    pub fn hits(&self) -> Vec<usize> {
        if self.form.is_some() {
            return (0..self.rows.len()).collect();
        }
        let q = self.query.trim().to_lowercase();
        let op = op_form(&q);
        let mut found: Vec<(bool, u8, usize)> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| rank(r, &q, &op).map(|k| (!r.enabled, k, i)))
            .collect();
        found.sort();
        found.into_iter().map(|(_, _, i)| i).collect()
    }

    /// How many of the hits can run: they come first, and only they can be picked.
    pub fn on(&self) -> usize {
        self.hits()
            .iter()
            .filter(|&&i| self.rows[i].enabled)
            .count()
    }

    /// The picked row (never one that cannot run).
    pub fn current(&self) -> Option<&Row> {
        self.hits()
            .get(self.selected)
            .map(|&i| &self.rows[i])
            .filter(|r| r.enabled)
    }

    /// Where hit `i` starts in the list, before scrolling: the rows that
    /// cannot run sit under their heading.
    fn row_top(i: usize, on: usize) -> f32 {
        i as f32 * Self::ROW_H + if i >= on { Self::HEAD_H } else { 0.0 }
    }

    /// The height of everything in the list.
    fn content_h(n: usize, on: usize) -> f32 {
        if n == 0 {
            return Self::ROW_H;
        }
        n as f32 * Self::ROW_H + if on < n { Self::HEAD_H } else { 0.0 }
    }

    fn inner_h(&self) -> f32 {
        Self::content_h(self.hits().len(), self.on()).min(Self::LIST_MAX_H - Self::LIST_PAD * 2.0)
    }

    pub(crate) fn replace_result_fields(&mut self, fields: Vec<crate::palette_args::Field>) {
        if let Some(form) = self.form.as_mut().filter(|f| f.output) {
            form.fields = fields;
            self.rows = form.rows();
            *self.result_layout.get_mut() = None;
        }
    }

    fn result_rows(&self, width: f32) -> Option<std::rc::Rc<crate::palette_args::ResultLayout>> {
        let form = self.form.as_ref().filter(|f| f.output)?;
        let mut cache = self.result_layout.borrow_mut();
        if cache.as_ref().is_none_or(|layout| layout.width != width) {
            *cache = Some(std::rc::Rc::new(crate::palette_args::ResultLayout::new(
                &form.fields,
                width,
            )));
        }
        cache.clone()
    }

    fn reveal_result(&mut self, size: (f32, f32)) {
        let l = self.layout(size);
        if let Some(rows) = self.result_rows(l.list.w - Self::LIST_PAD * 2.0) {
            let top = rows.tops[self.selected.min(rows.lines.len())];
            let bottom = rows.tops.get(self.selected + 1).copied().unwrap_or(top);
            let visible = l.list.h - Self::LIST_PAD * 2.0;
            if top < self.offset {
                self.offset = top;
            } else if bottom > self.offset + visible {
                self.offset = (bottom - visible).min(top);
            }
        }
    }

    pub fn selected_result_text(&self) -> Option<String> {
        let form = self.form.as_ref().filter(|f| f.output)?;
        if self.result_all {
            Some(
                form.fields
                    .iter()
                    .map(|f| {
                        if f.name == "group" {
                            f.label.clone()
                        } else {
                            format!("{}: {}", f.label, f.value)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        } else if self.result_selected {
            form.fields.get(self.selected).map(|f| f.value.clone())
        } else {
            None
        }
    }

    pub fn layout(&self, size: (f32, f32)) -> PaletteLayout {
        let (w, h) = size;
        let output = self.form.as_ref().is_some_and(|f| f.output);
        let (n, on) = if output {
            (self.rows.len(), self.rows.len())
        } else {
            (self.hits().len(), self.on())
        };
        let pw = Self::W.min(w * 0.92);
        let x = ((w - pw) / 2.0).round();
        // 12% of the window from its top, as the mock's veil puts it.
        let y = ((h + Layout::TOP_BAR_H + Layout::BOTTOM_BAR_H) * 0.12 - Layout::TOP_BAR_H)
            .max(Space::X2.px())
            .round();
        let result_heights = self.result_rows(pw - Self::LIST_PAD * 2.0);
        let inner = result_heights.as_ref().map_or_else(
            || self.inner_h(),
            |rows| {
                rows.total()
                    .min(Self::LIST_MAX_H - Self::LIST_PAD * 2.0)
                    .min((h - y - 50.0).max(40.0))
            },
        );
        let list_h = inner + Self::LIST_PAD * 2.0;
        let title_h = if self.form.is_some() { 28.0 } else { 0.0 };
        let search_h = if self
            .form
            .as_ref()
            .is_some_and(|form| form.fields.is_empty() || form.output)
        {
            0.0
        } else {
            Self::SEARCH_H
        };
        let error_h = if self.form.as_ref().is_some_and(|form| form.error.is_some()) {
            28.0
        } else {
            0.0
        };
        let rect = Rect::new(x, y, pw, title_h + search_h + list_h + error_h);
        let search = Rect::new(x, y + title_h, pw, search_h);
        let list = Rect::new(x, y + title_h + search_h, pw, list_h);
        let top = list.y + Self::LIST_PAD;
        let seen = |r: &Rect| r.bottom() > top && r.y < top + inner;
        let visible = result_heights
            .as_ref()
            .map_or(0..n, |rows| rows.visible(self.offset, inner));
        let rows = visible
            .map(|i| {
                let r = Rect::new(
                    x + Self::LIST_PAD,
                    top + result_heights
                        .as_ref()
                        .map_or_else(|| Self::row_top(i, on), |rows| rows.tops[i])
                        - self.offset,
                    pw - Self::LIST_PAD * 2.0,
                    result_heights
                        .as_ref()
                        .map_or(Self::ROW_H, |rows| rows.tops[i + 1] - rows.tops[i]),
                );
                (i, r)
            })
            .filter(|(_, r)| seen(r))
            .collect();
        let head = (on < n)
            .then(|| {
                Rect::new(
                    x + Self::LIST_PAD,
                    top + on as f32 * Self::ROW_H - self.offset,
                    pw - Self::LIST_PAD * 2.0,
                    Self::HEAD_H,
                )
            })
            .filter(seen);
        let off = (on < n)
            .then(|| {
                let y = (top + on as f32 * Self::ROW_H - self.offset).max(top);
                Rect::new(
                    x + Self::LIST_PAD,
                    y,
                    pw - Self::LIST_PAD * 2.0,
                    top + inner - y,
                )
            })
            .filter(|r| r.h > 0.0);
        PaletteLayout {
            rect,
            search,
            list,
            rows,
            head,
            off,
        }
    }

    /// Picks the row (one that can run, the nearest to `i`) and scrolls the
    /// list so it can be seen.
    pub fn select(&mut self, i: usize) {
        if self.form.is_some() {
            self.choose_field(i);
        }
        let on = if self.form.as_ref().is_some_and(|f| f.output) {
            self.rows.len()
        } else {
            self.on()
        };
        if on == 0 {
            self.selected = 0;
            return;
        }
        self.selected = i.min(on - 1);
        self.result_all = false;
        self.result_selected = self.form.as_ref().is_some_and(|f| f.output);
        if self.result_selected {
            return;
        }
        let top = Self::row_top(self.selected, on);
        let inner = self.inner_h();
        if top < self.offset {
            self.offset = top;
        } else if top + Self::ROW_H > self.offset + inner {
            self.offset = top + Self::ROW_H - inner;
        }
    }

    /// Scrolls the list by `dy` px, inside what there is.
    pub fn scroll(&mut self, dy: f32) {
        let width = self
            .result_layout
            .borrow()
            .as_ref()
            .map_or(Self::W - Self::LIST_PAD * 2.0, |r| r.width);
        let max = if let Some(rows) = self.result_rows(width) {
            (rows.total() - rows.total().min(Self::LIST_MAX_H - Self::LIST_PAD * 2.0)).max(0.0)
        } else {
            (Self::content_h(self.hits().len(), self.on()) - self.inner_h()).max(0.0)
        };
        self.offset = (self.offset + dy).clamp(0.0, max);
    }

    fn set_query(&mut self, query: String) {
        self.query = query;
        if self.form.is_some() {
            self.store_field();
            return;
        }
        self.hover = None;
        self.offset = 0.0;
        self.selected = 0;
    }
}

// ------------------------------------------------------------ text input

/// The UTF-16 offset `at` of `s` as a byte offset (at a char's start, and
/// at most the end).
pub(crate) fn byte_at(s: &str, at: usize) -> usize {
    let mut n = 0;
    for (b, c) in s.char_indices() {
        if n >= at {
            return b;
        }
        n += c.len_utf16();
    }
    s.len()
}

pub(crate) fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// What the IME gives, without the control characters a field cannot hold.
pub(crate) fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// The field's side of the IME (GPUI's `EntityInputHandler`, which the
/// screen hands on to these). Offsets are in UTF-16, as the IME counts.
/// The caret is always at the end: the field is one line that is typed
/// into and taken back with Backspace.
impl Palette {
    /// What the field shows: the query, then what is being composed.
    pub fn text(&self) -> String {
        format!("{}{}", self.query, self.marked)
    }

    /// Part of the field's text, and the range it really is.
    pub fn text_for_range(&self, range: Range<usize>) -> (String, Range<usize>) {
        let text = self.text();
        let a = byte_at(&text, range.start);
        let b = byte_at(&text, range.end).max(a);
        let real = utf16_len(&text[..a])..utf16_len(&text[..b]);
        (text[a..b].to_string(), real)
    }

    /// The caret, at the end of the field.
    pub fn selected_range(&self) -> Range<usize> {
        let n = utf16_len(&self.text());
        n..n
    }

    /// What the IME is composing, if anything.
    pub fn marked_range(&self) -> Option<Range<usize>> {
        (!self.marked.is_empty()).then(|| {
            let a = utf16_len(&self.query);
            a..a + utf16_len(&self.marked)
        })
    }

    /// The bytes of [`Palette::text`] an edit replaces: the range given,
    /// else what is being composed, else the caret.
    fn target(&self, text: &str, range: Option<Range<usize>>) -> (usize, usize) {
        let r = range
            .or_else(|| self.marked_range())
            .unwrap_or_else(|| self.selected_range());
        let a = byte_at(text, r.start);
        (a, byte_at(text, r.end).max(a))
    }

    /// Text put in for good: a typed letter, or what the IME composed.
    pub fn replace_text(&mut self, range: Option<Range<usize>>, text: &str) {
        if self
            .form
            .as_ref()
            .is_some_and(|form| form.output || self.selected >= form.fields.len())
        {
            return;
        }
        let full = self.text();
        let (a, b) = self.target(&full, range);
        let query = format!("{}{}{}", &full[..a], printable(text), &full[b..]);
        self.marked.clear();
        if query != self.query {
            self.set_query(query);
        }
    }

    /// Text the IME is composing: shown under the caret, not searched for.
    /// Empty text ends the composing with nothing put in.
    pub fn mark_text(&mut self, range: Option<Range<usize>>, text: &str) {
        let full = self.text();
        let (a, b) = self.target(&full, range);
        let query = format!("{}{}", &full[..a], &full[b..]);
        self.marked = printable(text);
        if query != self.query {
            self.set_query(query);
        }
    }

    /// The IME let go of what it was composing: it stays, as typed.
    pub fn unmark(&mut self) {
        if !self.marked.is_empty() {
            let query = self.text();
            self.marked.clear();
            self.set_query(query);
        }
    }

    /// Where a range of the field's text is on the screen (body
    /// coordinates), for the IME's window to sit by.
    pub fn text_bounds(
        &self,
        search: Rect,
        range: Range<usize>,
        mut measure: impl FnMut(&str) -> f32,
    ) -> Rect {
        let text = self.text();
        let field = field_of(search);
        let a = byte_at(&text, range.start);
        let b = byte_at(&text, range.end).max(a);
        let x0 = measure(&text[..a]);
        let x1 = measure(&text[..b]);
        Rect::new(
            field.x + x0,
            search.y + Space::X3.px(),
            (x1 - x0).max(1.5),
            search.h - Space::X3.px() * 2.0,
        )
    }
}

/// How well a row matches the query (lower is better), or `None`.
fn rank(r: &Row, q: &str, op: &str) -> Option<u8> {
    if q.is_empty() {
        return Some(0);
    }
    let label = r.label.to_lowercase();
    let name = op_form(r.name);
    let also = aliases(r.name);
    if label.starts_with(q) || name.starts_with(op) || also.iter().any(|a| a.starts_with(q)) {
        Some(0)
    } else if label.contains(q) || name.contains(op) || also.iter().any(|a| a.contains(q)) {
        Some(1)
    } else if in_order(op, &name) {
        Some(2)
    } else {
        None
    }
}

/// Other words a row is found by: Studio Pro's names of the tools, so
/// that 矢印・ペイント・リッスン find them though the screen keeps
/// 選択・鉛筆・試聴 (2026-10-21).
fn aliases(name: &str) -> &'static [&'static str] {
    match name {
        "tool.select" => &["矢印"],
        "tool.draw" => &["ペイント"],
        "tool.listen" => &["リッスン"],
        _ => &[],
    }
}

/// An operation's name as the query is matched with it: `save-as`,
/// `save_as` and `save as` are one.
fn op_form(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if matches!(c, '_' | '-' | ' ') { '.' } else { c })
        .collect()
}

/// Whether the letters of `q` come in `name` in this order (`ntr` finds
/// `note.transpose`).
fn in_order(q: &str, name: &str) -> bool {
    let mut letters = name.chars();
    q.chars().all(|c| letters.any(|n| n == c))
}

/// The operations a pop-over or a button of the screen does.
fn menu_of(name: &str) -> Option<(Hit, Option<Action>)> {
    use crate::export::Act;
    let add = |kind| (Hit::AddTrack, Some(Action::AddTrack(kind)));
    Some(match name {
        "version.list" => (Hit::Bar(BarButton::Saved), None),
        "version.save" => (Hit::Bar(BarButton::Saved), Some(Action::NameVersion)),
        "history.list" => (Hit::Bar(BarButton::History), None),
        "export.mixdown" => (
            Hit::Bar(BarButton::Export),
            Some(Action::Xp(Act::Mode(false))),
        ),
        "export.stems" => (
            Hit::Bar(BarButton::Export),
            Some(Action::Xp(Act::Mode(true))),
        ),
        "export.midi" => (Hit::Bar(BarButton::Export), Some(Action::Xp(Act::Midi))),
        "export.dawproject" => (
            Hit::Bar(BarButton::SongName),
            Some(Action::ExportDawproject),
        ),
        "track.add.audio" => add("audio"),
        "track.add.instrument" => add("instrument"),
        "track.add.bus" => add("bus"),
        "folder.add" => add("folder"),
        // The bar's own switch flips it (UI-302).
        "view.snap" => (Hit::Bar(BarButton::Snap), None),
        "view.link" => (Hit::Bar(BarButton::Grid), Some(Action::Link)),
        "view.scale" => (
            Hit::Bar(BarButton::Grid),
            Some(Action::OpenNoteEdit(crate::note_edit_panel::Kind::Scale)),
        ),
        "note.velocity.shape" => (
            Hit::Bar(BarButton::Grid),
            Some(Action::OpenNoteEdit(crate::note_edit_panel::Kind::Velocity)),
        ),
        "note.resize" => (
            Hit::Bar(BarButton::Grid),
            Some(Action::OpenNoteEdit(crate::note_edit_panel::Kind::Length)),
        ),
        _ => return None,
    })
}

/// Whether a command's arguments say what it acts on (the selection).
pub(crate) fn target_kind(name: &str, ty: &ArgType, required: bool) -> Option<IdKind> {
    let kind = match ty {
        ArgType::Id(k) | ArgType::Ids(k) => *k,
        _ => return None,
    };
    // `track` and `clip` name the target only when they must be given;
    // otherwise they say where to (clip.move --track).
    let named = matches!(name, "ids" | "id") || (required && matches!(name, "track" | "clip"));
    let known = matches!(
        kind,
        IdKind::Track | IdKind::Folder | IdKind::Node | IdKind::Clip | IdKind::NoteScope
    );
    (named && known).then_some(kind)
}

/// What a plain command's arguments come to here and now.
pub(crate) enum Fill {
    /// It needs values entered in the palette.
    Never,
    /// Nothing it acts on is selected now; the arguments are made with
    /// stand-ins (the song's first track, clip, ...) to tell whether it
    /// needs more than a selection, or `None` when the song has none.
    Later(Option<Value>),
    Now(Value),
}

impl Studio {
    /// What is selected, as ids of this kind.
    pub(crate) fn targets(&self, g: &Geometry, kind: IdKind) -> Vec<String> {
        let p = self.project();
        // Every selected row, the focus first: a command that takes one row takes the focus
        // (UI-340).
        let rows = self.picked_rows();
        let of = |want: fn(&RowKind) -> Option<String>| rows.iter().filter_map(want).collect();
        match kind {
            // A folder's row stands for the tracks in it, as on the mixer (UI-340).
            IdKind::Track => {
                let mut ids: Vec<String> = Vec::new();
                for r in &rows {
                    let tracks = match r {
                        RowKind::Track(t) => vec![*t],
                        RowKind::Folder(f) => p.tracks_in(*f),
                    };
                    for t in tracks {
                        let id = t.to_string();
                        if p.tracks.contains_key(&t) && !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
                ids
            }
            IdKind::Folder => of(|r| match r {
                RowKind::Folder(f) => Some(f.to_string()),
                RowKind::Track(_) => None,
            }),
            IdKind::Node => of(|r| match r {
                RowKind::Track(t) => Some(t.to_string()),
                RowKind::Folder(f) => Some(f.to_string()),
            }),
            IdKind::Clip => {
                let live = |t: &xirius_core::id::TrackId, c: &xirius_core::id::ClipId| {
                    p.tracks
                        .get(t)
                        .is_some_and(|t| t.clips.iter().any(|k| k.id == *c))
                };
                let mut ids: Vec<String> = self
                    .arrange
                    .clips
                    .iter()
                    .filter(|(t, c)| live(t, c))
                    .map(|(_, c)| c.to_string())
                    .collect();
                if ids.is_empty()
                    && let Some((t, c)) = self.selected_clip
                    && live(&t, &c)
                {
                    ids.push(c.to_string());
                }
                ids
            }
            IdKind::NoteScope => self
                .roll_ctx(g)
                .map(|ctx| {
                    self.roll
                        .selected_notes(&ctx)
                        .iter()
                        .map(|n| n.id.to_string())
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// One of the song's things of this kind, to stand in for a selection.
    fn stand_in(&self, kind: IdKind) -> Option<String> {
        let p = self.project();
        let clips = || p.tracks.values().flat_map(|t| t.clips.iter());
        // A track of the column (the master is not one).
        let track = p.root.iter().find_map(|n| match n {
            Node::Track(t) => Some(t.to_string()),
            Node::Folder(f) => p.tracks_in(*f).into_iter().next().map(|t| t.to_string()),
        });
        match kind {
            IdKind::Track | IdKind::Node => track,
            IdKind::Folder => p.folders.keys().next().map(|f| f.to_string()),
            IdKind::Clip => clips().next().map(|c| c.id.to_string()),
            IdKind::NoteScope => clips()
                .find_map(|c| match &c.body {
                    ClipBody::Midi { notes, .. } => notes.first(),
                    _ => None,
                })
                .map(|n| n.id.to_string()),
            _ => None,
        }
    }

    pub(crate) fn fill(&self, g: &Geometry, def: &CommandDef) -> Fill {
        let mut args = Map::new();
        let mut later = false;
        let mut stood_in = true;
        let mut aims = false;
        for arg in &def.args {
            if let Some(kind) = target_kind(arg.name, &arg.ty, arg.required) {
                aims = true;
                let mut ids = self.targets(g, kind);
                if ids.is_empty() {
                    later = true;
                    match self.stand_in(kind) {
                        Some(id) => ids.push(id),
                        None => {
                            stood_in = false;
                            continue;
                        }
                    }
                }
                if matches!(arg.ty, ArgType::Ids(_)) {
                    args.insert(arg.name.into(), json!(ids));
                } else {
                    args.insert(arg.name.into(), json!(ids[0]));
                }
            } else if arg.required {
                match arg.ty {
                    ArgType::Position => {
                        args.insert(arg.name.into(), json!(self.playhead));
                    }
                    _ => return Fill::Never,
                }
            }
        }
        // A command with arguments but nothing to act on (曲の設定) needs values.
        if !def.args.is_empty() && !aims {
            return Fill::Never;
        }
        if later {
            Fill::Later(stood_in.then_some(Value::Object(args)))
        } else {
            Fill::Now(Value::Object(args))
        }
    }

    /// Whether the key's operation can run here and now.
    pub(crate) fn key_enabled(&self, g: &Geometry, name: &str, scope: Scope) -> bool {
        let p = self.project();
        let take = self.transport.take.is_some();
        match scope {
            Scope::Global => match name {
                "history.undo" => self.song.can_undo() || self.job_undoable(),
                "history.redo" => self.song.can_redo(),
                "edit.copy" | "edit.cut" => self.has_selection(g),
                "edit.paste" => self.song.session().state.clipboard.is_some(),
                "select.on-tracks" => {
                    !self.arrange.clips.is_empty()
                        || matches!(self.selected_row, Some(RowKind::Track(_)))
                }
                "select.in-loop" => p.loop_range.is_some(),
                _ => true,
            },
            Scope::Roll => self.roll_ctx(g).is_some_and(|ctx| {
                if name == "note.quantize" {
                    !ctx.notes().is_empty()
                } else {
                    !self.roll.selected_notes(&ctx).is_empty()
                }
            }),
            Scope::Arrange => {
                !self.targets(g, IdKind::Clip).is_empty() && {
                    self.arrange.clips.iter().any(|(t, c)| {
                        p.tracks
                            .get(t)
                            .is_some_and(|t| t.clips.iter().any(|k| k.id == *c))
                    })
                }
            }
            Scope::Transport => match name {
                // Space plays, pauses, and ends a take: it always does something.
                "transport.play" => true,
                "transport.stop" => self.transport.playing || take,
                "record.start" => {
                    let (audio, midi) = crate::input::armed(&p);
                    !take && !(audio.is_empty() && midi.is_empty())
                }
                "record.stop" => take,
                _ => true,
            },
        }
    }

    /// The palette's rows, from the registry, as things stand now.
    pub fn palette_rows(&mut self, g: &Geometry) -> Vec<Row> {
        let defs: Vec<CommandDef> = self.song.registry().all().to_vec();
        let mut rows = Vec::new();
        for def in &defs {
            let (way, enabled) = if def.name == SETTINGS.0 {
                (Way::Settings, true)
            } else if def.name == "export.selection" {
                (
                    Way::Binding(def.name.into()),
                    !self.export_clip_ids().is_empty(),
                )
            } else if def.name == "view.export-run" {
                let enabled = self
                    .popup
                    .as_ref()
                    .is_some_and(|p| p.kind == crate::popup::PopupKind::Export)
                    && self.export_view().can_go();
                (Way::Binding(def.name.into()), enabled)
            } else if def.name == "auto.mode" {
                let enabled = matches!(self.selected_row, Some(RowKind::Track(t)) if self.project().tracks.contains_key(&t));
                (Way::Binding("auto.mode --mode read".into()), enabled)
            } else if def.name.starts_with("view.zoom.")
                || def.name.starts_with("tool.")
                || matches!(
                    def.name,
                    "view.export-stems"
                        | "view.instrument"
                        | "view.add-track"
                        | "clip.open"
                        | "loop.selection"
                        | "transport.locate-selection"
                        | "transport.locate-mouse"
                        | "view.palette"
                        | "view.settings"
                        | "view.lanes"
                        | "view.mixer"
                        | "view.record-panel"
                        | "view.browser"
                        | "view.autoscroll"
                        | "auto.lane.next"
                        | "project.new"
                        | "transport.start"
                        | "transport.metronome"
                )
            {
                let binding = def
                    .bindings()
                    .find(|e| e.command.split_whitespace().next() == Some(def.name))
                    .map(|e| e.command)
                    .unwrap_or(def.name);
                (Way::Binding(binding.into()), true)
            } else if let Some(scope) = scope_of(def.name) {
                (Way::Key(scope), self.key_enabled(g, def.name, scope))
            } else if let Some((hit, action)) = menu_of(def.name) {
                let enabled = match def.name {
                    "version.save" => self.song.dir().is_some(),
                    "note.velocity.shape" | "note.resize" => {
                        self.roll_ctx(g).is_some() && !self.roll.selected.is_empty()
                    }
                    _ if hit == Hit::AddTrack => g.toggle(Hit::AddTrack).is_some(),
                    _ => true,
                };
                (Way::Menu(hit, action), enabled)
            } else if matches!(def.name, "plugin.restart" | "plugin.reload") {
                let p = self.project();
                let down = p.tracks.keys().any(|t| self.crashed_on(*t).is_some());
                (Way::Restart, down)
            } else if def.history
                && matches!(def.risk, Risk::Write | Risk::Destructive)
                && matches!(def.handler, Handler::Edit(_))
            {
                match self.fill(g, def) {
                    Fill::Never => (
                        Way::Arguments(self.palette_seed(g, def)),
                        !def.args
                            .iter()
                            .filter_map(|arg| target_kind(arg.name, &arg.ty, arg.required))
                            .any(|kind| self.targets(g, kind).is_empty()),
                    ),
                    Fill::Later(Some(args)) => match self.song.check(def.name, args) {
                        Err(e) if e.code == ErrorCode::InvalidArgs => {
                            (Way::Arguments(self.palette_seed(g, def)), false)
                        }
                        _ => (Way::Plain(json!({})), false),
                    },
                    Fill::Later(None) => (Way::Plain(json!({})), false),
                    Fill::Now(args) => match self.song.check(def.name, args.clone()) {
                        // Ok(false): it would change nothing (no loop to clear).
                        Ok(changes) => (Way::Plain(args), changes),
                        // It needs a value besides what it acts on (--by, --length).
                        Err(e) if e.code == ErrorCode::InvalidArgs => (Way::Arguments(args), true),
                        Err(_) => (Way::Plain(args), false),
                    },
                }
            } else {
                if def.args.is_empty()
                    && matches!(
                        def.risk,
                        Risk::Guarded | Risk::Destructive | Risk::HumanOnly
                    )
                {
                    (Way::Arguments(json!({})), true)
                } else if def.args.is_empty() {
                    (Way::Plain(json!({})), true)
                } else {
                    (Way::Arguments(self.palette_seed(g, def)), true)
                }
            };
            rows.push(Row {
                name: def.name,
                label: def.label().unwrap_or(def.name).to_string(),
                key: {
                    let name = if def.name == SETTINGS.0 {
                        "view.settings"
                    } else {
                        def.name
                    };
                    let mut text = self.key_display(self.input_context(g), name);
                    // A command with many argument variants (the jumps of transport.goto) shows
                    // one variant's keys, not every key of the family in one chip.
                    if text.matches(" / ").count() > 3 {
                        text.clear();
                    }
                    if text.is_empty() {
                        let candidates: Vec<_> = def
                            .bindings()
                            .filter(|e| e.command.split_whitespace().next() == Some(name))
                            .map(|e| (e.context, e.command))
                            .collect();
                        for (ctx, cmd) in candidates {
                            text = self.key_display(ctx, cmd);
                            if !text.is_empty() {
                                break;
                            }
                        }
                    }
                    (!text.is_empty()).then_some(text)
                },
                icon: def.icon(),
                reason: if enabled {
                    String::new()
                } else {
                    crate::disabled_reason::command(self, def.name).into()
                },
                enabled,
                way,
            });
        }
        // Not a command: the panel the logo opens (LIC-015), which a narrow
        // bar has no logo for. No operation's name is written beside it.
        rows.push(Row {
            name: "",
            label: "Xirius について".to_string(),
            key: None,
            icon: "info",
            enabled: true,
            reason: String::new(),
            way: Way::Menu(Hit::Bar(BarButton::Logo), None),
        });
        // The song-files tab has no key of its own (BRW-004): the palette opens it.
        // It is `view.browser` with a tab, whose own row is above, so no
        // operation's name is written beside it either.
        rows.push(Row {
            name: "",
            label: "プール".to_string(),
            key: None,
            icon: "pool",
            enabled: true,
            reason: String::new(),
            way: Way::Binding("view.browser --tab song_files".into()),
        });
        // The mock's order first, the rest in the registry's; what can run
        // is put before what cannot by [`Palette::hits`].
        rows.sort_by_key(|r| place_of(r.name));
        rows
    }

    pub fn open_palette(&mut self, g: &Geometry) {
        self.popup = None;
        self.error = None;
        self.browser.typing = false;
        let rows = self.palette_rows(g);
        self.palette = Some(Palette::new(rows));
    }

    pub fn close_palette(&mut self) -> bool {
        self.palette.take().is_some()
    }

    /// A key while the palette is open (all are its own), or Ctrl+K to open
    /// it. The letters are not put in here: they come through the IME
    /// ([`Palette::replace_text`]), as the window hands text to the field.
    pub fn palette_key(&mut self, g: &Geometry, key: &str, mods: Mods) -> bool {
        let Some(p) = &mut self.palette else {
            if key == "k" && mods.ctrl && !mods.shift && !mods.alt {
                self.open_palette(g);
                return true;
            }
            return false;
        };
        // While the IME is composing, its keys are its own.
        if !p.marked.is_empty() {
            return true;
        }
        match key {
            "escape" => {
                self.palette = None;
            }
            "k" if mods.ctrl => {
                self.palette = None;
            }
            "a" if mods.ctrl && p.form.as_ref().is_some_and(|f| f.output) => {
                p.result_all = true;
                p.result_selected = true;
            }
            "up" => p.select(p.selected.saturating_sub(1)),
            "down" => p.select(p.selected + 1),
            "tab" if p.form.is_some() => p.select(if mods.shift {
                p.selected.saturating_sub(1)
            } else {
                p.selected + 1
            }),
            "space"
                if p.form.is_some()
                    && p.form.as_ref().is_some_and(|form| {
                        form.fields
                            .get(p.selected)
                            .is_some_and(|field| !field.choices.is_empty())
                    }) =>
            {
                p.cycle_field()
            }
            "enter" => {
                self.run_selected(g);
            }
            "backspace" => {
                let mut q = p.query.clone();
                q.pop();
                p.set_query(q);
            }
            _ => {}
        }
        if let Some(p) = &mut self.palette {
            p.reveal_result(g.size);
        }
        true
    }

    /// Runs the picked row. Returns whether it ran.
    pub fn run_selected(&mut self, g: &Geometry) -> bool {
        if self.palette.as_ref().is_some_and(|p| p.form.is_some()) {
            return self.palette_form_enter(g);
        }
        let Some(row) = self.palette.as_ref().and_then(|p| p.current().cloned()) else {
            return false;
        };
        self.run_row(g, &row)
    }

    /// Runs a row by the path of its key or menu; the palette closes first.
    pub fn run_row(&mut self, g: &Geometry, row: &Row) -> bool {
        if !row.enabled {
            return false;
        }
        self.palette = None;
        match &row.way {
            Way::Arguments(seed) => self.open_palette_form(row.name, seed.clone()),
            Way::Binding(binding) => self.run_binding(g, binding),
            Way::Key(scope) => {
                match scope {
                    Scope::Roll => self.focus_arrange(false),
                    Scope::Arrange => self.focus_arrange(true),
                    _ => {}
                }
                let binding = match row.name {
                    "note.transpose" => "note.transpose --by 1",
                    "note.move" => "edit.nudge.right",
                    _ => row.name,
                };
                self.run_binding(g, binding)
            }
            Way::Menu(hit, action) => {
                let pressed = self.press(g, *hit);
                let Some(action) = action else {
                    return pressed;
                };
                if self.popup.is_none() {
                    return false;
                }
                self.popup_action(g, action.clone())
            }
            Way::Restart => self.restart_plugins(),
            Way::Settings => {
                self.open_settings(crate::settings::Section::General);
                true
            }
            Way::Plain(args) => {
                if crate::palette_args::on_screen(row.name) {
                    let args = args.as_object().cloned().unwrap_or_default();
                    return self.run_parsed(g, row.name.into(), args);
                }
                if self
                    .song
                    .registry()
                    .get(row.name)
                    .is_some_and(|def| def.risk == Risk::Read)
                {
                    return self.palette_query(g, row.name, args.clone());
                }
                let ran = self.run_command(row.name, args.clone());
                self.rebuild(g);
                ran
            }
        }
    }

    /// A press while the palette is open: a row runs, outside closes it.
    pub fn palette_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(p) = &self.palette else {
            return false;
        };
        let l = p.layout(g.size);
        let selected_before = p.selected;
        if !l.rect.contains(x, y) {
            self.palette = None;
            return true;
        }
        let hit = l
            .rows
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(i, _)| *i);
        if let Some(i) = hit {
            let row = p.hits().get(i).map(|&k| p.rows[k].clone());
            // A row that cannot run is not picked by a click either.
            if let Some(row) = row.filter(|r| r.enabled) {
                if let Some(p) = &mut self.palette {
                    p.select(i);
                }
                if self.palette.as_ref().is_some_and(|p| p.form.is_some()) {
                    let last = self.palette.as_ref().unwrap().rows.len() - 1;
                    let field = self
                        .palette
                        .as_ref()
                        .and_then(|p| p.form.as_ref())
                        .and_then(|f| f.fields.get(i))
                        .cloned();
                    let r = l.rows.iter().find(|(j, _)| *j == i).map(|(_, r)| *r);
                    if i == last && !self.palette.as_ref().unwrap().form.as_ref().unwrap().output {
                        self.palette_form_enter(g);
                    } else if field.as_ref().is_some_and(|f| f.picks_folder())
                        && r.is_some_and(|r| crate::palette_args::folder_button(r).contains(x, y))
                    {
                        // The OS's folder window answers in `finish_picked` (BRW-004).
                        self.file_request = Some(crate::studio::FileRequest::MediaFolder);
                    } else if let Some(palette) = &mut self.palette
                        && (selected_before == i || field.is_some_and(|f| f.is_switch()))
                    {
                        // A switch flips on every press; another choice only once it is picked.
                        palette.cycle_field();
                    }
                } else {
                    self.run_row(g, &row);
                }
            }
        }
        true
    }

    /// The pointer moved while the palette is open. Returns whether the row under it changed.
    pub fn palette_hover(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(p) = &mut self.palette else {
            return false;
        };
        let l = p.layout(g.size);
        let hover = l
            .rows
            .iter()
            .find(|(_, r)| r.contains(x, y) && l.list.contains(x, y))
            .map(|(i, _)| *i);
        let folder = hover.is_some_and(|i| {
            p.form
                .as_ref()
                .and_then(|f| f.fields.get(i))
                .is_some_and(crate::palette_args::Field::picks_folder)
                && l.rows
                    .iter()
                    .any(|(j, r)| *j == i && crate::palette_args::folder_button(*r).contains(x, y))
        });
        let changed = hover != p.hover || folder != p.folder_hover;
        p.hover = hover;
        p.folder_hover = folder;
        changed
    }

    /// The wheel while the palette is open: over the list it scrolls.
    pub fn palette_wheel(&mut self, g: &Geometry, x: f32, y: f32, dy: f32) -> bool {
        let Some(p) = &mut self.palette else {
            return false;
        };
        if p.layout(g.size).list.contains(x, y) {
            if let Some(heights) = p.result_rows(p.layout(g.size).list.w - Palette::LIST_PAD * 2.0)
            {
                let layout = p.layout(g.size);
                let max = (heights.total() - layout.list.h + Palette::LIST_PAD * 2.0).max(0.0);
                p.offset = (p.offset - dy).clamp(0.0, max);
            } else {
                p.scroll(-dy);
            }
        }
        true
    }
}

// ------------------------------------------------------------ painting

/// The veil over the body and the palette on it.
pub fn paint(pen: &mut Pen, palette: &Palette, g: &Geometry) {
    let c = pen.c;
    pen.fill(Rect::new(0.0, 0.0, g.size.0, g.size.1), c.veil);
    let l = palette.layout(g.size);
    // A floating panel: the soft shadow all round it (UI-007), then its face.
    let at = Bounds::new(
        point(px(pen.origin.0 + l.rect.x), px(pen.origin.1 + l.rect.y)),
        size(px(l.rect.w), px(l.rect.h)),
    );
    pen.window
        .paint_drop_shadows(at, Corners::all(px(Radius::M.px())), &c.float);
    pen.round(l.rect, c.paper, Radius::M);
    if let Some(form) = &palette.form {
        pen.text(
            &form.title,
            FontSize::Small,
            Weight::Medium,
            c.ink_strong,
            Rect::new(
                l.rect.x + Space::X4.px(),
                l.rect.y + 4.0,
                l.rect.w - 2.0 * Space::X4.px(),
                24.0,
            ),
            Align::Left,
        );
    }
    if let Some(error) = palette.form.as_ref().and_then(|form| form.error.as_ref()) {
        let rect = Rect::new(
            l.rect.x + Space::X4.px(),
            l.rect.bottom() - 28.0,
            l.rect.w - 2.0 * Space::X4.px(),
            28.0,
        );
        pen.mask(rect, |pen| {
            pen.text(
                error,
                FontSize::Caption,
                Weight::Regular,
                c.ink_alert,
                rect,
                Align::Left,
            );
        });
    }
    paint_search(pen, palette, l.search);
    let output = palette.form.as_ref().is_some_and(|f| f.output);
    let results = palette.result_rows(l.list.w - Palette::LIST_PAD * 2.0);
    let hits = if output { Vec::new() } else { palette.hits() };
    let inner = Rect::new(
        l.list.x,
        l.list.y + Palette::LIST_PAD,
        l.list.w,
        l.list.h - Palette::LIST_PAD * 2.0,
    );
    if if output {
        palette.rows.is_empty()
    } else {
        hits.is_empty()
    } {
        pen.text(
            "見つかりません",
            FontSize::Body,
            Weight::Regular,
            c.ink,
            Rect::new(inner.x + Space::X4.px(), inner.y, inner.w, Palette::ROW_H),
            Align::Left,
        );
        return;
    }
    pen.mask(inner, |pen| {
        // What cannot run sits apart, on a band of its own.
        if let Some(off) = l.off {
            pen.round(off, c.surface_1, Radius::S);
        }
        if let Some(h) = l.head {
            pen.text(
                "今は使えない操作",
                FontSize::Caption,
                Weight::Medium,
                c.ink,
                Rect::new(h.x + Space::X3.px(), h.y, h.w - Space::X3.px(), h.h),
                Align::Left,
            );
        }
        for (i, r) in &l.rows {
            let row = &palette.rows[if output { *i } else { hits[*i] }];
            if let Some(field) = palette.form.as_ref().and_then(|form| form.fields.get(*i)) {
                let folder_hot = palette.folder_hover && palette.hover == Some(*i);
                if palette.form.as_ref().is_some_and(|f| f.output) {
                    crate::palette_args::paint_result(
                        pen,
                        field,
                        *r,
                        palette.result_all || (palette.result_selected && *i == palette.selected),
                        &results.as_ref().unwrap().lines[*i],
                    );
                    continue;
                }
                crate::palette_args::paint_field(
                    pen,
                    field,
                    *r,
                    *i == palette.selected,
                    folder_hot,
                );
                continue;
            }
            paint_row(
                pen,
                row,
                *r,
                row.enabled && *i == palette.selected,
                row.enabled && palette.hover == Some(*i),
            );
        }
    });
    if let Some(i) = palette.hover
        && let Some(&k) = hits.get(i)
        && !palette.rows[k].enabled
        && let Some((_, r)) = l.rows.iter().find(|(j, _)| *j == i)
    {
        crate::hover_tip::balloon(pen, &palette.rows[k].reason, *r, l.rect);
    }
    if palette.folder_hover
        && let Some(i) = palette.hover
        && let Some((_, r)) = l.rows.iter().find(|(j, _)| *j == i)
    {
        let button = crate::palette_args::folder_button(*r);
        crate::hover_tip::balloon(pen, crate::palette_args::PICK_FOLDER, button, l.rect);
    }
}

/// Where the typed text goes in the search row: after the magnifier.
pub fn field_of(search: Rect) -> Rect {
    let pad = Space::X4.px();
    let x = search.x + pad + Layout::ICON_SIZE + Space::X2.px();
    Rect::new(x, search.y, search.right() - pad - x, search.h)
}

fn paint_search(pen: &mut Pen, palette: &Palette, r: Rect) {
    if palette
        .form
        .as_ref()
        .is_some_and(|form| form.output || palette.selected >= form.fields.len())
    {
        return;
    }
    let c = pen.c;
    let pad = Space::X4.px();
    let icon = Rect::new(
        r.x + pad,
        r.y + (r.h - Layout::ICON_SIZE) / 2.0,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    );
    pen.icon(
        if palette.form.is_some() {
            "prompt"
        } else {
            "search"
        },
        icon,
        c.ink,
    );
    let field = field_of(r);
    pen.mask(field, |pen| {
        let mut w = 0.0;
        if palette.query.is_empty() && palette.marked.is_empty() {
            pen.text(
                palette
                    .form
                    .as_ref()
                    .and_then(|form| form.fields.get(palette.selected))
                    .map(|field| field.label.as_str())
                    .unwrap_or("操作を探す"),
                FontSize::Lead,
                Weight::Regular,
                c.ink,
                field,
                Align::Left,
            );
        } else {
            let displayed = palette
                .form
                .as_ref()
                .and_then(|form| form.fields.get(palette.selected))
                .filter(|field| !field.choices.is_empty())
                .map(|field| field.display(&palette.query))
                .unwrap_or_else(|| palette.query.clone());
            w = pen.text(
                &displayed,
                FontSize::Lead,
                Weight::Regular,
                c.ink_strong,
                field,
                Align::Left,
            );
        }
        // What the IME is still composing sits on the selection's face:
        // shown, not yet searched for.
        if !palette.marked.is_empty() {
            let mw = pen.measure(&palette.marked, FontSize::Lead, Weight::Regular);
            let inset = Space::X3.px();
            pen.round(
                Rect::new(field.x + w, r.y + inset, mw, r.h - inset * 2.0),
                c.sel,
                Radius::Xs,
            );
            pen.text(
                &palette.marked,
                FontSize::Lead,
                Weight::Regular,
                c.ink_strong,
                Rect::new(field.x + w, r.y, field.w - w, r.h),
                Align::Left,
            );
            w += mw;
        }
        pen.fill(
            Rect::new(field.x + w + 1.0, r.y + 14.0, 1.5, r.h - 28.0),
            c.ink_strong,
        );
    });
}

/// Where a row's parts go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowParts {
    pub icon: Rect,
    /// The name, as wide as it is drawn (cut short when there is no room).
    pub label: Rect,
    /// The key's chip, right after the name.
    pub chip: Option<Rect>,
    /// The operation's name, at the right.
    pub op: Rect,
}

/// Lays a row out; `measure` gives a text's width at a size and weight.
pub fn row_parts(
    row: &Row,
    r: Rect,
    mut measure: impl FnMut(&str, FontSize, Weight) -> f32,
) -> RowParts {
    let pad = Space::X3.px();
    let gap = Space::X2.px();
    let icon = Rect::new(
        r.x + pad,
        r.y + (r.h - Layout::ICON_SIZE) / 2.0,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    );
    // The operation's name sits at the right; the words take what is left.
    let op_w = measure(row.name, FontSize::Caption, Weight::Regular);
    let op = Rect::new(r.right() - pad - op_w, r.y, op_w, r.h);
    let x = icon.right() + gap;
    let words = measure(&row.label, FontSize::Body, Weight::Regular);
    let chip_w = row.key.as_deref().map_or(0.0, |k| {
        measure(k, FontSize::Micro, Weight::Medium) + Space::X1.px() * 2.0
    });
    let room = (op.x - gap - x - if chip_w > 0.0 { chip_w + gap } else { 0.0 }).max(0.0);
    let label = Rect::new(x, r.y, words.min(room), r.h);
    let chip = row.key.as_ref().map(|_| {
        let chip_h = FontSize::Micro.px() * 1.6;
        Rect::new(
            label.right() + gap,
            r.y + (r.h - chip_h) / 2.0,
            chip_w,
            chip_h,
        )
    });
    RowParts {
        icon,
        label,
        chip,
        op,
    }
}

fn paint_row(pen: &mut Pen, row: &Row, r: Rect, selected: bool, hover: bool) {
    let c = pen.c;
    if selected {
        pen.round(r, c.sel, Radius::S);
    } else if hover {
        pen.round(r, c.hover, Radius::S);
    }
    // A row that cannot run keeps readable words (UI-092), one step down:
    // its name in the secondary ink and its icon dim, as the mock dims a
    // button that cannot be pressed; it is never lit.
    let strong = if row.enabled { c.ink_strong } else { c.ink };
    let parts = row_parts(row, r, |s, f, w| pen.measure(s, f, w));
    let icon = if row.enabled {
        strong
    } else {
        c.ink.opacity(0.4)
    };
    pen.icon(row.icon, parts.icon, icon);
    let room = parts.chip.map_or(parts.op.x, |k| k.x) - parts.label.x;
    pen.mask(parts.label, |pen| {
        pen.text(
            &row.label,
            FontSize::Body,
            Weight::Regular,
            strong,
            Rect::new(parts.label.x, r.y, room.max(parts.label.w), r.h),
            Align::Left,
        );
    });
    if let (Some(k), Some(chip)) = (row.key.as_deref(), parts.chip) {
        pen.round(chip, c.surface_3, Radius::Xs);
        pen.text(
            k,
            FontSize::Micro,
            Weight::Medium,
            c.ink,
            chip,
            Align::Center,
        );
    }
    pen.digits(
        row.name,
        FontSize::Caption,
        Weight::Regular,
        c.ink,
        parts.op,
        Align::Left,
    );
}

// ------------------------------------------------------------ screenshots

/// The palette's run of pictures (`--script palette`): opened from the bar,
/// picked with the arrows, filtered, run, opened with Ctrl+K, undone, with
/// nothing picked, and Japanese composed in the field. The keys go through
/// the window like any key, and their letters through the IME's door, as
/// the window hands them on.
pub const SHOTS: &[crate::shot::Shot] = {
    use crate::shot::{Act, Play, Shot, Target};
    use Act::Key;
    /// Stage `n` of this script, played by [`shot`].
    const fn staged(n: u8) -> Act {
        Act::Stage(Play::Laid(shot), n)
    }
    &[
        Shot {
            name: "00-base",
            acts: &[staged(0)],
        },
        Shot {
            name: "01-open-from-bar",
            acts: &[Act::Press(Target::Top(BarButton::Palette))],
        },
        Shot {
            name: "02-arrow-down",
            acts: &[Key("down"), Key("down"), Key("down"), staged(1)],
        },
        Shot {
            name: "03-filter-mute",
            acts: &[Key("m"), Key("u"), Key("t"), Key("e")],
        },
        Shot {
            name: "04-ran-track-mute",
            acts: &[Key("enter")],
        },
        Shot {
            name: "05-ctrl-k-undo",
            acts: &[Key("ctrl-k"), Key("u"), Key("n"), Key("d"), Key("o")],
        },
        Shot {
            name: "06-undone",
            acts: &[Key("enter")],
        },
        Shot {
            name: "07-filter-note",
            acts: &[Key("ctrl-k"), Key("n"), Key("o"), Key("t"), Key("e")],
        },
        Shot {
            name: "08-not-found",
            acts: &[Key("z"), Key("z")],
        },
        Shot {
            name: "09-closed",
            acts: &[Key("escape")],
        },
        Shot {
            name: "10-nothing-picked",
            acts: &[staged(2), Key("ctrl-k")],
        },
        Shot {
            name: "11-scrolled",
            acts: &[staged(3)],
        },
        Shot {
            name: "12-ime-composing",
            acts: &[staged(4)],
        },
        Shot {
            name: "13-ime-committed",
            acts: &[staged(5)],
        },
    ]
};

/// The stages of [`SHOTS`] that are not a key or a press.
pub fn shot(s: &mut Studio, g: &Geometry, stage: u8) {
    match stage {
        // The Keys clip picked, its notes picked in the roll and the
        // playhead inside it, so the edits that act on them are on.
        0 => {
            let (Some((t, c)), Some(ctx)) = (s.selected_clip, s.roll_ctx(g)) else {
                return;
            };
            s.arrange.clips.insert((t, c));
            s.arrange.focus = false;
            let out = s.roll.key(&ctx, "a", Mods::ctrl());
            s.apply_roll(g, out);
            let p = s.project();
            if let Some(clip) = p.tracks[&t].clips.iter().find(|k| k.id == c) {
                s.playhead = clip.start + clip.length / 2;
            }
        }
        // The pointer over the second row.
        1 => {
            if let Some(p) = &s.palette
                && let Some((_, r)) = p.layout(g.size).rows.get(1).copied()
            {
                s.palette_hover(g, r.x + r.w / 2.0, r.y + r.h / 2.0);
            }
        }
        // Nothing picked, and the mixer instead of the roll.
        2 => {
            s.selected_row = None;
            s.selected_clip = None;
            s.arrange.clips.clear();
            s.tab = crate::studio::DrawerTab::Mixer;
        }
        // The list scrolled down to the rows that cannot run: the last
        // rows that can run, the heading, then those that cannot.
        3 => {
            if let Some(p) = &mut s.palette {
                let at = Palette::row_top(p.on(), p.on()) - Palette::ROW_H * 4.0;
                p.scroll(at - p.offset);
            }
        }
        // 「ほぞん」 being composed by the IME, and then put in as 「保存」:
        // the same calls the window's IME makes.
        4 => {
            if let Some(p) = &mut s.palette {
                p.set_query(String::new());
                p.mark_text(None, "ほ");
                p.mark_text(None, "ほぞん");
            }
        }
        5 => {
            if let Some(p) = &mut s.palette {
                p.mark_text(None, "保存");
                p.replace_text(None, "保存");
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::PopupKind;
    use crate::studio::FileRequest;
    use xirius_core::model::Project;

    fn setup() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 860.0);
        s.resized(&g);
        (s, g)
    }

    /// The Keys clip picked in the song overview, its notes put off the
    /// grid and all picked in the roll, and the playhead inside the clip.
    fn picked() -> (Studio, Geometry) {
        let (mut s, g) = setup();
        let (t, c) = s.selected_clip.expect("the Keys clip");
        let ctx = s.roll_ctx(&g).expect("the roll");
        let ids: Vec<String> = ctx.notes().iter().map(|n| n.id.to_string()).collect();
        s.song.run("note.delete", json!({ "ids": ids })).unwrap();
        s.song
            .run(
                "note.put",
                json!({"clip": c.to_string(), "notes": [
                    {"at": 970, "pitch": 60, "length": 900, "velocity": 80},
                    {"at": 2900, "pitch": 62, "length": 1400, "velocity": 90},
                    {"at": 4810, "pitch": 64, "length": 470, "velocity": 100}
                ]}),
            )
            .unwrap();
        s.resized(&g);
        s.arrange.clips.insert((t, c));
        s.arrange.focus = false;
        s.edit_key(&g, "a", Mods::ctrl());
        let ctx = s.roll_ctx(&g).unwrap();
        assert_eq!(s.roll.selected_notes(&ctx).len(), 3);
        let clip = s.project().tracks[&t]
            .clips
            .iter()
            .find(|k| k.id == c)
            .cloned()
            .unwrap();
        s.playhead = clip.start + clip.length / 2;
        (s, g)
    }

    /// As the window does: the key goes to the palette (which only takes
    /// it), and its letter comes after it through the IME's door.
    fn type_in(s: &mut Studio, g: &Geometry, text: &str) {
        for ch in text.chars() {
            let k = ch.to_string();
            assert!(s.palette_key(g, &k, Mods::NONE));
            s.palette.as_mut().unwrap().replace_text(None, &k);
        }
    }

    fn names(s: &Studio) -> Vec<&'static str> {
        let p = s.palette.as_ref().expect("open");
        p.hits().iter().map(|&i| p.rows[i].name).collect()
    }

    fn row(s: &mut Studio, g: &Geometry, name: &str) -> Row {
        s.palette_rows(g)
            .into_iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no row {name}"))
    }

    /// The command of the newest step the song is at.
    fn last_command(s: &Studio) -> String {
        s.song()
            .history()
            .into_iter()
            .rfind(|h| h.current)
            .map(|h| h.command)
            .unwrap_or_default()
    }

    fn state(s: &Studio) -> Project {
        (*s.project()).clone()
    }

    #[test]
    fn ctrl_k_and_the_bar_button_open_it_and_escape_closes_it() {
        let (mut s, g) = setup();
        let steps = s.song().steps();
        assert!(s.palette_key(&g, "k", Mods::ctrl()));
        assert!(s.palette.is_some());
        assert_eq!(s.bar_view().open, Some(BarButton::Palette));
        assert!(s.palette_key(&g, "escape", Mods::NONE));
        assert!(s.palette.is_none());

        assert!(s.press(&g, Hit::Bar(BarButton::Palette)));
        assert!(s.palette.is_some());
        // Ctrl+K again closes it; other keys with Ctrl do nothing else.
        assert!(s.palette_key(&g, "z", Mods::ctrl()));
        assert!(s.palette.is_some());
        assert!(s.palette_key(&g, "k", Mods::ctrl()));
        assert!(s.palette.is_none());
        // Shift+Ctrl+K and plain K are not its keys.
        assert!(!s.palette_key(&g, "k", Mods::ctrl_shift()));
        assert!(!s.palette_key(&g, "k", Mods::NONE));
        assert!(s.palette.is_none());
        // Opening it changes nothing in the song.
        assert_eq!(s.song().steps(), steps);
    }

    #[test]
    fn opening_it_closes_a_pop_over_and_the_error_line() {
        let (mut s, g) = setup();
        s.press(&g, Hit::Bar(BarButton::History));
        s.error = Some("x".into());
        assert!(s.popup.is_some());
        s.open_palette(&g);
        assert!(s.popup.is_none() && s.error.is_none());
    }

    #[test]
    fn it_filters_by_the_name_in_japanese_and_by_the_operation_name() {
        let (mut s, g) = picked();
        s.open_palette(&g);
        let all = names(&s).len();
        assert!(all > 20, "{all}");

        s.palette
            .as_mut()
            .unwrap()
            .set_query("ノートをクオンタイズ".into());
        assert_eq!(names(&s), ["note.quantize"]);
        // The record panel's own quantize and the arrange's clip quantize are found by the same
        // word, next to it.
        s.palette.as_mut().unwrap().set_query("クオンタイズ".into());
        assert_eq!(
            names(&s),
            [
                "quantize.program",
                "note.quantize",
                "record.input-quantize",
                "clip.quantize"
            ]
        );

        s.palette.as_mut().unwrap().set_query(String::new());
        type_in(&mut s, &g, "save-as");
        assert_eq!(names(&s), ["project.save-as"]);
        // `save_as` and `save as` are the same name.
        s.palette.as_mut().unwrap().set_query("save as".into());
        assert_eq!(names(&s), ["project.save-as"]);
        // Letters in order: "tkmute" is track.mute.
        s.palette.as_mut().unwrap().set_query("tkmute".into());
        assert!(names(&s).contains(&"track.mute"), "{:?}", names(&s));
        // Upper case matches too.
        // (`take.undo-lap` holds "undo" earlier in its name than `view.zoom.undo` does.)
        s.palette.as_mut().unwrap().set_query("UNDO".into());
        assert_eq!(names(&s)[..2], ["history.undo", "take.undo-lap"]);

        // Backspace takes a letter off and the list grows again.
        s.palette.as_mut().unwrap().set_query("undox".into());
        assert!(names(&s).is_empty());
        assert!(s.palette_key(&g, "backspace", Mods::NONE));
        assert_eq!(names(&s)[..2], ["history.undo", "take.undo-lap"]);
        s.palette.as_mut().unwrap().set_query("zzzz".into());
        assert!(names(&s).is_empty());
        assert!(!s.run_selected(&g));
        assert!(s.palette.is_some());
    }

    #[test]
    fn studio_pro_names_of_the_tools_find_them() {
        let (mut s, g) = setup();
        s.open_palette(&g);
        for (word, tool) in [
            ("矢印", "tool.select"),
            ("ペイント", "tool.draw"),
            ("リッスン", "tool.listen"),
        ] {
            s.palette.as_mut().unwrap().set_query(word.into());
            assert_eq!(names(&s).first(), Some(&tool), "{word}: {:?}", names(&s));
        }
    }

    #[test]
    fn its_rows_come_from_the_registry_once_each() {
        for (mut s, g) in [setup(), picked()] {
            let rows = s.palette_rows(&g);
            let mut seen = std::collections::BTreeSet::new();
            for r in rows.iter().filter(|r| !r.name.is_empty()) {
                assert!(seen.insert(r.name), "{} twice", r.name);
                let def = s.song().registry().get(r.name).expect("in the registry");
                assert_eq!(Some(r.label.as_str()), def.label());
                if r.way == Way::Settings {
                    assert_eq!(r.key.as_deref(), Some(SETTINGS.1));
                }
                if r.name == "transport.play" {
                    assert_eq!(r.key.as_deref(), Some("Space / MediaPlayPause"));
                }
                if r.name == "project.save-as" {
                    assert!(r.key.is_none());
                }
            }
            // Every key of the palette is a command of the registry.
            for k in SCOPES {
                assert!(s.song().registry().get(k.0).is_some(), "{}", k.0);
            }
        }
    }

    /// UI-087: a row is named as a menu names it, by the registry's short
    /// name, and every command of the session's registry has one.
    #[test]
    fn rows_are_named_as_a_menu_names_them() {
        let (mut s, g) = picked();
        for def in s.song().registry().all() {
            assert!(def.label().is_some(), "{} has no name", def.name);
        }
        let rows = s.palette_rows(&g);
        let label = |n: &str| rows.iter().find(|r| r.name == n).map(|r| r.label.as_str());
        for (n, l) in [
            ("transport.play", "再生 / 一時停止"),
            ("transport.stop", "停止"),
            ("record.start", "録音"),
            ("transport.loop", "ループ"),
            ("note.quantize", "ノートをクオンタイズ"),
            ("clip.split", "クリップを分割"),
            ("history.undo", "元に戻す"),
            ("project.save-as", "名前を付けて保存…"),
            ("export.mixdown", "ミックスダウンを書き出す…"),
        ] {
            assert_eq!(label(n), Some(l), "{n}");
        }
        for r in &rows {
            assert!(
                !r.label.ends_with("ます") && !r.label.contains('。'),
                "{}",
                r.label
            );
        }
    }

    /// The width a test gives a text: about what the UI font draws.
    fn measure(s: &str, size: FontSize, _: Weight) -> f32 {
        s.chars()
            .map(|c| if c.is_ascii() { 0.6 } else { 1.0 })
            .sum::<f32>()
            * size.px()
    }

    /// KEY-007: every operation with a key is listed with that key, and its
    /// chip sits in its row, after the name and before the operation's name.
    #[test]
    fn every_key_shows_its_chip_in_its_row() {
        for (mut s, g) in [setup(), picked()] {
            s.open_palette(&g);
            let p = s.palette.as_ref().unwrap();
            for row in p.rows.iter().filter(|r| r.key.is_some()) {
                let name = row.name;
                let r = Rect::new(
                    0.0,
                    0.0,
                    Palette::W - Palette::LIST_PAD * 2.0,
                    Palette::ROW_H,
                );
                let parts = row_parts(row, r, measure);
                let chip = parts.chip.unwrap_or_else(|| panic!("{name} has no chip"));
                assert!(chip.w > 0.0 && chip.h > 0.0, "{name}");
                assert!(chip.x >= parts.label.right(), "{name}: {chip:?} {parts:?}");
                assert!(chip.right() <= parts.op.x, "{name}: {chip:?} {parts:?}");
                assert!(chip.y >= r.y && chip.bottom() <= r.bottom(), "{name}");
                assert!(parts.label.w > 0.0, "{name}");
            }
            // A row without a key has no chip.
            let row = p.rows.iter().find(|r| r.key.is_none()).unwrap();
            assert_eq!(
                row_parts(row, Rect::new(0.0, 0.0, 592.0, 36.0), measure).chip,
                None
            );
        }
        // A name too long for the row is cut short; the chip stays in.
        let long = Row {
            name: "note.quantize",
            label: "ノート".repeat(40),
            key: Some("Ctrl+Shift+Z".into()),
            icon: "grid",
            enabled: true,
            reason: String::new(),
            way: Way::Plain(json!({})),
        };
        let r = Rect::new(0.0, 0.0, 592.0, 36.0);
        let parts = row_parts(&long, r, measure);
        let chip = parts.chip.unwrap();
        assert!(parts.label.right() <= chip.x && chip.right() <= parts.op.x);
    }

    /// With nothing typed, the list starts as § 05 of the mock does: the
    /// transport, then the edits; the rest after.
    #[test]
    fn with_nothing_typed_the_list_starts_as_the_mock_does() {
        let (mut s, g) = picked();
        for n in FIRST {
            assert!(s.song().registry().get(n).is_some(), "{n}");
        }
        s.transport.playing = true;
        s.open_palette(&g);
        let first: Vec<&str> = names(&s).into_iter().take(4).collect();
        assert_eq!(
            first,
            [
                "transport.play",
                "transport.stop",
                "transport.loop",
                "transport.seek"
            ]
        );
        // The rows that can run keep the order of FIRST, and come before
        // every other row.
        let p = s.palette.as_ref().unwrap();
        let on: Vec<&str> = p.hits()[..p.on()].iter().map(|&i| p.rows[i].name).collect();
        let places: Vec<usize> = on.iter().map(|n| place_of(n)).collect();
        assert!(places.windows(2).all(|w| w[0] <= w[1]), "{on:?}");
        assert!(on.contains(&"note.quantize") && on.contains(&"track.mute"));
        // Typed: a name that starts with it comes before one that has it.
        s.palette.as_mut().unwrap().set_query("保存".into());
        assert_eq!(names(&s)[0], "project.save");
        s.palette.as_mut().unwrap().set_query("mute".into());
        assert_eq!(names(&s)[0], "track.mute");
    }

    /// What cannot run is listed apart under its heading, and neither the
    /// arrows, Enter, the pointer nor a click picks it.
    #[test]
    fn rows_that_cannot_run_are_apart_and_cannot_be_picked() {
        let (mut s, g) = setup();
        s.selected_row = None;
        s.selected_clip = None;
        s.arrange.clips.clear();
        s.open_palette(&g);
        // 「履歴」: the history's pop-over can open; undo and redo cannot run.
        s.palette.as_mut().unwrap().set_query("history".into());
        let p = s.palette.as_ref().unwrap();
        let hits = p.hits();
        let on = p.on();
        assert!(on >= 1 && on < hits.len(), "{on} of {}", hits.len());
        assert!(hits[..on].iter().all(|&i| p.rows[i].enabled));
        assert!(hits[on..].iter().all(|&i| !p.rows[i].enabled));
        assert!(names(&s).contains(&"history.undo"));
        let l = p.layout(g.size);
        let head = l.head.expect("the heading");
        let (_, last_on) = l.rows[on - 1];
        let (_, first_off) = l.rows[on];
        assert_eq!(head.y, last_on.bottom());
        assert_eq!(first_off.y, head.bottom());
        // They sit on a band of their own, from the heading to the list's end.
        let off = l.off.expect("the band");
        assert_eq!((off.y, off.x, off.w), (head.y, head.x, head.w));
        assert!(off.bottom() >= first_off.bottom().min(l.list.bottom() - Palette::LIST_PAD));
        assert!(off.bottom() <= l.list.bottom() - Palette::LIST_PAD + 0.01);
        assert!(!off.contains(last_on.x + 1.0, last_on.y + 1.0));

        // The arrows stop at the last row that can run.
        for _ in 0..hits.len() + 2 {
            s.palette_key(&g, "down", Mods::NONE);
        }
        assert_eq!(s.palette.as_ref().unwrap().selected, on - 1);
        // The pointer does not light it, and a click does nothing.
        let steps = s.song().steps();
        assert!(s.palette_hover(&g, first_off.x + 10.0, first_off.y + 10.0));
        assert_eq!(s.palette.as_ref().unwrap().hover, Some(on));
        assert!(s.palette_press(&g, first_off.x + 10.0, first_off.y + 10.0));
        assert!(s.palette.is_some());
        assert_eq!(s.song().steps(), steps);
        // Only what cannot run is found: nothing is picked, Enter does nothing.
        s.palette.as_mut().unwrap().set_query("history.undo".into());
        assert_eq!(s.palette.as_ref().unwrap().on(), 0);
        assert!(s.palette.as_ref().unwrap().current().is_none());
        assert!(s.palette_key(&g, "enter", Mods::NONE));
        assert!(s.palette.is_some());
        assert_eq!(s.song().steps(), steps);
    }

    /// The field takes Japanese the way the window's IME gives it: composed
    /// text is shown but not searched for, the result is put in once.
    #[test]
    fn the_field_takes_japanese_from_the_ime() {
        let (mut s, g) = picked();
        s.open_palette(&g);
        let all = names(&s).len();
        let p = s.palette.as_mut().unwrap();
        p.mark_text(None, "く");
        p.mark_text(None, "くおん");
        assert_eq!((p.query.as_str(), p.marked.as_str()), ("", "くおん"));
        assert_eq!(p.marked_range(), Some(0..3));
        assert_eq!(p.selected_range(), 3..3);
        assert_eq!(p.text_for_range(0..3), ("くおん".to_string(), 0..3));
        assert_eq!(names(&s).len(), all, "composing does not filter");

        // While composing, the keys are the IME's: Enter, Escape and the
        // arrows do nothing to the palette.
        let steps = s.song().steps();
        for k in ["enter", "escape", "down", "backspace"] {
            assert!(s.palette_key(&g, k, Mods::NONE));
        }
        let p = s.palette.as_mut().expect("still open");
        assert_eq!((p.selected, p.marked.as_str()), (0, "くおん"));
        assert_eq!(s.song().steps(), steps);

        // The result goes in for good, in place of what was composed.
        let p = s.palette.as_mut().unwrap();
        p.mark_text(None, "クオンタイズ");
        p.replace_text(None, "クオンタイズ");
        assert_eq!((p.query.as_str(), p.marked.as_str()), ("クオンタイズ", ""));
        assert_eq!(p.marked_range(), None);
        assert!(names(&s).contains(&"note.quantize"));
        // Backspace takes a letter off again, now that nothing is composed.
        assert!(s.palette_key(&g, "backspace", Mods::NONE));
        assert_eq!(s.palette.as_ref().unwrap().query, "クオンタイ");

        // Composing called off: what was composed goes, the query stays.
        let p = s.palette.as_mut().unwrap();
        p.mark_text(None, "ず");
        p.replace_text(None, "");
        assert_eq!((p.query.as_str(), p.marked.as_str()), ("クオンタイ", ""));
        p.mark_text(None, "ず");
        p.mark_text(None, "");
        assert_eq!((p.query.as_str(), p.marked.as_str()), ("クオンタイ", ""));
        // Let go of: what was composed stays as typed.
        p.mark_text(None, "ズ");
        p.unmark();
        assert_eq!((p.query.as_str(), p.marked.as_str()), ("クオンタイズ", ""));

        // A key's letter comes once, through the IME's door only.
        let p = s.palette.as_mut().unwrap();
        p.set_query(String::new());
        type_in(&mut s, &g, "mute");
        assert_eq!(s.palette.as_ref().unwrap().query, "mute");

        // Offsets are UTF-16: 𝄞 is two.
        let p = s.palette.as_mut().unwrap();
        p.set_query(String::new());
        p.replace_text(None, "a𝄞b\n");
        assert_eq!(p.query, "a𝄞b", "control characters are not put in");
        assert_eq!(p.selected_range(), 4..4);
        assert_eq!(p.text_for_range(1..3), ("𝄞".to_string(), 1..3));
        let (t, r) = p.text_for_range(2..9);
        assert_eq!((t.as_str(), r), ("b", 3..4));
        // A range given replaces just that part.
        p.replace_text(Some(1..3), "x");
        assert_eq!(p.query, "axb");
    }

    /// The IME's window sits by the caret: the bounds of a range of the
    /// field are where its text is drawn.
    #[test]
    fn the_ime_window_goes_by_the_caret() {
        let (mut s, g) = setup();
        s.open_palette(&g);
        let p = s.palette.as_mut().unwrap();
        p.replace_text(None, "保存");
        p.mark_text(None, "ばーじょん");
        let search = p.layout(g.size).search;
        let field = field_of(search);
        let w = |t: &str| t.chars().count() as f32 * 10.0;
        let at = p.text_bounds(search, p.selected_range(), w);
        assert_eq!(at.x, field.x + 70.0);
        let marked = p.text_bounds(search, p.marked_range().unwrap(), w);
        assert_eq!((marked.x, marked.w), (field.x + 20.0, 50.0));
        assert!(search.contains(marked.x, marked.y) && marked.bottom() <= search.bottom());
    }

    /// The panel floats with the theme's soft shadow all round it (UI-007).
    #[test]
    fn the_panel_has_the_float_shadow() {
        for theme in xirius_design::Theme::ALL {
            let c = crate::paint::Colors::new(theme);
            let layers = xirius_design::FloatShadow::layers(theme);
            for (s, l) in c.float.iter().zip(layers) {
                assert_eq!(f32::from(s.blur_radius), l.blur);
                assert_eq!(f32::from(s.offset.y), l.offset_y);
                assert_eq!(f32::from(s.offset.x), 0.0);
                assert!(!s.inset && s.color.a > 0.0);
            }
        }
    }

    #[test]
    fn what_cannot_run_here_is_off_or_not_listed() {
        let (mut s, g) = setup();
        s.selected_row = None;
        s.selected_clip = None;
        s.arrange.clips.clear();
        s.tab = crate::studio::DrawerTab::Mixer;
        let rows = s.palette_rows(&g);
        let on = |n: &str| rows.iter().find(|r| r.name == n).map(|r| r.enabled);
        // Nothing picked: what acts on a selection is listed but off.
        for n in ["track.mute", "track.delete", "clip.delete", "note.quantize"] {
            assert_eq!(on(n), Some(false), "{n}");
        }
        // Nothing done yet, nothing playing, no take, no crashed plugin.
        for n in [
            "history.undo",
            "history.redo",
            "transport.stop",
            "record.stop",
            "plugin.restart",
        ] {
            assert_eq!(on(n), Some(false), "{n}");
        }
        // The song is not saved yet: a version cannot be named.
        assert_eq!(on("version.save"), Some(false));
        for n in [
            "project.save",
            "transport.play",
            "track.add.audio",
            "view.snap",
        ] {
            assert_eq!(on(n), Some(true), "{n}");
        }
        // Commands that need values open a form in the palette.
        for n in ["track.move", "clip.move", "clip.resize", "track.rename"] {
            assert_eq!(on(n), Some(false), "{n}");
        }
        assert_eq!(on("tempo.set"), Some(true));
        assert_eq!(on("project.get"), Some(true));
        // What would change nothing is off: there is no loop to clear yet.
        assert_eq!(on("loop.clear"), Some(false));
        s.song
            .run("loop.set", json!({"start": 0, "end": 1920}))
            .unwrap();
        let clear = row(&mut s, &g, "loop.clear");
        assert!(clear.enabled);
        assert!(s.run_row(&g, &clear));
        assert!(s.project().loop_range.is_none());
    }

    #[test]
    fn an_off_row_does_not_run() {
        let (mut s, g) = setup();
        s.open_palette(&g);
        let undo = row(&mut s, &g, "history.undo");
        assert!(!undo.enabled);
        let steps = s.song().steps();
        let before = state(&s);
        assert!(!s.run_row(&g, &undo));
        assert_eq!(s.song().steps(), steps);
        assert_eq!(state(&s), before);
        // Enter on an off row leaves the palette open.
        s.palette.as_mut().unwrap().set_query("history.undo".into());
        assert!(s.palette_key(&g, "enter", Mods::NONE));
        assert!(s.palette.is_some());
    }

    #[test]
    fn quantize_from_the_palette_is_one_step_and_one_undo_takes_it_back() {
        let (mut s, g) = picked();
        let before = state(&s);
        let steps = s.song().steps();
        s.open_palette(&g);
        type_in(&mut s, &g, "ノートをクオンタイズ");
        assert_eq!(names(&s), ["note.quantize"]);
        assert!(s.palette_key(&g, "enter", Mods::NONE));
        assert!(s.palette.is_none(), "it closes after running");
        assert!(s.error.is_none(), "{:?}", s.error);
        assert_eq!(s.song().steps(), steps + 1);
        assert_eq!(last_command(&s), "note.quantize");
        assert_ne!(state(&s), before);
        // Ctrl+Z, the same undo as for the key.
        assert!(s.edit_key(&g, "z", Mods::ctrl()));
        assert_eq!(state(&s), before);
    }

    #[test]
    fn a_click_on_a_row_runs_it_and_outside_closes() {
        let (mut s, g) = picked();
        // Undo here must land exactly on the state the test kept; the
        // instrument-input arm that would follow the selection is not
        // what this test checks (REC-024).
        s.rec_set(
            "prefs.set",
            serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
        );
        let before = state(&s);
        s.open_palette(&g);
        s.palette.as_mut().unwrap().set_query("track.mute".into());
        let l = s.palette.as_ref().unwrap().layout(g.size);
        let (i, r) = l.rows[0];
        assert_eq!(i, 0);
        assert!(s.palette_hover(&g, r.x + 10.0, r.y + 10.0));
        assert_eq!(s.palette.as_ref().unwrap().hover, Some(0));
        assert!(s.palette_press(&g, r.x + 10.0, r.y + 10.0));
        assert!(s.palette.is_none());
        assert_eq!(last_command(&s), "track.mute");
        assert!(s.undo(&g));
        assert_eq!(state(&s), before);

        s.open_palette(&g);
        let l = s.palette.as_ref().unwrap().layout(g.size);
        // Inside the box but not on a row: it stays.
        assert!(s.palette_press(&g, l.search.x + 4.0, l.search.y + 4.0));
        assert!(s.palette.is_some());
        assert!(s.palette_press(&g, 4.0, g.size.1 - 4.0));
        assert!(s.palette.is_none());
        assert_eq!(state(&s), before);
    }

    #[test]
    fn up_down_pick_a_row_and_the_list_scrolls_to_it() {
        let (mut s, g) = picked();
        s.open_palette(&g);
        let p = s.palette.as_ref().unwrap();
        // Only the rows that can run are picked.
        let n = p.on();
        assert!(n < p.hits().len());
        assert_eq!(p.selected, 0);
        assert!(s.palette_key(&g, "up", Mods::NONE));
        assert_eq!(s.palette.as_ref().unwrap().selected, 0);
        for _ in 0..n + 3 {
            s.palette_key(&g, "down", Mods::NONE);
        }
        let p = s.palette.as_ref().unwrap();
        assert_eq!(p.selected, n - 1);
        let l = p.layout(g.size);
        let (_, r) = l.rows.iter().find(|(i, _)| *i == n - 1).copied().unwrap();
        assert!(r.bottom() <= l.list.bottom() + 0.5, "{r:?} {:?}", l.list);
        // The wheel over the list scrolls; outside it does not.
        let top = s.palette.as_ref().unwrap().offset;
        s.palette_wheel(&g, l.list.x + 5.0, l.list.y + 5.0, 120.0);
        assert!(s.palette.as_ref().unwrap().offset < top);
        let o = s.palette.as_ref().unwrap().offset;
        s.palette_wheel(&g, 2.0, 2.0, -120.0);
        assert_eq!(s.palette.as_ref().unwrap().offset, o);
    }

    /// Every edit row that is on runs as one step, the step is its command,
    /// and one undo puts the song back.
    #[test]
    fn every_edit_row_that_is_on_is_one_step_and_one_undo() {
        let (mut s, g) = picked();
        // Undo here must land exactly on the state the test kept; the
        // instrument-input arm that would follow the selection is not
        // what this test checks (REC-024).
        s.rec_set(
            "prefs.set",
            serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
        );
        let rows = s.palette_rows(&g);
        let plain: Vec<&str> = rows
            .iter()
            .filter(|r| {
                r.enabled
                    && matches!(r.way, Way::Plain(_))
                    && s.song.registry().get(r.name).is_some_and(|def| def.history)
            })
            .map(|r| r.name)
            .collect();
        for n in ["track.mute", "track.solo", "track.delete", "clip.split"] {
            assert!(plain.contains(&n), "{n} in {plain:?}");
        }
        for name in plain {
            let (mut s, g) = picked();
            s.rec_set(
                "prefs.set",
                serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
            );
            let before = state(&s);
            let steps = s.song().steps();
            let r = row(&mut s, &g, name);
            assert!(s.run_row(&g, &r), "{name}");
            assert!(s.error.is_none(), "{name}: {:?}", s.error);
            assert_eq!(s.song().steps(), steps + 1, "{name}");
            assert_eq!(last_command(&s), name);
            assert!(s.undo(&g), "{name}");
            assert_eq!(state(&s), before, "{name}");
        }
    }

    /// KEY-007: the key shown on a row is the key that runs its command,
    /// and the row runs it by that key's path.
    #[test]
    fn a_row_runs_by_its_key_and_shows_that_key() {
        let skip = [
            "history.undo",
            "history.redo",
            "project.save",
            "project.save-as",
            "project.open",
        ];
        let (mut s, g) = picked();
        let rows = s.palette_rows(&g);
        let keyed: Vec<&str> = rows
            .iter()
            .filter(|r| {
                r.enabled
                    && matches!(&r.way, Way::Key(scope) if matches!(scope, Scope::Roll | Scope::Arrange))
            })
            .map(|r| r.name)
            .collect();
        assert!(keyed.len() >= 7, "{keyed:?}");
        for name in keyed {
            assert!(!skip.contains(&name));
            let (mut s, g) = picked();
            let before = state(&s);
            let r = row(&mut s, &g, name);
            assert!(s.run_row(&g, &r), "{name}");
            assert!(s.error.is_none(), "{name}: {:?}", s.error);
            assert_eq!(last_command(&s), name, "{name}");
            assert!(s.edit_key(&g, "z", Mods::ctrl()));
            assert_eq!(state(&s), before, "{name}");

            // The key itself, pressed in the same place, does the same.
            let (mut t, g) = picked();
            let scope = scope_of(name).unwrap();
            t.arrange.focus = scope == Scope::Arrange;
            let context = if scope == Scope::Arrange {
                "arrange"
            } else {
                "roll"
            };
            let binding = xirius_prefs::catalog::ENTRIES
                .iter()
                .find(|e| {
                    e.context == context
                        && (e.command.split_whitespace().next() == Some(name)
                            || name == "note.move" && e.command == "edit.nudge.right")
                })
                .expect("preset binding");
            let k = xirius_prefs::Chord::parse(binding.keys[0], xirius_prefs::Os::Win).unwrap();
            let mods = Mods {
                ctrl: k.mods & xirius_prefs::chord::MOD != 0,
                alt: k.mods & xirius_prefs::chord::ALT != 0,
                shift: k.mods & xirius_prefs::chord::SHIFT != 0,
            };
            assert!(t.edit_key(&g, &k.key.to_lowercase(), mods), "{name}");
            assert_eq!(last_command(&t), name, "{name} by its key");
        }
        // Undo and redo by their rows.
        let (mut s, g) = picked();
        let before = state(&s);
        let r = row(&mut s, &g, "track.mute");
        s.run_row(&g, &r);
        let after = state(&s);
        let undo = row(&mut s, &g, "history.undo");
        assert!(undo.enabled && s.run_row(&g, &undo));
        assert_eq!(state(&s), before);
        let redo = row(&mut s, &g, "history.redo");
        assert!(redo.enabled && s.run_row(&g, &redo));
        assert_eq!(state(&s), after);
    }

    /// The roll's menu and the palette show the same key for the same work.
    #[test]
    fn the_keys_match_the_note_menu() {
        let (mut s, g) = picked();
        let c = s.roll_ctx(&g).unwrap();
        let n = c.notes()[0];
        let v = s.roll.view(&c);
        assert!(s.scene_context(&g, v.x(n.start) + 6.0, v.y(n.pitch) + v.row_h / 2.0));
        let rows = s.context_rows();
        for (label, name) in [
            ("ノートを複製", "note.duplicate"),
            ("ノートをミュート", "note.mute"),
            ("ノートを削除", "note.delete"),
        ] {
            let row = rows.iter().find(|r| r.label == label).unwrap();
            assert_eq!(row.key, s.menu_key(name), "{name}");
            assert_eq!(scope_of(name), Some(Scope::Roll));
        }
    }

    #[test]
    fn the_transport_keys_are_those_of_the_transport() {
        let (mut s, g) = setup();
        // The row turns the loop on as `/` does, and `/` turns it off again.
        let looped = s.transport.looping;
        let r = row(&mut s, &g, "transport.loop");
        assert!(s.run_row(&g, &r));
        assert_ne!(s.transport.looping, looped);
        assert!(s.edit_key(&g, "/", Mods::NONE));
        assert_eq!(s.transport.looping, looped);
        // Home goes to the start.
        s.playhead = 1920;
        let r = row(&mut s, &g, "transport.seek");
        assert!(s.run_row(&g, &r));
        assert_eq!(s.playhead, 0);
    }

    #[test]
    fn save_without_a_folder_asks_where_and_versions_open_their_pop_over() {
        let (mut s, g) = setup();
        let r = row(&mut s, &g, "project.save");
        assert!(r.enabled && s.run_row(&g, &r));
        assert_eq!(s.file_request, Some(FileRequest::SaveAs));
        s.file_request = None;
        let r = row(&mut s, &g, "project.open");
        assert!(s.run_row(&g, &r));
        assert_eq!(s.file_request, Some(FileRequest::Open));

        let r = row(&mut s, &g, "version.list");
        assert!(s.run_row(&g, &r));
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Versions));
        let r = row(&mut s, &g, "history.list");
        s.popup = None;
        assert!(s.run_row(&g, &r));
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::History));
    }

    #[test]
    fn menu_rows_go_through_their_menus() {
        let (mut s, g) = setup();
        let tracks = s.project().tracks.len();
        let steps = s.song().steps();
        let r = row(&mut s, &g, "track.add.audio");
        assert!(r.enabled && s.run_row(&g, &r));
        assert_eq!(s.project().tracks.len(), tracks + 1);
        assert_eq!(s.song().steps(), steps + 1);
        assert_eq!(last_command(&s), "track.add.audio");

        // The grid switch flips the snap and leaves no pop-over.
        let snap = s.song().snap();
        let r = row(&mut s, &g, "view.snap");
        assert!(s.run_row(&g, &r));
        assert_ne!(s.song().snap(), snap);
        assert!(s.popup.is_none());

        // Export opens its panel in the asked mode.
        let r = row(&mut s, &g, "export.stems");
        assert!(s.run_row(&g, &r));
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Export));
    }

    /// The picture run without a window: each picture shows what its name says.
    #[test]
    fn the_picture_run_does_what_its_names_say() {
        use crate::shot::{Act, Target};
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 860.0 - Layout::TOP_BAR_H);
        s.resized(&g);
        let keys = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        for pic in SHOTS {
            for act in pic.acts {
                match *act {
                    Act::Stage(_, stage) => shot(&mut s, &g, stage),
                    Act::Press(Target::Top(b)) => {
                        s.press(&g, Hit::Bar(b));
                    }
                    // As the window does: the palette first, then the keys.
                    Act::Key(k) => {
                        let mods = Mods {
                            ctrl: k.contains("ctrl-"),
                            shift: false,
                            alt: false,
                        };
                        let key = k.rsplit('-').next().unwrap();
                        if !s.palette_key(&g, key, mods) {
                            s.edit_key(&g, key, mods);
                        } else if !mods.ctrl
                            && key.chars().count() == 1
                            && let Some(p) = &mut s.palette
                        {
                            p.replace_text(None, key);
                        }
                    }
                    other => panic!("{other:?}"),
                }
            }
            assert!(s.error.is_none(), "{}: {:?}", pic.name, s.error);
            let p = s.palette.as_ref();
            match pic.name {
                "00-base" | "09-closed" => assert!(p.is_none(), "{}", pic.name),
                "01-open-from-bar" => {
                    for n in ["track.mute", "note.quantize", "clip.split"] {
                        assert!(
                            p.unwrap().rows.iter().any(|r| r.name == n && r.enabled),
                            "{n}"
                        );
                    }
                }
                "02-arrow-down" => {
                    assert_eq!(p.unwrap().selected, 3);
                    assert_eq!(p.unwrap().hover, Some(1));
                }
                "03-filter-mute" => assert_eq!(names(&s)[0], "track.mute"),
                "04-ran-track-mute" => {
                    assert!(p.is_none());
                    assert!(s.project().tracks[&keys].mute);
                }
                "05-ctrl-k-undo" => assert_eq!(names(&s)[..2], ["history.undo", "take.undo-lap"]),
                "06-undone" => assert!(!s.project().tracks[&keys].mute),
                "07-filter-note" => assert!(names(&s).contains(&"note.quantize")),
                "08-not-found" => assert!(names(&s).is_empty()),
                "10-nothing-picked" => {
                    let mute = p.unwrap().rows.iter().find(|r| r.name == "track.mute");
                    assert!(!mute.unwrap().enabled);
                }
                "11-scrolled" => {
                    let p = p.unwrap();
                    assert!(p.offset > 0.0);
                    let l = p.layout(g.size);
                    assert!(l.head.is_some());
                    assert!(l.rows.iter().any(|(i, _)| *i < p.on()));
                    assert!(l.rows.iter().any(|(i, _)| *i >= p.on()));
                }
                "12-ime-composing" => {
                    let p = p.unwrap();
                    assert_eq!((p.query.as_str(), p.marked.as_str()), ("", "ほぞん"));
                    assert_eq!(p.hits().len(), p.rows.len());
                }
                "13-ime-committed" => {
                    let p = p.unwrap();
                    assert_eq!((p.query.as_str(), p.marked.as_str()), ("保存", ""));
                    assert_eq!(names(&s)[0], "project.save");
                    assert!(names(&s).contains(&"version.save"));
                    assert!(p.on() < p.hits().len());
                }
                other => panic!("{other}"),
            }
        }
    }
    #[test]
    fn large_result_cache_reuses_wraps_until_width_or_result_changes_and_copies_all() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 820.0);
        s.palette_query(&g, "project.get", json!({}));
        let p = s.palette.as_mut().unwrap();
        let fields = (0..10000)
            .map(|i| crate::palette_args::Field {
                name: "value",
                label: format!("結果{i}"),
                value: "WMi日本語".into(),
                choices: vec![],
            })
            .collect();
        p.replace_result_fields(fields);
        let first = p.result_rows(592.0).unwrap();
        for i in 0..50 {
            p.offset = i as f32 * 100.0;
            assert!(p.layout(g.size).rows.len() <= 10);
            assert!(std::rc::Rc::ptr_eq(&first, &p.result_rows(592.0).unwrap()));
        }
        p.result_all = true;
        let copied = p.selected_result_text().unwrap();
        assert_eq!(copied.lines().count(), 10000);
        assert!(copied.ends_with("結果9999: WMi日本語"));
        assert!(!std::rc::Rc::ptr_eq(&first, &p.result_rows(300.0).unwrap()));
        p.replace_result_fields(Vec::new());
        assert_eq!(p.result_rows(592.0).unwrap().total(), 0.0);
    }
}
