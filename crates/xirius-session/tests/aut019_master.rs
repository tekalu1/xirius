//! AUT-019: the master's automation is as late as the audio at its stage. Behind a master insert
//! with latency, the master's lanes and the plug-ins after it (in its splitter's branches too)
//! are evaluated that much earlier on the song clock, so an export does not drift.

use std::{collections::BTreeMap, sync::Arc};
use xirius_core::{
    auto::{AutoLane, AutoPoint, AutoTarget},
    id::{ClipId, Node, NodeId, PluginId, TrackId},
    model::{Clip, ClipBody, Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind},
    rack::{Branch, Rack, RackNode, Split, SplitMode},
};
use xirius_engine::{Plan, ProcessSetup, RenderMode};
use xirius_plugins::{Catalog, probe};
use xirius_session::{
    automation_timing::Timing, build::Builder, export, graph::Select, plugins::Isolation,
    tempo::TempoMap,
};

fn catalog() -> (Arc<Catalog>, String) {
    xirius_test_vst3::fixture_marker();
    let dll = xirius_test_vst3::fixture_path(std::env::current_exe().unwrap().parent().unwrap());
    assert!(dll.exists(), "a DLL on Windows, a bundle on macOS");
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

/// The fixture effect, `latency` of 256 frames late.
fn fixture(id: u64, uid: &str, latency: f64) -> Plugin {
    Plugin {
        samples: Vec::new(),
        id: PluginId(id),
        name: format!("Test Latency {id}"),
        kind: PluginKind::Effect,
        format: "vst3".into(),
        uid: uid.into(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::from([("1".into(), latency)]),
        state: None,
        isolation: PluginIsolation::InProcess,
        sidechain: None,
        preset: None,
    }
}

fn builder(catalog: &Arc<Catalog>) -> Builder {
    let mut b = Builder::new(48000, None);
    b.set_catalog(Some(catalog.clone()));
    b.set_isolation(Isolation::InProcess);
    b
}

fn master(p: &mut Project) -> &mut Track {
    Arc::make_mut(p.tracks.get_mut(&TrackId::MASTER).unwrap())
}

#[test]
fn the_master_lanes_behind_a_late_master_insert_do_not_drift_in_an_export() {
    let (catalog, uid) = catalog();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("aut019-master-{}.wav", std::process::id()));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for i in 0..3200 {
        let v = if [750, 1740].contains(&i) {
            1.0f32
        } else {
            0.0
        };
        writer.write_sample(v).unwrap();
        writer.write_sample(v).unwrap();
    }
    writer.finalize().unwrap();

    let mut p = Project::new("AUT-019");
    let id = TrackId(100);
    let mut track = Track::new(id, TrackKind::Audio, "impulses");
    track.clips.push(Clip {
        id: ClipId(300),
        name: "impulses".into(),
        color: None,
        start: 0,
        length: 128,
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Audio {
            file: path.to_string_lossy().into_owned(),
            offset: 0,
            gain_db: 0.0,
            process: Default::default(),
        },
    });
    p.tracks.insert(id, Arc::new(track));
    p.root.push(Node::Track(id));
    // The master: 256 frames of insert, and a mute that lets go at tick 38 (frame 950). Heard
    // as the audio is, the impulse at 750 reaches the fader 256 frames late, before the mute
    // lets go there; the one at 1740 after it.
    let m = master(&mut p);
    m.inserts.push(fixture(200, &uid, 1.0));
    let mut lane = AutoLane::new(AutoTarget::Mute);
    lane.points = Arc::new(
        [(0, 1.0), (38, 0.0)]
            .into_iter()
            .map(|(tick, value)| {
                let mut pt = AutoPoint::new(tick, value);
                pt.step = true;
                pt
            })
            .collect(),
    );
    m.automation.push(lane);

    let out = export::render(&mut builder(&catalog), &p, Select::All, 0, 2200, 0).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert_eq!(out.left[750], 0.0, "the mute still holds the first impulse");
    assert_eq!(out.left[1740], 1.0, "the second is heard");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn the_master_plug_ins_after_a_late_insert_and_in_its_branches_are_that_late() {
    let (catalog, uid) = catalog();
    let mut p = Project::new("AUT-019");
    let branch = |id: u64, fx: &[u64]| Branch {
        id: NodeId(id),
        fx: fx.iter().map(|p| RackNode::Plugin(PluginId(*p))).collect(),
        gain_db: 0.0,
        pan: 0.0,
        mute: false,
    };
    // 201 (256 late), then a splitter whose first branch has 202 (256 late) before 203, and
    // 204 after the splitter.
    let m = master(&mut p);
    m.inserts = vec![
        fixture(201, &uid, 1.0),
        fixture(202, &uid, 1.0),
        fixture(203, &uid, 0.0),
        fixture(204, &uid, 0.0),
    ];
    m.rack = Some(Rack {
        source: None,
        fx: vec![
            RackNode::Plugin(PluginId(201)),
            RackNode::Split(Split {
                id: NodeId(20),
                mode: SplitMode::Normal,
                crossovers: Vec::new(),
                branches: vec![branch(21, &[202, 203]), branch(22, &[])],
            }),
            RackNode::Plugin(PluginId(204)),
        ],
    });

    let mut b = builder(&catalog);
    let built = b.build(&p, Select::All);
    assert!(built.warnings.is_empty(), "{:?}", built.warnings);
    let setup = ProcessSetup {
        sample_rate: 48000.0,
        max_block: 512,
        mode: RenderMode::Offline,
    };
    let plan = Plan::build(built.spec, setup, None).unwrap();
    assert_eq!(plan.latency(), 512);
    let shape = plan.shape();
    assert_eq!(shape.automation.master.input, 0);
    assert_eq!(shape.automation.master.fader(), 512);
    let timing = Timing::new(&p, &built.graph, &shape, b.plugins());
    let map = TempoMap::from_project(&p);
    let frame = 48_000;
    let heard_at = |id: u64| {
        let target = AutoTarget::Param(PluginId(id), "1".into());
        timing.tick(TrackId::MASTER, &target, frame, &map, 48000.0)
    };
    let tick = |late: u64| map.tick_of_frame(frame - late, 48000.0) as f64;
    assert_eq!(heard_at(201), tick(0));
    assert_eq!(heard_at(202), tick(256), "in the branch, after 201");
    assert_eq!(heard_at(203), tick(512), "in the branch, after 202 too");
    assert_eq!(
        heard_at(204),
        tick(512),
        "after the splitter's longest branch"
    );
    let fader = timing.tick(TrackId::MASTER, &AutoTarget::Volume, frame, &map, 48000.0);
    assert_eq!(fader, tick(512));
}
