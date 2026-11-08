//! State fixtures exercise the real builders; JSON supplies expectations, never Items.
use super::tests::{all_labels, studio};
use crate::arrange::probe::track;
use serde_json::json;

#[test]
fn saved_presets_frozen_heads_and_existing_bus_candidates() {
    use crate::tree::RowKind;
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let keys = track(&s, "Keys");
    s.song
        .run(
            "track.preset.save",
            json!({"track":vocal.to_string(),"name":"Catalog vocal"}),
        )
        .unwrap();
    for t in [vocal, keys] {
        assert!(s.row_context(&g, RowKind::Track(t), 100.0, 300.0));
        let items = s.head_items(&g, t);
        super::placement::organize("other-48", items).unwrap();
    }
    s.song
        .run(
            "track.freeze.apply",
            json!({"tracks":[{"id":vocal.to_string(),"file":"frozen.wav","start":0}]}),
        )
        .unwrap();
    let items = s.head_items(&g, vocal);
    super::placement::organize("other-48", items).unwrap();
    assert!(s.row_context(&g, RowKind::Track(vocal), 100.0, 300.0));
    assert!(s.run_command("track.add.bus", json!({"name":"Catalog bus","into":"root"})));
    s.arrange.tracks = vec![RowKind::Track(vocal), RowKind::Track(keys)];
    assert!(s.row_context(&g, RowKind::Track(keys), 100.0, 300.0));
}

#[test]
fn disabled_insert_and_instrument_splitter_states() {
    let (mut s, _g) = studio();
    let t = track(&s, "Keys");
    assert!(s.set_instrument(t, &crate::instrument::Pick::built_in("Sampler")));
    assert!(s.run_command("plugin.add", json!({"track":t.to_string(),"name":"EQ"})));
    let id = s.project().tracks[&t].inserts.last().unwrap().id;
    assert!(s.run_command("plugin.deactivate", json!({"ids":[id.to_string()]})));
    let items = s.plugin_items(t, id);
    super::placement::organize("insert", items).unwrap();
    let items = s.cpu_items(t, id);
    super::placement::organize("other-68", items).unwrap();
    s.song
        .run("rack.split.add", json!({"track":t.to_string()}))
        .unwrap();
    let p = s.project();
    let xirius_core::rack::RackNode::Split(split) =
        p.tracks[&t].rack.as_ref().unwrap().fx.last().unwrap()
    else {
        panic!("split")
    };
    let branch = split.branches[0].id;
    let split = split.id;
    let items = s.split_items(t, split);
    super::placement::organize("other-62", items).unwrap();
    assert!(s.run_command("rack.node.move", json!({"id":id,"to":branch})));
    let items = s.plugin_items(t, id);
    super::placement::organize("insert", items).unwrap();
    s.song
        .run(
            "track.freeze.apply",
            json!({"tracks":[{"id":t.to_string(),"file":"frozen.wav","start":0}]}),
        )
        .unwrap();
    let items = s.cpu_items(t, id);
    super::placement::organize("other-68", items).unwrap();
}

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
