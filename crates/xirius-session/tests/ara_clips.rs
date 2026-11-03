//! The song's audio clips played through an ARA plugin (EDT-023, PLG-032, PLG-036, PLG-037,
//! PLG-047, ARC-025, ENG-025), with the stand-in plugin of `xirius_ara::fake` registered as
//! `fake:ara`. One test: the stand-in's edits are global to the process.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use xirius_ara::fake::{self, Config};
use xirius_ara::service::{Placement, modification_id};
use xirius_core::id::ClipId;
use xirius_core::model::{Clip, Project};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::Catalog;
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, write_wav};
use xirius_session::ext::ExtConfig;
use xirius_session::graph::Select;
use xirius_session::tempo::TempoMap;

const RATE: u32 = 48_000;
const PLUGIN: &str = "fake:ara";

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        let p = std::env::temp_dir().join(format!("xirius-ara-clips-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn session(p: Project) -> Session {
    let mut s = Session::with_registry(p, shared_registry());
    commands::attach(
        &mut s,
        ExtConfig {
            catalog: Some(Arc::new(Catalog::default())),
            ..ExtConfig::default()
        },
    );
    s
}

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
    try_run(s, name, args.clone()).unwrap_or_else(|e| panic!("{name} {args}: {e}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The plugins' archives taken into the song as a frame of the window does, without waiting:
/// whether the song changed.
fn take(s: &mut Session, b: &Builder) -> bool {
    let started = Instant::now();
    let changed = xirius_session::ara::take_archives(s, b.ara(), false).unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(150),
        "a frame waited {:?} for the archive",
        started.elapsed()
    );
    changed
}

/// Whether an archive of the song's plugins asked for is on its way, as a frame of the window
/// asks while it writes the song: never waiting.
fn pending(b: &Builder, p: &Project) -> bool {
    let started = Instant::now();
    let on = b.ara().take_arrived(p);
    assert!(
        started.elapsed() < Duration::from_millis(150),
        "a frame waited {:?} for the archive",
        started.elapsed()
    );
    on
}

fn clip(p: &Project, id: &str) -> Clip {
    let id: ClipId = id.parse().unwrap();
    p.clip(id).unwrap().1.clone()
}

fn live(rate: u32, dir: &Path) -> Builder {
    let mut b = Builder::new(rate, Some(dir.to_path_buf()));
    b.set_ara_live(true);
    b
}

/// The clip's sound through the plugin, as the builder plays it.
fn through(b: &Builder, p: &Project, id: &str) -> bool {
    let c = clip(p, id);
    let xirius_core::model::ClipBody::Audio { offset, .. } = c.body else {
        panic!("not audio");
    };
    b.ara()
        .sound(p, &c, offset, &TempoMap::from_project(p), b.rate())
        .is_some()
}

/// The loudest sample of the song's first second, rendered by `b`.
fn peak(b: &mut Builder, p: &Project) -> f32 {
    let frames = u64::from(b.rate());
    let out = export::render(b, p, Select::All, 0, frames, 0).unwrap();
    out.left.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

#[test]
fn clips_play_through_the_plugin_and_its_edits_stay_with_the_song() {
    xirius_session::ara::register(
        PLUGIN,
        "Fake ARA",
        Arc::new(|| {
            fake::loader(Config {
                analysis_ms: 20,
                ..Config::default()
            })
        }),
    );
    let dir = Dir::new();
    let file = dir.0.join("a.wav");
    write_wav(
        &file,
        &vec![0.5; RATE as usize],
        &vec![0.5; RATE as usize],
        RATE,
        32,
    )
    .unwrap();

    // EDT-023: attached through the session's command, one step.
    let mut s = session(Project::new("song"));
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let a = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 1920, "file": "a.wav"}),
    ));
    assert!(
        try_run(
            &mut s,
            "clip.ara.attach",
            json!({"ids": [a], "plugin": "nope"})
        )
        .is_err()
    );
    let r = run(
        &mut s,
        "clip.ara.attach",
        json!({"ids": [a], "plugin": PLUGIN}),
    );
    assert_eq!(r.data["name"], "Fake ARA");
    let row = s.project().ara.clips[&a.parse::<ClipId>().unwrap()].clone();
    assert_eq!(
        (row.plugin.as_str(), row.name.as_str()),
        (PLUGIN, "Fake ARA")
    );

    // PLG-032: the clip plays its region, as it is at first.
    let mut player = live(RATE, &dir.0);
    let p = s.project().clone();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    until("the analysis", || through(&player, &p, &a));
    assert!((peak(&mut player, &p) - 0.5).abs() < 1e-3);
    // ARC-025: nothing changed in the plugin, nothing changes in the song.
    assert!(!take(&mut s, &player));

    // PLG-036: the plugin's editor opens on its ARA thread without the caller waiting, and the
    // report says so until its close box closes it.
    let key = a.parse::<ClipId>().unwrap().0;
    player
        .ara()
        .open_editor(PLUGIN, "Fake ARA", Placement::default(), &[key])
        .unwrap();
    until("the editor", || player.ara().editing() == [PLUGIN]);
    let report = &player.ara().reports()[0];
    assert!(report.editing);
    assert_eq!(report.phases, [(key, xirius_ara::Phase::Ready)]);
    assert!(player.ara().take_editor_errors().is_empty());
    assert!(
        player
            .ara()
            .open_editor("nope", "x", Placement::default(), &[key])
            .is_err()
    );
    fake::close_windows();
    until("the editor closed", || player.ara().editing().is_empty());

    // PLG-037: an edit in the plugin is heard and its archive goes into the song, in place.
    let steps = s.steps().len();
    fake::edit(&modification_id(a.parse::<ClipId>().unwrap().0), 0.5);
    until("the archive", || take(&mut s, &player));
    assert_eq!(s.steps().len(), steps, "the archive is not a step");
    let archive = s.project().ara.archives[PLUGIN].clone();
    assert_eq!(archive.version, xirius_core::ara::ARCHIVE_VERSION);
    let p = s.project().clone();
    until("the edited render", || {
        (peak(&mut player, &p) - 0.25).abs() < 1e-3
    });

    // PLG-036, ARC-025: the window's frames do not wait for the archive. The plugin writes it
    // on its own thread, and a later frame puts it in.
    let modification = modification_id(key);
    fake::slow_store(Duration::from_millis(300));
    fake::edit(&modification, 0.4);
    let mut waited = false;
    until("the archive written on the plugin's thread", || {
        let changed = take(&mut s, &player);
        waited |= player.ara().take_arrived(s.project());
        changed
    });
    assert!(waited, "the archive came in at the frame that asked for it");
    assert_ne!(s.project().ara.archives[PLUGIN], archive);
    let p = s.project().clone();
    until("the render of that edit", || {
        (peak(&mut player, &p) - 0.2).abs() < 1e-3
    });

    // An edit in the window, and the window closed before its archive came: the edit still
    // comes into the song. The archive is asked for as the song would be written
    // (`SessionHost::begin_pull_states`).
    player
        .ara()
        .open_editor(PLUGIN, "Fake ARA", Placement::default(), &[key])
        .unwrap();
    until("the editor", || player.ara().editing() == [PLUGIN]);
    fake::edit(&modification, 0.3);
    until("the archive asked for", || {
        player.ara().ask_archives(s.project());
        player.ara().take_arrived(s.project())
    });
    fake::close_windows();
    until("the editor closed", || player.ara().editing().is_empty());
    // Come or still on its way, the archive asked for goes into the song.
    until("the archive after the close", || take(&mut s, &player));
    assert!(!player.ara().take_arrived(s.project()));
    let p = s.project().clone();
    until("the render of the edit before the close", || {
        (peak(&mut player, &p) - 0.15).abs() < 1e-3
    });

    // Written or left, the window waits for the archive on its way over its frames, none of
    // which waits for it, and then writes the song with it.
    fake::edit(&modification, 0.5);
    until("the archive asked for", || {
        player.ara().ask_archives(s.project());
        player.ara().take_arrived(s.project())
    });
    until("the archive on its way", || !pending(&player, s.project()));
    assert!(take(&mut s, &player));
    assert_eq!(s.project().ara.archives[PLUGIN], archive);
    fake::slow_store(Duration::ZERO);

    // PLG-036: an edit the plugin has not told of yet (it tells at its next turn) is still in
    // the archive asked for as the song is written: asked at once, it comes with the edit.
    for gain in [0.7, 0.5] {
        fake::edit(&modification, gain);
        player.ara().ask_archives(s.project());
        assert!(
            pending(&player, s.project()),
            "the edit before its notice asked for nothing"
        );
        until("the archive on its way", || !pending(&player, s.project()));
        assert!(take(&mut s, &player), "the edit before its notice was lost");
    }
    assert_eq!(s.project().ara.archives[PLUGIN], archive);
    // Nothing changed since: asked again, nothing is written and the song stays as it is.
    player.ara().ask_archives(s.project());
    until("the answer", || !pending(&player, s.project()));
    assert!(!take(&mut s, &player));

    // PLG-036: moved, the clip keeps its render: no gap while the plugin renders it again.
    run(&mut s, "clip.move", json!({"ids": [a], "to": 960}));
    let p = s.project().clone();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    assert!(
        through(&player, &p, &a),
        "the moved clip fell back to its file"
    );
    // Trimmed, it keeps the render where it still covers it, with the plugin's edit.
    run(&mut s, "clip.trim", json!({"ids": [a], "by": 480}));
    let p = s.project().clone();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    assert!(
        through(&player, &p, &a),
        "the trimmed clip fell back to its file"
    );
    let v = peak(&mut player, &p);
    assert!(
        (v - 0.25).abs() < 1e-3,
        "the trimmed clip lost its edit: {v}"
    );

    // ENG-025: an export uses the player's document when it has the same song, at any rate
    // (the plugin renders at the file's, PLG-047).
    let mut export = Builder::new(RATE, Some(dir.0.clone()));
    assert!((peak(&mut export, &p) - 0.25).abs() < 1e-3);
    let mut other = Builder::new(44_100, Some(dir.0.clone()));
    let started = Instant::now();
    // Converted to 44.1 kHz, it rings a little at its edges.
    assert!((peak(&mut other, &p) - 0.25).abs() < 0.02);
    assert!(started.elapsed() < Duration::from_secs(10));
    drop((export, other));

    // PLG-047: the player at another rate plays the plugin's render with its edit at once,
    // converted from the file's rate.
    let mut player2 = live(44_100, &dir.0);
    player2.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    assert!(through(&player2, &p, &a), "the new rate lost the render");
    let v = peak(&mut player2, &p);
    assert!((v - 0.25).abs() < 0.02, "the new rate lost the edit: {v}");

    // PLG-036: an archive that has not come when the window writes the song (after
    // `STORE_WAIT`) is left out of that save, and comes in after it: the song is unsaved again,
    // and the next save has the edit.
    let folder = dir.0.join("song");
    s.save_as(&folder).unwrap();
    fake::slow_store(Duration::from_millis(300));
    fake::edit(&modification, 0.6);
    until("the archive asked for", || {
        player.ara().ask_archives(s.project());
        pending(&player, s.project())
    });
    assert!(!take(&mut s, &player));
    s.save().unwrap();
    assert!(!s.is_dirty());
    until("the archive after the save", || take(&mut s, &player));
    assert!(s.is_dirty(), "the late edit did not make the song unsaved");
    s.save().unwrap();
    fake::slow_store(Duration::ZERO);
    let edited = s.project().ara.archives[PLUGIN].clone();
    assert_ne!(edited, archive);
    drop((player, player2));

    // PLG-037: written and opened again, the song gives the plugin its edits back.
    let mut reopened = Session::open_with(&folder, shared_registry()).unwrap();
    commands::attach(
        &mut reopened,
        ExtConfig {
            catalog: Some(Arc::new(Catalog::default())),
            ..ExtConfig::default()
        },
    );
    assert_eq!(reopened.project().ara.archives[PLUGIN], edited);
    let opened = reopened.project().clone();
    let mut player = live(RATE, &dir.0);
    let p = reopened.project().clone();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    until("the restored render", || through(&player, &p, &a));
    let v = peak(&mut player, &p);
    assert!(
        (v - 0.3).abs() < 1e-3,
        "the reopened song lost the edit: {v}"
    );
    // ARC-025: opening and playing it leaves it as it was.
    assert!(!take(&mut reopened, &player));
    assert!(
        *reopened.project() == opened,
        "opening and playing changed the song"
    );

    // A copy plays its own region, cloned with the edits; a second attach takes the same plugin.
    let b = id_of(&run(&mut reopened, "clip.duplicate", json!({"ids": [a]})));
    let p = reopened.project().clone();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    until("the copy's render", || through(&player, &p, &b));

    // PLG-036: the plugin's last clips detached while it writes its archive, and brought back
    // at once (an undo): the document is kept until the archive has come, so the plugin's last
    // edit is still there.
    fake::slow_store(Duration::from_millis(300));
    fake::edit(&modification, 0.8);
    until("the archive asked for", || {
        player.ara().ask_archives(reopened.project());
        pending(&player, reopened.project())
    });
    run(&mut reopened, "clip.ara.detach", json!({"ids": [a, b]}));
    let p = reopened.project().clone();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    assert!(
        player.ara().doc(PLUGIN).is_some(),
        "the document went with its archive on its way"
    );
    reopened.undo(1).unwrap();
    let p = reopened.project().clone();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    // The plugin renders the edit after it wrote the archive.
    until("the render of the plugin's last edit", || {
        through(&player, &p, &a) && (peak(&mut player, &p) - 0.4).abs() < 1e-3
    });
    until("the archive", || take(&mut reopened, &player));
    fake::slow_store(Duration::ZERO);

    // PLG-036: detached after the plugin's last edit, its document is let go of once its last
    // archive came, and an undo opens it again with that edit, not the song's older archive;
    // back and forth (redo, undo), the edit stays. Leaving or writing the song does not wait
    // for the document let go of, though its archive takes a while.
    fake::slow_store(Duration::from_millis(300));
    fake::edit(&modification, 0.9);
    run(&mut reopened, "clip.ara.detach", json!({"ids": [a, b]}));
    let p = reopened.project().clone();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    until("the document let go of", || {
        assert!(
            !pending(&player, &p),
            "the song waits for a plugin it no longer uses"
        );
        player.ara().doc(PLUGIN).is_none()
    });
    fake::slow_store(Duration::ZERO);
    for round in 0..2 {
        reopened.undo(1).unwrap();
        let p = reopened.project().clone();
        player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
        until("the render brought back", || through(&player, &p, &a));
        let v = peak(&mut player, &p);
        assert!(
            (v - 0.45).abs() < 1e-3,
            "undo {round} lost the plugin's last edit: {v}"
        );
        // The song takes the edit back once, in place, so it is written with it.
        assert_eq!(take(&mut reopened, &player), round == 0);
        reopened.redo(1).unwrap();
        let p = reopened.project().clone();
        player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
        until("the document let go of", || {
            assert!(!pending(&player, &p));
            player.ara().doc(PLUGIN).is_none()
        });
    }
    reopened.undo(1).unwrap();
    let p = reopened.project().clone();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    until("the render brought back", || through(&player, &p, &a));
    assert!((peak(&mut player, &p) - 0.45).abs() < 1e-3);
    assert!(!take(&mut reopened, &player));

    // EDT-023: detached, the clip plays its file.
    run(&mut reopened, "clip.ara.detach", json!({"ids": [a, b]}));
    let p = reopened.project().clone();
    assert!(p.ara.clips.is_empty());
    assert!((peak(&mut player, &p) - 0.5).abs() < 1e-3);
    run(&mut reopened, "clip.ara.attach", json!({"ids": [a]}));
    assert_eq!(
        reopened.project().ara.clips[&a.parse::<ClipId>().unwrap()].plugin,
        PLUGIN
    );
}
