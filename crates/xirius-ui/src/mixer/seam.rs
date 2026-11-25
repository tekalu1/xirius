//! The insert point between two rows of a strip's flow (UI-362, approved 2026-11-17): a line
//! the width of the insert area with a ＋ at its middle, on the boundary between two rows. It
//! shows at the top boundary, under each row and inside a splitter's branches; a press anywhere
//! on it opens the list of plug-ins there. Rows never move for it.
//!
//! The pointer shows it by how it comes: slowly, at once; fast and then stopping, after
//! [`SETTLE_MS`]; fast and through, not at all. It stays [`GRACE_MS`] after the pointer leaves
//! (longer, while the pointer goes slowly up or down the rows to the next boundary). A touch
//! has no pointer to come: the row picked shows its seam below it. A row held and moved, and a
//! plug-in dragged from the browser, show the same line (no ＋, no wait) where they would land.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use xirius_core::id::TrackId;
use xirius_core::model::Track;
use xirius_design::{Duration as Tok, Radius};

use super::MixerHit;
use super::flow::{AREA_SIDE, Flow, INDENT, Seam, Spot};
use crate::draw::Pen;
use crate::studio::{Geometry, Hit, Rect, Studio};

/// How far above and below a seam the pointer finds it before its line shows, and once it
/// shows (the row's name keeps the 12 px in the middle of the row either way).
pub const FIND: f32 = 5.0;
pub const KEEP: f32 = 8.0;
/// The columns of a row where its power mark and its close are: the line does not start
/// there, and shown it is only this far above and below the boundary.
pub const EDGE: f32 = 24.0;
pub const EDGE_KEEP: f32 = 3.0;
/// A pointer goes slowly when it goes no faster than this (px/s) over the last [`WINDOW_MS`].
pub const SLOW: f32 = 300.0;
pub const WINDOW_MS: u64 = 60;
/// A pointer that came fast and stopped shows the line after this (Fast, 120 ms).
pub const SETTLE_MS: u64 = 120;
/// How long the line stays after the pointer leaves it: enough for a hand to overshoot a
/// little and come back. It is for aiming, not for show, so a reduced-motion setting keeps it.
pub const GRACE_MS: u64 = 60;
/// The line's room under the last seam, for the ＋ to stay inside the panel.
pub const TAIL: f32 = 9.0;
/// The line, and the ＋ on it (the touch ＋ is larger).
pub const LINE_W: f32 = 2.0;
pub const LINE_INSET: f32 = 2.0;
pub const PLUS: f32 = 18.0;
pub const PLUS_ICON: f32 = 12.0;
pub const PLUS_COARSE: f32 = 24.0;
pub const PLUS_ICON_COARSE: f32 = 16.0;
/// How small the ＋ starts to grow from, and how far it sinks while it is pressed.
const PLUS_FROM: f32 = 0.4;
const PRESSED: f32 = 0.86;

/// How long the line grows and slides: Fast, the app's time for a pointer's light.
fn enter_ms() -> u64 {
    u64::from(Tok::Fast.ms())
}

/// How long it takes to go: two thirds of that, started fast. The set of times stays as it
/// is (120, 240, 560); this is derived from Fast.
fn leave_ms() -> u64 {
    enter_ms() * 2 / 3
}

fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn since(a: Instant, b: Instant) -> Duration {
    b.saturating_duration_since(a)
}

/// A seam under the pointer, on a strip.
pub type Cand = CandOf<TrackId, Spot>;
/// What a frame draws of a strip's seam.
pub type Look = LookOf<Spot>;
/// The seam of the mixer's strips.
pub type Live = LiveOf<TrackId, Spot>;

/// The seam on show: `T` holds the seams (a strip; nothing, for the arrangement's rows) and
/// `S` is the seam in it. The arrangement's drop of a file times its seams the same way
/// (UI-381).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShownOf<T, S> {
    pub track: T,
    pub spot: S,
    /// With the ＋ (the pointer's), or the line alone (a row on its way).
    pub plus: bool,
    pub drag: bool,
    began: Instant,
    /// Where it came from, relative to where it is: (left, y) and when it began to slide.
    slide: Option<(f32, f32, Instant)>,
    /// When it began to go, and how far in it was.
    going: Option<(Instant, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Wait<T, S> {
    since: Instant,
    track: T,
    spot: S,
    at: (f32, f32),
}

/// A seam under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CandOf<T, S> {
    pub track: T,
    pub spot: S,
    /// Where its line starts and its height.
    pub at: (f32, f32),
    /// In the middle of the row, not in the columns of its power mark and close.
    pub mid: bool,
}

/// What a frame draws of the seam.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookOf<S> {
    pub spot: S,
    pub plus: bool,
    /// 0 to 1: the line's length from the middle, and the ＋'s size and weight.
    pub grow: f32,
    /// Where it is, relative to where it settles.
    pub off: (f32, f32),
    pub going: bool,
}

#[derive(Clone, Debug)]
pub struct LiveOf<T, S> {
    samples: VecDeque<(Instant, f32, f32)>,
    out: Option<(Instant, f32, f32)>,
    pub shown: Option<ShownOf<T, S>>,
    wait: Option<Wait<T, S>>,
    leave: Option<Instant>,
    /// The line is pressed: the ＋ sinks.
    pub pressed: bool,
}

impl<T, S> Default for LiveOf<T, S> {
    fn default() -> Self {
        Self {
            samples: VecDeque::new(),
            out: None,
            shown: None,
            wait: None,
            leave: None,
            pressed: false,
        }
    }
}

impl<T: Copy + PartialEq, S: Copy + PartialEq> LiveOf<T, S> {
    /// The pointer at `(x, y)` at `now`.
    pub fn feed(&mut self, now: Instant, x: f32, y: f32) {
        self.samples.push_back((now, x, y));
        while self.samples.len() > 1
            && since(self.samples[0].0, now) > Duration::from_millis(WINDOW_MS)
        {
            self.out = self.samples.pop_front();
        }
    }

    /// The pointer's speed in px/s over the last [`WINDOW_MS`], and how far it went across and
    /// up or down in that time. A pointer just come from rest is slow.
    pub fn motion(&self) -> (f32, f32, f32) {
        let Some(&(tn, xn, yn)) = self.samples.back() else {
            return (0.0, 0.0, 0.0);
        };
        let from = if self.samples.len() > 1 {
            self.samples.front().copied()
        } else {
            self.out
        };
        let Some((t0, x0, y0)) = from else {
            return (0.0, 0.0, 0.0);
        };
        let dt = since(t0, tn).as_secs_f32();
        let (dx, dy) = (xn - x0, yn - y0);
        if dt <= 0.0 {
            return (0.0, dx, dy);
        }
        (dx.hypot(dy) / dt, dx, dy)
    }

    /// Where the pointer was last seen over the mixer.
    pub fn last(&self) -> Option<(f32, f32)> {
        self.samples.back().map(|&(_, x, y)| (x, y))
    }

    pub fn slow(&self) -> bool {
        self.motion().0 <= SLOW
    }

    /// Forgets how the pointer went: it left the window, or the rows changed.
    pub fn forget(&mut self) {
        self.samples.clear();
        self.out = None;
    }

    /// The seam that shows and can be pressed.
    pub fn live(&self) -> Option<&ShownOf<T, S>> {
        self.shown.as_ref().filter(|s| s.going.is_none())
    }

    pub fn spot_on(&self, track: T) -> Option<S> {
        self.live().filter(|s| s.track == track).map(|s| s.spot)
    }

    /// Drops what shows, without the going animation (the rows changed under it).
    pub fn clear(&mut self) {
        self.shown = None;
        self.wait = None;
        self.leave = None;
        self.pressed = false;
    }

    /// Starts the seam `c` at once; from another seam, it slides.
    fn start(
        &mut self,
        now: Instant,
        c: &CandOf<T, S>,
        plus: bool,
        drag: bool,
        from: Option<(f32, f32)>,
    ) {
        self.wait = None;
        self.leave = None;
        match (&mut self.shown, from) {
            (Some(s), Some(f)) if s.going.is_none() && s.track == c.track => {
                s.slide = Some((f.0 - c.at.0, f.1 - c.at.1, now));
                s.spot = c.spot;
                s.plus = plus;
                s.drag = drag;
            }
            _ => {
                self.shown = Some(ShownOf {
                    track: c.track,
                    spot: c.spot,
                    plus,
                    drag,
                    began: now,
                    slide: None,
                    going: None,
                });
            }
        }
    }

    /// The pointer is on `cand` (or on no seam). `from` is where the seam on show is now;
    /// `hold`: the pointer goes slowly up or down the rows, toward the next seam.
    pub fn pointer(
        &mut self,
        now: Instant,
        cand: Option<CandOf<T, S>>,
        from: Option<(f32, f32)>,
        hold: bool,
    ) {
        let slow = self.slow();
        match cand {
            Some(c)
                if self
                    .live()
                    .is_some_and(|s| s.track == c.track && s.spot == c.spot) =>
            {
                self.wait = None;
                self.leave = None;
            }
            // not a seam to start: the power mark's column
            Some(c) if !c.mid => {
                self.wait = None;
                self.begin_leave(now);
            }
            Some(c) if slow => self.start(now, &c, true, false, from),
            Some(c) => {
                if self.wait.map(|w| (w.track, w.spot)) != Some((c.track, c.spot)) {
                    self.wait = Some(Wait {
                        since: now,
                        track: c.track,
                        spot: c.spot,
                        at: c.at,
                    });
                }
                self.begin_leave(now);
            }
            None => {
                self.wait = None;
                if hold && self.live().is_some() {
                    self.leave = None;
                } else {
                    self.begin_leave(now);
                }
            }
        }
    }

    /// A row on its way: its seam shows at once, without the ＋.
    pub fn dragging(&mut self, now: Instant, cand: Option<CandOf<T, S>>, from: Option<(f32, f32)>) {
        match cand {
            Some(c)
                if self
                    .live()
                    .is_some_and(|s| s.track == c.track && s.spot == c.spot) =>
            {
                self.leave = None;
            }
            Some(c) => self.start(now, &c, false, true, from),
            None => self.begin_leave(now),
        }
    }

    fn begin_leave(&mut self, now: Instant) {
        if self.live().is_some() && self.leave.is_none() {
            self.leave = Some(now);
        }
    }

    /// Ends the grace and the going that `now` is past; shows what waited long enough.
    /// `again` finds the wait's seam under the pointer again (None: it is gone).
    pub fn advance(
        &mut self,
        now: Instant,
        reduced: bool,
        again: impl Fn(T, S) -> Option<CandOf<T, S>>,
        from: Option<(f32, f32)>,
    ) {
        if let Some(w) = self.wait
            && since(w.since, now) >= Duration::from_millis(SETTLE_MS)
        {
            match again(w.track, w.spot) {
                Some(c) => self.start(now, &c, true, false, from),
                None => self.wait = None,
            }
        }
        if let Some(l) = self.leave
            && since(l, now) >= Duration::from_millis(GRACE_MS)
        {
            self.leave = None;
            let at = l + Duration::from_millis(GRACE_MS);
            let grown = self.grown(at, reduced);
            if let Some(s) = &mut self.shown {
                s.going = Some((at, grown));
            }
        }
        if let Some(s) = self.shown
            && let Some((g, _)) = s.going
            && since(g, now) >= Duration::from_millis(if reduced { 0 } else { leave_ms() })
        {
            self.shown = None;
        }
    }

    /// The seam goes now (a row let go): it begins to shrink away.
    pub fn end_drag(&mut self, now: Instant, reduced: bool) {
        if self.shown.is_some_and(|s| s.drag && s.going.is_none()) {
            let grown = self.grown(now, reduced);
            if let Some(s) = &mut self.shown {
                s.going = Some((now, grown));
            }
            self.leave = None;
        }
    }

    fn grown(&self, now: Instant, reduced: bool) -> f32 {
        let Some(s) = &self.shown else { return 0.0 };
        if reduced {
            return 1.0;
        }
        ease(since(s.began, now).as_secs_f32() * 1000.0 / enter_ms() as f32)
    }

    /// What to draw at `now`.
    pub fn look(&self, now: Instant, reduced: bool) -> Option<LookOf<S>> {
        let s = self.shown.as_ref()?;
        let grow = match s.going {
            Some((at, from)) if reduced => {
                let _ = (at, from);
                0.0
            }
            Some((at, from)) => {
                from * (1.0 - ease(since(at, now).as_secs_f32() * 1000.0 / leave_ms() as f32))
            }
            None => self.grown(now, reduced),
        };
        let off = match s.slide {
            Some((dx, dy, at)) if !reduced => {
                let k = 1.0 - ease(since(at, now).as_secs_f32() * 1000.0 / enter_ms() as f32);
                (dx * k, dy * k)
            }
            _ => (0.0, 0.0),
        };
        Some(LookOf {
            spot: s.spot,
            plus: s.plus,
            grow,
            off,
            going: s.going.is_some(),
        })
    }

    /// Whether a frame is wanted for it: it is growing, sliding or going, or waits for its time.
    pub fn moving(&self, now: Instant, reduced: bool) -> bool {
        if self.wait.is_some() || self.leave.is_some() {
            return true;
        }
        let Some(s) = &self.shown else { return false };
        if reduced {
            return s.going.is_some();
        }
        s.going.is_some()
            || since(s.began, now) < Duration::from_millis(enter_ms())
            || s.slide
                .is_some_and(|(_, _, at)| since(at, now) < Duration::from_millis(enter_ms()))
    }

    #[cfg(test)]
    pub fn wait_pending(&self) -> bool {
        self.wait.is_some()
    }

    #[cfg(test)]
    pub fn leave_pending(&self) -> bool {
        self.leave.is_some()
    }

    #[cfg(test)]
    pub fn show_for_test(&mut self, now: Instant, track: T, spot: S, plus: bool) {
        let c = CandOf {
            track,
            spot,
            at: (0.0, 0.0),
            mid: true,
        };
        self.start(now, &c, plus, false, None);
    }
}

/// Where a seam's line runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub left: f32,
    pub right: f32,
    pub y: f32,
    /// The room the pointer finds it in, from the indent to the panel's side.
    pub from: f32,
    pub to: f32,
}

pub fn place(flow: &Flow, seam: &Seam) -> Option<Place> {
    let panel = flow.insert_panel()?;
    let indent = f32::from(seam.depth) * INDENT;
    Some(Place {
        left: panel.x + LINE_INSET + indent,
        right: panel.right() - LINE_INSET,
        y: seam.y,
        from: panel.x + AREA_SIDE + indent,
        to: panel.right() - AREA_SIDE,
    })
}

/// The ＋'s size, for a mouse and for a touch.
pub fn plus_size(coarse: bool) -> (f32, f32) {
    if coarse {
        (PLUS_COARSE, PLUS_ICON_COARSE)
    } else {
        (PLUS, PLUS_ICON)
    }
}

/// How far above and below a seam a press finds it, as it is on show.
pub fn keep(coarse: bool) -> f32 {
    if coarse { PLUS_COARSE / 2.0 } else { KEEP }
}

/// Whether the pointer at `(x, y)` is on the seam `place` (`shown`: its line is on show).
/// The columns of the power mark and the close take the pointer except right on the line.
pub fn zone(p: &Place, coarse: bool, shown: bool, x: f32, y: f32) -> Option<bool> {
    let half = if shown { keep(coarse) } else { FIND };
    let d = (y - p.y).abs();
    if d > half || x < p.from || x >= p.to {
        return None;
    }
    let edge = x < p.from + EDGE || x >= p.to - EDGE;
    if edge {
        return (shown && d <= EDGE_KEEP).then_some(false);
    }
    Some(true)
}

/// The seam that shows on `t`'s strip, for a frame: the pointer's, or on a touch screen the
/// one below the picked row.
pub fn visible(s: &Studio, t: &Track, now: Instant) -> Option<Look> {
    if s.mixer.coarse {
        let (id, pick) = s.mixer.chosen?;
        return (id == t.id).then_some(Look {
            spot: Spot::After(pick),
            plus: true,
            grow: 1.0,
            off: (0.0, 0.0),
            going: false,
        });
    }
    let live = &s.mixer.seam;
    live.shown
        .as_ref()
        .filter(|x| x.track == t.id)
        .and_then(|_| live.look(now, s.ai.motion.reduced))
}

/// Whether a seam is on show and can be pressed on `t`'s strip: the rows above and below it
/// light nothing under the pointer.
pub fn on_show(s: &Studio, t: TrackId) -> bool {
    s.mixer.seam.spot_on(t).is_some()
        || (s.mixer.coarse && s.mixer.chosen.is_some_and(|(x, _)| x == t))
}

/// Draws the seam on `t`'s strip over its rows.
pub fn paint(pen: &mut Pen, s: &Studio, t: &Track, flow: &Flow) {
    let Some(look) = visible(s, t, Instant::now()) else {
        return;
    };
    let Some(seam) = flow.seam(t, look.spot) else {
        return;
    };
    let Some(p) = place(flow, &seam) else { return };
    let left = p.left + look.off.0;
    let y = p.y + look.off.1;
    let len = (p.right - left) * look.grow;
    let mid = (left + p.right) / 2.0;
    let plus = if look.plus && look.grow > 0.0 {
        let (d, icon) = plus_size(flow.coarse());
        let sink = if s.mixer.seam.pressed { PRESSED } else { 1.0 };
        let k = (PLUS_FROM + (1.0 - PLUS_FROM) * look.grow) * sink;
        Some(((d * k).round(), (icon * k).round(), look.grow))
    } else {
        None
    };
    paint_line_plus(pen, mid, y, len, plus);
}

pub(crate) fn paint_line_plus(
    pen: &mut Pen,
    mid: f32,
    y: f32,
    len: f32,
    plus: Option<(f32, f32, f32)>,
) {
    let c = pen.c;
    if len > 0.5 {
        pen.round(
            Rect::new(mid - len / 2.0, y - LINE_W / 2.0, len, LINE_W),
            c.ring,
            Radius::Pill,
        );
    }
    if let Some((d, icon, opacity)) = plus {
        let r = Rect::new(mid - d / 2.0, y - d / 2.0, d, d);
        pen.round(r, c.primary.0.opacity(opacity), Radius::Pill);
        pen.icon(
            "plus",
            Rect::new(r.x + (d - icon) / 2.0, r.y + (d - icon) / 2.0, icon, icon),
            c.primary.1.opacity(opacity),
        );
    }
}

impl Studio {
    fn seam_reduced(&self) -> bool {
        self.ai.motion.reduced
    }

    /// Whether the open list was opened from a seam: its seam stays on show.
    fn seam_listed(&self) -> bool {
        matches!(
            self.popup.as_ref().map(|p| p.kind),
            Some(crate::popup::PopupKind::Insert(_))
        ) && (self.mixer.insert_after.is_some() || self.mixer.insert_into.is_some())
    }

    /// Whether something else holds the pointer: a held fader, a held row, a menu.
    fn seam_busy(&self) -> bool {
        self.context_open()
            || self.mixer.drag.is_some()
            || self.mixer.level_drag.is_some()
            || self.mixer.key_drag.is_some()
            || self.mixer.name_drag.is_some()
            || self.mixer.insert_drag.is_some()
            || self.popup.is_some()
    }

    /// The seam under `(x, y)`, on the strip there.
    pub(crate) fn seam_cand(&self, g: &Geometry, x: f32, y: f32) -> Option<Cand> {
        if !self.mixer_shown() || self.mixer.coarse {
            return None;
        }
        let p = self.project();
        let s = self
            .mixer_strips(g)
            .into_iter()
            .find(|s| self.strip_holds(g, s, x, y))?;
        let track = p.tracks.get(&s.track)?;
        if track.frozen.is_some() {
            return None;
        }
        let flow = self.strip_flow(&s, track);
        let on = self.mixer.seam.spot_on(s.track);
        let mut best: Option<(f32, Cand)> = None;
        for seam in flow.seams() {
            if seam.y < s.inserts.y || seam.y > s.inserts.bottom() {
                continue;
            }
            let Some(pl) = place(&flow, &seam) else {
                continue;
            };
            let Some(mid) = zone(&pl, false, on == Some(seam.spot), x, y) else {
                continue;
            };
            let d = (y - seam.y).abs();
            if best.as_ref().is_none_or(|(b, _)| d < *b) {
                best = Some((
                    d,
                    Cand {
                        track: s.track,
                        spot: seam.spot,
                        at: (pl.left, pl.y),
                        mid,
                    },
                ));
            }
        }
        best.map(|(_, c)| c)
    }

    /// Where the line of `spot` on `t` starts and its height, now.
    fn seam_at(&self, g: &Geometry, t: TrackId, spot: Spot) -> Option<Cand> {
        let p = self.project();
        let s = self.mixer_strips(g).into_iter().find(|s| s.track == t)?;
        let track = p.tracks.get(&t)?;
        let flow = self.strip_flow(&s, track);
        let seam = flow.seam(track, spot)?;
        let pl = place(&flow, &seam)?;
        Some(Cand {
            track: t,
            spot,
            at: (pl.left, pl.y),
            mid: true,
        })
    }

    /// The seam a row let go at `(x, y)` lands on, while one is held and moved.
    fn seam_drop(&self, g: &Geometry, x: f32, y: f32) -> Option<Cand> {
        if !self.mixer.fx_drag.as_ref().is_some_and(|d| d.moved) || !self.mixer_shown() {
            return None;
        }
        let p = self.project();
        let s = self
            .mixer_strips(g)
            .into_iter()
            .find(|s| self.strip_holds(g, s, x, y) && s.inserts.contains(x, y))?;
        let track = p.tracks.get(&s.track)?;
        if track.frozen.is_some() {
            return None;
        }
        let flow = self.strip_flow(&s, track);
        let index = super::flow::drop_index(track, &flow, y);
        self.seam_at(g, s.track, Spot::Drop(index))
    }

    /// The pointer moved over the body: the seam it shows or gives up.
    pub(crate) fn seam_moved(&mut self, g: &Geometry, x: f32, y: f32, now: Instant) {
        self.mixer.seam.feed(now, x, y);
        if self.seam_listed() {
            self.mixer.seam.leave = None;
            return;
        }
        let from = self
            .mixer
            .seam
            .live()
            .and_then(|s| self.seam_at(g, s.track, s.spot))
            .map(|c| c.at);
        if self.mixer.fx_drag.as_ref().is_some_and(|d| d.moved) {
            let cand = self.seam_drop(g, x, y);
            self.mixer.seam.dragging(now, cand, from);
            return;
        }
        // a plug-in held from the browser's list lands where the line shows
        if self.browser.drag.as_ref().is_some_and(|d| d.moved) {
            let cand = self.seam_cand(g, x, y);
            self.mixer.seam.dragging(now, cand, from);
            return;
        }
        let cand = if self.seam_busy() {
            None
        } else {
            self.seam_cand(g, x, y)
        };
        let hold = cand.is_none() && self.seam_hold(g, x, y);
        self.mixer.seam.pointer(now, cand, from, hold);
        self.seam_advance(g, now);
    }

    /// The pointer goes slowly up or down over the middle of the rows of the strip whose seam
    /// shows: it is on its way to the next seam.
    fn seam_hold(&self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(live) = self.mixer.seam.live().copied() else {
            return false;
        };
        let (speed, dx, dy) = self.mixer.seam.motion();
        if speed > SLOW || dy.abs() < dx.abs() {
            return false;
        }
        let Some(s) = self
            .mixer_strips(g)
            .into_iter()
            .find(|s| s.track == live.track)
        else {
            return false;
        };
        let p = self.project();
        let Some(track) = p.tracks.get(&live.track) else {
            return false;
        };
        let flow = self.strip_flow(&s, track);
        let Some(panel) = flow.insert_panel() else {
            return false;
        };
        s.inserts.contains(x, y)
            && x >= panel.x + AREA_SIDE + EDGE
            && x < panel.right() - AREA_SIDE - EDGE
    }

    /// Time passes (a frame): the grace ends, the going ends, what waited is shown. True while
    /// the seam needs frames.
    pub(crate) fn seam_tick(&mut self, g: &Geometry, now: Instant) -> bool {
        let reduced = self.seam_reduced();
        let held = self.mixer.fx_drag.as_ref().is_some_and(|d| d.moved)
            || self.browser.drag.as_ref().is_some_and(|d| d.moved);
        if !self.seam_busy() && !held {
            self.mixer.seam.end_drag(now, reduced);
        }
        if self.mixer.seam.shown.is_none() && !self.mixer.seam.moving(now, reduced) {
            return false;
        }
        // the rows changed under it: it is gone, with no going
        if let Some(sh) = self.mixer.seam.shown
            && sh.going.is_none()
            && self.seam_at(g, sh.track, sh.spot).is_none()
        {
            self.mixer.seam.clear();
            return false;
        }
        // the list that held it shut: it goes with the pointer's next move, or now when the
        // pointer is away
        if !self.seam_listed()
            && let Some(sh) = self.mixer.seam.shown
            && sh.going.is_none()
            && !sh.drag
            && self.mixer.seam.leave.is_none()
            && self.mixer.seam.wait.is_none()
        {
            // (a pointer not seen over the mixer has begun its own leave already)
            if let Some(at) = self.mixer.seam.last() {
                let cand = self.seam_cand(g, at.0, at.1);
                if !cand.is_some_and(|c| c.track == sh.track && c.spot == sh.spot) {
                    self.mixer.seam.leave = Some(now);
                }
            }
        }
        self.seam_advance(g, now);
        self.mixer.seam.moving(now, reduced)
    }

    fn seam_advance(&mut self, g: &Geometry, now: Instant) {
        let reduced = self.seam_reduced();
        let from = self
            .mixer
            .seam
            .live()
            .and_then(|s| self.seam_at(g, s.track, s.spot))
            .map(|c| c.at);
        let under = self
            .mixer
            .seam
            .last()
            .and_then(|at| self.seam_cand(g, at.0, at.1));
        let again =
            |t: TrackId, spot: Spot| under.filter(|c| c.track == t && c.spot == spot && c.mid);
        self.mixer.seam.advance(now, reduced, again, from);
    }

    /// The pointer left the mixer, or the window.
    pub(crate) fn seam_left(&mut self, now: Instant) {
        self.mixer.seam.forget();
        self.mixer.seam.wait = None;
        if !self.seam_listed() {
            self.mixer.seam.begin_leave(now);
        }
    }

    /// Whether the seam on show is pressed at `(x, y)`: the hit a press finds there.
    pub(crate) fn seam_hit(&self, t: &Track, flow: &Flow, x: f32, y: f32) -> Option<MixerHit> {
        if t.frozen.is_some() {
            return None;
        }
        let (spot, shown) = if self.mixer.coarse {
            let (id, pick) = self.mixer.chosen.filter(|(id, _)| *id == t.id)?;
            let _ = id;
            (Spot::After(pick), true)
        } else {
            let held = self.browser.drag.as_ref().is_some_and(|d| d.moved);
            let live = self
                .mixer
                .seam
                .live()
                .filter(|s| s.track == t.id && (!s.drag || held))?;
            (live.spot, true)
        };
        let seam = flow.seam(t, spot)?;
        let p = place(flow, &seam)?;
        zone(&p, flow.coarse(), shown, x, y)?;
        Some(MixerHit::Seam(t.id, spot))
    }

    /// The rows changed, or the song did: nothing shows.
    pub fn seam_reset(&mut self) {
        self.mixer.seam = Live::default();
    }

    /// A seam on show for a picture: the line under or over the row `pick` on `t`, drawn
    /// as if the pointer had been there a while.
    pub fn seam_show(&mut self, t: TrackId, spot: Spot, plus: bool) {
        let c = Cand {
            track: t,
            spot,
            at: (0.0, 0.0),
            mid: true,
        };
        let long_ago = Instant::now() - Duration::from_secs(5);
        self.mixer.seam.start(long_ago, &c, plus, !plus, None);
    }
}

#[cfg(test)]
#[path = "seam_tests.rs"]
mod tests;

impl Hit {
    /// The seam a hit is on.
    pub fn seam(self) -> Option<(TrackId, Spot)> {
        match self {
            Hit::Mixer(MixerHit::Seam(t, s)) => Some((t, s)),
            _ => None,
        }
    }
}
