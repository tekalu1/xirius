//! AUT-010: the range over a lane or over the tempo row, taken hold of. The upper half of the
//! range moves its values with a vertical drag, and the frame Alt+T or 「自由変形」 puts round
//! them scales and tilts them by its handles. Pressed as the window presses them.

use serde_json::json;
use xirius_core::auto::{AutoLane, AutoTarget};
use xirius_core::commands::{RangeMove, moved_lane, moved_tempo};
use xirius_core::id::TrackId;

use super::tests::{BAR, choose, labels, steps, studio};
use crate::arrange::probe::{hold, two_lanes};
use crate::arrange::range_trim::RangeFrame;
use crate::arrange::{Preview, Spot};
use crate::roll::Cursor;
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

fn lane(s: &Studio, t: TrackId, target: &AutoTarget) -> AutoLane {
    s.project().tracks[&t]
        .automation
        .iter()
        .find(|l| l.target == *target)
        .expect("the lane")
        .clone()
}

/// "Bass" with its volume and pan lanes, and the range over bars 5 to 9 (the volume points).
fn ranged() -> (Studio, Geometry, TrackId) {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Bass");
    s.arrange.range = Some((4 * BAR, 8 * BAR));
    s.arrange.ranges = vec![(4 * BAR, 8 * BAR)];
    (s, g, t)
}

/// A point in the upper quarter of the volume lane, between bars 5 and 6.
fn upper(s: &Studio, g: &Geometry, t: TrackId) -> (f32, f32) {
    let top = s
        .lane_top_of(g, t, &AutoTarget::Volume)
        .expect("the lane on screen");
    (s.tick_x(g, 4 * BAR + BAR / 2), top + 10.0)
}

/// What the drag in progress shows, as a move of the stretch.
fn shown(s: &Studio, g: &Geometry) -> RangeMove {
    let d = s.arrange.drag.as_ref().expect("a drag");
    match s.plan(g, d).expect("a plan").preview {
        Preview::RangeMove { how, .. } => how,
        p => panic!("not a range move: {p:?}"),
    }
}

#[test]
fn the_upper_half_of_the_range_moves_every_value_of_the_lane_as_the_drag_showed() {
    let (mut s, g, t) = ranged();
    let vol = AutoTarget::Volume;
    let at = upper(&s, &g, t);
    // The pointer over the upper half is the vertical arrows; the lower half is not taken.
    s.arrange_move(&g, at.0, at.1, Mods::NONE);
    assert_eq!(s.arrange.hover, Spot::RangeTrim(Some((t, vol.clone()))));
    assert_eq!(crate::arrange::paint::cursor(&s), Some(Cursor::ResizeV));
    let top = s.lane_top_of(&g, t, &vol).unwrap();
    let low = (at.0, top + crate::tree::LANE_H - 6.0);
    s.arrange_move(&g, low.0, low.1, Mods::NONE);
    assert!(!matches!(s.arrange.hover, Spot::RangeTrim(_)));

    let before = lane(&s, t, &vol);
    let pan = lane(&s, t, &AutoTarget::Pan);
    let n = steps(&s);
    hold(&mut s, &g, at, (at.0, at.1 - 20.0), Mods::NONE);
    let how = shown(&s, &g);
    assert!(matches!(how, RangeMove::Trim(d) if d > 0.0), "{how:?}");
    let want = moved_lane(
        &before,
        4 * BAR,
        8 * BAR,
        how,
        vol.stored_value(&s.project().tracks[&t]),
    )
    .expect("the stretch moves");
    s.arrange_up(&g, at.0, at.1 - 20.0, Mods::NONE);
    assert_eq!(steps(&s), n + 1, "one step");
    assert_eq!(lane(&s, t, &vol).points, want.points);
    assert_ne!(lane(&s, t, &vol).points, before.points);
    assert_eq!(lane(&s, t, &AutoTarget::Pan), pan);
    // The range is still there to move again.
    assert_eq!(s.arrange.range, Some((4 * BAR, 8 * BAR)));
}

#[test]
fn the_upper_half_of_the_range_over_the_tempo_row_moves_the_tempo_by_half_a_bpm_a_pixel() {
    let (mut s, g) = studio();
    s.song
        .run("tempo.set", json!({"at": 2 * BAR, "bpm": 150}))
        .unwrap();
    s.arrange.range = Some((BAR, 3 * BAR));
    let before = s.project().tempo.clone();
    let at = (s.tick_x(&g, BAR + BAR / 2), g.tempo.y + 3.0);
    s.arrange_move(&g, at.0, at.1, Mods::NONE);
    assert_eq!(s.arrange.hover, Spot::RangeTrim(None));
    hold(&mut s, &g, at, (at.0, at.1 - 10.0), Mods::NONE);
    let how = shown(&s, &g);
    assert_eq!(how, RangeMove::Trim(5.0));
    let tip = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap().tip;
    assert_eq!(tip, "+5 BPM");
    let want = moved_tempo(&before, BAR, 3 * BAR, how);
    s.arrange_up(&g, at.0, at.1 - 10.0, Mods::NONE);
    let got: Vec<_> = s
        .project()
        .tempo
        .iter()
        .map(|p| (p.tick, p.bpm, p.ramp))
        .collect();
    let want: Vec<_> = want.iter().map(|p| (p.tick, p.bpm, p.ramp)).collect();
    assert_eq!(got, want);
    assert!(
        got.iter()
            .any(|(tick, bpm, _)| *tick == 2 * BAR && *bpm == 155.0),
        "{got:?}"
    );
}

#[test]
fn shift_moves_the_values_by_a_tenth() {
    let (mut s, g, t) = ranged();
    let at = upper(&s, &g, t);
    hold(&mut s, &g, at, (at.0, at.1 - 20.0), Mods::NONE);
    let RangeMove::Trim(coarse) = shown(&s, &g) else {
        panic!("a trim")
    };
    s.arrange.drag = None;
    hold(&mut s, &g, at, (at.0, at.1 - 20.0), Mods::shift());
    let RangeMove::Trim(fine) = shown(&s, &g) else {
        panic!("a trim")
    };
    assert!((fine * 10.0 - coarse).abs() < 0.002, "{fine} {coarse}");
}

#[test]
fn alt_t_puts_the_frame_round_the_range_under_the_pointer_and_its_handles_scale_and_tilt() {
    let (mut s, g, t) = ranged();
    let vol = AutoTarget::Volume;
    let at = upper(&s, &g, t);
    // A press in the arrangement gives it the keys; the pointer stays over the lane.
    s.arrange_down(&g, at.0, at.1, Mods::NONE);
    s.arrange_up(&g, at.0, at.1, Mods::NONE);
    s.arrange_move(&g, at.0, at.1, Mods::NONE);
    assert!(s.mapped_key(&g, "t", Some("t"), Mods::alt()));
    assert_eq!(
        s.arrange.frame,
        Some(RangeFrame {
            place: Some((t, vol.clone())),
            from: 4 * BAR,
            to: 8 * BAR,
        })
    );
    let b = s.frame_box(&g, None).expect("the frame on screen");

    // The top edge's handle: up makes the values spread wider.
    let before = lane(&s, t, &vol);
    let n = steps(&s);
    let grip = (b.scale.x + 4.0, b.scale.y + 4.0);
    s.arrange_move(&g, grip.0, grip.1, Mods::NONE);
    assert_eq!(s.arrange.hover, Spot::FrameGrip { tilt: false });
    hold(&mut s, &g, grip, (grip.0, grip.1 - 12.0), Mods::NONE);
    let how = shown(&s, &g);
    assert!(
        matches!(how, RangeMove::Transform { scale, tilt } if scale > 1.0 && tilt == 0.0),
        "{how:?}"
    );
    let want = moved_lane(
        &before,
        4 * BAR,
        8 * BAR,
        how,
        vol.stored_value(&s.project().tracks[&t]),
    )
    .unwrap();
    s.arrange_up(&g, grip.0, grip.1 - 12.0, Mods::NONE);
    assert_eq!(steps(&s), n + 1);
    assert_eq!(lane(&s, t, &vol).points, want.points);
    assert!(
        s.arrange.frame.is_some(),
        "the frame stays for the next handle"
    );

    // The corner's handle tilts.
    let b = s.frame_box(&g, None).unwrap();
    let before = lane(&s, t, &vol);
    let grip = (b.tilt.x + 4.0, b.tilt.y + 4.0);
    hold(&mut s, &g, grip, (grip.0, grip.1 + 10.0), Mods::NONE);
    let how = shown(&s, &g);
    assert!(
        matches!(how, RangeMove::Transform { scale, tilt } if scale == 1.0 && tilt < 0.0),
        "{how:?}"
    );
    let tip = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap().tip;
    assert!(tip.starts_with("傾き -"), "{tip}");
    let want = moved_lane(
        &before,
        4 * BAR,
        8 * BAR,
        how,
        vol.stored_value(&s.project().tracks[&t]),
    )
    .unwrap();
    s.arrange_up(&g, grip.0, grip.1 + 10.0, Mods::NONE);
    assert_eq!(lane(&s, t, &vol).points, want.points);

    // Escape takes it away.
    assert!(s.arrange_key(&g, "escape", Mods::NONE));
    assert!(s.arrange.frame.is_none());
}

#[test]
fn the_menu_of_the_range_puts_the_frame_round_it_and_a_press_elsewhere_takes_it_away() {
    for tempo in [false, true] {
        let (mut s, g, t) = ranged();
        let (x, y) = if tempo {
            s.song
                .run("tempo.set", json!({"at": 6 * BAR, "bpm": 140}))
                .unwrap();
            (s.tick_x(&g, 5 * BAR), g.tempo.y + 4.0)
        } else {
            upper(&s, &g, t)
        };
        assert!(s.scene_context(&g, x, y));
        assert!(choose(&mut s, &g, "範囲をトランスフォーム"));
        let rows = labels(&s);
        // The first row after 「戻る」.
        assert_eq!(
            rows.get(1).map(String::as_str),
            Some("自由変形"),
            "{rows:?}"
        );
        assert_eq!(super::tests::key(&s, "自由変形"), "Alt+T");
        let n = steps(&s);
        assert!(choose(&mut s, &g, "自由変形"));
        assert_eq!(steps(&s), n, "the frame changes nothing yet");
        let place = if tempo {
            None
        } else {
            Some((t, AutoTarget::Volume))
        };
        assert_eq!(
            s.arrange.frame.as_ref().map(|f| f.place.clone()),
            Some(place)
        );
        assert!(s.frame_box(&g, None).is_some());
        // A press on a clip of another track lets the frame go.
        let (cx, cy) = crate::arrange::probe::clip_mid(&s, &g, "Vocal", 0);
        s.arrange_down(&g, cx, cy, Mods::NONE);
        s.arrange_up(&g, cx, cy, Mods::NONE);
        assert!(s.arrange.frame.is_none(), "tempo {tempo}");
    }
}

#[test]
fn the_command_typed_without_a_scale_puts_the_frame_on_the_lane_it_names() {
    let (mut s, g, t) = ranged();
    let line = format!(
        "range.transform --track {t} --target pan --from {} --to {}",
        4 * BAR,
        8 * BAR
    );
    assert!(s.run_binding(&g, &line), "{line}");
    assert_eq!(
        s.arrange.frame.as_ref().map(|f| f.place.clone()),
        Some(Some((t, AutoTarget::Pan)))
    );
    // With a scale it is the song's command, as before.
    let n = steps(&s);
    let line = format!(
        "range.transform --track {t} --target pan --from {} --to {} --scale 0.5",
        4 * BAR,
        8 * BAR
    );
    assert!(s.run_binding(&g, &line), "{line}");
    assert_eq!(steps(&s), n + 1);
}
