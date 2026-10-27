//! The menus of the song-files tab (BRW-004): a file's, a missing file's, and the tab's 「…」.
//!
//! Each row is only there when it makes sense for the file: a missing file can only be pointed
//! at its new place or have its clips selected, a file the song uses cannot go to the recycle bin
//! (the row stays, dimmed, with the reason), and only a file outside the song's folders is
//! copied in.

use std::path::PathBuf;

use serde_json::json;
use xirius_core::model::TrackKind;

use super::browser::BrowseAct;
use super::{Act, Item};
use crate::browser::BRow;
use crate::studio::{Geometry, Rect, Studio};

fn act(label: &str, run: BrowseAct) -> Item {
    Item::act(label, "", Act::Browse(run))
}

/// A row that says something and cannot be chosen.
fn info(label: &str, key: &str) -> Item {
    let mut row = Item::act(label, "", Act::Open).enabled(false);
    row.key = key.to_string();
    row
}

/// What the 「…」 rows say when there is nothing for them.
pub const NO_MISSING: &str = "見つからないファイルはありません";
pub const NO_EXTERNAL: &str = "外部のファイルはありません";

impl Studio {
    /// The menu of the pool's row `i`, which it selects (without starting its audition).
    pub(crate) fn pool_row_items(&mut self, g: &Geometry, i: usize) -> Option<Vec<Item>> {
        let l = self.browser.layout(g.panel);
        let BRow::Pool {
            key,
            file,
            clips,
            missing,
            external,
            ..
        } = l.rows.get(i).map(|p| p.row.clone())?
        else {
            return None;
        };
        self.browser.selected_file = Some(key.clone());
        self.browser.pool_heading = None;
        if self.browser.playing.as_deref() != Some(key.as_str()) {
            self.browser.playing = None;
        }
        let used = clips.unwrap_or(0);
        let select = act(
            "使っているクリップを選択",
            BrowseAct::SelectUsing(file.clone()),
        )
        .enabled(used > 0);
        if missing {
            return Some(vec![
                act("場所を指定…", BrowseAct::Locate(file)),
                Item::gap(),
                select,
            ]);
        }
        let path = PathBuf::from(&key);
        let here = self.selected_track_of(TrackKind::Audio).is_some();
        let mut rows = vec![
            act("新しいトラックに置く", BrowseAct::PutNew(path.clone())),
            act("選んだトラックに置く", BrowseAct::PutHere(path.clone())).enabled(here),
            Item::gap(),
        ];
        if used > 0 {
            rows.push(select);
        }
        rows.push(act(
            "エクスプローラーで表示",
            BrowseAct::Reveal(path.clone()),
        ));
        rows.push(self.rename_row(&path));
        if external {
            rows.push(Item::gap());
            rows.push(self.ctx_session(
                "曲のフォルダーにコピー",
                "file.collect",
                "file.collect",
                json!({ "paths": file }),
                self.song.dir().is_some(),
                "保存前の曲のため、曲のフォルダーにコピーできません",
            ));
        }
        rows.push(Item::gap());
        rows.push(self.trash_row(&path, "ファイルをごみ箱へ"));
        Some(rows)
    }

    /// The tab's 「…」: find the missing files, copy the outside ones in, show the song's folder,
    /// and put the recordings nothing uses in the recycle bin.
    pub(crate) fn pool_menu_items(&mut self) -> Vec<Item> {
        let missing = self.browser.pool_missing() > 0;
        let external = self
            .browser
            .pool
            .iter()
            .any(|e| e.external && e.exists && e.clips + e.frozen + e.devices > 0);
        let dir = self.song.dir();
        let mut find = act("見つからないファイルを探す…", BrowseAct::LocateFolder).enabled(missing);
        if !missing {
            find.reason = NO_MISSING.to_string();
        }
        let collect = self.ctx_session(
            "外部のファイルを曲のフォルダーにコピー",
            "file.collect",
            "file.collect",
            json!({}),
            external,
            NO_EXTERNAL,
        );
        let reveal = match &dir {
            Some(d) => act(
                "曲のフォルダーをエクスプローラーで表示",
                BrowseAct::Reveal(d.clone()),
            ),
            None => {
                let mut row = info("曲のフォルダーをエクスプローラーで表示", "");
                row.reason = "保存前の曲には、曲のフォルダーがありません".to_string();
                row
            }
        };
        vec![
            find,
            collect,
            Item::gap(),
            reveal,
            Item::gap(),
            self.cleanup_row(),
        ]
    }

    /// 「使われていない録音を整理…」: opens the dialog that lists them and asks. Dimmed with the
    /// reason when there are none.
    fn cleanup_row(&mut self) -> Item {
        const LABEL: &str = "使われていない録音を整理…";
        match self.cleanup_view() {
            Ok(_) => act(LABEL, BrowseAct::Cleanup),
            Err(reason) => {
                let mut row = info(LABEL, "");
                row.reason = reason;
                row
            }
        }
    }

    /// Opens the 「…」 under its button.
    pub(crate) fn open_pool_menu(&mut self, at: Rect) {
        let rows = self.pool_menu_items();
        self.open_context(rows, at.x, at.bottom());
    }
}
