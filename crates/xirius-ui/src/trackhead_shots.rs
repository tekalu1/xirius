//! `--script trackhead`: the track head dragged to 216, 340 and 420 wide at the row heights
//! 56, 64 and 128 (UI-312, UI-324), on the window's own pointer path, for the comparison with
//! the approved mock `track-header-width`. Each stage reports the width it reached and whether
//! the first track shows its volume fader on stderr (`trackhead-check stage=N ...`). The last
//! four stages are the review's: long and Japanese names, the mark pill, selected, muted, soloed
//! and armed rows, on the sample song (a folder three deep, whose column cannot go below 232)
//! and then flattened (216, the mock's floor), 216, 340 and 420 wide.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{App, Window};
use serde_json::json;

use crate::shot::{Act, Play, Shot};
use crate::side_edge::Side;
use crate::side_shots::{at, button, drag, geometry, mid, moved};
use crate::studio::{Hit, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;
use xirius_core::id::TrackId;

const WIDTHS: [f32; 3] = [216.0, 340.0, 420.0];
const HEIGHTS: [f32; 3] = [56.0, 64.0, 128.0];

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "th-01-h56-w216",
        acts: &[staged(0)],
    },
    Shot {
        name: "th-02-h56-w340",
        acts: &[staged(1)],
    },
    Shot {
        name: "th-03-h56-w420",
        acts: &[staged(2)],
    },
    Shot {
        name: "th-04-h64-w216",
        acts: &[staged(3)],
    },
    Shot {
        name: "th-05-h64-w340",
        acts: &[staged(4)],
    },
    Shot {
        name: "th-06-h64-w420",
        acts: &[staged(5)],
    },
    Shot {
        name: "th-07-h128-w216",
        acts: &[staged(6)],
    },
    Shot {
        name: "th-08-h128-w340",
        acts: &[staged(7)],
    },
    Shot {
        name: "th-09-h128-w420",
        acts: &[staged(8)],
    },
    // The first track widened with its ↕, at 420.
    Shot {
        name: "th-10-widened-w420",
        acts: &[staged(9)],
    },
    // The fader of the first track, held and moved (the value tag shows), let go, and
    // double-clicked (the number field opens).
    Shot {
        name: "th-11-fader-held",
        acts: &[staged(10)],
    },
    Shot {
        name: "th-12-fader-let-go",
        acts: &[staged(11)],
    },
    Shot {
        name: "th-13-fader-field",
        acts: &[staged(12)],
    },
    // Long, Japanese and marked names, selected / muted / soloed / armed rows: the sample song
    // (a folder three deep) dragged to 216, then flattened at 216, 340 and 420.
    Shot {
        name: "th-14-names-deep-w216",
        acts: &[staged(13)],
    },
    Shot {
        name: "th-15-names-flat-w216",
        acts: &[staged(14)],
    },
    Shot {
        name: "th-16-names-flat-w340",
        acts: &[staged(15)],
    },
    Shot {
        name: "th-17-names-flat-w420",
        acts: &[staged(16)],
    },
];

pub(crate) fn play(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let (h, w) = (
        HEIGHTS[(stage as usize / 3).min(2)],
        WIDTHS[stage as usize % 3],
    );
    if stage >= 13 {
        names_stage(stage, studio, window, cx);
        return;
    }
    if stage >= 10 {
        fader_stage(stage, studio, window, cx);
        return;
    }
    if stage == 9 {
        let g = geometry(studio, window);
        let first = studio
            .borrow()
            .tree()
            .rows
            .iter()
            .find_map(|r| match r.kind {
                RowKind::Track(t) => Some(t),
                RowKind::Folder(_) => None,
            });
        if let Some(t) = first {
            studio.borrow_mut().press(&g, Hit::Widen(t));
        }
    } else {
        let g = geometry(studio, window);
        {
            let mut s = studio.borrow_mut();
            s.arrange.track_h = h;
            s.rebuild(&g);
        }
        let now = geometry(studio, window).column.w;
        drag(Side::Head, w - now, studio, window, cx);
    }
    let g = geometry(studio, window);
    let s = studio.borrow();
    let first = s.tree().rows.iter().find_map(|r| match r.kind {
        RowKind::Track(t) => Some(t),
        RowKind::Folder(_) => None,
    });
    let fader = first.and_then(|t| s.head_fader_rect(&g, t)).map(|r| r.w);
    eprintln!(
        "trackhead-check stage={stage} head={} track_h={} fader={fader:?}",
        g.column.w, s.arrange.track_h
    );
}

/// The first track and where its fader is now, in window coordinates.
fn first_fader(studio: &Rc<RefCell<Studio>>, window: &Window) -> Option<(TrackId, (f32, f32))> {
    let g = geometry(studio, window);
    let s = studio.borrow();
    let t = s.tree().rows.iter().find_map(|r| match r.kind {
        RowKind::Track(t) => Some(t),
        RowKind::Folder(_) => None,
    })?;
    Some((t, mid(s.head_fader_rect(&g, t)?)))
}

fn volume(studio: &Rc<RefCell<Studio>>, t: TrackId) -> f64 {
    studio.borrow().project().tracks[&t].volume_db
}

/// Stages 10 to 12: the fader on the window's pointer path, at 340 wide.
fn fader_stage(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if stage == 10 {
        let now = geometry(studio, window).column.w;
        drag(Side::Head, 340.0 - now, studio, window, cx);
    }
    let Some((t, (x, y))) = first_fader(studio, window) else {
        return eprintln!("trackhead-check stage={stage} no fader");
    };
    let before = volume(studio, t);
    let p = at(x, y);
    match stage {
        10 => {
            window.dispatch_event(moved(p, false), cx);
            window.dispatch_event(button(true, p, 1), cx);
            for i in 1..=4 {
                window.dispatch_event(moved(at(x + 10.0 * i as f32, y), true), cx);
            }
        }
        11 => {
            window.dispatch_event(button(false, at(x + 40.0, y), 1), cx);
        }
        _ => {
            let p = at(x + 40.0, y);
            for n in 1..=2 {
                window.dispatch_event(button(true, p, n), cx);
                window.dispatch_event(button(false, p, n), cx);
            }
        }
    }
    let s = studio.borrow();
    eprintln!(
        "trackhead-check stage={stage} volume {before:.2} -> {:.2} dragging={} field={}",
        s.project().tracks[&t].volume_db,
        s.head_fader.dragging.is_some(),
        s.head_fader.entry.is_some()
    );
}

/// The tracks in the column's order.
fn tracks(studio: &Rc<RefCell<Studio>>) -> Vec<TrackId> {
    studio
        .borrow()
        .tree()
        .rows
        .iter()
        .filter_map(|r| match r.kind {
            RowKind::Track(t) => Some(t),
            RowKind::Folder(_) => None,
        })
        .collect()
}

/// Stages 13 to 16: names and states that stress the head, at the width each stage names.
/// Stage 13 sets them on the sample song; stage 14 unpacks its folders, so the column can go
/// down to the flat song's floor.
fn names_stage(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if stage == 13 {
        let g = geometry(studio, window);
        let ids = tracks(studio);
        let mut s = studio.borrow_mut();
        // The default row height, so the two lines and the marks are the usual ones.
        s.arrange.track_h = HEIGHTS[1];
        s.rebuild(&g);
        // A field left open by the last stage is let go of.
        s.head_fader_key("escape", Mods::NONE);
        let names = [
            "とても長いトラックの名前をここに入れて収まりを確かめる",
            "A very long track name that cannot possibly fit in the column",
            "ドラム",
            "Lead",
            "Bass",
        ];
        for (t, name) in ids.iter().zip(names) {
            s.arrange_run("track.rename", json!({ "id": t.to_string(), "name": name }));
        }
        let id = |i: usize| json!({ "ids": [ids[i].to_string()] });
        // The mark pill: transpose and velocity on the first two.
        s.arrange_run(
            "track.transpose",
            json!({ "ids": [ids[0].to_string()], "semis": 5 }),
        );
        s.arrange_run(
            "track.velocity",
            json!({ "ids": [ids[0].to_string()], "percent": -20 }),
        );
        s.arrange_run(
            "track.transpose",
            json!({ "ids": [ids[1].to_string()], "semis": -3 }),
        );
        // Muted, soloed, armed, and one selected.
        s.arrange_run("track.mute", id(2));
        s.arrange_run("track.solo", id(3));
        s.arrange_run("track.arm", id(4));
        s.selected_row = Some(RowKind::Track(ids[3]));
    }
    if stage == 14 {
        let mut s = studio.borrow_mut();
        for _ in 0..20 {
            let folder = s.tree().rows.iter().find_map(|r| match r.kind {
                RowKind::Folder(f) => Some(f),
                RowKind::Track(_) => None,
            });
            let Some(f) = folder else { break };
            s.arrange_run("folder.unpack", json!({ "id": f.to_string() }));
        }
    }
    let w = [216.0, 216.0, 340.0, 420.0][stage as usize - 13];
    let now = geometry(studio, window).column.w;
    drag(Side::Head, w - now, studio, window, cx);
    let g = geometry(studio, window);
    let s = studio.borrow();
    let depth = s.tree().rows.iter().map(|r| r.depth).max().unwrap_or(0);
    let faders = tracks(studio)
        .iter()
        .filter(|t| s.head_fader_rect(&g, **t).is_some())
        .count();
    eprintln!(
        "trackhead-check stage={stage} head={} asked={w} depth={depth} faders={faders}",
        g.column.w
    );
}
