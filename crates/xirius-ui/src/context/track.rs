//! The menus of the track column (S06 a track, S07 several rows, S08 a folder, S09 the
//! room under the last row).
//!
//! A right press on a row first makes it the selection, unless it is one of the rows
//! already selected; the menu acts on the selection, so what the rows do is what the
//! highlight shows. With several rows, the rows that fit tracks and folders alike.

use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::id::{FolderId, Node, TrackId};
use xirius_core::model::{Parent, Project, TrackKind};

use super::{Act, Item};
use crate::arrange::{lane_name, targets_of};
use crate::export::Group;
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

/// The rows a menu acts on, as the ids the commands take.
struct Picked {
    /// Every row, tracks and folders (the commands that take either).
    nodes: Vec<String>,
    /// The tracks among them.
    tracks: Vec<TrackId>,
}

impl Picked {
    fn of(rows: &[RowKind]) -> Picked {
        let mut got = Picked {
            nodes: Vec::new(),
            tracks: Vec::new(),
        };
        for r in rows {
            match r {
                RowKind::Track(t) => {
                    got.nodes.push(t.to_string());
                    got.tracks.push(*t);
                }
                RowKind::Folder(f) => got.nodes.push(f.to_string()),
            }
        }
        got
    }

    fn track_ids(&self) -> Vec<String> {
        self.tracks.iter().map(ToString::to_string).collect()
    }
}

/// Where a new row goes to sit right under `node`: its folder (or `root`) and its place.
fn below(p: &Project, node: Node) -> (String, i64) {
    match p.find_node(node) {
        Some((Parent::Folder(f), i)) => (f.to_string(), i as i64 + 1),
        Some((Parent::Root, i)) => ("root".to_string(), i as i64 + 1),
        None => ("root".to_string(), -1),
    }
}

impl Studio {
    /// The right press on a row of the column.
    pub(super) fn row_context(&mut self, g: &Geometry, kind: RowKind, x: f32, y: f32) -> bool {
        let rows = if self.arrange.tracks.len() > 1 && self.arrange.tracks.contains(&kind) {
            self.arrange.tracks.clone()
        } else {
            vec![kind]
        };
        self.arrange.tracks = rows.clone();
        self.select_row(Some(kind));
        self.arrange.touched();
        let items = match (rows.as_slice(), kind) {
            ([_], RowKind::Track(t)) => self.track_items(g, t),
            ([_], RowKind::Folder(f)) => self.folder_items(f),
            _ => self.rows_items(&rows),
        };
        self.open_context(items, x, y);
        true
    }

    /// The right press under the last row, in the column or in the lanes.
    pub(super) fn column_context(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if !g.column.contains(x, y) && !g.lanes.contains(x, y) {
            return false;
        }
        if self.tree().row_at(y - g.column.y + self.scroll.1).is_some() {
            return false;
        }
        let add = self.add_rows(json!({}));
        let board = matches!(
            self.song.session().state.clipboard,
            Some(xirius_core::clipboard::Clipboard::Tracks { .. })
        );
        let mut paste = self
            .ctx_as("貼り付け", "edit.paste", json!({}))
            .enabled(board);
        paste.key = self.menu_key("edit.paste");
        let folders = vec![
            Item::act("すべて開く", "", Act::Folders(true)),
            Item::act("すべて畳む", "", Act::Folders(false)),
        ];
        let has_folders = !self.project().folders.is_empty();
        let items = vec![
            Item::sub("トラックを追加", add),
            paste,
            Item::sub("フォルダー", folders).enabled(has_folders),
        ];
        self.open_context(items, x, y);
        true
    }

    /// The four kinds of row a menu can add, each with where it goes (`at`: `into` and
    /// `index`, none for the end of the column).
    pub(super) fn add_rows(&mut self, at: Value) -> Vec<Item> {
        let by =
            |label: &str, name: &'static str, me: &mut Studio| me.ctx_as(label, name, at.clone());
        let mut rows = vec![
            by("オーディオ", "track.add.audio", self),
            by("インストゥルメント", "track.add.instrument", self),
            by("フォルダー", "folder.add", self),
            by("バス", "track.add.bus", self),
        ];
        rows.extend(self.preset_add_rows(&at));
        rows
    }

    pub(super) fn label_of(&self, name: &str) -> String {
        self.song.registry().label(name).unwrap_or(name).to_string()
    }

    /// The row that asks for a name before it renames.
    pub(super) fn rename_item(&self, name: &'static str, id: String, current: &str) -> Item {
        Item::ask(
            &self.label_of(name),
            name,
            json!({ "id": id }),
            "name",
            current,
        )
    }

    /// Row "ステムを書き出す…", with the key the palette shows.
    pub(super) fn stems_item(&self, tracks: Vec<TrackId>) -> Item {
        let key = self.menu_key("export.stems");
        let enabled = !tracks.is_empty();
        Item::act("ステムを書き出す…", &key, Act::Stems(tracks)).enabled(enabled)
    }

    /// Row "書き出す" of one track: its stem, and the MIDI file of an instrument track (UI-281).
    fn export_item(&self, t: TrackId, notes: bool) -> Item {
        let stem = Item::act(
            "このトラックのステム…",
            &self.menu_key("export.stems"),
            Act::Stems(vec![t]),
        )
        .enabled(true);
        let midi = Item::act(
            "MIDI ファイル…",
            &self.menu_key("export.midi"),
            Act::MidiExport(vec![t], None),
        )
        .enabled(notes);
        Item::sub("書き出す", vec![stem, midi])
    }

    /// Row "ステムを書き出す" of a folder: the folder as one file, which the bus it is connected
    /// to makes possible, or the tracks inside one by one (CTX-018).
    fn folder_stems_item(&self, f: FolderId, inner: Vec<TrackId>) -> Item {
        let key = self.menu_key("export.stems");
        let bus = crate::export::folder_bus(&self.project(), f);
        let each = Item::act("中のトラックごと…", "", Act::Stems(inner)).enabled(true);
        let one =
            Item::act("このフォルダーを 1 本に…", "", Act::FolderStem(f)).enabled(bus.is_some());
        let mut row = Item::sub("ステムを書き出す", vec![one, each]);
        row.key = key;
        row
    }

    pub(super) fn duplicate_item(&mut self, ids: Vec<String>) -> Item {
        let rows = vec![
            self.ctx_as("中身ごと", "track.duplicate", json!({ "ids": ids })),
            self.ctx_as(
                "設定だけ",
                "track.duplicate",
                json!({ "ids": ids, "mode": "settings" }),
            ),
        ];
        Item::sub(&self.label_of("track.duplicate"), rows)
    }

    /// S06: one track.
    fn track_items(&mut self, g: &Geometry, t: TrackId) -> Vec<Item> {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return Vec::new();
        };
        let id = t.to_string();
        let ids = vec![id.clone()];
        let (into, index) = below(&p, Node::Track(t));
        let rename = self.rename_item("track.rename", id.clone(), &track.name);
        let color = Item::colors("既定", "track.color.set", json!({ "ids": ids }), true)
            .current(crate::draw::track_color(track));
        let duplicate = self.duplicate_item(ids.clone());
        let add = self.add_rows(json!({ "into": into, "index": index }));
        let lanes = self.lane_rows(&p, t);
        let pack = self.ctx_cmd("folder.pack", json!({ "ids": ids }));
        let stems = self.export_item(t, track.kind == TrackKind::Instrument);
        let bounce = self.ctx_session(
            "新しいトラックにバウンス",
            "track.bounce",
            "clip.bounce --new-track",
            json!({ "ids": ids }),
            true,
            "",
        );
        let freeze = if track.frozen.is_some() {
            self.ctx_cmd("track.unfreeze", json!({ "ids": ids }))
        } else {
            // What the session refuses: a track with no clips, and one that sends its MIDI out.
            let can = track.has_clips()
                && !(track.kind == TrackKind::Instrument && track.midi_out.is_some());
            self.ctx_session(
                "トラックをフリーズ",
                "track.freeze",
                "track.freeze",
                json!({ "ids": ids }),
                can,
                "フリーズできません",
            )
        };
        let delete = self.ctx_cmd("track.delete", json!({ "ids": ids }));
        let copy = self
            .ctx_as("コピー", "edit.copy", json!({"tracks": [id]}))
            .enabled(true);
        let _ = g;
        let mut rows = vec![
            rename,
            color,
            duplicate,
            Item::sub("下にトラックを追加", add),
            Item::gap(),
        ];
        rows.extend(self.preset_items(t));
        rows.extend([
            Item::sub("オートメーションを表示", lanes),
            pack,
            Item::gap(),
            bounce,
            freeze,
            Item::gap(),
            stems,
            Item::gap(),
            copy,
            Item::gap(),
            delete,
        ]);
        rows
    }

    /// The lanes a track can show: its own, its sends, and the parameters of its plugins.
    pub(super) fn lane_rows(&self, p: &Project, t: TrackId) -> Vec<Item> {
        let Some(track) = p.tracks.get(&t) else {
            return Vec::new();
        };
        let shown = |target: &AutoTarget| {
            self.arrange.open.contains(&t)
                && track
                    .automation
                    .iter()
                    .any(|l| !l.hidden && l.target == *target)
        };
        let mut rows = Vec::new();
        let mut params: Vec<Item> = Vec::new();
        for target in targets_of(track) {
            let row = Item::act(
                &lane_name(p, track, &target),
                "",
                Act::Lane(t, target.to_string()),
            )
            .on(shown(&target));
            match target {
                AutoTarget::Param(..) => params.push(row),
                _ => rows.push(row),
            }
        }
        if !params.is_empty() {
            // One list per plugin, the plugin's name leaving the rows.
            let mut by_plugin: Vec<(String, Vec<Item>)> = Vec::new();
            for plugin in &track.inserts {
                let prefix = format!("{} · ", plugin.name);
                let mine: Vec<Item> = params
                    .iter()
                    .filter(|r| r.label.starts_with(&prefix))
                    .map(|r| r.clone().labelled(&r.label[prefix.len()..]))
                    .collect();
                if !mine.is_empty() {
                    by_plugin.push((plugin.name.clone(), mine));
                }
            }
            let subs = by_plugin
                .into_iter()
                .map(|(name, rows)| Item::sub(&name, rows))
                .collect();
            rows.push(Item::gap());
            rows.push(Item::sub("パラメーターを選ぶ", subs));
        }
        rows
    }

    /// S07: several rows.
    fn rows_items(&mut self, rows: &[RowKind]) -> Vec<Item> {
        let picked = Picked::of(rows);
        let p = self.project();
        let tracks = picked.track_ids();
        let any_track = !tracks.is_empty();
        let bus = self
            .ctx_cmd("track.add.bus", json!({ "sources": tracks }))
            .labelled("新しいバスへまとめる");
        let mut out = vec![self.ctx_as(
            "マスター",
            "track.output.set",
            json!({ "ids": tracks, "to": "master" }),
        )];
        for b in p.tracks.values().filter(|t| t.kind == TrackKind::Bus) {
            out.push(self.ctx_as(
                &b.name,
                "track.output.set",
                json!({ "ids": tracks, "to": b.id.to_string() }),
            ));
        }
        let output = Item::sub(&self.label_of("track.output.set"), out).enabled(any_track);
        let pack = self.ctx_cmd("folder.pack", json!({ "ids": picked.nodes }));
        let color = Item::colors(
            "既定",
            "track.color.set",
            json!({ "ids": picked.nodes }),
            true,
        );
        let duplicate = self.duplicate_item(tracks.clone()).enabled(any_track);
        let stems = self.stems_item(picked.tracks.clone());
        let delete = self
            .ctx_cmd("track.delete", json!({ "ids": tracks }))
            .enabled(any_track);
        vec![
            bus,
            output,
            pack,
            color,
            duplicate,
            Item::gap(),
            stems,
            Item::gap(),
            delete,
        ]
    }

    /// S08: one folder.
    fn folder_items(&mut self, f: FolderId) -> Vec<Item> {
        let p = self.project();
        let Some(folder) = p.folders.get(&f) else {
            return Vec::new();
        };
        let id = f.to_string();
        let inner = p.tracks_in(f);
        let rename = self.rename_item("folder.rename", id.clone(), &folder.name);
        let paint = Item::color_rows("既定", "track.color.set", json!({ "ids": [id] }));
        let mut all = vec![id.clone()];
        all.extend(inner.iter().map(ToString::to_string));
        let all_color = Item::sub(
            "中のトラックにも塗る",
            Item::color_rows("既定", "track.color.set", json!({ "ids": all })),
        );
        let color = Item::sub("色", paint);
        let select =
            Item::act("中のトラックを選択", "", Act::SelectIn(f)).enabled(!inner.is_empty());
        let duplicate = self.ctx_cmd("folder.duplicate", json!({ "id": id }));
        let stems = self.folder_stems_item(f, inner);
        let unpack = self.ctx_cmd("folder.unpack", json!({ "id": id }));
        let delete = vec![
            self.ctx_as("フォルダーだけ", "folder.delete", json!({ "id": id })),
            self.ctx_as(
                "中のトラックごと",
                "folder.delete",
                json!({ "id": id, "with-tracks": true }),
            ),
        ];
        vec![
            rename,
            color,
            all_color,
            select,
            duplicate,
            Item::gap(),
            stems,
            Item::gap(),
            unpack,
            Item::sub(&self.label_of("folder.delete"), delete),
        ]
    }

    // ---------------------------------------------------------- what the rows do

    /// Shows a track's lane of `target`, making the lane when the track has none.
    pub(super) fn show_lane(&mut self, g: &Geometry, t: TrackId, target: &str) -> bool {
        let there = self.project().tracks.get(&t).is_some_and(|track| {
            track
                .automation
                .iter()
                .any(|l| !l.hidden && l.target.to_string() == target)
        });
        if !there
            && self
                .arrange_run(
                    "auto.lane.add",
                    json!({ "track": t.to_string(), "target": target }),
                )
                .is_none()
        {
            return false;
        }
        self.arrange.open.insert(t);
        self.arrange.touched();
        self.rebuild(g);
        true
    }

    /// The tracks inside the folder become the selection.
    pub(super) fn select_in(&mut self, f: FolderId) -> bool {
        let p = self.project();
        let rows: Vec<_> = p.tracks_in(f).into_iter().map(RowKind::Track).collect();
        if rows.is_empty() {
            return false;
        }
        self.select_row(rows.first().copied());
        self.arrange.tracks = rows;
        self.arrange.touched();
        true
    }

    /// Opens the export panel with the folder's bus as one stem named after the folder.
    pub(super) fn open_folder_stem(&mut self, g: &Geometry, f: FolderId) -> bool {
        let Some(bus) = crate::export::folder_bus(&self.project(), f) else {
            return false;
        };
        self.open_stems(g, vec![bus]);
        self.export.folders = vec![f];
        true
    }

    /// Opens the export panel with these tracks as the stems.
    pub(super) fn open_stems(&mut self, g: &Geometry, tracks: Vec<TrackId>) -> bool {
        if tracks.is_empty() {
            return false;
        }
        self.export.set_stems(true);
        self.export.group = Group::Selected;
        self.export.picked = tracks;
        self.export.folders.clear();
        let anchor = self.export_anchor(g);
        self.open_export(anchor);
        true
    }
}
