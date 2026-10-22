//! `--script keep`: what keeping the song looks like (PRJ-001, PRJ-009, PRJ-010, PRJ-012,
//! PRJ-014, UI-078) — the card after a crash, the three marks beside the song name, and
//! the song menu with the songs that were open. Everything lives in a scratch folder.

use serde_json::json;

use crate::keep::Keep;
use crate::recovery_card::Button;
use crate::shot::{Act, Play, Shot, Target};
use crate::song::Song;
use crate::studio::{FileRequest, Studio};
use crate::topbar::BarButton;

/// Stage `n` of this script, played by [`stage`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Studio(stage), n)
}

pub const KEEP: &[Shot] = &[
    // A song opened after the process died: how many steps came back, and the last one.
    Shot {
        name: "k01-recovery-card",
        acts: &[staged(0)],
    },
    // The mark while the record is written (right after an edit).
    Shot {
        name: "k02-mark-saving",
        acts: &[staged(1)],
    },
    // The mark once it is written.
    Shot {
        name: "k03-mark-saved",
        acts: &[staged(2)],
    },
    // A song with no place to be recorded in: the mark says so.
    Shot {
        name: "k04-mark-unsaved",
        acts: &[staged(3)],
    },
    // The song menu: 新規, 開く, and the songs that were open.
    Shot {
        name: "k05-song-menu",
        acts: &[staged(4), Act::Press(Target::Top(BarButton::SongName))],
    },
];

/// Where [`KEEP`] keeps its songs.
pub fn dir() -> std::path::PathBuf {
    std::env::temp_dir().join("xirius-keep-shots")
}

/// A song in `dir` with `edits` operations after its save, left as a killed process leaves
/// it: the lock stays and names a process that is gone.
pub fn leave_crashed(dir: &std::path::Path, name: &str, edits: usize) {
    let mut song = Song::empty();
    let _ = song.run("project.rename", json!({ "name": name }));
    let _ = song.run("track.add.audio", json!({ "name": "Vocal" }));
    let _ = song.save_as(dir);
    for (i, track) in ["Bass", "Pad", "Lead", "Perc"]
        .iter()
        .cycle()
        .take(edits)
        .enumerate()
    {
        let _ = song.run(
            "track.add.audio",
            json!({ "name": format!("{track} {}", i + 1) }),
        );
    }
    // A killed process does not run `Drop`. It went down once the edits were on the disk
    // (the writer's thread is still working on them as the last edit returns).
    song.simulate_crash();
    let _ = std::fs::write(dir.join("session.lock"), u32::MAX.to_string());
}

/// A saved song in `dir`, closed properly.
fn leave_saved(dir: &std::path::Path, name: &str) {
    let mut song = Song::empty();
    let _ = song.run("project.rename", json!({ "name": name }));
    let _ = song.run("track.add.audio", json!({ "name": "Vocal" }));
    let _ = song.save_as(dir);
}

/// Plays stage `stage` of [`KEEP`].
pub fn stage(s: &mut Studio, stage: u8) {
    let dir = dir();
    let g = s.geometry(
        1440.0,
        860.0 - xirius_design::Layout::TOP_BAR_H - xirius_design::Layout::BOTTOM_BAR_H,
    );
    match stage {
        0 => {
            let _ = std::fs::remove_dir_all(&dir);
            leave_crashed(&dir.join("crashed"), "夜の散歩", 3);
            s.keep = Keep::at(dir.join("keep"));
            s.finish_file_request(FileRequest::Open, &dir.join("crashed"));
        }
        1 => {
            s.recovery_button(&g, Button::Continue);
            s.run_command("track.add.audio", json!({ "name": "Strings" }));
            // The flash is 450 ms; a picture is taken later than that.
            s.keep.flash_until = u64::MAX;
        }
        2 => s.keep.flash_until = 0,
        3 => {
            s.keep = Keep::default();
            s.new_song();
            s.run_command("track.add.audio", json!({ "name": "Vocal" }));
        }
        4 => {
            let keep = Keep::at(dir.join("keep"));
            let now = xirius_core::store::now_ms();
            for (name, ago_ms, draft) in [
                ("夜の散歩", 2 * 60_000, false),
                ("無題の曲", 3 * 3_600_000, true),
                ("春のデモ", 2 * 86_400_000, false),
            ] {
                let place = dir.join("recent").join(name);
                leave_saved(&place, name);
                keep.remember(&place, name, draft, now - ago_ms);
            }
            s.keep = keep;
            s.popup = None;
        }
        _ => {}
    }
}
