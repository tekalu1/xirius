//! A song's player is started a little at a time (ENG-064): each step opens one plugin, the
//! progress says how many tracks are done, the build takes what was opened, and each plugin's
//! opening time is kept. A plugin that cannot be opened says so as before and the song opens.
use std::collections::BTreeMap;
use std::sync::Arc;
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_plugins::{Catalog, probe};
use xirius_session::{
    host::HostConfig,
    plugins::Isolation,
    starting::{Starting, UnitState},
};

fn catalog() -> (Arc<Catalog>, String) {
    xirius_test_vst3::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dll = deps.join("xirius_test_vst3.dll");
    assert!(
        dll.exists(),
        "the fixture's cdylib is built with its dev dependency"
    );
    let entries = probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    (
        Arc::new(Catalog {
            entries,
            failures: Vec::new(),
        }),
        uid,
    )
}

fn plugin(id: u64, kind: PluginKind, uid: &str) -> Plugin {
    Plugin {
        samples: Vec::new(),
        id: PluginId(id),
        name: format!("Test {id}"),
        kind,
        format: "vst3".into(),
        uid: uid.to_string(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::new(),
        state: None,
        isolation: PluginIsolation::InProcess,
        sidechain: None,
    }
}

fn song(tracks: &[(&str, TrackKind, Vec<Plugin>)]) -> Project {
    let mut p = Project::new("starting");
    for (i, (name, kind, inserts)) in tracks.iter().enumerate() {
        let id = TrackId(100 + i as u64);
        let mut t = Track::new(id, *kind, name.to_string());
        t.inserts = inserts.clone();
        p.tracks.insert(id, Arc::new(t));
        p.root.push(Node::Track(id));
    }
    p
}

#[test]
fn each_step_opens_one_plugin_and_the_build_takes_them() {
    let (catalog, uid) = catalog();
    let p = song(&[
        (
            "Gt",
            TrackKind::Audio,
            vec![plugin(1, PluginKind::Effect, &uid)],
        ),
        (
            "Ba",
            TrackKind::Audio,
            vec![plugin(2, PluginKind::Effect, &uid)],
        ),
        ("Vo", TrackKind::Audio, Vec::new()),
        (
            "Dr",
            TrackKind::Audio,
            vec![plugin(3, PluginKind::Effect, &uid)],
        ),
    ]);
    let mut s = Starting::new(
        &p,
        HostConfig {
            catalog: Some(catalog),
            isolation: Some(Isolation::InProcess),
            ..Default::default()
        },
    );
    assert!(!s.is_empty());
    let first = s.progress();
    let names: Vec<&str> = first.units.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        ["Gt", "Ba", "Dr"],
        "a track without plugins is not listed"
    );
    assert_eq!(first.done, 0);
    assert!(first.units.iter().all(|(_, u)| *u == UnitState::Waiting));
    // The first step asks the host processes (none here); the next one is under way.
    assert!(!s.step());
    let asked = s.progress();
    assert_eq!(asked.done, 0);
    assert_eq!(asked.units[0].1, UnitState::Working);
    assert_eq!(asked.units[1].1, UnitState::Waiting);
    let mut done = Vec::new();
    while !s.step() {
        done.push(s.progress().done);
        assert!(done.len() < 10, "{done:?}");
    }
    done.push(s.progress().done);
    assert_eq!(done, [1, 2, 3]);
    let (host, _audio, warnings) = s.finish(&p).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    for id in 1..=3 {
        assert!(
            host.plugin_timings().opened(id).is_some(),
            "plugin {id}'s opening time is kept"
        );
    }
}

#[test]
fn a_plugin_that_cannot_be_opened_is_said_and_the_song_opens() {
    let (catalog, uid) = catalog();
    let p = song(&[
        (
            "Keys",
            TrackKind::Instrument,
            vec![plugin(1, PluginKind::Instrument, "missing-uid")],
        ),
        (
            "Gt",
            TrackKind::Audio,
            vec![plugin(2, PluginKind::Effect, &uid)],
        ),
    ]);
    let mut s = Starting::new(
        &p,
        HostConfig {
            catalog: Some(catalog),
            isolation: Some(Isolation::InProcess),
            ..Default::default()
        },
    );
    while !s.step() {}
    assert_eq!(s.progress().done, 2);
    let (_host, _audio, warnings) = s.finish(&p).unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("Keys: ")
                && w.ends_with("代わりのインストゥルメントで再生します")),
        "{warnings:?}"
    );
}

#[test]
fn finishing_before_the_steps_opens_the_rest_on_the_spot() {
    let (catalog, uid) = catalog();
    let p = song(&[
        (
            "Gt",
            TrackKind::Audio,
            vec![plugin(1, PluginKind::Effect, &uid)],
        ),
        (
            "Ba",
            TrackKind::Audio,
            vec![plugin(2, PluginKind::Effect, &uid)],
        ),
    ]);
    let mut s = Starting::new(
        &p,
        HostConfig {
            catalog: Some(catalog),
            isolation: Some(Isolation::InProcess),
            ..Default::default()
        },
    );
    s.step();
    s.step();
    assert_eq!(s.progress().done, 1);
    let (host, _audio, warnings) = s.finish(&p).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(host.plugin_timings().opened(1).is_some());
    assert!(host.plugin_timings().opened(2).is_some());
}
