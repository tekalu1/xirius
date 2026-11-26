//! The mixdown report identifies muted tracks alongside the actual offline result.
use std::sync::Arc;
use xirius_core::id::{ClipId, Node, TrackId};
use xirius_core::model::{Clip, ClipBody, Project, Track, TrackKind};
use xirius_session::export::{self, Options, RangeSpec};

#[test]
fn a_mixdown_reports_muted_tracks_and_leaves_their_audio_out() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-muted-")
        .tempdir()
        .unwrap();
    let dir = tmp.path().to_path_buf();
    let input = dir.join("input.wav");
    let output = dir.join("mix.wav");
    let mut wav = hound::WavWriter::create(
        &input,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for _ in 0..48_000 * 2 {
        wav.write_sample(0.125f32).unwrap();
    }
    wav.finalize().unwrap();
    let mut project = Project::new("mute");
    for (n, muted, name) in [(1, false, "聞こえる音"), (2, true, "ミュートした音")] {
        let id = TrackId(n);
        let mut track = Track::new(id, TrackKind::Audio, name);
        track.mute = muted;
        track.clips.push(Clip {
            id: ClipId(n + 10),
            name: name.into(),
            color: None,
            take: None,
            controllers: Vec::new(),
            start: 0,
            length: 1920,
            muted: false,
            body: ClipBody::Audio {
                file: input.to_string_lossy().into_owned(),
                offset: 0,
                gain_db: 0.0,
                process: Default::default(),
            },
        });
        project.tracks.insert(id, Arc::new(track));
        project.root.push(Node::Track(id));
    }
    let report = export::mixdown(
        &project,
        &output,
        &Options {
            range: RangeSpec::Ticks(0, 960),
            bit_depth: Some(32),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(report.muted_tracks, ["ミュートした音"]);
    assert_eq!(
        report.to_json()["muted_tracks"],
        serde_json::json!(["ミュートした音"])
    );
    assert!(report.summary().contains("ミュートした音"));
    let mut result = hound::WavReader::open(&output).unwrap();
    assert!(result.samples::<f32>().all(|s| s.unwrap() == 0.125));
    drop(result);
    std::fs::remove_dir_all(dir).unwrap();
}
