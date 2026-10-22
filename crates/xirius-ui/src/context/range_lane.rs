//! The menu of a range over one automation lane or over the tempo row (UI-241, AUT-010): the
//! right press inside the range the range tool chose, on a lane or on the tempo row.

use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::id::TrackId;

use super::{Act, Item};
use crate::studio::Studio;

/// The size of one "up" or "down" step of a lane, as a part of its height.
const LANE_STEP: f64 = 0.1;

/// The size of one "up" or "down" step of the tempo, in bpm.
const TEMPO_STEP: f64 = 5.0;

impl Studio {
    /// The range over the lane `target` of `track`.
    pub(crate) fn lane_range_context(
        &mut self,
        track: TrackId,
        target: AutoTarget,
        (x, y): (f32, f32),
    ) -> bool {
        let Some((from, to)) = self.arrange.range else {
            return false;
        };
        let place = json!({ "track": track.to_string(), "target": target.to_string() });
        let inclusive = with(&place, json!({ "from": from, "to": to - 1 }));
        let mut items = vec![
            self.ctx_cmd("edit.copy", inclusive.clone()),
            self.ctx_cmd("edit.cut", inclusive.clone()),
            self.ctx_cmd("edit.paste", with(&place, json!({ "at": from }))),
            self.ctx_cmd("edit.delete", inclusive),
            Item::gap(),
        ];
        let frame = Act::RangeFrame(Some((track, target)), from, to);
        items.extend(self.range_shapes(&with(&place, json!({ "from": from, "to": to })), frame));
        items.push(Item::gap());
        items.extend(self.range_time(from, to));
        self.open_context(items, x, y);
        true
    }

    /// The range over the tempo row.
    pub(crate) fn tempo_range_context(&mut self, (x, y): (f32, f32)) -> bool {
        let Some((from, to)) = self.arrange.range else {
            return false;
        };
        let place = json!({ "tempo": true, "from": from, "to": to });
        let mut items = self.range_shapes(&place, Act::RangeFrame(None, from, to));
        items.push(Item::gap());
        items.extend(self.range_time(from, to));
        self.open_context(items, x, y);
        true
    }

    /// 直線・カーブ・平ら, 上げる・下げる and トランスフォーム of the stretch `place` names.
    fn range_shapes(&mut self, place: &Value, frame: Act) -> Vec<Item> {
        let tempo = matches!(frame, Act::RangeFrame(None, ..));
        let (step, tilt, unit) = if tempo {
            (TEMPO_STEP, TEMPO_STEP * 2.0, "bpm")
        } else {
            (LANE_STEP, LANE_STEP * 2.0, "%")
        };
        let shown = |v: f64| if tempo { v } else { v * 100.0 };
        let mut items = vec![self.ctx_as(
            "範囲を直線にする",
            "range.shape",
            with(place, json!({ "shape": "line" })),
        )];
        if !tempo {
            items.push(self.ctx_as(
                "範囲をカーブにする",
                "range.shape",
                with(place, json!({ "shape": "curve" })),
            ));
        }
        items.push(self.ctx_as(
            "範囲を平らにする",
            "range.shape",
            with(place, json!({ "shape": "flat" })),
        ));
        items.push(Item::gap());
        items.push(self.ctx_as(
            &format!("範囲を上げる（+{} {unit}）", shown(step)),
            "range.trim",
            with(place, json!({ "delta": step })),
        ));
        items.push(self.ctx_as(
            &format!("範囲を下げる（-{} {unit}）", shown(step)),
            "range.trim",
            with(place, json!({ "delta": -step })),
        ));
        let transform = |s: &mut Studio, label: String, args: Value| {
            s.ctx_as(&label, "range.transform", with(place, args))
        };
        let rows = vec![
            Item::act("自由変形", &self.menu_key("range.transform"), frame),
            Item::gap(),
            transform(self, "縮小 50%".into(), json!({ "scale": 0.5 })),
            transform(self, "拡大 150%".into(), json!({ "scale": 1.5 })),
            transform(self, "上下を反転".into(), json!({ "scale": -1.0 })),
            transform(
                self,
                format!("右上がりに傾ける（+{} {unit}）", shown(tilt)),
                json!({ "tilt": tilt }),
            ),
            transform(
                self,
                format!("右下がりに傾ける（-{} {unit}）", shown(tilt)),
                json!({ "tilt": -tilt }),
            ),
        ];
        items.push(Item::sub("範囲をトランスフォーム", rows));
        items
    }

    /// 時間を挿入・削除 (the commands of the whole song).
    fn range_time(&mut self, from: i64, to: i64) -> Vec<Item> {
        let span = json!({ "start": from, "end": to });
        vec![
            self.ctx_cmd("range.time.insert", span.clone()),
            self.ctx_cmd("range.time.delete", span),
        ]
    }
}

/// `base` with the members of `extra` added.
fn with(base: &Value, extra: Value) -> Value {
    let mut v = base.clone();
    if let (Some(a), Some(b)) = (v.as_object_mut(), extra.as_object()) {
        a.extend(b.clone());
    }
    v
}
