//! When the MP3 encoder cannot be loaded (EXP-005, LIC-006): the panel is told why, an MP3
//! export is refused before it starts, and WAV and FLAC are written as ever.
//!
//! Its own test binary, because it points the encoder's path (an environment variable) at a
//! file that is not there.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::{Catalog, ScanOptions};
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::mp3;
use xirius_session::output::OutputMode;

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, String> {
    commands::run(s, None, name, &map(args), &RunOptions::human()).map_err(|e| e.message)
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    try_run(s, name, args).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn song(tmp: &Path) -> Session {
    let mut s = Session::with_registry(Project::new("missing"), shared_registry());
    commands::attach(
        &mut s,
        ExtConfig {
            output: OutputMode::None,
            scan: ScanOptions {
                dirs: vec![],
                cache: tmp.join("plugins.json"),
                ..ScanOptions::default()
            },
            catalog: Some(Arc::new(Catalog::default())),
        },
    );
    let t = id_of(&run(&mut s, "track.add.instrument", json!({ "name": "A" })));
    let c = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": 1 }),
    ));
    run(
        &mut s,
        "note.add",
        json!({ "clip": c, "at": 0, "pitch": 60, "length": 3840 }),
    );
    s
}

#[test]
fn the_panel_is_told_and_wav_and_flac_still_export() {
    let scratch = tempfile::Builder::new()
        .prefix("xirius-export-mp3-missing-")
        .tempdir()
        .unwrap();
    let tmp: PathBuf = scratch.path().to_path_buf();
    let missing = tmp.join("nowhere").join(mp3::LAME_FILE);
    // SAFETY: the only test in this binary, set before anything else runs or reads it.
    unsafe { std::env::set_var(mp3::LAME_ENV, &missing) };
    assert_eq!(mp3::locate(), missing);

    let mut s = song(&tmp);
    let open = run(&mut s, "export.open", json!({}));
    let st = &open.data["mp3"];
    assert_eq!(st["available"], false, "{st}");
    assert_eq!(st["error"], mp3::CANNOT_LOAD);
    assert_eq!(st["path"], missing.display().to_string());
    assert!(
        st["why"]
            .as_str()
            .is_some_and(|w| w.contains(mp3::LAME_FILE)),
        "{st}"
    );

    let out = tmp.join("m.mp3");
    let e = try_run(
        &mut s,
        "export.mixdown",
        json!({ "out": out.display().to_string() }),
    )
    .unwrap_err();
    assert!(e.starts_with(mp3::CANNOT_LOAD), "{e}");
    assert!(!out.exists());
    let jobs = run(&mut s, "export.status", json!({})).data["jobs"].clone();
    assert!(
        jobs.as_array().is_none_or(Vec::is_empty),
        "a refused MP3 leaves no record: {jobs}"
    );

    for ext in ["wav", "flac"] {
        let out = tmp.join(format!("m.{ext}"));
        let r = run(
            &mut s,
            "export.mixdown",
            json!({ "out": out.display().to_string() }),
        );
        assert_eq!(r.data["format"], ext);
        assert!(out.exists(), "{ext}");
    }
}
