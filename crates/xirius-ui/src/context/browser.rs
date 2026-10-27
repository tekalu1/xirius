//! The right-click menus of the browser (S38–S43): an instrument, an effect, the blank
//! of a list, a file, a folder and a tab of the file tab.
//!
//! What the browser keeps (favourites, hidden things, the tabs) is the browser's own
//! and not the song's, so these rows change the browser and never the song's history.
//! Putting a file or an instrument on a track is the song's: one step, one undo, by the
//! path the drop and the double click take. A new name, a new folder and the recycle bin
//! change the disk (CTX-059); a rename of a file the song uses points its clips at the new
//! name, as one step. The file of a plug-in is not in the menu yet.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use xirius_core::id::TrackId;
use xirius_core::model::TrackKind;

use super::{Act, Item, Run};
use crate::browser::{BRow, BrowserHit, LibItem, RailTab};
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

/// Where an effect of the browser goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxTarget {
    /// The selected channel, after its last effect.
    Channel,
    /// A new bus that has the effect.
    NewBus,
    Master,
}

/// What a row of a browser menu does.
#[derive(Clone, Debug, PartialEq)]
pub enum BrowseAct {
    /// An instrument on a new track.
    UseNew(&'static str),
    /// An instrument in place of the selected track's.
    UseHere(&'static str),
    /// Makes an item a favourite (true) or takes it out of the favourites.
    Favourite(&'static str, bool),
    FavouriteFile(PathBuf, bool),
    /// Leaves an item out of the lists (true), or puts it back.
    Hide(&'static str, bool),
    Fx(FxTarget, &'static str),
    /// Opens (true) or folds every group of the list.
    Groups(bool),
    ShowHidden(bool),
    Rescan,
    /// A file on a track of its own.
    PutNew(PathBuf),
    /// An audio file on the selected audio track.
    PutHere(PathBuf),
    AddPlace(PathBuf),
    /// Takes a tab away, by its folder (or its name when it has none).
    RemovePlace(String),
    /// Opens the OS's file window on a file or a folder.
    Reveal(PathBuf),
    /// Selects the clips that use a file of the pool (as the song names it).
    SelectUsing(String),
    /// Asks where a missing file of the pool is now.
    Locate(String),
    /// Asks for a folder to look for every missing file in.
    LocateFolder,
    /// Opens the dialog that lists the recordings nothing uses and asks before they go.
    Cleanup,
}

/// How a command names a tab: its folder, or its name when it has none.
pub(super) fn place_key(name: &str, path: Option<&Path>) -> String {
    path.map_or_else(|| name.to_string(), |p| p.display().to_string())
}

fn act(label: &str, run: BrowseAct) -> Item {
    Item::act(label, "", Act::Browse(run))
}

/// A row that stays in its place and cannot be chosen.
fn dim(label: &str) -> Item {
    Item::act(label, "", Act::Open).enabled(false)
}

impl Studio {
    /// The selected track, when it is of `kind`.
    pub(super) fn selected_track_of(&self, kind: TrackKind) -> Option<TrackId> {
        let Some(RowKind::Track(t)) = self.selected_row else {
            return None;
        };
        (self.project().tracks.get(&t)?.kind == kind).then_some(t)
    }

    /// The item at row `i` of the list.
    fn browser_item(&self, g: &Geometry, i: usize) -> Option<LibItem> {
        let l = self.browser.layout(g.panel);
        match &l.rows.get(i)?.row {
            BRow::Item { item, .. } => Some(*item),
            _ => None,
        }
    }

    /// A right press on the browser: opens the menu of what is under the pointer. False
    /// when the point is not on the panel or has no menu.
    pub fn lib_context(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if !self.browser.open || !g.panel.contains(x, y) {
            return false;
        }
        // The inspector has no list to press (UI-312).
        if self.browser.tab == RailTab::Inspector {
            return false;
        }
        let l = self.browser.layout(g.panel);
        let hit = self.browser.hit(&l, x, y);
        if let Some(
            BrowserHit::File(i)
            | BrowserHit::Dir(i)
            | BrowserHit::Pool(i)
            | BrowserHit::PoolCount(i),
        ) = hit
        {
            let path = match l.rows.get(i).map(|p| &p.row) {
                Some(
                    BRow::File { key, .. }
                    | BRow::Dir { key, .. }
                    | BRow::Pool {
                        key,
                        missing: false,
                        ..
                    },
                ) => Some(PathBuf::from(key)),
                _ => None,
            };
            if let Some(path) = path
                && !self.menu_io_ready(&path, g, x, y)
            {
                if matches!(
                    l.rows.get(i).map(|p| &p.row),
                    Some(BRow::File { .. } | BRow::Pool { .. })
                ) {
                    self.browser.selected_file = Some(path.to_string_lossy().into_owned());
                }
                if matches!(l.rows.get(i).map(|p| &p.row), Some(BRow::Pool { .. })) {
                    self.browser.pool_heading = None;
                    if self.browser.playing.as_deref() != self.browser.selected_file.as_deref() {
                        self.browser.playing = None;
                    }
                }
                self.ctx = None;
                self.popup = None;
                return true;
            }
        }
        let files = self.browser.tab == RailTab::Files;
        let pool = self.browser.tab == RailTab::SongFiles;
        let rows = match hit {
            Some(BrowserHit::Pool(i) | BrowserHit::PoolCount(i)) => {
                let Some(rows) = self.pool_row_items(g, i) else {
                    return false;
                };
                rows
            }
            Some(BrowserHit::PoolMenu) => self.pool_menu_items(),
            Some(BrowserHit::Item(i) | BrowserHit::Fav(i)) => {
                let Some(item) = self.browser_item(g, i) else {
                    return false;
                };
                self.browser.selected_item = Some(item.name);
                self.browser.typing = false;
                if item.effect {
                    self.effect_items(item.name)
                } else {
                    self.instrument_items(item.name)
                }
            }
            Some(BrowserHit::File(i)) => {
                let Some(BRow::File { key, .. }) = l.rows.get(i).map(|p| &p.row) else {
                    return false;
                };
                let path = PathBuf::from(key);
                // The row is selected; the audition waits for the left press.
                self.browser.selected_file = Some(key.clone());
                self.file_items(&path)
            }
            Some(BrowserHit::Dir(i)) => {
                let Some(BRow::Dir { key, .. }) = l.rows.get(i).map(|p| &p.row) else {
                    return false;
                };
                self.dir_items(&PathBuf::from(key))
            }
            Some(BrowserHit::Loc(i) | BrowserHit::LocRemove(i)) => self.place_items(i),
            Some(BrowserHit::Group(_)) if !files && !pool => self.list_items(),
            None if !files && !pool && l.list.contains(x, y) => self.list_items(),
            _ => {
                self.cancel_browser_menu();
                return false;
            }
        };
        self.open_context(rows, x, y);
        true
    }

    /// S38: an instrument.
    fn instrument_items(&mut self, name: &'static str) -> Vec<Item> {
        let b = &self.browser;
        let (fav, hidden) = (b.favourites.contains(name), b.hidden.contains(name));
        let here = self.selected_track_of(TrackKind::Instrument).is_some();
        vec![
            act("新しいトラックで使う", BrowseAct::UseNew(name)),
            act(
                "選んだトラックのインストゥルメントにする",
                BrowseAct::UseHere(name),
            )
            .enabled(here),
            act("お気に入り", BrowseAct::Favourite(name, !fav)).on(fav),
            act(
                if hidden { "一覧に戻す" } else { "隠す" },
                BrowseAct::Hide(name, !hidden),
            ),
            Item::gap(),
            // The place of a plug-in's file is not known to the list yet (1.0).
            dim("ファイルの場所を表示"),
        ]
    }

    /// S39: an effect.
    fn effect_items(&mut self, name: &'static str) -> Vec<Item> {
        let b = &self.browser;
        let (fav, hidden) = (b.favourites.contains(name), b.hidden.contains(name));
        let found = self.effect_pick(name).is_some();
        let channel = matches!(self.selected_row, Some(RowKind::Track(_)));
        vec![
            act(
                "選んだチャンネルに追加",
                BrowseAct::Fx(FxTarget::Channel, name),
            )
            .enabled(found && channel),
            act("新しいバスに追加", BrowseAct::Fx(FxTarget::NewBus, name)).enabled(found),
            act("マスターに追加", BrowseAct::Fx(FxTarget::Master, name)).enabled(found),
            act("お気に入り", BrowseAct::Favourite(name, !fav)).on(fav),
            act(
                if hidden { "一覧に戻す" } else { "隠す" },
                BrowseAct::Hide(name, !hidden),
            ),
            Item::gap(),
            dim("ファイルの場所を表示"),
        ]
    }

    /// S40: the blank of an instrument or effect list, and a group's header.
    fn list_items(&mut self) -> Vec<Item> {
        let grouped = !self.browser.groups().is_empty();
        let shown = self.browser.show_hidden;
        let scanning = self.browser.scanning;
        vec![
            act("すべて開く", BrowseAct::Groups(true)).enabled(grouped),
            act("すべて畳む", BrowseAct::Groups(false)).enabled(grouped),
            self.ctx_do(
                "view.browser.hidden",
                Act::Browse(BrowseAct::ShowHidden(!shown)),
            )
            .on(shown),
            Item::gap(),
            self.ctx_do("plugin.rescan", Act::Browse(BrowseAct::Rescan))
                .enabled(!scanning),
        ]
    }

    /// S41: a file.
    fn file_items(&mut self, path: &Path) -> Vec<Item> {
        let audio = crate::drop_audio::is_audio(path);
        let placeable = audio || crate::file_drop::is_midi(path);
        let here = audio && self.selected_track_of(TrackKind::Audio).is_some();
        vec![
            act(
                "新しいトラックに置く",
                BrowseAct::PutNew(path.to_path_buf()),
            )
            .enabled(placeable),
            act(
                "選んだトラックに置く",
                BrowseAct::PutHere(path.to_path_buf()),
            )
            .enabled(here),
            act(
                "お気に入り",
                BrowseAct::FavouriteFile(path.to_path_buf(), !self.is_favourite_file(path)),
            )
            .on(self.is_favourite_file(path)),
            Item::gap(),
            act(
                "エクスプローラーで表示",
                BrowseAct::Reveal(path.to_path_buf()),
            ),
            self.rename_row(path),
            Item::gap(),
            self.trash_row(path, "ファイルをごみ箱へ"),
        ]
    }

    /// S42: a folder.
    fn dir_items(&mut self, path: &Path) -> Vec<Item> {
        let added = self.browser.places.iter().any(|p| p.path == path);
        vec![
            act("場所のタブに追加", BrowseAct::AddPlace(path.to_path_buf())).enabled(!added),
            act(
                "お気に入り",
                BrowseAct::FavouriteFile(path.to_path_buf(), !self.is_favourite_file(path)),
            )
            .on(self.is_favourite_file(path)),
            Item::gap(),
            act(
                "エクスプローラーで表示",
                BrowseAct::Reveal(path.to_path_buf()),
            ),
            Item::ask(
                &self.label_of("file.add.folder"),
                "file.add.folder",
                json!({ "path": path.display().to_string() }),
                "name",
                &self
                    .file_menu_info(path)
                    .map_or_else(|| "新しいフォルダー".into(), |i| i.name.clone()),
            ),
            self.rename_row(path),
            Item::gap(),
            self.trash_row(path, "フォルダーをごみ箱へ"),
        ]
    }

    /// The row that names a file or a folder anew (`file.rename`). A file's box has its name
    /// without the extension, which the command keeps.
    pub(super) fn rename_row(&mut self, path: &Path) -> Item {
        let shown = if self.file_menu_info(path).is_some_and(|i| i.dir) {
            path.file_name()
        } else {
            path.file_stem()
        };
        let shown = shown
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut row = Item::ask(
            "名前を変更",
            "file.rename",
            json!({ "path": path.display().to_string() }),
            "name",
            &shown,
        );
        row.enabled = self
            .file_menu_info(path)
            .is_some_and(|i| i.rename.is_none());
        if let Some(error) = self.file_menu_info(path).and_then(|i| i.rename.as_ref()) {
            row.reason = error.to_string();
        }
        row
    }

    /// 「ごみ箱へ」 and its one question (`file.trash`). What the song uses, and a folder that
    /// has it, stay in place dimmed, with the reason.
    pub(super) fn trash_row(&mut self, path: &Path, confirm: &str) -> Item {
        let args = json!({ "path": path.display().to_string() });
        let checked = self.file_menu_info(path).map_or_else(
            || Err(xirius_core::CmdError::conflict("読み込み中です")),
            |i| i.trash.clone().map_or(Ok(()), Err),
        );
        let confirm = Item::new(confirm, Run::Cmd("file.trash", args)).enabled(checked.is_ok());
        let mut row = Item::sub("ごみ箱へ", vec![confirm]);
        if let Err(e) = checked {
            row.reason = e.to_string();
        }
        row
    }

    /// A command on the files ran: the listings are read again, and what the browser keeps of
    /// the old path (the selected row, a favourite) follows a rename.
    pub(crate) fn after_file_command(&mut self, command: &str, data: &Value) {
        if command.starts_with("history.") {
            // An undo or a redo across a rename renamed the files back or again (BRW-013).
            let Some(renamed) = data["renamed"].as_array() else {
                return;
            };
            for r in renamed {
                self.after_file_command("file.rename", r);
            }
            return;
        }
        if matches!(
            command,
            "file.rename" | "file.trash" | "file.locate" | "file.collect" | "project.cleanup"
        ) {
            // The pool reads the disk again at once.
            self.browser.invalidate_pool();
        }
        if !matches!(command, "file.rename" | "file.trash" | "file.add.folder") {
            return;
        }
        crate::browser_files::forget();
        let Some(from) = data["from"].as_str().or(data["path"].as_str()) else {
            return;
        };
        let to = data["to"].as_str();
        if self.browser.selected_file.as_deref() == Some(from) {
            self.browser.selected_file = to.map(str::to_string);
        }
        let old = format!("file:{from}");
        if command == "file.rename"
            && let Some(to) = to
            && self.browser.favourites.contains(&old)
        {
            self.browser.toggle_favourite(&old);
            self.browser.toggle_favourite(&format!("file:{to}"));
        }
    }

    /// S43: a tab of the file tab (the song's cannot be renamed or taken away).
    fn place_items(&mut self, i: usize) -> Vec<Item> {
        let loc = self.browser.locations().into_iter().nth(i);
        let Some(loc) = loc else {
            return Vec::new();
        };
        let own = !loc.song;
        let remove = self.label_of("location.remove");
        let key = place_key(&loc.name, loc.path.as_deref());
        let mut rename = Item::ask(
            &self.label_of("location.rename"),
            "ui.place.rename",
            json!({ "index": i }),
            "name",
            &loc.name,
        );
        rename.enabled = own;
        vec![
            rename,
            match loc.path {
                Some(path) => act("エクスプローラーで表示", BrowseAct::Reveal(path)),
                None => dim("エクスプローラーで表示"),
            },
            Item::gap(),
            act(&remove, BrowseAct::RemovePlace(key)).enabled(own),
        ]
    }

    /// Whether a file or a folder is in the favourites.
    fn is_favourite_file(&self, path: &Path) -> bool {
        self.browser
            .favourites
            .contains(&format!("file:{}", path.display()))
    }

    /// The tab (counted with the song's first) a name or a folder stands for.
    fn place_index(&self, key: &str) -> Option<usize> {
        self.browser
            .locations()
            .iter()
            .position(|l| place_key(&l.name, l.path.as_deref()) == key || l.name == key)
    }

    /// Runs a row of a browser menu.
    pub(super) fn browse_run(&mut self, act: BrowseAct) -> bool {
        match act {
            BrowseAct::UseNew(name) => self.use_on_new_track(name),
            BrowseAct::UseHere(name) => match self.selected_track_of(TrackKind::Instrument) {
                Some(t) => self.use_on_track(t, name),
                None => false,
            },
            BrowseAct::Favourite(name, on) => {
                if self.browser.favourites.contains(name) != on {
                    self.browser.toggle_favourite(name);
                }
                true
            }
            BrowseAct::FavouriteFile(path, on) => {
                let key = format!("file:{}", path.display());
                if self.browser.favourites.contains(&key) != on {
                    self.browser.toggle_favourite(&key);
                }
                true
            }
            BrowseAct::Hide(name, hide) => {
                self.browser.set_hidden(name, hide);
                if hide {
                    if self.browser.selected_item == Some(name) {
                        self.browser.selected_item = None;
                    }
                    self.tell(format!("「{name}」を隠しました"));
                } else {
                    self.tell(format!("「{name}」を一覧に戻しました"));
                }
                true
            }
            BrowseAct::Fx(target, name) => self.browse_fx(target, name),
            BrowseAct::Groups(open) => {
                self.browser.fold_groups(open);
                true
            }
            BrowseAct::ShowHidden(on) => {
                self.browser.set_show_hidden(on);
                true
            }
            BrowseAct::Rescan => match self.start_plugin_scan() {
                Ok(()) => true,
                Err(e) => {
                    self.error = Some(e);
                    false
                }
            },
            BrowseAct::PutNew(path) => {
                self.put_files(&[path]);
                true
            }
            BrowseAct::PutHere(path) => {
                let Some(t) = self.selected_track_of(TrackKind::Audio) else {
                    return false;
                };
                match self.import_audio(&path, Some(t), self.playhead) {
                    Ok(placed) => {
                        self.tell_placed(vec![placed], String::new());
                        true
                    }
                    Err(e) => {
                        self.error = Some(e);
                        false
                    }
                }
            }
            BrowseAct::AddPlace(path) => self.browser.add_place_later(path),
            BrowseAct::RemovePlace(key) => match self.place_index(&key).map_or_else(
                || Err(format!("場所のタブ「{key}」はありません")),
                |i| self.browser.remove_place(i),
            ) {
                Ok(gone) => {
                    self.tell(format!("場所のタブ「{}」を外しました", gone.name));
                    true
                }
                Err(e) => {
                    self.error = Some(e);
                    false
                }
            },
            BrowseAct::SelectUsing(file) => {
                let ran = self
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    .arrange_run("select.using_file", json!({ "file": file }))
                    .is_some();
                if ran {
                    self.take_clips();
                }
                ran
            }
            BrowseAct::Locate(file) => {
                self.file_request = Some(crate::studio::FileRequest::Locate(file));
                true
            }
            BrowseAct::LocateFolder => {
                self.file_request = Some(crate::studio::FileRequest::LocateFolder);
                true
            }
            BrowseAct::Cleanup => self.open_cleanup(),
            BrowseAct::Reveal(path) => {
                // Only the real window opens the OS's file window.
                self.revealed.push(path.clone());
                if self.os_files
                    && let Err(e) = xirius_session::os::reveal(&path)
                {
                    self.error = Some(e);
                    return false;
                }
                true
            }
        }
    }

    /// An effect of the browser on the selected channel, on a new bus or on the master.
    fn browse_fx(&mut self, target: FxTarget, name: &str) -> bool {
        let Some(pick) = self.effect_pick(name) else {
            self.error = Some(format!("「{name}」はまだ見つかっていません"));
            return false;
        };
        let end =
            |s: &Studio, t: TrackId| s.project().tracks.get(&t).map_or(0, |x| x.inserts.len());
        match target {
            FxTarget::Channel => {
                let Some(RowKind::Track(t)) = self.selected_row else {
                    return false;
                };
                let at = end(self, t);
                self.add_effect(t, at, &pick)
            }
            FxTarget::Master => {
                let at = end(self, TrackId::MASTER);
                self.add_effect(TrackId::MASTER, at, &pick)
            }
            FxTarget::NewBus => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                let Some(data) = self.arrange_run(
                    "track.add.bus",
                    json!({ "plugin": pick.name, "format": pick.format, "uid": pick.uid }),
                ) else {
                    return false;
                };
                if let Some(t) = data["id"].as_str().and_then(|s| s.parse().ok()) {
                    self.select_row(Some(RowKind::Track(t)));
                }
                self.mixer
                    .recent
                    .retain(|p| p.format != pick.format || p.uid != pick.uid);
                self.mixer.recent.insert(0, pick);
                self.mixer.recent.truncate(8);
                true
            }
        }
    }
}
