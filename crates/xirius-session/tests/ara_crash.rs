// Deliberately raises an access violation; recovery uses Windows SEH.
#![cfg(target_os = "windows")]

//! An ARA plugin that faults (PLG-058): the app's crash filter hands the fault to the ARA
//! containment first, the plugin's thread ends, the app goes on, and the clips play their files
//! until the plugin is opened again. Its own process: the fault is process-wide state.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use xirius_ara::fake::{self, Config};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::Catalog;
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, write_wav};
use xirius_session::ext::ExtConfig;
use xirius_session::graph::Select;
use xirius_session::tempo::TempoMap;

const RATE: u32 = 48_000;
const PLUGIN: &str = "fake:faulting";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    let Value::Object(m) = args else {
        unreachable!()
    };
    commands::run(s, None, name, &m, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {}", e.message))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap()
        .to_string()
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn peak(b: &mut Builder, p: &Project) -> f32 {
    let frames = u64::from(b.rate());
    let out = export::render(b, p, Select::All, 0, frames, 0).unwrap();
    out.left.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

#[test]
fn a_faulting_plugin_leaves_the_app_and_the_song_playing() {
    let root = std::env::temp_dir().join(format!("xirius-ara-crash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let dir = Dir(root);
    // As the app starts: its crash report filter, then the plugin.
    xirius_engine::crash::install_at(&dir.0.join("crashes"), "ara-crash-test", true).unwrap();

    // Faults until it is mended, as a plugin updated after a crash.
    static MENDED: AtomicBool = AtomicBool::new(false);
    xirius_session::ara::register(
        PLUGIN,
        "Faulting ARA",
        Arc::new(|| {
            fake::loader(Config {
                analysis_ms: 5,
                fault_on_analysis: !MENDED.load(Ordering::Acquire),
                ..Config::default()
            })
        }),
    );
    let file = dir.0.join("a.wav");
    write_wav(
        &file,
        &vec![0.5; RATE as usize],
        &vec![0.5; RATE as usize],
        RATE,
        32,
    )
    .unwrap();

    let mut s = Session::with_registry(Project::new("song"), shared_registry());
    commands::attach(
        &mut s,
        ExtConfig {
            catalog: Some(Arc::new(Catalog::default())),
            ..ExtConfig::default()
        },
    );
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let a = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 1920, "file": "a.wav"}),
    ));
    run(
        &mut s,
        "clip.ara.attach",
        json!({"ids": [a], "plugin": PLUGIN}),
    );
    let p = s.project().clone();
    let tmap = TempoMap::from_project(&p);

    let mut player = Builder::new(RATE, Some(dir.0.clone()));
    player.set_ara_live(true);
    player.sync_ara(&p, &tmap, &mut Vec::new());
    until("the fault", || {
        player.ara().reports().iter().any(|r| r.crashed.is_some())
    });
    // The app is still here, and the clip plays its file.
    let r = &player.ara().reports()[0];
    assert_eq!((r.plugin.as_str(), r.clips), (PLUGIN, 1));
    assert!(xirius_ara::contain::faults() >= 1);
    assert!((peak(&mut player, &p) - 0.5).abs() < 1e-3);
    // An export does not wait for a plugin that is down.
    let started = Instant::now();
    let mut export = Builder::new(RATE, Some(dir.0.clone()));
    assert!((peak(&mut export, &p) - 0.5).abs() < 1e-3);
    assert!(started.elapsed() < Duration::from_secs(5));
    drop(export);
    // Nothing of the plugin's goes into the song.
    assert!(!xirius_session::ara::take_archives(&mut s, player.ara(), false).unwrap());

    // plugin.restart: the document that crashed goes, and the plugin is opened again.
    MENDED.store(true, Ordering::Release);
    assert_eq!(player.ara().restart(None), 1);
    assert!(player.ara().reports().is_empty());
    player.sync_ara(&p, &tmap, &mut Vec::new());
    until("the plugin again", || {
        player
            .ara()
            .reports()
            .first()
            .is_some_and(|r| r.crashed.is_none() && r.loaded && r.working == 0)
    });
    assert!((peak(&mut player, &p) - 0.5).abs() < 1e-3);
}
