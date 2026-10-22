//! A Save As changes the base of relative audio paths without replacing the engine.
use serde_json::json;
use xirius_core::session::{RunOptions, Session};
use xirius_engine::Command;
use xirius_session::host::{HostConfig, SessionHost};

#[test]
fn relocating_reloads_relative_audio_without_swapping_the_mix() {
    let root = tempfile::tempdir().unwrap();
    for (name, value) in [("old", 0.1f32), ("new", 0.6f32)] {
        let dir = root.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        let mut writer = hound::WavWriter::create(
            dir.join("take.wav"),
            hound::WavSpec {
                channels: 1,
                sample_rate: 48000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for _ in 0..48000 {
            writer.write_sample(value).unwrap();
        }
        writer.finalize().unwrap();
    }
    let mut s = Session::new("relocate");
    let t = s
        .run_value(
            "track.add.audio",
            json!({"name":"Take"}),
            &RunOptions::human(),
        )
        .unwrap();
    s.run_value(
        "clip.add.audio",
        json!({
            "track":t.data["id"], "file":"take.wav", "at":0, "bars":1
        }),
        &RunOptions::human(),
    )
    .unwrap();
    let (mut host, mut audio, warnings) = SessionHost::start(
        s.project(),
        HostConfig {
            dir: Some(root.path().join("old")),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    host.controller().send(Command::Play).ok().unwrap();
    let mut out = [0.0f32; 1024];
    for _ in 0..4 {
        audio.render(&mut out, 2);
    }
    let peak = |out: &[f32]| out.iter().fold(0.0f32, |peak, x| peak.max(x.abs()));
    assert!((peak(&out) - 0.1).abs() < 0.001);
    host.relocate(Some(root.path().join("new")));
    let report = host.sync(s.project());
    assert!(!report.swapped);
    assert_eq!(report.deferred, 0);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    host.seek_frame(0);
    for _ in 0..4 {
        audio.render(&mut out, 2);
    }
    assert!(
        (peak(&out) - 0.6).abs() < 0.001,
        "new file must play: {}",
        peak(&out)
    );
    host.relocate(Some(root.path().join("new")));
    assert_eq!(
        host.sync(s.project()).commands,
        0,
        "unchanged base needs no reload"
    );
}
