//! UI-326: the EQ's window, without a window.

use std::time::Instant;

use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;
use xirius_engine::stock::Tap;

use super::eq;
use super::layout;
use crate::held_input::{Held, HeldInput};
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
    s.instruments.detached_target = Some(id);
    assert!(s.instruments.is_open(id));
    (s, g, t, id)
}

#[test]
fn context_catalog_covers_each_eq_band_type_and_switch_state() {
    let (mut s, g, _, id) = eq_window();
    for (band, spec) in xirius_params::EQ_BANDS.iter().enumerate() {
        for kind in 0..spec.types.len() {
            for on in [0.0, 1.0] {
                s.song
                    .run(
                        "plugin.param.set",
                        json!({"id": id.to_string(),
                            "param": spec.kind, "value": eq::choice(spec.types.len(), kind),
                            "also": [{"param": spec.on, "value": on}]
                        }),
                    )
                    .unwrap();
                s.point_menu(&g, 0, (0, band), 600.0, 300.0);
                assert!(s.context_open());
                let rows: Vec<String> = s
                    .context_rows()
                    .into_iter()
                    .filter(|r| !r.gap)
                    .map(|r| r.label)
                    .collect();
                // Under 13 rows: no category group, the reset last.
                assert!(
                    rows.len() <= 12 && !rows.contains(&"機器".to_string()),
                    "{rows:?}"
                );
                assert_eq!(rows.last().map(String::as_str), Some("既定に戻す"));
                s.popup = None;
            }
        }
    }
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
        if s.instruments.focus == start {
            break;
        }
    }
    assert_eq!(s.instruments.focus, start, "{w}: wrapped in one turn");
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
    once(&crate::a11y::device_marks(&s, &g));
    // Narrow, the bands not chosen have only their points.
    resize(&mut s, t, 520.0);
    let marks = crate::a11y::device_marks(&s, &g);
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
    let marks = crate::a11y::device_marks(&s, &g);
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
    let marks = crate::a11y::device_marks(&s, &g);
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
    let marks = crate::a11y::device_marks(&s, &g);
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
    let marks = crate::a11y::device_marks(&s, &g);
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

/// Presses the LMF point at its middle (`clicks`), moves it by `d` and lets go.
fn tap_point(s: &mut Studio, g: &Geometry, id: PluginId, clicks: u32, d: (f32, f32)) {
    tap_at(s, g, id, 2, clicks, d);
}

/// Presses the `i`th band's point at its middle (`clicks`), moves it by `d` and lets go.
fn tap_at(s: &mut Studio, g: &Geometry, id: PluginId, i: usize, clicks: u32, d: (f32, f32)) {
    let (x, y) = s
        .instrument_center(g, id, Part::Point(0, i))
        .expect("the point");
    assert!(s.instrument_press(g, x, y, clicks, Mods::NONE));
    if d != (0.0, 0.0) {
        s.instrument_moved(g, x + d.0, y + d.1, false);
    }
    s.instrument_up();
}

/// Space goes past the window to the transport: the song starts (to a silent output) and
/// stops again.
fn space_plays(s: &mut Studio, g: &Geometry) {
    assert!(
        !s.field_keys(g, "space", None, Mods::NONE),
        "the window lets Space go"
    );
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    assert!(s.edit_key(g, "space", Mods::NONE));
    assert!(s.transport.playing, "the transport takes Space");
    assert!(s.edit_key(g, "space", Mods::NONE));
    assert!(!s.transport.playing);
}

/// Presses the knob of the parameter `name` and lets go.
fn press_knob(s: &mut Studio, g: &Geometry, t: TrackId, id: PluginId, name: &str) {
    let k = k_of(s, t, name);
    press(s, g, id, Part::Knob(k));
}

/// Where the history stands.
fn cursor(s: &Studio) -> u64 {
    s.song.session().cursor()
}

/// The last press on a point was a while ago: the next one is not a double press.
fn press_ago(s: &mut Studio) {
    if let Some(p) = &mut s.instruments.pressed {
        p.when -= std::time::Duration::from_millis(500);
    }
}

/// The last press on a point was just now, however slow the test runs: the next one is a
/// double press.
fn press_now(s: &mut Studio) {
    if let Some(p) = &mut s.instruments.pressed {
        p.when = Instant::now();
    }
}

/// UI-326 (2026-11-05): the first press on a point chooses it; pressed again and let go
/// unmoved, its band switches, one step to undo; moved within 4 px it still switches and
/// moves nothing, past 4 px it moves and switches nothing.
#[test]
fn ui_326_a_press_on_the_chosen_point_switches_its_band() {
    let (mut s, g, t, id) = eq_window();
    let f0 = plain(&s, t, "mid1_freq");
    let before = s.song().steps();
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    assert_eq!(s.instruments.chosen.get(&id), Some(&(0, 2)));
    assert_eq!(plain(&s, t, "b3_on"), 1.0, "the first press only chooses");
    assert_eq!(s.song().steps(), before);
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    assert_eq!(plain(&s, t, "b3_on"), 0.0);
    assert_eq!(s.song().steps(), before + 1, "one step");
    assert!(s.undo(&g));
    assert_eq!(plain(&s, t, "b3_on"), 1.0, "undone in one step");
    // A tremble within 4 px is still a press.
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 1, (3.0, -2.0));
    assert_eq!(plain(&s, t, "b3_on"), 0.0);
    assert_eq!(plain(&s, t, "mid1_freq"), f0, "nothing moved");
    // Past 4 px it is a drag: the frequency moves, the band stays as it is.
    press_ago(&mut s);
    let steps = s.song().steps();
    tap_point(&mut s, &g, id, 1, (30.0, 0.0));
    assert_eq!(plain(&s, t, "b3_on"), 0.0);
    assert!(plain(&s, t, "mid1_freq") > f0);
    assert_eq!(s.song().steps(), steps + 1);
    // A point not chosen moves at once, as before, and switches nothing.
    let (hx, hy) = s.instrument_center(&g, id, Part::Point(0, 4)).unwrap();
    let hf = plain(&s, t, "high_freq");
    assert!(s.instrument_press(&g, hx, hy, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, hx - 2.0, hy, false));
    s.instrument_up();
    assert_eq!(plain(&s, t, "b5_on"), 1.0);
    assert!(plain(&s, t, "high_freq") < hf);
}

/// UI-326: a second press within 400 ms types the frequency, putting back the band the first
/// press switched; a point not chosen is not switched by a double press; a slower second
/// press switches the band again.
#[test]
fn ui_326_a_double_press_types_the_frequency_and_puts_the_band_back() {
    let (mut s, g, t, id) = eq_window();
    let freq = k_of(&s, t, "mid1_freq");
    // Not chosen: the double press chooses and types.
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    tap_point(&mut s, &g, id, 2, (0.0, 0.0));
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
    assert_eq!(s.instruments.entry.as_ref().map(|e| e.k), Some(freq));
    s.device_blur();
    // Away from the window and back, the first press takes the keys again.
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
    // Chosen: the first press switches, the second takes that step back and types.
    let c0 = cursor(&s);
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    assert_eq!(plain(&s, t, "b3_on"), 0.0);
    assert_eq!(cursor(&s), c0 + 1);
    tap_point(&mut s, &g, id, 2, (0.0, 0.0));
    assert_eq!(plain(&s, t, "b3_on"), 1.0, "put back");
    assert_eq!(cursor(&s), c0, "nothing of the switch is left to undo");
    assert_eq!(s.instruments.entry.as_ref().map(|e| e.k), Some(freq));
    // The typed frequency is the one step: one undo takes it back, the band stays on.
    let f0 = plain(&s, t, "mid1_freq");
    s.ime_replace(None, "1000", false);
    assert!(s.field_keys(&g, "enter", None, Mods::NONE));
    assert!((plain(&s, t, "mid1_freq") - 1000.0).abs() < 1.0);
    assert_eq!(cursor(&s), c0 + 1, "one step");
    assert!(!s.song.session().can_redo());
    assert!(s.undo(&g));
    assert_eq!(plain(&s, t, "mid1_freq"), f0);
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
    assert_eq!(cursor(&s), c0);
    // A second press after 400 ms is a press of its own, though the system counts two.
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    assert_eq!(plain(&s, t, "b3_on"), 0.0);
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 2, (0.0, 0.0));
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
    assert!(s.instruments.entry.is_none());
}

/// UI-326: Space on the point pressed on the graph switches its band before the transport
/// takes it, and Enter types its frequency.
#[test]
fn ui_326_space_on_a_point_switches_its_band_and_enter_types() {
    let (mut s, g, t, id) = eq_window();
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    assert_eq!(s.instruments.focus, Some((id, k_of(&s, t, "mid1_freq"))));
    let before = s.song().steps();
    assert!(s.field_keys(&g, "space", None, Mods::NONE));
    assert_eq!(plain(&s, t, "b3_on"), 0.0);
    assert_eq!(s.song().steps(), before + 1);
    assert!(!s.transport.playing, "the transport did not take it");
    assert!(s.field_keys(&g, "space", None, Mods::NONE));
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
    assert!(s.field_keys(&g, "enter", None, Mods::NONE));
    assert_eq!(
        s.instruments.entry.as_ref().map(|e| e.k),
        Some(k_of(&s, t, "mid1_freq"))
    );
}

/// UI-326 (review H1): after a press on a column's knob, the analyser's button or Tab, Space
/// plays: only a point pressed on the graph has the keys for its band.
#[test]
fn ui_326_space_after_a_knob_or_another_part_is_the_transports() {
    let (mut s, g, t, id) = eq_window();
    let c0 = cursor(&s);
    // The LMF column's frequency knob: the band and the focus as on the LMF point.
    press_knob(&mut s, &g, t, id, "mid1_freq");
    assert_eq!(s.instruments.chosen.get(&id), Some(&(0, 2)));
    assert_eq!(s.instruments.focus, Some((id, k_of(&s, t, "mid1_freq"))));
    space_plays(&mut s, &g);
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
    // The HIGH column's frequency knob chooses HIGH; Space still plays.
    press_knob(&mut s, &g, t, id, "high_freq");
    assert_eq!(s.instruments.chosen.get(&id), Some(&(0, 4)));
    space_plays(&mut s, &g);
    assert_eq!(plain(&s, t, "b5_on"), 1.0);
    // Its gain and Q too.
    for k in ["high_gain", "b5_q"] {
        press_knob(&mut s, &g, t, id, k);
        space_plays(&mut s, &g);
    }
    // The point pressed on the graph has the keys; a knob pressed after takes them.
    tap_at(&mut s, &g, id, 4, 1, (0.0, 0.0));
    assert!(s.field_keys(&g, "space", None, Mods::NONE));
    assert_eq!(plain(&s, t, "b5_on"), 0.0);
    assert!(s.field_keys(&g, "space", None, Mods::NONE));
    assert_eq!(plain(&s, t, "b5_on"), 1.0);
    press_knob(&mut s, &g, t, id, "high_gain");
    space_plays(&mut s, &g);
    // So does the analyser's button.
    tap_at(&mut s, &g, id, 4, 1, (0.0, 0.0));
    let (sx, sy) = s.instrument_center(&g, id, Part::Spectrum).unwrap();
    assert!(s.instrument_press(&g, sx, sy, 1, Mods::NONE));
    s.instrument_up();
    s.popup = None;
    space_plays(&mut s, &g);
    // Tab from the point moves the keys on to a column: Space plays.
    tap_at(&mut s, &g, id, 4, 1, (0.0, 0.0));
    assert!(s.field_keys(&g, "tab", None, Mods::NONE));
    space_plays(&mut s, &g);
    assert_eq!(plain(&s, t, "b5_on"), 1.0);
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
    assert_eq!(cursor(&s), c0 + 2, "only the two switches on the point");
}

/// UI-326 (review M3): a point chosen in its column, by its menu or by Tab is only chosen;
/// its first press on the graph takes the keys and switches nothing, the next one switches.
#[test]
fn ui_326_a_point_chosen_another_way_takes_the_keys_on_its_first_press() {
    let (mut s, g, t, id) = eq_window();
    let c0 = cursor(&s);
    // In its column.
    press_knob(&mut s, &g, t, id, "high_freq");
    assert_eq!(s.instruments.chosen.get(&id), Some(&(0, 4)));
    tap_at(&mut s, &g, id, 4, 1, (0.0, 0.0));
    assert_eq!(plain(&s, t, "b5_on"), 1.0, "the first press takes the keys");
    assert_eq!(cursor(&s), c0);
    press_ago(&mut s);
    tap_at(&mut s, &g, id, 4, 1, (0.0, 0.0));
    assert_eq!(plain(&s, t, "b5_on"), 0.0, "the next one switches");
    assert!(s.undo(&g));
    // By its menu.
    let (x, y) = s.instrument_center(&g, id, Part::Point(0, 3)).unwrap();
    assert!(s.instrument_right(&g, x, y));
    assert_eq!(s.instruments.chosen.get(&id), Some(&(0, 3)));
    s.popup = None;
    press_ago(&mut s);
    tap_at(&mut s, &g, id, 3, 1, (0.0, 0.0));
    assert_eq!(plain(&s, t, "b4_on"), 1.0);
    press_ago(&mut s);
    tap_at(&mut s, &g, id, 3, 1, (0.0, 0.0));
    assert_eq!(plain(&s, t, "b4_on"), 0.0);
    assert!(s.undo(&g));
    // By Tab: on to the next column, whose point is chosen; its first press takes the keys.
    let was = *s.instruments.chosen.get(&id).unwrap();
    let mut n = 0;
    while s.instruments.chosen.get(&id) == Some(&was) {
        assert!(s.field_keys(&g, "tab", None, Mods::NONE));
        n += 1;
        assert!(n < 40, "Tab reaches the next band");
    }
    let (_, i) = *s.instruments.chosen.get(&id).unwrap();
    let on = xirius_params::EQ_BANDS[i].on;
    press_ago(&mut s);
    tap_at(&mut s, &g, id, i, 1, (0.0, 0.0));
    assert_eq!(plain(&s, t, on), 1.0);
    press_ago(&mut s);
    tap_at(&mut s, &g, id, i, 1, (0.0, 0.0));
    assert_eq!(plain(&s, t, on), 0.0);
}

/// UI-326 (review M6): a finger or a pen trembles more as it lifts: within 10 px it is still a
/// press that switches the band; a mouse past 4 px drags.
#[test]
fn ui_326_a_finger_may_tremble_further_and_still_switch() {
    let (mut s, g, t, id) = eq_window();
    let f0 = plain(&s, t, "mid1_freq");
    s.coarse_press = true;
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 1, (6.0, -5.0));
    assert_eq!(plain(&s, t, "b3_on"), 0.0, "an 8 px tremble is a press");
    assert_eq!(plain(&s, t, "mid1_freq"), f0, "nothing moved");
    // Past 10 px it drags.
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 1, (12.0, 0.0));
    assert_eq!(plain(&s, t, "b3_on"), 0.0);
    assert!(plain(&s, t, "mid1_freq") > f0);
    // The same tremble from a mouse drags.
    s.coarse_press = false;
    let f1 = plain(&s, t, "mid1_freq");
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 1, (6.0, -5.0));
    assert_eq!(plain(&s, t, "b3_on"), 0.0);
    assert!(plain(&s, t, "mid1_freq") > f1);
}

/// UI-326 (review L9): pulled past 4 px and brought back to where it was pressed, the press
/// is a drag still: the band does not switch.
#[test]
fn ui_326_a_point_pulled_and_brought_back_switches_nothing() {
    let (mut s, g, t, id) = eq_window();
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    press_ago(&mut s);
    let (x, y) = s.instrument_center(&g, id, Part::Point(0, 2)).unwrap();
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    s.instrument_moved(&g, x + 20.0, y, false);
    s.instrument_moved(&g, x, y, false);
    s.instrument_up();
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
}

/// UI-326: the point's menu has its types, «バンドをオフ» or «バンドをオン» and «既定に戻す»;
/// the switch is one step to undo, and the point and a screen reader say it is off.
#[test]
fn ui_326_the_points_menu_switches_its_band_and_the_reader_hears_it() {
    let (mut s, g, t, id) = eq_window();
    let (x, y) = s.instrument_center(&g, id, Part::Point(0, 3)).unwrap();
    assert!(s.instrument_right(&g, x, y));
    let labels: Vec<_> = crate::context::tests::all_rows(&s)
        .iter()
        .map(|r| r.label.clone())
        .filter(|l| !l.is_empty() && l != "機器")
        .collect();
    let mut want: Vec<String> = xirius_params::EQ_BANDS[3]
        .types
        .iter()
        .map(|t| t.label().to_string())
        .collect();
    want.extend(["バンドをオフ".to_string(), "既定に戻す".to_string()]);
    assert_eq!(labels, want);
    let before = s.song().steps();
    assert!(s.context_choose(&g, "バンドをオフ"));
    assert_eq!(plain(&s, t, "b4_on"), 0.0);
    assert_eq!(s.song().steps(), before + 1);
    let x = fx(&s, t);
    let def = x.builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let plane = s.instruments.body(r, &x, def).graphs[0];
    let pts = super::graph::points(def, 0, plane, &values(&x));
    assert!(!pts[3].on);
    assert_eq!(pts[3].name, "HMF オフ Peaking");
    // The reader: the HMF point (narrow, its knobs not on show) is named off.
    resize(&mut s, t, 520.0);
    s.instruments.chosen.insert(id, (0, 2));
    let marks = crate::a11y::device_marks(&s, &g);
    let hmf = marks
        .iter()
        .find(|m| m.key == format!("device-{id}-point-mid2_freq"))
        .expect("the HMF point's Slider");
    assert!(
        hmf.node.label().unwrap().starts_with("HMF オフ"),
        "{:?}",
        hmf.node.label()
    );
    // The menu now turns it back on.
    let (x, y) = s.instrument_center(&g, id, Part::Point(0, 3)).unwrap();
    assert!(s.instrument_right(&g, x, y));
    assert!(
        crate::context::tests::all_rows(&s)
            .iter()
            .any(|r| r.label == "バンドをオン")
    );
    s.popup = None;
    assert!(s.undo(&g));
    assert_eq!(plain(&s, t, "b4_on"), 1.0);
}

/// UI-326: the band's on keeps its stepped lane: while its lane plays it (Read), a press on
/// the chosen point switches nothing and Space goes on to the transport (review M1).
#[test]
fn ui_326_a_band_its_lane_plays_is_not_switched_by_hand() {
    use xirius_core::auto::AutoTarget;
    let (mut s, g, t, id) = eq_window();
    let target = AutoTarget::Param(id, "b3_on".into());
    for (at, v) in [(0, 1.0), (3840, 0.0)] {
        assert!(s.run_command(
            "auto.point.add",
            json!({ "track": t.to_string(), "target": target.to_string(), "at": at, "value": v }),
        ));
    }
    assert!(s.param_locked(t, id, "b3_on"), "its lane in Read holds it");
    let before = s.song().steps();
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    assert_eq!(s.song().steps(), before);
    space_plays(&mut s, &g);
    assert_eq!(s.song().steps(), before);
}

/// A plane for the graph's points: their values, not where they are, give the ring.
const PLANE: Rect = Rect::new(0.0, 0.0, 400.0, 160.0);

/// Whether the EQ's chosen point wears the ring of the keys.
fn ringed(s: &Studio, t: TrackId, id: PluginId) -> bool {
    let x = fx(s, t);
    s.instruments
        .point_ringed(id, x.builtin().unwrap(), 0, PLANE, &values(&x))
}

/// UI-326 (review 2 M1): the EQ's point wears the ring only while it has the keys, pressed
/// on the graph, as then its first press switches the band; chosen in its column, by Tab or
/// by its menu it is drawn chosen, without the ring. A point that switches nothing (the
/// Compressor's) keeps the ring the keys of its column give it.
#[test]
fn ui_326_the_ring_is_on_a_point_only_while_it_has_the_keys() {
    let (mut s, g, t, id) = eq_window();
    press_knob(&mut s, &g, t, id, "mid1_freq");
    assert_eq!(s.instruments.chosen.get(&id), Some(&(0, 2)));
    assert_eq!(s.instruments.focus, Some((id, k_of(&s, t, "mid1_freq"))));
    assert!(!ringed(&s, t, id), "chosen in its column: no ring");
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    assert!(ringed(&s, t, id), "pressed on the graph: the ring");
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    assert_eq!(
        plain(&s, t, "b3_on"),
        0.0,
        "the point with the ring switches"
    );
    assert!(ringed(&s, t, id));
    assert!(s.field_keys(&g, "tab", None, Mods::NONE));
    assert!(!ringed(&s, t, id), "Tab takes the ring off");
    let (x, y) = s.instrument_center(&g, id, Part::Point(0, 3)).unwrap();
    assert!(s.instrument_right(&g, x, y));
    s.popup = None;
    assert_eq!(s.instruments.chosen.get(&id), Some(&(0, 3)));
    assert!(!ringed(&s, t, id), "chosen by its menu: no ring");
    tap_at(&mut s, &g, id, 3, 1, (0.0, 0.0));
    assert!(ringed(&s, t, id));
    press_knob(&mut s, &g, t, id, "mid2_gain");
    assert!(!ringed(&s, t, id), "a knob takes the ring off");
    // A Compressor's point, its value's keys in its column: the ring as before.
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "Compressor" }),
    ));
    let c = s.project().tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == "Compressor")
        .cloned()
        .unwrap();
    let def = c.builtin().unwrap();
    let vals = values(&c);
    let k = super::graph::points(def, 0, PLANE, &vals)
        .first()
        .and_then(|p| p.params().next())
        .expect("a point with a value");
    s.instruments.chosen.insert(c.id, (0, 0));
    s.instruments.focus = Some((c.id, k));
    s.instruments.on_point = None;
    assert!(s.instruments.point_ringed(c.id, def, 0, PLANE, &vals));
}

/// A key let go of at once.
fn key_down(key: &str) -> gpui::PlatformInput {
    gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse(key).expect("a key"),
        is_held: false,
        prefer_character_input: false,
    })
}

/// Types `text` and presses Enter as the window's screen gets them: a key's event, then its
/// characters as text; while a short save runs both wait in `held`.
fn type_held(s: &Studio, held: &mut HeldInput, text: &str) {
    for ch in text.chars() {
        let ch = ch.to_string();
        assert!(held.hold(s.input_hold(), Held::Event(key_down(&ch))));
        assert!(held.hold(s.input_hold(), Held::Text(None, ch)));
    }
    assert!(held.hold(s.input_hold(), Held::Event(key_down("enter"))));
}

/// What waited comes in, in order, as the screen lets it through again: a key to the keys of
/// the window, text to the field.
fn let_through(s: &mut Studio, g: &Geometry, held: &mut HeldInput) {
    for h in held.release(s.input_hold()) {
        match h {
            Held::Event(gpui::PlatformInput::KeyDown(k)) => {
                if !s.field_keys(g, &k.keystroke.key, None, Mods::NONE) {
                    s.edit_key(g, &k.keystroke.key, Mods::NONE);
                }
            }
            Held::Text(range, text) => s.ime_replace(range, &text, false),
            Held::Event(_) => {}
        }
    }
}

/// The double press on the chosen LMF point in the window, its undo on the worker; the
/// history's place before the switch.
fn double_press_in_window(s: &mut Studio, g: &Geometry, t: TrackId, id: PluginId) -> u64 {
    s.bind_window_io();
    tap_point(s, g, id, 1, (0.0, 0.0));
    let c0 = cursor(s);
    press_ago(s);
    tap_point(s, g, id, 1, (0.0, 0.0));
    assert_eq!(plain(s, t, "b3_on"), 0.0);
    press_now(s);
    tap_point(s, g, id, 2, (0.0, 0.0));
    assert!(s.saving.is_some(), "the undo runs on the worker");
    assert!(s.input_blocked());
    assert_eq!(
        s.instruments.entry.as_ref().map(|e| e.k),
        Some(k_of(s, t, "mid1_freq"))
    );
    c0
}

/// UI-326 (review 3 M1): in the window the double press's undo goes to the worker, as Ctrl+Z
/// does. A value typed before the save needs its veil waits with its keys, the characters
/// too, and comes in order once it is done: the frequency goes in, the one step left.
#[test]
fn ui_326_a_value_typed_while_the_undo_runs_on_the_worker_goes_in() {
    let (mut s, g, t, id) = eq_window();
    let c0 = double_press_in_window(&mut s, &g, t, id);
    let typed = s.instruments.entry.as_ref().map(|e| e.field.text());
    let mut held = HeldInput::default();
    type_held(&s, &mut held, "1000");
    assert_eq!(
        s.instruments.entry.as_ref().map(|e| e.field.text()),
        typed,
        "nothing reaches the field meanwhile"
    );
    assert!(held.release(s.input_hold()).is_empty(), "it waits");
    crate::review_io_tests::finish(&mut s);
    assert_eq!(plain(&s, t, "b3_on"), 1.0, "put back");
    assert_eq!(cursor(&s), c0);
    let_through(&mut s, &g, &mut held);
    assert!(s.instruments.entry.is_none());
    assert!((plain(&s, t, "mid1_freq") - 1000.0).abs() < 1.0);
    assert_eq!(cursor(&s), c0 + 1, "one step");
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
}

/// UI-326 (review 3 M1): once the save shows its veil, what is typed goes, keys and text
/// alike, as every input then does; the field stays open with what it had.
#[test]
fn ui_326_what_is_typed_under_the_veil_goes() {
    let (mut s, g, t, id) = eq_window();
    let c0 = double_press_in_window(&mut s, &g, t, id);
    let typed = s.instruments.entry.as_ref().map(|e| e.field.text());
    let f0 = plain(&s, t, "mid1_freq");
    if let Some(save) = &mut s.saving {
        save.since -= crate::wait::OVERLAY_AFTER;
    }
    let mut held = HeldInput::default();
    type_held(&s, &mut held, "1000");
    crate::review_io_tests::finish(&mut s);
    let_through(&mut s, &g, &mut held);
    assert_eq!(s.instruments.entry.as_ref().map(|e| e.field.text()), typed);
    assert_eq!(plain(&s, t, "mid1_freq"), f0);
    assert_eq!(cursor(&s), c0);
}

/// UI-326 (review 2 L1): a switch the song did not take (a take being finished holds it) is
/// no step; a double press after sets the band back, and takes back no step before it.
#[test]
fn ui_326_a_refused_switch_takes_back_no_step_on_a_double_press() {
    let (mut s, g, t, id) = eq_window();
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    let gain = k_of(&s, t, "mid1_gain");
    assert!(s.device_set(id, gain, 0.8));
    let g1 = plain(&s, t, "mid1_gain");
    let c1 = cursor(&s);
    s.transport.finishing_take = true;
    press_ago(&mut s);
    tap_point(&mut s, &g, id, 1, (0.0, 0.0));
    s.transport.finishing_take = false;
    assert_eq!(plain(&s, t, "b3_on"), 1.0, "the song did not take it");
    assert_eq!(cursor(&s), c1);
    press_now(&mut s);
    tap_point(&mut s, &g, id, 2, (0.0, 0.0));
    assert_eq!(plain(&s, t, "mid1_gain"), g1, "the step before stays");
    assert_eq!(plain(&s, t, "b3_on"), 1.0);
    assert_eq!(cursor(&s), c1);
    assert!(s.song.session().can_undo());
    assert!(s.instruments.entry.is_some());
}
