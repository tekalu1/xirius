//! UI-351: deleting a draft from the start screen's 「最近の曲」. Only a row marked
//! 「（保存前）」 can go. A right press on it, the Delete key on the list, or the menu key opens
//! the way to a question (the one `unsaved.rs` draws); answering 「削除」 hands the folder to
//! the files' worker, which refuses a draft some other process still holds, and the row goes
//! once the keeper's list drops it.

use std::path::{Path, PathBuf};

use super::*;
use crate::ancillary_io::Ticket;
use crate::context::{Item, Run};
use crate::unsaved::DraftAsk;

/// What the worker says of a draft another process has open.
const HELD: &str = "ほかの Xirius で開いています";

/// A deletion in the worker's hands.
pub(crate) struct Removal {
    dir: PathBuf,
    ticket: Ticket<()>,
}

/// Is this key, with these modifiers, the one that deletes? Delete everywhere; Backspace only
/// when it has no search text to edit; Cmd+Backspace on a Mac (where the Mod key reaches here as
/// Ctrl), the way Finder deletes.
pub(crate) fn is_delete(key: &str, mods: Mods, search_empty: bool, mac: bool) -> bool {
    let plain = !mods.ctrl && !mods.alt && !mods.shift;
    match key {
        "delete" => plain,
        "backspace" if mac && mods.ctrl => !mods.alt && !mods.shift,
        "backspace" => plain && search_empty,
        _ => false,
    }
}

impl Home {
    /// The draft a row of the list opens, if it is one.
    fn draft_of(&self, act: &Action) -> Option<&RecentSong> {
        let Action::Pick(dir) = act else {
            return None;
        };
        self.songs
            .iter()
            .find(|s| s.draft && s.dir.as_path() == Path::new(dir))
    }

    /// The draft the cursor's row opens.
    pub(crate) fn chosen_draft(&self) -> Option<&RecentSong> {
        self.chosen().and_then(|act| self.draft_of(&act))
    }
}

impl Studio {
    /// Asks whether the cursor's draft goes. False when the cursor is not on a draft.
    pub(crate) fn home_ask_delete(&mut self) -> bool {
        let song = self.home.as_ref().and_then(Home::chosen_draft).cloned();
        self.ask_draft_delete(song)
    }

    /// Asks whether the draft in this folder goes (the menu's row).
    pub(crate) fn home_ask_delete_of(&mut self, dir: &Path) -> bool {
        let song = self
            .home
            .as_ref()
            .and_then(|h| h.songs.iter().find(|s| s.draft && s.dir == dir))
            .cloned();
        self.ask_draft_delete(song)
    }

    fn ask_draft_delete(&mut self, song: Option<RecentSong>) -> bool {
        let Some(song) = song.filter(|_| self.draft_removal.is_none()) else {
            return false;
        };
        self.ask_unsaved(crate::unsaved::Pending::DeleteDraft(DraftAsk {
            dir: song.dir,
            name: song.name,
            ago: crate::popup::ago(xirius_core::store::now_ms(), song.at_ms),
        }));
        true
    }

    /// The cursor goes to a row of the list (the shots'): the list has the focus.
    pub(crate) fn home_choose(&mut self, row: usize) {
        if let Some(home) = self.home.as_mut() {
            home.pick.selected = row;
            home.focus = Focus::List;
            home.touched = true;
        }
    }

    /// The middle of a row, as the layout puts it (the shots').
    pub(crate) fn home_row_centre(&self, g: &Geometry, row: usize) -> Option<(f32, f32)> {
        let layout = self.home_layout(g)?;
        let r = layout
            .rows
            .iter()
            .find(|(k, _)| *k == row)
            .map(|(_, r)| *r)?;
        Some((r.x + r.w / 2.0, r.y + r.h / 2.0))
    }

    /// The menu of a draft row, at a point of the window: one row, 「削除」.
    fn open_draft_menu(&mut self, dir: PathBuf, x: f32, y: f32) {
        let key = xirius_prefs::chord::Chord::new(0, "Delete")
            .display(xirius_prefs::chord::Os::current());
        let item = Item {
            key,
            ..Item::new("削除", Run::DeleteDraft(dir))
        };
        self.open_context(vec![item], x, y);
    }

    /// A right press on the start screen: a draft row takes the cursor and opens its menu.
    /// Anywhere else nothing opens, and the press goes no further (not to the song under).
    pub(crate) fn home_right_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if self.popup.is_some() || self.unsaved.is_some() || self.draft_removal.is_some() {
            return false;
        }
        let Some(Spot::Row(_, key)) = self.home_at(g, x, y) else {
            return false;
        };
        self.home_tick();
        let Some(home) = self.home.as_mut() else {
            return false;
        };
        let Some(i) = home
            .pick
            .nav()
            .iter()
            .position(|r| row_key(r.act.as_ref()) == key)
        else {
            return false;
        };
        home.pick.selected = i;
        home.focus = Focus::List;
        home.touched = true;
        let Some(dir) = home.chosen_draft().map(|s| s.dir.clone()) else {
            return false;
        };
        self.open_draft_menu(dir, x, y);
        true
    }

    /// The menu key, or Shift+F10: the cursor's draft opens its menu under its row. Any other
    /// row has no menu, and the key is still the list's.
    pub(crate) fn home_key_menu(&mut self, g: &Geometry) -> bool {
        let Some(home) = self.home.as_ref() else {
            return false;
        };
        let Some(dir) = home.chosen_draft().map(|s| s.dir.clone()) else {
            return true;
        };
        let selected = home.pick.selected;
        let at = self
            .home_layout(g)
            .and_then(|l| l.rows.iter().find(|(k, _)| *k == selected).map(|(_, r)| *r));
        let (x, y) = at.map_or((0.0, 0.0), |r| (r.x + 24.0, r.bottom()));
        self.open_draft_menu(dir, x, y);
        true
    }

    /// The question was answered 「削除」: the worker looks for a lock that is alive and removes
    /// the folder the way an empty draft's clean-up does.
    pub(crate) fn begin_draft_delete(&mut self, dir: PathBuf) {
        if self.draft_removal.is_some() || !self.keep.is_draft_dir(&dir) {
            return;
        }
        let path = dir.clone();
        let work = move || {
            if xirius_core::recovery::held(&path) {
                return Err(HELD.to_string());
            }
            crate::keep::remove_draft_folder(&path)
        };
        match crate::ancillary_io::submit(work) {
            Ok(ticket) => self.draft_removal = Some(Removal { dir, ticket }),
            Err(e) => self.warn(format!("曲を削除できません（{e}）"), None),
        }
    }

    /// The worker has answered: the keeper forgets the folder and the row that moves up to
    /// its place is chosen once the list drops it; or the row stays and the person is told.
    pub(crate) fn poll_draft_removal(&mut self) -> bool {
        let Some(result) = self.draft_removal.as_ref().and_then(|r| r.ticket.poll()) else {
            return false;
        };
        let Some(removal) = self.draft_removal.take() else {
            return false;
        };
        match result {
            Ok(()) => {
                self.keep.forget(&removal.dir);
                if let Some(home) = self.home.as_mut() {
                    let at = home.pick.nav().iter().position(
                        |r| matches!(&r.act, Some(Action::Pick(d)) if Path::new(d) == removal.dir),
                    );
                    home.after = at.map(|i| (removal.dir, i));
                }
            }
            Err(e) => self.warn(format!("曲を削除できません（{e}）"), None),
        }
        true
    }
}
