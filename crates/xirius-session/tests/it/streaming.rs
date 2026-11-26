//! Large native-rate WAVs use the streaming timeline through the real session builder.
use std::sync::Arc;
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::{Clip, ClipBody, Project, Track, TrackKind};
use xirius_engine::{
    Command, Plan, ProcessSetup, RenderMode,
    engine::{self, EngineConfig},
};
use xirius_session::{build::Builder, graph::Select, tempo::TempoMap};

#[test]
fn a_large_wav_is_streamed_by_the_session_and_seeks_without_changing_its_samples() {
    let dir = tempfile::Builder::new()
        .prefix("xirius-session-stream-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("stream.wav");
    let frames = 2_200_000;
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    let sample = |f: usize| (f % 8191) as f32 / 16384.0;
    for i in 0..frames {
        writer.write_sample(sample(i)).unwrap();
        writer.write_sample(-sample(i)).unwrap();
    }
    writer.finalize().unwrap();
    let mut p = Project::new("stream");
    let id = TrackId(100);
    let mut track = Track::new(id, TrackKind::Audio, "large file");
    track.clips.push(Clip {
        id: ClipId(101),
        name: "large".into(),
        color: None,
        take: None,
        controllers: Vec::new(),
        start: 0,
        length: 88_000,
        muted: false,
        body: ClipBody::Audio {
            file: path.to_string_lossy().into_owned(),
            offset: 0,
            gain_db: 0.0,
            process: Default::default(),
        },
    });
    p.tracks.insert(id, Arc::new(track));
    p.root.push(xirius_core::id::Node::Track(id));
    let mut builder = Builder::new(48_000, None);
    let timeline = builder.timeline(
        &p,
        &p.tracks[&id],
        &TempoMap::from_project(&p),
        &mut Vec::new(),
    );
    assert!(
        timeline.regions().is_empty(),
        "no whole-file decoded allocation"
    );
    assert_eq!(timeline.streams().len(), 1);
    drop(timeline);
    let built = builder.build(&p, Select::All);
    assert!(built.warnings.is_empty(), "{:?}", built.warnings);
    let plan = Plan::build(
        built.spec,
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 512,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    let (mut audio, mut controller) = engine::create(plan, EngineConfig::default());
    controller.send(Command::Seek(123_456)).ok().unwrap();
    controller.send(Command::Play).ok().unwrap();
    let mut out = [0.0; 4096 * 2];
    audio.render(&mut out, 2);
    assert_eq!(
        audio.lookahead(0).unwrap().cached_blocks,
        0,
        "stream arrivals are never cached as missing samples"
    );
    for (i, frame) in out.as_chunks::<2>().0.iter().enumerate() {
        assert_eq!(frame[0], sample(123_456 + i));
        assert_eq!(frame[1], -frame[0]);
    }
    drop(audio);
    drop(controller);
    drop(builder);
    std::fs::remove_file(path).unwrap();
}
