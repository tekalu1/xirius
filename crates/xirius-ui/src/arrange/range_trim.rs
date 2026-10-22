//! The range over one lane or over the tempo row, taken hold of (AUT-010). In the upper half of
//! the range the select and range tools move every value of the stretch up or down with a
//! vertical drag (`range.trim`); the frame Alt+T or 「自由変形」 puts round the stretch scales the
//! values by the handle on its top edge and tilts them by the one on its corner
//! (`range.transform`). The lane shows the values as they would land while the drag goes on.

use serde_json::{Map, Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::commands::{RangeMove, bpm_at, moved_lane, moved_tempo, stretch};
use xirius_core::id::TrackId;
use xirius_core::time::Tick;

use super::{Arrange, Drag, Plan, Preview, Spot};
use crate::studio::{Geometry, Rect, Studio};
use crate::tools::Tool;
use crate::tree::{LANE_H, RowKind};

/// The lane a range is taken hold of in; None is the tempo row.
pub type RangePlace = Option<(TrackId, AutoTarget)>;

/// The transform frame round the range over one lane or over the tempo row. It stays while the
/// range is the one it was put on.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeFrame {
    pub place: RangePlace,
    pub from: Tick,
    pub to: Tick,
}

/// The frame on screen, with its two handles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameBox {
    pub frame: Rect,
    /// On the top edge: scales the values about the middle of the highest and the lowest.
    pub scale: Rect,
    /// On the top right corner: tilts the values toward the end of the range.
    pub tilt: Rect,
    /// Half the distance from the lowest value to the highest, in the unit of the place.
    pub half: f64,
}

/// The side of a handle.
const HANDLE: f32 = 8.0;
/// A handle takes the pointer this far outside its square.
const GRAB: f32 = 3.0;
/// A frame round values that are all the same is still this tall.
const FRAME_MIN_H: f32 = 8.0;
/// The tempo moves this many bpm a pixel, as a tempo point does (a tenth with Shift).
const BPM_PX: f64 = 0.5;
const BPM_PX_FINE: f64 = 0.1;

impl Studio {
    /// The top of the lane of `target` under `track` on screen, while the track shows it.
    pub(crate) fn lane_top_of(
        &self,
        g: &Geometry,
        track: TrackId,
        target: &AutoTarget,
    ) -> Option<f32> {
        let row = self.tree().row_of(RowKind::Track(track))?;
        let p = self.project();
        let t = p.tracks.get(&track)?;
        let i = self
            .arrange
            .lanes_of(t)
            .iter()
            .position(|l| l.target == *target)?;
        Some(self.lane_top(g, row, i))
    }

    /// What a press of `tool` at `(x, y)` over `under` takes of the range: a handle of its frame
    /// with any tool, or its upper half with the select and range tools (not on a point).
    pub(crate) fn range_spot(
        &self,
        g: &Geometry,
        (x, y): (f32, f32),
        tool: Tool,
        under: &Spot,
    ) -> Option<Spot> {
        if let Some(tilt) = self.frame_grip(g, x, y) {
            return Some(Spot::FrameGrip { tilt });
        }
        if !matches!(tool, Tool::Select | Tool::Range) {
            return None;
        }
        let (from, to) = self.arrange.range?;
        let tick = self.tick_at(g, x);
        if tick < from || tick > to {
            return None;
        }
        match under {
            Spot::Lane { track, target } | Spot::Line { track, target, .. } => {
                let top = self.lane_top_of(g, *track, target)?;
                (y < top + LANE_H / 2.0).then(|| Spot::RangeTrim(Some((*track, target.clone()))))
            }
            Spot::TempoGap => (y < g.tempo.y + g.tempo.h / 2.0).then_some(Spot::RangeTrim(None)),
            _ => None,
        }
    }

    /// The values of the stretch `[from, to]` of a place, in the unit a drag moves them by
    /// (parts of the lane's height, or bpm): the values at both ends and the points between.
    /// `how` moves them as the drag in progress would.
    pub(crate) fn stretch_values(
        &self,
        place: &RangePlace,
        (from, to): (Tick, Tick),
        how: Option<RangeMove>,
    ) -> Option<Vec<f64>> {
        let p = self.project();
        match place {
            Some((t, target)) => {
                let track = p.tracks.get(t)?;
                let lane = track.automation.iter().find(|l| l.target == *target)?;
                let moved = match how {
                    Some(how) => moved_lane(lane, from, to, how, target.stored_value(track))?,
                    None => lane.clone(),
                };
                let all = stretch(&moved, from, to)?;
                Some(all.iter().map(|x| target.to_norm(x.value)).collect())
            }
            None => {
                if p.tempo.is_empty() {
                    return None;
                }
                let tempo = match how {
                    Some(how) => moved_tempo(&p.tempo, from, to, how),
                    None => p.tempo.to_vec(),
                };
                let mut all = vec![bpm_at(&tempo, from)];
                all.extend(
                    tempo
                        .iter()
                        .filter(|t| t.tick > from && t.tick < to)
                        .map(|t| t.bpm),
                );
                all.push(bpm_at(&tempo, to));
                Some(all)
            }
        }
    }

    /// The frame and its handles on screen, as the drag in progress (`how`) would leave them.
    pub(crate) fn frame_box(&self, g: &Geometry, how: Option<RangeMove>) -> Option<FrameBox> {
        let f = self.arrange.frame.as_ref()?;
        if self.arrange.range != Some((f.from, f.to)) {
            return None;
        }
        let values = self.stretch_values(&f.place, (f.from, f.to), how)?;
        let lo = values.iter().copied().fold(f64::MAX, f64::min);
        let hi = values.iter().copied().fold(f64::MIN, f64::max);
        let (top, bottom) = match &f.place {
            Some((t, target)) => {
                let lane = self.lane_top_of(g, *t, target)?;
                let inner = LANE_H - Arrange::LANE_PAD * 2.0;
                let y =
                    |n: f64| lane + Arrange::LANE_PAD + (1.0 - n.clamp(0.0, 1.0) as f32) * inner;
                (y(hi), y(lo))
            }
            None => (
                self.tempo_xy(g, f.from, hi).1,
                self.tempo_xy(g, f.from, lo).1,
            ),
        };
        let (top, bottom) = if bottom - top < FRAME_MIN_H {
            let mid = (top + bottom) / 2.0;
            (mid - FRAME_MIN_H / 2.0, mid + FRAME_MIN_H / 2.0)
        } else {
            (top, bottom)
        };
        let (x0, x1) = (self.tick_x(g, f.from), self.tick_x(g, f.to));
        let handle = |x: f32| Rect::new(x - HANDLE / 2.0, top - HANDLE / 2.0, HANDLE, HANDLE);
        Some(FrameBox {
            frame: Rect::new(x0, top, x1 - x0, bottom - top),
            scale: handle((x0 + x1) / 2.0),
            tilt: handle(x1),
            half: (hi - lo) / 2.0,
        })
    }

    /// Which handle of the frame (true for the tilt one) is under the pointer.
    pub(crate) fn frame_grip(&self, g: &Geometry, x: f32, y: f32) -> Option<bool> {
        let b = self.frame_box(g, None)?;
        let grab = |r: Rect| {
            Rect::new(r.x - GRAB, r.y - GRAB, r.w + GRAB * 2.0, r.h + GRAB * 2.0).contains(x, y)
        };
        if grab(b.tilt) {
            Some(true)
        } else if grab(b.scale) {
            Some(false)
        } else {
            None
        }
    }

    /// How far a drag of `dy` pixels moves the values of a place, in its unit (Shift: finer).
    fn place_delta(place: &RangePlace, dy: f32, fine: bool) -> f64 {
        let dy = f64::from(-dy);
        match place {
            Some(_) => {
                let per = 1.0 / f64::from(LANE_H - Arrange::LANE_PAD * 2.0);
                let d = dy * per * if fine { 0.1 } else { 1.0 };
                (d * 1000.0).round() / 1000.0
            }
            None if fine => (dy * BPM_PX_FINE * 10.0).round() / 10.0,
            None => (dy * BPM_PX).round(),
        }
    }

    /// A step of a place's unit as the balloon shows it: "+12 %" of the lane, "+5 BPM".
    fn place_text(place: &RangePlace, d: f64, fine: bool) -> String {
        match place {
            Some(_) if fine => format!("{:+.1} %", d * 100.0),
            Some(_) => format!("{:+.0} %", d * 100.0),
            None => format!("{d:+} BPM"),
        }
    }

    /// The command's arguments for the stretch `[from, to]` of a place.
    fn place_args(place: &RangePlace, from: Tick, to: Tick, extra: Value) -> Value {
        let mut v = match place {
            Some((t, target)) => json!({ "track": t.to_string(), "target": target.to_string(),
                                         "from": from, "to": to }),
            None => json!({ "tempo": true, "from": from, "to": to }),
        };
        if let (Some(a), Some(b)) = (v.as_object_mut(), extra.as_object()) {
            a.extend(b.clone());
        }
        v
    }

    /// A vertical drag on the upper half of the range: every value moves by the same step.
    pub(super) fn plan_range_trim(&self, d: &Drag, place: &RangePlace) -> Option<Plan> {
        let (from, to) = self.arrange.range?;
        let delta = Self::place_delta(place, d.at.1 - d.from.1, d.mods.shift);
        if delta == 0.0 {
            return None;
        }
        Some(Plan {
            command: Some((
                "range.trim",
                Self::place_args(place, from, to, json!({ "delta": delta })),
            )),
            select: None,
            tip: Self::place_text(place, delta, d.mods.shift),
            preview: Preview::RangeMove {
                place: place.clone(),
                from,
                to,
                how: RangeMove::Trim(delta),
            },
        })
    }

    /// A drag on a handle of the frame: the top edge scales, the corner tilts.
    pub(super) fn plan_frame(&self, g: &Geometry, d: &Drag, tilt: bool) -> Option<Plan> {
        let f = self.arrange.frame.as_ref()?;
        let b = self.frame_box(g, None)?;
        let fine = d.mods.shift;
        let delta = Self::place_delta(&f.place, d.at.1 - d.from.1, fine);
        if delta == 0.0 {
            return None;
        }
        let (how, args, tip) = if tilt {
            (
                RangeMove::Transform {
                    scale: 1.0,
                    tilt: delta,
                },
                json!({ "tilt": delta }),
                format!("傾き {}", Self::place_text(&f.place, delta, fine)),
            )
        } else {
            // A stretch of one value still scales by the frame it is shown in.
            let least = if f.place.is_some() { 0.025 } else { 1.0 };
            let half = b.half.max(least);
            let scale = (((half + delta) / half) * 100.0).round() / 100.0;
            (
                RangeMove::Transform { scale, tilt: 0.0 },
                json!({ "scale": scale }),
                format!("縮尺 {}%", (scale * 100.0).round()),
            )
        };
        Some(Plan {
            command: Some((
                "range.transform",
                Self::place_args(&f.place, f.from, f.to, args),
            )),
            select: None,
            tip,
            preview: Preview::RangeMove {
                place: f.place.clone(),
                from: f.from,
                to: f.to,
                how,
            },
        })
    }

    /// Puts the frame round the range over a place. False without a range.
    pub(crate) fn open_range_frame(&mut self, place: RangePlace) -> bool {
        let Some((from, to)) = self.arrange.range else {
            return false;
        };
        self.arrange.frame = Some(RangeFrame { place, from, to });
        self.arrange.touched();
        true
    }

    /// `range.transform` without a scale or a tilt: the frame instead of a step. The place and
    /// the stretch come from the arguments (the range's menu), or, for Alt+T, from the range
    /// and the lane under the pointer, the lane whose points are selected, or the tempo row.
    pub(crate) fn range_frame_command(&mut self, g: &Geometry, args: &Map<String, Value>) -> bool {
        let tick = |k: &str| self.tick_arg(args.get(k));
        if let (Some(from), Some(to)) = (tick("from"), tick("to")) {
            if to <= from {
                return false;
            }
            self.arrange.range = Some((from, to));
            self.arrange.ranges = vec![(from, to)];
        }
        let named = if args.get("tempo").and_then(Value::as_bool) == Some(true) {
            Some(None)
        } else {
            let track = args
                .get("track")
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok());
            let target = args
                .get("target")
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok());
            track.zip(target).map(Some)
        };
        let place = match named {
            Some(place) => place,
            None => match &self.arrange.hover {
                Spot::Lane { track, target }
                | Spot::Point { track, target, .. }
                | Spot::Line { track, target, .. }
                | Spot::LaneHead { track, target, .. } => Some((*track, target.clone())),
                Spot::RangeTrim(place) => place.clone(),
                Spot::Tempo(_) | Spot::TempoGap => None,
                _ => match self.arrange.points.lane() {
                    Some(lane) => Some(lane.clone()),
                    None => return false,
                },
            },
        };
        // A lane without a line has nothing to frame.
        if !self.open_range_frame(place) || self.frame_box(g, None).is_none() {
            self.arrange.frame = None;
            return false;
        }
        true
    }
}
