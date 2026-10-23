//! A sidechain key (ENG-078) in a track with a splitter (ENG-074): the keyed insert's index is
//! its place in the arranged row, where a splitter is one insert, and a keyed plug-in inside a
//! branch hears no key yet and says so.

use std::collections::BTreeMap;
use std::sync::Arc;

use xirius_core::id::{Node, NodeId, PluginId, SendId, TrackId};
use xirius_core::model::{
    KeyMode, KeySource, Plugin, PluginIsolation, PluginKind, Project, Sidechain, Track, TrackKind,
};
use xirius_core::rack::{Branch, Rack, RackNode, Split, SplitMode};
use xirius_engine::TrackSlot;
use xirius_session::build::Builder;
use xirius_session::graph::Select;

fn gain(id: u64, key_from: Option<TrackId>) -> Plugin {
    Plugin {
        id: PluginId(id),
        name: format!("G{id}"),
        kind: PluginKind::Effect,
        format: "builtin".into(),
        uid: "gain".into(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::new(),
        state: None,
        samples: Vec::new(),
        isolation: PluginIsolation::Dedicated,
        sidechain: key_from.map(|track| Sidechain {
            on: true,
            sources: vec![KeySource {
                id: SendId(id * 10),
                track,
                mode: KeyMode::Output,
            }],
        }),
        preset: None,
    }
}

fn branch(id: u64, fx: &[u64]) -> Branch {
    Branch {
        id: NodeId(id),
        fx: fx.iter().map(|p| RackNode::Plugin(PluginId(*p))).collect(),
        gain_db: 0.0,
        pan: 0.0,
        mute: false,
    }
}

/// A source track and a track whose inserts are 11, 12 and 13, with 11 and 12 in the two
/// branches of a splitter and 13 after it; `keyed` takes the key from the source.
fn song(keyed: u64) -> (Project, TrackId) {
    let mut p = Project::new("keys");
    let src = Track::new(TrackId(1), TrackKind::Audio, "S");
    let mut t = Track::new(TrackId(2), TrackKind::Audio, "T");
    t.inserts = [11, 12, 13]
        .map(|id| gain(id, (id == keyed).then_some(src.id)))
        .to_vec();
    t.rack = Some(Rack {
        source: None,
        fx: vec![
            RackNode::Split(Split {
                id: NodeId(20),
                mode: SplitMode::Normal,
                crossovers: Vec::new(),
                branches: vec![branch(21, &[11]), branch(22, &[12])],
            }),
            RackNode::Plugin(PluginId(13)),
        ],
    });
    for t in [src, t] {
        p.root.push(Node::Track(t.id));
        p.tracks.insert(t.id, Arc::new(t));
    }
    (p, TrackId(2))
}

#[test]
fn a_key_after_a_splitter_points_at_its_place_in_the_arranged_row() {
    let (p, t) = song(13);
    let built = Builder::new(48_000, None).build(&p, Select::All);
    let at = built.graph.track_index(t).unwrap();
    let TrackSlot::New(spec) = &built.spec.tracks[at] else {
        panic!("not built fresh");
    };
    assert_eq!(spec.inserts.len(), 2, "the splitter and the keyed gain");
    assert_eq!(spec.keys.len(), 1);
    assert_eq!(spec.keys[0].plugin, 13);
    assert_eq!(
        spec.keys[0].insert, 1,
        "the third plug-in is the second insert"
    );
}

#[test]
fn a_keyed_plugin_inside_a_branch_hears_no_key_and_says_so() {
    let (p, t) = song(12);
    let built = Builder::new(48_000, None).build(&p, Select::All);
    let at = built.graph.track_index(t).unwrap();
    let TrackSlot::New(spec) = &built.spec.tracks[at] else {
        panic!("not built fresh");
    };
    assert!(spec.keys.is_empty());
    assert!(
        built
            .warnings
            .iter()
            .any(|w| w.contains("G12") && w.contains("サイドチェーン")),
        "{:?}",
        built.warnings
    );
}
