//! UI-329: the Delay's window, without a window.

use std::time::{Duration, Instant};

use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;

use super::delay::{self, ECHO_FOR, Echo, LIT};
use super::layout;
use crate::instrument::window::{Part, values};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// The sample song with a Delay on Keys, its window open.
fn del_window() -> (Studio, Geometry, TrackId, PluginId) {
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
        json!({ "track": t.to_string(), "name": "Delay" }),
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
        .find(|x| x.name == "Delay")
        .cloned()
        .unwrap()
}

fn plain(s: &Studio, t: TrackId, id: &str) -> f64 {
    let x = fx(s, t);
    let (k, spec) = x.builtin().unwrap().param(id).unwrap();
    spec.to_plain(values(&x)[k])
}

fn set(s: &mut Studio, id: PluginId, param: &str, value: f64) {
    assert!(s.run_command(
        "plugin.param.set",
        json!({ "id": id.to_string(), "param": param, "value": value }),
    ));
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
fn ui_329_the_taps_are_over_the_band_with_the_knobs_beside_it_and_narrow_under_it() {
    let (mut s, g, t, _) = del_window();
    assert_eq!(s.instruments.open[0].body.0, delay::NATURAL_W);
    let def = fx(&s, t).builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let b = s.instruments.body(r, &fx(&s, t), def);
    let [taps, band] = b.graphs[..] else {
        panic!("two planes")
    };
    assert!(band.y > taps.bottom());
    let knob = |b: &layout::Body, id: &str| b.slot(def.param(id).unwrap().0).unwrap().cell;
    assert!(knob(&b, "feedback").x > band.right());
    assert_eq!(knob(&b, "feedback").y, knob(&b, "mix").y);
    // The top row: Sync, the tempo, the Beats box, Ping-Pong at the right.
    let sync = knob(&b, "sync");
    let beats = knob(&b, "division");
    let tempo = b.tempo.expect("the tempo while synced");
    assert!(sync.right() < tempo.x && tempo.right() < beats.x);
    assert!(knob(&b, "ping_pong").x > beats.right());
    assert!(sync.bottom() <= taps.y);
    assert_eq!(b.names[0].text, "Color");

    s.instruments.open[0].body = layout::size(def, delay::MIN_W + 20.0);
    let r = s.instruments.frame(0, &g);
    let b = s.instruments.body(r, &fx(&s, t), def);
    let band = b.graphs[1];
    assert!(knob(&b, "feedback").y > band.bottom(), "under it");
}

#[test]
fn ui_329_the_first_repeat_moves_the_time_and_the_second_the_feedback() {
    let (mut s, g, t, id) = del_window();
    set(&mut s, id, "sync", 0.0);
    let (t0, f0) = (plain(&s, t, "time"), plain(&s, t, "feedback"));
    let before = s.song().steps();
    drag(&mut s, &g, id, 0, 0, (60.0, 0.0));
    assert_eq!(s.song().steps(), before + 1);
    assert!(plain(&s, t, "time") > t0, "right is longer");
    assert_eq!(plain(&s, t, "feedback"), f0);
    drag(&mut s, &g, id, 0, 1, (0.0, -20.0));
    assert_eq!(s.song().steps(), before + 2);
    assert!(plain(&s, t, "feedback") > f0, "up is more");
    assert!(s.undo(&g));
    assert_eq!(plain(&s, t, "feedback"), f0);
    assert!(s.undo(&g));
    assert!((plain(&s, t, "time") - t0).abs() < 1e-6);
}

#[test]
fn ui_329_synced_the_first_repeat_steps_the_note_and_keeps_the_feel() {
    let (mut s, g, t, id) = del_window();
    assert_eq!(plain(&s, t, "division"), 2.0);
    assert_eq!(plain(&s, t, "feel"), 1.0);
    let x = fx(&s, t);
    let def = x.builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let taps = s.instruments.body(r, &x, def).graphs[0];
    let step = (taps.w - 44.0) / 6.0;
    drag(&mut s, &g, id, 0, 0, (step * 1.2, 0.0));
    assert_eq!(plain(&s, t, "division"), 3.0, "a note longer");
    assert_eq!(plain(&s, t, "feel"), 1.0);
}

#[test]
fn ui_329_the_beats_box_and_its_point_offer_each_note_straight_dotted_and_triplet() {
    let (mut s, g, t, id) = del_window();
    let x = fx(&s, t);
    let def = x.builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let pts = s.instruments.points_of(r, &x, def, 0);
    assert_eq!(pts[0].name, "Beats");
    assert_eq!(pts[0].value, "1/8 D");
    let rows: Vec<_> = pts[0].menu.iter().flatten().collect();
    assert_eq!(rows.len(), 18);
    assert_eq!(pts[0].menu.iter().filter(|r| r.is_none()).count(), 2);
    let labels: Vec<_> = rows.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(&labels[..3], ["1/32", "1/16", "1/8"]);
    assert_eq!(labels[8], "1/8 D");
    assert_eq!(labels[17], "1/1 T");
    assert!(rows[8].on && rows.iter().filter(|c| c.on).count() == 1);

    // A right press on the point: a row sets the note and its feel at once.
    let (px, py) = s.instrument_center(&g, id, Part::Point(0, 0)).unwrap();
    assert!(s.instrument_right(&g, px, py));

    let before = s.song().steps();
    assert!(s.context_choose(&g, "1/16 T"));
    assert_eq!(s.song().steps(), before + 1);
    assert_eq!(plain(&s, t, "division"), 1.0);
    assert_eq!(plain(&s, t, "feel"), 2.0);
    assert!(s.undo(&g));
    assert_eq!(plain(&s, t, "division"), 2.0);
    assert_eq!(plain(&s, t, "feel"), 1.0);
}

#[test]
fn ui_329_the_cuts_move_at_the_ends_of_one_band() {
    let (mut s, g, t, id) = del_window();
    let (lo, hi) = (plain(&s, t, "low_cut"), plain(&s, t, "high_cut"));
    let x = fx(&s, t);
    let def = x.builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let pts = s.instruments.points_of(r, &x, def, 1);
    let names: Vec<_> = pts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Low Cut", "High Cut"]);
    assert_eq!(pts[0].label, "80 Hz");
    assert!(pts[0].at.0 < pts[1].at.0 && pts[0].at.1 == pts[1].at.1);
    let before = s.song().steps();
    drag(&mut s, &g, id, 1, 0, (30.0, 0.0));
    assert!(plain(&s, t, "low_cut") > lo);
    drag(&mut s, &g, id, 1, 1, (-30.0, 0.0));
    assert!(plain(&s, t, "high_cut") < hi);
    assert_eq!(s.song().steps(), before + 2);
}

#[test]
fn ui_329_the_repeats_shrink_with_the_feedback_and_ping_pong_goes_side_to_side() {
    // 35 % feedback: the second repeat is 9 dB down, the third 18.
    assert_eq!(delay::height(0.35, 1), 1.0);
    assert!((delay::height(0.35, 2) - (1.0 - 9.12 / 48.0)).abs() < 0.01);
    assert!(delay::height(0.35, 3) < delay::height(0.35, 2));
    assert_eq!(delay::height(0.0, 2), 0.0);
    // The time at 1/8 dotted is three quarters of a beat: 375 ms at 120.
    let def = xirius_params::find("delay").unwrap();
    let v: Vec<f64> = def.params.iter().map(|p| p.default_norm()).collect();
    let taps = delay::taps(def, &v);
    assert!(taps.synced);
    assert!((taps.period - 0.75).abs() < 1e-9);
    assert!((delay::seconds(&taps, 120.0) - 0.375).abs() < 1e-9);
    assert_eq!(delay::beats_label(4, 2), "1/2 T");
}

#[test]
fn ui_329_the_repeats_light_as_the_onsets_come() {
    // Lit from each repeat's time for a moment.
    assert!(delay::lit(0.0, 0.375, 0));
    assert!(!delay::lit(0.2, 0.375, 0));
    assert!(delay::lit(0.38, 0.375, 1));
    assert!(!delay::lit(0.375 + LIT + 0.01, 0.375, 1));
    assert!(delay::lit(0.76, 0.375, 2));

    // The first look learns the count; a rise is an onset.
    let t0 = Instant::now();
    let mut e = Echo::default();
    assert!(!e.feed(4, t0));
    assert_eq!(e.since(t0), None);
    let t1 = t0 + Duration::from_millis(50);
    assert!(e.feed(5, t1));
    assert_eq!(e.since(t1), Some(0.0));
    let t2 = t1 + Duration::from_millis(400);
    assert!(e.feed(5, t2));
    assert!((e.since(t2).unwrap() - 0.4).abs() < 1e-6);
    assert!(!e.feed(5, t1 + ECHO_FOR));
    assert_eq!(e.since(t1 + ECHO_FOR), None);
}

#[test]
fn ui_329_the_window_watches_its_probe_for_onsets() {
    use std::sync::Arc;
    use xirius_engine::stock::Probe;
    let (mut s, _, _, id) = del_window();
    let probe = Arc::new(Probe::new());
    let p = s.project();
    let find = |x: PluginId| (x == id).then(|| probe.clone());
    s.instruments.poll_with(&p, 48_000.0, Instant::now(), find);
    assert!(probe.watching());
    assert!(s.instruments.echoes.contains_key(&id));
}

#[test]
fn ui_329_the_beats_box_and_the_time_box_read_their_values() {
    let (mut s, g, _, id) = del_window();
    let of = |s: &Studio, p: &str| {
        crate::a11y::marks(s, &g)
            .into_iter()
            .find(|m| m.key == format!("device-{id}-{p}"))
            .unwrap_or_else(|| panic!("no mark for {p}"))
            .node
    };
    let beats = of(&s, "division");
    assert_eq!(beats.role(), accesskit::Role::ComboBox);
    assert_eq!(beats.label(), Some("Beats"));
    assert_eq!(beats.value(), Some("1/8 D"));
    assert_eq!(of(&s, "ping_pong").label(), Some("Ping-Pong"));
    set(&mut s, id, "sync", 0.0);
    let time = of(&s, "time");
    assert_eq!(time.role(), accesskit::Role::Slider);
    assert_eq!(time.label(), Some("Time"));
    assert_eq!(time.value(), Some("375 ms"));
}
