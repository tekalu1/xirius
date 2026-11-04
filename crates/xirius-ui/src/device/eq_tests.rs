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
use crate::studio::{Geometry, Rect, Studio};
use crate::tools::Mods;

/// L-ui-1: a curve drawn at one rate is not drawn again at another from the cache.
#[test]
fn ui_325_the_curve_is_drawn_again_at_another_rate() {
    let values: Vec<f64> = xirius_params::EQ
        .params
        .iter()
        .map(|p| p.default_norm())
        .collect();
    let r = Rect::new(0.0, 0.0, 400.0, 160.0);
    let at = |rate| eq::curve_stamp(&values, r, Some(1), rate);
    assert_ne!(at(44_100.0), at(48_000.0));
    assert_ne!(at(48_000.0), at(96_000.0));
    assert_eq!(at(48_000.0), at(48_000.0));
}

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

/// The song playing to a silent output (M-ui-7): each move of a drag copies the song and
/// syncs the player, as it does on a real one.
fn play_silent(s: &mut Studio) {
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    assert!(s.toggle_play(), "{:?}", s.error);
    let t = Instant::now();
    while !s.transport.is_live() {
        assert!(
            t.elapsed().as_secs() < 60,
            "the player starts: {:?}",
            s.error
        );
        s.tick();
        std::thread::yield_now();
    }
}

/// The plain value of `id` in what the player hears of the EQ.
fn heard(s: &Studio, t: TrackId, id: &str) -> f64 {
    let p = s.auto_heard(&s.project());
    let x = p.tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == "EQ")
        .unwrap();
    let (k, spec) = x.builtin().unwrap().param(id).unwrap();
    spec.to_plain(values(x)[k])
}

#[test]
fn ui_326_a_point_drag_is_one_step_and_each_frame_keeps_up() {
    let (mut s, g, t, id) = eq_window();
    play_silent(&mut s);
    let (fx0, gx0) = (plain(&s, t, "mid1_freq"), plain(&s, t, "mid1_gain"));
    let before = s.song().steps();
    let (x, y) = s
        .instrument_center(&g, id, Part::Point(0, 2))
        .expect("the LMF point");
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    let def = fx(&s, t).builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let plane = s.instruments.body(r, &fx(&s, t), def).graphs[0];
    let mut frames = Vec::new();
    for d in 1..=120 {
        let start = Instant::now();
        // The move syncs the playing player with what it is dragged to (M-ui-7).
        assert!(s.instrument_moved(&g, x + d as f32, y - d as f32 * 0.5, false));
        // What a frame draws: the curve through the values it is dragged to.
        let shown = s.instruments.shown(&fx(&s, t));
        let _ = eq::curves(&shown, plane, Some(2), 48_000.0);
        frames.push(start.elapsed());
    }
    frames.sort();
    // The p99 by nearest rank: of 120 frames, the 119th, so one slow frame may pass. A
    // shared CI runner stalls threads for reasons not the app's, so there only the median
    // has to keep up (L2-ui-2); on a PC the p99 does.
    let ms = |at: usize| frames[at].as_secs_f64() * 1000.0;
    let p99 = (frames.len() * 99).div_ceil(100) - 1;
    assert!(
        ms(frames.len() / 2) <= 16.7,
        "median {:?}",
        frames[frames.len() / 2]
    );
    if std::env::var_os("CI").is_none() {
        assert!(ms(p99) <= 16.7, "p99 {:?}", frames[p99]);
    }
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

/// M-ui-3: a point held when the window loses the keyboard goes in where it is, as one
/// step, and is let go.
#[test]
fn ui_326_a_point_held_goes_in_when_the_window_loses_the_keyboard() {
    let (mut s, g, t, id) = eq_window();
    let (fx0, before) = (plain(&s, t, "mid1_freq"), s.song().steps());
    let (x, y) = s
        .instrument_center(&g, id, Part::Point(0, 2))
        .expect("the LMF point");
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, x + 40.0, y - 20.0, false));
    assert!(s.device_focus_lost());
    assert!(s.instruments.grab.is_none());
    assert_eq!(s.song().steps(), before + 1);
    assert!(plain(&s, t, "mid1_freq") > fx0);
    // The pointer coming back moves nothing.
    assert!(!s.instrument_moved(&g, x + 80.0, y, false));
}

/// M-ui-1: a point pulled aslant is heard moving both ways at once, and the other syncs
/// keep it, until it goes into the song.
#[test]
fn ui_326_a_point_pulled_aslant_is_heard_both_ways() {
    let (mut s, g, t, id) = eq_window();
    let (fx0, gx0) = (plain(&s, t, "mid1_freq"), plain(&s, t, "mid1_gain"));
    let (x, y) = s
        .instrument_center(&g, id, Part::Point(0, 2))
        .expect("the LMF point");
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, x + 40.0, y - 20.0, false));
    let (fh, gh) = (heard(&s, t, "mid1_freq"), heard(&s, t, "mid1_gain"));
    assert!(fh > fx0 && gh > gx0, "heard {fh} Hz {gh} dB");
    assert_eq!(plain(&s, t, "mid1_freq"), fx0, "the song has not taken it");
    // Another edit's sync goes through the same overlay.
    s.after_edit();
    assert_eq!(heard(&s, t, "mid1_freq"), fh);
    assert!(s.instrument_up());
    assert_eq!(heard(&s, t, "mid1_freq"), plain(&s, t, "mid1_freq"));
    assert_eq!(plain(&s, t, "mid1_gain"), gh);
}

/// M-ui-4: Tab goes through every band's point, each chosen as the keys reach it (on to
/// its first control), then out of the window, and Shift+Tab back in the same way; wide
/// (all the bands' columns) and narrow (the chosen band's only).
#[test]
fn ui_326_tab_reaches_every_band() {
    for w in [eq::NATURAL_W, 520.0] {
        tab_round(w);
    }
}

fn tab_round(w: f32) {
    let (mut s, g, t, id) = eq_window();
    resize(&mut s, t, w);
    s.device_blur();
    assert!(s.device_enter(true));
    let start = s.instruments.focus;
    let shift = Mods {
        shift: true,
        ..Mods::NONE
    };
    let mut seen = Vec::new();
    let mut ahead = vec![start];
    for _ in 0..200 {
        assert!(s.field_keys(&g, "tab", None, Mods::NONE));
        let band = s.instruments.band(id);
        if !seen.contains(&band) {
            seen.push(band);
        }
        ahead.push(s.instruments.focus);
        if s.instruments.focus.is_none() {
            break;
        }
    }
    assert_eq!(s.instruments.focus, None, "{w}: out in one turn");
    seen.sort_unstable();
    assert_eq!(seen, (0..eq::NAMES.len()).collect::<Vec<_>>(), "{w}");
    // Back in the same way.
    for want in ahead.iter().rev().skip(1) {
        if s.instruments.focus.is_none() {
            assert!(s.run_binding(&g, "view.focus --way prev"));
        } else {
            assert!(s.field_keys(&g, "tab", None, shift));
        }
        assert_eq!(s.instruments.focus, *want, "{w}");
    }
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
    let r = s.instruments.frame(0, &g);
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

/// M-ui-2, L2-ui-3: a band's point with no knob on show is a Slider reading all it moves,
/// named by its value; no value is two Sliders, and each Slider has its own place. A Slider
/// reads in its own units with its step; a screen reader's increment steps it as the arrow
/// keys do, a value set puts it in at once, and the knobs take the same.
#[test]
fn ui_325_the_points_are_sliders_a_screen_reader_turns() {
    use crate::a11y::Turn;
    let (mut s, g, t, id) = eq_window();
    let once = |marks: &[crate::a11y::Mark]| {
        let mut turns: Vec<_> = marks.iter().filter_map(|m| m.turn).collect();
        let n = turns.len();
        turns.sort_unstable_by_key(|t| t.1);
        turns.dedup();
        assert_eq!(turns.len(), n, "a value is one Slider");
        let mine = format!("device-{id}-");
        assert!(
            (marks.iter())
                .filter(|m| m.key.starts_with(&mine))
                .all(|m| m.children.is_empty())
        );
    };
    once(&crate::a11y::marks(&s, &g));
    // Narrow, the bands not chosen have only their points.
    resize(&mut s, t, 520.0);
    let marks = crate::a11y::marks(&s, &g);
    once(&marks);
    let points: Vec<_> = marks
        .iter()
        .filter(|m| m.key.starts_with(&format!("device-{id}-point-")))
        .collect();
    let mut places: Vec<_> = points
        .iter()
        .map(|m| {
            let b = m.node.bounds().unwrap();
            ((b.x0 * 10.0) as i64, (b.y0 * 10.0) as i64)
        })
        .collect();
    places.sort_unstable();
    places.dedup();
    assert_eq!(
        places.len(),
        eq::NAMES.len() - 1,
        "every other band's point"
    );
    let of = |p: &str| {
        points
            .iter()
            .find(|m| m.key == format!("device-{id}-point-{p}"))
            .unwrap_or_else(|| panic!("no Slider for {p}"))
    };
    let lmf = of("mid2_freq");
    assert_eq!(lmf.node.role(), accesskit::Role::Slider);
    assert!(lmf.node.value().unwrap().contains("Hz"));
    assert_eq!(lmf.turn, Some((id, k_of(&s, t, "mid2_freq"))));
    // Its gain and its Q are Sliders of their own on the same point, each reading itself
    // (low 16).
    let (gain, q) = (of("mid2_gain"), of("mid2_q"));
    assert_eq!(gain.turn, Some((id, k_of(&s, t, "mid2_gain"))));
    assert!(
        gain.node.value().unwrap().contains("dB"),
        "{:?}",
        gain.node.value()
    );
    assert!(!lmf.node.value().unwrap().contains("dB"));
    assert_eq!(q.turn, Some((id, k_of(&s, t, "mid2_q"))));
    assert_eq!(gain.node.bounds(), lmf.node.bounds());
    let labels = [lmf, gain, q].map(|m| m.node.label().unwrap().to_string());
    assert!(
        labels.iter().all(|l| l.starts_with(&labels[0][..3])),
        "{labels:?}"
    );
    assert!(
        labels[0] != labels[1] && labels[1] != labels[2],
        "{labels:?}"
    );
    // In Hz, from the least to the most, with a step.
    let def = fx(&s, t).builtin().unwrap();
    let (_, spec) = def.param("mid2_freq").unwrap();
    assert_eq!(lmf.node.min_numeric_value(), Some(spec.range().0));
    assert_eq!(lmf.node.max_numeric_value(), Some(spec.range().1));
    let hz = lmf.node.numeric_value().unwrap();
    assert!((hz - plain(&s, t, "mid2_freq")).abs() < 1e-6, "{hz}");
    assert!(lmf.node.numeric_value_step().unwrap() > 0.0);
    resize(&mut s, t, eq::NATURAL_W);

    // An increment steps as the arrow does, and goes in once still.
    let (k, f0) = (k_of(&s, t, "mid1_freq"), plain(&s, t, "mid1_freq"));
    let before = s.song().steps();
    assert!(crate::a11y::turn(&mut s, id, k, Turn::Up));
    assert!(s.device_sweep_end());
    assert_eq!(s.song().steps(), before + 1);
    assert!(plain(&s, t, "mid1_freq") > f0);
    assert!(crate::a11y::turn(&mut s, id, k, Turn::Down));
    assert!(s.device_sweep_end());
    assert!((plain(&s, t, "mid1_freq") - f0).abs() < 1e-6 * f0.max(1.0) + 0.5);
    // A value set, in dB: at once, in one step.
    let gain = k_of(&s, t, "mid1_gain");
    assert!(crate::a11y::turn(&mut s, id, gain, Turn::To(6.0)));
    assert_eq!(s.song().steps(), before + 3);
    assert!((plain(&s, t, "mid1_gain") - 6.0).abs() < 0.01);
    let marks = crate::a11y::marks(&s, &g);
    let knob = marks
        .iter()
        .find(|m| m.key == format!("device-{id}-mid1_gain"))
        .unwrap();
    assert_eq!(knob.turn, Some((id, gain)));
    assert!((knob.node.numeric_value().unwrap() - 6.0).abs() < 0.01);
    // A value set while a sweep of it waits: one step, to the value set.
    let before = s.song().steps();
    assert!(crate::a11y::turn(&mut s, id, gain, Turn::Up));
    assert!(crate::a11y::turn(&mut s, id, gain, Turn::To(-3.0)));
    assert!(s.instruments.sweep.is_none());
    assert_eq!(s.song().steps(), before + 1);
    assert!((plain(&s, t, "mid1_gain") + 3.0).abs() < 0.01);
    // And back to where the song has it: no step, and the sweep is not heard.
    assert!(crate::a11y::turn(&mut s, id, gain, Turn::Up));
    assert!(crate::a11y::turn(&mut s, id, gain, Turn::To(-3.0)));
    assert_eq!(s.song().steps(), before + 1);
    assert_eq!(heard(&s, t, "mid1_gain"), plain(&s, t, "mid1_gain"));
    let on = marks
        .iter()
        .find(|m| m.key == format!("device-{id}-b3_on"))
        .unwrap();
    assert_eq!(on.turn, None, "a button is pressed, not turned");
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

/// Low 16: a point's Slider is named by the value it turns and reads that value alone (a
/// point is not named by one value and read by another); no two share a key.
#[test]
fn ui_325_a_points_slider_says_the_value_it_turns() {
    let (mut s, g, t, id) = eq_window();
    resize(&mut s, t, 520.0);
    let x = fx(&s, t);
    let def = x.builtin().unwrap();
    let now = values(&x);
    let marks = crate::a11y::marks(&s, &g);
    let points: Vec<_> = marks
        .iter()
        .filter(|m| m.key.starts_with(&format!("device-{id}-point-")))
        .collect();
    assert!(!points.is_empty());
    let mut keys: Vec<_> = points.iter().map(|m| m.key.clone()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), points.len());
    for m in points {
        let (_, k) = m.turn.unwrap();
        let spec = &def.params[k];
        assert_eq!(m.key, format!("device-{id}-point-{}", spec.id));
        let label = m.node.label().unwrap();
        assert!(
            label.ends_with(super::control::name(def, spec)),
            "{label} turns {}",
            spec.id
        );
        assert_eq!(
            m.node.value(),
            Some(super::control::text(spec, now[k]).as_str())
        );
    }
}
