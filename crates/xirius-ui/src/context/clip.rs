//! The menus of the clips (S02 an audio clip, S03 a MIDI clip, S04 several clips).
//!
//! A right press on a clip first makes it the selection, unless it is already one of the
//! selected clips; the menu then acts on the selection, so what the rows do is what the
//! highlight shows. With one clip the menu is the one of its kind; with two or more, the
//! rows that fit every kind.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::ClipBody;
use xirius_core::time::Tick;

use super::{Act, Item, fields};
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

/// A frozen track keeps its clips as they are.
fn unfrozen(got: &Picked) -> (bool, &'static str) {
    if got.frozen > 0 {
        (false, crate::toast::FROZEN)
    } else {
        (true, "")
    }
}

/// What the menu knows of each selected clip.
struct Picked {
    adjacent: bool,
    ids: Vec<String>,
    audio: usize,
    midi: usize,
    muted: usize,
    /// The audio clips that follow the song's tempo.
    follow: usize,
    /// The clips on a frozen track.
    frozen: usize,
    /// The first start and the last end of the clips.
    span: (Tick, Tick),
    /// The only clip's start and length, when it is the only one.
    only: Option<(Tick, Tick)>,
    /// The tracks of the clips, once each.
    tracks: Vec<TrackId>,
}

impl Studio {
    /// The clips the menu acts on, after the press on `(track, clip)`: the selection when
    /// the clip is part of it, otherwise the clip alone (and it becomes the selection). The
    /// automation points stay selected only with a clip that was selected (CTX-007).
    fn clips_for_press(&mut self, track: TrackId, clip: ClipId) -> BTreeSet<(TrackId, ClipId)> {
        let key = (track, clip);
        if !self.arrange.clips.contains(&key) {
            self.arrange.points.clear();
            self.arrange.clips = BTreeSet::from([key]);
        }
        self.selected_clip = Some(key);
        self.select_row(Some(RowKind::Track(track)));
        self.arrange.touched();
        self.arrange.clips.clone()
    }

    fn picked(&self, clips: &BTreeSet<(TrackId, ClipId)>) -> Picked {
        let p = self.project();
        let mut got = Picked {
            adjacent: false,
            ids: Vec::new(),
            audio: 0,
            midi: 0,
            muted: 0,
            follow: 0,
            frozen: 0,
            span: (Tick::MAX, 0),
            only: None,
            tracks: Vec::new(),
        };
        for (t, c) in clips {
            let Some(clip) = p
                .tracks
                .get(t)
                .and_then(|t| t.clips.iter().find(|k| k.id == *c))
            else {
                continue;
            };
            got.ids.push(c.to_string());
            if !got.tracks.contains(t) {
                got.tracks.push(*t);
            }
            match &clip.body {
                ClipBody::Audio { process, .. } => {
                    got.audio += 1;
                    got.follow += usize::from(process.follow);
                }
                ClipBody::Midi { .. } => got.midi += 1,
            }
            got.frozen += usize::from(p.tracks.get(t).is_some_and(|t| t.frozen.is_some()));
            got.muted += usize::from(clip.muted);
            got.span = (got.span.0.min(clip.start), got.span.1.max(clip.end()));
            got.only = Some((clip.start, clip.length));
        }
        if got.ids.len() != 1 {
            got.only = None;
        }
        let mut ordered: Vec<_> = clips
            .iter()
            .filter_map(|(t, c)| p.clip(*c).ok().map(|(_, clip)| (*t, clip)))
            .collect();
        ordered.sort_by_key(|(_, c)| c.start);
        got.adjacent = ordered.len() > 1
            && ordered
                .windows(2)
                .all(|w| w[0].0 == w[1].0 && w[0].1.end() == w[1].1.start);
        got
    }

    /// The right press on a clip (or on one of an audio clip's handles).
    pub(super) fn clip_context(
        &mut self,
        g: &Geometry,
        track: TrackId,
        clip: ClipId,
        x: f32,
        y: f32,
    ) -> bool {
        let clips = self.clips_for_press(track, clip);
        let got = self.picked(&clips);
        if got.ids.is_empty() {
            return false;
        }
        if self.clips_and_points_selected() {
            let delete = self.mixed_delete();
            let items = self.mixed_items(g, delete);
            self.open_context(items, x, y);
            return true;
        }
        let at = self.ctx_tick(g, x);
        let mut items = self.clip_items(g, &got, at);
        let audio = got.audio > 0 && got.midi == 0;
        // An audio clip's menu starts with 「曲のテンポに合わせる」 and a gap.
        let head = if audio { 2 } else { 0 };
        if got.ids.len() == 1 {
            let p = self.project();
            let c = &p.tracks[&track]
                .clips
                .iter()
                .find(|c| c.id == clip)
                .expect("pressed clip");
            items.insert(
                head + 1,
                self.rename_item("clip.rename", clip.to_string(), &c.name),
            );
            if xirius_core::takes::stack_of(&p.tracks[&track], clip).is_some() {
                items.insert(head + 2, Item::sub("テイク", self.take_items(clip)));
            }
            if let ClipBody::Audio { file, .. } = &c.body {
                let path = std::path::PathBuf::from(file);
                let path = if path.is_absolute() {
                    path
                } else {
                    self.song.dir().map(|d| d.join(&path)).unwrap_or(path)
                };
                let index = items.len().saturating_sub(4);
                items.insert(
                    index,
                    Item::act(
                        "エクスプローラーで表示",
                        "",
                        Act::Browse(super::browser::BrowseAct::Reveal(path)),
                    ),
                );
            }
        }
        if got.midi > 0 {
            // The same field as an audio clip's (UI-311): the clip's own transpose, ±24. A MIDI
            // clip keeps its notes as written; the value is added when it plays. A selection of
            // both kinds gets it too: the audio clips' own transpose is the same value, and the
            // field shows 「—」 when they differ, as the audio clips' fields do.
            let p = self.project();
            let ids: Vec<ClipId> = got.ids.iter().filter_map(|i| i.parse().ok()).collect();
            let value = fields::shared(&p, &ids, fields::Kind::Semis);
            let (enabled, reason) = unfrozen(&got);
            let field = Item {
                reason: reason.to_string(),
                ..Item::field(fields::Kind::Semis, &got.ids, value).enabled(enabled)
            };
            let mut head = vec![field];
            head.push(Item::gap());
            items.splice(0..0, head);
        }
        if audio {
            // The clip's tempo, speed, transpose and fine tune, typed in place (EDT-007, EDT-016).
            let p = self.project();
            let ids: Vec<ClipId> = got.ids.iter().filter_map(|i| i.parse().ok()).collect();
            let (enabled, reason) = unfrozen(&got);
            let fields = super::fields::Kind::ALL.map(|kind| Item {
                reason: reason.to_string(),
                ..Item::field(kind, &got.ids, super::fields::shared(&p, &ids, kind))
                    .enabled(enabled)
            });
            items.splice(0..0, fields);
        }
        self.open_context(items, x, y);
        true
    }

    fn clip_items(&mut self, g: &Geometry, got: &Picked, at: Tick) -> Vec<Item> {
        let ids = json!(got.ids);
        let by_ids = || json!({ "ids": got.ids });
        let one = got.ids.len() == 1;
        let audio = got.audio > 0 && got.midi == 0;
        let midi = got.midi > 0 && got.audio == 0;

        let mut items = Vec::new();
        if audio {
            let all = got.follow == got.ids.len();
            let follow = if all { "off" } else { "on" };
            let row = self.ctx_as(
                "曲のテンポに合わせる",
                "clip.stretch",
                json!({ "ids": got.ids, "follow": follow }),
            );
            items.push(row.on(all));
            items.push(Item::gap());
        }
        if !one {
            let join = self.ctx_cmd("clip.join", by_ids()).selecting();
            items.push(join.clone().enabled(join.enabled && got.adjacent));
        }
        if midi && one {
            items.push(self.ctx_open());
        }
        if one {
            items.push(self.ctx_cmd("clip.split", json!({ "ids": ids, "at": at })));
        }
        items.push(self.ctx_key(g, "clip.duplicate"));
        items.push(
            self.ctx_cmd("clip.mute", by_ids())
                .on(got.muted == got.ids.len()),
        );
        items.push(self.ctx_cmd(
            "loop.set",
            json!({ "start": got.span.0, "end": got.span.1 }),
        ));
        items.push(Item::colors(
            "トラックの色",
            "clip.color.set",
            by_ids(),
            true,
        ));
        items.push(Item::gap());

        if midi {
            let mut rows = vec![
                self.ctx_as(
                    "今のグリッド",
                    "note.quantize",
                    self.quantize_args(got, 100),
                ),
                self.ctx_as("50%", "note.quantize", self.quantize_args(got, 50)),
                self.ctx_as(
                    "元に戻す",
                    "note.quantize",
                    json!({"ids":got.ids,"restore":true}),
                ),
            ];
            // Writing the value into the notes is a note operation, so it goes with the
            // quantize rows, not right under the field (UI-313).
            let value = {
                let p = self.project();
                let ids: Vec<ClipId> = got.ids.iter().filter_map(|i| i.parse().ok()).collect();
                fields::shared(&p, &ids, fields::Kind::Semis)
            };
            let (enabled, reason) = unfrozen(got);
            let mut write = self.ctx_cmd("clip.transpose.write", json!({ "ids": got.ids }));
            write.enabled &= enabled;
            if !write.enabled {
                write.reason = if !enabled {
                    reason.to_string()
                } else if value == Some(0.0) {
                    "トランスポーズが 0 です".into()
                } else {
                    "ノートがキーボードの外へ出ます".into()
                };
            }
            rows.push(write);
            items.push(Item::sub("クオンタイズ", rows));
        }
        if audio {
            items.push(self.ctx_cmd("clip.fade.clear", by_ids()));
            items.push(self.ctx_cmd("clip.gain.reset", by_ids()));
            if !one {
                items.push(self.ctx_cmd("clip.fade.cross", by_ids()));
            }
            items.push(self.process_rows(got));
        }
        items.push(self.bounce_rows(got));
        items.push(Item::gap());

        items.push(Item::act(
            "選択範囲を書き出す…",
            "",
            Act::ExportSpan(got.span.0, got.span.1),
        ));
        if got.midi > 0 && got.audio == 0 {
            items.push(Item::act(
                "MIDI ファイルとして書き出す…",
                "",
                Act::MidiExport(got.tracks.clone(), Some(got.span)),
            ));
        }
        items.push(Item::gap());
        items.push(self.ctx_key(g, "edit.cut"));
        items.push(self.ctx_key(g, "edit.copy"));
        items.push(Item::gap());
        items.push(self.ctx_key(g, "clip.delete"));
        items
    }

    /// 処理: what reads the sound of the audio clips and changes how they play (EDT-016).
    fn process_rows(&self, got: &Picked) -> Item {
        let (enabled, reason) = unfrozen(got);
        let row = |s: &Studio, name: &'static str| {
            let label = s.song.registry().label(name).unwrap_or(name).to_string();
            s.ctx_session(
                &label,
                name,
                name,
                json!({ "ids": got.ids }),
                enabled,
                reason,
            )
        };
        Item {
            reason: reason.to_string(),
            ..Item::sub(
                "処理",
                vec![
                    row(self, "clip.normalize"),
                    row(self, "clip.reverse"),
                    row(self, "clip.tempo.detect"),
                ],
            )
        }
    }

    /// バウンス: the clips, or the range the range tool chose (EDT-015, EXP-003).
    fn bounce_rows(&self, got: &Picked) -> Item {
        if let (Some(one), Some(each)) =
            (self.range_bounce_args(false), self.range_bounce_args(true))
        {
            return Item::sub("バウンス", self.range_rows(one, each));
        }
        let (enabled, reason) = unfrozen(got);
        let ids = json!({ "ids": got.ids });
        let new = json!({ "ids": got.ids, "new-track": true });
        Item::sub(
            "バウンス",
            vec![
                self.ctx_session(
                    "クリップをバウンス",
                    "clip.bounce",
                    "clip.bounce",
                    ids,
                    enabled,
                    reason,
                ),
                self.ctx_session(
                    "新しいトラックにバウンス",
                    "clip.bounce",
                    "clip.bounce --new-track",
                    new,
                    true,
                    "",
                ),
            ],
        )
    }

    /// The bounces of a range: mixed into one new track, or one new track for each track.
    pub(super) fn range_rows(&self, one: Value, each: Value) -> Vec<Item> {
        vec![
            self.ctx_session(
                "選択範囲をバウンス",
                "range.bounce",
                "clip.bounce",
                one,
                true,
                "",
            ),
            self.ctx_session(
                "新しいトラックにバウンス",
                "range.bounce",
                "clip.bounce --new-track",
                each,
                true,
                "",
            ),
        ]
    }

    /// `note.quantize` on the clips, to the grid in use.
    fn quantize_args(&self, got: &Picked, strength: i64) -> Value {
        json!({ "ids": got.ids, "grid": self.song.grid(), "strength": strength })
    }

    /// 開く: the piano roll of the clip.
    fn ctx_open(&self) -> Item {
        Item::act("ピアノロールで開く", &self.menu_key("clip.open"), Act::Open)
    }
}
