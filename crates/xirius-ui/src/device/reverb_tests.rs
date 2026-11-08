//! UI-330: the Reverb's window, without a window.

use std::sync::Arc;
use std::time::Instant;

use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;
use xirius_engine::stock::Probe;

use super::layout;
use super::reverb::{self, FLOOR, GRIP, LIT};
use crate::instrument::window::{Part, values};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// The sample song with a Reverb on Keys, its window open.
fn rev_window() -> (Studio, Geometry, TrackId, PluginId) {
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let t = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "Reverb" }),
    ));
    let id = fx(&s, t).id;
    s.show_instrument(t, id);
    assert!(s.instruments.is_open(id));
    (s, g, t, id)
}

fn fx(s: &Studio, t: TrackId) -> Plugin {
    s.project().tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == "Reverb")
        .cloned()
        .unwrap()
}

fn plain(s: &Studio, t: TrackId, id: &str) -> f64 {
    let x = fx(s, t);
    let (k, spec) = x.builtin().unwrap().param(id).unwrap();
    spec.to_plain(values(&x)[k])
}

/// Drags the `at`th point of the `n`th plane by `(dx, dy)` and lets go.
fn drag(s: &mut Studio, g: &Geometry, id: PluginId, n: usize, at: usize, d: (f32, f32)) {
    let (x, y) = s
        .instrument_center(g, id, Part::Point(n, at))
        .expect("the point");
    assert!(s.instrument_press(g, x, y, 1, Mods::NONE));
    for i in 1..=10 {
        let f = i as f32 / 10.0;
        s.instrument_moved(g, x + d.0 * f, y + d.1 * f, false);
    }
    assert!(s.instrument_up());
}

#[test]
fn ui_330_the_plane_is_over_the_color_band_and_the_six_knobs_wrap_when_narrow() {
    let (mut s, g, t, _) = rev_window();
    assert_eq!(s.instruments.open[0].body.0, reverb::NATURAL_W);
    let def = fx(&s, t).builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let b = s.instruments.body(r, &fx(&s, t), def);
    let [plane, band] = b.graphs[..] else {
        panic!("two planes")
    };
    assert!(band.y > plane.bottom());
    assert_eq!(b.names[0].text, "Color");
    let knob = |b: &layout::Body, id: &str| b.slot(def.param(id).unwrap().0).unwrap().cell;
    let cells: Vec<_> = reverb::KNOBS.iter().map(|id| knob(&b, id)).collect();
    assert!(
        cells
            .iter()
            .all(|c| c.y == cells[0].y && c.y > band.bottom())
    );
    assert!(cells.windows(2).all(|w| w[0].right() < w[1].x));
    for id in reverb::COLOR {
        assert!(
            b.slot(def.param(id).unwrap().0).is_none(),
            "{id} is a point"
        );
    }

    s.instruments.open[0].body = layout::size(def, reverb::MIN_W);
    let r = s.instruments.frame(0, &g);
    let b = s.instruments.body(r, &fx(&s, t), def);
    let cells: Vec<_> = reverb::KNOBS.iter().map(|id| knob(&b, id)).collect();
    assert_eq!(cells[0].y, cells[2].y);
    assert!(cells[3].y > cells[0].y, "two rows");
    assert!(b.graphs[0].h < plane.h, "a lower plane");
    assert!(cells.iter().all(|c| c.right() <= b.graphs[1].right()));
}

#[test]
fn ui_330_the_points_sit_where_the_engine_puts_the_sound() {
    let (s, g, t, _) = rev_window();
    let x = fx(&s, t);
    let def = x.builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let plane = s.instruments.body(r, &x, def).graphs[0];
    let pts = s.instruments.points_of(r, &x, def, 0);
    let names: Vec<_> = pts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Pre-delay", "Decay", "Early"]);
    assert_eq!(pts[0].value, "10.0 ms");
    assert_eq!(pts[2].value, "Size 50 %, Early 40 %");
    let v = values(&x);
    let sound = reverb::shape(def, &v, 48_000.0);
    let (tx, ly) = (reverb::time_axis(plane), reverb::level_axis(plane));
    // The pre-delay at the foot, the early point on the fourth reflection.
    assert!((pts[0].at.0 - tx.px(0.010)).abs() < 0.01);
    assert!((pts[0].at.1 - ly.px(FLOOR)).abs() < 0.01);
    let (at, gain) = sound.reflections[GRIP];
    assert!((pts[2].at.0 - tx.px(at)).abs() < 0.01);
    let level = 20.0 * (sound.wet * 0.4 * gain).log10();
    assert!((pts[2].at.1 - ly.px(level)).abs() < 0.01);
    // The decay's halfway along its tail, on it.
    let half = 0.010 + 2.0 / 2.0;
    assert!((pts[1].at.0 - tx.px(half)).abs() < 0.01);
    assert!((pts[1].at.1 - ly.px(sound.tail(half, sound.decay))).abs() < 0.01);
    // The tail starts after the first line's length and the reflections
    // before it; the highs die sooner.
    assert!(sound.reflections[0].0 < sound.start);
    assert!(sound.high < sound.decay);
    // The band's ends are Low Cut and Damping.
    let band = s.instruments.points_of(r, &x, def, 1);
    let names: Vec<_> = band.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Low Cut", "Damping"]);
    assert_eq!(band[1].label, "7.00 kHz");
}

#[test]
fn ui_330_the_drawn_tails_are_the_engines_decays() {
    let def = xirius_params::find("reverb").unwrap();
    let mut v: Vec<f64> = def.params.iter().map(|p| p.default_norm()).collect();
    let set = |v: &mut Vec<f64>, id: &str, plain: f64| {
        let (k, spec) = def.param(id).unwrap();
        v[k] = spec.to_norm(plain);
    };
    let s = reverb::shape(def, &v, 48_000.0);
    let at = |hz| xirius_engine::stock::reverb::decay_at(50.0, 2.0, 7000.0, hz, 48_000.0);
    assert_eq!(s.decay, at(1000.0));
    assert_eq!(s.high, at(8000.0));
    // The tail falls 60 dB over its decay.
    assert!((s.tail(s.start + s.decay, s.decay) - (s.top + FLOOR)).abs() < 1e-9);
    // Less damping: the highs last longer; a longer decay: both do.
    set(&mut v, "damping", 15_000.0);
    let brighter = reverb::shape(def, &v, 48_000.0);
    assert!(brighter.high > s.high);
    set(&mut v, "decay", 4.0);
    assert!(reverb::shape(def, &v, 48_000.0).decay > brighter.decay);
    // All wet: no dry column, the tail at its loudest.
    set(&mut v, "mix", 100.0);
    let wet = reverb::shape(def, &v, 48_000.0);
    assert_eq!((wet.dry, wet.wet), (0.0, 1.0));
    assert!(wet.top > s.top);
}

#[test]
fn ui_330_the_foot_moves_the_pre_delay_and_the_tail_the_decay() {
    let (mut s, g, t, id) = rev_window();
    let (p0, d0, s0, e0) = (
        plain(&s, t, "predelay"),
        plain(&s, t, "decay"),
        plain(&s, t, "size"),
        plain(&s, t, "early"),
    );
    let before = s.song().steps();
    drag(&mut s, &g, id, 0, 0, (40.0, 0.0));
    assert!(plain(&s, t, "predelay") > p0, "right is later");
    drag(&mut s, &g, id, 0, 1, (60.0, 0.0));
    assert!(plain(&s, t, "decay") > d0, "right is longer");
    // The early point: across the size, up the early level, in one step.
    drag(&mut s, &g, id, 0, 2, (40.0, -30.0));
    assert!(plain(&s, t, "size") > s0);
    assert!(plain(&s, t, "early") > e0);
    assert_eq!(s.song().steps(), before + 3);
    assert!(s.undo(&g));
    assert_eq!((plain(&s, t, "size"), plain(&s, t, "early")), (s0, e0));
    assert!(s.undo(&g));
    assert!((plain(&s, t, "decay") - d0).abs() < 1e-6);
    assert!(s.undo(&g));
    assert!((plain(&s, t, "predelay") - p0).abs() < 1e-6);
}

#[test]
fn ui_330_a_drag_on_the_log_scale_lands_where_the_pointer_is() {
    // The pre-delay's and the decay's axes are the time's own: a point
    // dragged to a time sets the value that puts it there.
    let (mut s, g, t, id) = rev_window();
    let x = fx(&s, t);
    let def = x.builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let plane = s.instruments.body(r, &x, def).graphs[0];
    let tx = reverb::time_axis(plane);
    drag(&mut s, &g, id, 0, 0, (tx.px(0.050) - tx.px(0.010), 0.0));
    assert!((plain(&s, t, "predelay") - 50.0).abs() < 1.0);
    // Halfway along the tail at 2 s is 1.05 s with 50 ms of pre-delay; at
    // 4 s it is 2.05 s.
    let pre = plain(&s, t, "predelay") * 0.001;
    drag(
        &mut s,
        &g,
        id,
        0,
        1,
        (tx.px(pre + 2.0) - tx.px(pre + 1.0), 0.0),
    );
    assert!((plain(&s, t, "decay") - 4.0).abs() < 0.1);
}

#[test]
fn ui_330_the_tails_point_offers_the_usual_decays_and_its_wheel_the_damping() {
    let (mut s, g, t, id) = rev_window();
    let x = fx(&s, t);
    let def = x.builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let pts = s.instruments.points_of(r, &x, def, 0);
    let labels: Vec<_> = pts[1]
        .menu
        .iter()
        .flatten()
        .map(|c| c.label.as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "Decay を 0.8 s に",
            "Decay を 1.5 s に",
            "Decay を 2.5 s に",
            "Decay を 4 s に"
        ]
    );
    let (px, py) = s.instrument_center(&g, id, Part::Point(0, 1)).unwrap();
    assert!(s.instrument_right(&g, px, py));

    assert!(s.context_choose(&g, "Decay を 4 s に"));
    assert!((plain(&s, t, "decay") - 4.0).abs() < 1e-6);
    let x = fx(&s, t);
    let pts = s.instruments.points_of(r, &x, def, 0);
    assert!(pts[1].menu[3].as_ref().unwrap().on);

    let d0 = plain(&s, t, "damping");
    let (px, py) = s.instrument_center(&g, id, Part::Point(0, 1)).unwrap();
    assert!(s.device_wheel(&g, px, py, 1.0, false));
    assert!(s.device_sweep_end());
    assert!(plain(&s, t, "damping") > d0);
    assert!((plain(&s, t, "decay") - 4.0).abs() < 1e-6);
}

#[test]
fn ui_330_the_band_moves_the_low_cut_and_the_damping() {
    let (mut s, g, t, id) = rev_window();
    let (lo, hi) = (plain(&s, t, "low_cut"), plain(&s, t, "damping"));
    drag(&mut s, &g, id, 1, 0, (30.0, 0.0));
    assert!(plain(&s, t, "low_cut") > lo);
    drag(&mut s, &g, id, 1, 1, (-30.0, 0.0));
    assert!(plain(&s, t, "damping") < hi);
}

#[test]
fn ui_330_the_reflections_light_and_a_dot_runs_down_the_tail() {
    assert!(reverb::lit(0.02, 0.02));
    assert!(!reverb::lit(0.02 + LIT + 0.001, 0.02));
    assert!(!reverb::lit(0.0, 0.02));
    let def = xirius_params::find("reverb").unwrap();
    let v: Vec<f64> = def.params.iter().map(|p| p.default_norm()).collect();
    let s = reverb::shape(def, &v, 48_000.0);
    assert_eq!(reverb::live(&s, s.start / 2.0), None);
    let (t, d) = reverb::live(&s, s.start + s.decay / 2.0).unwrap();
    assert!((d - (s.top + FLOOR / 2.0)).abs() < 1e-9);
    assert_eq!(t, s.start + s.decay / 2.0);
    assert_eq!(reverb::live(&s, s.start + s.decay + 0.01), None);

    // The window watches its probe's onsets as the Delay's does.
    let (mut st, _, _, id) = rev_window();
    let probe = Arc::new(Probe::new());
    let p = st.project();
    let find = |x: PluginId| (x == id).then(|| probe.clone());
    st.instruments.poll_with(&p, 48_000.0, Instant::now(), find);
    assert!(probe.watching());
    assert!(st.instruments.echoes.contains_key(&id));
}

#[test]
fn ui_330_the_knobs_read_their_names_and_values() {
    let (s, g, _, id) = rev_window();
    let marks = crate::a11y::marks(&s, &g);
    let of = |p: &str| {
        marks
            .iter()
            .find(|m| m.key == format!("device-{id}-{p}"))
            .unwrap_or_else(|| panic!("no mark for {p}"))
            .node
            .clone()
    };
    let want = [
        ("size", "Size", "50 %"),
        ("decay", "Decay", "2.00 s"),
        ("predelay", "Pre-delay", "10.0 ms"),
        ("early", "Early", "40 %"),
        ("width", "Width", "100 %"),
        ("mix", "Mix", "25 %"),
    ];
    for (p, label, value) in want {
        let n = of(p);
        assert_eq!(n.role(), accesskit::Role::Slider);
        assert_eq!(n.label(), Some(label), "{p}");
        assert_eq!(n.value(), Some(value), "{p}");
    }
}
