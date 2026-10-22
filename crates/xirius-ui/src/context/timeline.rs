//! The menus of the arrangement (group A of the matrix): the empty lane, the clips, the
//! ruler, a range.

use serde_json::json;

use super::Item;
use crate::arrange::Spot;
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

impl Studio {
    /// The right press in the arrangement: the menu of what is under the pointer.
    pub(crate) fn timeline_context(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        self.focus_arrange(true);
        self.input_map.tempo_selected = None;
        self.input_map.marker_selected = None;
        if self.marker_context(g, x, y) {
            return true;
        }
        if let Some((track, target)) = self.lane_head_at(g, x, y) {
            return self.auto_head_context(track, target, (x, y));
        }
        match self.arrange_spot(g, x, y) {
            Spot::Space(RowKind::Track(_)) if self.in_range(g, x) => {
                let items = self.range_items();
                self.open_context(items, x, y);
                true
            }
            Spot::Space(RowKind::Track(t)) => {
                let items = self.lane_items(g, t, x);
                self.open_context(items, x, y);
                true
            }
            Spot::Clip { track, clip, .. } | Spot::Fx { track, clip, .. } => {
                self.clip_context(g, track, clip, x, y)
            }
            Spot::Loop(grip) => self.ruler_context(g, Some(grip), x, y),
            Spot::Section { id, .. } => self.section_context(g, id, x, y),
            Spot::Band => self.band_context(g, x, y),
            Spot::Tempo(i) => self.tempo_point_context(i, x, y),
            Spot::Rec {
                part: crate::rec_area::RecPart::Bar { take, from, to },
                ..
            } => self.take_context_over(take, Some((from, to)), x, y),
            Spot::Point {
                track,
                target,
                index,
            } => self.auto_point_context(g, track, target, index, (x, y)),
            Spot::Line { track, target, .. } | Spot::Lane { track, target }
                if self.in_range(g, x) =>
            {
                self.lane_range_context(track, target, (x, y))
            }
            Spot::Line {
                track,
                target,
                index,
            } => self.auto_lane_context(g, track, target, Some(index), (x, y)),
            Spot::Lane { track, target } => self.auto_lane_context(g, track, target, None, (x, y)),
            Spot::None if g.ruler.contains(x, y) => self.ruler_context(g, None, x, y),
            Spot::TempoGap if self.in_range(g, x) => self.tempo_range_context((x, y)),
            Spot::TempoGap => self.tempo_row_context(g, x, y),
            Spot::Row(kind) => self.row_context(g, kind, x, y),
            Spot::None => self.column_context(g, x, y),
            _ => false,
        }
    }

    /// Whether `x` is inside the range the range tool chose.
    fn in_range(&mut self, g: &Geometry, x: f32) -> bool {
        let at = self.ctx_tick(g, x);
        self.arrange.range.is_some_and(|(a, b)| a <= at && at <= b)
    }

    /// The range the range tool chose: what can be done to the clips in it (S06 of the audio
    /// processing) and to the time it spans. The time rows are the commands of the whole song.
    fn range_items(&mut self) -> Vec<Item> {
        let Some((from, to)) = self.arrange.range else {
            return Vec::new();
        };
        let mut items = Vec::new();
        if let (Some(one), Some(each)) =
            (self.range_bounce_args(false), self.range_bounce_args(true))
        {
            items.extend(self.range_rows(one, each));
            items.push(Item::gap());
            items.push(Item::act(
                "選択範囲を書き出す…",
                "",
                super::Act::ExportSpan(from, to),
            ));
        }
        let ids: Vec<String> = self
            .arrange
            .clips
            .iter()
            .map(|(_, clip)| clip.to_string())
            .collect();
        items.push(
            self.ctx_cmd(
                "clip.split.range",
                json!({ "ids": ids, "start": from, "end": to }),
            )
            .enabled(!ids.is_empty()),
        );
        for command in ["range.cut", "range.copy", "range.duplicate", "range.delete"] {
            items.push(
                self.ctx_cmd(command, json!({ "ids": ids, "start": from, "end": to }))
                    .enabled(!ids.is_empty()),
            );
        }
        items.push(self.ctx_cmd("loop.set", json!({ "start": from, "end": to })));
        items.push(Item::gap());
        let span = json!({ "start": from, "end": to });
        items.push(self.ctx_cmd("range.time.insert", span.clone()));
        items.push(self.ctx_cmd("range.time.delete", span));
        items
    }

    /// S01: the empty part of a track's lane.
    fn lane_items(&mut self, g: &Geometry, track: xirius_core::id::TrackId, x: f32) -> Vec<Item> {
        let at = self.ctx_tick(g, x);
        let t = track.to_string();
        let instrument = self
            .project()
            .tracks
            .get(&track)
            .is_some_and(|t| t.kind == xirius_core::model::TrackKind::Instrument);
        let paste = self
            .ctx_cmd("edit.paste", json!({ "at": at, "track": t }))
            .selecting();
        let make = self
            .ctx_cmd("clip.add.midi", json!({ "track": t, "at": at, "bars": 1 }))
            .selecting()
            .enabled(instrument);
        let seek = self.ctx_do("transport.seek", super::Act::Seek(at));
        let select = vec![
            self.ctx_key(g, "select.all"),
            self.ctx_key(g, "select.on-tracks"),
            self.ctx_key(g, "select.in-loop"),
            self.ctx_key(g, "select.invert"),
        ];
        let marker = self.ctx_cmd("marker.add", json!({"at": at}));
        vec![paste, make, seek, marker, Item::sub("選択", select)]
    }
}
