//! The volume fader of a wide track head (UI-324): a text-less fader on the second line, left
//! of the snowflake. It moves the value the mixer's fader moves (the track's volume), so a drag
//! goes the mixer fader's way: the selected tracks move together, one step of the history is
//! made when it is let go, and a control its lane plays (UI-063) does not move. It is dragged
//! sideways (Shift: fine); the value shows in a tag only while it moves.
//!
//! It works as the device knob does (UI-324): a double click or Enter types the value in a
//! field (Enter puts it in, Esc leaves the old one), Alt or Delete sets 0 dB, ←→ move it by
//! 1 dB (Shift: 0.1) and PageUp and PageDown by 6 dB. "-3", "-3 dB" and "-inf" are read.

use serde_json::json;
use xirius_core::auto::AutoTarget;
use xirius_core::id::TrackId;
use xirius_core::model::{FADER_FLOOR_DB, MAX_DB, MIN_DB};
use xirius_design::{FontSize, Radius, Weight};

use crate::draw::{Align, Pen};
use crate::mixer::MixerHit;
use crate::mixer::meters::db_to_t;
use crate::settings::FieldId;
use crate::settings::field::Field;
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

/// What an arrow key moves the value by, and with Shift.
pub const STEP_DB: f64 = 1.0;
pub const FINE_DB: f64 = 0.1;
/// What PageUp and PageDown move it by.
pub const PAGE_DB: f64 = 6.0;
/// The width of the cap, and the groove's thickness.
pub const CAP_W: f32 = 4.0;
const CAP_H: f32 = 12.0;
const RAIL_H: f32 = 3.0;
/// The tag's height, and its gap over the fader.
const TAG_H: f32 = 24.0;
const TAG_LIFT: f32 = 4.0;

/// A value being typed in the fader: it opens with the value it has, and the first letter typed
/// takes the place of all of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub track: TrackId,
    pub field: Field,
    /// Nothing typed yet.
    pub fresh: bool,
}

impl Entry {
    /// What is typed, the old value going at the first letter.
    pub fn type_text(&mut self, range: Option<std::ops::Range<usize>>, text: &str, marked: bool) {
        if std::mem::take(&mut self.fresh) {
            self.field.set(String::new());
        }
        if marked {
            self.field.mark_text(range, text);
        } else {
            self.field.replace_text(range, text);
        }
    }
}

/// What the head faders hold: the one with the keys, the one being dragged and the field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeadFader {
    pub focus: Option<TrackId>,
    /// The press began a drag of this track's fader (the mixer's drag, sideways).
    pub dragging: Option<TrackId>,
    pub entry: Option<Entry>,
}

/// What the fader says of `db`, as the tag and the field do.
pub fn text(db: f64) -> String {
    crate::autovalue::text(&AutoTarget::Volume, db)
}

/// Where the cap's left edge is on a fader `r` wide for `db`.
fn cap_x(r: Rect, db: f64) -> f32 {
    r.x + db_to_t(db as f32) * (r.w - CAP_W).max(0.0)
}

impl Studio {
    /// Where the fader of `t` is on the screen, when its row shows it.
    pub fn head_fader_rect(&self, g: &Geometry, t: TrackId) -> Option<Rect> {
        let row = self.tree().row_of(RowKind::Track(t))?;
        self.head_fader_row_rect(g, row)
    }

    /// The fader of `row` when the row has one: the row is in hand, so nothing is searched for.
    pub fn head_fader_row_rect(&self, g: &Geometry, row: &crate::tree::Row) -> Option<Rect> {
        let r = g.parts(row).fader?;
        Some(Rect::new(
            g.column.x + r.x,
            self.row_y(g, row) + r.y,
            r.w,
            r.h,
        ))
    }

    /// The value the fader shows now. Only this track's lanes are read (see
    /// [`Studio::mixer_rest`]), so a screen of many faders costs one track's worth each.
    pub(crate) fn head_fader_value(&self, t: TrackId) -> Option<f64> {
        self.mixer_value(MixerHit::Fader(t))
    }

    /// A press on the fader: it takes the keys, and a field open on another fader keeps what
    /// is typed (the press moves on to this one).
    pub(crate) fn head_fader_press(&mut self, t: TrackId) -> bool {
        if self.head_fader.entry.as_ref().is_some_and(|e| e.track != t) {
            self.head_fader_commit();
        }
        self.head_fader.focus = Some(t);
        true
    }

    /// The mouse went down on `hit`: Alt sets 0 dB, a double click types the value, any other
    /// press takes hold of the fader. `x` is where the pointer is. True when it took the press.
    pub(crate) fn head_fader_down(
        &mut self,
        g: &Geometry,
        hit: Hit,
        x: f32,
        clicks: u32,
        mods: Mods,
    ) -> bool {
        let Hit::HeadFader(t) = hit else {
            return false;
        };
        // The field open on this fader takes the press as it is; one open on another keeps what
        // is typed, and the press goes on to this fader.
        if self.head_fader.entry.as_ref().is_some_and(|e| e.track == t) {
            return true;
        }
        if self.head_fader.entry.is_some() {
            self.head_fader_commit();
        }
        if mods.alt {
            self.head_fader_zero(t);
            return true;
        }
        if clicks >= 2 {
            self.head_fader_type(t);
            return true;
        }
        self.head_fader_press(t);
        // The drag keeps where it began in `y0`: sideways, it is the pointer's x.
        self.drag_begin(g, hit, x);
        true
    }

    /// Opens the field the value is typed in.
    pub(crate) fn head_fader_type(&mut self, t: TrackId) {
        if self.mixer_locked(MixerHit::Fader(t)) {
            return;
        }
        let Some(now) = self.head_fader_value(t) else {
            return;
        };
        self.head_fader.focus = Some(t);
        self.head_fader.entry = Some(Entry {
            track: t,
            field: Field::new(FieldId::Value, text(now)),
            fresh: true,
        });
    }

    /// What is typed goes into the song when it reads as a value; else the fader keeps the one it
    /// had.
    pub(crate) fn head_fader_commit(&mut self) -> bool {
        let Some(e) = self.head_fader.entry.take() else {
            return false;
        };
        let Some(db) = crate::autovalue::parse(&AutoTarget::Volume, &e.field.text()) else {
            return false;
        };
        self.head_fader_set(e.track, db)
    }

    /// Every selected track takes `db`, as the menu's 「値を入力…」 does; one step of the history.
    fn head_fader_set(&mut self, t: TrackId, db: f64) -> bool {
        if self.mixer_locked(MixerHit::Fader(t)) {
            return false;
        }
        let ids = self.track_ids(t);
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command("track.volume.set", json!({ "ids": ids, "db": db }))
    }

    /// 0 dB, as 「0 dB に戻す」 puts it.
    pub(crate) fn head_fader_zero(&mut self, t: TrackId) -> bool {
        self.head_fader.focus = Some(t);
        self.head_fader_set(t, 0.0)
    }

    /// Moves the fader by `by` dB as a short drag would: a control that writes its lane (Touch,
    /// Latch, Write) writes it, and the tracks that move together keep their differences.
    fn head_fader_nudge(&mut self, t: TrackId, by: f64) -> bool {
        let hit = MixerHit::Fader(t);
        if self.mixer.drag.is_some() || self.mixer_locked(hit) {
            return false;
        }
        let Some(start) = self.mixer_value(hit) else {
            return false;
        };
        let to = if start <= FADER_FLOOR_DB {
            // Out of silence the first step lands just over the floor.
            if by > 0.0 { FADER_FLOOR_DB + by } else { start }
        } else {
            (((start + by) * 10.0).round() / 10.0).clamp(MIN_DB, MAX_DB)
        };
        let to = if to <= FADER_FLOOR_DB { MIN_DB } else { to };
        if to == start {
            return true;
        }
        self.drag_open(hit, 0.0, 1.0);
        self.drag_to(to);
        self.drag_end();
        true
    }

    /// The keys let go: a field keeps what is typed.
    pub(crate) fn head_fader_blur(&mut self) {
        self.head_fader_commit();
        self.head_fader.focus = None;
    }

    /// A press that is not on the fader takes the keys from it.
    pub(crate) fn head_fader_press_off(&mut self, g: &Geometry, x: f32, y: f32) {
        if self.head_fader.focus.is_none() && self.head_fader.entry.is_none() {
            return;
        }
        // Another fader is not the same one: the keys and the field stay only on this track's.
        let on = self
            .head_fader
            .entry
            .as_ref()
            .map(|e| e.track)
            .or(self.head_fader.focus);
        if !matches!(self.hit(g, x, y), Hit::HeadFader(t) if Some(t) == on) {
            self.head_fader_blur();
        }
    }

    /// A key for the fader that has the keys or its field. False when it is not the fader's.
    pub(crate) fn head_fader_key(&mut self, key: &str, mods: Mods) -> bool {
        // A menu or another field open has the keys, not the fader behind it.
        if self.popup.is_some()
            || self
                .ime_target()
                .is_some_and(|t| t != crate::interaction::ImeTarget::Head)
            || self.bar_edit.is_typing()
            || self.inspector.field.is_some()
            || self.field_typing().is_some()
        {
            return false;
        }
        // A track that is gone (undone, removed) takes no key.
        let gone = |s: &Self, t: TrackId| s.tree().row_of(RowKind::Track(t)).is_none();
        if self
            .head_fader
            .entry
            .as_ref()
            .is_some_and(|e| gone(self, e.track))
        {
            self.head_fader.entry = None;
        }
        if self.head_fader.focus.is_some_and(|t| gone(self, t)) {
            self.head_fader.focus = None;
        }
        let blocked = self.input_blocked();
        if let Some(e) = &mut self.head_fader.entry {
            if !e.field.marked.is_empty() {
                return true;
            }
            // While a save or an undo on the worker holds the song, Enter keeps what is typed:
            // the song would not take it.
            if matches!(key, "enter" | "tab") && blocked {
                return true;
            }
            match key {
                "escape" => self.head_fader.entry = None,
                "backspace" if e.fresh => {
                    e.fresh = false;
                    e.field.set(String::new());
                }
                "backspace" => e.field.backspace(),
                "enter" | "tab" => {
                    self.head_fader_commit();
                }
                _ => {}
            }
            return true;
        }
        let Some(t) = self.head_fader.focus else {
            return false;
        };
        if mods.ctrl || mods.alt {
            return false;
        }
        let small = if mods.shift { FINE_DB } else { STEP_DB };
        match key {
            "right" => self.head_fader_nudge(t, small),
            "left" => self.head_fader_nudge(t, -small),
            "pageup" => self.head_fader_nudge(t, PAGE_DB),
            "pagedown" => self.head_fader_nudge(t, -PAGE_DB),
            "delete" => self.head_fader_zero(t),
            "enter" => {
                self.head_fader_type(t);
                true
            }
            "escape" => {
                self.head_fader.focus = None;
                true
            }
            _ => return false,
        };
        true
    }

    /// The field's place, over the fader being typed in.
    pub fn head_fader_field_rect(&self, g: &Geometry) -> Option<Rect> {
        let t = self.head_fader.entry.as_ref()?.track;
        self.head_fader_rect(g, t)
    }
}

/// The fader of a row: the groove, the amount in the focus line and the cap, the focus ring
/// around it while it has the keys.
pub fn paint_fader(pen: &mut Pen, studio: &Studio, t: TrackId, r: Rect) {
    let c = pen.c;
    if studio
        .head_fader
        .entry
        .as_ref()
        .is_some_and(|e| e.track == t)
    {
        return;
    }
    let db = studio.head_fader_value(t).unwrap_or(0.0);
    let hot = studio.hover == Hit::HeadFader(t) || studio.head_fader.dragging == Some(t);
    let x = cap_x(r, db);
    let mid = r.y + (r.h - RAIL_H) / 2.0;
    pen.round(Rect::new(r.x, mid, r.w, RAIL_H), c.well, Radius::Xs);
    pen.round(
        Rect::new(r.x, mid, (x - r.x + CAP_W / 2.0).max(CAP_W / 2.0), RAIL_H),
        c.ring,
        Radius::Xs,
    );
    let cap = Rect::new(x, r.y + (r.h - CAP_H) / 2.0, CAP_W, CAP_H);
    // Like the mixer's cap: the thumb face, the primary hover face under the pointer (UI-309).
    pen.round(
        cap,
        if hot { c.primary_hover.0 } else { c.thumb },
        Radius::Xs,
    );
    if studio.head_fader.focus == Some(t) {
        pen.ring_outside(r, c.ring, Radius::S, 2.0);
    }
}

/// The value tag over the fader while it moves, and the field while a value is typed.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    if let (Some(t), Some(d)) = (studio.head_fader.dragging, studio.mixer.drag)
        && let Some(r) = studio.head_fader_rect(g, t)
    {
        let said = format!("{} dB", text(d.value));
        let w = crate::browser::text_width(&said, FontSize::Caption)
            + xirius_design::Space::X2.px() * 2.0;
        let x = (cap_x(r, d.value) + CAP_W / 2.0 - w / 2.0).clamp(g.column.x, g.column.right() - w);
        let tag = Rect::new(x, (r.y - TAG_H - TAG_LIFT).max(g.column.y), w, TAG_H);
        pen.floating(tag, pen.c.surface_3, Radius::S);
        pen.digits(
            &said,
            FontSize::Caption,
            Weight::Medium,
            pen.c.ink_strong,
            tag,
            Align::Center,
        );
    }
    let (Some(e), Some(r)) = (&studio.head_fader.entry, studio.head_fader_field_rect(g)) else {
        return;
    };
    pen.round(r, pen.c.surface_3, Radius::S);
    let inner = Rect::new(r.x + 6.0, r.y, (r.w - 12.0).max(0.0), r.h);
    let width = pen.digits(
        &e.field.text(),
        FontSize::Small,
        Weight::Regular,
        pen.c.ink_strong,
        inner,
        Align::Left,
    );
    pen.fill(
        Rect::new(
            (inner.x + width + 1.0).min(r.right() - 3.0),
            r.y + 4.0,
            1.5,
            (r.h - 8.0).max(0.0),
        ),
        pen.c.ink_strong,
    );
}
