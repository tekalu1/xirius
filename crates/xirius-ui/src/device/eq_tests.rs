//! UI-326: the EQ's window, without a window.

use std::time::Instant;

use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;
use xirius_engine::stock::Tap;

use super::eq;
use super::layout;
use crate::instrument::window::{Part, values};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// The sample song with an EQ on Keys, its window open.
fn eq_window() -> (Studio, Geometry, TrackId, PluginId) {
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
        json!({ "track": t.to_string(), "name": "EQ" }),
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
        .find(|x| x.name == "EQ")
        .cloned()
        .unwrap()
}

fn plain(s: &Studio, t: TrackId, id: &str) -> f64 {
    let x = fx(s, t);
    let (k, spec) = x.builtin().unwrap().param(id).unwrap();
    spec.to_plain(values(&x)[k])
}

fn k_of(s: &Studio, t: TrackId, id: &str) -> usize {
    fx(s, t).builtin().unwrap().param(id).unwrap().0
}

fn press(s: &mut Studio, g: &Geometry, id: PluginId, part: Part) {
    let (x, y) = s.instrument_center(g, id, part).expect("in sight");
    assert!(s.instrument_press(g, x, y, 1, Mods::NONE));
    s.instrument_up();
}

/// Makes the window `w` wide.
fn resize(s: &mut Studio, t: TrackId, w: f32) {
    let def = fx(s, t).builtin().unwrap();
    s.instruments.open[0].body = layout::size(def, w);
}

#[test]
fn ui_326_it_opens_wide_on_the_lmf_band_and_the_output_spectrum() {
    let (s, _, _, id) = eq_window();
    assert_eq!(s.instruments.open[0].body.0, eq::NATURAL_W);
    assert_eq!(s.instruments.band(id), 2);
    assert_eq!(s.instruments.tap(id), Tap::Output);
}

#[test]
fn ui_326_a_point_drag_is_one_step_and_each_frame_keeps_up() {
    let (mut s, g, t, id) = eq_window();
    let (fx0, gx0) = (plain(&s, t, "mid1_freq"), plain(&s, t, "mid1_gain"));
    let before = s.song().steps();
    let (x, y) = s
        .instrument_center(&g, id, Part::Point(0, 2))
        .expect("the LMF point");
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    let def = fx(&s, t).builtin().unwrap();
    let r = s.instruments.rect(0, g.size);
    let plane = s.instruments.body(r, &fx(&s, t), def).graphs[0];
    let mut frames = Vec::new();
    for d in 1..=120 {
        let start = Instant::now();
        assert!(s.instrument_moved(&g, x + d as f32, y - d as f32 * 0.5, false));
        // What a frame draws: the curve through the values it is dragged to.
        let shown = s.instruments.shown(&fx(&s, t));
        let _ = eq::curves(&shown, plane, Some(2), 48_000.0);
        frames.push(start.elapsed());
    }
    frames.sort();
    let p99 = frames[frames.len() * 99 / 100];
    assert!(p99.as_secs_f64() * 1000.0 <= 16.7, "p99 {p99:?}");
    // The song has not taken it yet.
    assert_eq!(plain(&s, t, "mid1_freq"), fx0);
    assert!(s.instrument_up());
    assert_eq!(s.song().steps(), before + 1, "x and y in one step");
    assert!(plain(&s, t, "mid1_freq") > fx0);
    assert!(plain(&s, t, "mid1_gain") > gx0);
    assert!(s.undo(&g));
    assert_eq!(plain(&s, t, "mid1_freq"), fx0);
    assert_eq!(plain(&s, t, "mid1_gain"), gx0);
}

#[test]
fn ui_326_the_spectrum_menu_chooses_what_the_analyser_shows() {
    let (mut s, g, _, id) = eq_window();
    press(&mut s, &g, id, Part::Spectrum);
    let rows = s.context_rows();
    let labels: Vec<_> = rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, eq::SPECTRUM);
    let on: Vec<_> = rows
        .iter()
        .filter(|r| r.on)
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(on, ["Output"]);
    assert!(s.context_choose(&g, "Input + Output"));
    assert_eq!(s.instruments.tap(id), Tap::Both);
    // The button grows with what it says.
    let both = s.instrument_center(&g, id, Part::Spectrum).unwrap();
    assert!(s.run_binding(&g, &format!("plugin.analyzer --id {id} --spectrum off")));
    assert_eq!(s.instruments.tap(id), Tap::Off);
    let off = s.instrument_center(&g, id, Part::Spectrum).unwrap();
    assert!(off.0 < both.0);
}

#[test]
fn ui_326_a_press_in_a_column_or_on_a_tab_chooses_the_band() {
    let (mut s, g, t, id) = eq_window();
    let k = k_of(&s, t, "high_freq");
    press(&mut s, &g, id, Part::Knob(k));
    assert_eq!(s.instruments.band(id), 4);
    // Narrow, one band shows and the tabs choose it.
    resize(&mut s, t, 480.0);
    let r = s.instruments.rect(0, g.size);
    let def = fx(&s, t).builtin().unwrap();
    let body = s.instruments.body(r, &fx(&s, t), def);
    assert_eq!(body.tabs.len(), 6);
    assert!(body.tabs[4].chosen);
    assert!(body.slot(k_of(&s, t, "mid1_freq")).is_none());
    press(&mut s, &g, id, Part::Tab(2));
    assert_eq!(s.instruments.band(id), 2);
    let body = s.instruments.body(r, &fx(&s, t), def);
    assert!(body.slot(k_of(&s, t, "mid1_freq")).is_some());
    assert!(body.slot(k).is_none());
}

#[test]
fn ui_326_the_dot_switches_a_band_and_the_type_menu_sets_its_type() {
    let (mut s, g, t, id) = eq_window();
    let before = s.song().steps();
    let on = k_of(&s, t, "b3_on");
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
    press(&mut s, &g, id, Part::Knob(on));
    assert_eq!(plain(&s, t, "b3_on"), 0.0);
    assert_eq!(s.song().steps(), before + 1);

    let kind = k_of(&s, t, "b3_type");
    press(&mut s, &g, id, Part::Knob(kind));
    let rows = s.context_rows();
    let labels: Vec<_> = rows.iter().map(|r| r.label.as_str()).collect();
    let types: Vec<_> = xirius_params::EQ_BANDS[2]
        .types
        .iter()
        .map(|t| t.label())
        .collect();
    assert_eq!(labels, types);
    assert!(s.context_choose(&g, "Notch"));
    let kind = xirius_params::EQ_BANDS[2].type_of(plain(&s, t, "b3_type"));
    assert_eq!(kind, xirius_params::BandType::Notch);
    assert_eq!(s.song().steps(), before + 2);
}

#[test]
fn ui_326_the_tabs_the_spectrum_and_the_head_read_their_names() {
    let (mut s, g, t, id) = eq_window();
    let marks = crate::a11y::marks(&s, &g);
    let of = |marks: &[crate::a11y::Mark], p: &str| {
        marks
            .iter()
            .find(|m| m.key == format!("device-{id}-{p}"))
            .unwrap_or_else(|| panic!("no mark for {p}"))
            .node
            .clone()
    };
    let spec = of(&marks, "spectrum");
    assert_eq!(spec.role(), accesskit::Role::ComboBox);
    assert_eq!(spec.label(), Some("Spectrum"));
    assert_eq!(spec.value(), Some("Output"));
    let on = of(&marks, "b3_on");
    assert_eq!(on.role(), accesskit::Role::Button);
    assert_eq!(on.label(), Some("LMF On"));
    let kind = of(&marks, "b3_type");
    assert_eq!(kind.role(), accesskit::Role::ComboBox);
    assert_eq!(kind.label(), Some("LMF Type"));
    assert_eq!(of(&marks, "mid1_freq").label(), Some("LMF Freq"));

    resize(&mut s, t, 480.0);
    let marks = crate::a11y::marks(&s, &g);
    let tab = of(&marks, "tab-2");
    assert_eq!(tab.role(), accesskit::Role::Tab);
    assert_eq!(tab.label(), Some("LMF"));
    assert_eq!(tab.is_selected(), Some(true));
}
