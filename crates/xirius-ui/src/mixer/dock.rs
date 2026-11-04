//! The mixer opened out (MIX-046): the strips at the left of the drawer, the face of the device
//! picked on a strip at the right, and the grip between them. The face is the device's window
//! sitting in the drawer: its bar has 「窓で開く」 where the pin is, and a double click on the bar
//! takes it out as a window. The grip sets the face's width, ←→ move it by [`STEP`]. When the
//! strips have less than [`FOLD_W`], the other channels fold to bands of [`BAND_W`] with their
//! name and meter, and only the strip in front stays, [`super::WIDE_W`] wide; the strips then
//! take the room every band needs, as far as the face keeps [`FOLD_W`].

use gpui::{Hsla, accesskit};
use xirius_core::id::TrackId;
use xirius_design::{FontSize, Radius, Weight};

use xirius_session::host::Strip;

use super::flow::Pick;
use super::{MASTER_GAP, MixerHit, STRIP_GAP, WIDE_W};
use crate::a11y::{Mark, bounds};
use crate::draw::{Align, Pen};
use crate::instrument::window::{Win, is_built_in, live};
use crate::paint::Colors;
use crate::studio::{Geometry, Hit, Rect, Studio};

/// The gap the grip sits in, between the strips and the face.
pub const GRIP_W: f32 = 10.0;
/// The mark of the grip.
/// The mark on the grip: the mock's `.handle i`, 4×28 (review 3 低15).
const GRIP_MARK: (f32, f32) = (4.0, 28.0);
/// Below this the strips fold (two strips' width).
pub const FOLD_W: f32 = 240.0;
/// A folded channel: its name and its meter.
pub const BAND_W: f32 = 40.0;
/// One press of ← or → on the grip.
pub const STEP: f32 = 16.0;
/// The least the folded strips keep: the strip in front and a band each side.
pub const FOLDED_MIN: f32 = WIDE_W + 2.0 * BAND_W + 4.0 * STRIP_GAP;
/// The grip's name for a screen reader.
pub const GRIP_LABEL: &str = "ミキサーと機器の面の境目";
/// The face's 「窓で開く」.
pub const POPOUT_LABEL: &str = "窓で開く";

/// Where the strips, the grip and the face sit in the drawer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Split {
    pub mixer: Rect,
    pub grip: Rect,
    pub panel: Rect,
    /// The strips fold to bands.
    pub folded: bool,
}

/// How wide `strips` strips (the master's with them) are folded: the one in front, a band
/// for each of the others and their gaps, so that none scrolls away. At least
/// [`FOLDED_MIN`].
pub fn folded_w(strips: usize) -> f32 {
    let others = strips.saturating_sub(1) as f32;
    (WIDE_W + others * (BAND_W + STRIP_GAP) + 2.0 * STRIP_GAP + MASTER_GAP).max(FOLDED_MIN)
}

/// The drawer's body `body` split for a face that wants to be `want` wide; folded, the strips
/// take `fold` ([`folded_w`]) as far as the face keeps [`FOLD_W`]. The face is always at the
/// strips' right, also in the narrowest drawer (2026-11-04).
pub fn split(body: Rect, want: f32, fold: f32) -> Split {
    let room = (body.w - GRIP_W).max(0.0);
    let want = want.min(room);
    let open = room - want;
    let folded = open < FOLD_W;
    let mixer_w = if folded {
        let most = (room - FOLD_W).max(FOLDED_MIN).min(room);
        open.max(fold.max(FOLDED_MIN).min(most))
    } else {
        open
    };
    let mixer = Rect::new(body.x, body.y, mixer_w, body.h);
    let grip = Rect::new(mixer.right(), body.y, GRIP_W, body.h);
    let panel = Rect::new(
        grip.right(),
        body.y,
        (body.right() - grip.right()).max(0.0),
        body.h,
    );
    Split {
        mixer,
        grip,
        panel,
        folded,
    }
}

/// The widest a face can be asked to be in `body`: a band's room left for the strips, which
/// fold before that and keep [`FOLDED_MIN`].
pub fn widest(body: Rect) -> f32 {
    (body.w - GRIP_W - BAND_W).max(0.0)
}

impl Studio {
    /// The face in the drawer: its window's index, while the mixer shows.
    pub(crate) fn docked(&self) -> Option<usize> {
        if !self.mixer_shown() {
            return None;
        }
        let i = self.instruments.dock_slot()?;
        live(&self.project(), &self.instruments.open[i])?;
        Some(i)
    }

    /// The drawer split, while a face sits in it.
    pub fn mixer_split(&self, g: &Geometry) -> Option<Split> {
        if !g.panels.mixer || g.drawer_body.h <= 0.0 {
            return None;
        }
        let i = self.docked()?;
        Some(self.instruments.dock_split(i, g))
    }

    /// The part of the drawer the strips have.
    pub fn mixer_body(&self, g: &Geometry) -> Rect {
        self.mixer_split(g).map_or(g.drawer_body, |s| s.mixer)
    }

    /// The strips are folded to bands.
    pub fn mixer_folded(&self, g: &Geometry) -> bool {
        self.mixer_split(g).is_some_and(|s| s.folded)
    }

    /// The channel whose strip stays open when they fold: the band pressed last, else the
    /// one with the picked row, else the face's.
    pub fn mixer_lead(&self) -> Option<TrackId> {
        let p = self.project();
        self.mixer
            .lead
            .filter(|t| p.tracks.contains_key(t))
            .or_else(|| self.mixer.chosen.map(|(t, _)| t))
            .or_else(|| {
                self.instruments
                    .dock_slot()
                    .map(|i| self.instruments.open[i].track)
            })
    }

    /// The row picked on a strip comes into the face, when it is a built-in device: a window
    /// of it comes into the drawer too.
    pub(crate) fn dock_follow(&mut self, g: &Geometry) {
        if !self.mixer_shown() {
            return;
        }
        let Some((t, Pick::Plugin(id))) = self.mixer.chosen else {
            return;
        };
        let p = self.project();
        let Some(plugin) = p
            .tracks
            .get(&t)
            .and_then(|x| x.inserts.iter().find(|x| x.id == id))
        else {
            return;
        };
        if !is_built_in(plugin) || self.health.crashed(id) {
            return;
        }
        let Some(def) = plugin.builtin() else {
            return;
        };
        // Shown already: in the face, or in a window of its own.
        if self.instruments.is_open(id) {
            return;
        }
        if let Some(i) = self.instruments.dock_slot()
            && self
                .instruments
                .note
                .is_some_and(|n| n.plugin == self.instruments.open[i].plugin)
        {
            self.window_note_off();
        }
        let ins = &mut self.instruments;
        let size = |w: f32| crate::device::layout::size_of(def, w, Some(plugin));
        let natural = size(crate::device::layout::natural_w(def));
        let w = ins.dock_w.unwrap_or(natural.0);
        // Under the strips it takes the height the face has at its smallest width.
        let tall = size(crate::device::layout::min_w(def)).1;
        let win = Win {
            track: t,
            plugin: id,
            knobs: def.params.len(),
            body: (w, tall),
            at: None,
            pinned: false,
            seat: 0,
            placed_w: natural.0,
            pick: 0,
            layer: 0,
            filled: !plugin.samples.is_empty(),
            docked: true,
        };
        match ins.dock_slot() {
            Some(i) => ins.open[i] = win,
            None => ins.open.insert(0, win),
        }
        // The channel the device is on stays in sight beside the face.
        if let Some(off) = self.strip_offset(g, t) {
            let right = off + self.width_of(g, t) + 2.0 * STRIP_GAP - self.mixer_view(g).w;
            self.mixer.scroll = self.mixer_scroll(g).max(right).min(off).max(0.0);
        }
    }

    /// Sets the face's width, as far as the drawer lets it.
    fn dock_width(&mut self, g: &Geometry, w: f32) -> bool {
        let Some(i) = self.docked() else {
            return false;
        };
        let p = self.project();
        let min = live(&p, &self.instruments.open[i])
            .map_or(0.0, |(_, _, def)| crate::device::layout::min_w(def));
        let w = w.clamp(min.min(widest(g.drawer_body)), widest(g.drawer_body));
        let win = &mut self.instruments.open[i];
        if win.body.0 == w {
            return false;
        }
        win.body.0 = w;
        self.instruments.dock_w = Some(w);
        true
    }

    /// A press on the grip takes hold of it, and its keys move it. A press elsewhere lets the
    /// keys go.
    pub fn grip_down(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let on = self.popup.is_none()
            && !self.over_float(g, x, y)
            && self.mixer_split(g).is_some_and(|s| s.grip.contains(x, y));
        self.mixer.grip_focus = on;
        if !on {
            return false;
        }
        let w = self.instruments.open[self.docked().expect("split")].body.0;
        self.mixer.grip_drag = Some((w, x));
        true
    }

    /// The pointer moved with the grip held.
    pub fn grip_move(&mut self, g: &Geometry, x: f32) -> bool {
        let Some((w, x0)) = self.mixer.grip_drag else {
            return false;
        };
        self.dock_width(g, w - (x - x0));
        true
    }

    /// The grip is let go.
    pub fn grip_up(&mut self) -> bool {
        self.mixer.grip_drag.take().is_some()
    }

    /// The pointer moved: whether it is over the grip changed.
    pub fn grip_hover(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let on = self.popup.is_none()
            && !self.over_float(g, x, y)
            && self.mixer_split(g).is_some_and(|s| s.grip.contains(x, y));
        let changed = on != self.mixer.grip_hover;
        self.mixer.grip_hover = on;
        changed
    }

    /// ← and → on the grip pressed last move it by [`STEP`].
    pub(crate) fn grip_key(&mut self, g: &Geometry, key: &str) -> bool {
        if !self.mixer.grip_focus || self.docked().is_none() || self.popup.is_some() {
            return false;
        }
        let w = self.instruments.open[self.docked().expect("docked")].body.0;
        match key {
            "left" => self.dock_width(g, w + STEP),
            "right" => self.dock_width(g, w - STEP),
            "escape" => {
                self.mixer.grip_focus = false;
                true
            }
            _ => return false,
        };
        true
    }

    /// The folded channels on screen: each band's track and rect.
    pub fn mixer_bands(&self, g: &Geometry) -> Vec<(TrackId, Rect)> {
        self.mixer_row(g).1
    }
}

/// The mark in the middle of the grip.
pub(crate) fn grip_mark(grip: Rect) -> Rect {
    Rect::new(
        grip.x + (grip.w - GRIP_MARK.0) / 2.0,
        grip.y + (grip.h - GRIP_MARK.1) / 2.0,
        GRIP_MARK.0,
        GRIP_MARK.1,
    )
}

/// The mark's colour: `ring` while held (approved 2026-11-04), the mock's `--thumbh` under the
/// pointer or holding the keys (`:hover`, `:focus-visible`), `--thumb` at rest.
pub(crate) fn grip_ink(studio: &Studio, c: &Colors) -> Hsla {
    let m = &studio.mixer;
    if m.grip_drag.is_some() {
        c.ring
    } else if m.grip_hover || m.grip_focus {
        c.scroll_thumb[0]
    } else {
        c.scroll_thumb[1]
    }
}

/// The grip between the strips and the face: its mark, and `ring`'s frame round it while it
/// holds the keys.
pub fn paint_grip(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let Some(s) = studio.mixer_split(g) else {
        return;
    };
    let c = pen.c;
    let mark = grip_mark(s.grip);
    pen.round(mark, grip_ink(studio, c), Radius::Pill);
    if studio.mixer.grip_focus {
        pen.outline(
            Rect::new(mark.x - 2.0, mark.y - 2.0, mark.w + 4.0, mark.h + 4.0),
            c.ring,
            Radius::Pill,
            1.5,
        );
    }
}

/// A folded channel: its colour, its name down the band and its meter.
pub fn paint(pen: &mut Pen, studio: &Studio, t: TrackId, r: Rect) {
    let c = pen.c;
    let p = studio.project();
    let Some(track) = p.tracks.get(&t) else {
        return;
    };
    let selected = !t.is_master() && studio.is_picked(crate::tree::RowKind::Track(t));
    pen.fill(r, if selected { c.track_sel } else { c.surface_1 });
    let dot = 8.0;
    pen.round(
        Rect::new(r.x + (r.w - dot) / 2.0, r.y + 10.0, dot, dot),
        crate::draw::color_of(c, track),
        Radius::S,
    );
    // The name, a letter a line, in the middle of the band.
    let name: Vec<char> = track.shown_name().chars().collect();
    let line = 13.0;
    let room = ((r.h - 2.0 * METER_H - 40.0) / line).max(1.0) as usize;
    let shown: Vec<String> = if name.len() > room {
        name[..room.saturating_sub(1)]
            .iter()
            .map(char::to_string)
            .chain(["…".to_string()])
            .collect()
    } else {
        name.iter().map(char::to_string).collect()
    };
    let top = r.y + (r.h - shown.len() as f32 * line) / 2.0;
    let ink = if selected { c.ink_strong } else { c.ink };
    for (i, ch) in shown.iter().enumerate() {
        pen.text(
            ch,
            FontSize::Small,
            Weight::Regular,
            ink,
            Rect::new(r.x, top + i as f32 * line, r.w, line),
            Align::Center,
        );
    }
    let m = studio.mixer.meters.get(if t.is_master() {
        Strip::Master
    } else {
        Strip::Track(t)
    });
    let pair = 2.0 * super::METER_W + super::METER_GAP;
    for ch in 0..2 {
        let x = r.x + (r.w - pair) / 2.0 + ch as f32 * (super::METER_W + super::METER_GAP);
        super::paint::meter(
            pen,
            Rect::new(x, r.bottom() - 8.0 - METER_H, super::METER_W, METER_H),
            &m,
            ch,
        );
    }
}

/// How tall a band's meter is.
const METER_H: f32 = 100.0;

/// The face's 「窓で開く」 and its close, the grip and the bands, for a screen reader.
pub fn marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    let mut out = Vec::new();
    let Some(split) = s.mixer_split(g) else {
        return out;
    };
    let mark = |key: String, node: accesskit::Node, press: Option<Hit>| Mark {
        key,
        children: Vec::new(),
        node,
        fold: None,
        pick: None,
        press,
        turn: None,
    };
    let mut grip = accesskit::Node::new(accesskit::Role::Splitter);
    grip.set_label(GRIP_LABEL);
    grip.set_numeric_value(f64::from(split.panel.w.round()));
    grip.set_bounds(bounds(split.grip));
    out.push(mark("dock-grip".into(), grip, None));
    if let Some(i) = s.docked() {
        use crate::instrument::window::Part;
        let w = s.instruments.open[i];
        let p = s.project();
        if let Some((track, plugin, def)) = live(&p, &w) {
            let r = s.instruments.frame(i, g);
            for (part, name) in [(Part::Pin, POPOUT_LABEL), (Part::Close, "閉じる")] {
                let mut node = accesskit::Node::new(accesskit::Role::Button);
                node.set_label(format!("{name}: {} › {}", track.shown_name(), plugin.name));
                node.set_bounds(bounds(
                    s.instruments.part_rect(r, &p, track, plugin, def, part),
                ));
                out.push(mark(format!("dock-{part:?}"), node, None));
            }
        }
    }
    let p = s.project();
    for (t, r) in s.mixer_bands(g) {
        let Some(track) = p.tracks.get(&t) else {
            continue;
        };
        let mut node = accesskit::Node::new(accesskit::Role::Button);
        node.set_label(track.shown_name());
        node.set_bounds(bounds(r));
        out.push(mark(
            format!("band-{t}"),
            node,
            Some(Hit::Mixer(MixerHit::Strip(t))),
        ));
    }
    out
}

#[cfg(test)]
#[path = "dock_tests.rs"]
mod tests;
