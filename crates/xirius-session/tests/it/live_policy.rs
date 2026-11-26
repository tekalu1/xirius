//! Session arm/monitor changes switch the active plan to synchronous rendering (ENG-016).
use std::sync::Arc;
use xirius_core::id::{ClipId, Node, TrackId};
use xirius_core::model::{Clip, ClipBody, Project, Track, TrackKind};
use xirius_engine::Command;
use xirius_session::host::{HostConfig, SessionHost};
#[test]
fn arming_or_monitoring_a_track_disables_its_lookahead_without_adding_latency() {
    let dir = tempfile::Builder::new()
        .prefix("xirius-live-policy-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("input.wav");
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for _ in 0..4096 {
        writer.write_sample(1.0f32).unwrap();
    }
    writer.finalize().unwrap();
    let id = TrackId(100);
    let mut t = Track::new(id, TrackKind::Audio, "live");
    t.clips.push(Clip {
        id: ClipId(200),
        name: "audio".into(),
        color: None,
        start: 0,
        length: 400,
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
    let mut p = Project::new("live");
    p.tracks.insert(id, Arc::new(t));
    p.root.push(Node::Track(id));
    let (mut host, mut audio, warnings) = SessionHost::start(&p, HostConfig::default()).unwrap();
    assert!(warnings.is_empty());
    host.controller().send(Command::Play).ok().unwrap();
    let mut out = [0.0; 128];
    audio.render(&mut out, 2);
    host.poll();
    assert_eq!(audio.lookahead(0).unwrap().cached_blocks, 2);
    let baseline = audio.latency();
    for (armed, monitor) in [(true, false), (false, true)] {
        let t = Arc::make_mut(p.tracks.get_mut(&id).unwrap());
        t.armed = armed;
        t.monitor = monitor;
        host.sync(&p);
        audio.render(&mut out, 2);
        host.poll();
        let before = audio.lookahead(0).unwrap().rendered_frames;
        audio.render(&mut out, 2);
        host.poll();
        assert_eq!(audio.lookahead(0).unwrap().cached_blocks, 0);
        assert_eq!(audio.lookahead(0).unwrap().rendered_frames, before);
        assert_eq!(audio.latency(), baseline);
        assert!(out.iter().all(|v| *v == 1.0));
    }
    Arc::make_mut(p.tracks.get_mut(&id).unwrap()).monitor = false;
    host.sync(&p);
    audio.render(&mut out, 2);
    assert_eq!(audio.lookahead(0).unwrap().cached_blocks, 2);
    drop(audio);
    drop(host);
    std::fs::remove_file(path).unwrap();
}
