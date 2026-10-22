//! The menus of the section band: a section (CTX-023) and the empty part of the band
//! (CTX-024).

use serde_json::json;
use xirius_core::model::Section;

use super::{Act, Item, Run};
use crate::studio::{Geometry, Studio};

/// The names a section is given in one choice (the order of the mock).
pub const SECTION_NAMES: [&str; 6] = ["Intro", "Verse", "Pre-Chorus", "Chorus", "Bridge", "Outro"];

impl Studio {
    /// The right press on a section: it is selected, and its menu opens.
    pub(crate) fn section_context(
        &mut self,
        g: &Geometry,
        section: xirius_core::id::SectionId,
        x: f32,
        y: f32,
    ) -> bool {
        let Some(s) = self
            .project()
            .sections
            .iter()
            .find(|s| s.id == section)
            .cloned()
        else {
            return false;
        };
        self.select_section(section);
        let items = self.section_items(g, &s);
        self.open_context(items, x, y);
        true
    }

    /// The right press on the part of the band that has no section.
    pub(crate) fn band_context(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        self.clear_section();
        let at = self.bar_start(g, x);
        let names = SECTION_NAMES
            .iter()
            .map(|name| {
                self.ctx_as(
                    name,
                    "section.add",
                    json!({ "name": name, "at": at, "bars": 1 }),
                )
            })
            .collect();
        let mut paste = self
            .ctx_cmd("section.paste", json!({ "at": at }))
            .labelled("貼り付け");
        paste.key = self.menu_key("edit.paste");
        let items = vec![Item::sub("セクションを追加", names), paste];
        self.open_context(items, x, y);
        true
    }

    /// The double click on a section: the name box, with the name in it.
    pub(crate) fn section_rename_box(
        &mut self,
        section: xirius_core::id::SectionId,
        x: f32,
        y: f32,
    ) -> bool {
        let Some(name) = self
            .project()
            .sections
            .iter()
            .find(|s| s.id == section)
            .map(|s| s.name.clone())
        else {
            return false;
        };
        self.select_section(section);
        let item = Item::ask(
            "名前を入力…",
            "section.rename",
            json!({ "id": section.to_string() }),
            "name",
            &name,
        );
        let run = item.run.clone();
        self.open_context(vec![item], x, y);
        self.ask_open(run);
        true
    }

    /// The name a section made from the loop gets: the first of the names no section has.
    pub(crate) fn fresh_section_name(&self) -> &'static str {
        let p = self.project();
        SECTION_NAMES
            .iter()
            .find(|n| p.sections.iter().all(|s| s.name != **n))
            .copied()
            .unwrap_or("Verse")
    }

    /// The start of the bar under a window x.
    fn bar_start(&self, g: &Geometry, x: f32) -> xirius_core::time::Tick {
        let at = self.tick_at(g, x).max(0);
        let map = self.project().time_map();
        map.bar_start(map.to_bbt(at).0)
    }

    /// A row for a command that only reads (copy, select). A trial run of it would do the
    /// work (it would copy), so the row is not tried: it is on unless `enabled` says not.
    fn ctx_read(
        &self,
        name: &'static str,
        label: &str,
        args: serde_json::Value,
        enabled: bool,
    ) -> Item {
        Item {
            key: self.menu_key(name),
            enabled,
            reason: if enabled {
                String::new()
            } else {
                "セクションにかかるクリップがありません".into()
            },
            ..Item::new(label, Run::Cmd(name, args))
        }
    }

    /// CTX-023: what can be done to a section.
    fn section_items(&mut self, g: &Geometry, s: &Section) -> Vec<Item> {
        let id = s.id.to_string();
        let (start, end) = (s.start, s.start + s.length);
        let mut names: Vec<Item> = SECTION_NAMES
            .iter()
            .map(|name| {
                self.ctx_as(name, "section.rename", json!({ "id": id, "name": name }))
                    .enabled(true)
                    .on(s.name == *name)
            })
            .collect();
        names.push(Item::gap());
        names.push(Item::ask(
            "名前を入力…",
            "section.rename",
            json!({ "id": id }),
            "name",
            &s.name,
        ));
        let color = Item::colors("なし", "section.color.set", json!({ "id": id }), true);
        let color = match s.color.as_deref().and_then(crate::song::palette_color) {
            Some(c) => color.current(c.index()),
            None => color,
        };
        // The split is at the playhead, when it is inside; at the edge it has nothing to cut.
        let at = self.playhead.clamp(start, end);
        let split = self
            .ctx_cmd("section.split", json!({ "id": id, "at": at }))
            .labelled("ここで分割");
        let mut looped = self.ctx_as(
            "セクションをループ",
            "loop.set",
            json!({ "start": start, "end": end }),
        );
        looped.key = self.menu_key("loop.selection");
        let duplicate = self
            .ctx_cmd("section.duplicate", json!({ "id": id }))
            .labelled("複製");
        let cut = self
            .ctx_cmd("section.cut", json!({ "id": id }))
            .labelled("切り取り");
        let mut copy = self.ctx_read("section.copy", "コピー", json!({ "id": id }), true);
        copy.key = self.menu_key("edit.copy");
        let paste = self
            .ctx_cmd("section.paste", json!({ "at": end }))
            .labelled("貼り付け");
        let delete = self
            .ctx_cmd("section.delete", json!({ "id": id }))
            .labelled("削除");
        let mut remove_time = self
            .ctx_cmd("range.time.delete", json!({ "start": start, "end": end }))
            .labelled("時間を削除");
        remove_time.key = self.menu_key("range.time.delete");
        let mut cut = cut;
        cut.key = self.menu_key("edit.cut");
        let mut paste = paste;
        paste.key = self.menu_key("edit.paste");
        let mut delete = delete;
        delete.key = self.menu_key("clip.delete");
        let has_clips = self
            .project()
            .tracks
            .values()
            .flat_map(|t| t.clips.iter())
            .any(|c| c.start < end && c.end() > start);
        let select = self.ctx_read(
            "select.in-section",
            "セクションのものを選択",
            json!({ "id": id }),
            has_clips,
        );
        let export = Item::act("セクションを書き出す…", "", Act::ExportSpan(start, end));
        let _ = g;
        vec![
            Item::sub("セクション名を変更", names),
            color,
            Item::gap(),
            duplicate,
            looped,
            select,
            split,
            export,
            Item::gap(),
            cut,
            copy,
            paste,
            Item::gap(),
            delete,
            remove_time,
        ]
    }
}
