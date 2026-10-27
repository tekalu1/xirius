//! The inspector: the chosen track's and clip's values, in the rail's panel (UI-312).
//!
//! Like the browser, this file has no window in it. [`Layout`] says where everything sits for
//! the panel's rectangle, [`Studio::inspector_hit`] says what is under a point, and `paint`
//! draws those numbers. The panel is one of the rail's faces: opening it shuts the browser's
//! list, and the other way round ([`crate::browser::RailTab::Inspector`]).
//!
//! It shows the selected track's colour, icon and name, the track's transpose and velocity (an
//! instrument track only), and the selected clip's name and transpose. The values are the same
//! fields the clip menu has: ↑↓ (Shift: further), the wheel, typing and Enter
//! ([`crate::context::fields`]).

use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::TrackKind;
use xirius_design::{FontSize, Radius, Space, Weight};

use crate::context::fields::{self, Kind, Typing};
use crate::draw::{Align, Pen};
use crate::popup::{Cell, Look};
use crate::studio::{Geometry, Rect, Studio};
use crate::tree::RowKind;

/// The height of the chosen track's heading.
pub const HEADER_H: f32 = 32.0;
/// The height of the chosen clip's heading.
pub const CLIP_HEADER_H: f32 = 28.0;
/// The height of a value's row.
pub const ROW_H: f32 = 32.0;
const PAD: f32 = 8.0;
const FIELD_W: f32 = 64.0;
const FIELD_H: f32 = 24.0;
const ICON: f32 = 16.0;
const CLIP_GAP: f32 = 8.0;

/// What the inspector keeps between frames: the field that has the keys, and which track or
/// clip and which value it is for. The target is kept by id, not by row: choosing another track
/// must not give the half-typed text to its row (UI-312).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inspector {
    pub field: Option<Typing>,
    pub target: Option<FieldTarget>,
    /// How far a wheel has turned since the last notch (UI-313).
    pub wheel: fields::WheelTurn,
}

/// The value a field edits: the track or clip (`--ids`) and which of its values.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldTarget {
    pub kind: Kind,
    pub ids: Vec<String>,
}

impl FieldTarget {
    fn of(row: &Row) -> FieldTarget {
        FieldTarget {
            kind: row.kind,
            ids: row.ids.clone(),
        }
    }

    fn is(&self, row: &Row) -> bool {
        self.kind == row.kind && self.ids == row.ids
    }
}

/// A part of the inspector under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The value's row; index into [`Layout::rows`].
    Field(usize),
}

/// The chosen track and clip the inspector shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Chosen {
    pub track: Option<TrackId>,
    pub clip: Option<ClipId>,
}

impl Chosen {
    pub fn of(s: &Studio) -> Chosen {
        let track = match s.selected_row {
            Some(RowKind::Track(t)) => Some(t),
            _ => s.selected_clip.map(|(t, _)| t),
        };
        // The clip's part is there only while the clip is the one chosen in the arrangement
        // (UI-313): a track press or an empty lane leaves it out.
        let clip = (s.arrange.clips.len() == 1)
            .then(|| s.arrange.clips.iter().next().copied())
            .flatten()
            .filter(|key| s.selected_clip == Some(*key))
            .filter(|(t, _)| Some(*t) == track)
            .map(|(_, c)| c);
        Chosen { track, clip }
    }
}

/// A value's row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: Kind,
    /// The song's value; `None` when the clips differ or it is not known.
    pub value: Option<f64>,
    /// What the command takes (`--ids`).
    pub ids: Vec<String>,
    pub rect: Rect,
}

/// Where the inspector's parts sit for the panel's rectangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub panel: Rect,
    pub header: Option<Rect>,
    pub clip_header: Option<Rect>,
    pub rows: Vec<Row>,
}

impl Layout {
    /// The field at `(x, y)`.
    pub fn field_at(&self, x: f32, y: f32) -> Option<usize> {
        self.rows.iter().position(|r| r.rect.contains(x, y))
    }
}

impl Studio {
    /// The inspector's layout for its panel.
    pub fn inspector_layout(&self, g: &Geometry) -> Layout {
        self.inspector_layout_in(g.panel)
    }

    fn inspector_layout_in(&self, panel: Rect) -> Layout {
        let p = self.project();
        let chosen = Chosen::of(self);
        let mut layout = Layout {
            panel,
            ..Layout::default()
        };
        let mut y = panel.y + PAD;
        let track = chosen.track.and_then(|id| p.tracks.get(&id));
        if let Some(t) = track {
            layout.header = Some(Rect::new(panel.x + PAD, y, panel.w - PAD * 2.0, HEADER_H));
            y += HEADER_H;
            if t.kind == TrackKind::Instrument {
                for kind in Kind::TRACK {
                    let value = match kind {
                        Kind::TrackSemis => Some(f64::from(t.transpose)),
                        _ => Some(f64::from(t.velocity)),
                    };
                    layout.rows.push(Row {
                        kind,
                        value,
                        ids: vec![t.id.to_string()],
                        rect: Rect::new(panel.x, y, panel.w, ROW_H),
                    });
                    y += ROW_H;
                }
            }
        }
        if let (Some(t), Some(id)) = (track, chosen.clip)
            && let Some(c) = t.clips.iter().find(|c| c.id == id)
        {
            y += CLIP_GAP;
            layout.clip_header = Some(Rect::new(
                panel.x + PAD,
                y,
                panel.w - PAD * 2.0,
                CLIP_HEADER_H,
            ));
            y += CLIP_HEADER_H;
            // A MIDI clip has its own transpose; an audio clip has the transpose, the fine tune
            // and the speed its menu has (UI-312, spec §3.5).
            let kinds: &[Kind] = if c.is_midi() {
                &[Kind::Semis]
            } else {
                &[Kind::Semis, Kind::Cents, Kind::Speed]
            };
            for (n, kind) in kinds.iter().enumerate() {
                layout.rows.push(Row {
                    kind: *kind,
                    value: fields::shared(&p, &[c.id], *kind),
                    ids: vec![c.id.to_string()],
                    rect: Rect::new(panel.x, y + n as f32 * ROW_H, panel.w, ROW_H),
                });
            }
        }
        layout
    }

    /// What is under a point of the inspector.
    pub fn inspector_hit(&self, g: &Geometry, x: f32, y: f32) -> Option<Part> {
        if !g.panel.contains(x, y) {
            return None;
        }
        self.inspector_layout(g).field_at(x, y).map(Part::Field)
    }

    /// Gives the keys to the field of row `i`.
    pub(crate) fn inspector_focus(&mut self, g: &Geometry, i: usize) -> bool {
        let Some(row) = self.inspector_layout(g).rows.get(i).cloned() else {
            return false;
        };
        self.inspector.target = Some(FieldTarget::of(&row));
        self.inspector.field = Some(Typing {
            row: i,
            text: String::new(),
            fresh: true,
            marked: String::new(),
        });
        // Another field starts a new turn of the wheel (P3-1 of review 15).
        self.inspector.wheel = fields::WheelTurn::default();
        true
    }

    /// The one place a press away from the fields comes to: the inspector gives up what was
    /// half typed (UI-312).
    pub(crate) fn field_blur(&mut self) {
        self.close_wheel_group();
        self.inspector_cancel();
    }

    /// Leaves the field without keeping what was half typed.
    pub(crate) fn inspector_cancel(&mut self) {
        self.inspector.field = None;
        self.inspector.target = None;
    }

    /// Drops a half-typed value whose track or clip is no longer shown, and keeps the row the
    /// typing points at right when the rows above it came or went (UI-312).
    pub(crate) fn inspector_follow(&mut self, g: &Geometry) {
        let Some(target) = self.inspector.target.clone() else {
            self.inspector.field = None;
            return;
        };
        let rows = self.inspector_layout(g).rows;
        let Some(i) = rows.iter().position(|row| target.is(row)) else {
            self.inspector_cancel();
            return;
        };
        if let Some(t) = self.inspector.field.as_mut() {
            t.row = i;
        }
    }

    /// The row the typing is for, in the layout as it is now.
    fn inspector_row(&mut self, g: &Geometry) -> Option<usize> {
        self.inspector_follow(g);
        let target = self.inspector.target.as_ref()?;
        self.inspector_layout(g)
            .rows
            .iter()
            .position(|row| target.is(row))
    }

    /// Hands `to` to the command of row `i`; the panel and the field stay.
    fn inspector_set(&mut self, g: &Geometry, i: usize, to: f64) -> bool {
        let Some(row) = self.inspector_layout(g).rows.get(i).cloned() else {
            return false;
        };
        let (lo, hi) = row.kind.range();
        let to = to.clamp(lo, hi);
        let mut args = serde_json::json!({ "ids": row.ids });
        // A semitone or a percent is whole; the speed keeps its fraction (UI-312).
        args[row.kind.arg()] = if row.kind.signed() {
            serde_json::Value::from(to.round() as i64)
        } else {
            serde_json::Value::from(to)
        };
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        let done = self.arrange_run(row.kind.command(), args).is_some();
        if done {
            self.rebuild(g);
        }
        if let Some(t) = self.inspector.field.as_mut() {
            t.text.clear();
            t.marked.clear();
            t.fresh = true;
        }
        done
    }

    /// A press on a part of the inspector.
    pub(crate) fn inspector_press(&mut self, g: &Geometry, part: Part) -> bool {
        match part {
            Part::Field(i) => self.inspector_focus(g, i),
        }
    }

    /// The field that has the keys, for the a11y names.
    pub fn inspector_typing(&self) -> Option<&Typing> {
        self.inspector.field.as_ref()
    }

    /// A key while an inspector field has the keys: ↑↓ change the value at once, Enter hands
    /// over what was typed, Tab goes to the next field, Backspace takes a character back, and a
    /// digit types. False when no inspector field has the keys, and for the keys that are not
    /// the field's (F4, Space, Ctrl+Z …): they go on to the screen's commands, as the clip
    /// menu's fields leave them (UI-312).
    pub(crate) fn inspector_key(
        &mut self,
        g: &Geometry,
        key: &str,
        ch: Option<&str>,
        mods: crate::tools::Mods,
    ) -> bool {
        if self.inspector.field.is_none() {
            return false;
        }
        // A modifier makes a command's key (Ctrl+Z), not a value's.
        if mods.ctrl || mods.alt {
            return false;
        }
        if key == "space" {
            return false;
        }
        let named = matches!(
            key,
            "up" | "down" | "enter" | "tab" | "backspace" | "escape"
        );
        let text = ch.unwrap_or(key);
        if !named && text.chars().count() != 1 {
            // F4, Home, Delete and the like are the screen's keys, not a value's.
            return false;
        }
        let Some(i) = self.inspector_row(g) else {
            return false;
        };
        let t = self.inspector.field.clone().expect("checked above");
        let Some(row) = self.inspector_layout(g).rows.get(i).cloned() else {
            return false;
        };
        let kind = row.kind;
        match key {
            "up" | "down" => {
                let sign = if key == "up" { 1.0 } else { -1.0 };
                let by = sign * kind.step(mods.shift);
                let now = row.value.unwrap_or(0.0);
                self.inspector_set(g, i, now + by);
                true
            }
            "enter" => {
                if !t.fresh
                    && let Some(to) = t.number(kind)
                {
                    self.inspector_set(g, i, to);
                }
                true
            }
            "tab" => {
                let rows = self.inspector_layout(g).rows.len();
                if rows < 2 {
                    return true;
                }
                let next = if mods.shift {
                    (i + rows - 1) % rows
                } else {
                    (i + 1) % rows
                };
                self.inspector_focus(g, next);
                true
            }
            "backspace" => {
                if let Some(t) = self.inspector.field.as_mut() {
                    if t.fresh {
                        t.text.clear();
                        t.fresh = false;
                    } else {
                        t.text.pop();
                    }
                }
                true
            }
            "escape" => {
                self.inspector_cancel();
                true
            }
            _ => {
                let Some(t) = self.inspector.field.as_mut() else {
                    return false;
                };
                for c in text.chars() {
                    t.put(kind, c);
                }
                true
            }
        }
    }

    /// A wheel turn over a field of the inspector (UI-313): a notch is one step, away from you
    /// raises the value, Shift is the arrows' further step and Ctrl is finer where the value
    /// keeps a fraction. The field keeps the keys afterwards.
    pub(crate) fn inspector_wheel(
        &mut self,
        g: &Geometry,
        at: (f32, f32),
        px: f32,
        shift: bool,
        ctrl: bool,
    ) -> bool {
        self.inspector_wheel_at(g, at, px, shift, ctrl, std::time::Instant::now())
    }

    /// The same with the time the turn happened (the tests pass a fixed one, P3-4 of review 15).
    pub(crate) fn inspector_wheel_at(
        &mut self,
        g: &Geometry,
        at: (f32, f32),
        px: f32,
        shift: bool,
        ctrl: bool,
        now: std::time::Instant,
    ) -> bool {
        let (x, y) = at;
        let Some(i) = self.inspector_layout(g).field_at(x, y) else {
            return false;
        };
        let Some(row) = self.inspector_layout(g).rows.get(i).cloned() else {
            return false;
        };
        // Another row starts its own turn of the wheel (P3-1).
        let same = self.inspector.field.is_some()
            && self
                .inspector
                .target
                .as_ref()
                .is_some_and(|target| target.is(&row));
        if !same {
            self.inspector_focus(g, i);
        }
        let whole = self.inspector.wheel.notches(px);
        if whole == 0 {
            return true;
        }
        let step = row.kind.wheel_step(shift, ctrl) * f64::from(whole);
        self.wheel_group_start_at(now, row.kind, &row.ids);
        self.wheel_running = true;
        let done = self.inspector_set(g, i, row.value.unwrap_or(0.0) + step);
        self.wheel_running = false;
        done
    }
}

/// Where a value's field sits in its row.
pub fn field_rect(r: Rect) -> Rect {
    Rect::new(
        r.right() - Space::X2.px() - FIELD_W,
        r.y + (r.h - FIELD_H) / 2.0,
        FIELD_W,
        FIELD_H,
    )
}

/// The field's text as it shows now: what was typed while the field has the keys, else the
/// value, with 「−」 for the sign (UI-311).
pub fn field_text(studio: &Studio, _i: usize, row: &Row) -> String {
    let focused = studio.inspector_open()
        && studio
            .inspector
            .target
            .as_ref()
            .is_some_and(|target| target.is(row));
    match studio.inspector.field.as_ref().filter(|_| focused) {
        Some(t) if !t.fresh => format!("{}{}", t.text.replace('-', "−"), t.marked),
        Some(t) => format!("{}{}", row.kind.text(row.value), t.marked),
        _ => row.kind.text(row.value),
    }
}

// -------------------------------------------------------------------- painting

/// The inspector's panel: its face, the chosen track and clip, and their fields.
pub fn paint(pen: &mut Pen, studio: &Studio, hover: Option<Part>, g: &Geometry) {
    let panel = g.panel;
    let c = pen.c;
    pen.round(panel, c.surface_2, Radius::M);
    let l = studio.inspector_layout(g);
    pen.mask(panel, |pen| paint_in(pen, studio, hover, &l));
}

fn paint_in(pen: &mut Pen, studio: &Studio, hover: Option<Part>, l: &Layout) {
    let c = pen.c;
    let p = studio.project();
    let chosen = Chosen::of(studio);
    if let (Some(r), Some(id)) = (l.header, chosen.track)
        && let Some(t) = p.tracks.get(&id)
    {
        let swatch = Rect::new(r.x + Space::X1.px(), r.y + (r.h - 12.0) / 2.0, 12.0, 12.0);
        pen.round(swatch, crate::draw::color_of(c, t), Radius::Xs);
        pen.icon(
            crate::draw::track_icon(t),
            Rect::new(r.x + 24.0, r.y + (r.h - ICON) / 2.0, ICON, ICON),
            c.ink,
        );
        let name = Rect::new(r.x + 46.0, r.y, r.w - 46.0, r.h);
        pen.mask(name, |pen| {
            pen.text(
                t.shown_name(),
                FontSize::Body,
                Weight::Semibold,
                pen.c.ink_strong,
                name,
                Align::Left,
            );
        });
    }
    if let (Some(r), Some(id)) = (l.clip_header, chosen.clip)
        && let Some(t) = chosen.track.and_then(|t| p.tracks.get(&t))
        && let Some(clip) = t.clips.iter().find(|c| c.id == id)
    {
        let icon = if clip.is_midi() { "keys" } else { "wave" };
        pen.icon(
            icon,
            Rect::new(r.x + Space::X1.px(), r.y + (r.h - ICON) / 2.0, ICON, ICON),
            c.ink,
        );
        let name = Rect::new(r.x + 28.0, r.y, r.w - 28.0, r.h);
        pen.mask(name, |pen| {
            pen.text(
                &clip.name,
                FontSize::Small,
                Weight::Semibold,
                pen.c.ink,
                name,
                Align::Left,
            );
        });
    }
    for (i, row) in l.rows.iter().enumerate() {
        let focused = studio.inspector_open()
            && studio
                .inspector
                .target
                .as_ref()
                .is_some_and(|target| target.is(row));
        let text = field_text(studio, i, row);
        let field = field_rect(row.rect);
        let label = Rect::new(
            row.rect.x + Space::X2.px(),
            row.rect.y,
            field.x - row.rect.x - Space::X2.px() * 2.0,
            row.rect.h,
        );
        let hot = hover == Some(Part::Field(i));
        if hot && !focused {
            pen.round(row.rect, c.hover, Radius::S);
        }
        pen.text(
            row.kind.label(),
            FontSize::Small,
            Weight::Medium,
            c.ink,
            label,
            Align::Left,
        );
        let cell = Cell::new(field, Look::Field)
            .text(text.clone())
            .spoken(row.kind.spoken(&text))
            .on(focused);
        fields::paint_cell(pen, &cell);
    }
    // Nothing chosen: the face stays empty (the words that used to stand here did not fit the
    // panel's width, and the person asked for less UI, UI-312).
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::RailTab;
    use crate::studio::Hit;
    use crate::tools::Mods;

    fn sample() -> (Studio, Geometry) {
        let s = Studio::sample();
        let g = s.geometry(1440.0, 860.0);
        (s, g)
    }

    /// An instrument track of the sample that has a MIDI clip.
    fn chosen(s: &Studio) -> (TrackId, ClipId) {
        let p = s.project();
        let t = p
            .tracks
            .values()
            .find(|t| t.kind == TrackKind::Instrument && t.clips.iter().any(|c| c.is_midi()))
            .expect("an instrument track with a MIDI clip");
        let c = t.clips.iter().find(|c| c.is_midi()).unwrap();
        (t.id, c.id)
    }

    #[test]
    fn the_rail_opens_the_inspector_and_shuts_it_again() {
        let (mut s, g) = sample();
        assert!(!s.inspector_open());
        assert!(s.press(&g, Hit::Rail(RailTab::Inspector)));
        assert!(s.inspector_open());
        assert_eq!(s.browser.tab, RailTab::Inspector);
        // The rail's face stays the left panel's width, as the browser's.
        assert!(g.panel.w > 0.0);
        assert!(s.press(&g, Hit::Rail(RailTab::Inspector)));
        assert!(!s.inspector_open());
        // Another face shuts the inspector, and there is only one open.
        assert!(s.press(&g, Hit::Rail(RailTab::Inspector)));
        assert!(s.press(&g, Hit::Rail(RailTab::Instruments)));
        assert!(s.browser.open);
        assert!(!s.inspector_open());
    }

    /// A key goes as the screen sends it (the same `Studio::edit_key` the window calls).
    fn screen_key(s: &mut Studio, g: &Geometry, key: &str, ch: Option<&str>, mods: Mods) -> bool {
        s.field_keys(g, key, ch, mods)
    }

    #[test]
    fn nothing_chosen_shows_an_empty_face_and_no_field() {
        let (mut s, g) = sample();
        s.selected_row = None;
        s.selected_clip = None;
        s.press(&g, Hit::Rail(RailTab::Inspector));
        let l = s.inspector_layout(&g);
        assert!(l.header.is_none());
        assert!(l.rows.is_empty());
        assert!(
            s.inspector_hit(&g, g.panel.x + 10.0, g.panel.y + 10.0)
                .is_none()
        );
    }

    #[test]
    fn a_track_shows_its_two_fields_and_an_instrument_clip_its_own() {
        let (mut s, g) = sample();
        let (t, c) = chosen(&s);
        s.selected_row = Some(RowKind::Track(t));
        s.selected_clip = None;
        s.press(&g, Hit::Rail(RailTab::Inspector));
        let l = s.inspector_layout(&g);
        assert!(l.header.is_some());
        assert_eq!(
            l.rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
            [Kind::TrackSemis, Kind::TrackVelocity]
        );
        // The clip adds its own transpose row.
        s.selected_clip = Some((t, c));
        s.arrange.clips = std::collections::BTreeSet::from([(t, c)]);
        let l = s.inspector_layout(&g);
        assert!(l.clip_header.is_some());
        assert_eq!(
            l.rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
            [Kind::TrackSemis, Kind::TrackVelocity, Kind::Semis]
        );
        assert_eq!(l.rows[2].ids, [c.to_string()]);
        // An audio clip has its transpose, its fine tune and its speed, as its menu has them.
        let p = s.project();
        let audio = p
            .tracks
            .values()
            .find(|t| t.kind == TrackKind::Audio && !t.clips.is_empty())
            .map(|t| (t.id, t.clips[0].id));
        if let Some((at, ac)) = audio {
            s.selected_row = Some(RowKind::Track(at));
            s.selected_clip = Some((at, ac));
            s.arrange.clips = std::collections::BTreeSet::from([(at, ac)]);
            let l = s.inspector_layout(&g);
            assert_eq!(
                l.rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
                [Kind::Semis, Kind::Cents, Kind::Speed]
            );
        }
    }

    /// An audio clip's speed keeps its fraction: the inspector passes it as it is, not rounded
    /// to a whole number (UI-312).
    #[test]
    fn an_audio_clips_speed_keeps_its_fraction() {
        let (mut s, g) = sample();
        let p = s.project();
        let (at, ac) = p
            .tracks
            .values()
            .find(|t| t.kind == TrackKind::Audio && !t.clips.is_empty())
            .map(|t| (t.id, t.clips[0].id))
            .expect("an audio clip");
        s.selected_row = Some(RowKind::Track(at));
        s.selected_clip = Some((at, ac));
        s.arrange.clips = std::collections::BTreeSet::from([(at, ac)]);
        s.press(&g, Hit::Rail(RailTab::Inspector));
        let rows = s.inspector_layout(&g).rows;
        let at_speed = rows
            .iter()
            .position(|r| r.kind == Kind::Speed)
            .expect("the speed's row");
        assert!(s.press(&g, Hit::Inspector(Part::Field(at_speed))));
        for c in "87.5".chars() {
            assert!(screen_key(
                &mut s,
                &g,
                &c.to_string(),
                Some(&c.to_string()),
                Mods::NONE
            ));
        }
        assert!(screen_key(&mut s, &g, "enter", None, Mods::NONE));
        let p = s.project();
        let (_, clip) = p.clip(ac).expect("the clip");
        let speed = match &clip.body {
            xirius_core::model::ClipBody::Audio { process, .. } => process.speed,
            _ => panic!("an audio clip"),
        };
        assert_eq!(speed, 87.5);
    }

    #[test]
    fn typing_in_a_field_sets_the_tracks_value() {
        let (mut s, g) = sample();
        let (t, _) = chosen(&s);
        s.selected_row = Some(RowKind::Track(t));
        s.selected_clip = None;
        s.press(&g, Hit::Rail(RailTab::Inspector));
        let row = s.inspector_layout(&g).rows[0].rect;
        let at = (row.x + 30.0, row.y + row.h / 2.0);
        let hit = s.hit(&g, at.0, at.1);
        assert!(matches!(hit, Hit::Inspector(Part::Field(0))), "{hit:?}");
        assert!(s.press(&g, hit));
        assert!(s.inspector_typing().is_some());
        for c in "7".chars() {
            assert!(screen_key(
                &mut s,
                &g,
                "7",
                Some(&c.to_string()),
                Mods::NONE
            ));
        }
        assert!(screen_key(&mut s, &g, "enter", None, Mods::NONE));
        assert_eq!(s.project().tracks[&t].transpose, 7);
        // ↓ takes one back; the panel stays open and its field keeps the keys.
        assert!(screen_key(&mut s, &g, "down", None, Mods::NONE));
        assert_eq!(s.project().tracks[&t].transpose, 6);
        assert!(s.inspector_typing().is_some());
    }

    /// The half-typed value belongs to its track and its kind, not to a row number: choosing
    /// another track gives it up instead of writing it there (UI-312).
    #[test]
    fn a_half_typed_value_is_not_given_to_another_track() {
        let (mut s, g) = sample();
        let (a, _) = chosen(&s);
        let b = s
            .project()
            .tracks
            .values()
            .find(|t| t.kind == TrackKind::Instrument && t.id != a)
            .map(|t| t.id)
            .expect("another instrument track");
        s.selected_row = Some(RowKind::Track(a));
        s.selected_clip = None;
        s.press(&g, Hit::Rail(RailTab::Inspector));
        assert!(s.press(&g, Hit::Inspector(Part::Field(0))));
        assert!(screen_key(&mut s, &g, "7", Some("7"), Mods::NONE));
        assert!(s.press(&g, Hit::Row(RowKind::Track(b))));
        assert!(s.inspector_typing().is_none(), "the typing was given up");
        // Enter at the new track's field writes nothing of the old track's.
        assert!(s.press(&g, Hit::Inspector(Part::Field(0))));
        assert!(screen_key(&mut s, &g, "enter", None, Mods::NONE));
        assert_eq!(s.project().tracks[&b].transpose, 0);
    }

    /// A press outside the face gives the half-typed value up, and so does starting another
    /// field (UI-312).
    #[test]
    fn a_press_outside_the_face_leaves_the_typing() {
        let (mut s, g) = sample();
        let (t, _) = chosen(&s);
        s.selected_row = Some(RowKind::Track(t));
        s.selected_clip = None;
        s.press(&g, Hit::Rail(RailTab::Inspector));
        assert!(s.press(&g, Hit::Inspector(Part::Field(0))));
        assert!(screen_key(&mut s, &g, "7", Some("7"), Mods::NONE));
        assert!(!s.press(&g, Hit::None), "nothing was hit");
        assert!(s.inspector_typing().is_none());
        assert_eq!(s.project().tracks[&t].transpose, 0, "nothing was written");
        // Starting the other field of the same track moves the keys there.
        assert!(s.press(&g, Hit::Inspector(Part::Field(0))));
        assert!(screen_key(&mut s, &g, "5", Some("5"), Mods::NONE));
        assert!(s.press(&g, Hit::Inspector(Part::Field(1))));
        let target = s.inspector.target.as_ref().expect("a target");
        assert_eq!(target.kind, Kind::TrackVelocity);
        assert_eq!(s.project().tracks[&t].transpose, 0, "5 was given up");
    }

    /// F4, Space and Ctrl+Z are the screen's keys while a field has the keys; the field keeps
    /// what was typed and the command still runs (UI-312).
    #[test]
    fn the_screens_keys_go_past_a_focused_field() {
        let (mut s, g) = sample();
        let (t, _) = chosen(&s);
        s.selected_row = Some(RowKind::Track(t));
        s.selected_clip = None;
        assert!(s.press(&g, Hit::Rail(RailTab::Inspector)));
        assert!(s.inspector_open());
        assert!(s.press(&g, Hit::Inspector(Part::Field(0))));
        assert!(screen_key(&mut s, &g, "5", Some("5"), Mods::NONE));
        assert!(!screen_key(&mut s, &g, "f4", None, Mods::NONE));
        assert!(!screen_key(&mut s, &g, "space", Some(" "), Mods::NONE));
        assert!(!screen_key(
            &mut s,
            &g,
            "z",
            Some("z"),
            Mods {
                ctrl: true,
                ..Mods::NONE
            }
        ));
        assert!(s.inspector_typing().is_some(), "the typing stays");
        // The screen's own handling still has the keys.
        assert!(s.mapped_key(&g, "f4", None, Mods::NONE));
        assert!(!s.inspector_open(), "F4 shut the face");
        assert!(
            s.inspector_typing().is_none(),
            "the face gave the typing up"
        );
    }

    /// A press on the top bar that its own handler takes also leaves the field (UI-312).
    #[test]
    fn a_press_on_the_bar_leaves_the_field() {
        let (mut s, g) = sample();
        let (t, _) = chosen(&s);
        s.selected_row = Some(RowKind::Track(t));
        s.selected_clip = None;
        s.press(&g, Hit::Rail(RailTab::Inspector));
        assert!(s.press(&g, Hit::Inspector(Part::Field(0))));
        assert!(screen_key(&mut s, &g, "7", Some("7"), Mods::NONE));
        assert!(s.inspector_typing().is_some());
        let r = s.bar_rect(&g, crate::topbar::BarButton::Tempo);
        assert!(
            s.bar_down(&g, r.x + r.w / 2.0, r.y + r.h / 2.0, 1),
            "the tempo readout took the press"
        );
        assert!(s.inspector_typing().is_none(), "the field was left");
    }

    /// A character the field does not take leaves the value it shows, as main's UI-311 fix
    /// does for the clip menu (UI-312).
    #[test]
    fn a_character_the_field_does_not_take_leaves_the_value_shown() {
        let (mut s, g) = sample();
        let (t, _) = chosen(&s);
        s.selected_row = Some(RowKind::Track(t));
        s.selected_clip = None;
        assert!(s.run_command(
            "track.transpose",
            serde_json::json!({"ids": [t.to_string()], "semis": 2})
        ));
        s.press(&g, Hit::Rail(RailTab::Inspector));
        assert!(s.press(&g, Hit::Inspector(Part::Field(0))));
        assert!(screen_key(&mut s, &g, "x", Some("x"), Mods::NONE));
        assert!(screen_key(&mut s, &g, "q", Some("q"), Mods::NONE));
        let t0 = s.inspector_typing().expect("still typing");
        assert!(t0.fresh, "the value is still shown, not an empty field");
        assert_eq!(field_text(&s, 0, &s.inspector_layout(&g).rows[0]), "+2");
        assert!(screen_key(&mut s, &g, "enter", None, Mods::NONE));
        assert_eq!(s.project().tracks[&t].transpose, 2, "nothing changed");
    }

    #[test]
    fn pressing_another_track_changes_what_the_inspector_shows() {
        let (mut s, g) = sample();
        s.press(&g, Hit::Rail(RailTab::Inspector));
        let keys = s.inspector_layout(&g).rows[0].ids.clone();
        let bass = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Bass" && t.kind == TrackKind::Instrument)
            .map(|t| t.id)
            .expect("the sample's Bass");
        assert_ne!(keys, [bass.to_string()]);
        assert!(s.press(&g, Hit::Row(RowKind::Track(bass))));
        assert_eq!(s.inspector_layout(&g).rows[0].ids, [bass.to_string()]);
        // The clip of the track chosen before is not shown with the new track.
        assert!(s.inspector_layout(&g).clip_header.is_none());
    }

    /// An IME's full-width digit is the field's, as on the clip menu (UI-313).
    #[test]
    fn an_ime_full_width_digit_sets_the_inspectors_value() {
        let (mut s, g) = sample();
        let (t, _) = chosen(&s);
        s.selected_row = Some(RowKind::Track(t));
        s.selected_clip = None;
        s.arrange.clips.clear();
        s.press(&g, Hit::Rail(RailTab::Inspector));
        assert!(s.press(&g, Hit::Inspector(Part::Field(0))));
        assert!(screen_key(&mut s, &g, "５", Some("５"), Mods::NONE));
        assert!(screen_key(&mut s, &g, "enter", None, Mods::NONE));
        assert_eq!(s.project().tracks[&t].transpose, 5);
    }

    #[test]
    fn choosing_a_clip_puts_the_highlight_row_and_inspector_together() {
        let (mut s, g) = sample();
        let (t, c) = chosen(&s);
        s.select_clip_here(t, c);
        assert_eq!(s.selected_clip, Some((t, c)));
        assert_eq!(
            s.arrange.clips.iter().copied().collect::<Vec<_>>(),
            [(t, c)]
        );
        assert_eq!(s.selected_row, Some(RowKind::Track(t)));
        s.press(&g, Hit::Rail(RailTab::Inspector));
        assert!(s.inspector_layout(&g).clip_header.is_some());
    }

    #[test]
    fn a_track_press_keeps_the_clip_the_piano_roll_shows() {
        let (mut s, g) = sample();
        let (t, c) = chosen(&s);
        s.select_clip_here(t, c);
        s.drawer_open = true;
        s.tab = crate::studio::DrawerTab::Roll;
        s.arrange_run(
            "track.transpose",
            serde_json::json!({ "ids": [t.to_string()], "semis": -12 }),
        );
        assert_eq!(
            s.roll_target().map(|r| r.clip.id),
            Some(c),
            "the roll shows the clip"
        );
        s.press(&g, Hit::Rail(RailTab::Inspector));
        assert!(s.inspector_layout(&g).clip_header.is_some());
        // Pressing the track's head takes the clip's part away from the inspector, but the clip
        // the roll edits stays (P2-5 of review 15).
        assert!(s.press(&g, Hit::Row(RowKind::Track(t))));
        assert_eq!(s.selected_clip, Some((t, c)));
        assert_eq!(s.roll_target().map(|r| r.clip.id), Some(c));
        assert!(s.inspector_layout(&g).clip_header.is_none());
    }

    #[test]
    fn a_turn_left_in_one_inspector_field_does_not_move_the_next() {
        let (mut s, g) = sample();
        let (t, _) = chosen(&s);
        s.selected_row = Some(RowKind::Track(t));
        s.selected_clip = None;
        s.arrange.clips.clear();
        s.press(&g, Hit::Rail(RailTab::Inspector));
        let rows = s.inspector_layout(&g).rows;
        let first = rows[0].rect;
        let second = rows[1].rect;
        let mut now = std::time::Instant::now();
        now += std::time::Duration::from_millis(10);
        assert!(s.inspector_wheel_at(
            &g,
            (first.x + 30.0, first.y + first.h / 2.0),
            40.0,
            false,
            false,
            now
        ));
        now += std::time::Duration::from_millis(10);
        assert!(s.inspector_wheel_at(
            &g,
            (second.x + 30.0, second.y + second.h / 2.0),
            20.0,
            false,
            false,
            now
        ));
        assert_eq!(s.project().tracks[&t].velocity, 0);
    }

    #[test]
    fn a_track_press_and_an_empty_lane_leave_the_clip_part_out() {
        let (mut s, g) = sample();
        let (t, c) = chosen(&s);
        s.selected_row = Some(RowKind::Track(t));
        s.selected_clip = Some((t, c));
        s.arrange.clips = std::collections::BTreeSet::from([(t, c)]);
        s.press(&g, Hit::Rail(RailTab::Inspector));
        assert!(s.inspector_layout(&g).clip_header.is_some());
        // The track's head: the clip is not chosen in the arrangement any more (UI-313).
        assert!(s.press(&g, Hit::Row(RowKind::Track(t))));
        assert!(s.inspector_layout(&g).clip_header.is_none());
        // A press on an empty lane also takes the clip's part away.
        s.selected_clip = Some((t, c));
        s.arrange.clips = std::collections::BTreeSet::from([(t, c)]);
        let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
        let end = s.project().tracks[&t]
            .clips
            .iter()
            .map(|c| c.end())
            .max()
            .unwrap_or(0);
        let at = (
            s.tick_x(&g, end + 3840),
            s.lane_top(&g, &row, 0) + row.h / 2.0,
        );
        assert!(at.0 < g.lanes.right(), "{at:?}");
        s.arrange_down(&g, at.0, at.1, Mods::NONE);
        s.arrange_up(&g, at.0, at.1, Mods::NONE);
        assert!(s.arrange.clips.is_empty());
        assert!(s.inspector_layout(&g).clip_header.is_none());
        // A press on the clip brings it back.
        let (x, y) = crate::arrange::probe::clip_mid(&s, &g, "Keys", 0);
        s.arrange_down(&g, x, y, Mods::NONE);
        s.arrange_up(&g, x, y, Mods::NONE);
        assert!(s.inspector_layout(&g).clip_header.is_some());
    }

    #[test]
    fn a_screen_reader_hears_the_values_with_their_units() {
        let (mut s, g) = sample();
        let (t, _) = chosen(&s);
        s.selected_row = Some(RowKind::Track(t));
        s.selected_clip = None;
        assert!(s.run_command(
            "track.transpose",
            serde_json::json!({"ids": [t.to_string()], "semis": 2})
        ));
        assert!(s.run_command(
            "track.velocity",
            serde_json::json!({"ids": [t.to_string()], "percent": 10})
        ));
        s.press(&g, Hit::Rail(RailTab::Inspector));
        let said: Vec<String> = crate::a11y::marks(&s, &g)
            .into_iter()
            .filter_map(|m| m.node.label().map(str::to_string))
            .collect();
        assert!(
            said.contains(&"トランスポーズ +2 半音".to_string()),
            "{said:?}"
        );
        assert!(said.contains(&"ベロシティ +10%".to_string()), "{said:?}");
    }

    #[test]
    fn a_wheel_turn_over_a_field_changes_it_a_notch_at_a_time() {
        let (mut s, g) = sample();
        let (t, _) = chosen(&s);
        s.selected_row = Some(RowKind::Track(t));
        s.selected_clip = None;
        s.arrange.clips.clear();
        s.press(&g, Hit::Rail(RailTab::Inspector));
        let row = s.inspector_layout(&g).rows[1].rect;
        let at = (row.x + 30.0, row.y + row.h / 2.0);
        // Fixed times: the whole turn is one undo step whatever the machine's load (P3-4 of
        // review 15).
        let mut now = std::time::Instant::now();
        let mut turn = |s: &mut Studio, px: f32| {
            now += std::time::Duration::from_millis(10);
            s.inspector_wheel_at(&g, at, px, false, false, now)
        };
        // Away from you raises it (UI-313).
        assert!(turn(&mut s, 60.0));
        assert_eq!(s.project().tracks[&t].velocity, 1);
        // A touchpad's small turns add up to a notch.
        for _ in 0..3 {
            assert!(turn(&mut s, 20.0));
        }
        assert_eq!(s.project().tracks[&t].velocity, 2);
        // Turning back throws away what was gathered the other way.
        assert!(turn(&mut s, 20.0));
        assert!(turn(&mut s, -20.0));
        assert_eq!(s.project().tracks[&t].velocity, 2);
        assert!(turn(&mut s, -20.0));
        assert!(turn(&mut s, -20.0));
        assert_eq!(s.project().tracks[&t].velocity, 1);
        // The field keeps the keys.
        assert!(s.inspector_typing().is_some());
        // One undo takes the whole turn back.
        s.undo(&g);
        assert_eq!(s.project().tracks[&t].velocity, 0);
    }

    #[test]
    fn shift_and_ctrl_change_how_far_a_wheel_notch_goes() {
        let (mut s, g) = sample();
        let p = s.project();
        let (at, ac) = p
            .tracks
            .values()
            .find(|t| t.kind == TrackKind::Audio && !t.clips.is_empty())
            .map(|t| (t.id, t.clips[0].id))
            .expect("an audio clip");
        s.selected_row = Some(RowKind::Track(at));
        s.selected_clip = Some((at, ac));
        s.arrange.clips = std::collections::BTreeSet::from([(at, ac)]);
        s.press(&g, Hit::Rail(RailTab::Inspector));
        let rows = s.inspector_layout(&g).rows;
        let semis = rows.iter().find(|r| r.kind == Kind::Semis).unwrap().rect;
        let semis_at = (semis.x + 30.0, semis.y + semis.h / 2.0);
        // Windows sends Shift + wheel sideways, 20 px for one character: still one notch.
        assert!(s.window_wheel(&g, semis_at, 20.0, 0.0, Mods::shift(), false));
        let semis_value = s
            .inspector_layout(&g)
            .rows
            .iter()
            .find(|r| r.kind == Kind::Semis)
            .unwrap()
            .value;
        assert_eq!(semis_value, Some(12.0));
        let speed = s
            .inspector_layout(&g)
            .rows
            .iter()
            .find(|r| r.kind == Kind::Speed)
            .unwrap()
            .rect;
        let speed_at = (speed.x + 30.0, speed.y + speed.h / 2.0);
        assert!(s.window_wheel(
            &g,
            speed_at,
            0.0,
            60.0,
            Mods {
                ctrl: true,
                ..Mods::NONE
            },
            true
        ));
        let speed_value = s
            .inspector_layout(&g)
            .rows
            .iter()
            .find(|r| r.kind == Kind::Speed)
            .unwrap()
            .value;
        assert!(
            (speed_value.unwrap() - 100.1).abs() < 1e-9,
            "{speed_value:?}"
        );
    }
}
