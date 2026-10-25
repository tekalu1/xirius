//! Files out of the browser and out of the OS's file window (UI-017, UI-045, UI-046, BRW-005).
//!
//! A file row of the browser dragged onto the arrangement is put there as a drop from the OS
//! would be ([`Studio::drop_files`]): over an audio track's row onto that track, anywhere else
//! on a track of its own. 「ファイルから…」 asks for audio or MIDI files and puts each at the
//! playhead on a new track; the browser's plus asks for a folder and gives it a tab.

use std::path::{Path, PathBuf};

use xirius_design::{FontSize, Radius, Space, Weight};

use crate::browser::{BRow, BrowserHit};
use crate::draw::{Align, Pen};
use crate::studio::{FileRequest, Geometry, Hit, Rect, Studio};

/// How far the pointer goes before a press on a file row becomes a drag.
const SLOP: f32 = 4.0;

/// A file row held with the pointer.
#[derive(Clone, Debug, PartialEq)]
pub struct FileDrag {
    pub path: PathBuf,
    pub name: String,
    pub from: (f32, f32),
    pub at: (f32, f32),
    /// The pointer went past [`SLOP`]: letting go drops it.
    pub moved: bool,
}

/// What letting go of a dragged file does there, as the chip says it.
pub fn drop_label(studio: &Studio, g: &Geometry, d: &FileDrag) -> Option<String> {
    let (x, y) = d.at;
    if studio.start_shown() || !(g.lanes.contains(x, y) || g.column.contains(x, y)) {
        return None;
    }
    if crate::drop_audio::is_audio(&d.path)
        && let Some(t) = studio.audio_track_at(g, x, y)
    {
        let name = &studio.project().tracks[&t].name;
        return Some(format!("{name} に置く"));
    }
    Some("新しいトラックに置く".into())
}

impl Studio {
    /// After a press on the browser: a file row starts a drag, and a double press on a folder
    /// goes into it.
    pub fn file_grab(&mut self, g: &Geometry, hit: Hit, x: f32, y: f32, clicks: usize) -> bool {
        let Hit::Browser(hit) = hit else {
            return false;
        };
        let l = self.browser.layout(g.panel);
        match hit {
            BrowserHit::File(i) => {
                let Some(BRow::File { key, name, .. }) = l.rows.get(i).map(|p| p.row.clone())
                else {
                    return false;
                };
                self.browser.file_drag = Some(FileDrag {
                    path: PathBuf::from(key),
                    name,
                    from: (x, y),
                    at: (x, y),
                    moved: false,
                });
                true
            }
            // A file of the song-files tab drags like one of the files tab (BRW-004); a missing
            // one has nothing to put.
            BrowserHit::Pool(i) | BrowserHit::PoolCount(i) => {
                let Some(BRow::Pool {
                    key,
                    name,
                    missing: false,
                    ..
                }) = l.rows.get(i).map(|p| p.row.clone())
                else {
                    return false;
                };
                self.browser.file_drag = Some(FileDrag {
                    path: PathBuf::from(key),
                    name,
                    from: (x, y),
                    at: (x, y),
                    moved: false,
                });
                true
            }
            BrowserHit::Dir(i) if clicks >= 2 => {
                let Some(BRow::Dir { key, .. }) = l.rows.get(i).map(|p| p.row.clone()) else {
                    return false;
                };
                // The first press of the two opened it in the tree: it closes again.
                self.browser.open_dirs.remove(&key);
                self.browser.enter_dir(&key);
                true
            }
            _ => false,
        }
    }

    /// The pointer moved while a file row is held.
    pub fn file_move(&mut self, x: f32, y: f32) -> bool {
        let Some(d) = &mut self.browser.file_drag else {
            return false;
        };
        d.at = (x, y);
        if (x - d.from.0).hypot(y - d.from.1) > SLOP {
            d.moved = true;
        }
        true
    }

    /// Letting go of a held file row: put where the pointer is, when that is the arrangement.
    pub fn file_up(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(d) = self.browser.file_drag.take() else {
            return false;
        };
        if d.moved && !self.start_shown() && (g.lanes.contains(x, y) || g.column.contains(x, y)) {
            self.drop_files(g, x, y, &[d.path]);
        }
        true
    }

    /// What the OS's file window gave for a request of the browser or of 「ファイルから…」.
    pub fn finish_picked(&mut self, request: FileRequest, paths: &[PathBuf]) {
        match request {
            FileRequest::Place => {
                for dir in paths {
                    self.browser.add_place_later(dir.clone());
                }
            }
            FileRequest::Audio => self.put_files(paths),
            FileRequest::Locate(file) => {
                if let Some(to) = paths.first() {
                    let to = to.to_string_lossy();
                    self.arrange_run(
                        "file.locate",
                        serde_json::json!({ "path": file, "new_path": to }),
                    );
                }
            }
            FileRequest::LocateFolder => {
                if let Some(folder) = paths.first() {
                    let folder = folder.to_string_lossy();
                    self.arrange_run("file.locate", serde_json::json!({ "folder": folder }));
                }
            }
            FileRequest::MediaFolder => {
                if let Some(folder) = paths.first() {
                    self.set_media_folder(folder);
                }
            }
            _ => {}
        }
    }

    /// Each file at the playhead on a track of its own (audio clips, MIDI clips).
    pub fn put_files(&mut self, paths: &[PathBuf]) {
        let at = self.playhead;
        let mut placed = Vec::new();
        for path in paths {
            let name = file_name(path);
            let done = if crate::drop_audio::is_audio(path) {
                self.import_audio(path, None, at)
            } else if crate::file_drop::is_midi(path) {
                self.drop_midi(path, at)
                    .map(|()| format!("{name} をインポートしました").into())
                    .map_err(|e| format!("「{name}」: {e}"))
            } else {
                Err(format!(
                    "「{name}」は読み込めません（音声ファイルと MIDI ファイル .mid に対応しています）"
                ))
            };
            match done {
                Ok(p) => placed.push(p),
                Err(e) => self.error = Some(e),
            }
        }
        let n = placed.len();
        self.tell_placed(placed, format!("ファイル {n} 個を置きました"));
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The dragged file at the pointer, with what letting go does.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let Some(d) = studio.browser.file_drag.as_ref().filter(|d| d.moved) else {
        return;
    };
    let c = pen.c;
    let label = drop_label(studio, g, d).unwrap_or_default();
    let pad = Space::X2.px();
    let icon = 16.0;
    let name_w = pen.measure(&d.name, FontSize::Small, Weight::Medium);
    let label_w = pen.measure(&label, FontSize::Caption, Weight::Regular);
    let w = pad * 2.0 + icon + Space::X1.px() * 2.0 + name_w.max(label_w);
    let h = if label.is_empty() { 28.0 } else { 44.0 };
    let x = (d.at.0 + 14.0).min(g.size.0 - w - Space::X1.px());
    let y = (d.at.1 + 16.0).min(g.size.1 - h - Space::X1.px());
    let r = Rect::new(x, y, w, h);
    pen.round(r, c.surface_3, Radius::S);
    let midi = crate::file_drop::is_midi(&d.path);
    pen.icon(
        if midi { "keys" } else { "wave" },
        Rect::new(r.x + pad, r.y + 6.0, icon, icon),
        c.ink_strong,
    );
    let text_x = r.x + pad + icon + Space::X1.px() * 2.0;
    pen.text(
        &d.name,
        FontSize::Small,
        Weight::Medium,
        c.ink_strong,
        Rect::new(text_x, r.y + 4.0, name_w, 20.0),
        Align::Left,
    );
    if !label.is_empty() {
        pen.text(
            &label,
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(text_x, r.y + 22.0, label_w, 18.0),
            Align::Left,
        );
    }
}

#[cfg(test)]
mod tests;
