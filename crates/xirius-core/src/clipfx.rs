//! What shapes the sound of an audio clip besides its own gain: fades at both
//! ends, a gain envelope, and the crossfade where two clips overlap (EDT-002).
//!
//! It is kept in a table of the project beside the tracks, keyed by clip, so a
//! clip with none of it (almost every clip) costs nothing in the song file and
//! the model of the clip itself stays as it was.
//!
//! * A fade is a length and a curve. The curve runs from -1 to 1: 0 is a line,
//!   0.5 keeps the power of two fades constant (what a crossfade wants), and the
//!   sign bends it to either side.
//! * The envelope is a list of points, `tick` counted from the start of the
//!   clip, `db` an extra gain on top of the clip's own. Between two points the
//!   gain moves in a straight line in dB; before the first and after the last
//!   it holds.
//! * A crossfade is stored on the right clip as a reference to the left one. Its
//!   length is never stored: it is what the two clips overlap by, so moving a
//!   clip changes the crossfade with it, and a pair that no longer overlaps has
//!   none.
//! * A loop (EDT-041) is a stretch of the clip's content, `start` counted from
//!   the start of the clip: the clip plays up to its end and then repeats it
//!   until the clip ends. It is the one part of the row a MIDI clip has too.

use serde::{Deserialize, Serialize};

use crate::id::ClipId;
use crate::model::{Clip, Project, Track};
use crate::time::Tick;

/// The curve limits.
pub const CURVE_MIN: f64 = -1.0;
pub const CURVE_MAX: f64 = 1.0;
/// What a crossfade starts with: equal power.
pub const CROSS_CURVE: f64 = 0.5;

#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Fade {
    #[serde(default)]
    pub length: Tick,
    #[serde(default)]
    pub curve: f64,
}

impl Fade {
    pub fn is_none(&self) -> bool {
        self.length <= 0
    }
    fn is_default(&self) -> bool {
        self.length == 0 && self.curve == 0.0
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct GainPoint {
    /// From the start of the clip.
    pub tick: Tick,
    pub db: f64,
}

/// A crossfade with the clip that ends under this one's start.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Cross {
    pub with: ClipId,
    #[serde(default)]
    pub curve: f64,
}

/// The stretch of a clip's content that repeats (EDT-041), in ticks from the
/// start of the clip.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ClipLoop {
    pub start: Tick,
    pub length: Tick,
}

impl ClipLoop {
    pub fn end(&self) -> Tick {
        self.start + self.length
    }

    /// What a clip `clip_len` long plays: `(at, from, len)` in ticks from the
    /// start of the clip, the content from `from` sounding at `at` for `len`.
    /// First the content up to the loop's end, then the loop again and again.
    pub fn pieces(&self, clip_len: Tick) -> Vec<(Tick, Tick, Tick)> {
        let mut out = vec![(0, 0, self.end().min(clip_len))];
        if self.length <= 0 {
            return out;
        }
        let mut at = self.end();
        while at < clip_len {
            out.push((at, self.start, self.length.min(clip_len - at)));
            at += self.length;
        }
        out
    }
}

/// What a clip with no loop plays: its content where it is.
pub fn pieces(looped: Option<ClipLoop>, clip_len: Tick) -> Vec<(Tick, Tick, Tick)> {
    match looped {
        Some(l) => l.pieces(clip_len),
        None => vec![(0, 0, clip_len)],
    }
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct ClipFx {
    #[serde(default, skip_serializing_if = "Fade::is_default")]
    pub fade_in: Fade,
    #[serde(default, skip_serializing_if = "Fade::is_default")]
    pub fade_out: Fade,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<GainPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross: Option<Cross>,
    #[serde(default, rename = "loop", skip_serializing_if = "Option::is_none")]
    pub looped: Option<ClipLoop>,
}

impl ClipFx {
    pub fn is_default(&self) -> bool {
        self.fade_in.is_default()
            && self.fade_out.is_default()
            && self.points.is_empty()
            && self.cross.is_none()
            && self.looped.is_none()
    }
}

/// The gain, 0 to 1, of a fade that is `x` of the way through (0 at its quiet
/// end, 1 at its loud end). The exponent is 1 for a line, 1/2 for equal power.
pub fn fade_gain(x: f64, curve: f64) -> f64 {
    x.clamp(0.0, 1.0)
        .powf(fade_exponent(curve.clamp(CURVE_MIN, CURVE_MAX)))
}

pub fn fade_exponent(curve: f64) -> f64 {
    (-2.0 * curve).exp2()
}

/// The envelope in dB at `tick` (from the clip's start); 0 with no points.
pub fn envelope_db_at(points: &[GainPoint], tick: Tick) -> f64 {
    let Some(first) = points.first() else {
        return 0.0;
    };
    if tick <= first.tick {
        return first.db;
    }
    let i = points.partition_point(|x| x.tick <= tick);
    if i >= points.len() {
        return points[points.len() - 1].db;
    }
    let (a, b) = (points[i - 1], points[i]);
    let span = (b.tick - a.tick).max(1) as f64;
    a.db + (b.db - a.db) * ((tick - a.tick) as f64 / span)
}

/// What a clip plays with once the crossfades are worked out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    pub fade_in: Fade,
    pub fade_out: Fade,
    pub points: Vec<GainPoint>,
    /// Whether the in or the out fade is the clip's share of a crossfade.
    pub crossed_in: bool,
    pub crossed_out: bool,
}

impl Resolved {
    pub fn is_plain(&self) -> bool {
        self.fade_in.is_none() && self.fade_out.is_none() && self.points.is_empty()
    }
}

/// How far `right` runs into `left`, if `left` is the clip a crossfade on
/// `right` means: the left one starts before and ends inside the right.
pub fn overlap(left: &Clip, right: &Clip) -> Tick {
    if left.start >= right.start || left.end() <= right.start {
        return 0;
    }
    (left.end().min(right.end()) - right.start).min(right.length)
}

impl Project {
    pub fn fx(&self, id: ClipId) -> Option<&ClipFx> {
        self.clip_fx.get(&id)
    }

    /// The loop of a clip, if it repeats (EDT-041).
    pub fn clip_loop(&self, id: ClipId) -> Option<ClipLoop> {
        self.clip_fx.get(&id).and_then(|f| f.looped)
    }

    /// The table row of a clip, made if it has none. Call `tidy_fx` after.
    pub fn fx_mut(&mut self, id: ClipId) -> &mut ClipFx {
        self.clip_fx.entry(id).or_default()
    }

    /// Drops the row of a clip that has nothing in it.
    pub fn tidy_fx(&mut self, id: ClipId) {
        if self.clip_fx.get(&id).is_some_and(ClipFx::is_default) {
            self.clip_fx.remove(&id);
        }
    }

    /// Forgets a clip: its row, and the crossfades that point at it.
    pub fn fx_forget(&mut self, id: ClipId) {
        self.clip_fx.remove(&id);
        for fx in self.clip_fx.values_mut() {
            if fx.cross.is_some_and(|c| c.with == id) {
                fx.cross = None;
            }
        }
        self.clip_fx.retain(|_, fx| !fx.is_default());
    }

    /// Drops the rows of clips that are gone. A command that deletes a clip
    /// does not have to know about this table; the session calls it after every
    /// edit.
    pub fn prune_clip_fx(&mut self) {
        if self.clip_fx.is_empty() {
            return;
        }
        let live: std::collections::HashSet<ClipId> = self
            .tracks
            .values()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        self.clip_fx.retain(|id, _| live.contains(id));
        for fx in self.clip_fx.values_mut() {
            if fx.cross.is_some_and(|c| !live.contains(&c.with)) {
                fx.cross = None;
            }
        }
        self.clip_fx.retain(|_, fx| !fx.is_default());
    }

    /// The crossfade this clip is the right side of: the left clip, and how
    /// far they overlap. `None` when the pair does not overlap any more.
    pub fn cross_in(&self, track: &Track, clip: &Clip) -> Option<(ClipId, Tick, f64)> {
        let c = self.clip_fx.get(&clip.id)?.cross?;
        let left = track.clips.iter().find(|x| x.id == c.with)?;
        let len = overlap(left, clip);
        (len > 0).then_some((left.id, len, c.curve))
    }

    /// The crossfade this clip is the left side of (the right clip, the
    /// overlap, the curve). With several, the longest.
    pub fn cross_out(&self, track: &Track, clip: &Clip) -> Option<(ClipId, Tick, f64)> {
        if self.clip_fx.is_empty() {
            return None;
        }
        track
            .clips
            .iter()
            .filter_map(|r| {
                let c = self.clip_fx.get(&r.id)?.cross?;
                (c.with == clip.id).then(|| (r.id, overlap(clip, r), c.curve))
            })
            .filter(|x| x.1 > 0)
            .max_by_key(|x| x.1)
    }

    /// The fades and envelope a clip plays with.
    pub fn resolve_fx(&self, track: &Track, clip: &Clip) -> Resolved {
        let own = self.clip_fx.get(&clip.id).cloned().unwrap_or_default();
        let mut r = Resolved {
            fade_in: own.fade_in,
            fade_out: own.fade_out,
            points: own.points,
            ..Resolved::default()
        };
        if let Some((_, len, curve)) = self.cross_in(track, clip) {
            r.fade_in = Fade { length: len, curve };
            r.crossed_in = true;
        }
        if let Some((_, len, curve)) = self.cross_out(track, clip) {
            r.fade_out = Fade { length: len, curve };
            r.crossed_out = true;
        }
        // The two fades never cross each other.
        r.fade_in.length = r.fade_in.length.clamp(0, clip.length);
        r.fade_out.length = r.fade_out.length.clamp(0, clip.length);
        r
    }

    /// The row of `from` for a clip that is a copy of it: the fades and the envelope come along, a crossfade does not (it is the pair
    /// that has it).
    pub fn copy_fx(&mut self, from: ClipId, to: ClipId) {
        if let Some(fx) = self.clip_fx.get(&from) {
            let mut fx = fx.clone();
            fx.cross = None;
            if !fx.is_default() {
                self.clip_fx.insert(to, fx);
            }
        }
    }

    /// After a set of clips was copied, the crossfades between them: each copy
    /// is crossed with the copy of the clip its original was crossed with.
    pub fn copy_crosses(&mut self, pairs: &[(ClipId, ClipId)]) {
        for (old, new) in pairs {
            let Some(c) = self.clip_fx.get(old).and_then(|f| f.cross) else {
                continue;
            };
            if let Some((_, with)) = pairs.iter().find(|(o, _)| *o == c.with) {
                self.fx_mut(*new).cross = Some(Cross { with: *with, ..c });
            }
        }
    }

    /// The left edge of a clip moved by `by` (its content stays where it was in
    /// the song): the envelope points shift the other way, the value at the old
    /// edge is held, and a fade-in stays on the edge.
    pub fn fx_trimmed(&mut self, id: ClipId, by: Tick) {
        let Some(fx) = self.clip_fx.get_mut(&id) else {
            return;
        };
        if let Some(l) = fx.looped {
            // What is cut off the front of the loop is gone with the content.
            let start = l.start - by;
            let cut = (-start).max(0);
            fx.looped = (l.length - cut > 0).then_some(ClipLoop {
                start: start.max(0),
                length: l.length - cut,
            });
        }
        if !fx.points.is_empty() {
            let held = envelope_db_at(&fx.points, by);
            let mut pts: Vec<GainPoint> = fx
                .points
                .iter()
                .filter(|x| x.tick - by > 0)
                .map(|x| GainPoint {
                    tick: x.tick - by,
                    db: x.db,
                })
                .collect();
            pts.insert(0, GainPoint { tick: 0, db: held });
            fx.points = pts;
        }
        self.tidy_fx(id);
    }

    /// A clip cut at `rel`: the left keeps the fade-in, the right gets the
    /// fade-out, the envelope is cut with the held value at the cut. A
    /// crossfade pointing at the original now points at the right half (the
    /// part that ends under it).
    pub fn fx_split(&mut self, left: ClipId, right: ClipId, rel: Tick, left_len: Tick) {
        let own = self.clip_fx.get(&left).cloned();
        for fx in self.clip_fx.values_mut() {
            if fx.cross.is_some_and(|c| c.with == left) {
                // The overlap is at the right end of the original.
                fx.cross = fx.cross.map(|c| Cross { with: right, ..c });
            }
        }
        let Some(own) = own else {
            return;
        };
        let mut l = ClipFx {
            fade_in: own.fade_in,
            looped: own.looped,
            ..ClipFx::default()
        };
        let mut r = ClipFx {
            fade_out: own.fade_out,
            cross: None,
            // The right half repeats only a loop that is wholly in it.
            looped: own.looped.filter(|l| l.start >= rel).map(|l| ClipLoop {
                start: l.start - rel,
                ..l
            }),
            ..ClipFx::default()
        };
        // A crossfade in belongs to the left half, which keeps the start.
        l.cross = own.cross;
        l.fade_in.length = l.fade_in.length.min(left_len);
        if !own.points.is_empty() {
            let at = envelope_db_at(&own.points, rel);
            l.points = own
                .points
                .iter()
                .filter(|x| x.tick < rel)
                .copied()
                .collect();
            if !l.points.is_empty() {
                l.points.push(GainPoint { tick: rel, db: at });
            }
            let mut rp = vec![GainPoint { tick: 0, db: at }];
            rp.extend(
                own.points
                    .iter()
                    .filter(|x| x.tick > rel)
                    .map(|x| GainPoint {
                        tick: x.tick - rel,
                        db: x.db,
                    }),
            );
            r.points = rp;
        }
        if l.is_default() {
            self.clip_fx.remove(&left);
        } else {
            self.clip_fx.insert(left, l);
        }
        if !r.is_default() {
            self.clip_fx.insert(right, r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_and_equal_power() {
        for x in [0.0, 0.25, 0.5, 1.0] {
            assert!((fade_gain(x, 0.0) - x).abs() < 1e-12);
        }
        // two fades meeting at the same x keep their power
        for x in [0.1, 0.3, 0.5, 0.9] {
            let p = fade_gain(x, 0.5).powi(2) + fade_gain(1.0 - x, 0.5).powi(2);
            assert!((p - 1.0).abs() < 1e-9, "{p}");
        }
        // a negative curve is quieter in the middle, a positive one louder
        assert!(fade_gain(0.5, -0.5) < 0.5);
        assert!(fade_gain(0.5, 0.5) > 0.5);
    }

    #[test]
    fn the_envelope_holds_at_the_ends_and_moves_in_decibels_between() {
        let pts = [
            GainPoint {
                tick: 100,
                db: -6.0,
            },
            GainPoint { tick: 300, db: 6.0 },
        ];
        assert_eq!(envelope_db_at(&[], 50), 0.0);
        assert_eq!(envelope_db_at(&pts, 0), -6.0);
        assert_eq!(envelope_db_at(&pts, 200), 0.0);
        assert_eq!(envelope_db_at(&pts, 900), 6.0);
    }

    #[test]
    fn edt_041_a_loop_plays_up_to_its_end_then_again_and_follows_a_trim_and_a_cut() {
        let l = ClipLoop {
            start: 100,
            length: 200,
        };
        assert_eq!(
            l.pieces(750),
            vec![
                (0, 0, 300),
                (300, 100, 200),
                (500, 100, 200),
                (700, 100, 50)
            ]
        );
        assert_eq!(pieces(None, 750), vec![(0, 0, 750)]);
        // A clip shorter than the loop's end plays only the start of it.
        assert_eq!(l.pieces(250), vec![(0, 0, 250)]);

        let mut p = Project::new("loop");
        let (a, b) = (ClipId(1), ClipId(2));
        p.fx_mut(a).looped = Some(l);
        // The left edge 150 later: the loop keeps its place, its first 50 gone.
        p.fx_trimmed(a, 150);
        assert_eq!(
            p.clip_loop(a),
            Some(ClipLoop {
                start: 0,
                length: 150
            })
        );
        // Wholly cut off: no loop, no row.
        p.fx_trimmed(a, 400);
        assert_eq!(p.clip_loop(a), None);
        assert!(p.fx(a).is_none());

        // A cut before the loop: both halves know it, the right one from its own start.
        p.fx_mut(a).looped = Some(l);
        p.fx_split(a, b, 50, 50);
        assert_eq!(p.clip_loop(a), Some(l));
        assert_eq!(
            p.clip_loop(b),
            Some(ClipLoop {
                start: 50,
                length: 200
            })
        );
        // A cut inside it: only the left half repeats.
        p.fx_split(b, ClipId(3), 100, 100);
        assert!(p.clip_loop(b).is_some());
        assert_eq!(p.clip_loop(ClipId(3)), None);
    }
}
