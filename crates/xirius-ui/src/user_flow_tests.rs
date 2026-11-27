//! The flows of the test audit's chapter 6 that no other test walks end to end (ARC-058):
//! recording, playing and a plug-in's knob, each by its keys through the window (a headless
//! platform's input) and the real transport, into the silent output and the save worker; the
//! song saved is opened by another process. No window on the screen and no sound device.

use crate::save_flow_tests::{Desk, Win};
use crate::song::Song;
use crate::studio::Studio;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_session::output::OutputMode;

const REOPEN: &str = "XIRIUS_USER_FLOW_REOPEN";
/// The plug-in the other process exports the song with.
const PLUGIN: &str = "XIRIUS_USER_FLOW_PLUGIN";

/// The other process: opens the folder in `XIRIUS_USER_FLOW_REOPEN` and writes its song next
/// to it as JSON; with `XIRIUS_USER_FLOW_PLUGIN`, it also exports the song next to it as WAV.
#[test]
fn reopen_child() {
    let Some(dir) = std::env::var_os(REOPEN) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let mut session =
        xirius_core::session::Session::open_with(&dir, xirius_session::commands::shared_registry())
            .unwrap();
    let song = serde_json::to_vec(session.project()).unwrap();
    std::fs::write(dir.with_extension("json"), song).unwrap();
    let Some(plugin) = std::env::var_os(PLUGIN) else {
        return;
    };
    let entries = xirius_plugins::probe(Path::new(&plugin)).unwrap();
    xirius_session::commands::attach(
        &mut session,
        xirius_session::ext::ExtConfig {
            output: OutputMode::None,
            scan: xirius_plugins::ScanOptions {
                dirs: Vec::new(),
                cache: dir.with_extension("plugins.json"),
                ..Default::default()
            },
            catalog: Some(Arc::new(xirius_plugins::Catalog {
                entries,
                failures: Vec::new(),
            })),
        },
    );
    let args = json!({"out": dir.with_extension("wav"), "format": "wav", "bit_depth": 32});
    xirius_session::commands::run(
        &mut session,
        None,
        "export.mixdown",
        args.as_object().unwrap(),
        &xirius_core::session::RunOptions::human(),
    )
    .unwrap();
}

/// The song in `dir`, as another process opens it (and exports it with `plugin`).
fn reopened_elsewhere(dir: &Path, plugin: Option<&Path>) -> Value {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "user_flow_tests::reopen_child", "--nocapture"])
        .env(REOPEN, dir)
        .env_remove(PLUGIN)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if let Some(plugin) = plugin {
        child.env(PLUGIN, plugin);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        child.creation_flags(0x08000000);
    }
    let mut child = child.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "the other process: {status}");
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the other process never opened the song");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    serde_json::from_slice(&std::fs::read(dir.with_extension("json")).unwrap()).unwrap()
}

/// Draws frames, as the window would, until `done`; 30 seconds at most.
fn until(w: &Win, what: &str, mut done: impl FnMut(&mut Studio) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        {
            let mut s = w.studio.borrow_mut();
            if done(&mut s) {
                return;
            }
            assert!(Instant::now() < deadline, "{what}: error={:?}", s.error);
        }
        w.window.draw();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The song's sound made ready before the first key, as the window does when it opens.
fn prepare(w: &Win) {
    w.studio.borrow_mut().prepare_song();
    until(w, "the song's sound is ready", |s| {
        s.poll_preparing();
        s.preparing.is_none()
    });
    assert_eq!(w.studio.borrow().error, None);
}

/// The silent output's peak stays at `level` for five windows of 20 ms in a row.
fn settles(w: &Win, level: f32) {
    let mut steady = 0;
    until(w, &format!("the output settles at {level}"), |s| {
        std::thread::sleep(Duration::from_millis(20));
        let peak = s.transport.take_output_peak();
        if peak.iter().all(|p| (p - level).abs() < 0.01) {
            steady += 1;
        } else {
            steady = 0;
        }
        steady >= 5
    });
}

fn studio(root: &Path) -> Studio {
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&root.join("song")).unwrap();
    s
}

fn add_audio(s: &mut Studio, name: &str) -> String {
    s.song
        .run("track.add.audio", json!({"name": name}))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Ten seconds of a steady 0.25 on `track`.
fn import_level(s: &mut Studio, root: &Path, track: &str) {
    let wav = root.join("level.wav");
    xirius_session::export::write_wav(&wav, &vec![0.25; 480_000], &vec![0.25; 480_000], 48_000, 16)
        .unwrap();
    s.song
        .run("clip.import", json!({"file": wav, "track": track}))
        .unwrap();
}

fn song_json(s: &Studio) -> Value {
    serde_json::to_value(&*s.project()).unwrap()
}

fn track_named(project: &Value, name: &str) -> Value {
    project["tracks"]
        .as_object()
        .unwrap()
        .values()
        .find(|t| t["name"] == name)
        .unwrap_or_else(|| panic!("no track {name}"))
        .clone()
}

/// Every `"file"` a value names.
fn files(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, v) in map {
                match v {
                    Value::String(file) if k == "file" => out.push(file.clone()),
                    _ => files(v, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| files(v, out)),
        _ => {}
    }
}

/// A float WAV's samples, read by its chunks: its data chunk's bytes and its left channel.
fn wav_data(path: &Path) -> (Vec<u8>, Vec<f32>) {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    let mut at = 12;
    let (mut format, mut channels) = (0, 0);
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &bytes[at + 8..at + 8 + len];
        if id == b"fmt " {
            format = u16::from_le_bytes([body[0], body[1]]);
            channels = u16::from_le_bytes([body[2], body[3]]) as usize;
        } else if id == b"data" {
            // 3: IEEE float; 0xFFFE: the extensible header, which this export writes with it.
            assert!(format == 3 || format == 0xFFFE, "format {format}");
            let left = body
                .chunks_exact(4 * channels)
                .map(|frame| f32::from_le_bytes(frame[..4].try_into().unwrap()))
                .collect();
            return (body.to_vec(), left);
        }
        at += 8 + len + len % 2;
    }
    panic!("{} has no data", path.display());
}

/// NumPad* (the record key) records a take into an armed track, Space stops it and the take goes
/// into the song; Ctrl+S saves it and another process opens the take at the same length (test
/// audit §6 録音).
#[test]
fn the_record_key_records_a_take_space_stops_it_and_another_process_opens_it_at_its_length() {
    let root = crate::shot::TestFolder::new("xirius-ui-user-flow-rec-");
    let desk = Desk::new();
    let mut s = studio(root.path());
    let track = add_audio(&mut s, "Rec");
    s.song.toggle_arm(track.parse().unwrap()).unwrap();
    s.transport.mode = OutputMode::Silent { paced: true };
    s.transport.bind_window();
    let w = desk.open(s);
    prepare(&w);

    w.key("numpad*");
    until(&w, "NumPad* starts recording", |s| {
        s.transport.recording && s.transport.host().is_some_and(|h| h.is_recording())
    });
    let start = w
        .studio
        .borrow_mut()
        .transport
        .host_mut()
        .unwrap()
        .poll()
        .frame;
    until(&w, "half a second is recorded", |s| {
        s.transport.host_mut().unwrap().poll().frame >= start + 24_000
    });
    w.key("space");
    until(&w, "Space finishes the take", |s| {
        s.poll_finished_take();
        !s.transport.finishing_take && s.transport.take.is_none()
    });
    let here = {
        let s = w.studio.borrow();
        assert_eq!(s.error, None);
        assert!(!s.transport.recording);
        track_named(&song_json(&s), "Rec")["clips"].clone()
    };
    let clips = here.as_array().unwrap();
    assert_eq!(clips.len(), 1, "{here}");
    assert!(clips[0]["length"].as_i64().unwrap() > 0, "{here}");

    w.key("ctrl-s");
    w.settle();
    let dir = {
        let s = w.studio.borrow();
        assert_eq!(s.error, None);
        assert!(s.song.is_saved());
        s.song.dir().unwrap()
    };
    let elsewhere = reopened_elsewhere(&dir, None);
    assert_eq!(track_named(&elsewhere, "Rec")["clips"], here);
    let mut taken = Vec::new();
    files(&here, &mut taken);
    assert_eq!(taken.len(), 1, "{here}");
    let file = Path::new(&taken[0]);
    assert!(
        dir.join(file).is_file(),
        "the take {} is in the song's folder",
        file.display()
    );

    w.studio.borrow_mut().transport.reset();
    xirius_session::peaks::wait();
}

/// Space, pressed while playing, stops: past the time the screen waits for the engine's answer
/// the transport still says stopped (the engine's own state) and the silent output carries
/// nothing.
fn space_stops(w: &Win) {
    w.key("space");
    assert!(!w.studio.borrow().transport.playing);
    let answered = Instant::now() + Duration::from_millis(500);
    until(w, "the engine's answer", |_| Instant::now() > answered);
    assert!(!w.studio.borrow().transport.playing, "the engine plays on");
    w.studio.borrow_mut().transport.take_output_peak();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(w.studio.borrow_mut().transport.take_output_peak(), [0.0; 2]);
}

/// Space plays a clip into the silent output, which carries its level; Space again stops it
/// and the output falls silent (test audit §6 再生).
#[test]
fn space_plays_a_clip_into_the_silent_output_and_space_again_stops_it_to_silence() {
    let root = crate::shot::TestFolder::new("xirius-ui-user-flow-play-");
    let desk = Desk::new();
    let mut s = studio(root.path());
    let track = add_audio(&mut s, "Level");
    import_level(&mut s, root.path(), &track);
    s.transport.mode = OutputMode::Silent { paced: true };
    s.transport.bind_window();
    let w = desk.open(s);
    prepare(&w);

    w.key("space");
    assert!(w.studio.borrow().transport.playing);
    settles(&w, 0.25);

    space_stops(&w);
    assert_eq!(w.studio.borrow().error, None);

    w.studio.borrow_mut().transport.reset();
    xirius_session::peaks::wait();
}

/// The test CLAP as a `.clap` file. It stays loaded until the process ends, so the copy lives
/// beside the test's executable and is written only when it differs.
fn clap_fixture() -> PathBuf {
    xirius_test_clap::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let built = std::fs::read(deps.join(format!(
        "{}xirius_test_clap{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )))
    .expect("xirius-test-clap is built with the tests");
    let dir = deps.join("user-flow-clap");
    std::fs::create_dir_all(&dir).unwrap();
    let clap = dir.join(format!("{}.clap", xirius_test_clap::NAME));
    if std::fs::read(&clap).ok().as_deref() != Some(&built[..]) {
        std::fs::write(&clap, &built).unwrap();
    }
    clap
}

/// A plug-in on the way the window plays it (its own process, prepared without blocking): Space
/// plays a level through it, its knob halves the output, Ctrl+S saves it, and the export here
/// and the one another process makes from the saved song carry the same knob and the same
/// samples (test audit §6 プラグイン).
fn plugin_knob_flow(path: &Path, format: &str, param: &str) {
    crate::plugin_reload_shots::require_plugin_host();
    let _records = crate::shot::CrashRecords::new();
    let root = crate::shot::TestFolder::new("xirius-ui-user-flow-plugin-");
    let entries = xirius_plugins::probe(path).unwrap();
    let entry = entries[0].clone();
    let desk = Desk::new();
    let mut s = studio(root.path());
    // The song's export reads the list of plug-ins the window keeps.
    xirius_plugins::CacheFile {
        version: 0,
        bundles: vec![xirius_plugins::BundleRecord {
            path: path.to_path_buf(),
            fingerprint: xirius_plugins::fingerprint(path),
            entries: entries.clone(),
            failure: None,
            probe_ms: 0,
        }],
    }
    .write(s.song.plugin_cache())
    .unwrap();
    let track = add_audio(&mut s, "Fx");
    import_level(&mut s, root.path(), &track);
    assert!(s.run_command(
        "plugin.add",
        json!({
            "track": track, "name": entry.name, "kind": "effect",
            "format": format, "uid": entry.uid
        })
    ));
    let id = track_named(&song_json(&s), "Fx")["inserts"][0]["id"].clone();
    s.use_plugins(xirius_plugins::Catalog {
        entries,
        failures: Vec::new(),
    });
    s.transport.mode = OutputMode::Silent { paced: true };
    s.transport.bind_window();
    let w = desk.open(s);
    prepare(&w);

    w.key("space");
    settles(&w, 0.25);
    assert!(w.studio.borrow_mut().run_command(
        "plugin.param.set",
        json!({"id": id, "param": param, "value": 0.5})
    ));
    settles(&w, 0.125);
    space_stops(&w);

    w.key("ctrl-s");
    w.settle();
    let (dir, insert) = {
        let s = w.studio.borrow();
        assert_eq!(s.error, None);
        assert!(s.song.is_saved());
        let insert = track_named(&song_json(&s), "Fx")["inserts"][0].clone();
        (s.song.dir().unwrap(), insert)
    };
    assert_eq!(insert["params"][param], 0.5, "{insert}");

    let here = root.path().join("here.wav");
    assert_eq!(
        w.studio.borrow_mut().export_run(
            "export.mixdown",
            json!({"out": here, "format": "wav", "bit_depth": 32}),
        ),
        None
    );
    w.settle();
    assert_eq!(w.studio.borrow().error, None);
    let (here_data, left) = wav_data(&here);
    let middle = left[left.len() / 2];
    assert!((middle - 0.125).abs() < 0.01, "the export's level {middle}");

    let elsewhere = reopened_elsewhere(&dir, Some(path));
    assert_eq!(track_named(&elsewhere, "Fx")["inserts"][0], insert);
    let (there_data, _) = wav_data(&dir.with_extension("wav"));
    assert_eq!(
        xirius_core::store::sha256_hex(&here_data),
        xirius_core::store::sha256_hex(&there_data),
        "the samples exported here and by the other process"
    );

    w.studio.borrow_mut().transport.reset();
    xirius_session::peaks::wait();
}

#[test]
fn a_vst3_knob_turned_while_playing_is_saved_and_exports_the_same_in_another_process() {
    xirius_test_vst3::fixture_marker();
    plugin_knob_flow(
        &crate::plugin_fixture_shots::fixture_path(),
        "vst3",
        &xirius_test_vst3::GAIN_PARAMETER.to_string(),
    );
}

#[test]
fn a_clap_knob_turned_while_playing_is_saved_and_exports_the_same_in_another_process() {
    plugin_knob_flow(&clap_fixture(), "clap", &xirius_test_clap::GAIN.to_string());
}
