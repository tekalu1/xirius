//! The macro controls (MIX-041): the row of Macro 1–8 under the channel editor's toolbar, and
//! their own window with the eight in four columns of two at the left and one X/Y pad at the
//! right. The pad moves the two knobs its picks under it choose. Without its pin the window
//! shows the chosen channel's macros, as a device window does.

pub mod paint;
#[cfg(test)]
mod tests;

use serde_json::json;
use xirius_core::controls::{ControlId, KNOBS};
use xirius_core::id::TrackId;
use xirius_core::model::{Project, Track};

use crate::mixer::MixerHit;
use crate::studio::{Geometry, Rect, Studio};
use crate::title_bar::{self, BAR_H, PlainBar};
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
/// How far a window's top stands from the screen's top, unless moved.
const FROM_TOP: f32 = 96.0;
/// How much of the window stays on the screen.
const KEEP: f32 = 80.0;
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
    pub at: Option<(f32, f32)>,
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
            at: None,
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
    Move {
        from: (f32, f32),
        pointer: (f32, f32),
    },
    /// The pad's point: where it was and where it is now (0..1 across and up).
    Pad { from: (f64, f64), now: (f64, f64) },
}

/// A part of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Close,
    Pin,
    /// Opens the channel editor of the track.
    Channel,
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
    pub bar: PlainBar,
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

/// The window for the track `t` on a screen of `size`.
pub fn view(t: &Track, w: &MacroWin, size: (f32, f32)) -> View {
    let (x, y) = w.at.unwrap_or(((size.0 - W) / 2.0, FROM_TOP));
    let r = Rect::new(
        x.clamp(0.0, (size.0 - KEEP).max(0.0)),
        y.clamp(0.0, (size.1 - BAR_H).max(0.0)),
        W,
        H,
    );
    let name = t.shown_name().to_string();
    let bar = title_bar::plain_layout_of(Rect::new(r.x, r.y, r.w, BAR_H), &name, Some(NAME));
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
    } else if b.channel.is_some_and(|c| c.contains(x, y)) {
        Part::Channel
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
    pub fn macro_view(&self, g: &Geometry) -> Option<View> {
        let w = self.macro_pad.as_ref()?;
        let t = self.macro_track()?;
        let p = self.project();
        Some(view(p.tracks.get(&t)?, w, g.size))
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
    pub fn macro_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(v) = self.macro_view(g) else {
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
            Part::Channel => self.open_channel(t),
            Part::Head => {
                w.grab = Some(Grab::Move {
                    from: (v.r.x, v.r.y),
                    pointer: (x, y),
                })
            }
            Part::Knob(i) => {
                self.control_drag(t, ControlId::Knob(i), y);
            }
            Part::Pad => {
                let p = self.project();
                let Some(track) = p.tracks.get(&t) else {
                    return true;
                };
                let from = self.pad_now(track);
                let now = pad_values(v.pad, x, y);
                if let Some(w) = self.macro_pad.as_mut() {
                    w.grab = Some(Grab::Pad { from, now });
                }
                self.pad_heard(t, now);
            }
            Part::Pick(axis) => self.pick_menu(t, axis, v.picks[axis]),
            Part::Body => {}
        }
        true
    }

    /// The menu of an axis's pick: the eight knobs, the one it has checked.
    fn pick_menu(&mut self, t: TrackId, axis: usize, at: Rect) {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return;
        };
        let now = track.controls.xy[axis];
        let items = (0..KNOBS as u8)
            .map(|k| {
                let id = ControlId::Knob(k);
                self.ctx_as(
                    &track.control_label(id),
                    "control.xy.assign",
                    json!({
                        "track": t.to_string(),
                        "axis": AXES[axis].to_lowercase(),
                        "control": id.to_string(),
                    }),
                )
                .on(k == now)
            })
            .collect();
        self.open_context(items, at.x, at.bottom());
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
    pub fn macro_moved(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(grab) = self.macro_pad.as_ref().and_then(|w| w.grab) else {
            return false;
        };
        match grab {
            Grab::Move { from, pointer } => {
                if let Some(w) = self.macro_pad.as_mut() {
                    w.at = Some((from.0 + x - pointer.0, from.1 + y - pointer.1));
                }
            }
            Grab::Pad { from, now } => {
                let Some(v) = self.macro_view(g) else {
                    return false;
                };
                let next = pad_values(v.pad, x, y);
                if next == now {
                    return true;
                }
                if let Some(w) = self.macro_pad.as_mut() {
                    w.grab = Some(Grab::Pad { from, now: next });
                }
                self.pad_heard(v.track, next);
            }
        }
        true
    }

    /// The press ends: a moved pad is one step for both its knobs.
    pub fn macro_up(&mut self) -> bool {
        let Some(grab) = self.macro_pad.as_mut().and_then(|w| w.grab.take()) else {
            return false;
        };
        if let Grab::Pad { from, now } = grab
            && let Some(t) = self.macro_track()
        {
            if now == from {
                let p = self.project();
                self.transport.sync(&p, self.song().dir());
            } else {
                self.pad_set(t, now);
            }
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
    pub fn macro_hover(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let hot = self.macro_view(g).and_then(|v| part_at(&v, x, y));
        let Some(w) = self.macro_pad.as_mut() else {
            return false;
        };
        if w.hot == hot {
            return false;
        }
        w.hot = hot;
        true
    }

    /// Is the window under the point?
    pub(crate) fn over_macros(&self, g: &Geometry, x: f32, y: f32) -> bool {
        self.macro_view(g).is_some_and(|v| v.r.contains(x, y))
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
        let (x, y) = self.pad_now(track);
        let to = |v: f64, d: f64| ((v + d).clamp(0.0, 1.0) * 100.0).round() / 100.0;
        let next = (to(x, dx), to(y, dy));
        if next != (x, y) {
            self.pad_set(t, next);
        }
        true
    }
}

/// The reader's marks of the macro window.
pub fn marks(s: &Studio, g: &Geometry) -> Vec<crate::a11y::Mark> {
    use crate::a11y::{Mark, bounds};
    use gpui::accesskit;
    let Some(w) = &s.macro_pad else {
        return Vec::new();
    };
    let Some(v) = s.macro_view(g) else {
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
    if let Some(c) = v.bar.channel {
        children.push(mark(
            "macros-channel".into(),
            button(crate::channel::NAME, c),
        ));
    }
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
