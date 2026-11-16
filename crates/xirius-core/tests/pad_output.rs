//! PLG-069: pad output is one song edit, including a newly created bus.

use serde_json::json;
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_core::session::{RunOptions, Session};
use xirius_core::{chain_preset::Chain, track_preset::TrackPreset};

fn song_with_bus(existing_bus: bool) -> Session {
    let mut p = Project::new("Pad outputs");
    p.next_id = 6;
    let mut kit = Track::new(TrackId(1), TrackKind::Instrument, "Drums");
    kit.inserts.push(Plugin {
        pad_outputs: Default::default(),
        id: PluginId(2),
        name: "Drum Kit".into(),
        kind: PluginKind::Instrument,
        format: "builtin".into(),
        uid: "drum-kit".into(),
        bypass: false,
        deactivated: false,
        params: Default::default(),
        state: None,
        samples: vec![],
        isolation: PluginIsolation::default(),
        sidechain: None,
        note_source: None,
        preset: None,
    });
    p.tracks.insert(TrackId(1), kit.into());
    p.root.push(Node::Track(TrackId(1)));
    if existing_bus {
        let bus = TrackId(3);
        p.next_id = 6;
        p.tracks
            .insert(bus, Track::new(bus, TrackKind::Bus, "Kick").into());
        p.root.push(Node::Track(bus));
    }
    Session::from_project(p)
}

fn song() -> Session {
    song_with_bus(false)
}

fn output(s: &Session) -> Option<TrackId> {
    s.project().track(TrackId(1)).unwrap().inserts[0]
        .pad_outputs
        .get(&0)
        .copied()
}

#[test]
fn emptying_a_pad_clears_its_output_and_undo_restores_it() {
    let mut s = song_with_bus(true);
    s.run_value(
        "plugin.samples.add",
        json!({"id":"plg_2","file":"old.wav","pad":"A1"}),
        &RunOptions::human(),
    )
    .unwrap();
    s.run_value(
        "plugin.pad_output.set",
        json!({"id":"plg_2","pads":[{"pad":"A1"}],"to":"trk_3"}),
        &RunOptions::human(),
    )
    .unwrap();
    assert_eq!(output(&s), Some(TrackId(3)));
    s.run_value(
        "plugin.samples.remove",
        json!({"id":"plg_2","pad":"A1"}),
        &RunOptions::human(),
    )
    .unwrap();
    assert_eq!(output(&s), None);
    s.undo(1).unwrap();
    assert_eq!(output(&s), Some(TrackId(3)));
    s.redo(1).unwrap();
    assert_eq!(output(&s), None);
    s.run_value(
        "plugin.samples.add",
        json!({"id":"plg_2","file":"new.wav","pad":"A1"}),
        &RunOptions::human(),
    )
    .unwrap();
    assert_eq!(output(&s), None);
}

#[test]
fn replacing_samples_clears_the_old_pad_output() {
    let mut s = song_with_bus(true);
    for (pad, file) in [("A1", "one.wav"), ("A2", "two.wav")] {
        s.run_value(
            "plugin.samples.add",
            json!({"id":"plg_2","file":file,"pad":pad}),
            &RunOptions::human(),
        )
        .unwrap();
    }
    s.run_value(
        "plugin.pad_output.set",
        json!({"id":"plg_2","pads":[{"pad":"A1"},{"pad":"A2"}],"to":"trk_3"}),
        &RunOptions::human(),
    )
    .unwrap();
    s.run_value(
        "plugin.samples.add",
        json!({"id":"plg_2","file":"replacement.wav","pad":"A1","replace":true}),
        &RunOptions::human(),
    )
    .unwrap();
    assert!(
        s.project().track(TrackId(1)).unwrap().inserts[0]
            .pad_outputs
            .is_empty()
    );
    s.undo(1).unwrap();
    assert_eq!(
        s.project().track(TrackId(1)).unwrap().inserts[0]
            .pad_outputs
            .len(),
        2
    );
    s.redo(1).unwrap();
    assert!(
        s.project().track(TrackId(1)).unwrap().inserts[0]
            .pad_outputs
            .is_empty()
    );
}

#[test]
fn setting_main_and_a_new_bus_is_one_undo_step() {
    let mut s = song();
    let r = s.run_value(
        "plugin.pad_output.set",
        json!({"id":"plg_2", "pads":[{"pad":"A1"}, {"pad":"A2"}], "to":"new", "active":"A2"}),
        &RunOptions::human(),
    );
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(s.project().root.len(), 2);
    let bus = s.project().root[1];
    let Node::Track(bus_id) = bus else {
        panic!("expected bus")
    };
    assert_ne!(bus_id, TrackId(1));
    assert_eq!(s.project().track(bus_id).unwrap().kind, TrackKind::Bus);
    let outputs = &s.project().track(TrackId(1)).unwrap().inserts[0].pad_outputs;
    assert_eq!(outputs.get(&0), Some(&bus_id));
    assert_eq!(outputs.get(&1), Some(&bus_id));
    s.run_value("history.undo", json!({}), &RunOptions::human())
        .unwrap();
    assert_eq!(s.project().root.len(), 1);
    assert!(
        s.project().track(TrackId(1)).unwrap().inserts[0]
            .pad_outputs
            .is_empty()
    );
}

#[test]
fn existing_bus_and_main_round_trip_through_undo_and_save() {
    let mut s = song_with_bus(true);
    let bus = TrackId(3);
    let args = json!({"id":"plg_2", "pads":[{"pad":"A1"},{"pad":"A2"}], "to":"trk_3"});
    s.run_value("plugin.pad_output.set", args, &RunOptions::human())
        .unwrap();
    let outputs = &s.project().track(TrackId(1)).unwrap().inserts[0].pad_outputs;
    assert_eq!(outputs.get(&0), Some(&bus));
    assert_eq!(outputs.get(&1), Some(&bus));
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("song");
    s.save_as(&path).unwrap();
    drop(s);
    let mut s = Session::open(&path).unwrap();
    assert_eq!(
        s.project().track(TrackId(1)).unwrap().inserts[0]
            .pad_outputs
            .get(&0),
        Some(&bus)
    );
    s.run_value(
        "plugin.pad_output.set",
        json!({"id":"plg_2","pads":[{"pad":"A1"},{"pad":"A2"}],"to":"Main"}),
        &RunOptions::human(),
    )
    .unwrap();
    assert!(
        s.project().track(TrackId(1)).unwrap().inserts[0]
            .pad_outputs
            .is_empty()
    );
    s.run_value("history.undo", json!({}), &RunOptions::human())
        .unwrap();
    assert_eq!(
        s.project().track(TrackId(1)).unwrap().inserts[0]
            .pad_outputs
            .get(&0),
        Some(&bus)
    );
    drop(s);
}

#[test]
fn invalid_target_pad_and_instrument_do_not_change_the_song() {
    let mut s = song();
    let before = serde_json::to_value(s.project()).unwrap();
    for args in [
        json!({"id":"plg_2","pads":[{"pad":"A1"}],"to":"trk_77"}),
        json!({"id":"plg_2","pads":[{"pad":"I1"}],"to":"new"}),
        json!({"id":"plg_77","pads":[{"pad":"A1"}],"to":"new"}),
    ] {
        assert!(
            s.run_value("plugin.pad_output.set", args, &RunOptions::human())
                .is_err()
        );
        assert_eq!(serde_json::to_value(s.project()).unwrap(), before);
    }
    let mut other = s.project().clone();
    other.track_mut(TrackId(1)).unwrap().inserts[0].uid = "sampler".into();
    let mut other = Session::from_project(other);
    assert!(
        other
            .run_value(
                "plugin.pad_output.set",
                json!({"id":"plg_2","pads":[{"pad":"A1"}],"to":"new"}),
                &RunOptions::human()
            )
            .is_err()
    );
}

#[test]
fn removing_a_destination_bus_returns_pads_to_main_and_undo_restores_it() {
    let mut s = song_with_bus(true);
    s.run_value(
        "plugin.pad_output.set",
        json!({"id":"plg_2","pads":[{"pad":"A1"}],"to":"trk_3"}),
        &RunOptions::human(),
    )
    .unwrap();
    s.run_value(
        "track.delete",
        json!({"ids":["trk_3"]}),
        &RunOptions::human(),
    )
    .unwrap();
    assert!(
        s.project().track(TrackId(1)).unwrap().inserts[0]
            .pad_outputs
            .is_empty()
    );
    s.run_value("history.undo", json!({}), &RunOptions::human())
        .unwrap();
    assert_eq!(
        s.project().track(TrackId(1)).unwrap().inserts[0]
            .pad_outputs
            .get(&0),
        Some(&TrackId(3))
    );
}

#[test]
fn new_bus_follows_the_kit_and_existing_buses_inside_a_folder() {
    let mut s = song_with_bus(true);
    let result = s
        .run_value("folder.add", json!({"name":"Drums"}), &RunOptions::human())
        .unwrap();
    let folder = result.data["id"].as_str().unwrap();
    for id in ["trk_1", "trk_3"] {
        s.run_value(
            "track.move",
            json!({"ids":[id], "into":folder}),
            &RunOptions::human(),
        )
        .unwrap();
    }
    let kit_parent = s.project().find_node(Node::Track(TrackId(1))).unwrap().0;
    let result = s
        .run_value(
            "plugin.pad_output.set",
            json!({"id":"plg_2","pads":[{"pad":"A1"}],"to":"new"}),
            &RunOptions::human(),
        )
        .unwrap();
    let bus: TrackId = result.data["output"].as_str().unwrap().parse().unwrap();
    let (parent, at) = s.project().find_node(Node::Track(bus)).unwrap();
    assert_eq!(parent, kit_parent);
    assert_eq!(
        s.project().children(parent)[at - 2..=at],
        [
            Node::Track(TrackId(1)),
            Node::Track(TrackId(3)),
            Node::Track(bus)
        ]
    );
    s.run_value("history.undo", json!({}), &RunOptions::human())
        .unwrap();
    assert!(!s.project().tracks.contains_key(&bus));
}

#[test]
fn a_folder_bus_stays_when_a_pad_uses_it() {
    let mut s = song();
    let result = s
        .run_value("folder.add", json!({"name":"Drums"}), &RunOptions::human())
        .unwrap();
    let folder = result.data["id"].as_str().unwrap().to_string();
    let bus: TrackId = result.data["bus"].as_str().unwrap().parse().unwrap();
    s.run_value(
        "plugin.pad_output.set",
        json!({"id":"plg_2","pads":[{"pad":"A1"}],"to":bus.to_string()}),
        &RunOptions::human(),
    )
    .unwrap();
    s.run_value(
        "folder.delete",
        json!({"ids":[folder]}),
        &RunOptions::human(),
    )
    .unwrap();
    assert!(s.project().tracks.contains_key(&bus));
    assert_eq!(
        s.project().track(TrackId(1)).unwrap().inserts[0]
            .pad_outputs
            .get(&0),
        Some(&bus)
    );
}

#[test]
fn new_bus_uses_the_active_pads_sound_name() {
    let mut s = song();
    s.run_value(
        "plugin.samples.add",
        json!({"id":"plg_2","files":[{"file":"Snare.wav"}],"pad":"A2"}),
        &RunOptions::human(),
    )
    .unwrap();
    let result = s
        .run_value(
            "plugin.pad_output.set",
            json!({"id":"plg_2","pads":[{"pad":"A1"},{"pad":"A2"}],"active":"A2","to":"new"}),
            &RunOptions::human(),
        )
        .unwrap();
    let bus: TrackId = result.data["output"].as_str().unwrap().parse().unwrap();
    assert_eq!(s.project().track(bus).unwrap().name, "Snare");
}

#[test]
fn a_pad_bus_cannot_feed_back_to_its_kit() {
    let mut s = song_with_bus(true);
    assert!(
        s.project()
            .routing_ok_with(&[(TrackId(3), TrackId(1))])
            .is_ok()
    );
    s.run_value(
        "plugin.pad_output.set",
        json!({"id":"plg_2","pads":[{"pad":"A1"}],"to":"trk_3"}),
        &RunOptions::human(),
    )
    .unwrap();
    assert!(
        s.project()
            .routing_ok_with(&[(TrackId(3), TrackId(1))])
            .is_err()
    );
}

#[test]
fn a_plugin_saved_before_output_existed_defaults_to_main() {
    let s = song();
    let plugin = &s.project().track(TrackId(1)).unwrap().inserts[0];
    let mut old = serde_json::to_value(plugin).unwrap();
    old.as_object_mut().unwrap().remove("pad_outputs");
    let opened: Plugin = serde_json::from_value(old).unwrap();
    assert!(opened.pad_outputs.is_empty());
}

#[test]
fn freezing_a_routed_kit_explains_why_it_is_refused() {
    let mut s = song_with_bus(true);
    s.run_value(
        "plugin.pad_output.set",
        json!({"id":"plg_2","pads":[{"pad":"A1"}],"to":"trk_3"}),
        &RunOptions::human(),
    )
    .unwrap();
    let error = s
        .run_value(
            "track.freeze.apply",
            json!({"tracks":[{"id":"trk_1","file":"freeze.wav","start":0}]}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert_eq!(
        error.message,
        "「Drums」はパッドをバスへ出力しているため、フリーズできません"
    );
}

#[test]
fn duplicating_the_track_keeps_its_bus_but_presets_leave_the_route_out() {
    let mut s = song_with_bus(true);
    s.run_value(
        "plugin.pad_output.set",
        json!({"id":"plg_2","pads":[{"pad":"A1"}],"to":"trk_3"}),
        &RunOptions::human(),
    )
    .unwrap();
    let duplicate = s
        .run_value(
            "track.duplicate",
            json!({"ids":["trk_1"],"mode":"all"}),
            &RunOptions::human(),
        )
        .unwrap();
    let copy: TrackId = duplicate.data["ids"][0].as_str().unwrap().parse().unwrap();
    assert_eq!(
        s.project().track(copy).unwrap().inserts[0]
            .pad_outputs
            .get(&0),
        Some(&TrackId(3))
    );
    let original = s.project().track(TrackId(1)).unwrap();
    let (chain, _) = Chain::of_track(original);
    assert!(chain.devices[0].pad_outputs.is_empty());
    let preset = TrackPreset::snapshot(s.project(), TrackId(1), "Drums").unwrap();
    assert!(preset.inserts[0].pad_outputs.is_empty());
}
