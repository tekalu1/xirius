//! Alt on an audio clip's right edge changes its speed (EDT-038): the drag stretches the sound
//! to the new end, the clip's tempo stays, and the speed stops at 50 and 200%. While Alt is held
//! over the edge, the edge carries the stretch mark; while it is dragged, the clip is drawn at
//! the new speed, so the waveform stretches under the pointer.

use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::{AudioProcess, Clip, ClipBody};
use xirius_core::tempo::TempoMap;
use xirius_design::Radius;

use super::{Drag, Grip, Plan, Preview, Spot};
use crate::draw::Pen;
use crate::studio::{Geometry, Rect, Studio};
use crate::tools::Tool;

/// The stretch mark on the edge.
const MARK: f32 = 16.0;

/// The speed of an audio clip; None for MIDI.
fn speed_of(clip: &Clip) -> Option<f64> {
    match &clip.body {
        ClipBody::Audio { process, .. } => Some(process.speed),
        ClipBody::Midi { .. } => None,
    }
}

/// `clip` played at `percent`, as `clip.speed.set` leaves it: the same part of the source, its
/// length and offset scaled by the change.
pub fn sped(map: &TempoMap, clip: &Clip, percent: f64) -> Clip {
    let mut c = clip.clone();
    let ClipBody::Audio {
        process, offset, ..
    } = &mut c.body
    else {
        return c;
    };
    let scale = process.speed / percent;
    process.speed = percent;
    let base = map.seconds_at(c.start);
    let off = (map.seconds_at(c.start + *offset) - base) * scale;
    *offset = (map.tick_at(base + off) - c.start).max(0);
    let len = (map.seconds_at(c.start + c.length) - base) * scale;
    c.length = (map.tick_at(base + len) - c.start).max(1);
    c
}

impl Studio {
    /// The right edge of the audio clip `clip` dragged with Alt: the speed that brings its end
    /// under the pointer, for it and the other audio clips moving with it.
    pub(super) fn plan_speed(&self, d: &Drag, t: TrackId, clip: &Clip) -> Option<Plan> {
        let p = self.project();
        let speed = speed_of(clip)?;
        let map = TempoMap::from_project(&p);
        let dt = self.px_to_ticks(d.at.0 - d.from.0);
        let end = self
            .snap_tick(clip.start + clip.length + dt, d.mods.shift)
            .max(clip.start + 1);
        let base = map.seconds_at(clip.start);
        let was = map.seconds_at(clip.start + clip.length) - base;
        let want = map.seconds_at(end) - base;
        if was <= 0.0 || want <= 0.0 {
            return None;
        }
        let percent = (speed * was / want).clamp(AudioProcess::MIN_SPEED, AudioProcess::MAX_SPEED);
        if (percent - speed).abs() < 1e-9 {
            return None;
        }
        let sel: Vec<(TrackId, &Clip)> = self
            .moving_clips(t, clip.id)
            .into_iter()
            .filter_map(|(tid, cid)| {
                let c = p.tracks.get(&tid)?.clips.iter().find(|c| c.id == cid)?;
                speed_of(c).map(|_| (tid, c))
            })
            .collect();
        let ids: Vec<String> = sel.iter().map(|(_, c)| c.id.to_string()).collect();
        let length = sped(&map, clip, percent).length;
        Some(Plan {
            command: Some((
                "clip.speed.set",
                serde_json::json!({ "ids": ids, "percent": percent }),
            )),
            select: None,
            tip: format!(
                "長さ {:.2} 小節　速さ {:.0}%",
                length as f64 / self.bar_ticks() as f64,
                percent
            ),
            preview: Preview::Speed {
                percent,
                clips: sel.iter().map(|(_, c)| c.id).collect(),
            },
        })
    }

    /// The clips an Alt drag on an edge is stretching, at their new speed and length.
    pub fn speed_preview(&self, g: &Geometry) -> Option<(f64, Vec<ClipId>)> {
        let d = self.arrange.drag.as_ref().filter(|d| d.started)?;
        if !matches!(
            d.spot,
            Spot::Clip {
                grip: Grip::Right,
                ..
            }
        ) {
            return None;
        }
        let plan = self.plan(g, d)?;
        match plan.preview {
            Preview::Speed { percent, clips } => Some((percent, clips)),
            _ => None,
        }
    }

    /// The audio clip whose right edge Alt would stretch: the one under the pointer, or the one
    /// being stretched.
    pub(crate) fn speed_edge(&self) -> Option<(TrackId, ClipId)> {
        let (spot, mods, tool) = match &self.arrange.drag {
            Some(d) => (&d.spot, d.mods, d.tool),
            None => {
                let tool =
                    self.holds()
                        .tool(crate::tools::Area::Arrange, self.tool, self.arrange.mods);
                let mods =
                    self.mouse_modifiers(&self.arrange.hover, tool, self.arrange.mods, false);
                (&self.arrange.hover, mods, tool)
            }
        };
        let Spot::Clip {
            track,
            clip,
            grip: Grip::Right,
        } = *spot
        else {
            return None;
        };
        if tool != Tool::Select || !mods.alt || mods.ctrl {
            return None;
        }
        let p = self.project();
        p.clip(clip)
            .ok()
            .and_then(|(_, c)| speed_of(c))
            .map(|_| (track, clip))
    }
}

/// The stretch mark on the right edge of `r`, the rect of the clip `clip`, while Alt is held
/// over it.
pub fn paint_mark(pen: &mut Pen, studio: &Studio, clip: ClipId, r: Rect) {
    if studio.speed_edge().map(|(_, c)| c) != Some(clip) {
        return;
    }
    let s = MARK.min(r.h - 4.0).max(8.0);
    let m = Rect::new(r.right() - s - 2.0, r.y + (r.h - s) / 2.0, s, s);
    pen.round(m, pen.c.fill, Radius::Xs);
    let i = s - 4.0;
    pen.icon(
        "stretch",
        Rect::new(m.x + 2.0, m.y + 2.0, i, i),
        pen.c.on_fill,
    );
}
