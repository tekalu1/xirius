//! The menus of the piano roll (groups D and E of the matrix): a note, the empty part of the
//! grid, a key of the piano, a velocity bar, a controller point, the empty part of a
//! controller lane and a lane's tab.

use serde_json::{Value, json};
use xirius_core::clipboard::Clipboard;
use xirius_core::model::{CtrlKind, CtrlShape, Note};

use super::{Act, Item, Run};
use crate::roll::menu::Spot;
use crate::roll::{Ctx, LaneKind, ids_json};
use crate::studio::{Geometry, Studio};

impl Studio {
    /// The right press in the roll: opens the menu of what is under it. False where the roll
    /// has no menu, or when no roll is open.
    pub(crate) fn roll_context(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(ctx) = self.roll_input(g) else {
            return false;
        };
        if !ctx.body.contains(x, y) {
            return false;
        }
        let Some(spot) = self.roll.context_spot(&ctx, x, y) else {
            return false;
        };
        // The keys the rows run are the roll's own.
        self.focus_arrange(false);
        let items = match spot {
            Spot::Note | Spot::Point if self.roll.mixed(&ctx) => {
                let delete = self.ctx_key(g, "note.delete");
                self.mixed_items(g, delete)
            }
            Spot::Note => self.note_items(g, &ctx),
            Spot::Grid => self.roll_empty_items(g),
            Spot::Keys(pitch) => self.keys_items(&ctx, pitch),
            Spot::Bar => self.bar_items(&ctx),
            Spot::Point => self.point_items(g, &ctx),
            Spot::Lane { tick, value } => self.ctrl_lane_items(g, &ctx, tick, value),
            Spot::Tab(index) => self.tab_items(&ctx, index),
        };
        self.open_context(items, x, y);
        true
    }

    /// S20: the selected notes (one or many: the same rows).
    fn note_items(&mut self, g: &Geometry, ctx: &Ctx) -> Vec<Item> {
        let notes: Vec<Note> = self.roll.selected_notes(ctx).into_iter().cloned().collect();
        let ids = ids_json(notes.iter().map(|n| &n.id));
        let values = notes
            .first()
            .map(|n| {
                [
                    ("位置…", "start", n.start),
                    ("長さ…", "length", n.length),
                    ("ピッチ…", "pitch", i64::from(n.pitch)),
                    ("ベロシティ…", "velocity", i64::from(n.velocity)),
                ]
                .into_iter()
                .map(|(label, field, value)| {
                    Item::ask_number(
                        label,
                        "note.value.set",
                        json!({"ids": ids, "field": field}),
                        "value",
                        &value.to_string(),
                    )
                })
                .collect()
            })
            .unwrap_or_default();

        let now = Item {
            label: "今のグリッド".to_string(),
            ..self.ctx_key(g, "note.quantize")
        };
        let half = Item {
            key: self.menu_key("note.quantize --strength 50"),
            ..self.ctx_as(
                "50%",
                "note.quantize",
                self.quant_args(json!({ "ids": ids, "strength": 50 })),
            )
        };

        let step = |s: &mut Studio, label: &str, by: i64| Item {
            key: s.menu_key(&format!("note.transpose --by {by}")),
            ..s.ctx_as(label, "note.transpose", json!({ "ids": ids, "by": by }))
        };
        let transpose = vec![
            step(self, "+1", 1),
            step(self, "−1", -1),
            step(self, "+12", 12),
            step(self, "−12", -12),
        ];

        let first = notes.first().map_or(100, |n| n.velocity);
        let mean = if notes.is_empty() {
            100
        } else {
            let sum: i64 = notes.iter().map(|n| i64::from(n.velocity)).sum();
            (sum as f64 / notes.len() as f64).round() as i64
        };
        let by = |s: &mut Studio, label: &str, by: i64| {
            s.ctx_as(label, "note.velocity.set", json!({ "ids": ids, "by": by }))
        };
        let velocity = vec![
            Item::ask_number(
                "値を入力…",
                "note.velocity.set",
                json!({ "ids": ids }),
                "value",
                &first.to_string(),
            ),
            by(self, "強く", 10),
            by(self, "弱く", -10),
            self.ctx_as(
                "揃える",
                "note.velocity.set",
                json!({ "ids": ids, "value": mean }),
            ),
        ];

        let lengths = |s: &mut Studio, label: &str, f: &dyn Fn(&Note) -> i64| {
            let list: Vec<_> = notes
                .iter()
                .map(|n| json!({ "id": n.id.to_string(), "length": f(n) }))
                .collect();
            s.ctx_as(label, "note.set", json!({ "notes": list }))
        };
        let end = ctx.clip_len();
        let all = ctx.notes();
        let to_next = |n: &Note| {
            let next = all
                .iter()
                .map(|o| o.start)
                .filter(|&t| t > n.start)
                .min()
                .unwrap_or(end);
            (next - n.start).max(1)
        };
        let length = vec![
            lengths(self, "次のノートまで", &to_next),
            self.ctx_as(
                "グリッド 1 つ分",
                "note.resize",
                json!({ "ids": ids, "length": self.song.grid_for("roll") }),
            ),
            lengths(self, "2 倍", &|n| {
                (n.length * 2).min((end - n.start).max(1))
            }),
            lengths(self, "半分", &|n| (n.length / 2).max(1)),
        ];

        vec![
            Item::sub("クオンタイズ", vec![now, half]),
            Item::sub("トランスポーズ", transpose),
            Item::sub("ベロシティ", velocity),
            Item::sub("長さ", length),
            self.ctx_key(g, "note.duplicate"),
            self.ctx_key(g, "note.mute"),
            Item::sub("値を入力", values),
            self.tool_item("tool.erase"),
            Item::gap(),
            self.ctx_key(g, "edit.cut"),
            self.ctx_key(g, "edit.copy"),
            Item::gap(),
            self.ctx_key(g, "note.delete"),
        ]
    }

    /// S21: the empty part of the grid.
    fn roll_empty_items(&mut self, g: &Geometry) -> Vec<Item> {
        let paste = Item {
            enabled: !self.roll.clipboard.is_empty(),
            ..self.ctx_key(g, "edit.paste")
        };
        let now = self.song.grid_for("roll").to_string();
        let triplet = now.ends_with('t');
        let base = now.trim_end_matches('t').to_string();
        let mut grids: Vec<Item> = ["1/4", "1/8", "1/16", "1/32"]
            .into_iter()
            .map(|t| {
                // A view setting changes no song step, so the dry run never lights it.
                self.ctx_as(t, "view.grid", json!({ "grid": t, "editor": "roll" }))
                    .enabled(true)
                    .on(now == t)
            })
            .collect();
        let triple = if triplet { base } else { format!("{now}t") };
        grids.push(
            self.ctx_as(
                "3 連",
                "view.grid",
                json!({ "grid": triple, "editor": "roll" }),
            )
            .enabled(true)
            .on(triplet),
        );
        vec![
            paste,
            self.ctx_key(g, "select.all"),
            self.ctx_key(g, "select.invert"),
            Item::sub("グリッド", grids),
            self.tool_item("tool.erase"),
        ]
    }

    /// S22: a key of the piano, or a row's name in the drum view. 鍵盤の表示 is the switch of
    /// the drawer's bar (CTX-058).
    fn keys_items(&mut self, ctx: &Ctx, pitch: u8) -> Vec<Item> {
        let any = ctx.notes().iter().any(|n| n.pitch == pitch);
        let clip = ctx.clip.id.to_string();
        let drums = ctx.drum.is_some();
        let view = vec![
            self.ctx_setting("音名", "roll.view", json!({ "clip": clip, "mode": "keys" }))
                .on(!drums),
            self.ctx_setting(
                "ドラムの名前",
                "roll.view",
                json!({ "clip": clip, "mode": "drums" }),
            )
            .on(drums),
        ];
        let mut items = vec![
            Item::act("同じ高さのノートを選択", "", Act::RollPitch(pitch)).enabled(any),
            Item::sub("鍵盤の表示", view),
        ];
        items.extend(self.drum_name_items(ctx, pitch));
        items
    }

    /// S23: the velocity bars of the selected notes.
    fn bar_items(&mut self, ctx: &Ctx) -> Vec<Item> {
        let mut notes: Vec<Note> = self.roll.selected_notes(ctx).into_iter().cloned().collect();
        notes.sort_by_key(|n| n.start);
        let ids = ids_json(notes.iter().map(|n| &n.id));
        let first = notes.first().map_or(100, |n| n.velocity);
        let mean = if notes.is_empty() {
            100
        } else {
            let sum: i64 = notes.iter().map(|n| i64::from(n.velocity)).sum();
            (sum as f64 / notes.len() as f64).round() as i64
        };
        let (t0, t1) = (
            notes.first().map_or(0, |n| n.start),
            notes.last().map_or(0, |n| n.start),
        );
        let (v0, v1) = (
            f64::from(first),
            f64::from(notes.last().map_or(first, |n| n.velocity)),
        );
        let slope: Vec<Value> = notes
            .iter()
            .map(|n| {
                let v = if t1 > t0 {
                    v0 + (v1 - v0) * (n.start - t0) as f64 / (t1 - t0) as f64
                } else {
                    f64::from(n.velocity)
                };
                json!({ "id": n.id.to_string(), "velocity": v.round() as i64 })
            })
            .collect();
        let shape = vec![
            self.ctx_as(
                "同じ値に",
                "note.velocity.set",
                json!({ "ids": ids, "value": mean }),
            ),
            self.ctx_as("直線で傾ける", "note.set", json!({ "notes": slope })),
        ];
        vec![
            Item::ask_number(
                "値を入力…",
                "note.velocity.set",
                json!({ "ids": ids }),
                "value",
                &first.to_string(),
            ),
            Item::sub("揃える", shape),
        ]
    }

    /// The arguments that name the selected points of the open controller lane.
    fn point_args(&self, ctx: &Ctx) -> Option<(CtrlKind, Value)> {
        let kind = self.roll.active()?.kind.ctrl()?;
        let ticks = self
            .roll
            .points
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let args = json!({
            "clip": ctx.clip.id.to_string(),
            "lane": kind.key(),
            "ticks": ticks,
        });
        Some((kind, args))
    }

    /// S24: the selected points of a controller lane.
    fn point_items(&mut self, g: &Geometry, ctx: &Ctx) -> Vec<Item> {
        let Some((kind, args)) = self.point_args(ctx) else {
            return Vec::new();
        };
        let picked = self
            .roll
            .song_points(ctx, kind)
            .iter()
            .find(|p| self.roll.points.contains(&p.tick))
            .copied();
        let value = picked.map_or(0, |p| p.value);
        let shape_of = picked.map_or(CtrlShape::Step, |p| p.shape);
        let with = |shape: &str| {
            let mut a = args.clone();
            a["shape"] = json!(shape);
            a
        };
        let shapes = vec![
            self.ctx_as("直線", "cc.point.shape", with("linear"))
                .on(shape_of == CtrlShape::Linear),
            self.ctx_as("階段", "cc.point.shape", with("step"))
                .on(shape_of == CtrlShape::Step),
        ];
        let shape = Item::sub("次のポイントまでの形", shapes.clone())
            .enabled(shapes.iter().any(|r| r.enabled));
        let delete = Item {
            key: self.menu_key("note.delete"),
            ..self.ctx_as("削除", "edit.delete", args.clone())
        };
        vec![
            Item::ask_number(
                "値を入力…",
                "cc.point.set",
                args,
                "value",
                &value.to_string(),
            ),
            shape,
            Item::gap(),
            self.ctx_key(g, "edit.cut"),
            self.ctx_key(g, "edit.copy"),
            Item::gap(),
            delete,
        ]
    }

    /// The row that clears every point of a controller lane.
    fn clear_item(&mut self, ctx: &Ctx, kind: Option<CtrlKind>) -> Item {
        let label = "すべてのポイントを削除";
        let Some(kind) = kind else {
            // The velocity lane has no points of its own.
            return Item::new(label, Run::Cmd("cc.replace", json!({}))).enabled(false);
        };
        let to = self
            .roll
            .song_points(ctx, kind)
            .last()
            .map_or(0, |p| p.tick)
            .max(ctx.clip_len());
        self.ctx_as(
            label,
            "cc.replace",
            json!({ "clip": ctx.clip.id.to_string(), "lane": kind.key(), "from": 0, "to": to }),
        )
    }

    /// S25: the empty part of a controller lane.
    fn ctrl_lane_items(&mut self, g: &Geometry, ctx: &Ctx, tick: i64, value: i32) -> Vec<Item> {
        let Some(kind) = self.roll.active().and_then(|l| l.kind.ctrl()) else {
            return Vec::new();
        };
        let add = self.ctx_as(
            "ここにポイントを追加",
            "cc.replace",
            json!({
                "clip": ctx.clip.id.to_string(), "lane": kind.key(),
                "from": tick, "to": tick,
                "points": [{ "tick": tick, "value": value }],
            }),
        );
        let points = matches!(
            self.song.session().state.clipboard,
            Some(Clipboard::Controller { .. })
        );
        let paste = Item {
            enabled: points,
            ..self.ctx_key(g, "edit.paste")
        };
        vec![add, paste, Item::gap(), self.clear_item(ctx, Some(kind))]
    }

    /// S26: the tab of a controller lane.
    fn tab_items(&mut self, ctx: &Ctx, index: usize) -> Vec<Item> {
        let Some(lane) = self.roll.lanes.get(index).copied() else {
            return Vec::new();
        };
        let shown = |k: LaneKind| self.roll.lanes.iter().any(|l| l.kind == k);
        let kinds = |list: &[LaneKind]| -> Vec<Item> {
            list.iter()
                .map(|&k| {
                    Item::act(&kind_label(k), "", Act::RollLaneKind(index, k))
                        .enabled(!shown(k))
                        .on(lane.kind == k)
                })
                .collect()
        };
        let common: Vec<LaneKind> = crate::roll::COMMON
            .iter()
            .copied()
            .filter(|k| *k != LaneKind::Velocity)
            .collect();
        let numbered: Vec<LaneKind> = crate::roll::NUMBERED
            .iter()
            .map(|n| LaneKind::Ctrl(CtrlKind::Cc(*n)))
            .collect();
        let mut change = kinds(&common);
        change.push(Item::sub("番号で選ぶ", kinds(&numbered)));
        let hide = Item::act("このレーンを隠す", "", Act::RollLaneHide(index))
            .enabled(lane.kind != LaneKind::Velocity && self.roll.lanes.len() > 1);
        vec![
            Item::sub("種類を変更", change),
            hide,
            Item::gap(),
            self.clear_item(ctx, lane.kind.ctrl()),
        ]
    }
}

/// A controller's name in the menu: the short names of the mock.
fn kind_label(kind: LaneKind) -> String {
    match kind {
        LaneKind::Ctrl(CtrlKind::PitchBend) => "ピッチベンド".to_string(),
        LaneKind::Ctrl(CtrlKind::Aftertouch) => "アフタータッチ".to_string(),
        LaneKind::Ctrl(CtrlKind::Cc(n)) => format!("CC{n}"),
        LaneKind::Velocity => "ベロシティ".to_string(),
    }
}
