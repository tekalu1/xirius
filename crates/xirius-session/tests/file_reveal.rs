//! `file.reveal_os` through a session (BRW-016): only dry runs, so no file window opens.

use std::sync::Arc;

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, Session};
use xirius_plugins::{Catalog, ScanOptions};
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::output::OutputMode;

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// A song in a folder of its own; the folder goes when the guard is dropped.
fn song(tag: &str) -> (tempfile::TempDir, Session) {
    let root = tempfile::Builder::new()
        .prefix(&format!("xirius-file-reveal-{tag}-"))
        .tempdir()
        .unwrap();
    let dir = root.path().join("song");
    drop(Session::create(&dir, Project::new("Reveal")).unwrap());
    let mut s = Session::open_with(&dir, shared_registry()).unwrap();
    commands::attach(
        &mut s,
        ExtConfig {
            output: OutputMode::None,
            scan: ScanOptions {
                dirs: vec![],
                cache: root.path().join("plugins.json"),
                ..ScanOptions::default()
            },
            catalog: Some(Arc::new(Catalog::default())),
        },
    );
    (root, s)
}

#[test]
fn the_command_is_in_the_table_and_a_dry_run_names_the_file_it_would_show() {
    let (root, mut s) = song("dry");
    let def = shared_registry()
        .get("file.reveal_os")
        .expect("file.reveal_os is in the command table")
        .summary;
    assert!(def.contains("エクスプローラー"), "{def}");

    let kick = root.path().join("song").join("kick.wav");
    std::fs::write(&kick, b"x").unwrap();
    let mut dry = RunOptions::human();
    dry.dry_run = true;
    let r = commands::run(
        &mut s,
        None,
        "file.reveal_os",
        &map(json!({ "path": "kick.wav" })),
        &dry,
    )
    .unwrap();
    let said = r.data["path"].as_str().unwrap().replace('\\', "/");
    assert!(said.ends_with("/song/kick.wav"), "{said}");

    let e = commands::run(
        &mut s,
        None,
        "file.reveal_os",
        &map(json!({ "path": "nothing.wav" })),
        &dry,
    )
    .unwrap_err();
    assert!(e.message.contains("nothing.wav"), "{}", e.message);
    drop(s);
    drop(root);
}
