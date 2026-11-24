//! UI-375: where two clips overlap, the one painted on top is the one a press takes, whichever
//! order the track keeps them in.

use super::probe::{down, track};
use super::*;
use crate::tools::SnapMode;

const BAR: Tick = 3840;

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.snap_mode = SnapMode::Bar;
    s.song
        .run(
            "prefs.set",
            json!({"key":"edit.snap.arrange.mode","value":"bar"}),
        )
        .unwrap();
    s.drawer_open = false;
    let g = s.geometry(1440.0, 1000.0);
    s.resized(&g);
    (s, g)
}

/// The Keys track emptied, then two 4-bar parts: one at bar 2, one at bar 4. `later_first` adds
/// the later one first, so the earlier one lies on top. Returns (track, early, late, top).
fn overlap(s: &mut Studio, g: &Geometry, later_first: bool) -> (TrackId, ClipId, ClipId, ClipId) {
    let t = track(s, "Keys");
    let ids: Vec<String> = s.project().tracks[&t]
        .clips
        .iter()
        .map(|c| c.id.to_string())
        .collect();
    if !ids.is_empty() {
        s.song.run("clip.delete", json!({ "ids": ids })).unwrap();
    }
    let mut add = |bar: Tick| -> ClipId {
        s.song
            .run(
                "clip.add.midi",
                json!({"track":t.to_string(),"at":bar*BAR,"length":4*BAR}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    };
    let (early, late) = if later_first {
        let late = add(3);
        (add(1), late)
    } else {
        let early = add(1);
        (early, add(3))
    };
    s.resized(g);
    let top = if later_first { early } else { late };
    (t, early, late, top)
}

fn rect(s: &Studio, g: &Geometry, t: TrackId, id: ClipId) -> Rect {
    let row = *s.tree().row_of(RowKind::Track(t)).expect("row");
    let clip = s.project().tracks[&t]
        .clips
        .iter()
        .find(|c| c.id == id)
        .unwrap()
        .clone();
    s.clip_rect(g, &row, &clip)
}

fn spot(s: &Studio, g: &Geometry, x: f32, y: f32) -> (ClipId, Grip) {
    match s.arrange_spot(g, x, y) {
        Spot::Clip { clip, grip, .. } => (clip, grip),
        other => panic!("no clip at ({x}, {y}): {other:?}"),
    }
}

#[test]
fn the_top_clip_is_the_one_painted_last_and_the_one_taken() {
    for later_first in [false, true] {
        let (mut s, g) = studio();
        let (t, early, late, top) = overlap(&mut s, &g, later_first);
        let stacks = xirius_core::takes::stacks(&s.project().tracks[&t]);
        let painted: Vec<ClipId> = crate::draw::painted(&s.project().tracks[&t], &stacks)
            .map(|c| c.id)
            .collect();
        assert_eq!(painted.last(), Some(&top), "the last painted lies on top");
        let (e, l) = (rect(&s, &g, t, early), rect(&s, &g, t, late));
        assert!(l.x < e.right(), "the parts overlap");
        let y = e.y + e.h / 2.0;
        // Across the overlap, away from the edges.
        for f in [0.25, 0.5, 0.75] {
            let x = l.x + (e.right() - l.x) * f;
            assert_eq!(s.hit(&g, x, y), Hit::Clip(t, top), "hit at {f}");
            assert_eq!(spot(&s, &g, x, y), (top, Grip::Body), "spot at {f}");
        }
        // The edges inside the overlap belong to the top part only.
        let (right_of_early, left_of_late) = (e.right() - 2.0, l.x + 2.0);
        if later_first {
            assert_eq!(spot(&s, &g, right_of_early, y), (early, Grip::Right));
            assert_eq!(spot(&s, &g, left_of_late, y), (early, Grip::Body));
        } else {
            assert_eq!(spot(&s, &g, right_of_early, y), (late, Grip::Body));
            assert_eq!(spot(&s, &g, left_of_late, y), (late, Grip::Left));
        }
    }
}

#[test]
fn a_drag_in_the_overlap_moves_and_resizes_the_top_clip() {
    for later_first in [false, true] {
        let (mut s, g) = studio();
        let (t, early, late, top) = overlap(&mut s, &g, later_first);
        let under = if top == early { late } else { early };
        let start = |s: &Studio, id: ClipId| {
            let c = s.project().tracks[&t]
                .clips
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .clone();
            (c.start, c.length)
        };
        let before = (start(&s, top), start(&s, under));
        let (e, l) = (rect(&s, &g, t, early), rect(&s, &g, t, late));
        let y = e.y + e.h / 2.0;
        let from = ((l.x + e.right()) / 2.0, y);
        let to = (from.0 + s.ticks_to_px(BAR), y);
        super::probe::hold(&mut s, &g, from, to, Mods::default());
        s.arrange_up(&g, to.0, to.1, Mods::default());
        assert_eq!(
            start(&s, top),
            (before.0.0 + BAR, before.0.1),
            "the top part moved"
        );
        assert_eq!(start(&s, under), before.1, "the part under it stayed");

        // Its edge inside the overlap: the early part's right edge or the late part's left.
        let (e, l) = (rect(&s, &g, t, early), rect(&s, &g, t, late));
        let r = if top == early { e } else { l };
        let edge = if top == early {
            r.right() - 2.0
        } else {
            r.x + 2.0
        };
        assert!(e.right() > l.x, "still overlapping");
        let before = (start(&s, top), start(&s, under));
        let delta = if top == early { BAR } else { -BAR };
        let to = (edge + s.ticks_to_px(delta), y);
        super::probe::hold(&mut s, &g, (edge, y), to, Mods::default());
        s.arrange_up(&g, to.0, to.1, Mods::default());
        let (at, len) = before.0;
        let want = if top == early {
            (at, len + BAR)
        } else {
            (at - BAR, len + BAR)
        };
        assert_eq!(start(&s, top), want, "the top part's edge moved");
        assert_eq!(start(&s, under), before.1, "the part under it stayed");
    }
}

#[test]
fn a_press_in_the_overlap_selects_the_top_clip() {
    for later_first in [false, true] {
        let (mut s, g) = studio();
        let (t, early, late, top) = overlap(&mut s, &g, later_first);
        let (e, l) = (rect(&s, &g, t, early), rect(&s, &g, t, late));
        let at = ((l.x + e.right()) / 2.0, e.y + e.h / 2.0);
        down(&mut s, &g, at, Mods::default());
        s.arrange_up(&g, at.0, at.1, Mods::default());
        assert_eq!(s.selected_clip, Some((t, top)));
    }
}
