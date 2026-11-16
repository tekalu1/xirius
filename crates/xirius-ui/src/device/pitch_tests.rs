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
fn pitch_ui_automation_offers_all_seven_targets() {
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
    assert_eq!(pitch.len(), 7);
    let names: Vec<_> = pitch.iter().map(|c| c.label.as_str()).collect();
    for label in [
        "キー",
        "スケール",
        "スピード",
        "ノートトランジション",
        "補正の量",
        "フォルマント",
        "音域",
    ] {
        assert!(
            names.contains(&format!("Pitch Correction · {label}").as_str()),
            "{names:?}"
        );
    }
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
