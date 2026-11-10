//! The macro controls (MIX-041): the row of Macro 1–8 under the channel editor's toolbar, and
//! their own window with the eight in four columns of two at the left and one X/Y pad at the
//! right. The pad moves the two knobs its picks under it choose. Without its pin the window
//! shows the chosen channel's macros, as a device window does. The window is an independent
//! one of a fixed size (UI-337, opened by `device_window`): its view is laid out from (0, 0) of
//! its own surface, and the system moves it by the bar's empty part.

pub mod assign;
#[cfg(test)]
mod assign_tests;
pub mod paint;
#[cfg(test)]
mod tests;

use serde_json::json;
use xirius_core::controls::{ControlId, KNOBS};
use xirius_core::id::TrackId;
use xirius_core::model::{Project, Track};
use xirius_session::autowrite::Phase;

use crate::mixer::MixerHit;
use crate::popup::PopupKind;
use crate::studio::{Rect, Studio};
use crate::title_bar::{self, BAR_H, Bar, Seats};
use crate::tools::Mods;
use crate::tree::RowKind;

/// The window's name in its bar, after the track's.
pub const NAME: &str = "Macro Controls";
/// The room round the window's content and between the knobs and the pad.
const ROOM: f32 = 16.0;
/// The room over the content, under the bar.
const TOP: f32 = 12.0;
/// A knob's cell in the window's grid.
pub const CELL: f32 = 88.0;
/// The pad's side.
pub const SIDE: f32 = 216.0;
/// The picks under the pad.
pub const PICK_H: f32 = 28.0;
const PICK_GAP: f32 = 8.0;
const PICK_L: f32 = 10.0;
const PICK_IN: f32 = 6.0;
pub const CHEV: f32 = 12.0;
const PICK_R: f32 = 8.0;
/// The window's size.
pub const W: f32 = 3.0 * ROOM + 4.0 * CELL + SIDE;
pub const H: f32 = BAR_H + TOP + SIDE + TOP + PICK_H + ROOM;
/// A dial and the lines under it: the value, then the name.
pub const DIAL: f32 = 26.0;
pub const DIAL_GAP: f32 = 4.0;
pub const LINE_H: f32 = 16.0;
pub const KNOB_H: f32 = DIAL + DIAL_GAP + 2.0 * LINE_H;
/// One step of the pad's arrow keys, and with Shift.
const STEP: f64 = 0.01;
const BIG_STEP: f64 = 0.1;

/// The axes' letters, as the picks and a reader say them.
pub const AXES: [&str; 2] = ["X", "Y"];

/// The macro window.
#[derive(Clone, Debug, PartialEq)]
pub struct MacroWin {
    /// The track it shows while pinned, and while no track is chosen.
    pub track: TrackId,
    pub pinned: bool,
    /// The pad has the keys: ←→↑↓ move it.
    pub focused: bool,
    /// The track that was chosen when it opened or was pinned: until another is chosen, it
    /// keeps its own.
    pub seen: Option<TrackId>,
    pub hot: Option<Part>,
    pub(crate) grab: Option<Grab>,
}

impl MacroWin {
    pub fn new(track: TrackId) -> MacroWin {
        MacroWin {
            track,
            pinned: false,
            focused: false,
            seen: None,
            hot: None,
            grab: None,
        }
    }
}

/// What in the window is held.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Grab {
    /// The pad's point on the track it was pressed for: where it was and where it is now (0..1
    /// across and up).
    Pad {
        track: TrackId,
        from: (f64, f64),
        now: (f64, f64),
    },
}

/// A part of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Close,
    Pin,
    Head,
    Knob(u8),
    Pad,
    /// The pick of the axis (`0` X, `1` Y).
    Pick(usize),
    Body,
}

/// The window as it is laid out.
#[derive(Clone, Debug)]
pub struct View {
    pub r: Rect,
    pub track: TrackId,
    pub name: String,
    pub bar: Bar,
    pub knobs: [Rect; KNOBS],
    pub pad: Rect,
    pub picks: [Rect; 2],
}

impl View {
    pub fn bar_rect(&self) -> Rect {
        Rect::new(self.r.x, self.r.y, self.r.w, BAR_H)
    }
}

/// A knob's dial in its cell: centred at the top.
pub fn dial_rect(cell: Rect) -> Rect {
    Rect::new(cell.x + (cell.w - DIAL) / 2.0, cell.y, DIAL, DIAL)
}

/// The pick's text, «X Macro 1», and its width.
pub fn pick_w(name: &str) -> f32 {
    use crate::draw::estimated_text_width;
    use xirius_design::FontSize;
    PICK_L
        + estimated_text_width("X", FontSize::Small) * 1.12
        + PICK_IN
        + estimated_text_width(name, FontSize::Small)
        + PICK_IN
        + CHEV
        + PICK_R
}

/// The window for the track `t`: the whole of its own surface, which is [`W`] by [`H`].
pub fn view(t: &Track) -> View {
    let r = Rect::new(0.0, 0.0, W, H);
    let name = t.shown_name().to_string();
    let bar = title_bar::layout_plain(
        Rect::new(r.x, r.y, r.w, BAR_H),
        &name,
        NAME,
        Seats::default(),
    );
    let top = r.y + BAR_H + TOP;
    let knobs = std::array::from_fn(|i| {
        let (col, row) = ((i % 4) as f32, (i / 4) as f32);
        Rect::new(r.x + ROOM + col * CELL, top + row * CELL, CELL, KNOB_H)
    });
    let pad = Rect::new(r.x + 2.0 * ROOM + 4.0 * CELL, top, SIDE, SIDE);
    let [xk, yk] = t.controls.xy_knobs();
    let yw = pick_w(&t.control_label(yk));
    let xw = pick_w(&t.control_label(xk));
    let py = pad.bottom() + TOP;
    let ypick = Rect::new(pad.right() - yw, py, yw, PICK_H);
    let xpick = Rect::new(ypick.x - PICK_GAP - xw, py, xw, PICK_H);
    View {
        r,
        track: t.id,
        name,
        bar,
        knobs,
        pad,
        picks: [xpick, ypick],
    }
}

/// The pad's point for its two values: across from the left, up from the bottom.
pub fn pad_point(pad: Rect, (x, y): (f64, f64)) -> (f32, f32) {
    (pad.x + pad.w * x as f32, pad.bottom() - pad.h * y as f32)
}

/// The pad's two values at a point on it, held to its edges, by the hundredth.
pub fn pad_values(pad: Rect, x: f32, y: f32) -> (f64, f64) {
    let v = |n: f32| (f64::from(n.clamp(0.0, 1.0)) * 100.0).round() / 100.0;
    (v((x - pad.x) / pad.w), v((pad.bottom() - y) / pad.h))
}

/// What a reader hears of the pad: «X Macro 1 40 %, Y Macro 2 70 %».
pub fn pad_text(t: &Track, (x, y): (f64, f64)) -> String {
    let [xk, yk] = t.controls.xy_knobs();
    format!(
        "X {} {:.0} %, Y {} {:.0} %",
        t.control_label(xk),
        x * 100.0,
        t.control_label(yk),
        y * 100.0
    )
}

/// A knob's value as the window says it: «40 %».
pub fn percent(v: f64) -> String {
    format!("{:.0} %", v * 100.0)
}

/// The parts of the bar that move the window (UI-337): what [`part_at`] calls the head. The
/// system moves the window by them (`WindowControlArea::Drag`).
pub fn drag_rects(v: &View) -> Vec<Rect> {
    title_bar::drag_rects(v.bar_rect(), &v.bar, |x, y| {
        part_at(v, x, y) == Some(Part::Head)
    })
}

/// The part under the point.
pub fn part_at(v: &View, x: f32, y: f32) -> Option<Part> {
    if !v.r.contains(x, y) {
        return None;
    }
    let b = &v.bar;
    let part = if b.close.contains(x, y) {
        Part::Close
    } else if b.pin.contains(x, y) {
        Part::Pin
    } else if v.bar_rect().contains(x, y) {
        Part::Head
    } else if v.pad.contains(x, y) {
        Part::Pad
    } else if let Some(i) = v.picks.iter().position(|p| p.contains(x, y)) {
        Part::Pick(i)
    } else if let Some(i) = v.knobs.iter().position(|k| k.contains(x, y)) {
        Part::Knob(i as u8)
    } else {
        Part::Body
    };
    Some(part)
}

impl Studio {
    /// The track the macro window shows: its own while pinned or while no track is chosen,
    /// else the chosen one.
    pub fn macro_track(&self) -> Option<TrackId> {
        let w = self.macro_pad.as_ref()?;
        let p = self.project();
        let chosen = self
            .chosen_row_track()
            .filter(|t| !w.pinned && w.seen != Some(*t) && p.tracks.contains_key(t));
        chosen.or_else(|| p.tracks.contains_key(&w.track).then_some(w.track))
    }

    /// The macro window as it is laid out.
    pub fn macro_view(&self) -> Option<View> {
        let t = self.macro_track()?;
        let p = self.project();
        Some(view(p.tracks.get(&t)?))
    }

    /// Before each frame: an unpinned window takes the track chosen since it last looked.
    pub fn macro_follow(&mut self) {
        let Some(chosen) = self.chosen_row_track() else {
            return;
        };
        if !self.project().tracks.contains_key(&chosen) {
            return;
        }
        if let Some(w) = self.macro_pad.as_mut()
            && !w.pinned
            && w.seen != Some(chosen)
        {
            w.track = chosen;
            w.seen = Some(chosen);
        }
    }

    /// The chosen track: its row's, else the chosen clip's.
    fn chosen_row_track(&self) -> Option<TrackId> {
        match self.selected_row {
            Some(RowKind::Track(t)) => Some(t),
            _ => self.selected_clip.map(|(t, _)| t),
        }
    }

    /// Opens the macro window on `t`; a pinned one stays on its own track.
    pub fn open_macros(&mut self, t: TrackId) {
        if !self.project().tracks.contains_key(&t) {
            return;
        }
        let seen = self.chosen_row_track();
        match &mut self.macro_pad {
            Some(w) if w.pinned => {}
            Some(w) => {
                w.track = t;
                w.seen = seen;
            }
            None => {
                self.macro_pad = Some(MacroWin {
                    seen,
                    ..MacroWin::new(t)
                })
            }
        }
    }

    /// The pad's values now: the held point's while it is dragged.
    pub fn pad_now(&self, t: &Track) -> (f64, f64) {
        if let Some(Grab::Pad { now, .. }) = self.macro_pad.as_ref().and_then(|w| w.grab) {
            return now;
        }
        let [xk, yk] = t.controls.xy_knobs();
        let v = |k: ControlId| {
            self.mixer_value(MixerHit::Control(t.id, k))
                .unwrap_or(t.controls.get(k).value)
        };
        (v(xk), v(yk))
    }

    /// A press: true when the window took it. A press elsewhere takes the keys from the pad.
    pub fn macro_press(&mut self, x: f32, y: f32) -> bool {
        let Some(v) = self.macro_view() else {
            return false;
        };
        let Some(part) = part_at(&v, x, y) else {
            if let Some(w) = self.macro_pad.as_mut() {
                w.focused = false;
            }
            return false;
        };
        self.error = None;
        let t = v.track;
        let Some(w) = self.macro_pad.as_mut() else {
            return false;
        };
        w.focused = part == Part::Pad;
        match part {
            Part::Close => self.macro_pad = None,
            Part::Pin => {
                // what it shows now is what it keeps
                w.track = t;
                w.pinned = !w.pinned;
                w.seen = match self.selected_row {
                    Some(RowKind::Track(c)) => Some(c),
                    _ => self.selected_clip.map(|(c, _)| c),
                };
            }
            // the system moves the window by its head: the surface starts that
            Part::Head => {}
            Part::Knob(i) => {
                self.control_drag(t, ControlId::Knob(i), y);
            }
            Part::Pad => {
                let p = self.project();
                let Some(track) = p.tracks.get(&t) else {
                    return true;
                };
                if self.pad_locked(t) {
                    return true;
                }
                let from = self.pad_now(track);
                let now = pad_values(v.pad, x, y);
                if let Some(w) = self.macro_pad.as_mut() {
                    w.grab = Some(Grab::Pad {
                        track: t,
                        from,
                        now,
                    });
                }
                // as a knob's drag does: while writing, the lanes take the pad's way
                self.pad_touch(t, Phase::Begin, from);
                self.pad_to(t, now);
            }
            Part::Pick(axis) => self.open_popup(PopupKind::MacroAxis(t, axis as u8), v.picks[axis]),
            Part::Body => {}
        }
        true
    }

    /// A row of an axis's pick list: the axis takes the knob (the other axis takes its old one).
    pub(crate) fn macro_axis(&mut self, t: TrackId, axis: u8, k: u8) -> bool {
        self.popup = None;
        let axis = AXES[usize::from(axis.min(1))].to_lowercase();
        let control = ControlId::Knob(k).to_string();
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            "control.xy.assign",
            json!({ "track": t.to_string(), "axis": axis, "control": control }),
        );
        true
    }

    /// A knob of the pad is held by its lane (Read): the pad does not move by hand.
    fn pad_locked(&self, t: TrackId) -> bool {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return true;
        };
        track
            .controls
            .xy_knobs()
            .into_iter()
            .any(|k| self.mixer_locked(MixerHit::Control(t, k)))
    }

    /// Both knobs of the pad touched as one, for the lanes being written. True when written.
    fn pad_touch(&mut self, t: TrackId, phase: Phase, (x, y): (f64, f64)) -> bool {
        let Some(track) = self.project().tracks.get(&t).cloned() else {
            return false;
        };
        let [xk, yk] = track.controls.xy_knobs();
        let a = self.auto_touch(MixerHit::Control(t, xk), phase, Some(x));
        let b = self.auto_touch(MixerHit::Control(t, yk), phase, Some(y));
        a | b
    }

    /// The point moved with the pad held: the lanes being written take it, else the player
    /// hears it.
    fn pad_to(&mut self, t: TrackId, now: (f64, f64)) {
        if !self.pad_touch(t, Phase::Change, now) {
            self.pad_heard(t, now);
        }
    }

    /// While the pad is held, the player hears its two knobs where the point is.
    fn pad_heard(&mut self, t: TrackId, (x, y): (f64, f64)) {
        if !self.transport.is_live() {
            return;
        }
        let mut p = (*self.project()).clone();
        if let Ok(track) = p.track_mut(t) {
            let [xk, yk] = track.controls.xy_knobs();
            for (k, v) in [(xk, x), (yk, y)] {
                track.controls.get_mut(k).value = v;
                track.apply_control_at(k, v);
            }
        }
        let heard = self.auto_heard(&p);
        self.transport.sync(&heard, self.song().dir());
    }

    /// The pointer moves with something of the window held.
    pub fn macro_moved(&mut self, x: f32, y: f32) -> bool {
        let Some(grab) = self.macro_pad.as_ref().and_then(|w| w.grab) else {
            return false;
        };
        let Grab::Pad { track, from, now } = grab;
        let Some(v) = self.macro_view() else {
            return false;
        };
        let next = pad_values(v.pad, x, y);
        if next == now {
            return true;
        }
        if let Some(w) = self.macro_pad.as_mut() {
            w.grab = Some(Grab::Pad {
                track,
                from,
                now: next,
            });
        }
        self.pad_to(track, next);
        true
    }

    /// The press ends: a moved pad is one step for both its knobs.
    pub fn macro_up(&mut self) -> bool {
        let Some(grab) = self.macro_pad.as_mut().and_then(|w| w.grab.take()) else {
            return false;
        };
        // the track it was pressed for, whichever the window shows now
        let Grab::Pad {
            track: t,
            from,
            now,
        } = grab;
        if self.pad_touch(t, Phase::End, now) {
            return true;
        }
        if now == from {
            let p = self.project();
            self.transport.sync(&p, self.song().dir());
        } else {
            self.pad_set(t, now);
        }
        true
    }

    fn pad_set(&mut self, t: TrackId, (x, y): (f64, f64)) {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            "control.xy",
            json!({ "track": t.to_string(), "x": x, "y": y }),
        );
    }

    /// The pointer moved over the screen: what of the window it is on.
    pub fn macro_hover(&mut self, x: f32, y: f32) -> bool {
        let hot = self.macro_view().and_then(|v| part_at(&v, x, y));
        let Some(w) = self.macro_pad.as_mut() else {
            return false;
        };
        if w.hot == hot {
            return false;
        }
        w.hot = hot;
        true
    }

    /// The pad's keys while it has them: ←→↑↓ move it by a hundredth (Shift, a tenth), each
    /// press one step; Esc lets the keys go.
    pub(crate) fn macro_key(&mut self, key: &str, mods: Mods) -> bool {
        if !self.macro_pad.as_ref().is_some_and(|w| w.focused) || mods.ctrl || mods.alt {
            return false;
        }
        let Some(t) = self.macro_track() else {
            return false;
        };
        let step = if mods.shift { BIG_STEP } else { STEP };
        let (dx, dy) = match key {
            "left" => (-step, 0.0),
            "right" => (step, 0.0),
            "down" => (0.0, -step),
            "up" => (0.0, step),
            "escape" => {
                if let Some(w) = self.macro_pad.as_mut() {
                    w.focused = false;
                }
                return true;
            }
            _ => return false,
        };
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return false;
        };
        if self.pad_locked(t) {
            return true;
        }
        let (x, y) = self.pad_now(track);
        let to = |v: f64, d: f64| ((v + d).clamp(0.0, 1.0) * 100.0).round() / 100.0;
        let next = (to(x, dx), to(y, dy));
        if next != (x, y) {
            self.pad_set(t, next);
        }
        true
    }
}

/// The reader's marks of the macro window, in the window's own coordinates (its own tree).
pub fn marks(s: &Studio) -> Vec<crate::a11y::Mark> {
    use crate::a11y::{Mark, local_bounds as bounds};
    use gpui::accesskit;
    let Some(w) = &s.macro_pad else {
        return Vec::new();
    };
    let Some(v) = s.macro_view() else {
        return Vec::new();
    };
    let p = s.project();
    let Some(t) = p.tracks.get(&v.track) else {
        return Vec::new();
    };
    let mark = |key: String, node: accesskit::Node| Mark {
        key,
        children: Vec::new(),
        node,
        fold: None,
        pick: None,
        press: None,
        turn: None,
        edge: None,
    };
    let button = |name: &str, r: Rect| {
        let mut node = accesskit::Node::new(accesskit::Role::Button);
        node.set_label(name);
        node.set_bounds(bounds(r));
        node
    };
    let mut children = Vec::new();
    let mut pin = button("ピン留め", v.bar.pin);
    pin.set_toggled(if w.pinned {
        accesskit::Toggled::True
    } else {
        accesskit::Toggled::False
    });
    children.push(mark("macros-pin".into(), pin));
    children.push(mark("macros-close".into(), button("閉じる", v.bar.close)));
    for (i, cell) in v.knobs.iter().enumerate() {
        let id = ControlId::Knob(i as u8);
        let value = s
            .mixer_value(MixerHit::Control(t.id, id))
            .unwrap_or(t.controls.get(id).value);
        let mut node = accesskit::Node::new(accesskit::Role::Slider);
        node.set_label(t.control_label(id));
        node.set_bounds(bounds(*cell));
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(100.0);
        node.set_numeric_value((value * 100.0).round());
        node.set_value(percent(value));
        children.push(mark(format!("macros-knob-{i}"), node));
    }
    let now = s.pad_now(t);
    let mut pad = accesskit::Node::new(accesskit::Role::Slider);
    pad.set_label("X/Y パッド");
    pad.set_bounds(bounds(v.pad));
    pad.set_value(pad_text(t, now));
    children.push(mark("macros-pad".into(), pad));
    for (axis, r) in v.picks.iter().enumerate() {
        let k = t.controls.xy_knobs()[axis];
        let mut node = accesskit::Node::new(accesskit::Role::ComboBox);
        node.set_label(AXES[axis]);
        node.set_bounds(bounds(*r));
        node.set_value(t.control_label(k));
        children.push(mark(format!("macros-pick-{axis}"), node));
    }
    let mut node = accesskit::Node::new(accesskit::Role::Window);
    node.set_label(format!("{} › {NAME}", v.name));
    node.set_bounds(bounds(v.r));
    vec![Mark {
        key: "macros".into(),
        children,
        node,
        fold: None,
        pick: None,
        press: None,
        turn: None,
        edge: None,
    }]
}

/// The knob's value for the screen: the dragged one's while it is held.
pub fn knob_value(s: &Studio, t: &Track, k: u8) -> f64 {
    let id = ControlId::Knob(k);
    s.mixer_value(MixerHit::Control(t.id, id))
        .unwrap_or(t.controls.get(id).value)
}

/// Is `p` a song the window can show (its track is still there)?
pub fn shows(p: &Project, w: &MacroWin) -> bool {
    p.tracks.contains_key(&w.track)
}
