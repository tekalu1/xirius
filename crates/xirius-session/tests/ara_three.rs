//! ARA loops and analysis-only saves through the public session path; no audio device or UI.
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_ara::{Phase, fake};
use xirius_core::id::ClipId;
use xirius_core::model::{ClipBody, Project};
use xirius_core::session::{RunOptions, Session};
use xirius_plugins::Catalog;
use xirius_session::build::Builder;
use xirius_session::ext::ExtConfig;
use xirius_session::graph::Select;
use xirius_session::tempo::TempoMap;
use xirius_session::{ara, commands, export};
const UID: &str = "fake:ara-three";
const RATE: u32 = 48000;
const WAIT: Duration = Duration::from_secs(15);
fn run(s: &mut Session, name: &str, args: Value) -> Value {
    commands::run(
        s,
        None,
        name,
        args.as_object().unwrap(),
        &RunOptions::human(),
    )
    .unwrap_or_else(|error| panic!("{name} {args}: {error:?}"))
    .data
}
fn id(v: Value) -> String {
    v["id"]
        .as_str()
        .or_else(|| v["ids"][0].as_str())
        .unwrap()
        .into()
}
fn attach(s: &mut Session) {
    commands::attach(
        s,
        ExtConfig {
            catalog: Some(Arc::new(Catalog::default())),
            ..ExtConfig::default()
        },
    );
}
fn sync(b: &mut Builder, s: &Session) {
    let p = s.project();
    b.sync_ara(p, &TempoMap::from_project(p), &mut Vec::new());
    let deadline = Instant::now() + WAIT;
    loop {
        let doc = b.ara().doc(UID).unwrap();
        let idle = doc.service().wait_idle(deadline);
        if idle
            && p.ara
                .clips
                .iter()
                .filter(|(_, row)| row.plugin == UID)
                .all(|(key, _)| {
                    doc.service()
                        .state(key.0)
                        .is_some_and(|s| s.phase == Phase::Ready)
                })
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the source load/analysis did not complete"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn closed() {
    let deadline = Instant::now() + WAIT;
    while fake::live_documents() != 0 {
        assert!(Instant::now() < deadline, "the document did not close");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn analysis_save_reopen_and_edited_trimmed_loops() {
    ara::register(
        UID,
        "Fake ARA",
        Arc::new(|| fake::loader(fake::Config::default())),
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("source.wav");
    let samples: Vec<f32> = (0..RATE * 4)
        .map(|i| 0.1 + (i / 25) as f32 / 40000.0)
        .collect();
    export::write_wav(&file, &samples, &samples, RATE, 32).unwrap();
    let mut s = Session::with_registry(Project::new("ara-three"), commands::shared_registry());
    attach(&mut s);
    let track = id(run(&mut s, "track.add.audio", json!({})));
    let clip = id(run(
        &mut s,
        "clip.add.audio",
        json!({"track":track,"at":0,"length":7680,"file":file}),
    ));
    run(
        &mut s,
        "clip.ara.attach",
        json!({"ids":[clip],"plugin":UID}),
    );
    let key = clip.parse::<ClipId>().unwrap().0;
    let folder = dir.path().join("song");
    s.save_as(&folder).unwrap();
    let mut b = Builder::new(RATE, Some(dir.path().into()));
    b.set_ara_live(true);
    sync(&mut b, &s);
    assert!(!ara::take_archives(&mut s, b.ara(), true).unwrap());
    assert!(!s.is_dirty());
    assert!(s.project().ara.archives.is_empty());
    s.save().unwrap();
    assert!(s.project().ara.archives.contains_key(UID));
    assert!(!s.is_dirty());
    let service = b.ara().doc(UID).unwrap();
    drop((service, b, s));
    closed();
    let mut s = Session::open_with(&folder, commands::shared_registry()).unwrap();
    attach(&mut s);
    let mut b = Builder::new(RATE, Some(dir.path().into()));
    b.set_ara_live(true);
    sync(&mut b, &s);
    let probe = b.ara().doc(UID).unwrap().service().probe(WAIT).unwrap();
    assert!(probe.analysis_queries > 0);
    assert_eq!(probe.analysis_requests, 0);
    assert_eq!(probe.analysis_starts, 0);
    assert!(!ara::take_archives(&mut s, b.ara(), true).unwrap());
    assert!(!s.is_dirty());
    // Every pass must play the plugin's edit, with a head trim, slip and partial final pass.
    fake::edit(&xirius_ara::service::modification_id(key), 0.5);
    let deadline = Instant::now() + WAIT;
    while !ara::take_archives(&mut s, b.ara(), false).unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    run(
        &mut s,
        "clip.loop.set",
        json!({"ids":[clip],"start":480,"length":960}),
    );
    run(&mut s, "clip.slip", json!({"ids":[clip],"by":-240}));
    run(&mut s, "clip.trim", json!({"ids":[clip],"by":240}));
    run(&mut s, "clip.resize", json!({"ids":[clip],"by":-137}));
    for length in [None, Some(480), Some(1920)] {
        if let Some(length) = length {
            run(
                &mut s,
                "clip.loop.set",
                json!({"ids":[clip],"start":480,"length":length}),
            );
        }
        sync(&mut b, &s);
        let c = s.project().clip(ClipId(key)).unwrap().1;
        let ClipBody::Audio { offset, .. } = c.body else {
            unreachable!()
        };
        let parts = xirius_core::clipfx::pieces(s.project().clip_loop(c.id), c.length);
        let service = b.ara().doc(UID).unwrap();
        let probe = service.service().probe(WAIT).unwrap();
        assert_eq!(probe.regions, parts.len());
        assert_eq!(probe.preparations, 1);
        let state = service.service().state(key).unwrap();
        assert_eq!(state.phase, Phase::Ready);
        let output =
            export::render(&mut b, s.project(), Select::All, 0, u64::from(RATE) * 5, 0).unwrap();
        let c = s.project().clip(ClipId(key)).unwrap().1;
        for (at, from, len) in parts {
            // The engine applies its normal clip-edge fades after the ARA render.
            for tick in (32..len - 32).step_by(97) {
                let out = ((c.start + at + tick) * 25 + 12) as usize;
                let source = ((offset + from + tick) * 25 + 12) as usize;
                let expected = samples[source] * 0.5;
                assert!(
                    (output.left[out] - expected).abs() < 1e-4,
                    "tick {at}+{tick}: {} vs {expected}",
                    output.left[out]
                );
            }
        }
    }
    s.undo(1).unwrap();
    sync(&mut b, &s);
    let c = s.project().clip(ClipId(key)).unwrap().1;
    let count = xirius_core::clipfx::pieces(s.project().clip_loop(c.id), c.length).len();
    assert_eq!(
        b.ara()
            .doc(UID)
            .unwrap()
            .service()
            .probe(WAIT)
            .unwrap()
            .regions,
        count
    );
    drop(b);
    closed();
    // Same path and rate, changed content: the saved cache must be rejected.
    let changed = vec![0.05; RATE as usize * 4];
    export::write_wav(&file, &changed, &changed, RATE, 32).unwrap();
    let s = Session::open_with(&folder, commands::shared_registry()).unwrap();
    let mut b = Builder::new(RATE, Some(dir.path().into()));
    b.set_ara_live(true);
    sync(&mut b, &s);
    let probe = b.ara().doc(UID).unwrap().service().probe(WAIT).unwrap();
    assert_eq!(probe.analysis_requests, 1);
    assert_eq!(probe.analysis_starts, 1);
    drop(b);
    closed();
    assert!(fake::violations().is_empty(), "{:?}", fake::violations());
}
