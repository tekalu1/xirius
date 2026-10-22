//! The menus of the automation lanes (group C of the matrix): a point, a line or the empty
//! part of a lane, and the head of a lane.

use serde_json::json;
use xirius_core::auto::AutoTarget;
use xirius_core::id::TrackId;

use super::{Act, Item};
use crate::arrange::{Arrange, LaneButton, lane_value};
use crate::autovalue::CURVE;
use crate::studio::{Geometry, Studio};
use crate::tree::{LANE_H, RowKind};

impl Studio {
    /// S17: a point. A press on a point that is not selected selects it alone; the rows then
    /// act on every selected point of the lane.
    pub(crate) fn auto_point_context(
        &mut self,
        g: &Geometry,
        track: TrackId,
        target: AutoTarget,
        index: usize,
        (x, y): (f32, f32),
    ) -> bool {
        let p = self.project();
        let Some((_, l)) = p.tracks.get(&track).and_then(|t| {
            t.automation
                .iter()
                .enumerate()
                .find(|(_, l)| l.target == target)
        }) else {
            return false;
        };
        let Some(pt) = l.points.get(index).copied() else {
            return false;
        };
        if !self.arrange.points.has(track, &target, pt.tick) {
            // Not selected: the point is the whole selection, clips and all.
            self.arrange.points.select(track, &target, pt.tick, false);
            self.arrange.clips.clear();
            self.arrange.touched();
        }
        self.focus_arrange(true);
        if self.clips_and_points_selected() {
            let delete = self.mixed_delete();
            let items = self.mixed_items(g, delete);
            self.open_context(items, x, y);
            return true;
        }
        let ticks = self
            .arrange
            .points
            .ticks()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let args = |curve: f64, step: bool| {
            json!({ "track": track.to_string(), "target": target.to_string(),
                    "ticks": ticks, "curve": curve, "step": step })
        };
        let line = !target.stepped();
        let shapes = vec![
            self.ctx_as("直線", "auto.point.shape", args(0.0, false))
                .on(line && !pt.step && pt.curve == 0.0),
            self.ctx_as("曲線", "auto.point.shape", args(CURVE, false))
                .on(line && !pt.step && pt.curve != 0.0),
            self.ctx_as("階段", "auto.point.shape", args(0.0, true))
                .on(line && pt.step),
        ];
        let shape = Item::sub("次のポイントまでの形", shapes.clone())
            .enabled(line && shapes.iter().any(|r| r.enabled));
        let items = vec![
            Item::act(
                "値を入力…",
                "",
                Act::PointValue(track, target.clone(), pt.tick, x, y),
            ),
            shape,
            Item::gap(),
            self.ctx_key(g, "edit.cut"),
            self.ctx_key(g, "edit.copy"),
            Item::gap(),
            Item::act("削除", &self.menu_key("clip.delete"), Act::DeletePoints),
        ];
        self.open_context(items, x, y);
        true
    }

    /// S18: the line from point `line` to the next, or an empty part of a lane.
    pub(crate) fn auto_lane_context(
        &mut self,
        g: &Geometry,
        track: TrackId,
        target: AutoTarget,
        line: Option<usize>,
        (x, y): (f32, f32),
    ) -> bool {
        let p = self.project();
        let Some(t) = p.tracks.get(&track) else {
            return false;
        };
        let Some(l) = t.automation.iter().find(|l| l.target == target) else {
            return false;
        };
        let at = self.ctx_tick(g, x);
        let value = match line {
            Some(_) => l.curve_at(at as f64),
            None => None,
        }
        .or_else(|| self.lane_value_at_y(g, track, &target, y));
        let Some(value) = value else {
            return false;
        };
        let (tid, tgt) = (track.to_string(), target.to_string());
        let add = self.ctx_as(
            "ここにポイントを追加",
            "auto.point.add",
            json!({ "track": tid, "target": tgt, "at": at, "value": value }),
        );
        let seg = line.and_then(|i| l.points.get(i).copied().map(|pt| (i, pt)));
        let shape = match seg {
            Some((index, pt)) => {
                let args = |curve: f64, step: bool| {
                    json!({ "track": tid, "target": tgt, "index": index,
                            "curve": curve, "step": step })
                };
                Item::sub(
                    "この線の形",
                    vec![
                        self.ctx_as("直線", "auto.segment.shape", args(0.0, false))
                            .on(!pt.step && pt.curve == 0.0),
                        self.ctx_as("曲線", "auto.segment.shape", args(CURVE, false))
                            .on(!pt.step && pt.curve != 0.0),
                        self.ctx_as("階段", "auto.segment.shape", args(0.0, true))
                            .on(pt.step),
                    ],
                )
            }
            None => Item::sub("この線の形", Vec::new()).enabled(false),
        };
        let paste = self.ctx_cmd(
            "edit.paste",
            json!({ "at": at, "track": tid, "target": tgt }),
        );
        let in_loop = self.ctx_as(
            "ループの中のポイントを削除",
            "auto.clear",
            json!({ "track": tid, "target": tgt, "range": "loop" }),
        );
        let all = self.ctx_as(
            "すべてのポイントを削除",
            "auto.clear",
            json!({ "track": tid, "target": tgt }),
        );
        self.open_context(vec![add, shape, paste, Item::gap(), in_loop, all], x, y);
        true
    }

    /// S19: the head of a lane.
    pub(crate) fn auto_head_context(
        &mut self,
        track: TrackId,
        target: AutoTarget,
        (x, y): (f32, f32),
    ) -> bool {
        let p = self.project();
        let add = Item::sub("レーンを追加", self.lane_rows(&p, track));
        let hide = Item::act("このレーンを隠す", "", Act::HideLane(track, target.clone()));
        let clear = self.ctx_as(
            "すべてのポイントを削除",
            "auto.clear",
            json!({ "track": track.to_string(), "target": target.to_string() }),
        );
        self.open_context(vec![add, hide, Item::gap(), clear], x, y);
        true
    }

    /// 隠す: hides a lane (the button at the lane's head does the same).
    pub(super) fn hide_lane(&mut self, g: &Geometry, t: TrackId, target: &AutoTarget) -> bool {
        self.lane_button(g, t, target, LaneButton::Remove);
        true
    }

    /// The lane head under a window point of the track column.
    pub(crate) fn lane_head_at(
        &self,
        g: &Geometry,
        x: f32,
        y: f32,
    ) -> Option<(TrackId, AutoTarget)> {
        if !g.column.contains(x, y) {
            return None;
        }
        let row = *self.tree().row_at(y - g.column.y + self.scroll.1)?;
        let RowKind::Track(t) = row.kind else {
            return None;
        };
        let below = y - self.row_y(g, &row) - row.h;
        if below < 0.0 {
            return None;
        }
        let p = self.project();
        let lanes = self.arrange.lanes_of(p.tracks.get(&t)?);
        lanes
            .get((below / LANE_H) as usize)
            .map(|l| (t, l.target.clone()))
    }

    /// The value of a lane at a window y.
    fn lane_value_at_y(
        &self,
        g: &Geometry,
        track: TrackId,
        target: &AutoTarget,
        y: f32,
    ) -> Option<f64> {
        let row = *self.tree().row_of(RowKind::Track(track))?;
        let p = self.project();
        let i = self
            .arrange
            .lanes_of(p.tracks.get(&track)?)
            .iter()
            .position(|l| l.target == *target)?;
        let n = 1.0
            - ((y - self.lane_top(g, &row, i) - Arrange::LANE_PAD)
                / (LANE_H - Arrange::LANE_PAD * 2.0))
                .clamp(0.0, 1.0);
        Some(lane_value(target, f64::from(n)))
    }
}
