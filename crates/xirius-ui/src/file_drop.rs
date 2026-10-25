//! Files dropped on the screen (UI-076). Each track of a MIDI file (each
//! channel of a format 0 file) becomes an instrument track playing the
//! built-in instrument, with its notes in a clip: at the bar under the drop in
//! the lanes, else at the song's head. A song without clips takes the file's
//! tempo and time signatures (`midi.import`).

use std::path::{Path, PathBuf};

use xirius_core::time::Tick;

use crate::studio::{Geometry, Studio};

/// A file the screen takes when dropped.
pub fn is_midi(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi"))
}

impl Studio {
    /// Where a drop at a point puts its clip: the start of the bar under it
    /// in the lanes, else the song's head.
    pub fn drop_tick(&self, g: &Geometry, x: f32, y: f32) -> Tick {
        if self.start_shown() || !g.lanes.contains(x, y) {
            return 0;
        }
        let tick = self.px_to_ticks(x - g.lanes.x + self.scroll.0).max(0);
        let tm = self.project().time_map();
        let (bar, _, _) = tm.to_bbt(tick);
        tm.bar_start(bar)
    }

    /// Files dragged over the window: the empty screen lights its zone.
    pub fn drop_hover(&mut self, over: bool) -> bool {
        let changed = self.drop_over != over || (!over && self.drop_dawproject.is_some());
        self.drop_over = over;
        if !over {
            self.drop_dawproject = None;
        }
        changed
    }

    /// Files dropped at a point. Each MIDI file makes a track; the last one
    /// made is selected. Other files are refused.
    pub fn drop_files(&mut self, g: &Geometry, x: f32, y: f32, paths: &[PathBuf]) -> bool {
        // A `.dawproject` opens as a new song (EXP-006), wherever it falls.
        if self.drop_dawproject_file(paths) {
            return true;
        }
        if self.groove_drop(g, x, y, paths) {
            return true;
        }
        self.drop_over = false;
        let at = self.drop_tick(g, x, y);
        // Audio files become audio clips (UI-075).
        let audio: Vec<&Path> = paths
            .iter()
            .map(PathBuf::as_path)
            .filter(|p| crate::drop_audio::is_audio(p))
            .collect();
        let placed = !audio.is_empty() && self.import_audio_at(g, x, y, &audio);
        let mut read = Vec::new();
        for path in paths.iter().filter(|p| !crate::drop_audio::is_audio(p)) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !is_midi(path) {
                self.error = Some(format!(
                    "「{name}」は読み込めません（音声ファイルと MIDI ファイル .mid に対応しています）"
                ));
                continue;
            }
            match self.drop_midi(path, at) {
                Ok(()) => read.push(name),
                Err(e) => self.error = Some(format!("「{name}」: {e}")),
            }
        }
        if !read.is_empty() && !placed {
            self.browser_import_io.many = Some(format!("{} をインポートしました", read.join("、")));
        }
        true
    }

    /// A MIDI file as new tracks of the built-in instrument at `at`; the first
    /// is selected.
    pub(crate) fn drop_midi(&mut self, path: &Path, at: Tick) -> Result<(), String> {
        self.queue_browser_import(path.to_path_buf(), None, at, true)
    }
}

#[cfg(test)]
mod tests;
