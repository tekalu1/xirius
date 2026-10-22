//! The pencil's stroke in an automation lane and in the tempo row (UI-128): a drag lays the
//! shape the top bar chose (`auto.draw`, `tempo.draw`), a click lays one point.
//!
//! The shape reads the stroke like this. Freehand keeps the path. A line, a parabola or a wave
//! runs from the press to the release on the grid, and the heights of the two ends are the
//! values at its ends (a wave is centred on the press and as high as the stroke strayed from
//! it). With Alt, a wave takes the whole stroke for one period and every other shape becomes a
//! straight line of two points.
//!
//! A wave also reads Mod while it is drawn (AUT-010): what the pointer moves with Mod held
//! shifts the wave's phase and leaves its ends where they are, and with Mod+Alt it moves the
//! whole stroke in time. Mod at the press still puts the pencil away for the arrow.

use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::id::TrackId;
use xirius_core::shape::{Dot, Shape, Stroke, generate};
use xirius_core::time::Tick;

use super::{Arrange, Drag, Plan, Preview, Spot, lane_value, lane_value_y};
use crate::studio::{Geometry, Studio};
use crate::tools::Tool;
use crate::tree::{LANE_H, RowKind};

/// Where a stroke is drawn.
enum Place {
    Lane {
        track: TrackId,
        target: AutoTarget,
        top: f32,
    },
    Tempo,
}

impl Place {
    fn range(&self) -> (f64, f64) {
        match self {
            Place::Lane { target, .. } => target.range(),
            Place::Tempo => (60.0, 200.0),
        }
    }
}

impl Studio {
    /// The top of the lane `target` of `track` shows, in window y.
    fn lane_slot(&self, g: &Geometry, track: TrackId, target: &AutoTarget) -> Option<f32> {
        let row = self.tree().row_of(RowKind::Track(track)).copied()?;
        let p = self.project();
        let i = p
            .tracks
            .get(&track)
            .map(|t| self.arrange.lanes_of(t))
            .and_then(|l| l.iter().position(|l| l.target == *target))
            .unwrap_or(0);
        Some(self.lane_top(g, &row, i))
    }

    fn stroke_place(&self, g: &Geometry, spot: &Spot) -> Option<Place> {
        match spot {
            Spot::Lane { track, target } => Some(Place::Lane {
                track: *track,
                target: target.clone(),
                top: self.lane_slot(g, *track, target)?,
            }),
            Spot::TempoGap => Some(Place::Tempo),
            _ => None,
        }
    }

    /// The value a window y stands for in `place`.
    fn stroke_value(&self, g: &Geometry, place: &Place, y: f32) -> f64 {
        match place {
            Place::Lane { target, top, .. } => {
                let n = 1.0
                    - ((y - top - Arrange::LANE_PAD) / (LANE_H - Arrange::LANE_PAD * 2.0))
                        .clamp(0.0, 1.0);
                lane_value(target, f64::from(n))
            }
            Place::Tempo => self.tempo_at_y(g, y),
        }
    }

    /// The window point of a value at a tick in `place`.
    fn stroke_xy(&self, g: &Geometry, place: &Place, tick: Tick, value: f64) -> (f32, f32) {
        match place {
            Place::Lane { target, top, .. } => {
                (self.tick_x(g, tick), top + lane_value_y(target, value))
            }
            Place::Tempo => self.tempo_xy(g, tick, value),
        }
    }

    /// What letting go of a pencil drag in a lane or the tempo row would draw.
    pub(super) fn plan_stroke(&self, g: &Geometry, d: &Drag) -> Option<Plan> {
        let place = self.stroke_place(g, &d.spot)?;
        let shape = self.song.session().state.view.draw_shape;
        if shape == Shape::Transform || self.arrange.stroke.len() < 2 {
            return None;
        }
        let (lo, hi) = place.range();
        let pts: Vec<(Tick, f64)> = self
            .arrange
            .stroke
            .iter()
            .map(|&(x, y, _)| (self.tick_at(g, x).max(0), self.stroke_value(g, &place, y)))
            .collect();
        let alt = d.mods.alt;
        let wave = shape.is_wave();
        let shape = if alt && !wave && shape != Shape::Line {
            Shape::Line
        } else {
            shape
        };
        let mut args = serde_json::Map::new();
        args.insert("shape".into(), json!(shape.name()));
        let path: Vec<(Tick, f64, bool)> = if shape == Shape::Free {
            let from = pts.iter().map(|p| p.0).min()?;
            let to = pts.iter().map(|p| p.0).max()?;
            if to <= from {
                return None;
            }
            args.insert(
                "points".into(),
                Value::Array(
                    pts.iter()
                        .map(|(t, v)| json!({ "tick": t, "value": v }))
                        .collect(),
                ),
            );
            args.insert("from".into(), json!(from));
            args.insert("to".into(), json!(to));
            pts.iter().map(|&(t, v)| (t, v, false)).collect()
        } else {
            let (first, last) = (pts[0], pts[pts.len() - 1]);
            let (start, end, phase) = if wave {
                self.wave_walk(g)
            } else {
                (first.0, last.0, 0)
            };
            let a = self.snap_tick(start, d.mods.shift).max(0);
            let b = self.snap_tick(end, d.mods.shift).max(0);
            let ((from, v0), (to, v1)) = if a <= b {
                ((a, first.1), (b, last.1))
            } else {
                ((b, last.1), (a, first.1))
            };
            if to <= from {
                return None;
            }
            let ppq = f64::from(self.project().ppq);
            let (v0, v1, amp, period) = if wave {
                let centre = first.1;
                let strayed = pts
                    .iter()
                    .map(|p| (p.1 - centre).abs())
                    .fold(0.0_f64, f64::max);
                let amp = if strayed > (hi - lo) * 0.02 {
                    strayed
                } else {
                    (hi - lo) / 4.0
                };
                let period = if alt && !d.mods.ctrl {
                    (to - from) as f64
                } else {
                    ppq
                };
                (centre, centre, amp, period)
            } else {
                (v0, v1, (hi - lo) / 4.0, ppq)
            };
            args.insert("from".into(), json!(from));
            args.insert("to".into(), json!(to));
            args.insert("value".into(), json!(v0));
            args.insert("end-value".into(), json!(v1));
            let phase = phase.rem_euclid((period.round() as Tick).max(1));
            if wave {
                args.insert("amp".into(), json!(amp));
                args.insert("period".into(), json!(period.round() as i64));
                args.insert("phase".into(), json!(phase));
            }
            let s = Stroke {
                from,
                to,
                v0,
                v1,
                amp,
                period,
                phase: phase as f64,
            };
            generate(shape, &s)
                .into_iter()
                .map(
                    |Dot {
                         tick, value, step, ..
                     }| (tick, value, step),
                )
                .collect()
        };
        let (command, tip) = match &place {
            Place::Lane { track, target, .. } => {
                args.insert("track".into(), json!(track.to_string()));
                args.insert("target".into(), json!(target.to_string()));
                ("auto.draw", shape.label().to_string())
            }
            Place::Tempo => ("tempo.draw", shape.label().to_string()),
        };
        let mut line: Vec<(f32, f32)> = Vec::new();
        for (i, (tick, value, _)) in path.iter().enumerate() {
            let (x, y) = self.stroke_xy(g, &place, *tick, *value);
            if let (Some(&(_, py)), true) = (line.last(), i > 0 && path[i - 1].2) {
                line.push((x, py));
            }
            line.push((x, y));
        }
        Some(Plan {
            command: Some((command, Value::Object(args))),
            select: None,
            tip,
            preview: Preview::Stroke(line),
        })
    }

    /// Where a wave's stroke starts and ends and how far its phase moved, before the grid: a step
    /// of the pointer moves the end, with Mod the phase, with Mod+Alt the start and the end.
    fn wave_walk(&self, g: &Geometry) -> (Tick, Tick, Tick) {
        let mut steps = self.arrange.stroke.iter();
        let Some(&(x, _, _)) = steps.next() else {
            return (0, 0, 0);
        };
        let mut at = self.tick_at(g, x);
        let (mut start, mut end, mut phase) = (at, at, 0);
        for &(x, _, mods) in steps {
            let t = self.tick_at(g, x);
            let dt = t - at;
            at = t;
            match (mods.ctrl, mods.alt) {
                (false, _) => end += dt,
                (true, false) => phase += dt,
                (true, true) => {
                    start += dt;
                    end += dt;
                }
            }
        }
        (start, end, phase)
    }

    /// A click with the pencil in a lane or the tempo row lays one point.
    pub(super) fn stroke_click(&mut self, g: &Geometry, d: &Drag) -> bool {
        if d.tool != Tool::Draw {
            return false;
        }
        let Some(place) = self.stroke_place(g, &d.spot) else {
            return false;
        };
        let at = self.snap_tick(d.tick, d.mods.shift).max(0);
        let value = self.stroke_value(g, &place, d.from.1);
        match place {
            Place::Lane { track, target, .. } => {
                let added = self.arrange_run(
                    "auto.point.add",
                    json!({ "track": track.to_string(), "target": target.to_string(),
                            "at": at, "value": value }),
                );
                if added.is_some() {
                    self.point_drawn(track, &target, at);
                }
            }
            Place::Tempo => {
                self.arrange_run("tempo.set", json!({ "at": at, "bpm": value }));
            }
        }
        true
    }
}
