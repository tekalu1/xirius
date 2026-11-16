//! UI-359: the Pitch Correction window without opening a native window.

use super::control::{self, Look, Step};
use super::layout::{self, Shape};
use xirius_params::{PITCH_CORRECTION, pitch_correction};

#[test]
fn pitch_ui_has_the_approved_controls_and_graph_at_both_widths() {
    let def = &PITCH_CORRECTION;
    for width in [360.0, 640.0] {
        let values: Vec<_> = def.params.iter().map(|p| p.default_norm()).collect();
        let body = layout::body(
            def,
            &Shape {
                w: width,
                values: &values,
                ..Shape::default()
            },
        );
        assert!(body.w <= width);
        assert_eq!(body.graphs.len(), 1);
        assert_eq!(body.keys.len(), 12);
        assert_eq!(body.slots.len(), 7);
        assert_eq!(body.slot(0).unwrap().look, Look::Menu);
        assert_eq!(body.slot(1).unwrap().look, Look::Menu);
        assert!(body.slots.iter().all(|s| s.cell.bottom() <= body.h));
        for key in &body.keys {
            for slot in &body.slots {
                let r = slot.cell;
                assert!(
                    key.r.right() <= r.x
                        || key.r.x >= r.right()
                        || key.r.bottom() <= r.y
                        || key.r.y >= r.bottom(),
                    "keyboard overlaps parameter {} at width {width}",
                    slot.k
                );
            }
        }
    }
}

#[test]
fn pitch_ui_pick_lists_and_bottom_row_match_the_approved_layout() {
    let def = &PITCH_CORRECTION;
    let values: Vec<_> = def.params.iter().map(|p| p.default_norm()).collect();
    for width in [360.0, 640.0] {
        let body = layout::body(
            def,
            &Shape {
                w: width,
                values: &values,
                ..Shape::default()
            },
        );
        for (k, label) in [(0, "Key"), (1, "Scale")] {
            let slot = body.slot(k).unwrap();
            let name = body.names.iter().find(|n| n.text == label).unwrap();
            assert!(slot.cell.h <= 26.0, "{label} must use the PickList box");
            assert!(
                (slot.cell.y - name.r.y).abs() < 12.0,
                "{label} belongs beside its box"
            );
        }
        let formant = body.slot(5).unwrap();
        let range = body.slot(6).unwrap();
        assert!((formant.cell.y - range.cell.y).abs() < 1.0);
        let range_name = body.names.iter().find(|n| n.text == "Range").unwrap();
        assert!((range_name.r.y - range.cell.y).abs() < 12.0);
        if width >= 500.0 {
            assert!(
                formant.cell.x >= body.slot(2).unwrap().cell.x,
                "wide Formant belongs under the three knobs"
            );
        }
    }
}

#[test]
fn pitch_ui_millisecond_value_has_no_unneeded_decimal() {
    let speed = &PITCH_CORRECTION.params[2];
    assert_eq!(control::text(speed, speed.default_norm()), "20 ms");
}

#[test]
fn pitch_ui_legend_pairs_each_sample_with_its_name_at_even_spacings() {
    let graph = crate::studio::Rect::new(0.0, 0.0, 640.0, 220.0);
    let pairs = super::pitch::legend_items(graph);
    let sample_to_name: Vec<_> = pairs
        .iter()
        .map(|(sample, name)| name.x - sample.right())
        .collect();
    let between_pairs: Vec<_> = pairs
        .windows(2)
        .map(|pair| pair[1].0.x - pair[0].1.right())
        .collect();
    assert!(sample_to_name.iter().all(|gap| *gap == sample_to_name[0]));
    assert!(between_pairs.iter().all(|gap| *gap == between_pairs[0]));
    assert!(sample_to_name[0] < between_pairs[0]);
    assert!(pairs[2].1.right() <= graph.right());
}

#[test]
fn pitch_ui_automation_formats_key_amount_and_speed() {
    use xirius_core::auto::AutoTarget;
    use xirius_core::id::PluginId;
    let id = PluginId(123);
    let label = |key: &str, value: f64| {
        crate::arrange::value_text(&AutoTarget::Param(id, key.to_string()), value)
    };
    assert_eq!(label("key", PITCH_CORRECTION.params[0].to_norm(0.0)), "C");
    assert_eq!(label("key", PITCH_CORRECTION.params[0].to_norm(2.0)), "D");
    assert_eq!(
        label("amount", PITCH_CORRECTION.params[4].to_norm(85.0)),
        "85 %"
    );
    assert_eq!(
        label("speed", PITCH_CORRECTION.params[2].default_norm()),
        "20 ms"
    );
}

#[test]
fn pitch_ui_axis_tracks_low_middle_and_high_voice() {
    use std::time::{Duration, Instant};
    for (hz, note) in [(110.0, 45_u8), (266.0, 60), (440.0, 69)] {
        let mut history = super::pitch::History::default();
        let now = Instant::now();
        history.feed((hz, hz, i32::from(note), 0.0), now, Duration::ZERO);
        let lo = history.axis_low();
        let actual = super::pitch::note_of(hz);
        assert_eq!(
            crate::roll::pitch_name(actual),
            xirius_params::pitch::name(note)
        );
        assert!(f32::from(actual) >= lo && f32::from(actual) <= lo + 12.0);
    }
}

#[test]
fn pitch_ui_keyboard_follows_key_and_restores_the_named_scale() {
    let major = 1.0 / 3.0;
    let key = 2_u8; // D
    let lit = |norm: f64, root: u8, pitch: u8| {
        pitch_correction::scale_mask(norm) & (1 << ((pitch + 12 - root) % 12)) != 0
    };
    assert_eq!(
        (0..12).filter(|&n| lit(major, key, n)).collect::<Vec<_>>(),
        vec![1, 2, 4, 6, 7, 9, 11]
    );
    let custom = pitch_correction::toggle_relative(major, 4);
    assert!(!lit(custom, key, 6));
    let restored = pitch_correction::toggle_relative(custom, 4);
    assert_eq!(
        pitch_correction::scale_mask(restored),
        pitch_correction::MAJOR
    );
    assert!(lit(restored, 1, 5)); // C# Major includes E# (the same key as F).
    assert_eq!(super::pitch::toggle_scale(custom, 4), major);
}

#[test]
fn pitch_ui_knobs_use_the_existing_drag_and_arrow_sensitivity() {
    let speed = &PITCH_CORRECTION.params[2];
    let initial = speed.default_norm();
    let dragged = control::dragged(speed, initial, -40.0, false);
    assert!(speed.to_plain(dragged) > 20.0);
    let down = control::step(speed, dragged, -1.0, Step::Small);
    assert!(speed.to_plain(down) < speed.to_plain(dragged));
}

#[test]
fn pitch_ui_history_waits_for_the_audio_latency_and_keeps_silence_empty() {
    use std::time::{Duration, Instant};
    let mut history = super::pitch::History::default();
    let now = Instant::now();
    history.feed((220.0, 220.0, 57, -12.0), now, Duration::from_millis(20));
    assert_eq!(history.audible(now).count(), 0);
    assert_eq!(history.audible(now + Duration::from_millis(20)).count(), 1);
    history.feed((0.0, 0.0, -1, 0.0), now, Duration::ZERO);
    assert_eq!(history.audible(now).count(), 0);
}

#[test]
fn pitch_ui_automation_offers_six_targets_without_range() {
    use crate::studio::Studio;
    use serde_json::json;
    let mut s = Studio::sample();
    let bass = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Bass")
        .unwrap()
        .id;
    assert!(s.run_command("plugin.add", json!({"track": bass.to_string(),
        "name": "Pitch Correction", "kind": "effect", "format": "builtin", "uid": "pitch-correction"})));
    let choices = s.auto_target_choices(bass, None);
    let pitch: Vec<_> = choices
        .iter()
        .filter(|c| c.label.starts_with("Pitch Correction · "))
        .collect();
    assert_eq!(pitch.len(), 6);
    let names: Vec<_> = pitch.iter().map(|c| c.label.as_str()).collect();
    for label in [
        "キー",
        "スケール",
        "スピード",
        "ノートトランジション",
        "補正の量",
        "フォルマント",
    ] {
        assert!(
            names.contains(&format!("Pitch Correction · {label}").as_str()),
            "{names:?}"
        );
    }
    assert!(!names.contains(&"Pitch Correction · 音域"));
    let id = s.project().tracks[&bass].inserts.last().unwrap().id;
    assert!(!s.run_command(
        "auto.lane.add",
        json!({"track": bass.to_string(),
        "target": format!("param:{id}:range")})
    ));
    assert!(s.run_command(
        "plugin.param.set",
        json!({"id": id.to_string(),
        "param": "range", "plain": "High"})
    ));
    let project = s.project();
    let p = project.tracks[&bass].inserts.last().unwrap();
    assert_eq!(
        p.builtin().unwrap().params[6].to_plain(p.params["range"]),
        2.0
    );
}

#[test]
fn pitch_ui_builtin_insert_opens_its_device_window() {
    use crate::studio::Studio;
    use serde_json::json;
    let mut s = Studio::sample();
    let bass = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Bass")
        .unwrap()
        .id;
    assert!(s.run_command("plugin.add", json!({"track": bass.to_string(),
        "name": "Pitch Correction", "kind": "effect", "format": "builtin", "uid": "pitch-correction"})));
    let id = s.project().tracks[&bass].inserts.last().unwrap().id;
    s.show_instrument(bass, id);
    assert!(s.instruments.is_open(id));
    assert!(crate::instrument::window::is_built_in(
        s.project().tracks[&bass].inserts.last().unwrap()
    ));
}

#[test]
fn pitch_ui_sender_gets_a_mixer_row_and_receiver_gets_a_mark() {
    use crate::studio::Studio;
    use serde_json::json;
    let mut s = Studio::sample();
    let bass = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Bass")
        .unwrap()
        .id;
    let keys = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    assert!(s.run_command("plugin.add", json!({"track": bass.to_string(),
        "name": "Pitch Correction", "kind": "effect", "format": "builtin", "uid": "pitch-correction"})));
    let id = s.project().tracks[&bass].inserts.last().unwrap().id;
    assert!(s.run_command(
        "plugin.note_source.set",
        json!({"id":id.to_string(), "from":keys.to_string()})
    ));
    assert_eq!(
        crate::mixer::keys::note_sends(&s.project(), keys),
        vec![(bass, id, true)]
    );
    assert!(
        s.project().tracks[&bass]
            .inserts
            .last()
            .unwrap()
            .note_source
            .is_some()
    );
}

#[test]
fn pitch_ui_power_and_default_preset_follow_the_builtin_window() {
    use crate::instrument::window::Part;
    use crate::studio::Studio;
    use serde_json::json;
    let mut s = Studio::sample();
    let track = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Bass")
        .unwrap()
        .id;
    assert!(s.run_command(
        "plugin.add",
        json!({"track":track.to_string(),
        "name":"Pitch Correction","kind":"effect","format":"builtin","uid":"pitch-correction"})
    ));
    let id = s.project().tracks[&track].inserts.last().unwrap().id;
    assert!(s.run_command(
        "plugin.param.set",
        json!({"id":id.to_string(),"param":"speed","plain":"85"})
    ));
    assert!(s.load_preset(id, "Default"));
    let project = s.project();
    let plugin = project.tracks[&track].inserts.last().unwrap();
    for p in PITCH_CORRECTION.params {
        assert!(
            (plugin.params.get(p.id).copied().unwrap_or(p.default_norm()) - p.default_norm()).abs()
                < 1e-12
        );
    }
    s.show_instrument(track, id);
    s.instruments.detached_target = Some(id);
    let g = s.geometry(1600.0, 1000.0);
    let (x, y) = s.instrument_center(&g, id, Part::Power).unwrap();
    assert!(s.instrument_down(&g, x, y));
    assert!(s.project().tracks[&track].inserts.last().unwrap().bypass);
    assert!(s.undo(&g));
    assert!(!s.project().tracks[&track].inserts.last().unwrap().bypass);
}
