//! `--script home-draft-delete`: deleting an unsaved draft from the start screen's list
//! (UI-351): the menu of a draft row, the question (first on 「戻る」, on 「削除」, with a long
//! name) and the list once the draft is gone. Run once dark and once with `--light`. Silent;
//! no OS input.

use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "hdd01-menu",
        acts: &[Act::Reset, stage(0)],
    },
    Shot {
        name: "hdd02-question",
        acts: &[Act::Reset, stage(1)],
    },
    Shot {
        name: "hdd03-question-on-delete",
        acts: &[Act::Reset, stage(2)],
    },
    Shot {
        name: "hdd04-question-long-name",
        acts: &[Act::Reset, stage(3)],
    },
    Shot {
        name: "hdd05-after",
        acts: &[Act::Reset, stage(4)],
    },
];

const LONG: &str = "夜明け前のうた（仮）をギターとピアノで録り直したバージョン その3 最終";

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    home(s, g);
    let g = &s.geometry(g.size.0, g.size.1);
    match n {
        0 => {
            let at = row_centre(s, g, 0);
            s.window_right_press(g, at.0, at.1);
        }
        1..=3 => {
            choose(s, usize::from(n == 3));
            s.home_key(g, "delete", Mods::NONE);
            if n == 2 {
                s.unsaved_key("tab");
            }
        }
        _ => {
            choose(s, 0);
            s.home_key(g, "delete", Mods::NONE);
            s.unsaved_key("tab");
            s.unsaved_key("enter");
            let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while std::time::Instant::now() < end {
                s.home_tick();
                s.keep.poll_recent();
                if s.draft_removal.is_none() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            s.home_tick();
        }
    }
}

fn choose(s: &mut Studio, row: usize) {
    s.home_choose(row);
}

fn row_centre(s: &mut Studio, g: &Geometry, row: usize) -> (f32, f32) {
    choose(s, row);
    s.home_row_centre(g, row).unwrap_or((0.0, 0.0))
}

/// Four songs: two drafts (the newest edited just now, the other with a long name), a saved
/// one and the sample. Folders of the shots' own.
fn home(s: &mut Studio, g: &Geometry) {
    *s = Studio::new(crate::song::Song::empty());
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    s.resized(g);
    let root = std::env::temp_dir().join(format!("xirius-draft-shots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let keep = crate::keep::Keep::at(root.join("keep"));
    let now = xirius_core::store::now_ms();
    for (name, draft, ago_ms) in [
        ("朝の曲", false, 4 * 86_400_000),
        (LONG, true, 7 * 60_000),
        ("新しい曲", true, 0),
    ] {
        let place = if draft {
            root.join("keep").join("drafts").join(format!("d{ago_ms}"))
        } else {
            root.join("songs").join(name)
        };
        let mut song = crate::song::Song::empty();
        let _ = song.save_as(&place);
        keep.remember(&place, name, draft, now - ago_ms);
    }
    s.keep = keep;
    s.keep.sync(&s.song.project());
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while s.keep.listed().len() < 3 && std::time::Instant::now() < end {
        s.keep.poll_recent();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    s.home = Some(crate::home::Home::default());
    s.home_tick();
}
