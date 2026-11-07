//! A real ARA plugin through the song's clips (PLG-038, PLG-049): attach to an audio clip, the
//! analysis completes, the clip plays through the plugin, the song written and opened again
//! gives the plugin its archive back, and detach.
//!
//! Needs `XIRIUS_ARA_REAL_PLUGIN` set to the plugin's `.vst3` (any ARA plugin's; the
//! results are in `docs/ara-verification.md`) and `--ignored`; without the plugin it fails. The song is made of
//! tones written into a folder of its own under the temp folder, and opened again at another
//! rate.
//!
//! The plugin must not need anyone: a window of the plugin (an activation, a question) ends
//! the test at once, untouched, and the test says which window it was.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use xirius_ara::service::{Loader, source_id};
use xirius_ara::vst3::Vst3Binary;
use xirius_ara::{ClipSpec, Phase, Service, Song, SourceAudio};
use xirius_core::id::ClipId;
use xirius_core::model::{Clip, ClipBody, Project};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::Catalog;
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, write_wav};
use xirius_session::ext::ExtConfig;
use xirius_session::graph::Select;
use xirius_session::tempo::TempoMap;

const RATE: u32 = 48_000;
/// The song's rate when opened again.
const OTHER_RATE: u32 = 44_100;
const UID: &str = "real:ara";
const WAIT: Duration = Duration::from_secs(120);

fn plugin() -> PathBuf {
    let path = std::env::var_os("XIRIUS_ARA_REAL_PLUGIN")
        .map(PathBuf::from)
        .expect("XIRIUS_ARA_REAL_PLUGIN names no plugin");
    assert!(path.exists(), "XIRIUS_ARA_REAL_PLUGIN names no file");
    path
}

fn loader(path: &Path) -> Loader {
    let path = path.to_path_buf();
    Box::new(move || {
        Ok(Box::new(Vst3Binary::load(&path, None)?) as Box<dyn xirius_ara::service::Binary>)
    })
}

/// Ends the process when a window of it shows: a plugin asking for someone. Nothing is
/// clicked or typed; the window goes with the process.
#[cfg(windows)]
fn watch_windows() {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> i32 {
        let mut pid = 0u32;
        // SAFETY: `hwnd` comes from EnumWindows; `found` is the `Option<String>` below.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != std::process::id() || IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let mut class = [0u16; 256];
            let mut title = [0u16; 256];
            let c = GetClassNameW(hwnd, class.as_mut_ptr(), 256).max(0) as usize;
            let t = GetWindowTextW(hwnd, title.as_mut_ptr(), 256).max(0) as usize;
            *(found as *mut Option<String>) = Some(format!(
                "class {:?}, title {:?}",
                String::from_utf16_lossy(&class[..c]),
                String::from_utf16_lossy(&title[..t])
            ));
        }
        0
    }
    std::thread::spawn(|| {
        loop {
            let mut found: Option<String> = None;
            // SAFETY: the callback writes only `found`, which outlives the call.
            unsafe {
                EnumWindows(Some(each), &mut found as *mut Option<String> as LPARAM);
            }
            if let Some(w) = found {
                eprintln!("STOPPED: the plugin showed a window ({w}); left untouched");
                std::process::exit(3);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
}

#[cfg(not(windows))]
fn watch_windows() {}

/// A short tune: four tones with rests between, so the plugin finds notes.
fn tune(rate: f64, seconds: f64) -> Vec<f32> {
    let hz = [220.0, 246.94, 261.63, 293.66];
    (0..(rate * seconds) as usize)
        .map(|i| {
            let t = i as f64 / rate;
            if t.fract() < 0.6 {
                let f = hz[t as usize % hz.len()];
                0.4 * (t * f * std::f64::consts::TAU).sin() as f32
            } else {
                0.0
            }
        })
        .collect()
}

fn rms(v: &[f32]) -> f64 {
    (v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt()
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
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

fn through(b: &Builder, p: &Project, id: &str) -> bool {
    let id: ClipId = id.parse().unwrap();
    let c: Clip = p.clip(id).unwrap().1.clone();
    let ClipBody::Audio { offset, .. } = c.body else {
        panic!("not audio")
    };
    b.ara()
        .sound(p, &c, offset, &TempoMap::from_project(p), b.rate())
        .is_some()
}

fn render(b: &mut Builder, p: &Project, seconds: u64) -> Vec<f32> {
    let frames = u64::from(b.rate()) * seconds;
    export::render(b, p, Select::All, 0, frames, 0)
        .unwrap()
        .left
}

fn stopped(b: &Builder) -> Option<String> {
    b.ara()
        .reports()
        .into_iter()
        .find_map(|r| r.crashed.or(r.failed))
}

#[test]
#[ignore = "needs XIRIUS_ARA_REAL_PLUGIN; run with --ignored"]
fn a_real_plugin_through_the_songs_clips() {
    let path = plugin();
    watch_windows();
    // As the app: the fault filter is there before any plugin.
    xirius_ara::contain::install_own_filter();
    let dir = tempfile::tempdir().unwrap();
    let seconds = 4.0;
    let samples = tune(f64::from(RATE), seconds);
    write_wav(&dir.path().join("tune.wav"), &samples, &samples, RATE, 32).unwrap();

    // The plugin's own steps first: its name, notes and archive, outside the song.
    let audio = Arc::new(SourceAudio::stereo(
        f64::from(RATE),
        samples.clone(),
        samples.clone(),
    ));
    let spec = ClipSpec {
        regions: Vec::new(),
        key: 1,
        clone_of: None,
        source: source_id("tune.wav"),
        name: "tune".into(),
        audio,
        start: 0.0,
        offset: 0.0,
        duration: seconds,
    };
    let song = Song {
        clips: vec![spec],
        ..Song::default()
    };
    let begun = Instant::now();
    // On the plugin's one thread, as the app: a plugin tried faults on a second document made
    // on another thread than its first.
    let s = Service::start_shared(UID, loader(&path), None, Arc::new(|| {}));
    s.sync(&song);
    until("the plugin's render", || {
        if let Some(why) = s.crashed().or(s.failed()) {
            panic!("the plugin stopped: {why}");
        }
        s.state(1).is_some_and(|c| c.phase == Phase::Ready)
    });
    let info = s.info().unwrap();
    println!(
        "plugin: {} {} by {} (factory {}, archive {})",
        info.plugin_name, info.version, info.manufacturer, info.factory_id, info.archive_id
    );
    println!("analysed and rendered in {:?}", begun.elapsed());
    let notes = s.notes(1, false, WAIT);
    println!("notes: {} (the first {:?})", notes.len(), notes.first());
    let first = s.state(1).unwrap().rendered.unwrap();
    println!(
        "render: {} frames, rms {:.4} (the file {:.4})",
        first.left.len(),
        rms(&first.left),
        rms(&samples)
    );
    let stored = s.store(WAIT).expect("an archive");
    println!("archive: {} bytes ({})", stored.0.len(), stored.1);
    assert!(s.close(Instant::now() + WAIT));
    let s = Service::start_shared(UID, loader(&path), Some(stored), Arc::new(|| {}));
    s.sync(&song);
    until("the restored render", || {
        if let Some(why) = s.crashed().or(s.failed()) {
            panic!(
                "the plugin stopped: {why} (faults {}, code {:#x}, state {:?})",
                xirius_ara::contain::faults(),
                xirius_ara::contain::last_code(),
                s.state(1).map(|c| c.phase)
            );
        }
        s.state(1).is_some_and(|c| c.phase == Phase::Ready)
    });
    std::thread::sleep(Duration::from_millis(500));
    let again = s.state(1).unwrap().rendered.unwrap();
    let dirty = s.take_dirty();
    println!(
        "restored: rms {:.4}, notes {}, changed by opening: {dirty}",
        rms(&again.left),
        s.notes(1, false, WAIT).len()
    );
    let probe = s.probe(WAIT).unwrap();
    assert!(probe.analysis_queries > 0);
    assert_eq!(
        probe.analysis_requests, 0,
        "restoring requested another analysis"
    );
    assert_eq!(
        probe.analysis_starts, 0,
        "restoring started another analysis"
    );
    assert!(!dirty);
    // Tempo and signature changes arrive once; they do not transform the rendered audio.
    let mut mapped = song.clone();
    mapped.tempos = vec![(0.0, 0.0), (2.0, 4.0), (6.0, 8.0), (6.25, 9.0)];
    mapped.bars = vec![(0.0, 4, 4), (4.0, 3, 4), (7.0, 7, 8)];
    s.sync(&mapped);
    assert!(s.wait_idle(Instant::now() + WAIT));
    assert_eq!(s.probe(WAIT).unwrap().musical_updates, 1);
    let mapped_audio = s.state(1).unwrap().rendered.unwrap();
    let difference: Vec<f32> = mapped_audio
        .left
        .iter()
        .zip(&again.left)
        .map(|(a, b)| a - b)
        .collect();
    println!(
        "ARA3 tempo difference rms {}, kept_render {}",
        rms(&difference),
        Arc::ptr_eq(&mapped_audio, &again)
    );
    assert!(
        rms(&difference) < 1e-5,
        "tempo changed the rendered sound: {}",
        rms(&difference)
    );
    let updated_notes = s.notes(1, false, WAIT);
    assert_eq!(
        updated_notes
            .iter()
            .map(|n| n.frequency)
            .collect::<Vec<_>>(),
        notes.iter().map(|n| n.frequency).collect::<Vec<_>>()
    );
    println!("ARA3 tempo: complete maps, one notification, unchanged audio");
    // Repeated regions refer to the same modification and keep the prepared renderer.
    let prepared = s.probe(WAIT).unwrap().preparations;
    for count in [4usize, 7, 2, 4] {
        let spec = &mut mapped.clips[0];
        spec.duration = count as f64 - 0.5;
        spec.regions = (0..count)
            .map(|i| xirius_ara::document::RegionProps {
                start_in_modification: 0.5,
                duration_in_modification: if i + 1 == count { 0.5 } else { 1.0 },
                start_in_playback: i as f64,
                duration_in_playback: if i + 1 == count { 0.5 } else { 1.0 },
            })
            .collect();
        s.sync(&mapped);
        assert!(s.wait_idle(Instant::now() + WAIT));
        let probe = s.probe(WAIT).unwrap();
        assert_eq!(probe.regions, count);
        assert_eq!(probe.preparations, prepared);
        let looped = s.state(1).unwrap().rendered.unwrap();
        for i in 0..count {
            let duration = if i + 1 == count { 0.5 } else { 1.0 };
            // Ignore only 20 ms around seams: region boundaries may have plugin fades.
            let from = ((i as f64 + 0.02) * f64::from(RATE)) as usize;
            let to = ((i as f64 + duration - 0.02) * f64::from(RATE)) as usize;
            let reference = ((0.5 + 0.02) * f64::from(RATE)) as usize;
            let difference: Vec<f32> = looped.left[from..to]
                .iter()
                .zip(&first.left[reference..reference + to - from])
                .map(|(a, b)| a - b)
                .collect();
            assert!(
                rms(&difference) < 0.02,
                "loop pass {i} differs: {}",
                rms(&difference)
            );
        }
    }
    println!("ARA3 loops: counts 4/7/2/4, same modification and processing setup");

    let before_load = s.probe(WAIT).unwrap();
    let spec = &mut mapped.clips[0];
    spec.start = 0.0;
    spec.offset = 0.125;
    spec.duration = 3.2;
    spec.regions = (0..64)
        .map(|i| xirius_ara::document::RegionProps {
            start_in_modification: 0.125,
            duration_in_modification: 0.05,
            start_in_playback: i as f64 * 0.05,
            duration_in_playback: 0.05,
        })
        .collect();
    let load_started = Instant::now();
    s.sync(&mapped);
    assert!(s.wait_idle(Instant::now() + WAIT));
    let load = s.probe(WAIT).unwrap();
    let loaded = s.state(1).unwrap().rendered.unwrap();
    assert_eq!(load.regions, 64);
    assert_eq!(load.preparations, before_load.preparations);
    assert_eq!(load.analysis_requests, before_load.analysis_requests);
    assert!(loaded.left.iter().all(|v| v.is_finite()));
    println!(
        "ARA3 load: regions=64 elapsed={:?} frames={} source_reads={} prepare_delta=0 analysis_delta=0",
        load_started.elapsed(),
        loaded.left.len(),
        load.reads - before_load.reads
    );
    assert!(
        rms(&loaded.left) > 0.01,
        "the short repeated regions played silence"
    );

    assert!(s.close(Instant::now() + WAIT));

    // The song: attach, analysis, playback, written and opened again, detach.
    xirius_session::ara::register(UID, "Real ARA", {
        let path = path.clone();
        Arc::new(move || loader(&path))
    });
    let mut song = session(Project::new("song"));
    let t = id_of(&run(&mut song, "track.add.audio", json!({})));
    let a = id_of(&run(
        &mut song,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 3840, "file": "tune.wav"}),
    ));
    run(
        &mut song,
        "clip.ara.attach",
        json!({"ids": [a], "plugin": UID}),
    );
    println!("1. attached");
    let mut player = Builder::new(RATE, Some(dir.path().to_path_buf()));
    player.set_ara_live(true);
    let p = song.project().clone();
    let begun = Instant::now();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    until("the analysis", || {
        if let Some(why) = stopped(&player) {
            panic!("the plugin stopped: {why}");
        }
        through(&player, &p, &a)
    });
    println!("2. analysed in {:?}", begun.elapsed());
    let played = render(&mut player, &p, 4);
    println!(
        "3. played through the plugin: rms {:.4} (the file {:.4})",
        rms(&played),
        rms(&samples)
    );
    assert!(rms(&played) > 0.01, "the plugin played silence");
    let changed = xirius_session::ara::take_archives(&mut song, player.ara(), true).unwrap();
    println!("   the song changed by playing: {changed}");

    assert!(!changed, "analysis alone changed the song");
    let folder = dir.path().join("saved-song");
    song.save_as(&folder).unwrap();
    assert!(song.project().ara.archives.contains_key(UID));
    assert!(!song.is_dirty());
    write_wav(&folder.join("tune.wav"), &samples, &samples, RATE, 32).unwrap();
    drop(player);
    let mut reopened = Session::open_with(&folder, shared_registry()).unwrap();
    commands::attach(
        &mut reopened,
        ExtConfig {
            catalog: Some(Arc::new(Catalog::default())),
            ..ExtConfig::default()
        },
    );
    let opened = reopened.project().clone();
    // At another rate: the plugin still renders at the file's, so its edits stay (PLG-047).
    let mut player = Builder::new(OTHER_RATE, Some(folder.clone()));
    player.set_ara_live(true);
    let p = reopened.project().clone();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    until("the reopened render", || {
        if let Some(why) = stopped(&player) {
            panic!("the plugin stopped: {why}");
        }
        through(&player, &p, &a)
    });
    let replayed = render(&mut player, &p, 4);
    std::thread::sleep(Duration::from_millis(500));
    let changed = xirius_session::ara::take_archives(&mut reopened, player.ara(), true).unwrap();
    println!(
        "4. written and opened again at {OTHER_RATE} Hz: archive in the song {}, rms {:.4}, changed by opening: {}",
        p.ara.archives.contains_key(UID),
        rms(&replayed),
        changed || *reopened.project() != opened
    );
    assert!(rms(&replayed) > 0.01, "the reopened song played silence");
    assert!(
        !changed && *reopened.project() == opened,
        "opening changed the song"
    );

    let probe = player
        .ara()
        .doc(UID)
        .unwrap()
        .service()
        .probe(WAIT)
        .unwrap();
    assert!(probe.analysis_queries > 0);
    assert_eq!(probe.analysis_requests, 0);
    assert_eq!(probe.analysis_starts, 0);
    assert!(!reopened.is_dirty());
    println!("ARA3 archive: analysis-only save, no new analysis and no dirty on reopen");

    run(&mut reopened, "clip.ara.detach", json!({"ids": [a]}));
    let p = reopened.project().clone();
    player.sync_ara(&p, &TempoMap::from_project(&p), &mut Vec::new());
    let dry = render(&mut player, &p, 4);
    assert!(stopped(&player).is_none());
    // Detach keeps the document until its asynchronous final archive is collected.
    until("the detached document's final archive", || {
        player.ara().take_arrived(&p);
        player.ara().reports().is_empty()
    });
    println!(
        "5. detached: rms {:.4}, the plugin's documents {}",
        rms(&dry),
        player.ara().reports().len()
    );
    assert!(player.ara().reports().is_empty());
    assert!((rms(&dry) - rms(&played)).abs() < 0.05);
    drop(player);
    println!("done: no window, no fault");
}
