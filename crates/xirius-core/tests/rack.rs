//! The rack model (PRJ-026): saving, loading with repair, the relaxed
//! instrument rule and `rack.show`.
use serde_json::{Value, json};
use std::sync::Arc;
use xirius_core::id::{NodeId, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginKind, Project, Track, TrackKind};
use xirius_core::rack::{Branch, Layer, Rack, RackNode, Source, Split, SplitMode};
use xirius_core::store::Store;
use xirius_core::track_preset::AdjustKind;
use xirius_core::{Session, session::RunOptions};

fn run(s: &mut Session, n: &str, a: Value) -> Value {
    s.run_value(n, a, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{n}: {e:?}"))
        .data
}

fn fresh_id() -> impl FnMut() -> u64 {
    let mut n = 9_000u64;
    move || {
        n += 1;
        n
    }
}

fn plugin(id: u64, kind: PluginKind) -> Plugin {
    Plugin {
        id: PluginId(id),
        name: format!("p{id}"),
        kind,
        format: "builtin".into(),
        uid: format!("u{id}"),
        bypass: false,
        deactivated: false,
        params: Default::default(),
        state: None,
        samples: Vec::new(),
        isolation: Default::default(),
        sidechain: None,
        preset: None,
    }
}

fn split(id: u64, branches: Vec<Vec<RackNode>>) -> RackNode {
    RackNode::Split(Split {
        id: NodeId(id),
        mode: SplitMode::Normal,
        crossovers: Vec::new(),
        branches: branches
            .into_iter()
            .enumerate()
            .map(|(i, fx)| Branch {
                id: NodeId(id * 10 + i as u64 + 1),
                fx,
                gain_db: 0.0,
                pan: 0.0,
                mute: false,
            })
            .collect(),
    })
}

fn multi(id: u64, instruments: &[u64]) -> Source {
    Source::Multi {
        id: NodeId(id),
        layers: instruments
            .iter()
            .enumerate()
            .map(|(i, x)| Layer {
                id: NodeId(id * 10 + i as u64 + 1),
                name: format!("l{x}"),
                instrument: PluginId(*x),
                fx: Vec::new(),
                keys: (0, 127),
                velocity: (1, 127),
                transpose: 0,
                gain_db: 0.0,
                pan: 0.0,
                mute: false,
            })
            .collect(),
    }
}

fn track(id: u64, kind: TrackKind, inserts: Vec<Plugin>, rack: Option<Rack>) -> Track {
    let mut t = Track::new(TrackId(id), kind, format!("t{id}"));
    t.inserts = inserts;
    t.rack = rack;
    t
}

fn project(tracks: Vec<Track>) -> Project {
    let mut p = Project::new("r");
    // The default tempo/timesig entries hold ids 1 and 2, under the test
    // plug-ins'; move them out of the way so `validate_deep` sees a clean
    // id space.
    for (i, t) in p.tempo.iter_mut().enumerate() {
        t.id = xirius_core::id::TempoId(900 + i as u64);
    }
    for (i, t) in p.timesig.iter_mut().enumerate() {
        t.id = xirius_core::id::TimeSigId(950 + i as u64);
    }
    for t in tracks {
        p.tracks.insert(t.id, Arc::new(t.clone()));
        p.root.push(xirius_core::id::Node::Track(t.id));
    }
    p.next_id = 1000;
    p
}

fn dir(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::Builder::new()
        .prefix(&format!("xirius-rack-{name}-"))
        .tempdir()
        .unwrap();
    let dir = d.path().join("song.xirius");
    std::fs::create_dir_all(&dir).unwrap();
    (d, dir)
}

#[test]
fn a_track_without_a_rack_writes_no_rack_key() {
    let (_d, dir) = dir("shape");
    let p = project(vec![track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Effect),
        ],
        None,
    )]);
    Store::new(&dir).write_project(&p).unwrap();
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    assert!(!text.contains("\"rack\""));
}

#[test]
fn a_rack_round_trips_through_project_json() {
    let (_d, dir) = dir("roundtrip");
    let rack = Rack {
        source: Some(multi(50, &[1, 2])),
        fx: vec![
            RackNode::Plugin(PluginId(3)),
            split(60, vec![vec![RackNode::Plugin(PluginId(4))], vec![]]),
        ],
    };
    let p = project(vec![track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Instrument),
            plugin(3, PluginKind::Effect),
            plugin(4, PluginKind::Effect),
        ],
        Some(rack.clone()),
    )]);
    Store::new(&dir).write_project(&p).unwrap();
    let (q, rows) = Store::new(&dir).read_project_report().unwrap();
    assert!(rows.is_empty(), "a valid rack needs no repair: {rows:?}");
    assert_eq!(q.tracks[&TrackId(1)].rack, Some(rack));
}

#[test]
fn an_old_song_without_a_rack_opens_unchanged() {
    let (_d, dir) = dir("old");
    let p = project(vec![track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Effect),
        ],
        None,
    )]);
    Store::new(&dir).write_project(&p).unwrap();
    let (q, rows) = Store::new(&dir).read_project_report().unwrap();
    assert!(rows.is_empty());
    assert_eq!(q.tracks[&TrackId(1)].rack, None);
}

#[test]
fn reading_repairs_the_rack_and_reports_every_change() {
    let t = track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Effect),
            plugin(3, PluginKind::Effect),
            plugin(4, PluginKind::Effect),
        ],
        Some(Rack {
            source: Some(Source::Single(PluginId(2))), // an effect: rejected
            fx: vec![
                RackNode::Plugin(PluginId(9)), // not in inserts: dropped
                RackNode::Plugin(PluginId(3)),
                RackNode::Plugin(PluginId(3)), // twice: dropped
                RackNode::Plugin(PluginId(1)), // instrument in fx: rejected
            ],
        }),
    );
    let mut t = t;
    let rows = t.fix_rack(&mut fresh_id());
    // p1 becomes the source, p4 is filled at the end, then the rack is a
    // plain line and folds away.
    assert!(t.rack.is_none());
    let kinds: Vec<AdjustKind> = rows.iter().map(|r| r.kind).collect();
    assert!(kinds.contains(&AdjustKind::Rejected));
    assert!(kinds.contains(&AdjustKind::Dropped));
    assert!(kinds.contains(&AdjustKind::Filled));
    assert_eq!(
        t.inserts.iter().map(|x| x.id.0).collect::<Vec<_>>(),
        vec![1, 3, 2, 4]
    );
}

#[test]
fn a_serial_rack_folds_back_to_the_plain_shape() {
    let mut t = track(
        1,
        TrackKind::Audio,
        vec![plugin(1, PluginKind::Effect), plugin(2, PluginKind::Effect)],
        Some(Rack {
            source: None,
            fx: vec![RackNode::Plugin(PluginId(2)), RackNode::Plugin(PluginId(1))],
        }),
    );
    assert!(t.fix_rack(&mut fresh_id()).is_empty());
    assert_eq!(t.rack, None);
    assert_eq!(
        t.inserts.iter().map(|x| x.id.0).collect::<Vec<_>>(),
        vec![2, 1]
    );
}

#[test]
fn splits_over_the_limits_are_mended() {
    // A split nested deeper than MAX_SPLIT_DEPTH is dropped; a one-branch
    // split unfolds; more than five branches are trimmed.
    let mut t = track(
        1,
        TrackKind::Audio,
        vec![
            plugin(1, PluginKind::Effect),
            plugin(2, PluginKind::Effect),
            plugin(3, PluginKind::Effect),
            plugin(4, PluginKind::Effect),
            plugin(5, PluginKind::Effect),
            plugin(6, PluginKind::Effect),
            plugin(7, PluginKind::Effect),
            plugin(8, PluginKind::Effect),
        ],
        Some(Rack {
            source: None,
            fx: vec![
                split(10, vec![vec![RackNode::Plugin(PluginId(1))]]),
                split(
                    20,
                    vec![
                        vec![RackNode::Plugin(PluginId(2))],
                        vec![RackNode::Plugin(PluginId(3))],
                        vec![RackNode::Plugin(PluginId(4))],
                        vec![RackNode::Plugin(PluginId(5))],
                        vec![RackNode::Plugin(PluginId(6))],
                        vec![],
                    ],
                ),
                split(
                    30,
                    vec![
                        vec![RackNode::Plugin(PluginId(8))],
                        vec![split(
                            40,
                            vec![
                                vec![],
                                vec![split(
                                    41,
                                    vec![
                                        vec![],
                                        vec![split(
                                            42,
                                            vec![
                                                vec![],
                                                vec![split(
                                                    43,
                                                    vec![
                                                        vec![RackNode::Plugin(PluginId(7))],
                                                        vec![],
                                                    ],
                                                )],
                                            ],
                                        )],
                                    ],
                                )],
                            ],
                        )],
                    ],
                ),
            ],
        }),
    );
    let rows = t.fix_rack(&mut fresh_id());
    assert!(!rows.is_empty());
    let Some(r) = &t.rack else {
        panic!("a real split stays")
    };
    // The one-branch split unfolded inline; the six-branch kept five; the
    // five-deep split is gone and its plug-in is filled at the end.
    let top: Vec<&RackNode> = r.fx.iter().collect();
    assert!(matches!(top[0], RackNode::Plugin(_)));
    if let RackNode::Split(s) = top[1] {
        assert_eq!(s.branches.len(), 5);
    } else {
        panic!("second node is the trimmed split");
    }
    assert_eq!(
        t.inserts.iter().map(|x| x.id.0).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 8, 7]
    );
}

#[test]
fn a_trimmed_branchs_plugins_go_back_into_the_chain() {
    // The sixth branch holds a plug-in: trimming must not leave the insert
    // orphaned (which failed the song's open with "not in the rack").
    let (_d, dir) = dir("trim");
    let p = project(vec![track(
        2,
        TrackKind::Audio,
        (1..=6).map(|i| plugin(i, PluginKind::Effect)).collect(),
        Some(Rack {
            source: None,
            fx: vec![split(
                10,
                (1..=6)
                    .map(|i| vec![RackNode::Plugin(PluginId(i))])
                    .collect(),
            )],
        }),
    )]);
    Store::new(&dir).write_project(&p).unwrap();
    let (q, rows) = Store::new(&dir).read_project_report().unwrap();
    let t = &q.tracks[&TrackId(2)];
    let r = t.rack.as_ref().expect("the trimmed split stays");
    let RackNode::Split(s) = &r.fx[0] else {
        panic!("first node is the split")
    };
    assert_eq!(s.branches.len(), 5);
    // The sixth plug-in rejoins the chain right after the split.
    assert!(matches!(&r.fx[1], RackNode::Plugin(x) if *x == PluginId(6)));
    assert!(rows.iter().any(|r| r.kind == AdjustKind::Dropped));
    q.validate().unwrap();
}

#[test]
fn frequency_crossovers_are_made_to_match_the_branches() {
    let s = Split {
        id: NodeId(1),
        mode: SplitMode::Frequency,
        crossovers: vec![2000.0, 200.0, -5.0],
        branches: (0..3)
            .map(|i| Branch {
                id: NodeId(10 + i),
                fx: Vec::new(),
                gain_db: 0.0,
                pan: 0.0,
                mute: false,
            })
            .collect(),
    };
    // branches 3 => 2 ascending positive crossovers.
    let mut t = track(
        1,
        TrackKind::Audio,
        vec![plugin(1, PluginKind::Effect)],
        Some(Rack {
            source: None,
            fx: vec![RackNode::Split(s.clone())],
        }),
    );
    t.fix_rack(&mut fresh_id());
    let Some(Rack { fx, .. }) = &t.rack else {
        panic!("rack stays")
    };
    let RackNode::Split(s) = &fx[0] else {
        panic!("split stays")
    };
    assert_eq!(s.crossovers.len(), 2);
    assert!(s.crossovers[0] > 0.0 && s.crossovers[0] < s.crossovers[1]);
}

#[test]
fn a_multi_lets_a_track_hold_several_instruments() {
    let p = project(vec![track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Instrument),
            plugin(3, PluginKind::Effect),
        ],
        Some(Rack {
            source: Some(multi(50, &[1, 2])),
            fx: vec![RackNode::Plugin(PluginId(3))],
        }),
    )]);
    p.validate().unwrap();
    // Two instruments without a multi is still refused.
    let bad = project(vec![track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Instrument),
        ],
        None,
    )]);
    assert!(bad.validate().is_err());
}

/// A session whose track `TrackId(2)` of `kind` carries `inserts` and
/// `rack`, saved as a new song folder and opened again.
fn racked_session(
    name: &str,
    kind: TrackKind,
    inserts: Vec<Plugin>,
    rack: Option<Rack>,
) -> (tempfile::TempDir, Session, TrackId) {
    let (_d, dir) = dir(name);
    let mut p = project(vec![track(2, kind, inserts, rack)]);
    // Re-number the rack's node ids inside the project's own id space.
    fn bump(r: &mut Rack, p: &mut Project) {
        let mut map = |id: &mut NodeId| *id = NodeId(p.alloc());
        if let Some(Source::Multi { id, layers }) = &mut r.source {
            map(id);
            for l in layers {
                map(&mut l.id);
                for n in &mut l.fx {
                    if let RackNode::Split(s) = n {
                        map(&mut s.id);
                        for b in &mut s.branches {
                            map(&mut b.id);
                        }
                    }
                }
            }
        }
        for n in &mut r.fx {
            if let RackNode::Split(s) = n {
                map(&mut s.id);
                for b in &mut s.branches {
                    map(&mut b.id);
                }
            }
        }
    }
    let t = p.tracks.get_mut(&TrackId(2)).map(Arc::make_mut).unwrap();
    if let Some(mut r) = t.rack.take() {
        bump(&mut r, &mut p);
        p.tracks
            .get_mut(&TrackId(2))
            .map(Arc::make_mut)
            .unwrap()
            .rack = Some(r);
    }
    let s = Session::create(&dir, p).unwrap();
    (_d, s, TrackId(2))
}

#[test]
fn the_relaxed_instrument_rule_only_applies_to_a_multi() {
    // One instrument already sits in the multi's layer.
    let (_d, mut s, tid) = racked_session(
        "multi",
        TrackKind::Instrument,
        vec![plugin(3, PluginKind::Instrument)],
        Some(Rack {
            source: Some(multi(50, &[3])),
            fx: Vec::new(),
        }),
    );
    // plugin.add --kind instrument joins the multi as a new layer.
    let id: PluginId = run(
        &mut s,
        "plugin.add",
        json!({"track": tid.to_string(), "kind": "instrument", "name": "Synth"}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let t = &s.project().tracks[&tid];
    let Some(Rack {
        source: Some(Source::Multi { layers, .. }),
        ..
    }) = &t.rack
    else {
        panic!("the multi stays: {:?}", t.rack)
    };
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[1].instrument, id);
    run(&mut s, "history.undo", json!({}));
    assert!(s.project().tracks[&tid].rack.as_ref().is_some_and(
        |r| matches!(&r.source, Some(Source::Multi { layers, .. }) if layers.len() == 1)
    ));
}

#[test]
fn plugin_commands_keep_the_rack_consistent() {
    let (_d, mut s, tid) = racked_session(
        "ops",
        TrackKind::Audio,
        Vec::new(),
        Some(Rack {
            source: None,
            fx: vec![split(50, vec![vec![], vec![]])],
        }),
    );
    let a: PluginId = run(
        &mut s,
        "plugin.add",
        json!({"track": tid.to_string(), "name": "EQ"}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let b: PluginId = run(
        &mut s,
        "plugin.add",
        json!({"track": tid.to_string(), "name": "Comp"}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    // Every insert is in the rack exactly once; the new ones filled the end.
    let t = &s.project().tracks[&tid];
    let flat = t.rack.as_ref().unwrap().flatten_order();
    let mut sorted = flat.clone();
    sorted.sort();
    let mut inserts: Vec<_> = t.inserts.iter().map(|x| x.id).collect();
    inserts.sort();
    assert_eq!(sorted, inserts);
    // plugin.move --index 0 puts the node first in the top chain.
    run(
        &mut s,
        "plugin.move",
        json!({"id": b.to_string(), "track": tid.to_string(), "index": 0}),
    );
    let t = &s.project().tracks[&tid];
    assert!(matches!(
        t.rack.as_ref().unwrap().fx.first(),
        Some(RackNode::Plugin(x)) if *x == b
    ));
    // plugin.remove drops the reference from the rack.
    run(&mut s, "plugin.remove", json!({"ids": a.to_string()}));
    let t = &s.project().tracks[&tid];
    assert!(!t.rack.as_ref().unwrap().uses_plugin(a));
    s.project().validate_deep().unwrap();
}

#[test]
fn filled_layers_get_their_own_node_ids() {
    let (_d, mut s, tid) = racked_session(
        "layerids",
        TrackKind::Instrument,
        vec![plugin(3, PluginKind::Instrument)],
        Some(Rack {
            source: Some(multi(50, &[3])),
            fx: Vec::new(),
        }),
    );
    for name in ["A", "B"] {
        run(
            &mut s,
            "plugin.add",
            json!({"track": tid.to_string(), "kind": "instrument", "name": name}),
        );
    }
    let t = &s.project().tracks[&tid];
    let Some(Rack {
        source: Some(Source::Multi { layers, .. }),
        ..
    }) = &t.rack
    else {
        panic!("the multi stays")
    };
    assert_eq!(layers.len(), 3);
    let ids: Vec<NodeId> = layers.iter().map(|l| l.id).collect();
    assert!(ids.iter().all(|i| i.0 > 0));
    assert_ne!(ids[1], ids[2], "each filled layer gets a fresh id");
    s.project().validate_deep().unwrap();
}

#[test]
fn an_instrument_moved_inside_its_track_keeps_its_layer() {
    let mut layer_multi = multi(50, &[3]);
    if let Source::Multi { layers, .. } = &mut layer_multi {
        layers[0].name = "LOW".into();
        layers[0].keys = (36, 59);
        layers[0].velocity = (1, 100);
        layers[0].transpose = -12;
        layers[0].gain_db = -3.0;
        layers[0].pan = -0.5;
    }
    let (_d, mut s, tid) = racked_session(
        "layermove",
        TrackKind::Instrument,
        vec![plugin(3, PluginKind::Instrument)],
        Some(Rack {
            source: Some(layer_multi),
            fx: Vec::new(),
        }),
    );
    // A no-op move (same track, no index) must not rebuild the layer.
    run(&mut s, "plugin.move", json!({"id": "plg_3"}));
    let t = &s.project().tracks[&tid];
    let Some(Rack {
        source: Some(Source::Multi { layers, .. }),
        ..
    }) = &t.rack
    else {
        panic!("the multi stays")
    };
    assert_eq!(layers.len(), 1);
    let l = &layers[0];
    assert_eq!(l.name, "LOW");
    assert_eq!(l.keys, (36, 59));
    assert_eq!(l.velocity, (1, 100));
    assert_eq!(l.transpose, -12);
    assert_eq!(l.gain_db, -3.0);
    assert_eq!(l.pan, -0.5);
    s.project().validate_deep().unwrap();
}

#[test]
fn a_track_preset_goes_on_a_racked_track() {
    let (_d, mut s, tid) = racked_session(
        "preset",
        TrackKind::Instrument,
        vec![
            plugin(3, PluginKind::Instrument),
            plugin(4, PluginKind::Effect),
            plugin(5, PluginKind::Effect),
        ],
        Some(Rack {
            source: Some(Source::Single(PluginId(3))),
            fx: vec![
                RackNode::Plugin(PluginId(4)),
                split(50, vec![vec![RackNode::Plugin(PluginId(5))], vec![]]),
            ],
        }),
    );
    // Saving the same track and putting it back used to leave the rack
    // pointing at the old instrument and fail the project check.
    let preset = xirius_core::track_preset::TrackPreset::snapshot(s.project(), tid, "x")
        .unwrap()
        .to_value();
    run(
        &mut s,
        "track.preset.put",
        json!({"ids": [tid.to_string()], "preset": preset.to_string()}),
    );
    let t = &s.project().tracks[&tid];
    // The seat kept its place: the new instrument is the source again.
    let Some(Rack {
        source: Some(Source::Single(inst)),
        ..
    }) = &t.rack
    else {
        panic!("the rack keeps its source: {:?}", t.rack)
    };
    assert_ne!(*inst, PluginId(3), "the new instrument took the seat");
    s.project().validate_deep().unwrap();
}

#[test]
fn rack_show_returns_the_tree() {
    let (_d, mut s, tid) = racked_session(
        "show",
        TrackKind::Audio,
        Vec::new(),
        Some(Rack {
            source: None,
            fx: vec![split(50, vec![vec![], vec![]])],
        }),
    );
    let v = run(&mut s, "rack.show", json!({"track": tid.to_string()}));
    assert_eq!(v["implicit"], false);
    assert!(v["fx"][0]["split"]["branches"].as_array().unwrap().len() == 2);
    // A track with no rack answers the implicit serial shape.
    let plain: TrackId = run(&mut s, "track.add.audio", json!({}))["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let v = run(&mut s, "rack.show", json!({"track": plain.to_string()}));
    assert_eq!(v["implicit"], true);
}

#[test]
fn a_repaired_rack_is_announced_when_the_song_opens() {
    let (_d, dir) = dir("notice");
    let p = project(vec![track(
        2,
        TrackKind::Audio,
        vec![plugin(1, PluginKind::Effect)],
        Some(Rack {
            source: None,
            fx: vec![RackNode::Plugin(PluginId(99))],
        }),
    )]);
    Session::create(&dir, p.clone()).unwrap();
    // project.json holds the inconsistent rack; the history must not shadow
    // it, so the open reads the file itself.
    std::fs::remove_dir_all(dir.join("history")).unwrap();
    Store::new(&dir).write_project(&p).unwrap();
    let s = Session::open(&dir).unwrap();
    assert!(
        s.notices.iter().any(|n| n.contains("ラック")),
        "notices: {:?}",
        s.notices
    );
}

#[test]
fn a_state_only_repair_is_announced_too() {
    // project.json is clean but the history state holds an inconsistent
    // rack: the open still has to say it repaired something.
    let (_d, dir) = dir("statenotice");
    let good = project(vec![track(
        2,
        TrackKind::Audio,
        vec![plugin(1, PluginKind::Effect)],
        Some(Rack {
            source: None,
            fx: vec![
                RackNode::Plugin(PluginId(1)),
                split(50, vec![vec![], vec![]]),
            ],
        }),
    )]);
    Session::create(&dir, good.clone()).unwrap();
    let store = Store::new(&dir);
    let cursor = store.read_index().unwrap().unwrap().cursor;
    let mut bad = good;
    Arc::make_mut(bad.tracks.get_mut(&TrackId(2)).unwrap()).rack = Some(Rack {
        source: None,
        fx: vec![
            RackNode::Plugin(PluginId(1)),
            split(50, vec![vec![RackNode::Plugin(PluginId(99))], vec![]]),
        ],
    });
    store.write_state(cursor, &bad).unwrap();
    let s = Session::open(&dir).unwrap();
    assert!(
        s.notices.iter().any(|n| n.contains("ラック")),
        "notices: {:?}",
        s.notices
    );
}
