//! PLG-067, PLG-068: `plugin.samples.load` reads a folder's audio files into a Drum Kit (the
//! dry run tells the pads it would fill), and `plugin.samples.step` goes round a file's folder,
//! each one step of the history.
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_core::session::{RunOptions, Session};
use xirius_session::export::write_wav;
use xirius_session::{commands, ext::ExtConfig, output::OutputMode};

fn session(dir: &Path, uid: &str) -> Session {
    let mut p = Project::new("s");
    let mut t = Track::new(TrackId(1), TrackKind::Instrument, "Drums");
    t.inserts.push(Plugin {
        id: PluginId(2),
        name: "Drum Kit".into(),
        kind: PluginKind::Instrument,
        format: "builtin".into(),
        uid: uid.into(),
        bypass: false,
        deactivated: false,
        params: Default::default(),
        state: None,
        sidechain: None,
        preset: None,
        samples: Vec::new(),
        isolation: PluginIsolation::default(),
    });
    p.tracks.insert(TrackId(1), Arc::new(t));
    p.root.push(Node::Track(TrackId(1)));
    let mut s = Session::with_registry(p, commands::shared_registry());
    commands::attach(
        &mut s,
        ExtConfig {
            output: OutputMode::None,
            catalog: Some(Arc::default()),
            scan: xirius_plugins::ScanOptions {
                dirs: vec![],
                cache: dir.join("plugins.json"),
                ..Default::default()
            },
        },
    );
    s
}

fn run(s: &mut Session, name: &str, args: Value, dry: bool) -> Value {
    let opts = RunOptions {
        dry_run: dry,
        ..RunOptions::human()
    };
    s.run_value(name, args, &opts)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn leaves(s: &Session) -> Vec<String> {
    s.project().tracks[&TrackId(1)].inserts[0]
        .samples
        .iter()
        .map(|f| {
            Path::new(f)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn a_folder_fills_a_kits_pads_and_steps_go_round_its_files() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let dir = tmp.path().join("kit");
    std::fs::create_dir_all(&dir).unwrap();
    let x = vec![0.0f32; 64];
    for n in ["kick_v1.wav", "kick_v2.wav", "snare.wav", "hat.wav"] {
        write_wav(&dir.join(n), &x, &x, 48_000, 16).unwrap();
    }
    std::fs::write(dir.join("notes.txt"), "-").unwrap();
    let mut s = session(tmp.path(), "drum-kit");
    let path = dir.display().to_string();

    let dry = run(
        &mut s,
        "plugin.samples.load",
        json!({ "id": "plg_2", "path": path }),
        true,
    );
    assert_eq!(dry["pads"], json!(["A1", "A2", "A3"]));
    assert!(leaves(&s).is_empty());

    run(
        &mut s,
        "plugin.samples.load",
        json!({ "id": "plg_2", "path": path }),
        false,
    );
    assert_eq!(
        leaves(&s),
        ["hat.wav", "kick_v1.wav", "kick_v2.wav", "snare.wav"]
    );
    assert!(s.can_undo());

    // After the last file comes the first; back from there is the last again.
    run(
        &mut s,
        "plugin.samples.step",
        json!({ "id": "plg_2", "index": 4 }),
        false,
    );
    assert_eq!(leaves(&s)[3], "hat.wav");
    run(
        &mut s,
        "plugin.samples.step",
        json!({ "id": "plg_2", "index": 4, "by": -1 }),
        false,
    );
    assert_eq!(leaves(&s)[3], "snare.wav");
    run(&mut s, "history.undo", json!({}), false);
    assert_eq!(leaves(&s)[3], "hat.wav");
    run(&mut s, "history.undo", json!({}), false);
    run(&mut s, "history.undo", json!({}), false);
    assert!(leaves(&s).is_empty());

    // A file that is not a sound is refused.
    let e = s
        .run_value(
            "plugin.samples.load",
            json!({ "id": "plg_2", "path": dir.join("notes.txt").display().to_string() }),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert!(format!("{e:?}").contains("音声ファイル"), "{e:?}");

    // N-smp-5: among sounds it is passed over and named; the sounds go in.
    let list: Vec<Value> = ["kick_v1.wav", "notes.txt", "snare.wav"]
        .iter()
        .map(|n| json!({ "path": dir.join(n).display().to_string() }))
        .collect();
    let out = run(
        &mut s,
        "plugin.samples.load",
        json!({ "id": "plg_2", "paths": list }),
        false,
    );
    assert_eq!(out["skipped"], json!(["notes.txt"]));
    assert_eq!(leaves(&s), ["kick_v1.wav", "snare.wav"]);
}
