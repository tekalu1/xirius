//! State fixtures exercise the real builders; JSON supplies expectations, never Items.
use super::tests::{all_labels, studio};
use crate::arrange::probe::track;
use serde_json::json;

#[test]
fn audio_and_midi_take_stacks_keep_every_state_operation() {
    for name in ["Vocal", "Keys"] {
        let (mut s, g) = studio();
        let track = track(&s, name);
        let takes: Vec<_> = (0..3)
            .map(|n| {
                let mut take = json!({"track": track.to_string(), "start": 0, "length": 3840});
                if name == "Vocal" {
                    take["file"] = json!(format!("take-{n}.wav"));
                } else {
                    take["notes"] = json!([{"start": 0, "length": 240, "pitch": 60+n}]);
                }
                take
            })
            .collect();
        let made = s
            .song
            .run("record.take.add", json!({"takes": takes}))
            .unwrap();
        for id in made["clips"].as_array().unwrap() {
            let clip = id.as_str().unwrap().parse().unwrap();
            assert!(s.clip_context(&g, track, clip, 600.0, 300.0));
            let labels = all_labels(&s);
            for label in [
                "このテイクを使用",
                "未使用のテイクをすべて削除",
                "テイクを削除",
            ] {
                assert!(labels.iter().any(|s| s == label));
            }
            // The inventory also lists the reusable take builder in isolation.
            let items = s.take_items(clip);
            super::placement::organize("other-15", items).unwrap();
        }
    }
}
