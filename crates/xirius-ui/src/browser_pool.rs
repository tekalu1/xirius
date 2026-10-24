//! The browser's song-files tab (BRW-004): every file the song uses, and the recordings no clip
//! uses, grouped by what made them, with the clips that use each and the files that are gone.

use std::time::{Duration, Instant};

use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::ClipBody;
use xirius_design::{FontSize, Radius, Space, Weight};
use xirius_session::file_ops::Entry;

use crate::browser::{BRow, Browser, RailTab, text_width};
use crate::draw::{Align, Pen};
use crate::studio::Rect;

/// The kinds in the order the list shows them, with their headings and the icons their rows
/// have in the name order.
pub const KINDS: [(&str, &str, &str); 4] = [
    ("record", "録音", "mic"),
    ("import", "インポート", "file"),
    ("bounce", "バウンス", "wave"),
    ("freeze", "フリーズ", "freeze"),
];

/// How often the list looks at the disk again when the song did not change.
const REREAD: Duration = Duration::from_secs(1);

/// What a missing file says at the end of its row.
pub const MISSING: &str = "見つかりません";
/// The chip of a file outside the song's folders.
pub const EXTERNAL: &str = "外部";

/// The key of a kind's heading.
pub fn heading_key(kind: &str) -> String {
    format!("SongFiles/{kind}")
}

impl Browser {
    /// The rows of the song-files tab: a heading for every kind that has a file, and the files
    /// under the open ones; in the name order, only the files, each with its kind's icon. A
    /// search keeps the files whose name or path has it and opens every heading with a hit.
    /// Nothing when there is nothing (the tab says nothing then).
    pub(crate) fn pool_rows(&self) -> Vec<BRow> {
        let q = self.query().to_lowercase();
        let hit = |e: &&Entry| {
            q.is_empty()
                || e.file.to_lowercase().contains(&q)
                || e.path.to_string_lossy().to_lowercase().contains(&q)
        };
        let by_name = |files: &mut Vec<&Entry>| {
            files.sort_by_key(|e| (file_name(e).to_lowercase(), e.file.clone()));
        };
        let mut rows = Vec::new();
        if !self.pool_view.grouped() {
            let mut files: Vec<&Entry> = self.pool.iter().filter(hit).collect();
            by_name(&mut files);
            for e in files {
                self.pool_file_rows(&mut rows, e, true);
            }
            return rows;
        }
        for (kind, name, _) in KINDS {
            let mut files: Vec<&Entry> = self
                .pool
                .iter()
                .filter(|e| e.kind == kind)
                .filter(hit)
                .collect();
            if files.is_empty() {
                continue;
            }
            by_name(&mut files);
            let key = heading_key(kind);
            let open = !q.is_empty() || !self.is_folded(&key);
            rows.push(BRow::Group {
                key,
                name,
                count: files.len(),
                open,
            });
            if !open {
                continue;
            }
            for e in files {
                self.pool_file_rows(&mut rows, e, false);
            }
        }
        rows
    }

    /// A file's row, and its audition under it when it is selected and there.
    fn pool_file_rows(&self, rows: &mut Vec<BRow>, e: &Entry, flat: bool) {
        let key = e.path.to_string_lossy().into_owned();
        let selected = self.selected_file.as_deref() == Some(&key);
        let icon = if !e.exists {
            "alert"
        } else if flat {
            KINDS
                .iter()
                .find(|k| k.0 == e.kind)
                .map_or("audio", |k| k.2)
        } else {
            "audio"
        };
        rows.push(BRow::Pool {
            key: key.clone(),
            file: e.file.clone(),
            name: file_name(e),
            clips: (e.clips + e.devices > 0 || e.frozen == 0).then_some(e.clips + e.devices),
            missing: !e.exists,
            external: e.external,
            selected,
            icon,
        });
        if selected && e.exists {
            rows.push(BRow::Audition {
                playing: self.playing.as_deref() == Some(&key),
                key,
                depth: 0,
            });
        }
    }

    /// The files of the song that are not found.
    pub fn pool_missing(&self) -> usize {
        self.pool.iter().filter(|e| !e.exists).count()
    }

    /// The pool's file under a row's key.
    pub fn pool_entry(&self, key: &str) -> Option<&Entry> {
        self.pool.iter().find(|e| e.path.to_string_lossy() == key)
    }

    /// Selecting a file of the pool: one that is there starts its audition, like on the files
    /// tab; a missing one only stops the one that played.
    pub fn select_pool(&mut self, key: &str, missing: bool) {
        self.pool_heading = None;
        if missing {
            self.selected_file = Some(key.to_string());
            self.playing = None;
        } else {
            self.select_file(key);
        }
    }

    /// The count's chip at a row's end, when the row has one.
    pub fn pool_count_rect(&self, row: Rect, r: &BRow) -> Option<Rect> {
        let BRow::Pool {
            clips: Some(n),
            missing: false,
            ..
        } = r
        else {
            return None;
        };
        let w = chip_w(&n.to_string(), true);
        Some(Rect::new(
            row.right() - Space::X2.px() - w,
            row.y + (row.h - CHIP_H) / 2.0,
            w,
            CHIP_H,
        ))
    }
}

/// A file's name without its folders.
fn file_name(e: &Entry) -> String {
    e.path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| e.file.clone())
}

const CHIP_H: f32 = 16.0;

fn chip_w(s: &str, digits: bool) -> f32 {
    let size = if digits {
        FontSize::Micro
    } else {
        FontSize::Caption
    };
    (text_width(s, size) + Space::X2.px() * 1.5).max(18.0)
}

/// A file row of the pool: its icon, its name, and at the end whether it is missing, or the
/// external chip and the count of the clips that use it.
pub fn paint_row(pen: &mut Pen, browser: &Browser, row: &BRow, r: Rect, hot: bool) {
    let BRow::Pool {
        name,
        clips,
        missing,
        external,
        selected,
        icon,
        ..
    } = row
    else {
        return;
    };
    let c = pen.c;
    let pad = Space::X2.px();
    let inner = Rect::new(r.x + Space::X1.px(), r.y, r.w - Space::X1.px() * 2.0, r.h);
    if *selected {
        pen.round(inner, c.sel, Radius::S);
    } else if hot {
        pen.round(inner, c.hover, Radius::S);
    }
    let ink = if *missing { c.ink_alert } else { c.ink_strong };
    // Under a heading the row stands in from it; in the name order there is none.
    let x = r.x
        + Browser::indent(0)
        + if browser.pool_view.grouped() {
            18.0
        } else {
            0.0
        };
    pen.icon(
        icon,
        Rect::new(x, r.y + (r.h - 14.0) / 2.0, 14.0, 14.0),
        ink,
    );
    let chip_bg = if *selected { c.surface_1 } else { c.surface_3 };
    // The end of the row, from the right.
    let mut end = r.right() - pad;
    if *missing {
        let w = text_width(MISSING, FontSize::Caption);
        pen.text(
            MISSING,
            FontSize::Caption,
            Weight::Medium,
            c.ink_alert,
            Rect::new(end - w, r.y, w, r.h),
            Align::Right,
        );
        end -= w + pad;
    } else {
        if let Some(chip) = browser.pool_count_rect(r, row) {
            pen.round(chip, chip_bg, Radius::Clip);
            pen.digits(
                &clips.unwrap_or(0).to_string(),
                FontSize::Micro,
                Weight::Semibold,
                c.ink_strong,
                chip,
                Align::Center,
            );
            end = chip.x - Space::X1.px();
        }
        if *external {
            let w = chip_w(EXTERNAL, false);
            let chip = Rect::new(end - w, r.y + (r.h - CHIP_H) / 2.0, w, CHIP_H);
            pen.round(chip, chip_bg, Radius::Clip);
            pen.text(
                EXTERNAL,
                FontSize::Caption,
                Weight::Medium,
                c.ink_strong,
                chip,
                Align::Center,
            );
            end = chip.x - Space::X1.px();
        }
        end -= Space::X1.px();
    }
    let name_x = x + 14.0 + pad;
    let name_r = Rect::new(name_x, r.y, (end - name_x).max(0.0), r.h);
    pen.mask(name_r, |pen| {
        pen.text(
            name,
            FontSize::Small,
            Weight::Regular,
            ink,
            name_r,
            Align::Left,
        );
    });
}

impl crate::studio::Studio {
    /// The pool follows the song and the disk: read again when the song changed, or a second
    /// after it was read. It is read with the tab closed too, so the rail can say a file is
    /// missing. True when it changed.
    pub fn follow_pool(&mut self) -> bool {
        let project = self.project();
        let at = std::sync::Arc::as_ptr(&project) as usize;
        let fresh = self
            .browser
            .pool_read
            .is_some_and(|(p, t)| p == at && t.elapsed() < REREAD);
        if fresh {
            return false;
        }
        let dir = self.song().dir();
        let mut pool = xirius_session::file_ops::pool(&project, dir.as_deref());
        // The song's folder and the file's place in it may part with different separators; the
        // tip shows the path the OS would write.
        for e in &mut pool {
            e.path = e.path.components().collect();
        }
        self.browser.pool_read = Some((at, Instant::now()));
        if pool == self.browser.pool {
            return false;
        }
        self.browser.pool = pool;
        true
    }

    /// The pool's file a clip plays, as its row's key.
    pub(crate) fn pool_key_of_clip(&self, track: TrackId, clip: ClipId) -> Option<String> {
        let project = self.project();
        let c = project
            .tracks
            .get(&track)?
            .clips
            .iter()
            .find(|c| c.id == clip)?;
        let ClipBody::Audio { file, .. } = &c.body else {
            return None;
        };
        self.browser
            .pool
            .iter()
            .find(|e| &e.file == file)
            .map(|e| e.path.to_string_lossy().into_owned())
    }

    /// Pressing an audio clip while the tab shows selects the row of the file it plays (without
    /// starting its audition). True when the selection moved.
    pub(crate) fn follow_pool_clip(&mut self) -> bool {
        if self.browser.pool_clip == self.selected_clip {
            return false;
        }
        self.browser.pool_clip = self.selected_clip;
        if !self.pool_shown() {
            return false;
        }
        let Some(key) = self
            .selected_clip
            .and_then(|(t, c)| self.pool_key_of_clip(t, c))
        else {
            return false;
        };
        if self.browser.selected_file.as_deref() == Some(&key) {
            return false;
        }
        self.browser.selected_file = Some(key);
        self.browser.playing = None;
        self.browser.pool_heading = None;
        true
    }

    /// The tip of the tab's row or count under the pointer: a row says where its file is, the
    /// count says what it counts, the 「…」 what it holds.
    pub(crate) fn pool_tip(
        &self,
        g: &crate::studio::Geometry,
        hit: crate::studio::Hit,
    ) -> Option<String> {
        use crate::browser::BrowserHit;
        let crate::studio::Hit::Browser(b) = hit else {
            return None;
        };
        if !self.pool_shown() {
            return None;
        }
        let row = |i: usize| {
            self.browser
                .layout(g.panel)
                .rows
                .get(i)
                .map(|p| p.row.clone())
        };
        match b {
            BrowserHit::Pool(i) => match row(i)? {
                BRow::Pool { key, .. } => Some(key),
                _ => None,
            },
            BrowserHit::PoolCount(i) => match row(i)? {
                BRow::Pool { clips: Some(n), .. } => Some(format!("使っているクリップ {n}")),
                _ => None,
            },
            BrowserHit::PoolMenu => Some("メニュー".to_string()),
            _ => None,
        }
    }

    /// The keys of the tab while the browser has them: ↑↓ go from row to row (a heading is a
    /// stop too), ← folds a heading or goes up to it, → opens it, F2 renames the file.
    pub(crate) fn pool_key(
        &mut self,
        g: &crate::studio::Geometry,
        key: &str,
        mods: crate::tools::Mods,
    ) -> bool {
        if !(self.pool_shown() || (self.browser.open && self.browser.tab == RailTab::Files))
            || !self.input_map.browser_focus
            || self.browser.typing
            || self.popup.is_some()
            || mods.ctrl
            || mods.alt
        {
            return false;
        }
        // Shift+F10 and the menu key: the browser has no keys of its own in the table, so the
        // tab opens its row's menu here.
        if self.pool_shown() && ((key == "f10" && mods.shift) || (key == "menu" && !mods.shift)) {
            return self.pool_key_menu(g);
        }
        if mods.shift {
            return false;
        }
        let l = self.browser.layout(g.panel);
        let stops: Vec<usize> = l
            .rows
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                matches!(
                    p.row,
                    BRow::Group { .. } | BRow::Pool { .. } | BRow::File { .. } | BRow::Dir { .. }
                )
            })
            .map(|(i, _)| i)
            .collect();
        let heading = self.browser.pool_heading.clone();
        let file = self.browser.selected_file.clone();
        let at = stops.iter().position(|&i| match &l.rows[i].row {
            BRow::Group { key, .. } | BRow::Dir { key, .. } => heading.as_deref() == Some(key),
            BRow::Pool { key, .. } | BRow::File { key, .. } => {
                heading.is_none() && file.as_deref() == Some(key)
            }
            _ => false,
        });
        match key {
            "up" | "down" => {
                let Some(last) = stops.len().checked_sub(1) else {
                    return false;
                };
                let next = match (at, key == "down") {
                    (None, true) => 0,
                    (None, false) => last,
                    (Some(i), true) => (i + 1).min(last),
                    (Some(i), false) => i.saturating_sub(1),
                };
                self.pool_stop(g, stops[next]);
                true
            }
            "left" | "right" => {
                let Some(i) = at.map(|a| stops[a]) else {
                    return false;
                };
                match &l.rows[i].row {
                    BRow::Dir { key: k, open, .. } => {
                        if *open == (key == "left") {
                            self.browser.toggle_dir(k);
                        }
                    }
                    BRow::Group { key: k, open, .. } => {
                        if *open == (key == "left") {
                            let k = k.clone();
                            self.browser.toggle_group(&k);
                        }
                    }
                    BRow::Pool { .. } if key == "left" => {
                        if let Some(h) = (0..i)
                            .rev()
                            .find(|&j| matches!(l.rows[j].row, BRow::Group { .. }))
                        {
                            self.pool_stop(g, h);
                        }
                    }
                    _ => {}
                }
                let l = self.browser.layout(g.panel);
                self.browser.clamp_scroll(&l);
                true
            }
            "f2" => {
                let Some(i) = at.map(|a| stops[a]) else {
                    return false;
                };
                let BRow::Pool { missing: false, .. } = l.rows[i].row else {
                    return false;
                };
                let Some(rows) = self.pool_row_items(g, i) else {
                    return false;
                };
                let r = self.browser.row_rect(&l, &l.rows[i]);
                self.open_context(rows, r.x + r.w / 2.0, r.y + r.h / 2.0);
                self.context_choose(g, "名前を変更")
            }
            _ => false,
        }
    }

    /// Shift+F10 and the menu key on the tab: the selected file's menu, at the middle of its row.
    pub(crate) fn pool_key_menu(&mut self, g: &crate::studio::Geometry) -> bool {
        let l = self.browser.layout(g.panel);
        let Some(i) = l.rows.iter().position(|p| {
            matches!(&p.row, BRow::Pool { key, .. }
                if self.browser.pool_heading.is_none()
                    && self.browser.selected_file.as_deref() == Some(key))
        }) else {
            return false;
        };
        let Some(rows) = self.pool_row_items(g, i) else {
            return false;
        };
        let r = self.browser.row_rect(&l, &l.rows[i]);
        self.popup = None;
        self.open_context(rows, r.x + r.w / 2.0, r.y + r.h / 2.0);
        true
    }

    /// Puts the keys' place on row `i` (a heading or a file) and scrolls it into the list.
    fn pool_stop(&mut self, g: &crate::studio::Geometry, i: usize) {
        let l = self.browser.layout(g.panel);
        let Some(p) = l.rows.get(i) else {
            return;
        };
        match &p.row {
            BRow::Group { key, .. } => {
                self.browser.pool_heading = Some(key.clone());
                self.browser.selected_file = None;
                self.browser.playing = None;
            }
            BRow::Dir { key, .. } => {
                self.browser.pool_heading = Some(key.clone());
                self.browser.selected_file = None;
                self.browser.playing = None;
            }
            BRow::Pool { key, missing, .. } => self.browser.select_pool(key, *missing),
            BRow::File { key, .. } => {
                self.browser.pool_heading = None;
                self.browser.select_file(key);
            }
            _ => return,
        }
        self.follow_audition();
        let l = self.browser.layout(g.panel);
        let Some(p) = l
            .rows
            .iter()
            .find(|p| match (&p.row, &self.browser.pool_heading) {
                (BRow::Group { key, .. } | BRow::Dir { key, .. }, Some(h)) => key == h,
                (BRow::Pool { key, .. } | BRow::File { key, .. }, None) => {
                    self.browser.selected_file.as_deref() == Some(key)
                }
                _ => false,
            })
        else {
            return;
        };
        if p.y < self.browser.scroll {
            self.browser.scroll = p.y;
        } else if p.y + p.h > self.browser.scroll + l.list.h {
            self.browser.scroll = p.y + p.h - l.list.h;
        }
        self.browser.clamp_scroll(&l);
    }

    /// Whether the song-files tab is the one the browser shows.
    pub(crate) fn pool_shown(&self) -> bool {
        self.browser.open && self.browser.tab == RailTab::SongFiles
    }
}

#[cfg(test)]
mod tests;
