//! `--script ripple`: the ripple switch left of the grid in the arrangement's bar (EDT-135).
//! Off, a delete leaves the gap; on, the same delete closes up the track and a longer clip
//! pushes what follows; Ctrl+Alt+R turns it off again.
use serde_json::json;

use crate::shot::{Act, Play, Shot, Target};
use crate::studio::{Geometry, Hit, Studio};
use crate::tools::Mods;
use crate::topbar::BarButton;

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "ripple-01-off",
        acts: &[Act::Reset, staged(0)],
    },
    Shot {
        name: "ripple-02-on",
        acts: &[
            Act::Press(Target::Top(BarButton::Ripple)),
            Act::Hover(Target::Top(BarButton::Ripple)),
        ],
    },
    Shot {
        name: "ripple-03-deleted",
        acts: &[staged(1)],
    },
    Shot {
        name: "ripple-04-longer",
        acts: &[staged(2)],
    },
    Shot {
        name: "ripple-05-key-off",
        acts: &[staged(3)],
    },
];

/// The clips of the Keys track, earliest first.
fn keys(s: &Studio) -> (xirius_core::id::TrackId, Vec<xirius_core::model::Clip>) {
    let p = s.project();
    let t = p
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .expect("the sample has Keys");
    let mut clips = t.clips.clone();
    clips.sort_by_key(|c| c.start);
    (t.id, clips)
}

/// Clicks the `i`-th clip of Keys, so it is the selection.
fn pick(s: &mut Studio, g: &Geometry, i: usize) {
    let (t, clips) = keys(s);
    let (x, y) = s
        .center_of(g, Hit::Clip(t, clips[i].id))
        .expect("the clip is on screen");
    s.arrange_down(g, x, y, Mods::default());
    s.arrange_up(g, x, y, Mods::default());
}

/// Stage 0 shuts the drawer, adds two clips after the first of Keys (a bar apart) and selects
/// the first; 1 deletes it with ripple on; 2 drags the right end of the new first clip a bar
/// longer; 3 presses Ctrl+Alt+R.
fn play(s: &mut Studio, g: &Geometry, n: u8) {
    let bar = 4 * i64::from(s.project().ppq);
    match n {
        0 => {
            s.popup = None;
            s.drawer_open = false;
            s.choose_editor();
            let (t, clips) = keys(s);
            let end = clips[0].start + clips[0].length;
            for at in [end + bar, end + 3 * bar] {
                assert!(s.run_command(
                    "clip.add.midi",
                    json!({"track": t.to_string(), "at": at, "length": bar}),
                ));
            }
            pick(s, g, 0);
        }
        1 => {
            s.hover = Hit::None;
            pick(s, g, 0);
            let before = keys(s).1.len();
            assert!(s.arrange_key(g, "delete", Mods::default()));
            assert_eq!(keys(s).1.len(), before - 1);
        }
        2 => {
            let (t, clips) = keys(s);
            let row = *s
                .tree()
                .row_of(crate::tree::RowKind::Track(t))
                .expect("Keys has a row");
            let r = s.clip_rect(g, &row, &clips[0]);
            let (x, y) = (r.right() - 2.0, r.y + r.h / 2.0);
            let to = x + s.ticks_to_px(bar) * 1.1;
            s.arrange.drag = None;
            s.arrange_down(g, x, y, Mods::default());
            // Two steps, so the drag starts before it reaches the end.
            s.arrange_move(g, (x + to) / 2.0, y, Mods::default());
            s.arrange_move(g, to, y, Mods::default());
            s.arrange_up(g, to, y, Mods::default());
        }
        _ => {
            let ctrl_alt = Mods {
                ctrl: true,
                alt: true,
                ..Mods::default()
            };
            assert!(s.mapped_key(g, "r", None, ctrl_alt));
            assert!(!s.bar_view().ripple);
        }
    }
}
