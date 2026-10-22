//! The second round of the editing feel (impl-feel2) in the arrangement, driven through the
//! screen's own wheel, press, move and release entry points.
use super::probe::{hold, two_lanes};
use super::*;
use crate::tools::SnapMode;
use crate::topbar::BarButton;
use xirius_core::shape::Shape;

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

fn ctrl() -> Mods {
    Mods {
        ctrl: true,
        ..Mods::default()
    }
}

fn shape(s: &Studio) -> Shape {
    s.song.session().state.view.draw_shape
}

/// The wheel over the pencil of the top bar, as `Screen::wheel` hands it on: `notches` down.
fn wheel_on_pencil(s: &mut Studio, g: &Geometry, notches: i32) {
    let (x, y) = s
        .center_of(g, Hit::Bar(BarButton::Tool(Tool::Draw)))
        .expect("the pencil on the bar");
    for _ in 0..notches.abs() {
        assert!(s.bar_shape_wheel(g, x, y, -notches.signum() as f32));
    }
    assert!(s.error.is_none(), "{:?}", s.error);
}

/// Whether a line between `from` and `to` bends (a parabola is one bent line, a line a straight
/// one).
fn bent(s: &Studio, t: xirius_core::id::TrackId, from: Tick, to: Tick) -> bool {
    s.project().tracks[&t].automation[0]
        .points
        .iter()
        .any(|p| p.tick >= from && p.tick < to && p.curve != 0.0)
}

fn points(s: &Studio, t: xirius_core::id::TrackId, from: Tick, to: Tick) -> Vec<(Tick, f64)> {
    s.project().tracks[&t].automation[0]
        .points
        .iter()
        .filter(|p| p.tick >= from && p.tick <= to)
        .map(|p| (p.tick, p.value))
        .collect()
}

#[test]
fn edt_032_the_wheel_on_the_pencil_picks_the_shape_the_lane_draws_and_alt_draws_a_line() {
    let (mut s, g) = studio();
    let bass = two_lanes(&mut s, &g, "Bass");
    assert!(s.press(&g, Hit::Bar(BarButton::Tool(Tool::Draw))));
    assert_eq!(s.tool, Tool::Draw);
    let row = *s.tree().row_of(RowKind::Track(bass)).unwrap();
    let top = s.lane_top(&g, &row, 0);
    let (lo, hi) = (top + 12.0, top + crate::tree::LANE_H - 12.0);
    // Bars on the screen (the view does not move here); the strokes snap to whole bars.
    let (x0, bar_px) = (s.tick_x(&g, 0), s.ticks_to_px(BAR));
    let x = |bar: f32| x0 + bar * bar_px;
    let at = |bar: f32| (bar * BAR as f32) as Tick;
    assert!(x(13.5) < g.lanes.right());

    // The freehand with Alt held is a straight line of two points.
    assert_eq!(shape(&s), Shape::Free);
    let steps = s.song.steps();
    hold(&mut s, &g, (x(1.0), lo), (x(2.0), hi), alt());
    s.arrange_up(&g, x(2.0), hi, alt());
    assert_eq!(s.song.steps(), steps + 1, "one stroke, one step");
    assert_eq!(points(&s, bass, at(1.0), at(2.0)).len(), 2);
    assert!(!bent(&s, bass, at(1.0), at(2.0)));

    // Two notches down: line, parabola. The parabola bends; with Alt it is straight again.
    wheel_on_pencil(&mut s, &g, 2);
    assert_eq!(shape(&s), Shape::Parabola);
    hold(&mut s, &g, (x(3.0), lo), (x(4.0), hi), Mods::default());
    s.arrange_up(&g, x(4.0), hi, Mods::default());
    assert!(bent(&s, bass, at(3.0), at(4.0)));
    hold(&mut s, &g, (x(5.0), lo), (x(6.0), hi), alt());
    s.arrange_up(&g, x(6.0), hi, alt());
    assert_eq!(points(&s, bass, at(5.0), at(6.0)).len(), 2);
    assert!(!bent(&s, bass, at(5.0), at(6.0)));
    assert_eq!(shape(&s), Shape::Parabola, "Alt is held, not chosen");

    // Two more: square, triangle.
    wheel_on_pencil(&mut s, &g, 2);
    assert_eq!(shape(&s), Shape::Triangle);
    hold(&mut s, &g, (x(7.0), lo), (x(9.0), hi), Mods::default());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert_eq!(plan.command.as_ref().unwrap().1["shape"], json!("triangle"));
    s.arrange_up(&g, x(9.0), hi, Mods::default());
    let tri = points(&s, bass, at(7.0), at(9.0));
    assert!(tri.len() > 4, "a triangle has its corners: {tri:?}");
    // On a wave Alt keeps its own meaning (AUT-010): one period over the stroke.
    hold(&mut s, &g, (x(10.0), lo), (x(11.0), hi), alt());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert_eq!(plan.command.as_ref().unwrap().1["period"], json!(BAR));
    s.arrange_up(&g, x(11.0), hi, alt());

    // One notch up goes back to the square; its stroke would be a square.
    wheel_on_pencil(&mut s, &g, -1);
    assert_eq!(shape(&s), Shape::Square);
    hold(&mut s, &g, (x(12.0), lo), (x(13.0), hi), Mods::default());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert_eq!(plan.command.as_ref().unwrap().1["shape"], json!("square"));
    assert!(
        s.edit_key(&g, "escape", Mods::default()),
        "let go of it unwritten"
    );

    // Three down to the sine: Mod on the way still moves its phase (AUT-010).
    wheel_on_pencil(&mut s, &g, 3);
    assert_eq!(shape(&s), Shape::Sine);
    let y = top + crate::tree::LANE_H / 2.0;
    let past = x(13.0) + s.ticks_to_px(240);
    s.arrange.drag = None;
    s.arrange_down(&g, x(12.0), y, Mods::default());
    s.arrange_move(&g, x(13.0), y, Mods::default());
    s.arrange_move(&g, past, y, ctrl());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    let args = plan.command.unwrap().1;
    assert_eq!(args["shape"], json!("sine"));
    assert_eq!(args["to"], json!(13 * BAR), "{args}");
    assert!((args["phase"].as_i64().unwrap() - 240).abs() <= 2, "{args}");
    s.arrange_up(&g, past, y, ctrl());
    assert!(s.error.is_none(), "{:?}", s.error);
}

/// The lower right corner of the first clip of `name` (a point just inside it), and the clip.
fn corner(s: &Studio, g: &Geometry, name: &str) -> ((f32, f32), xirius_core::model::Clip) {
    let t = super::probe::track(s, name);
    let c = s.project().tracks[&t].clips[0].clone();
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let r = s.clip_rect(g, &row, &c);
    ((r.right() - 2.0, r.bottom() - 2.0), c)
}

#[test]
fn edt_041_the_lower_right_corner_sets_the_loop_and_alt_its_head() {
    use xirius_core::clipfx::ClipLoop;
    let (mut s, g) = studio();
    let t = super::probe::track(&s, "Bass");
    let (at, c) = corner(&s, &g, "Bass");
    assert!(c.length >= 3 * BAR, "{}", c.length);
    let bar = s.ticks_to_px(BAR);
    let looped = |s: &Studio| s.project().clip_loop(c.id);

    // The corner is a grip of its own; a little higher up is the right edge.
    s.arrange_move(&g, at.0, at.1, Mods::default());
    let at_corner = Spot::Clip {
        track: t,
        clip: c.id,
        grip: Grip::Corner,
    };
    assert_eq!(s.arrange.hover, at_corner);
    s.arrange_move(&g, at.0, at.1 - 20.0, Mods::default());
    assert!(matches!(
        s.arrange.hover,
        Spot::Clip {
            grip: Grip::Right,
            ..
        }
    ));

    // A drag one bar to the left ends the loop a bar before the clip ends; the clip keeps its
    // length and the drawing shows the loop while it is dragged.
    let steps = s.song.steps();
    let to = (at.0 - bar, at.1);
    hold(&mut s, &g, at, to, Mods::default());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    let want = ClipLoop {
        start: 0,
        length: c.length - BAR,
    };
    assert_eq!(
        plan.command.unwrap(),
        (
            "clip.loop.set",
            json!({"ids": [c.id.to_string()], "start": 0, "length": c.length - BAR})
        )
    );
    assert_eq!(
        plan.tip,
        format!("ループ {:.2} 小節", (c.length - BAR) as f64 / BAR as f64)
    );
    assert_eq!(s.shown_loop(c.id), Some(want));
    assert_eq!(looped(&s), None, "nothing is written until it is let go");
    s.arrange_up(&g, to.0, to.1, Mods::default());
    assert!(s.error.is_none(), "{:?}", s.error);
    assert_eq!(s.song.steps(), steps + 1, "one drag, one step");
    assert_eq!(looped(&s), Some(want));
    let (_, now) = corner(&s, &g, "Bass");
    assert_eq!((now.start, now.length), (c.start, c.length));

    // With Alt the drag moves the loop's head; its end stays.
    let to = (at.0 + bar, at.1);
    hold(&mut s, &g, at, to, alt());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert!(plan.tip.starts_with("ループの頭 "), "{}", plan.tip);
    s.arrange_up(&g, to.0, to.1, alt());
    assert!(s.error.is_none(), "{:?}", s.error);
    assert_eq!(s.song.steps(), steps + 2);
    assert_eq!(
        looped(&s),
        Some(ClipLoop {
            start: BAR,
            length: c.length - 2 * BAR,
        })
    );

    // Ctrl+Z takes back the head, then the loop.
    assert!(s.edit_key(&g, "z", ctrl()));
    assert_eq!(looped(&s), Some(want));
    assert!(s.edit_key(&g, "z", ctrl()));
    assert_eq!(looped(&s), None);
}
