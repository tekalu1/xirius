//! The selection defines time bounds while the mix retains every audible track.
use serde_json::{Value, json};
use xirius_core::{Session, model::Project, session::RunOptions};
use xirius_session::{commands, ext::ExtConfig, output::OutputMode};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run(name, args.as_object().unwrap(), &RunOptions::human())
        .unwrap()
        .data
}

#[test]
fn selected_clip_bounds_exclude_distant_clips_and_survive_reopening_the_export() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/export-selection-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.wav");
    let mut wav = hound::WavWriter::create(
        &source,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for _ in 0..288000 {
        wav.write_sample(0.2f32).unwrap();
    }
    wav.finalize().unwrap();
    let mut s = Session::with_registry(Project::new("selection"), commands::shared_registry());
    commands::attach(
        &mut s,
        ExtConfig {
            output: OutputMode::None,
            catalog: Some(std::sync::Arc::default()),
            ..ExtConfig::default()
        },
    );
    s.save_as(dir.join("song")).unwrap();
    let track = run(&mut s, "track.add.audio", json!({"name":"source"}))["id"].clone();
    let first = run(
        &mut s,
        "clip.add.audio",
        json!({"track":track,"at":1920,"length":1920,"file":source}),
    )["id"]
        .clone();
    let last = run(
        &mut s,
        "clip.add.audio",
        json!({"track":track,"at":4800,"length":1920,"file":source}),
    )["id"]
        .clone();
    run(
        &mut s,
        "clip.add.audio",
        json!({"track":track,"at":19200,"length":1920,"file":source}),
    );
    let result = run(
        &mut s,
        "export.selection",
        json!({"ids":[first,last],"out":dir.join("chosen.wav")}),
    );
    let written = hound::WavReader::open(result["path"].as_str().unwrap()).unwrap();
    assert_eq!(written.duration(), 120000);
    let record = run(&mut s, "export.status", json!({"job":result["record"]}));
    assert_eq!(record["args"]["range"], "custom");
    assert_eq!(record["args"]["from"], 1920);
    assert_eq!(record["args"]["to"], 6720);
    assert_eq!(
        run(&mut s, "export.open", json!({"job":result["record"]}))["args"],
        record["args"]
    );
    for ids in [json!([]), json!(["c999999"])] {
        assert!(
            s.run(
                "export.selection",
                json!({"ids":ids}).as_object().unwrap(),
                &RunOptions::human()
            )
            .is_err()
        );
    }
    drop(written);
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}
