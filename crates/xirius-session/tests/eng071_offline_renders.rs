//! A builder used for several offline renders, as for the stems (ENG-071): each render's mix
//! plays as soon as it is built, so the plugin is the instance the last render opened, not one
//! kept from an earlier render.
use std::{collections::BTreeMap, sync::Arc};
use xirius_core::{
    id::{Node, PluginId, TrackId},
    model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind},
};
use xirius_plugins::{Catalog, probe};
use xirius_session::{build::Builder, export, graph::Select, plugins::Isolation};

const PID: PluginId = PluginId(1);

#[test]
fn each_offline_render_plays_the_instance_it_opened() {
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    assert!(dll.is_file());
    let entries = probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });
    let mut track = Track::new(TrackId(100), TrackKind::Audio, "T".to_string());
    track.inserts.push(Plugin {
        id: PID,
        name: "Test".into(),
        kind: PluginKind::Effect,
        format: "vst3".into(),
        uid,
        bypass: false,
        deactivated: false,
        params: BTreeMap::from([("1".into(), 0.5)]),
        state: None,
        isolation: PluginIsolation::InProcess,
        sidechain: None,
    });
    let mut p = Project::new("offline renders");
    p.root.push(Node::Track(track.id));
    p.tracks.insert(track.id, Arc::new(track));

    let mut builder = Builder::new(48000, None);
    builder.set_catalog(Some(catalog));
    builder.set_isolation(Isolation::InProcess);
    let mut seen = Vec::new();
    for _ in 0..2 {
        let out = export::render(&mut builder, &p, Select::All, 0, 480, 0).unwrap();
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        let (_, generation) = builder.plugins().local(PID).expect("it plays in the app");
        seen.push(generation);
    }
    assert_ne!(
        seen[0], seen[1],
        "the second render plays the instance it opened"
    );
}
