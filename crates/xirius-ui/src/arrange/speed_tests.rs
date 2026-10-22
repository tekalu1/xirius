//! Alt on an audio clip's right edge changes its speed (EDT-038), pressed and moved as the
//! window does it.

use super::probe::{hold, track};
use super::*;
use crate::tools::SnapMode;
use xirius_core::model::{AudioProcess, Clip};

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

fn alt() -> Mods {
    Mods {
        alt: true,
        ..Mods::default()
    }
}

fn clip(s: &Studio, name: &str) -> Clip {
    let t = track(s, name);
    s.project().tracks[&t].clips[0].clone()
}

fn process(c: &Clip) -> AudioProcess {
    match &c.body {
        ClipBody::Audio { process, .. } => *process,
        ClipBody::Midi { .. } => unreachable!(),
    }
}

/// The right edge of the first clip of `name`, and the clip's rect.
fn edge(s: &Studio, g: &Geometry, name: &str) -> ((f32, f32), Rect) {
    let t = track(s, name);
    let c = clip(s, name);
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let r = s.clip_rect(g, &row, &c);
    ((r.right() - 2.0, r.y + r.h / 2.0), r)
}

fn bars(s: &Studio, n: f32) -> f32 {
    s.ticks_to_px(BAR) * n
}

#[test]
fn alt_on_the_right_edge_marks_it_and_a_drag_sets_the_speed_keeping_the_tempo() {
    let (mut s, g) = studio();
    let before = clip(&s, "Vocal");
    let (at, _) = edge(&s, &g, "Vocal");
    // Alt over the edge: the stretch mark is on it; without Alt it is not.
    s.arrange_move(&g, at.0, at.1, Mods::default());
    assert_eq!(s.speed_edge(), None);
    s.arrange_move(&g, at.0, at.1, alt());
    assert_eq!(s.speed_edge().map(|(_, c)| c), Some(before.id));

    let n = s.song.steps();
    let to = (at.0 + bars(&s, 1.0), at.1);
    hold(&mut s, &g, at, to, alt());
    let d = s.arrange.drag.clone().expect("a drag");
    let plan = s.plan(&g, &d).expect("a plan");
    let want = 100.0 * before.length as f64 / (before.length + BAR) as f64;
    assert_eq!(
        plan.tip,
        format!(
            "長さ {:.2} 小節　速さ {want:.0}%",
            (before.length + BAR) as f64 / BAR as f64
        )
    );
    // The clip is drawn at the speed it would have, the edge still marked.
    let (percent, ids) = s
        .speed_preview(&g)
        .expect("the clip stretches as it is dragged");
    assert!((percent - want).abs() < 1e-9 && ids == [before.id]);
    assert_eq!(s.speed_edge().map(|(_, c)| c), Some(before.id));

    s.arrange_up(&g, to.0, to.1, alt());
    let after = clip(&s, "Vocal");
    assert_eq!(s.song.steps(), n + 1);
    assert!((process(&after).speed - want).abs() < 1e-6);
    assert!((after.length - (before.length + BAR)).abs() <= 1);
    assert_eq!(process(&after).tempo, process(&before).tempo);
    assert_eq!(after.start, before.start);
    s.undo(&g);
    assert_eq!(clip(&s, "Vocal"), before);
}

#[test]
fn the_speed_stops_at_half_and_double() {
    let (mut s, g) = studio();
    let before = clip(&s, "Vocal");
    let (at, r) = edge(&s, &g, "Vocal");
    let far = (at.0 + r.w * 1.5, at.1);
    hold(&mut s, &g, at, far, alt());
    s.arrange_up(&g, far.0, far.1, alt());
    let slow = clip(&s, "Vocal");
    assert_eq!(process(&slow).speed, AudioProcess::MIN_SPEED);
    assert!((slow.length - before.length * 2).abs() <= 1);
    s.undo(&g);

    let (at, r) = edge(&s, &g, "Vocal");
    let near = (r.x + 4.0, at.1);
    hold(&mut s, &g, at, near, alt());
    s.arrange_up(&g, near.0, near.1, alt());
    let fast = clip(&s, "Vocal");
    assert_eq!(process(&fast).speed, AudioProcess::MAX_SPEED);
    assert!((fast.length - before.length / 2).abs() <= 1);
}

#[test]
fn without_alt_the_edge_changes_the_length_only_and_a_midi_clip_is_never_sped() {
    let (mut s, g) = studio();
    let before = clip(&s, "Vocal");
    let (at, _) = edge(&s, &g, "Vocal");
    let to = (at.0 + bars(&s, 1.0), at.1);
    hold(&mut s, &g, at, to, Mods::default());
    s.arrange_up(&g, to.0, to.1, Mods::default());
    let after = clip(&s, "Vocal");
    assert_eq!(process(&after).speed, 100.0);
    assert_eq!(after.length, before.length + BAR);

    let keys = clip(&s, "Keys");
    let (at, _) = edge(&s, &g, "Keys");
    s.arrange_move(&g, at.0, at.1, alt());
    assert_eq!(s.speed_edge(), None);
    let to = (at.0 + bars(&s, 1.0), at.1);
    hold(&mut s, &g, at, to, alt());
    assert!(s.speed_preview(&g).is_none());
    s.arrange_up(&g, to.0, to.1, alt());
    assert_eq!(clip(&s, "Keys").length, keys.length + BAR);
}
