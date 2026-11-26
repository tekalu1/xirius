//! UI-328: the Limiter's window, without a window.

use std::time::Instant;

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;

use super::dynamics::{self, Dynamics, STEP};
use super::layout;
use super::limiter::{self, CEILINGS, SPAN};
use crate::instrument::window::{Part, values};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// The sample song with a Limiter on Keys, its window open.
fn lim_window() -> (Studio, Geometry, TrackId, PluginId) {
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
        json!({ "track": t.to_string(), "name": "Limiter" }),
    ));
    let id = fx(&s, t).id;
    s.show_instrument(t, id);
    s.instruments.detached_target = Some(id);
    assert!(s.instruments.is_open(id));
    (s, g, t, id)
}

fn fx(s: &Studio, t: TrackId) -> Plugin {
    s.project().tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == "Limiter")
        .cloned()
        .unwrap()
}

fn plain(s: &Studio, t: TrackId, id: &str) -> f64 {
    let x = fx(s, t);
    let (k, spec) = x.builtin().unwrap().param(id).unwrap();
    spec.to_plain(values(&x)[k])
}

/// Drags the `at`th point of the history by `dy` and lets go.
fn drag(s: &mut Studio, g: &Geometry, id: PluginId, at: usize, dy: f32) {
    let (x, y) = s
        .instrument_center(g, id, Part::Point(0, at))
        .expect("the point");
    assert!(s.instrument_press(g, x, y, 1, Mods::NONE));
    for i in 1..=10 {
        s.instrument_moved(g, x, y + dy * i as f32 / 10.0, false);
    }
    assert!(s.instrument_up());
}

#[test]
fn ui_328_it_opens_with_the_meters_beside_the_history_and_narrow_under_it() {
    let (mut s, g, t, _) = lim_window();
    assert_eq!(s.instruments.open[0].body.0, limiter::NATURAL_W);
    let def = fx(&s, t).builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let b = s.instruments.body(r, &fx(&s, t), def);
    let [hist] = b.graphs[..] else {
        panic!("one plane")
    };
    let names: Vec<_> = b.levels.iter().map(|l| l.label).collect();
    assert_eq!(names, ["Input", "Reduction", "Output"]);
    assert!(b.levels[0].name.x > hist.right());
    let knobs: Vec<_> = limiter::KNOBS
        .iter()
        .map(|id| b.slot(def.param(id).unwrap().0).unwrap().cell)
        .collect();
    assert!(
        knobs
            .iter()
            .all(|c| c.y == knobs[0].y && c.y > hist.bottom())
    );

    s.instruments.open[0].body = layout::size(def, limiter::MIN_W + 20.0);
    let r = s.instruments.frame(0, &g);
    let b = s.instruments.body(r, &fx(&s, t), def);
    let [hist] = b.graphs[..] else {
        panic!("one plane")
    };
    assert!(
        b.levels.iter().all(|l| l.name.y > hist.bottom()),
        "under it"
    );
}

#[test]
fn ui_328_the_ceiling_and_gain_points_drag_up_and_down_in_one_step_each() {
    let (mut s, g, t, id) = lim_window();
    let (c0, g0) = (plain(&s, t, "ceiling"), plain(&s, t, "input"));
    let before = s.song().steps();
    drag(&mut s, &g, id, 1, 40.0);
    assert_eq!(s.song().steps(), before + 1);
    assert!(plain(&s, t, "ceiling") < c0, "down lowers the ceiling");
    assert_eq!(plain(&s, t, "input"), g0);
    drag(&mut s, &g, id, 0, -40.0);
    assert_eq!(s.song().steps(), before + 2);
    assert!(plain(&s, t, "input") > g0, "up raises the gain");
    assert!(s.undo(&g));
    assert_eq!(plain(&s, t, "input"), g0);
    assert!(s.undo(&g));
    assert!((plain(&s, t, "ceiling") - c0).abs() < 1e-9);
}

#[test]
fn ui_328_the_ceilings_point_offers_the_usual_ceilings() {
    let (s, g, t, id) = lim_window();
    let x = fx(&s, t);
    let def = x.builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let pts = s.instruments.points_of(r, &x, def, 0);
    let menu: Vec<_> = pts[1].menu.iter().flatten().collect();
    let labels: Vec<_> = menu.iter().map(|c| c.label.clone()).collect();
    let want: Vec<_> = CEILINGS
        .iter()
        .map(|c| format!("Ceiling を {c:.1} dB に"))
        .collect();
    assert_eq!(labels, want);
    // The default is the one ticked.
    let (k, spec) = def.param("ceiling").unwrap();
    let default = spec.to_plain(spec.default_norm());
    assert_eq!(
        menu.iter().map(|c| c.on).collect::<Vec<_>>(),
        CEILINGS
            .iter()
            .map(|c| (c - default).abs() < 0.01)
            .collect::<Vec<_>>()
    );
    assert_eq!(menu.iter().filter(|c| c.on).count(), 1);
    for (c, want) in menu.iter().zip(CEILINGS) {
        assert_eq!(c.set.len(), 1);
        assert_eq!(c.set[0].0, k);
        assert!((spec.to_plain(c.set[0].1) - want).abs() < 1e-6);
    }
    let _ = id;
}

#[test]
fn ui_328_a_right_press_on_the_ceiling_sets_it_from_its_menu() {
    let (mut s, g, t, id) = lim_window();
    let (x, y) = s.instrument_center(&g, id, Part::Point(0, 1)).unwrap();
    assert!(s.instrument_right(&g, x, y));

    assert!(s.context_choose(&g, "Ceiling を -1.0 dB に"));
    assert!((plain(&s, t, "ceiling") - -1.0).abs() < 1e-6);
}

#[test]
fn ui_328_the_history_keeps_five_seconds_with_the_gain_on_the_input() {
    let (s, _, _, _) = lim_window();
    let mut d = Dynamics::default();
    let t0 = Instant::now();
    for i in 0..400u32 {
        d.feed(([0.5, 0.5], [0.9, 0.9]), 1.0, t0 + STEP * i, SPAN);
    }
    assert_eq!(
        d.history.len(),
        (SPAN.as_millis() / STEP.as_millis()) as usize
    );
    // The input meter shows what the gain made of what the probe heard.
    let x = fx(&s, s.instruments.open[0].track);
    let def = x.builtin().unwrap();
    assert_eq!(dynamics::pre(def, &values(&x)), 1.0);
    let (k, spec) = def.param("input").unwrap();
    let mut v = values(&x);
    v[k] = spec.to_norm(6.0);
    assert!((dynamics::pre(def, &v) - 1.995).abs() < 0.01);
}
