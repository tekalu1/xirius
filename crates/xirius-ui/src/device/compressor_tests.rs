//! UI-327: the Compressor's window, without a window.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;
use xirius_engine::stock::{Probe, reduction};

use super::compressor::{self, SPAN};
use super::dynamics::{self, Dynamics, STEP};
use super::layout;
use crate::instrument::window::{Part, values};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// The sample song with a Compressor on Keys, its window open.
fn cmp_window() -> (Studio, Geometry, TrackId, PluginId) {
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
        json!({ "track": t.to_string(), "name": "Compressor" }),
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
        .find(|x| x.name == "Compressor")
        .cloned()
        .unwrap()
}

fn plain(s: &Studio, t: TrackId, id: &str) -> f64 {
    let x = fx(s, t);
    let (k, spec) = x.builtin().unwrap().param(id).unwrap();
    spec.to_plain(values(&x)[k])
}

/// Makes the window `w` wide.
fn resize(s: &mut Studio, t: TrackId, w: f32) {
    let def = fx(s, t).builtin().unwrap();
    s.instruments.open[0].body = layout::size(def, w);
}

/// Drags the `at`th point of the curve by `(dx, dy)` and lets go.
fn drag(s: &mut Studio, g: &Geometry, id: PluginId, at: usize, (dx, dy): (f32, f32)) {
    let (x, y) = s
        .instrument_center(g, id, Part::Point(0, at))
        .expect("the point");
    assert!(s.instrument_press(g, x, y, 1, Mods::NONE));
    for i in 1..=10 {
        let f = i as f32 / 10.0;
        s.instrument_moved(g, x + dx * f, y + dy * f, false);
    }
    assert!(s.instrument_up());
}

#[test]
fn ui_327_the_curve_is_the_engines_without_the_makeup() {
    let knobs = (-20.0, 4.0, 0.0);
    // Below the threshold it passes; above, a quarter of what is over.
    assert_eq!(compressor::out_db(-30.0, knobs), -30.0);
    assert!((compressor::out_db(-10.0, knobs) - -17.5).abs() < 1e-9);
    let r = crate::studio::Rect::new(0.0, 0.0, 200.0, 200.0);
    let (ax, ay) = (compressor::in_axis(r), compressor::out_axis(r));
    for (x, y) in compressor::curve(r, (-24.0, 3.0, 6.0)) {
        let v = ax.value(x);
        let want = v - reduction(v, -24.0, 3.0, 6.0);
        assert!((y - ay.px(want)).abs() < 0.01, "at {v} dB");
    }
}

#[test]
fn ui_327_it_opens_wide_with_the_history_beside_the_curve_and_narrow_under_it() {
    let (mut s, g, t, _) = cmp_window();
    assert_eq!(s.instruments.open[0].body.0, compressor::NATURAL_W);
    let def = fx(&s, t).builtin().unwrap();
    let r = s.instruments.rect(0, g.size);
    let b = s.instruments.body(r, &fx(&s, t), def);
    let [xfer, hist] = b.graphs[..] else {
        panic!("two planes")
    };
    assert_eq!(xfer.w, xfer.h, "the curve's plane is square");
    assert!(hist.x > xfer.right() && hist.y == xfer.y);
    assert_eq!(b.levels.len(), 3);
    assert!(b.levels[0].name.x > hist.right());
    let knobs: Vec<_> = compressor::KNOBS
        .iter()
        .map(|id| b.slot(def.param(id).unwrap().0).unwrap().cell)
        .collect();
    assert!(knobs.iter().all(|c| c.y == knobs[0].y), "one row");
    for id in ["filter", "listen", "low_cut", "high_cut"] {
        assert!(b.slot(def.param(id).unwrap().0).is_some(), "{id}");
    }

    resize(&mut s, t, compressor::MIN_W + 40.0);
    let r = s.instruments.rect(0, g.size);
    let b = s.instruments.body(r, &fx(&s, t), def);
    let [xfer, hist] = b.graphs[..] else {
        panic!("two planes")
    };
    assert!(hist.y > xfer.bottom(), "the history goes under");
    assert!(b.levels[0].name.x > xfer.right());
    let rows: std::collections::BTreeSet<i32> = compressor::KNOBS
        .iter()
        .map(|id| b.slot(def.param(id).unwrap().0).unwrap().cell.y as i32)
        .collect();
    assert_eq!(rows.len(), 2, "two rows of four");
}

#[test]
fn ui_327_the_threshold_and_ratio_points_are_each_one_step() {
    let (mut s, g, t, id) = cmp_window();
    let (t0, r0) = (plain(&s, t, "threshold"), plain(&s, t, "ratio"));
    let before = s.song().steps();
    drag(&mut s, &g, id, 1, (-30.0, 0.0));
    assert_eq!(s.song().steps(), before + 1);
    assert!(plain(&s, t, "threshold") < t0);
    assert_eq!(plain(&s, t, "ratio"), r0);
    // A drag down makes the ratio stronger.
    drag(&mut s, &g, id, 0, (0.0, 30.0));
    assert_eq!(s.song().steps(), before + 2);
    assert!(plain(&s, t, "ratio") > r0);
    assert!(s.undo(&g));
    assert_eq!(plain(&s, t, "ratio"), r0);
    assert!(s.undo(&g));
    assert_eq!(plain(&s, t, "threshold"), t0);
}

#[test]
fn ui_327_the_history_keeps_four_seconds_and_the_readouts_their_most() {
    let mut d = Dynamics::default();
    let t0 = Instant::now();
    let loud = ([0.5, 0.25], [0.9, 0.8]);
    for i in 0..400u32 {
        d.feed(loud, 6.0, t0 + STEP * i, SPAN);
    }
    let cap = (SPAN.as_millis() / STEP.as_millis()) as usize;
    assert_eq!(d.history.len(), cap);
    let last = *d.history.back().unwrap();
    assert!((last.input - dynamics::db(0.5)).abs() < 1e-4);
    assert_eq!(last.gr, 6.0);
    assert_eq!(d.readout(0), "-6.0");
    assert_eq!(d.readout(1), "-6.0");
    assert_eq!(d.readout(2), "-0.9");
    assert!(!d.over());
    // A long gap fills the history at once, not past its span.
    d.feed(
        ([1.2, 0.0], [1.5, 0.0]),
        0.0,
        t0 + Duration::from_secs(60),
        SPAN,
    );
    assert_eq!(d.history.len(), cap);
    assert!(d.over());
    d.clear(2);
    assert_eq!(d.readout(2), "-\u{221e}");
    assert!(!d.over());
}

#[test]
fn ui_327_a_press_on_a_readout_clears_its_most() {
    let (mut s, g, _, id) = cmp_window();
    let d = s.instruments.dynamics.entry(id).or_default();
    d.feed(([0.5, 0.5], [0.5, 0.5]), 3.0, Instant::now(), SPAN);
    let (x, y) = s.instrument_center(&g, id, Part::Readout(1)).unwrap();
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    s.instrument_up();
    let d = &s.instruments.dynamics[&id];
    assert_eq!(d.readout(1), "0.0");
    assert_eq!(d.readout(0), "-6.0", "the others keep theirs");
}

#[test]
fn ui_327_the_probe_is_watched_while_the_window_is_open() {
    let (mut s, g, _, id) = cmp_window();
    let probe = Arc::new(Probe::new());
    let p = s.project();
    let find = |x: PluginId| (x == id).then(|| probe.clone());
    s.instruments.poll_with(&p, 48_000.0, Instant::now(), find);
    assert!(probe.watching());
    assert!(s.instruments.dynamics.contains_key(&id));
    let (x, y) = s.instrument_center(&g, id, Part::Close).unwrap();
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    s.instrument_up();
    let p = s.project();
    s.instruments.poll_with(&p, 48_000.0, Instant::now(), find);
    assert!(!probe.watching());
}

#[test]
fn ui_327_the_gain_reads_by_what_it_drives_and_the_readouts_are_buttons() {
    let (mut s, g, t, id) = cmp_window();
    let of = |s: &Studio, p: &str| {
        crate::a11y::marks(s, &g)
            .into_iter()
            .find(|m| m.key == format!("device-{id}-{p}"))
            .unwrap_or_else(|| panic!("no mark for {p}"))
            .node
    };
    assert_eq!(of(&s, "gain").label(), Some("Input Gain"));
    assert_eq!(of(&s, "threshold").role(), accesskit::Role::Slider);
    let peak = of(&s, "readout-0");
    assert_eq!(peak.role(), accesskit::Role::Button);
    assert_eq!(peak.label(), Some("Input peak"));
    assert_eq!(peak.value(), Some("-\u{221e}"));
    assert_eq!(of(&s, "readout-1").label(), Some("Reduction peak"));
    assert_eq!(of(&s, "readout-2").label(), Some("Output peak"));
    let drums = s
        .project()
        .tracks
        .values()
        .find(|x| x.name != "Keys" && x.kind == xirius_core::model::TrackKind::Audio)
        .map(|x| x.id)
        .unwrap();
    assert!(
        s.run_command(
            "plugin.sidechain.add",
            json!({ "id": id.to_string(), "from": drums.to_string() }),
        ),
        "{:?}",
        s.error
    );
    if !fx(&s, t).sidechain.is_some_and(|x| x.on) {
        assert!(s.run_command(
            "plugin.sidechain.enable",
            json!({ "id": id.to_string(), "state": "on" }),
        ));
    }
    assert_eq!(of(&s, "gain").label(), Some("Side Gain"));
}
