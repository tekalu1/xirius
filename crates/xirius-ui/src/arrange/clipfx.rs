//! What the pointer does to an audio clip beyond moving and trimming it: the
//! fade flags and their curve squares, the volume square, the gain line and
//! its points, the crossfade's centre, the slip, and the boundary of two
//! clips that touch (EDT-002, 017, 036, 037, 039, 040).
//!
//! Each one is a [`Spot::Fx`]: [`Studio::fx_spot`] finds it under the pointer
//! and [`Studio::plan_fx`] works out the one core command a drag runs
//! (`clip.fade.set`, `clip.gain.set`, `clip.gain.point.*`, `clip.fade.cross`,
//! `clip.slip`, `clip.resize --adjacent`), so a drag is one history step.
//! How these are painted is in [`super::clipfx_paint`]; both read the same
//! [`Handles`], so what is drawn is what is hit.

use xirius_core::clipfx::{Fade, GainPoint, envelope_db_at, fade_gain};
use xirius_core::model::Clip;

use super::*;

/// The side of a clip a fade is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    In,
    Out,
}

/// The part of a clip under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxPart {
    /// The square at the end of a fade: its length.
    Flag(Side),
    /// The square on a fade's curve: its shape.
    Mid(Side),
    /// The centre of a crossfade (on the right clip): left and right move it,
    /// up and down shape it.
    Cross,
    /// The square on the gain line: the clip's gain.
    Gain,
    /// A point of the gain line.
    Point(usize),
    /// The gain line: a click on it of a selected clip adds a point, a drag
    /// moves the clip as its body does.
    Line { armed: bool },
    /// Ctrl + Alt on an audio clip: what it plays slips, the clip stays.
    Slip,
    /// The join with the clip that starts where this one ends.
    Boundary(ClipId),
}

/// What a drag shows in the clip itself while it is under way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FxEdit {
    Fade(Side, Fade),
    GainBy(f64),
    Point(usize, GainPoint),
    CrossIn(f64),
    CrossOut(f64),
}

/// The handle squares: the flags are 9 px, the rest a little smaller.
pub const FLAG: f32 = 9.0;
pub const SQUARE: f32 = 8.0;
/// How far from a handle a press still takes it.
const SLOP: f32 = 2.0;
/// Below this width a clip shows no handles (the edges are all there is).
pub const MIN_W: f32 = 40.0;
/// A fade has to be this wide to carry a curve square.
const MIN_FADE_PX: f32 = 28.0;
/// How near the gain line a press is on it, and how near a point it is on it.
const LINE_HIT: f32 = 4.0;
const POINT_HIT: f32 = 6.0;
/// The gain line spans these decibels over the height of the clip.
pub const DB_LO: f64 = -36.0;
pub const DB_HI: f64 = MAX_DB;
/// The clip's edge, where the line cannot be taken.
const PAD: f32 = 3.0;

/// What an audio clip plays with, as the arrangement shows and takes hold of
/// it: the crossfades are worked out, so a crossfaded end is a fade that is
/// not the clip's own.
#[derive(Clone, Debug, PartialEq)]
pub struct FxView {
    pub gain_db: f64,
    pub own_in: Fade,
    pub own_out: Fade,
    pub fade_in: Fade,
    pub fade_out: Fade,
    pub points: Vec<GainPoint>,
    pub crossed_in: bool,
    pub crossed_out: bool,
}

impl FxView {
    /// None for a MIDI clip: it has no fades or gain line.
    pub fn of(p: &Project, track: &Track, clip: &Clip) -> Option<FxView> {
        let ClipBody::Audio { gain_db, .. } = &clip.body else {
            return None;
        };
        let own = p.fx(clip.id).cloned().unwrap_or_default();
        let r = p.resolve_fx(track, clip);
        Some(FxView {
            gain_db: *gain_db,
            own_in: own.fade_in,
            own_out: own.fade_out,
            fade_in: r.fade_in,
            fade_out: r.fade_out,
            points: r.points,
            crossed_in: r.crossed_in,
            crossed_out: r.crossed_out,
        })
    }

    pub fn apply(&mut self, edit: &FxEdit) {
        match *edit {
            FxEdit::Fade(Side::In, f) => {
                self.own_in = f;
                if !self.crossed_in {
                    self.fade_in = f;
                }
            }
            FxEdit::Fade(Side::Out, f) => {
                self.own_out = f;
                if !self.crossed_out {
                    self.fade_out = f;
                }
            }
            FxEdit::GainBy(d) => self.gain_db = (self.gain_db + d).clamp(MIN_DB, MAX_DB),
            FxEdit::Point(i, pt) => {
                if let Some(x) = self.points.get_mut(i) {
                    *x = pt;
                }
            }
            FxEdit::CrossIn(c) => self.fade_in.curve = c,
            FxEdit::CrossOut(c) => self.fade_out.curve = c,
        }
    }

    /// The level, in dB, the line is at `tick` from the clip's start.
    pub fn level_at(&self, tick: Tick) -> f64 {
        self.gain_db + envelope_db_at(&self.points, tick)
    }
}

/// Where a clip's handles are on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Handles {
    pub flag_in: Option<Rect>,
    pub flag_out: Option<Rect>,
    pub mid_in: Option<(f32, f32)>,
    pub mid_out: Option<(f32, f32)>,
    pub cross: Option<(f32, f32)>,
    pub gain: (f32, f32),
    pub points: Vec<(f32, f32)>,
    /// The gain line from the clip's left edge to its right.
    pub line: Vec<(f32, f32)>,
}

/// The y of `db` on the gain line of a clip in `r`.
pub fn db_y(r: Rect, db: f64) -> f32 {
    let t = ((db - DB_LO) / (DB_HI - DB_LO)).clamp(0.0, 1.0) as f32;
    r.bottom() - PAD - t * (r.h - PAD * 2.0)
}

/// How many decibels one pixel up is worth on a clip in `r`.
pub fn db_per_px(r: Rect) -> f64 {
    (DB_HI - DB_LO) / f64::from((r.h - PAD * 2.0).max(1.0))
}

/// A point on a fade's curve, `f` (0 to 1) of the way along it: the level is
/// the height of the clip.
pub fn fade_xy(r: Rect, width: f32, side: Side, f: f32, curve: f64) -> (f32, f32) {
    let (x, level) = match side {
        Side::In => (r.x + f * width, fade_gain(f64::from(f), curve)),
        Side::Out => (
            r.right() - width + f * width,
            fade_gain(1.0 - f64::from(f), curve),
        ),
    };
    (x, r.bottom() - r.h * level as f32)
}

impl FxView {
    pub fn handles(&self, s: &Studio, r: Rect) -> Handles {
        let w_in = s.ticks_to_px(self.fade_in.length).min(r.w);
        let w_out = s.ticks_to_px(self.fade_out.length).min(r.w);
        let flag_in = (!self.crossed_in).then(|| {
            let x = (r.x + w_in).clamp(r.x, (r.right() - FLAG * 2.0 - 1.0).max(r.x));
            Rect::new(x, r.y, FLAG, FLAG)
        });
        let flag_out = (!self.crossed_out).then(|| {
            let lo = flag_in.map_or(r.x, |f| f.right() + 1.0);
            let x = (r.right() - w_out - FLAG).clamp(lo, (r.right() - FLAG).max(lo));
            Rect::new(x, r.y, FLAG, FLAG)
        });
        let mid_in = (!self.crossed_in && w_in >= MIN_FADE_PX)
            .then(|| fade_xy(r, w_in, Side::In, 0.5, self.fade_in.curve));
        let mid_out = (!self.crossed_out && w_out >= MIN_FADE_PX)
            .then(|| fade_xy(r, w_out, Side::Out, 0.5, self.fade_out.curve));
        let cross = self.crossed_in.then(|| (r.x + w_in / 2.0, r.y + r.h / 2.0));
        let at = |t: Tick| (r.x + s.ticks_to_px(t)).clamp(r.x, r.right());
        let len = s.px_to_ticks(r.w);
        let mut line = vec![(r.x, db_y(r, self.level_at(0)))];
        let mut points = Vec::new();
        for pt in &self.points {
            let xy = (at(pt.tick), db_y(r, self.gain_db + pt.db));
            points.push(xy);
            line.push(xy);
        }
        line.push((r.right(), db_y(r, self.level_at(len))));
        let mid = len / 2;
        let gain = (r.x + r.w / 2.0, db_y(r, self.level_at(mid)));
        Handles {
            flag_in,
            flag_out,
            mid_in,
            mid_out,
            cross,
            gain,
            points,
            line,
        }
    }
}

impl Handles {
    /// The y of the gain line at window x.
    pub fn line_y(&self, x: f32) -> f32 {
        let l = &self.line;
        for w in l.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            if x >= x0 && x <= x1 {
                if x1 - x0 < 0.5 {
                    return y1;
                }
                return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
            }
        }
        l.last().map_or(0.0, |p| p.1)
    }
}

fn near(c: (f32, f32), x: f32, y: f32, half: f32) -> bool {
    (x - c.0).abs() <= half && (y - c.1).abs() <= half
}

fn grow(r: Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.w + by * 2.0, r.h + by * 2.0)
}

impl Studio {
    /// The handle under (`x`, `y`) on a track's clips, if any. The parts are
    /// tried flags first, then the curve squares, the crossfade, the gain
    /// points and square, the boundary of touching clips, and last the line.
    pub(super) fn fx_spot(
        &self,
        g: &Geometry,
        row: &Row,
        track: &Track,
        x: f32,
        y: f32,
    ) -> Option<Spot> {
        let p = self.project();
        let t = track.id;
        let spot = |clip: &Clip, part: FxPart| Spot::Fx {
            track: t,
            clip: clip.id,
            part,
        };
        let shown: Vec<(&Clip, Rect, FxView)> = track
            .clips
            .iter()
            .filter_map(|c| {
                let r = self.clip_rect(g, row, c);
                (r.w >= MIN_W && r.contains(x, y)).then_some(())?;
                Some((c, r, FxView::of(&p, track, c)?))
            })
            .collect();
        // The one painted last is on top.
        for (clip, r, view) in shown.iter().rev() {
            let h = view.handles(self, *r);
            if let Some(f) = h.flag_in
                && grow(f, SLOP).contains(x, y)
            {
                return Some(spot(clip, FxPart::Flag(Side::In)));
            }
            if let Some(f) = h.flag_out
                && grow(f, SLOP).contains(x, y)
            {
                return Some(spot(clip, FxPart::Flag(Side::Out)));
            }
            if let Some(m) = h.mid_in
                && near(m, x, y, SQUARE / 2.0 + SLOP)
            {
                return Some(spot(clip, FxPart::Mid(Side::In)));
            }
            if let Some(m) = h.mid_out
                && near(m, x, y, SQUARE / 2.0 + SLOP)
            {
                return Some(spot(clip, FxPart::Mid(Side::Out)));
            }
            if let Some(m) = h.cross
                && near(m, x, y, SQUARE / 2.0 + SLOP + 1.0)
            {
                return Some(spot(clip, FxPart::Cross));
            }
            if let Some(i) = h
                .points
                .iter()
                .position(|pt| near(*pt, x, y, POINT_HIT.min(r.w / 2.0)))
            {
                return Some(spot(clip, FxPart::Point(i)));
            }
            if near(h.gain, x, y, SQUARE / 2.0 + SLOP) {
                return Some(spot(clip, FxPart::Gain));
            }
        }
        // The join of two clips that touch.
        for left in &track.clips {
            let Some(right) = track
                .clips
                .iter()
                .find(|r| r.id != left.id && r.start == left.end())
            else {
                continue;
            };
            let r = self.clip_rect(g, row, left);
            let jx = self.tick_x(g, left.end());
            if (x - jx).abs() <= Arrange::GRIP / 2.0 && y >= r.y && y < r.bottom() {
                return Some(spot(left, FxPart::Boundary(right.id)));
            }
        }
        for (clip, r, view) in shown.iter().rev() {
            let h = view.handles(self, *r);
            if x < r.x + Arrange::GRIP || x >= r.right() - Arrange::GRIP {
                continue;
            }
            if (y - h.line_y(x)).abs() <= LINE_HIT {
                let key = (t, clip.id);
                let armed = self.arrange.clips.contains(&key) || self.selected_clip == Some(key);
                return Some(spot(clip, FxPart::Line { armed }));
            }
        }
        None
    }

    /// Ctrl + Alt over the body of an audio clip starts a slip.
    pub(super) fn slip_spot(&self, spot: &Spot, mods: Mods) -> Option<Spot> {
        if !(mods.ctrl && mods.alt) || self.tool != Tool::Select {
            return None;
        }
        let (track, clip) = match spot {
            Spot::Clip { track, clip, .. } | Spot::Fx { track, clip, .. } => (*track, *clip),
            _ => return None,
        };
        let p = self.project();
        let c = p.tracks.get(&track)?.clips.iter().find(|c| c.id == clip)?;
        matches!(c.body, ClipBody::Audio { .. }).then_some(Spot::Fx {
            track,
            clip,
            part: FxPart::Slip,
        })
    }

    /// A press on a handle. False when it is finished (a point erased).
    pub(super) fn fx_down(&mut self, d: &Drag, t: TrackId, c: ClipId, part: FxPart) -> bool {
        self.error = None;
        if let FxPart::Point(index) = part
            && (d.mods.alt || d.tool == Tool::Eraser)
        {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            self.arrange_run(
                "clip.gain.point.delete",
                json!({ "id": c.to_string(), "index": index }),
            );
            return false;
        }
        if matches!(part, FxPart::Line { .. }) {
            return self.clip_down(d, t, c);
        }
        let key = (t, c);
        self.select_row(Some(RowKind::Track(t)));
        if !self.arrange.clips.contains(&key) {
            self.arrange.clips = BTreeSet::from([key]);
        }
        self.selected_clip = Some(key);
        self.arrange.touched();
        true
    }

    /// A press on a handle that did not move.
    pub(super) fn fx_click(&mut self, d: &Drag, t: TrackId, c: ClipId, part: FxPart) -> bool {
        if let FxPart::Line { armed: true } = part
            && d.tool == Tool::Select
            && !d.mods.shift
        {
            let p = self.project();
            let Some(clip) = p
                .tracks
                .get(&t)
                .and_then(|tr| tr.clips.iter().find(|k| k.id == c))
            else {
                return true;
            };
            let at = (d.tick - clip.start).clamp(0, clip.length);
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            self.arrange_run(
                "clip.gain.point.add",
                json!({ "id": c.to_string(), "at": at }),
            );
            return true;
        }
        if !d.mods.shift {
            self.arrange.clips = BTreeSet::from([(t, c)]);
            self.arrange.touched();
        }
        true
    }

    /// X makes a crossfade of the selected audio clips (or of each with its
    /// neighbour); Shift + X gives the ends of each a short fade.
    pub(super) fn fx_key(&mut self, key: &str, mods: Mods) -> bool {
        if key != "x" {
            return false;
        }
        let p = self.project();
        let ids: Vec<String> = self
            .arrange
            .clips
            .iter()
            .filter(|(t, c)| {
                p.tracks
                    .get(t)
                    .and_then(|t| t.clips.iter().find(|k| k.id == *c))
                    .is_some_and(|k| matches!(k.body, ClipBody::Audio { .. }))
            })
            .map(|(_, c)| c.to_string())
            .collect();
        if ids.is_empty() {
            return false;
        }
        if mods.shift {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            self.arrange_run("clip.fade.set", json!({ "ids": ids, "auto": true }));
        } else {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            self.arrange_run("clip.fade.cross", json!({ "ids": ids }));
        }
        true
    }

    /// What letting go of a drag on a handle would do.
    pub(super) fn plan_fx(
        &self,
        g: &Geometry,
        d: &Drag,
        t: TrackId,
        c: ClipId,
        part: FxPart,
    ) -> Option<Plan> {
        if let FxPart::Line { .. } = part {
            return self.plan_clip(g, d, t, c, Grip::Body);
        }
        let p = self.project();
        let track = p.tracks.get(&t)?;
        let clip = track.clips.iter().find(|k| k.id == c)?;
        let row = *self.tree().row_of(RowKind::Track(t))?;
        let r = self.clip_rect(g, &row, clip);
        let view = FxView::of(&p, track, clip);
        let (dx, dy) = (d.at.0 - d.from.0, d.at.1 - d.from.1);
        let shift = d.mods.shift;
        let ppq = f64::from(p.ppq);
        let beats = |ticks: Tick| ticks as f64 / ppq;
        let id = c.to_string();
        let plan = |command: &'static str, args: Value, tip: String, preview: Preview| Plan {
            command: Some((command, args)),
            select: None,
            tip,
            preview,
        };
        // A fade's length for a pointer `dx` px from where it was pressed.
        let fade_length = |side: Side, from: Tick| {
            let edge = match side {
                Side::In => clip.start + from,
                Side::Out => clip.end() - from,
            };
            let to = self.snap_tick(edge + self.px_to_ticks(dx), shift);
            match side {
                Side::In => to - clip.start,
                Side::Out => clip.end() - to,
            }
            .clamp(0, clip.length)
        };
        let curve_of = |from: f64| {
            let c = (from - f64::from(dy) / f64::from((r.h * 0.5).max(1.0))).clamp(-1.0, 1.0);
            (c * 100.0).round() / 100.0
        };
        match part {
            FxPart::Flag(side) => {
                let view = view?;
                let own = if side == Side::In {
                    view.own_in
                } else {
                    view.own_out
                };
                let length = fade_length(side, own.length);
                if length == own.length {
                    return None;
                }
                let key = if side == Side::In { "in" } else { "out" };
                let name = if side == Side::In {
                    "フェードイン"
                } else {
                    "フェードアウト"
                };
                Some(plan(
                    "clip.fade.set",
                    json!({ "ids": [id], key: length }),
                    format!("{name} {:.2} 拍", beats(length)),
                    Preview::Fx(vec![(
                        c,
                        FxEdit::Fade(
                            side,
                            Fade {
                                length,
                                curve: own.curve,
                            },
                        ),
                    )]),
                ))
            }
            FxPart::Mid(side) => {
                let view = view?;
                let own = if side == Side::In {
                    view.own_in
                } else {
                    view.own_out
                };
                let curve = curve_of(own.curve);
                let length = if shift {
                    fade_length(side, own.length).max(1)
                } else {
                    own.length
                };
                if curve == own.curve && length == own.length {
                    return None;
                }
                let (lk, ck) = if side == Side::In {
                    ("in", "in_curve")
                } else {
                    ("out", "out_curve")
                };
                let mut args = json!({ "ids": [id], ck: curve });
                if length != own.length {
                    args[lk] = json!(length);
                }
                Some(plan(
                    "clip.fade.set",
                    args,
                    format!("カーブ {curve:+.2}"),
                    Preview::Fx(vec![(c, FxEdit::Fade(side, Fade { length, curve }))]),
                ))
            }
            FxPart::Gain => {
                let view = view?;
                let sel: Vec<(TrackId, ClipId)> = if d.mods.alt {
                    vec![(t, c)]
                } else {
                    self.moving_clips(t, c)
                };
                let target = ((view.gain_db - f64::from(dy) * db_per_px(r)) * 10.0).round() / 10.0;
                let target = target.clamp(MIN_DB, MAX_DB);
                let by = target - view.gain_db;
                if by.abs() < 1e-9 {
                    return None;
                }
                let ids: Vec<String> = sel
                    .iter()
                    .filter(|(tt, cc)| {
                        p.tracks
                            .get(tt)
                            .and_then(|tr| tr.clips.iter().find(|k| k.id == *cc))
                            .is_some_and(|k| matches!(k.body, ClipBody::Audio { .. }))
                    })
                    .map(|(_, cc)| cc.to_string())
                    .collect();
                let edits = sel
                    .iter()
                    .map(|(_, cc)| (*cc, FxEdit::GainBy(by)))
                    .collect();
                Some(plan(
                    "clip.gain.set",
                    json!({ "ids": ids, "by": by }),
                    format!("{target:+.1} dB"),
                    Preview::Fx(edits),
                ))
            }
            FxPart::Point(i) => {
                let view = view?;
                let pt = *view.points.get(i)?;
                let at = self
                    .snap_tick(clip.start + pt.tick + self.px_to_ticks(dx), shift)
                    .clamp(clip.start, clip.end())
                    - clip.start;
                let db = ((pt.db - f64::from(dy) * db_per_px(r)) * 10.0).round() / 10.0;
                let db = db.clamp(MIN_DB - view.gain_db, MAX_DB - view.gain_db);
                if at == pt.tick && (db - pt.db).abs() < 1e-9 {
                    return None;
                }
                Some(plan(
                    "clip.gain.point.move",
                    json!({ "id": id, "index": i, "at": at, "db": db }),
                    format!("{:+.1} dB", view.gain_db + db),
                    Preview::Fx(vec![(c, FxEdit::Point(i, GainPoint { tick: at, db }))]),
                ))
            }
            FxPart::Cross => {
                let (left, len, curve) = p.cross_in(track, clip)?;
                // Whichever way the pointer has gone further decides.
                if dx.abs() >= dy.abs() {
                    let centre = clip.start + len / 2;
                    let by = self.snap_tick(centre + self.px_to_ticks(dx), shift) - centre;
                    let lc = track.clips.iter().find(|k| k.id == left)?;
                    let min = (i64::from(p.ppq) / 32).max(1);
                    let by = by.clamp(min - lc.length, clip.length - min);
                    if by == 0 {
                        return None;
                    }
                    let color = crate::draw::track_color(track);
                    let ghosts = vec![
                        Ghost {
                            track: t,
                            start: lc.start,
                            length: lc.length + by,
                            color,
                        },
                        Ghost {
                            track: t,
                            start: clip.start + by,
                            length: clip.length - by,
                            color,
                        },
                    ];
                    Some(plan(
                        "clip.fade.cross",
                        json!({ "ids": [id], "move": by }),
                        p.time_map().format(centre + by),
                        Preview::Clips(ghosts),
                    ))
                } else {
                    let to = curve_of(curve);
                    if to == curve {
                        return None;
                    }
                    Some(plan(
                        "clip.fade.cross",
                        json!({ "ids": [id], "curve": to }),
                        format!("カーブ {to:+.2}"),
                        Preview::Fx(vec![(c, FxEdit::CrossIn(to)), (left, FxEdit::CrossOut(to))]),
                    ))
                }
            }
            FxPart::Slip => {
                let sel = self.moving_clips(t, c);
                let mut lowest: Option<Tick> = None;
                let mut ids = Vec::new();
                for (tt, cc) in &sel {
                    let Some(k) = p
                        .tracks
                        .get(tt)
                        .and_then(|tr| tr.clips.iter().find(|k| k.id == *cc))
                    else {
                        continue;
                    };
                    if let ClipBody::Audio { offset, .. } = &k.body {
                        lowest = Some(lowest.map_or(*offset, |l| l.min(*offset)));
                        ids.push(cc.to_string());
                    }
                }
                // The content cannot slip back past the start of the file.
                let by = self.px_to_ticks(dx).min(lowest?);
                if by == 0 {
                    return None;
                }
                Some(plan(
                    "clip.slip",
                    json!({ "ids": ids, "by": by }),
                    format!("スリップ {:+.2} 拍", beats(by)),
                    Preview::Clips(Vec::new()),
                ))
            }
            FxPart::Boundary(right_id) => {
                let right = track.clips.iter().find(|k| k.id == right_id)?;
                let min = self
                    .tools()
                    .snap
                    .active(&self.meter(), shift)
                    .unwrap_or(i64::from(p.ppq) / 16)
                    .max(1);
                let to = self.snap_tick(clip.end() + self.px_to_ticks(dx), shift);
                let room_back = match &right.body {
                    ClipBody::Audio { offset, .. } => (*offset).min(right.start),
                    ClipBody::Midi { .. } => right.start,
                };
                let lo = (min.min(clip.length) - clip.length).max(-room_back);
                let hi = right.length - min.min(right.length);
                let by = (to - clip.end()).clamp(lo, hi.max(lo));
                if by == 0 {
                    return None;
                }
                let color = crate::draw::track_color(track);
                let ghosts = vec![
                    Ghost {
                        track: t,
                        start: clip.start,
                        length: clip.length + by,
                        color,
                    },
                    Ghost {
                        track: t,
                        start: right.start + by,
                        length: right.length - by,
                        color,
                    },
                ];
                Some(plan(
                    "clip.resize",
                    json!({ "ids": [id], "by": by, "adjacent": true }),
                    p.time_map().format(clip.end() + by),
                    Preview::Clips(ghosts),
                ))
            }
            FxPart::Line { .. } => None,
        }
    }
}
