//! MIX-049: a channel taken as a key in the output mode is a trigger. It keys the plug-in with
//! its whole output but is not heard at the master; a send-mode source still is. A stem of the
//! trigger itself still hears it.

use std::collections::BTreeMap;
use std::sync::Arc;

use xirius_core::id::{ClipId, Node, PluginId, SendId, TrackId};
use xirius_core::model::{
    Clip, ClipBody, KeyMode, KeySource, Plugin, PluginIsolation, PluginKind, Project, Sidechain,
    Track, TrackKind,
};
use xirius_engine::{KeyFrom, Output, TrackSlot};
use xirius_session::build::Builder;
use xirius_session::export;
use xirius_session::graph::Select;

fn gain_keyed_by(id: u64, src: TrackId, mode: KeyMode) -> Plugin {
    Plugin {
        id: PluginId(id),
        name: "Gain".into(),
        kind: PluginKind::Effect,
        format: "builtin".into(),
        uid: "gain".into(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::new(),
        state: None,
        samples: Vec::new(),
        isolation: PluginIsolation::Dedicated,
        sidechain: Some(Sidechain {
            on: true,
            sources: vec![KeySource {
                id: SendId(id + 1),
                track: src,
                mode,
            }],
        }),
        preset: None,
    }
}

/// A trigger track playing impulses, and a silent track whose plug-in it keys.
fn song(mode: KeyMode, wav: &std::path::Path) -> (Project, TrackId) {
    let mut p = Project::new("MIX-049");
    let kick = TrackId(1);
    let mut t = Track::new(kick, TrackKind::Audio, "kick");
    t.clips.push(Clip {
        id: ClipId(300),
        name: "kick".into(),
        color: None,
        start: 0,
        length: 128,
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Audio {
            file: wav.to_string_lossy().into_owned(),
            offset: 0,
            gain_db: 0.0,
            process: Default::default(),
        },
    });
    p.tracks.insert(kick, Arc::new(t));
    p.root.push(Node::Track(kick));
    let mut r = Track::new(TrackId(2), TrackKind::Audio, "bass");
    r.inserts.push(gain_keyed_by(20, kick, mode));
    p.tracks.insert(r.id, Arc::new(r));
    p.root.push(Node::Track(TrackId(2)));
    (p, kick)
}

fn impulses(name: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("mix049-{name}-{}.wav", std::process::id()));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut w = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for i in 0..2000 {
        let v = if i % 500 == 100 { 1.0f32 } else { 0.0 };
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
    path
}

#[test]
fn an_output_mode_source_keys_but_is_not_heard() {
    let wav = impulses("output");
    let (p, kick) = song(KeyMode::Output, &wav);

    let built = Builder::new(48_000, None).build(&p, Select::All);
    assert!(built.warnings.is_empty(), "{:?}", built.warnings);
    let TrackSlot::New(src) = &built.spec.tracks[0] else {
        panic!("the trigger was not built fresh");
    };
    assert_eq!(
        src.output,
        Output::Keys,
        "the trigger goes nowhere but its key"
    );
    let TrackSlot::New(r) = &built.spec.tracks[1] else {
        panic!("the receiver was not built fresh");
    };
    assert_eq!(r.keys[0].sources[0].from, KeyFrom::Track(0));
    assert_eq!(r.keys[0].sources[0].level, 1.0, "the output as it is");

    let out = export::render(&mut Builder::new(48_000, None), &p, Select::All, 0, 1800, 0).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert!(
        out.left.iter().chain(&out.right).all(|&v| v == 0.0),
        "nothing of the trigger reaches the master"
    );

    // Its own stem still hears it.
    let stem = export::render(
        &mut Builder::new(48_000, None),
        &p,
        Select::Track(kick),
        0,
        1800,
        0,
    )
    .unwrap();
    assert_eq!(stem.left[100], 1.0);
    std::fs::remove_file(wav).unwrap();
}

#[test]
fn a_send_mode_source_is_still_heard() {
    let wav = impulses("send");
    let (p, _) = song(
        KeyMode::Send {
            pre: false,
            level_db: 0.0,
            muted_level: None,
        },
        &wav,
    );
    let built = Builder::new(48_000, None).build(&p, Select::All);
    let TrackSlot::New(src) = &built.spec.tracks[0] else {
        panic!("the source was not built fresh");
    };
    assert_eq!(src.output, Output::Master);
    let out = export::render(&mut Builder::new(48_000, None), &p, Select::All, 0, 1800, 0).unwrap();
    assert_eq!(out.left[100], 1.0);
    std::fs::remove_file(wav).unwrap();
}
