//! A plugin the catalogue marks as ARA runs in this process whatever the song asks (PLG-025),
//! with a warning; the same plugin without the mark goes to the host the song names. The
//! host path does not exist: a plugin sent there would fail to open.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_plugins::{Catalog, probe};
use xirius_session::build::Builder;
use xirius_session::graph::Select;
use xirius_session::plugins::Isolation;

const PID: PluginId = PluginId(200);

fn song(uid: &str, isolation: PluginIsolation) -> Project {
    let mut p = Project::new("ara");
    let id = TrackId(100);
    let mut track = Track::new(id, TrackKind::Audio, "Vocal");
    track.inserts.push(Plugin {
        id: PID,
        name: "Pitch".into(),
        kind: PluginKind::Effect,
        format: "vst3".into(),
        uid: uid.into(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::new(),
        state: None,
        isolation,
        sidechain: None,
    });
    p.tracks.insert(id, Arc::new(track));
    p.root.push(Node::Track(id));
    p
}

fn builder(catalog: Catalog) -> Builder {
    let mut b = Builder::new(48_000, None);
    b.set_catalog(Some(Arc::new(catalog)));
    b.set_isolation(Isolation::PerPlugin(Some(PathBuf::from(
        "C:/nowhere/xirius-plugin-host.exe",
    ))));
    b
}

#[test]
fn an_ara_plugin_stays_in_this_process() {
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    assert!(dll.exists(), "the fixture's cdylib is built with the tests");
    let entries = probe(&dll).unwrap();
    assert!(entries.iter().all(|e| !e.ara), "the fixture is not ARA");
    let uid = entries[0].uid.clone();

    for mode in [PluginIsolation::Shared, PluginIsolation::Dedicated] {
        let p = song(&uid, mode);
        let mut ara = entries.clone();
        ara.iter_mut().for_each(|e| e.ara = true);
        let mut b = builder(Catalog {
            entries: ara,
            failures: Vec::new(),
        });
        let built = b.build(&p, Select::All);
        assert_eq!(b.plugins().placement_of(PID), None, "{mode:?}: in process");
        assert!(b.plugins().server().is_none());
        let [w] = built.warnings.as_slice() else {
            panic!("{mode:?}: {:?}", built.warnings);
        };
        assert!(w.contains("ARA") && w.contains(mode.name()), "{w}");

        // Without the mark it goes where the song says.
        let mut b = builder(Catalog {
            entries: entries.clone(),
            failures: Vec::new(),
        });
        b.build(&p, Select::All);
        assert_eq!(b.plugins().placement_of(PID), Some(mode));
    }

    // Asked to run here, an ARA plugin says nothing.
    let p = song(&uid, PluginIsolation::InProcess);
    let mut ara = entries.clone();
    ara.iter_mut().for_each(|e| e.ara = true);
    let mut b = builder(Catalog {
        entries: ara,
        failures: Vec::new(),
    });
    let built = b.build(&p, Select::All);
    assert!(built.warnings.is_empty(), "{:?}", built.warnings);
}
