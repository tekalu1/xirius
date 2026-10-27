//! The right-click menu of an export in the export panel (S44).
//!
//! The rows do what the card's own buttons do (the same `export_action`), so the files and
//! the record of the export change the way they change from the card. The panel is the one
//! popup there is, so it gives way to the menu and is opened again where it was when a row has
//! run. An export that is still running has no menu: it can only be cancelled, from its card.
//! A file of a stems export has a menu of its own, whose rows are about that file alone
//! (EXP-020).

use std::path::PathBuf;

use serde_json::json;

use super::{Act, Item};
use crate::export::{self, Card, Kid, State};
use crate::studio::{Geometry, Rect, Studio};

/// What a row of the menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// Plays the mixdown, as the browser plays a file.
    Listen,
    /// Writes the files again from the song as it is now.
    Rewrite,
    /// Opens the panel with the settings this export was made with.
    Same,
    /// Brings the files into the song as new tracks, at the head of the exported range.
    Import,
    /// Shows the written file in the OS's file window.
    Reveal,
    /// The files to the trash, and the card away.
    Trash,
}

/// A row, for the export it is about.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportAct {
    pub job: String,
    pub row: Row,
    /// Where the panel hangs, to open it again.
    pub anchor: Rect,
    /// The one file of the export it is about, by its path; `None` for all of them.
    pub file: Option<String>,
}

/// A row for the export `card` is of.
fn row(card: &Card, anchor: Rect, label: &str, row: Row) -> Item {
    Item::act(
        label,
        "",
        Act::Export(ExportAct {
            job: card.job.clone(),
            row,
            anchor,
            file: None,
        }),
    )
}

/// A row for one file of the export `card` is of.
fn file_row(card: &Card, kid: &Kid, anchor: Rect, label: &str, row: Row) -> Item {
    Item::act(
        label,
        "",
        Act::Export(ExportAct {
            job: card.job.clone(),
            row,
            anchor,
            file: Some(kid.path.clone()),
        }),
    )
}

impl Studio {
    pub(crate) fn export_confirm_trash(&mut self, job: &str, anchor: Rect) -> bool {
        let files = self.written_files(job);
        // A cached absence cannot authorize skipping confirmation: a file may
        // have appeared since that snapshot. The worker checks again after Yes.
        if files.is_empty()
            || (!self.transport.is_windowed() && !files.iter().any(|p| self.export_file_exists(p)))
        {
            return false;
        }
        let act = ExportAct {
            job: job.to_string(),
            row: Row::Trash,
            anchor,
            file: None,
        };
        self.open_context(
            vec![Item::act("ファイルをごみ箱へ", "", Act::Export(act))],
            anchor.x,
            anchor.y,
        );
        true
    }
    /// The files an export wrote.
    pub(crate) fn written_files(&self, job: &str) -> Vec<PathBuf> {
        let Some(j) = self.export_job(job) else {
            return Vec::new();
        };
        match j["result"]["files"].as_array() {
            Some(files) => files
                .iter()
                .filter_map(|f| f["path"].as_str())
                .map(PathBuf::from)
                .collect(),
            None => j["result"]["path"]
                .as_str()
                .or(j["path"].as_str())
                .filter(|_| j["command"] == "export.mixdown")
                .map(PathBuf::from)
                .into_iter()
                .collect(),
        }
    }

    /// The rows of the menu of a card.
    fn export_items(&self, card: &Card, anchor: Rect) -> Vec<Item> {
        let act = |label: &str, r: Row| row(card, anchor, label, r);
        let files = self.written_files(&card.job);
        let there = files.iter().any(|p| self.export_file_exists(p));
        let done = card.state == State::Done;
        let listen = done
            && !card.stems
            && files
                .first()
                .is_some_and(|p| self.export_file_exists(p) && crate::drop_audio::is_audio(p));
        vec![
            act("聴く", Row::Listen).enabled(listen),
            // An export made before records kept the song may be out of date: it can be written
            // again.
            act("書き直す", Row::Rewrite).enabled(done && card.stale != Some(false)),
            act("同じ設定で書き出しを開く", Row::Same),
            Item::gap(),
            act("曲に取り込む", Row::Import).enabled(done && there),
            act("エクスプローラーで表示", Row::Reveal).enabled(done && there),
            Item::gap(),
            Item::sub("ごみ箱へ", vec![act("ファイルをごみ箱へ", Row::Trash)]),
        ]
    }

    /// The rows of the menu of one file of a card.
    fn export_file_items(&self, card: &Card, kid: &Kid, anchor: Rect) -> Vec<Item> {
        let act = |label: &str, r: Row| file_row(card, kid, anchor, label, r);
        let path = std::path::Path::new(&kid.path);
        let there = self.export_file_exists(path);
        vec![
            act("聴く", Row::Listen).enabled(there && crate::drop_audio::is_audio(path)),
            act("書き直す", Row::Rewrite).enabled(kid.stale == Some(true)),
            act("曲に取り込む", Row::Import).enabled(there),
            act("エクスプローラーで表示", Row::Reveal).enabled(there),
        ]
    }

    /// The file of a stems export under a point of the open panel, with its card.
    pub(crate) fn export_file_at(&self, g: &Geometry, x: f32, y: f32) -> Option<(Card, Kid)> {
        let p = self
            .popup
            .as_ref()
            .filter(|p| p.kind == crate::popup::PopupKind::Export)?;
        let kid = p
            .layout(g.size)
            .cells
            .into_iter()
            .find_map(|c| match c.look {
                crate::popup::Look::Xp(export::XpLook::File(kid)) if c.rect.contains(x, y) => {
                    Some(*kid)
                }
                _ => None,
            })?;
        Some((self.export_card_at(g, x, y)?, kid))
    }

    /// The right press on the open export panel: the menu of the card under the point. True
    /// when the panel took the press (a card that is still running has no menu).
    pub(crate) fn export_context(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(p) = self
            .popup
            .as_ref()
            .filter(|p| p.kind == crate::popup::PopupKind::Export)
        else {
            return false;
        };
        let (anchor, inside) = (p.anchor, p.layout(g.size).rect.contains(x, y));
        if !inside {
            return false;
        }
        let Some(card) = self.export_card_at(g, x, y) else {
            return true;
        };
        if card.state == State::Running {
            return true;
        }
        let file = self
            .export_file_at(g, x, y)
            .map(|(_, kid)| kid)
            .filter(|k| !k.path.is_empty());
        let items = if let Some(kid) = file.filter(|_| card.state == State::Done) {
            self.export_file_items(&card, &kid, anchor)
        } else if card.state == State::Done {
            self.export_items(&card, anchor)
        } else {
            // A failed export wrote nothing: it can be made again with its settings, or go.
            vec![
                row(&card, anchor, "同じ設定で書き出しを開く", Row::Same),
                Item::gap(),
                row(&card, anchor, "一覧から削除", Row::Trash),
            ]
        };
        self.open_context(items, x, y);
        true
    }

    /// Runs a row of the menu.
    pub(crate) fn export_run_row(&mut self, a: ExportAct) -> bool {
        let ExportAct {
            job,
            row,
            anchor,
            file,
        } = a;
        // The menu took the panel's place: it comes back where it was.
        self.open_export(anchor);
        if let Some(file) = file {
            return self.export_file_row(job, row, file);
        }
        match row {
            Row::Listen => {
                let Some(path) = self.written_files(&job).into_iter().next() else {
                    return false;
                };
                if !self.export_file_exists(&path) {
                    return false;
                }
                self.browser.playing = Some(path.display().to_string());
                true
            }
            Row::Reveal => {
                let Some(path) = self
                    .written_files(&job)
                    .into_iter()
                    .find(|p| self.export_file_exists(p))
                else {
                    return false;
                };
                // Only the real window opens the OS's file window.
                self.revealed.push(path);
                if self.os_files {
                    self.export_action(export::Act::Reveal(job));
                    return self.error.is_none();
                }
                true
            }
            Row::Rewrite => self.export_action(export::Act::Update(job)),
            Row::Import => {
                self.export_action(export::Act::Import(job));
                self.error.is_none()
            }
            Row::Trash => self.export_action(export::Act::DeleteConfirmed(job)),
            Row::Same => {
                if !self.export_like(&job) {
                    return false;
                }
                self.refresh_popup();
                true
            }
        }
    }

    /// Runs a row of the menu of one file of an export.
    fn export_file_row(&mut self, job: String, row: Row, file: String) -> bool {
        let path = PathBuf::from(&file);
        match row {
            Row::Listen => {
                if !self.export_file_exists(&path) {
                    return false;
                }
                self.browser.playing = Some(path.display().to_string());
                true
            }
            Row::Reveal => {
                if !self.export_file_exists(&path) {
                    return false;
                }
                // Only the real window opens the OS's file window.
                self.revealed.push(path);
                if self.os_files {
                    self.export_run("export.reveal_os", json!({ "job": job, "file": file }));
                    return self.error.is_none();
                }
                true
            }
            Row::Rewrite => {
                // Only this file is written again; the others stay as they are.
                let args = json!({ "job": job, "file": file, "background": true });
                self.export_run("export.update", args);
                self.reload_exports();
                true
            }
            Row::Import => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command("export.import", json!({ "job": job, "file": file }));
                self.error.is_none()
            }
            // The card's rows: they are about every file.
            Row::Same | Row::Trash => self.export_run_row(ExportAct {
                job,
                row,
                anchor: self.popup.as_ref().map_or(Rect::default(), |p| p.anchor),
                file: None,
            }),
        }
    }
}
