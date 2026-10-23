//! ENG-078's session side: the keyed insert's index follows the built insert
//! row (skipped plugins do not move it), `KeyMode::Output` taps the channel
//! output as-is, and a channel hidden by solo reaches the plan as `mute`, so
//! its keys stop — for a track and a bus source alike.

use std::collections::BTreeMap;
use std::sync::Arc;

use xirius_core::id::{Node, PluginId, SendId, TrackId};
use xirius_core::model::{
    KeyMode, KeySource, Output, Plugin, PluginIsolation, PluginKind, Project, Sidechain, Track,
    TrackKind,
};
use xirius_engine::{KeyFrom, TrackSlot};
use xirius_session::build::Builder;
use xirius_session::graph::{Mix, Scope, Select};

fn effect(id: u64, format: &str, uid: &str, sidechain: Option<Sidechain>) -> Plugin {
    Plugin {
        id: PluginId(id),
        name: uid.to_string(),
        kind: PluginKind::Effect,
        format: format.into(),
        uid: uid.into(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::new(),
        state: None,
        samples: Vec::new(),
        isolation: PluginIsolation::Dedicated,
        sidechain,
        preset: None,
    }
}

fn add(p: &mut Project, t: Track) -> TrackId {
    let id = t.id;
    p.root.push(Node::Track(id));
    p.tracks.insert(id, Arc::new(t));
    id
}

fn track(id: u64) -> Track {
    Track::new(TrackId(id), TrackKind::Audio, format!("T{id}"))
}

/// A plugin that cannot be built does not count in the keyed insert index;
/// the mode's `pre` and gain are what reach the plan.
#[test]
fn a_skipped_plugin_does_not_move_the_keyed_insert() {
    let mut p = Project::new("keys");
    let src = add(&mut p, track(1));
    let mut r = track(2);
    r.inserts
        .push(effect(10, "vst3", "com.maker.Nothing", None));
    r.inserts.push(effect(
        11,
        "builtin",
        "gain",
        Some(Sidechain {
            on: true,
            sources: vec![
                KeySource {
                    id: SendId(50),
                    track: src,
                    mode: KeyMode::Send {
                        pre: true,
                        level_db: -6.0,
                        muted_level: None,
                    },
                },
                KeySource {
                    id: SendId(51),
                    track: src,
                    mode: KeyMode::Output,
                },
            ],
        }),
    ));
    add(&mut p, r);

    let built = Builder::new(48_000, None).build(&p, Select::All);
    let TrackSlot::New(t) = &built.spec.tracks[1] else {
        panic!("the receiver was not built fresh");
    };
    assert_eq!(t.inserts.len(), 1, "only the gain could be built");
    assert_eq!(t.keys.len(), 1);
    let k = &t.keys[0];
    assert_eq!(k.insert, 0, "the gain is the first built insert");
    assert_eq!(k.plugin, 11);
    assert!(k.on);
    assert_eq!(k.sources.len(), 2);
    assert_eq!(k.sources[0].id, 50);
    assert_eq!(k.sources[0].from, KeyFrom::Track(0));
    assert!(k.sources[0].pre);
    assert!((k.sources[0].level - 0.501_187_2).abs() < 1e-6, "-6 dB");
    assert!(!k.sources[1].pre, "Output taps the channel output");
    assert_eq!(k.sources[1].level, 1.0);
}

fn keyed_by(id: u64, src: TrackId) -> Track {
    let mut r = track(id);
    r.inserts.push(effect(
        id * 10,
        "builtin",
        "gain",
        Some(Sidechain {
            on: true,
            sources: vec![KeySource {
                id: SendId(id * 10 + 1),
                track: src,
                mode: KeyMode::Send {
                    pre: true,
                    level_db: 0.0,
                    muted_level: None,
                },
            }],
        }),
    ));
    r
}

/// Soloing the receiver silences its key source — as `mute`, which the
/// engine applies to the key taps too — so the keys stop, the Studio Pro
/// behavior (ENG-078; a Solo Safe escape is left to a later item). A bus
/// source behaves like a track source, and so does what feeds it.
#[test]
fn a_soloed_receiver_silences_its_key_source() {
    let mut p = Project::new("solo");
    let drums = add(&mut p, Track::new(TrackId(3), TrackKind::Bus, "Drums"));
    let mut kick = track(1);
    kick.output = Some(Output::Bus(drums));
    let kick = add(&mut p, kick);
    let mut by_track = keyed_by(2, kick);
    by_track.solo = true;
    add(&mut p, by_track);
    let mut by_bus = keyed_by(4, drums);
    by_bus.solo = true;
    add(&mut p, by_bus);

    let g = xirius_session::graph::Graph::of(&p);
    let mix = Mix::of(&p, &g, &Scope::new(&p, Select::All));
    let kick_at = g.track_index(kick).unwrap();
    let drums_at = g.bus_index(drums).unwrap();
    assert!(mix.tracks[kick_at].mute, "a track source is silenced");
    assert!(mix.buses[drums_at].mute, "a bus source is silenced");

    let built = Builder::new(48_000, None).build(&p, Select::All);
    let TrackSlot::New(t) = &built.spec.tracks[kick_at] else {
        panic!("the source was not built fresh");
    };
    assert!(t.mute, "the plan gets the solo silence as the mute");
    assert!(built.spec.buses[drums_at].mute);
}
