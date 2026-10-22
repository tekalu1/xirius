//! The fields at the head of a clip's menu (EDT-007, EDT-016, UI-311): an audio clip's tempo,
//! speed, transpose and fine tune, and a MIDI clip's transpose. A press puts the keys in a
//! field; digits type a new value and Enter hands it to the command, ↑↓ (Shift: further) change
//! it at once, and Tab goes to the next field. The menu stays open, so the clip changes under
//! it. Clips whose values differ show 「—」, and a value typed there goes to all of them.
//!
//! The unit is not drawn beside the field (UI-311): the value carries it (「100%」) or is a
//! signed number (「+2」), and only what a screen reader says keeps the unit
//! (「トランスポーズ +2 半音」).
//!
//! The rows are `Run::Field`; the value each shows is the song's, read again after each change.

use std::time::{Duration, Instant};

use serde_json::Value;
use xirius_core::id::ClipId;
use xirius_core::model::{AudioProcess, ClipBody, Project};
use xirius_design::{FontSize, Radius, Weight};

use super::{Item, Run};
use crate::draw::{Align, Pen};
use crate::popup::{Action, Cell, Look};
use crate::studio::{Geometry, Rect, Studio};

/// How long a wheel keeps adding to the same undo step, and how long a list that is already
/// being scrolled keeps scrolling over a field (UI-313).
pub(crate) const WHEEL_GROUP_MS: u64 = 400;

/// The height of a field's row: the four sit closer than the menu's rows.
pub const ROW_H: f32 = 25.0;
/// The room a field's name takes: the longest name (「トランスポーズ」, 84 px at 12 px
/// Noto Sans JP) and the row's 8 px before it, as the mock has it (UI-311).
const LABEL_W: f32 = 92.0;
const FIELD_W: f32 = 64.0;
const BUTTON_W: f32 = 30.0;
const BUTTON_GAP: f32 = 4.0;
/// How wide a field's row wants the menu to be: the name and the field, and room for the
/// tempo's ×2 and ÷2 at the row's right (as the menu was before the name was widened).
pub const ROW_W: f32 = 196.0;
/// The most characters a field takes.
const LONGEST: usize = 7;

/// Which value a field shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Tempo,
    Speed,
    Semis,
    Cents,
    /// A track's transpose (UI-312).
    TrackSemis,
    /// A track's velocity, in percent (UI-312).
    TrackVelocity,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Tempo, Kind::Speed, Kind::Semis, Kind::Cents];
    /// The fields of the inspector's track (UI-312).
    pub const TRACK: [Kind; 2] = [Kind::TrackSemis, Kind::TrackVelocity];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Tempo => "テンポ",
            Kind::Speed => "速さ",
            Kind::Semis | Kind::TrackSemis => "トランスポーズ",
            Kind::Cents => "微調整",
            Kind::TrackVelocity => "ベロシティ",
        }
    }

    pub fn command(self) -> &'static str {
        match self {
            Kind::Tempo => "clip.tempo.set",
            Kind::Speed => "clip.speed.set",
            Kind::Semis => "clip.transpose",
            Kind::Cents => "clip.tune",
            Kind::TrackSemis => "track.transpose",
            Kind::TrackVelocity => "track.velocity",
        }
    }

    pub(crate) fn arg(self) -> &'static str {
        match self {
            Kind::Tempo => "bpm",
            Kind::Speed => "percent",
            Kind::Semis | Kind::TrackSemis => "semis",
            Kind::Cents => "cents",
            Kind::TrackVelocity => "percent",
        }
    }

    pub(crate) fn range(self) -> (f64, f64) {
        match self {
            Kind::Tempo => (1.0, 999.0),
            Kind::Speed => (AudioProcess::MIN_SPEED, AudioProcess::MAX_SPEED),
            Kind::Semis => {
                let s = f64::from(AudioProcess::SEMIS);
                (-s, s)
            }
            Kind::Cents => {
                let c = f64::from(AudioProcess::CENTS);
                (-c, c)
            }
            Kind::TrackSemis => {
                let s = f64::from(xirius_core::model::Track::SEMIS);
                (-s, s)
            }
            Kind::TrackVelocity => {
                let v = f64::from(xirius_core::model::Track::VELOCITY);
                (-v, v)
            }
        }
    }

    /// Whole numbers only, with a sign.
    pub(crate) fn signed(self) -> bool {
        !matches!(self, Kind::Tempo | Kind::Speed)
    }

    /// What ↑↓ add, and with Shift.
    pub(crate) fn step(self, shift: bool) -> f64 {
        match (self, shift) {
            (_, false) => 1.0,
            (Kind::Semis | Kind::TrackSemis, true) => 12.0,
            (_, true) => 10.0,
        }
    }

    /// What one notch of the wheel adds (UI-313): Shift is the arrows' further step, and Ctrl
    /// is finer for the fields that keep a fraction (the tempo and the speed).
    pub(crate) fn wheel_step(self, shift: bool, ctrl: bool) -> f64 {
        if ctrl && !shift && matches!(self, Kind::Tempo | Kind::Speed) {
            0.1
        } else {
            self.step(shift)
        }
    }

    /// The value as the field shows it; 「—」 when the clips differ (or the tempo is not known).
    pub fn text(self, v: Option<f64>) -> String {
        let Some(v) = v else {
            return "—".to_string();
        };
        match self {
            Kind::Tempo => format!("{v:.2}"),
            Kind::Speed if v.fract() == 0.0 => format!("{v:.0}%"),
            Kind::Speed => format!("{v:.1}%"),
            Kind::TrackVelocity => {
                let n = v.round() as i64;
                match n {
                    0 => "0%".to_string(),
                    n if n > 0 => format!("+{n}%"),
                    n => format!("−{}%", -n),
                }
            }
            Kind::Semis | Kind::Cents | Kind::TrackSemis => match v.round() as i64 {
                0 => "0".to_string(),
                n if n > 0 => format!("+{n}"),
                n => format!("−{}", -n),
            },
        }
    }

    /// What a screen reader says for the field `text` shows: the name, the value and the unit
    /// (the unit is not drawn beside the field, UI-311).
    pub fn spoken(self, text: &str) -> String {
        match self {
            Kind::Tempo => format!("{} {text} BPM", self.label()),
            Kind::Speed => format!("{} {text}", self.label()),
            Kind::Semis | Kind::TrackSemis => format!("{} {text} 半音", self.label()),
            Kind::Cents => format!("{} {text} セント", self.label()),
            Kind::TrackVelocity => format!("{} {text}", self.label()),
        }
    }

    fn of(self, x: &AudioProcess) -> Option<f64> {
        match self {
            Kind::Tempo => x.tempo,
            Kind::Speed => Some(x.speed),
            Kind::Semis => Some(f64::from(x.semis)),
            Kind::Cents => Some(f64::from(x.cents)),
            Kind::TrackSemis | Kind::TrackVelocity => None,
        }
    }
}

/// The value the clips `ids` share, or `None` when they differ.
pub fn shared(p: &Project, ids: &[ClipId], kind: Kind) -> Option<f64> {
    let mut shared: Option<f64> = None;
    for id in ids {
        let (_, clip) = p.clip(*id).ok()?;
        let v = match (&clip.body, kind) {
            (ClipBody::Audio { process, .. }, _) => kind.of(process)?,
            // A MIDI clip's own transpose is the only value it has (UI-311).
            (ClipBody::Midi { transpose, .. }, Kind::Semis) => f64::from(*transpose),
            (ClipBody::Midi { .. }, _) => return None,
        };
        match shared {
            Some(s) if (s - v).abs() > 1e-9 => return None,
            _ => shared = Some(v),
        }
    }
    shared
}

/// The half-width form of a character an IME may compose (UI-313): the full-width digits,
/// the two signs, the long sound mark and the full-width point.
pub(crate) fn halfwidth(c: char) -> char {
    match c {
        '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
        '＋' => '+',
        '－' | 'ー' | '−' => '-',
        '．' => '.',
        c => c,
    }
}

/// A wheel's turn, gathered into whole notches (UI-313). Turning back throws away what was
/// gathered the other way, as the waveform zoom does (UI-310).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WheelTurn {
    turn: f32,
}

impl WheelTurn {
    /// The whole notches in `px` (60 px a notch): away from you is positive.
    pub fn notches(&mut self, px: f32) -> i32 {
        let notches = px / crate::screen::ZOOM_NOTCH_PX;
        if self.turn * notches < 0.0 {
            self.turn = 0.0;
        }
        self.turn += notches;
        let whole = self.turn.trunc();
        if whole != 0.0 {
            self.turn -= whole;
        }
        whole as i32
    }
}

/// One turn of the wheel and the undo step it adds to (UI-313): turns of the same field within
/// [`WHEEL_GROUP_MS`] are one step of the history.
#[derive(Clone, Debug)]
pub struct WheelGroup {
    field: (Kind, Vec<String>),
    at: Instant,
    open: bool,
}

impl WheelGroup {
    /// Whether `now` is still the same turn: the same field within the group's time.
    fn continues(&self, field: &(Kind, Vec<String>), now: Instant) -> bool {
        self.open
            && self.field == *field
            && now.duration_since(self.at) < Duration::from_millis(WHEEL_GROUP_MS)
    }
}

/// The field that has the keys: its row and what was typed in it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Typing {
    pub row: usize,
    pub text: String,
    /// Nothing was typed yet: the field shows its value, and the first key replaces it.
    pub fresh: bool,
    /// What the IME is still composing after `text` (UI-313).
    pub marked: String,
}

impl Typing {
    /// Whether the field takes `c` (after the IME's half-width forms).
    fn takes(kind: Kind, c: char) -> bool {
        match halfwidth(c) {
            '0'..='9' => true,
            '.' => !kind.signed(),
            '-' | '+' => kind.signed(),
            _ => false,
        }
    }

    pub(crate) fn put(&mut self, kind: Kind, c: char) -> bool {
        let c = halfwidth(c);
        // A character the field does not take leaves the value it shows: a space, or the
        // 「\n」 a simulated Enter types after it handed the value over.
        if !Self::takes(kind, c) {
            return false;
        }
        self.marked.clear();
        if self.fresh {
            self.text.clear();
            self.fresh = false;
        }
        match c {
            '0'..='9' if self.text.chars().count() < LONGEST => self.text.push(c),
            '.' if !self.text.contains('.') => self.text.push(c),
            '-' | '+' => {
                let digits = self.text.trim_start_matches(['-', '+']).to_string();
                self.text = if c == '-' {
                    format!("-{digits}")
                } else {
                    digits
                };
            }
            _ => return false,
        }
        true
    }

    /// Puts the IME's composition (`marked`) in the field; true when the field takes a character
    /// of it. The search box does not keep the composition then (UI-313).
    fn put_marked(&mut self, kind: Kind, text: &str) -> bool {
        let marked: String = text
            .chars()
            .map(halfwidth)
            .filter(|c| Self::takes(kind, *c))
            .take(LONGEST)
            .collect();
        if marked.is_empty() {
            // The IME took its composition back: the field keeps none of it (P2-2 of review 15).
            self.marked.clear();
            return false;
        }
        self.marked = marked;
        true
    }

    pub(crate) fn number(&self, kind: Kind) -> Option<f64> {
        let v: f64 = self.text.parse().ok()?;
        let (lo, hi) = kind.range();
        let v = v.clamp(lo, hi);
        Some(if kind.signed() { v.round() } else { v })
    }
}

/// What the row of a field shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub kind: Kind,
    pub text: String,
    /// The field has the keys.
    pub focused: bool,
    /// The value is known (×2 and ÷2 work on it).
    pub known: bool,
}

impl Shown {
    pub(super) fn of(kind: Kind, value: Option<f64>, typing: Option<&Typing>) -> Shown {
        let text = match typing {
            Some(t) if !t.fresh => format!("{}{}", t.text.replace('-', "−"), t.marked),
            Some(t) => format!("{}{}", kind.text(value), t.marked),
            _ => kind.text(value),
        };
        Shown {
            kind,
            text,
            focused: typing.is_some(),
            known: value.is_some(),
        }
    }
}

/// The parts of a field's row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldAct {
    /// The field's name: it gives the field the keys.
    Label,
    /// The field: it takes the keys.
    Focus,
    /// ×2 and ÷2 of the tempo.
    Double,
    Halve,
}

/// The row's parts: the name, the field, and for the tempo ×2 and ÷2 at the end.
pub fn cells(r: Rect, i: usize, shown: &Shown, enabled: bool) -> Vec<Cell> {
    let h = ROW_H;
    let field = Rect::new(r.x + LABEL_W, r.y + 1.0, FIELD_W, h - 2.0);
    let put = |cell: Cell, act: FieldAct| {
        if enabled {
            cell.act(Action::Field(i, act))
        } else {
            cell.off()
        }
    };
    let mut cells = vec![
        put(
            Cell::new(Rect::new(r.x, r.y, LABEL_W, h), Look::Note).text(shown.kind.label()),
            FieldAct::Label,
        ),
        put(
            Cell::new(field, Look::Field)
                .text(shown.text.clone())
                .spoken(shown.kind.spoken(&shown.text))
                .on(shown.focused),
            FieldAct::Focus,
        ),
    ];
    if shown.kind == Kind::Tempo {
        let end = r.x + r.w;
        let halve = Rect::new(end - BUTTON_W, r.y + 1.0, BUTTON_W, h - 2.0);
        let double = Rect::new(halve.x - BUTTON_GAP - BUTTON_W, halve.y, BUTTON_W, h - 2.0);
        for (rect, text, act) in [
            (double, "×2", FieldAct::Double),
            (halve, "÷2", FieldAct::Halve),
        ] {
            let cell = Cell::new(rect, Look::Button).text(text);
            cells.push(if shown.known {
                put(cell, act)
            } else {
                cell.off()
            });
        }
    }
    cells
}

/// The field: its value at the right, a ring while it has the keys, the caret after what was
/// typed.
pub fn paint_cell(pen: &mut Pen, cell: &Cell) {
    let c = pen.c;
    let r = cell.rect;
    if cell.on {
        pen.round(r, c.ring, Radius::S);
        let inner = Rect::new(r.x + 1.5, r.y + 1.5, r.w - 3.0, r.h - 3.0);
        pen.round(inner, c.well, Radius::S);
    } else {
        pen.round(r, c.well, Radius::S);
    }
    let pad = 8.0;
    let ink = if cell.enabled { c.ink_strong } else { c.ink };
    let text = Rect::new(r.x, r.y, r.w - pad, r.h);
    pen.digits(
        &cell.text,
        FontSize::Small,
        Weight::Medium,
        ink,
        text,
        Align::Right,
    );
    if cell.on {
        pen.fill(
            Rect::new(r.right() - pad + 1.0, r.y + 5.0, 1.5, r.h - 10.0),
            c.ink_strong,
        );
    }
}

/// The arguments of 「トランスポーズをノートに反映」 wherever it sits (it is inside
/// 「クオンタイズ」 now, UI-313).
fn write_args(items: &[Item]) -> Option<Value> {
    for item in items {
        match &item.run {
            Run::Cmd("clip.transpose.write", args) => return Some(args.clone()),
            Run::Sub(rows) => {
                if let Some(args) = write_args(rows) {
                    return Some(args);
                }
            }
            _ => {}
        }
    }
    None
}

/// Reads a row's value (and the write row's reach) from the song again, down through the
/// rows of a submenu.
fn reread_fields(item: &mut Item, p: &Project, write: Option<bool>, at_zero: bool) {
    match &mut item.run {
        Run::Field(kind, args, value) => {
            let ids: Vec<ClipId> = args["ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str()?.parse().ok())
                .collect();
            *value = shared(p, &ids, *kind);
        }
        Run::Cmd("clip.transpose.write", _) => {
            if let Some(on) = write {
                item.enabled = on;
                item.reason = if on {
                    String::new()
                } else if at_zero {
                    "トランスポーズが 0 です".into()
                } else {
                    "現在の状態では使えません".into()
                };
            }
        }
        Run::Sub(rows) => {
            for row in rows {
                reread_fields(row, p, write, at_zero);
            }
        }
        _ => {}
    }
}

impl Item {
    /// A row that is the field of `kind` for the clips `ids`, showing `value`.
    pub fn field(kind: Kind, ids: &[String], value: Option<f64>) -> Item {
        Item::new(
            kind.label(),
            Run::Field(kind, serde_json::json!({ "ids": ids }), value),
        )
    }
}

impl Studio {
    /// The kind of row `i` of the open menu and its arguments, when it is an enabled field.
    fn field_row(&self, i: usize) -> Option<(Kind, Value)> {
        let st = self.ctx.as_ref().filter(|s| s.ask.is_none())?;
        match st.items.get(i) {
            Some(Item {
                run: Run::Field(kind, args, _),
                enabled: true,
                ..
            }) => Some((*kind, args.clone())),
            _ => None,
        }
    }

    /// The field that has the keys.
    pub fn field_typing(&self) -> Option<&Typing> {
        if !self.context_open() {
            return None;
        }
        self.ctx.as_ref()?.field.as_ref()
    }

    /// Gives the keys to the field of row `i`.
    pub(crate) fn field_focus(&mut self, i: usize) -> bool {
        if self.field_row(i).is_none() {
            return false;
        }
        if let Some(st) = self.ctx.as_mut() {
            // The field takes the keys, so the search box keeps no composition (UI-313).
            if let Some(search) = st.search.as_mut() {
                search.field.marked.clear();
            }
            st.field = Some(Typing {
                row: i,
                text: String::new(),
                fresh: true,
                marked: String::new(),
            });
            // Another field starts a new turn of the wheel (P3-1 of review 15).
            st.turn = WheelTurn::default();
        }
        self.refresh_popup();
        true
    }

    /// A press on a part of row `i`.
    pub(crate) fn field_action(&mut self, g: &Geometry, i: usize, act: FieldAct) -> bool {
        match act {
            FieldAct::Label | FieldAct::Focus => self.field_focus(i),
            FieldAct::Double | FieldAct::Halve => {
                let Some(Run::Field(_, _, Some(v))) = self
                    .ctx
                    .as_ref()
                    .and_then(|s| s.items.get(i))
                    .map(|item| &item.run)
                else {
                    return false;
                };
                let to = if act == FieldAct::Double {
                    v * 2.0
                } else {
                    v / 2.0
                };
                self.field_set(g, i, to, None)
            }
        }
    }

    /// Hands `to` (or a change `by`) to the command of row `i`; the menu stays, its fields show
    /// the song again, and the field keeps the keys.
    fn field_set(&mut self, g: &Geometry, i: usize, to: f64, by: Option<f64>) -> bool {
        let Some((kind, mut args)) = self.field_row(i) else {
            return false;
        };
        let (lo, hi) = kind.range();
        match by {
            Some(by) => args["by"] = Value::from(by as i64),
            None if kind.signed() => args[kind.arg()] = Value::from(to.clamp(lo, hi) as i64),
            None => args[kind.arg()] = Value::from(to.clamp(lo, hi)),
        }
        let done = self.arrange_run(kind.command(), args).is_some();
        if done {
            self.rebuild(g);
            self.fields_again();
        }
        if let Some(t) = self.ctx.as_mut().and_then(|s| s.field.as_mut()) {
            t.text.clear();
            t.marked.clear();
            t.fresh = true;
        }
        self.refresh_popup();
        done
    }

    /// Reads the values of the fields from the song again. 「トランスポーズをノートに反映」 is
    /// possible only while a value is not 0, so it is read again too (UI-311, UI-313).
    fn fields_again(&mut self) {
        let write_args = self.ctx.as_ref().and_then(|st| write_args(&st.items));
        let write = write_args.as_ref().map(|args| {
            self.song
                .check("clip.transpose.write", args.clone())
                .unwrap_or(false)
        });
        let at_zero = write_args.as_ref().is_some_and(|args| {
            let ids: Vec<ClipId> = args["ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str()?.parse().ok())
                .collect();
            let p = self.project();
            ids.iter()
                .all(|id| p.clip(*id).is_ok_and(|(_, c)| c.transpose() == 0))
        });
        let p = self.project();
        let Some(st) = self.ctx.as_mut() else {
            return;
        };
        for item in st.items.iter_mut() {
            reread_fields(item, &p, write, at_zero);
        }
        if let Some(search) = st.search.as_mut() {
            for item in search.pool.iter_mut() {
                reread_fields(item, &p, write, at_zero);
            }
        }
    }

    /// A key while a field has the keys: ↑↓ change the value at once, Enter hands over what was
    /// typed, Tab goes to the next field, Backspace takes a character back, and a digit types.
    /// None when no field has the keys (or the key is not the field's).
    pub(crate) fn field_key(
        &mut self,
        g: &Geometry,
        key: &str,
        ch: Option<&str>,
        shift: bool,
    ) -> Option<bool> {
        let t = self.field_typing()?.clone();
        let (kind, _) = self.field_row(t.row)?;
        // While the IME is composing, its keys are its own: Enter commits, Backspace and the
        // arrows belong to the composition, and Esc cancels it (P2-2 of review 15).
        if !t.marked.is_empty() {
            return Some(true);
        }
        match key {
            "up" | "down" => {
                let sign = if key == "up" { 1.0 } else { -1.0 };
                let by = sign * kind.step(shift);
                if kind.signed() {
                    return Some(self.field_set(g, t.row, 0.0, Some(by)));
                }
                let now = match &self.ctx.as_ref()?.items.get(t.row)?.run {
                    Run::Field(_, _, v) => *v,
                    _ => None,
                };
                Some(now.is_some_and(|v| self.field_set(g, t.row, v + by, None)))
            }
            "enter" => {
                let Some(to) = t.number(kind).filter(|_| !t.fresh) else {
                    return Some(true);
                };
                Some(self.field_set(g, t.row, to, None))
            }
            "tab" => {
                let rows = self.ctx.as_ref()?.items.len();
                let next = (1..rows)
                    .map(|d| {
                        if shift {
                            (t.row + rows - d) % rows
                        } else {
                            (t.row + d) % rows
                        }
                    })
                    .find(|&i| self.field_row(i).is_some())?;
                Some(self.field_focus(next))
            }
            "backspace" => {
                if let Some(t) = self.ctx.as_mut().and_then(|s| s.field.as_mut()) {
                    if t.fresh {
                        t.text.clear();
                        t.fresh = false;
                    } else {
                        t.text.pop();
                    }
                }
                self.refresh_popup();
                Some(true)
            }
            "escape" => {
                // Nothing typed: Esc leaves the field and shuts the menu, as it did before the
                // field took the keys (P2-1 of review 15). Half-typed: it only gives the typing
                // up, as UI-312 says.
                let typed = !t.fresh || !t.marked.is_empty();
                if let Some(st) = self.ctx.as_mut() {
                    st.field = None;
                }
                if !typed {
                    self.ctx = None;
                    self.popup = None;
                }
                self.close_wheel_group();
                self.refresh_popup();
                Some(true)
            }
            _ => {
                if self.ask_field().is_some() {
                    // The menu's search box hands the text on (`field_text`).
                    return None;
                }
                Some(self.field_text(ch.unwrap_or(key)))
            }
        }
    }

    /// Typed text while a field has the keys: it takes digits, the point and the sign (also the
    /// IME's full-width forms); false when it took no character, so the search box may have it.
    pub(crate) fn field_text(&mut self, text: &str) -> bool {
        let Some(t) = self.field_typing() else {
            return false;
        };
        let Some((kind, _)) = self.field_row(t.row) else {
            return false;
        };
        let mut any = false;
        if let Some(t) = self.ctx.as_mut().and_then(|s| s.field.as_mut()) {
            t.marked.clear();
            for c in text.chars() {
                any |= t.put(kind, c);
            }
        }
        if any {
            // What the IME had put in the search box is the field's now (UI-313).
            if let Some(search) = self.ctx.as_mut().and_then(|s| s.search.as_mut()) {
                search.field.marked.clear();
            }
        }
        self.refresh_popup();
        any
    }

    /// The IME's composition while a field has the keys: the field keeps it, and the search box
    /// keeps none of it. False when the field takes no character of it (UI-313).
    pub(crate) fn field_marked(&mut self, text: &str) -> bool {
        let Some(t) = self.field_typing() else {
            return false;
        };
        let Some((kind, _)) = self.field_row(t.row) else {
            return false;
        };
        let mut took = false;
        if let Some(t) = self.ctx.as_mut().and_then(|s| s.field.as_mut()) {
            took = t.put_marked(kind, text);
        }
        if took && let Some(search) = self.ctx.as_mut().and_then(|s| s.search.as_mut()) {
            search.field.marked.clear();
        }
        self.refresh_popup();
        took
    }

    /// A wheel turn over the field of row `i` (UI-313): away from you raises it, Shift further,
    /// and Ctrl finer for the fields with a fraction.
    pub(crate) fn context_field_wheel(
        &mut self,
        g: &Geometry,
        i: usize,
        px: f32,
        shift: bool,
        ctrl: bool,
    ) -> bool {
        self.context_field_wheel_at(g, i, px, shift, ctrl, Instant::now())
    }

    /// The same with the time the turn happened (the tests pass a fixed one, P3-4 of review 15).
    pub(crate) fn context_field_wheel_at(
        &mut self,
        g: &Geometry,
        i: usize,
        px: f32,
        shift: bool,
        ctrl: bool,
        now: Instant,
    ) -> bool {
        let Some((kind, args)) = self.field_row(i) else {
            return false;
        };
        // Putting the keys in another field starts its own turn of the wheel (P3-1).
        if !self.field_typing().is_some_and(|t| t.row == i) {
            self.field_focus(i);
        }
        let whole = match self.ctx.as_mut() {
            Some(st) => st.turn.notches(px),
            None => return false,
        };
        if whole == 0 {
            return true;
        }
        let ids: Vec<String> = args["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        self.wheel_group_start_at(now, kind, &ids);
        let step = kind.wheel_step(shift, ctrl) * f64::from(whole);
        self.wheel_run(g, |s, g| {
            if kind.signed() {
                s.field_set(g, i, 0.0, Some(step))
            } else {
                let now =
                    s.ctx
                        .as_ref()
                        .and_then(|st| st.items.get(i))
                        .and_then(|item| match &item.run {
                            Run::Field(_, _, v) => *v,
                            _ => None,
                        });
                now.is_some_and(|v| s.field_set(g, i, v + step, None))
            }
        })
    }

    /// The field under a point of the open menu, when it is one that takes the wheel.
    pub(crate) fn context_field_at(&self, g: &Geometry, x: f32, y: f32) -> Option<usize> {
        let p = self.popup.as_ref()?;
        if p.kind != crate::popup::PopupKind::Context {
            return None;
        }
        p.layout(g.size)
            .cells
            .iter()
            .find(|c| c.rect.contains(x, y))
            .and_then(|c| match c.action.as_ref() {
                Some(Action::Field(i, FieldAct::Focus)) => Some(*i),
                _ => None,
            })
    }

    /// Starts a turn of the wheel's undo step, or keeps the one of the same field that is still
    /// open (UI-313).
    pub(crate) fn wheel_group_start(&mut self, kind: Kind, ids: &[String]) {
        self.wheel_group_start_at(Instant::now(), kind, ids);
    }

    /// The same at a given time (P3-4 of review 15).
    pub(crate) fn wheel_group_start_at(&mut self, now: Instant, kind: Kind, ids: &[String]) {
        let key = (kind, ids.to_vec());
        if self
            .wheel_group
            .as_ref()
            .is_some_and(|g| g.continues(&key, now))
        {
            if let Some(g) = self.wheel_group.as_mut() {
                g.at = now;
            }
            return;
        }
        self.close_wheel_group();
        self.song.session_mut().begin_group();
        self.wheel_group = Some(WheelGroup {
            field: key,
            at: now,
            open: true,
        });
    }

    /// Ends a turn of the wheel's undo step, so the next command is a step of its own.
    pub(crate) fn close_wheel_group(&mut self) {
        if self.wheel_group.as_ref().is_some_and(|g| g.open) {
            self.song.session_mut().end_group();
        }
        self.wheel_group = None;
    }

    /// Runs a wheel's change inside its undo step (UI-313).
    fn wheel_run(&mut self, g: &Geometry, run: impl FnOnce(&mut Self, &Geometry) -> bool) -> bool {
        self.wheel_running = true;
        let done = run(self, g);
        self.wheel_running = false;
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_values_show_as_the_mock_has_them() {
        assert_eq!(Kind::Tempo.text(Some(120.0)), "120.00");
        assert_eq!(Kind::Speed.text(Some(100.0)), "100%");
        assert_eq!(Kind::Speed.text(Some(87.5)), "87.5%");
        assert_eq!(Kind::Semis.text(Some(0.0)), "0");
        assert_eq!(Kind::Semis.text(Some(12.0)), "+12");
        assert_eq!(Kind::Cents.text(Some(-5.0)), "−5");
        assert_eq!(Kind::Cents.text(None), "—");
        assert_eq!(Kind::Semis.spoken("+2"), "トランスポーズ +2 半音");
        assert_eq!(Kind::Tempo.spoken("120.00"), "テンポ 120.00 BPM");
        assert_eq!(Kind::Speed.spoken("100%"), "速さ 100%");
        assert_eq!(Kind::Cents.spoken("−5"), "微調整 −5 セント");
    }

    #[test]
    fn a_speed_may_be_typed_with_its_percent_sign() {
        let mut t = Typing {
            fresh: true,
            ..Typing::default()
        };
        for c in "87.5%".chars() {
            t.put(Kind::Speed, c);
        }
        assert_eq!(t.text, "87.5");
        assert_eq!(t.number(Kind::Speed), Some(87.5));
    }

    #[test]
    fn typing_replaces_the_value_and_keeps_to_what_the_field_takes() {
        let mut t = Typing {
            row: 0,
            text: String::new(),
            fresh: true,
            marked: String::new(),
        };
        for c in "93.5.x".chars() {
            t.put(Kind::Tempo, c);
        }
        assert_eq!(t.text, "93.5");
        assert_eq!(t.number(Kind::Tempo), Some(93.5));
        let mut t = Typing {
            fresh: true,
            ..Typing::default()
        };
        for c in "3−".chars() {
            t.put(Kind::Semis, c);
        }
        assert_eq!(t.text, "-3");
        t.put(Kind::Semis, '9');
        assert_eq!(t.number(Kind::Semis), Some(-24.0), "kept inside the range");
        assert!(!t.put(Kind::Semis, '.'));
    }

    #[test]
    fn a_character_the_field_does_not_take_leaves_the_value_shown() {
        let mut t = Typing {
            fresh: true,
            ..Typing::default()
        };
        for c in ['\n', ' ', '\t', 'x'] {
            assert!(!t.put(Kind::Semis, c));
        }
        assert!(!t.put(Kind::Semis, '.'), "a semitone has no point");
        assert!(t.fresh, "the value is still shown");
        assert!(t.put(Kind::Semis, '5'));
        assert_eq!((t.text.as_str(), t.fresh), ("5", false));
    }

    #[test]
    fn an_ime_writes_full_width_digits_and_signs_as_the_field_takes_them() {
        let mut t = Typing {
            fresh: true,
            ..Typing::default()
        };
        for c in "１２３".chars() {
            assert!(t.put(Kind::Semis, c));
        }
        assert_eq!(t.number(Kind::Semis), Some(24.0), "kept inside the range");
        let mut t = Typing {
            fresh: true,
            ..Typing::default()
        };
        for c in "－５".chars() {
            assert!(t.put(Kind::Semis, c));
        }
        assert_eq!(t.text, "-5");
        let mut t = Typing {
            fresh: true,
            ..Typing::default()
        };
        for c in "９０．５".chars() {
            assert!(t.put(Kind::Tempo, c));
        }
        assert_eq!(t.number(Kind::Tempo), Some(90.5));
        assert_eq!(halfwidth('ー'), '-');
        assert_eq!(halfwidth('＋'), '+');
    }

    #[test]
    fn a_wheel_turn_stays_one_undo_step_for_four_hundred_milliseconds() {
        let now = Instant::now();
        let key = (Kind::Semis, vec!["c1".to_string()]);
        let group = WheelGroup {
            field: key.clone(),
            at: now,
            open: true,
        };
        assert!(group.continues(&key, now + Duration::from_millis(399)));
        assert!(!group.continues(&key, now + Duration::from_millis(400)));
        assert!(!group.continues(&(Kind::Cents, vec!["c1".to_string()]), now));
    }

    #[test]
    fn the_wheel_gathers_whole_notches_and_turning_back_drops_them() {
        let mut w = WheelTurn::default();
        assert_eq!(w.notches(60.0), 1);
        assert_eq!(w.notches(20.0), 0);
        assert_eq!(w.notches(20.0), 0);
        assert_eq!(w.notches(20.0), 1);
        // Turning back throws away what was gathered the other way.
        assert_eq!(w.notches(20.0), 0);
        assert_eq!(w.notches(-20.0), 0);
        assert_eq!(w.notches(-20.0), 0);
        assert_eq!(w.notches(-20.0), -1);
        // A fast turn gives several notches at once.
        assert_eq!(w.notches(-180.0), -3);
    }
}
