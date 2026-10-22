//! The menus of the ruler (S10) and of the loop band above it (S11).

use serde_json::json;

use super::{Act, Item};
use crate::export::Range;
use crate::ruler::LoopGrip;
use crate::studio::{Geometry, Studio};

/// The meters the ruler's menu offers.
const METERS: [(u32, u32); 8] = [
    (2, 4),
    (3, 4),
    (4, 4),
    (5, 4),
    (6, 8),
    (7, 8),
    (9, 8),
    (12, 8),
];

impl Studio {
    /// The right press on the ruler: the loop band's own menu on the loop, the ruler's
    /// everywhere else.
    pub(crate) fn ruler_context(
        &mut self,
        g: &Geometry,
        grip: Option<LoopGrip>,
        x: f32,
        y: f32,
    ) -> bool {
        let items = match grip {
            Some(LoopGrip::Start | LoopGrip::End | LoopGrip::Body) => self.loop_items(g),
            _ => self.ruler_items(g, x),
        };
        self.open_context(items, x, y);
        true
    }

    /// S10: what can be done at a point of the ruler.
    fn ruler_items(&mut self, g: &Geometry, x: f32) -> Vec<Item> {
        let at = self.ctx_tick(g, x);
        let p = self.project();
        let bpm = p
            .tempo
            .iter()
            .rev()
            .find(|t| t.tick <= at)
            .or(p.tempo.first())
            .map_or(120.0, |t| t.bpm);
        let bar = p.time_map().to_bbt(at).0;
        let meters = METERS
            .iter()
            .map(|&(num, den)| {
                self.ctx_as(
                    &format!("{num}/{den}"),
                    "timesig.set",
                    json!({ "num": num, "den": den, "bar": bar }),
                )
            })
            .collect();
        vec![
            Item::act("ここから再生", "", Act::PlayFrom(at)),
            self.ctx_cmd("marker.add", json!({"at": at})),
            self.ctx_cmd("loop.start.set", json!({ "at": at })),
            self.ctx_cmd("loop.end.set", json!({ "at": at })),
            Item::gap(),
            Item::ask_number(
                "テンポを設定…",
                "tempo.set",
                json!({ "at": at }),
                "bpm",
                &format!("{bpm}"),
            ),
            Item::sub("拍子を設定", meters),
            Item::gap(),
            Item::sub(
                "目盛り",
                vec![
                    self.ctx_as("小節と拍", "view.ruler", json!({"unit":"bars"}))
                        .enabled(true)
                        .on(!self.song.session().state.view.ruler_seconds),
                    self.ctx_as("分と秒", "view.ruler", json!({"unit":"seconds"}))
                        .enabled(true)
                        .on(self.song.session().state.view.ruler_seconds),
                ],
            ),
        ]
    }

    /// S11: the loop.
    fn loop_items(&mut self, g: &Geometry) -> Vec<Item> {
        let duplicate = self.ctx_cmd("loop.duplicate", json!({})).labelled("複製");
        let name = self.fresh_section_name();
        let section = self
            .ctx_as(
                "セクションにする",
                "section.add",
                json!({ "name": name, "range": "loop" }),
            )
            .labelled("セクションにする");
        let mut insert = self.ctx_cmd("range.time.insert", json!({ "range": "loop" }));
        insert.key = self.menu_key("range.time.insert");
        let mut delete = self.ctx_cmd("range.time.delete", json!({ "range": "loop" }));
        delete.key = self.menu_key("range.time.delete");
        vec![
            self.ctx_key(g, "transport.loop"),
            self.ctx_key(g, "select.in-loop"),
            Item::act("ループの中を書き出す…", "", Act::ExportLoop),
            Item::gap(),
            duplicate,
            section,
            Item::gap(),
            insert,
            delete,
        ]
    }

    /// Starts playing from `at`.
    pub(super) fn play_from(&mut self, at: xirius_core::time::Tick) -> bool {
        self.seek(at);
        if !self.transport.playing {
            self.toggle_play();
        }
        true
    }

    /// Opens the export panel for the mixdown of the loop.
    pub(super) fn open_loop_export(&mut self, g: &Geometry) -> bool {
        self.load_export();
        self.export.set_stems(false);
        self.export.range = Range::Loop;
        let anchor = self.export_anchor(g);
        self.open_export(anchor);
        true
    }
}
