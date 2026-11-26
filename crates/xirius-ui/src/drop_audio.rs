//! Audio files put into the song (UI-075, UI-017, UI-046, UI-045): dropped on the window,
//! dragged from the file browser, or picked with 「ファイルから…」.
//!
//! Each file is one `clip.import`, so one undo takes it away. Over an audio track's row the
//! first file goes onto that track, at the bar under the point; anywhere else each file gets a
//! track of its own. The file stays where it is: the song refers to it until it is saved
//! (BRW-003). What the import did with the tempo is said in one line. A clip whose file has a
//! tempo of its own, other than the song's, stays as it sounds; the line offers to make it
//! follow the song's (EDT-007).

use std::path::Path;

use serde_json::Value;
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::TrackKind;
use xirius_core::time::Tick;

use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

/// Files the song takes as audio clips.
pub fn is_audio(path: &Path) -> bool {
    xirius_session::decode::is_audio(path)
}

fn bpm(v: &Value) -> Option<String> {
    let b = v.as_f64()?;
    let r = (b * 100.0).round() / 100.0;
    Some(if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    })
}

/// What one file put into the song said, and the clip it placed when that clip could follow the
/// song's tempo.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub line: String,
    pub follows: Option<ClipId>,
}

impl From<String> for Placed {
    fn from(line: String) -> Self {
        Placed {
            line,
            follows: None,
        }
    }
}

/// The line said after an import, from what `clip.import` found.
pub fn said(name: &str, data: &Value) -> String {
    match (bpm(&data["bpm"]), bpm(&data["file_bpm"])) {
        (Some(set), _) => format!("「{name}」を置き、曲のテンポを {set} BPM にしました"),
        (None, Some(file)) if data["file_bpm"] != data["song_bpm"] => format!(
            "「{name}」を置きました（ファイルのテンポは {file} BPM。曲のテンポは変えていません）"
        ),
        _ => format!("「{name}」を置きました"),
    }
}

impl Studio {
    /// The audio track whose row is under a point in the lanes or the track column.
    pub fn audio_track_at(&self, g: &Geometry, x: f32, y: f32) -> Option<TrackId> {
        if self.start_shown() || !(g.lanes.contains(x, y) || g.column.contains(x, y)) {
            return None;
        }
        let row = self.tree().row_at(y - g.column.y + self.scroll.1)?;
        let RowKind::Track(t) = row.kind else {
            return None;
        };
        (self.project().tracks.get(&t)?.kind == TrackKind::Audio).then_some(t)
    }

    /// Audio files put in at a point (a drop, or a drag from the browser). True when one was.
    /// They go where the ghost showed them going (UI-381): onto the audio track under the
    /// point, or onto new tracks at the seam; nowhere over the ruler. The start is on the grid
    /// unless Shift is held; with Alt the files go end to end on one track. The first file's
    /// ghost stays until it is read (UI-386).
    pub fn import_audio_at(&mut self, g: &Geometry, x: f32, y: f32, paths: &[&Path]) -> bool {
        let (shift, alt) = (self.arrange.clip_drop.shift, self.arrange.clip_drop.alt);
        let aim = self.clip_drop_aim(g, x, y, shift);
        let first = self.place_audio(aim, paths, alt);
        if let Some((ticket, path)) = &first {
            self.clip_drop_land(*ticket, path.clone(), aim);
        }
        self.clip_drop_end();
        first.is_some()
    }

    /// Says what was put in: the one line, or `many` for more. When a clip could follow the
    /// song's tempo, the line stays with 「曲のテンポに合わせる」 for those clips.
    pub fn tell_placed(&mut self, placed: Vec<Placed>, many: String) {
        let placed: Vec<_> = placed.into_iter().filter(|p| !p.line.is_empty()).collect();
        if placed.is_empty() {
            self.browser_import_io.many = Some(many);
            return;
        }
        let line = match placed.len() {
            0 => return,
            1 => placed[0].line.clone(),
            _ => many,
        };
        let follows: Vec<ClipId> = placed.iter().filter_map(|p| p.follows).collect();
        if follows.is_empty() {
            self.tell(line);
        } else {
            self.offer(
                line,
                "曲のテンポに合わせる",
                crate::toast::Answer::Follow(follows),
            );
        }
    }

    /// One audio file as a clip at `at`, on `track` or on a new track; the clip is selected.
    /// Returns the line to say.
    pub fn import_audio(
        &mut self,
        path: &Path,
        track: Option<TrackId>,
        at: Tick,
    ) -> Result<Placed, String> {
        self.queue_browser_import(path.to_path_buf(), track, at, false)?;
        Ok(Placed {
            line: String::new(),
            follows: None,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
