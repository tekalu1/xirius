//! The session layer's extension: background exports, the player and VST3 plugins in a song.
//! The player plays into a silent output; nothing here opens an audio device.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::{Catalog, ScanOptions, encode_state, probe, uid_hex};
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::output::OutputMode;

// ---- helpers ------------------------------------------------------------------------------

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh folder under the temp dir, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p =
            std::env::temp_dir().join(format!("xirius-ext-test-{}-{n}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A config that never touches the user's plugin cache.
fn config(dir: &Path, output: OutputMode, catalog: Option<Arc<Catalog>>) -> ExtConfig {
    ExtConfig {
        output,
        scan: ScanOptions {
            dirs: vec![],
            cache: dir.join("plugins.json"),
            ..ScanOptions::default()
        },
        catalog,
    }
}

fn song(name: &str, config: ExtConfig) -> Session {
    let mut s = Session::with_registry(Project::new(name), shared_registry());
    commands::attach(&mut s, config);
    s
}

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    commands::run(s, None, name, &map(args), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

/// An instrument track with one MIDI clip of `bars` bars holding one long note.
fn voice(s: &mut Session, name: &str, bars: f64) -> String {
    let t = id_of(&run(s, "track.add.instrument", json!({ "name": name })));
    let c = id_of(&run(
        s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": bars }),
    ));
    run(
        s,
        "note.add",
        json!({ "clip": c, "at": 0, "pitch": 60, "length": (bars * 3840.0) as i64, "velocity": 100 }),
    );
    t
}

fn write_sine(path: &Path, rate: u32, secs: f32, amp: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let n = (secs * rate as f32) as usize;
    for i in 0..n {
        let x = (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * amp;
        w.write_sample(x).unwrap();
    }
    w.finalize().unwrap();
}

/// The RMS of a float WAV between `from` and `to` seconds, both channels.
fn rms_between(path: &Path, from: f64, to: f64) -> f64 {
    let mut r = hound::WavReader::open(path).unwrap();
    let spec = r.spec();
    assert_eq!(spec.sample_format, hound::SampleFormat::Float);
    let ch = spec.channels as usize;
    let rate = f64::from(spec.sample_rate);
    let samples: Vec<f32> = r.samples::<f32>().map(|x| x.unwrap()).collect();
    let (a, b) = ((from * rate) as usize * ch, (to * rate) as usize * ch);
    let part = &samples[a.min(samples.len())..b.min(samples.len())];
    let sum: f64 = part.iter().map(|x| f64::from(*x).powi(2)).sum();
    (sum / part.len().max(1) as f64).sqrt()
}

/// Polls `export.status --job` until the job is no longer running.
fn wait_job(s: &mut Session, job: &str) -> Value {
    let start = Instant::now();
    loop {
        let r = run(s, "export.status", json!({ "job": job }));
        if r.data["state"] != "running" {
            return r.data;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "the job did not end: {:?}",
            r.data
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---- background exports (EXP-001, AI-002) -------------------------------------------------

#[test]
fn a_background_export_reports_progress_and_ends_with_the_file() {
    let dir = Dir::new("job");
    let mut s = song("job", config(dir.path(), OutputMode::None, None));
    voice(&mut s, "A", 2.0);
    let out = dir.path().join("mix.wav");
    let r = run(
        &mut s,
        "export.mixdown",
        json!({ "out": out.display().to_string(), "background": true, "tail": 2.0 }),
    );
    assert_eq!(r.data["state"], "running", "{:?}", r.data);
    assert_eq!(r.data["status_command"], "export.status");
    assert!(r.step.is_none(), "an export is not an edit");
    let job = r.data["job"].as_str().unwrap().to_string();

    let done = wait_job(&mut s, &job);
    assert_eq!(done["state"], "done", "{done:?}");
    assert_eq!(done["fraction"], 1.0);
    assert_eq!(done["done"], done["total"]);
    // 2 bars at 120 bpm and a 2 s tail.
    assert_eq!(done["total"], 6 * 48000);
    assert_eq!(done["result"]["frames"], 6 * 48000);
    assert!(out.exists());

    // The list has it too.
    let all = run(&mut s, "export.status", json!({}));
    assert!(
        all.data["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j["job"] == job.as_str())
    );

    // Without --background it is done when the command returns.
    let now = dir.path().join("now.wav");
    let r = run(
        &mut s,
        "export.mixdown",
        json!({ "out": now.display().to_string() }),
    );
    assert!(now.exists());
    assert_eq!(r.data["frames"], 4 * 48000);
    assert!(r.data.get("job").is_none());
}

#[test]
fn a_cancelled_export_writes_no_file() {
    let dir = Dir::new("cancel");
    let mut s = song("cancel", config(dir.path(), OutputMode::None, None));
    voice(&mut s, "A", 1.0);
    let out = dir.path().join("long.wav");
    // Ten minutes of tail: long enough to still be running when the cancel arrives.
    let r = run(
        &mut s,
        "export.mixdown",
        json!({ "out": out.display().to_string(), "background": true, "tail": 600.0 }),
    );
    let job = r.data["job"].as_str().unwrap().to_string();
    run(&mut s, "export.cancel", json!({ "job": job }));
    let end = wait_job(&mut s, &job);
    assert_eq!(end["state"], "cancelled", "{end:?}");
    assert!(end["fraction"].as_f64().unwrap() < 1.0, "{end:?}");
    assert!(!out.exists());

    // Stems as a job, and an unknown job.
    let stems = dir.path().join("st");
    let r = run(
        &mut s,
        "export.stems",
        json!({ "dir": stems.display().to_string(), "background": true }),
    );
    let job = r.data["job"].as_str().unwrap().to_string();
    assert_eq!(wait_job(&mut s, &job)["state"], "done");
    // The stems are named {曲}_{トラック} unless another pattern is given (EXP-015).
    assert!(stems.join("cancel_A.wav").exists());
    let missing = commands::run(
        &mut s,
        None,
        "export.status",
        &map(json!({ "job": "export-0" })),
        &RunOptions::human(),
    );
    assert!(missing.is_err());
}

// ---- a background plugin scan (PLG-021) --------------------------------------------------

#[test]
fn a_background_scan_is_a_job_and_its_list_is_used_when_it_ends() {
    let dir = Dir::new("scan");
    // One bundle the cache already knows, so the scan needs no child process.
    let plugins = dir.path().join("VST3");
    let bundle = plugins.join("Comp.vst3");
    let bin = bundle.join("Contents").join("x86_64-win");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("Comp.vst3"), b"MZ").unwrap();
    let cache = dir.path().join("plugins.json");
    let entry = xirius_plugins::Entry {
        uid: "0123456789ABCDEF0123456789ABCDEF".into(),
        name: "Comp".into(),
        vendor: "Test Audio".into(),
        kind: xirius_core::model::PluginKind::Effect,
        format: "vst3".into(),
        category: "Fx|Dynamics".into(),
        version: "1.0".into(),
        path: bundle.clone(),
        ara: false,
    };
    let mut config = config(
        dir.path(),
        OutputMode::None,
        Some(Arc::new(Catalog::default())),
    );
    config.scan.dirs = vec![plugins];
    let mut s = song("scan", config);
    xirius_plugins::CacheFile {
        version: 2,
        bundles: vec![xirius_plugins::BundleRecord {
            fingerprint: xirius_plugins::fingerprint(&bundle),
            path: bundle,
            entries: vec![entry],
            failure: None,
            probe_ms: 1,
        }],
    }
    .write(&cache)
    .unwrap();
    let list = run(&mut s, "browser.plugin.list", json!({}));
    assert_eq!(list.data["count"], 0);
    // Nothing has scanned in this process yet (PLG-059).
    let st = run(&mut s, "plugin.scan.status", json!({}));
    assert_eq!(
        (st.data["state"].as_str(), st.data["plugins"].as_u64()),
        (Some("idle"), Some(0)),
        "{:?}",
        st.data
    );
    assert!(st.data["file"].is_null(), "no bundle is being opened");

    let r = run(&mut s, "plugin.rescan", json!({ "background": true }));
    assert_eq!(r.data["state"], "running", "{:?}", r.data);
    assert_eq!(r.data["status_command"], "export.status");
    assert!(r.step.is_none(), "a scan is not an edit");
    let job = r.data["job"].as_str().unwrap().to_string();
    assert!(job.starts_with("scan-"), "{job}");
    let done = wait_job(&mut s, &job);
    assert_eq!(done["state"], "done", "{done:?}");
    assert_eq!(done["command"], "plugin.rescan");
    assert_eq!(done["total"], 0, "nothing had to be opened");
    assert_eq!(done["result"]["plugins"], 1);
    assert_eq!(done["result"]["cached"], 1);
    // The next command sees the new list.
    let list = run(&mut s, "browser.plugin.list", json!({}));
    let names: Vec<&str> = list.data["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert_eq!(names, ["Comp"]);
    // plugin.scan.status says the same as the screen's plugin page (PLG-059).
    let st = run(&mut s, "plugin.scan.status", json!({}));
    assert_eq!(st.data["state"], "done", "{:?}", st.data);
    assert_eq!(st.data["job"], job.as_str());
    assert!(st.data["file"].is_null(), "an ended scan opens nothing");
    assert_eq!(
        (st.data["plugins"].as_u64(), st.data["failed"].as_u64()),
        (Some(1), Some(0))
    );
    assert_eq!(st.message, "プラグイン 1");
}

// ---- the player (ARR-001, ENG-009) --------------------------------------------------------

fn status(s: &mut Session) -> Value {
    run(s, "transport.status", json!({})).data
}

/// Polls `transport.status` until `ok` holds.
fn wait_status(s: &mut Session, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    let start = Instant::now();
    loop {
        let st = status(s);
        if ok(&st) {
            return st;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "{what}: {st:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_player_plays_stops_seeks_and_loops_into_a_silent_output() {
    let dir = Dir::new("player");
    let mut s = song(
        "player",
        config(dir.path(), OutputMode::Silent { paced: true }, None),
    );
    voice(&mut s, "A", 8.0);
    run(&mut s, "loop.set", json!({ "start": 3840, "end": 7680 }));
    let rate = 48000u64;

    let r = run(&mut s, "transport.play", json!({}));
    assert_eq!(r.data["output"], "無音");
    assert!(r.step.is_none(), "the transport is not an edit");
    let st = wait_status(&mut s, "playing", |v| {
        v["playing"] == true && v["frame"].as_u64().unwrap() > rate / 10
    });
    assert_eq!(st["looping"], false);

    run(&mut s, "transport.stop", json!({}));
    let st = wait_status(&mut s, "stopped", |v| v["playing"] == false);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        status(&mut s)["frame"],
        st["frame"],
        "stopped keeps the place"
    );

    // Bar 3 at 120 bpm is 4 s.
    run(&mut s, "transport.seek", json!({ "to": "3.1.0" }));
    let st = wait_status(&mut s, "seek", |v| v["frame"] == 4 * rate);
    assert_eq!(st["position"], "3.1.0");

    // The key table's jumps resolve against the song from where the player stands.
    run(&mut s, "transport.goto", json!({ "to": "prev-bar" }));
    wait_status(&mut s, "goto prev-bar", |v| v["frame"] == 2 * rate);
    run(&mut s, "transport.goto", json!({ "to": "start" }));
    wait_status(&mut s, "goto start", |v| v["frame"] == 0);
    let err = commands::run(
        &mut s,
        None,
        "transport.goto",
        &map(json!({ "to": "next-marker" })),
        &RunOptions::human(),
    )
    .unwrap_err();
    assert!(format!("{err:?}").contains("マーカー"), "{err:?}");

    // Looping bars 2 to 3 (2 s to 4 s), played from bar 2.
    let r = run(&mut s, "transport.loop", json!({ "on": true }));
    assert!(r.step.is_none());
    run(&mut s, "transport.play", json!({ "from": 3840 }));
    wait_status(&mut s, "looping", |v| {
        v["looping"] == true && v["playing"] == true
    });
    // Past the loop's end in wall time; the place stays inside it.
    std::thread::sleep(Duration::from_millis(2300));
    let st = status(&mut s);
    let f = st["frame"].as_u64().unwrap();
    assert!((2 * rate..4 * rate).contains(&f), "{st:?}");

    // An edit reaches the playing engine; the player keeps going.
    run(&mut s, "track.add.instrument", json!({ "name": "B" }));
    assert_eq!(status(&mut s)["playing"], true);
    run(&mut s, "transport.stop", json!({}));
    wait_status(&mut s, "stopped again", |v| v["playing"] == false);
}

#[test]
fn the_transport_is_refused_without_an_output() {
    let dir = Dir::new("nooutput");
    let mut s = song("nooutput", config(dir.path(), OutputMode::None, None));
    let r = commands::run(
        &mut s,
        None,
        "transport.status",
        &Map::new(),
        &RunOptions::human(),
    );
    assert!(r.is_err());
}

// ---- VST3 in a song (PLG-002, PLG-052) ----------------------------------------------------

/// A catalogue of one installed bundle, or `None` (the test skips) when it is not installed.
fn installed(relative: &str) -> Option<(PathBuf, Arc<Catalog>)> {
    let path = xirius_vst3::default_vst3_dir().join(relative);
    if !path.exists() {
        eprintln!("skipped: {} is not installed", path.display());
        return None;
    }
    let entries = probe(&path).expect("probe");
    Some((
        path,
        Arc::new(Catalog {
            entries,
            failures: vec![],
        }),
    ))
}

/// The leading number of a plugin's display text, e.g. "-6.02 dB" -> -6.02.
fn leading_number(text: &str) -> f64 {
    let t: String = text
        .trim()
        .chars()
        .map(|c| if c == '\u{2212}' { '-' } else { c })
        .take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.'))
        .collect();
    t.parse()
        .unwrap_or_else(|_| panic!("no number in {text:?}"))
}

fn warnings(r: &RunResult) -> Vec<String> {
    r.data["warnings"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|w| w.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn export(s: &mut Session, out: &Path) -> RunResult {
    run(
        s,
        "export.mixdown",
        json!({ "out": out.display().to_string(), "bit_depth": 32 }),
    )
}

#[test]
fn a_vst3_insert_sounds_in_the_export_and_keeps_its_state_through_save_and_open() {
    let Some((path, catalog)) = installed("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    let uid = catalog.entries[0].uid.clone();
    let dir = Dir::new("vst3");
    let song_dir = dir.path().join("song");
    let mut s = song(
        "vst3",
        config(dir.path(), OutputMode::None, Some(catalog.clone())),
    );
    s.save_as(&song_dir).unwrap();
    write_sine(&song_dir.join("sine.wav"), 48000, 2.0, 0.5);
    let t = id_of(&run(&mut s, "track.add.audio", json!({ "name": "Sine" })));
    run(
        &mut s,
        "clip.add.audio",
        json!({ "track": t, "at": 0, "bars": 1, "file": "sine.wav" }),
    );
    let plain = dir.path().join("plain.wav");
    export(&mut s, &plain);
    let unity = rms_between(&plain, 0.5, 1.5);
    assert!((unity - 0.5 / 2f64.sqrt()).abs() < 1e-3, "{unity}");

    // The plugin with its own defaults leaves the level alone.
    let r = run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "kHs Gain", "format": "vst3", "uid": uid }),
    );
    let pid = id_of(&r);
    let with = dir.path().join("with.wav");
    let r = export(&mut s, &with);
    assert!(warnings(&r).is_empty(), "{:?}", warnings(&r));
    let same = rms_between(&with, 0.5, 1.5);
    assert!((same / unity - 1.0).abs() < 1e-3, "{same} vs {unity}");

    // A state made by the plugin itself with the gain turned down.
    let module = Arc::new(xirius_vst3::Module::load(&path).unwrap());
    let class = module
        .classes()
        .into_iter()
        .find(|c| uid_hex(&c.cid) == uid)
        .unwrap();
    let plugin = xirius_vst3::Plugin::new(module, &class).unwrap();
    let gain = plugin
        .params()
        .into_iter()
        .find(|p| p.title == "Gain")
        .expect("a Gain parameter");
    assert!(plugin.set_param(gain.id, 0.3));
    let db = leading_number(&plugin.param_text(gain.id, 0.3).unwrap());
    let state = encode_state(&plugin.get_state().unwrap());
    drop(plugin);
    run(
        &mut s,
        "plugin.state.set",
        json!({ "id": pid, "state": state }),
    );
    let quiet = dir.path().join("quiet.wav");
    export(&mut s, &quiet);
    let lower = rms_between(&quiet, 0.5, 1.5);
    let want = unity * 10f64.powf(db / 20.0);
    assert!(
        (lower / want - 1.0).abs() < 0.01,
        "{db} dB: {lower}, expected {want}"
    );
    eprintln!("kHs Gain in a song: unity {unity:.6}, {db} dB -> {lower:.6} (expected {want:.6})");

    // Saved, closed and opened again: the reference and the state come back, and so does the
    // sound.
    s.save().unwrap();
    drop(s);
    let mut s = Session::open_with(&song_dir, shared_registry()).unwrap();
    commands::attach(&mut s, config(dir.path(), OutputMode::None, Some(catalog)));
    let x = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|x| x.id.to_string() == pid)
        .expect("the plugin is in the song")
        .clone();
    assert_eq!((x.format.as_str(), x.uid.as_str()), ("vst3", uid.as_str()));
    assert_eq!(x.state.as_deref(), Some(state.as_str()));
    let again = dir.path().join("again.wav");
    export(&mut s, &again);
    assert_eq!(
        std::fs::read(&quiet).unwrap(),
        std::fs::read(&again).unwrap(),
        "the reopened song renders the same bytes"
    );

    // A parameter set by name through the command reaches the plugin too.
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": pid, "param": "gain", "value": 0.3 }),
    );
    let by_name = dir.path().join("by-name.wav");
    export(&mut s, &by_name);
    let named = rms_between(&by_name, 0.5, 1.5);
    assert!((named / want - 1.0).abs() < 0.01, "{named} vs {want}");
}

#[test]
fn a_vst3_instrument_plays_the_notes_of_its_track() {
    let Some((_, catalog)) = installed("Vital.vst3") else {
        return;
    };
    let Some(entry) = catalog
        .entries
        .iter()
        .find(|e| e.kind == xirius_core::model::PluginKind::Instrument)
        .cloned()
    else {
        eprintln!("skipped: Vital offers no instrument class");
        return;
    };
    let dir = Dir::new("vital");
    let mut s = song("vital", config(dir.path(), OutputMode::None, Some(catalog)));
    let t = voice(&mut s, "Lead", 1.0);
    run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": entry.name, "kind": "instrument", "format": "vst3", "uid": entry.uid }),
    );
    let out = dir.path().join("vital.wav");
    let r = export(&mut s, &out);
    assert!(warnings(&r).is_empty(), "{:?}", warnings(&r));
    let rms = rms_between(&out, 0.2, 1.8);
    eprintln!("Vital in a song: RMS {rms:.6}");
    assert!(rms > 0.005, "{rms}");

    // A missing plugin is a warning and the song still exports, with the stand-in voice.
    let other = voice(&mut s, "Other", 1.0);
    run(
        &mut s,
        "plugin.add",
        json!({ "track": other, "name": "Gone", "kind": "instrument", "format": "vst3", "uid": "00000000000000000000000000000000" }),
    );
    let gone = dir.path().join("gone.wav");
    let r = export(&mut s, &gone);
    assert!(
        warnings(&r).iter().any(|w| w.contains("見つかりません")),
        "{:?}",
        warnings(&r)
    );
    assert!(gone.exists());
}

#[test]
fn the_test_tone_plays_left_then_right_into_a_silent_output() {
    let dir = Dir::new("tone");
    let mut s = song(
        "tone",
        config(dir.path(), OutputMode::Silent { paced: true }, None),
    );
    let r = run(&mut s, "audio.test-tone", json!({}));
    assert_eq!(r.data["playing"], true);
    assert_eq!(r.data["order"], json!(["left", "right"]));
    assert!(r.step.is_none(), "not an edit");
    let seconds = r.data["seconds"].as_f64().unwrap();
    assert!((1.0..2.0).contains(&seconds), "{seconds}");
    // What the device was sent, read as it goes: the left first, then the right.
    let start = Instant::now();
    let (mut left_at, mut right_at) = (None, None);
    let mut loudest = [0f64; 2];
    loop {
        let e = run(&mut s, "engine.stats", json!({}));
        let peak: Vec<f64> = e.data["output_peak"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        for c in 0..2 {
            loudest[c] = loudest[c].max(peak[c]);
        }
        let now = start.elapsed();
        if peak[0] > 0.05 && left_at.is_none() {
            left_at = Some(now);
        }
        if peak[1] > 0.05 && right_at.is_none() {
            right_at = Some(now);
        }
        if e.data["tone_left_ms"] == 0.0 && right_at.is_some() {
            break;
        }
        assert!(now < Duration::from_secs(20), "{e:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(left_at.unwrap() < right_at.unwrap());
    for p in loudest {
        assert!((20.0 * p.log10() + 18.0).abs() < 0.2, "{loudest:?}");
    }

    // Stopping ends it at once.
    run(&mut s, "audio.test-tone", json!({}));
    let r = run(&mut s, "audio.test-tone", json!({"stop": true}));
    assert_eq!(r.data["playing"], false);
    let start = Instant::now();
    while run(&mut s, "engine.stats", json!({})).data["tone_left_ms"] != 0.0 {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_rescan_looks_in_the_folders_added_in_the_settings_too() {
    let tmp = Dir::new("scan-folders");
    let settings = xirius_prefs::Settings::memory().shared();
    let mut s = Session::with_registry(Project::new("folders"), shared_registry());
    commands::attach_with(
        &mut s,
        config(tmp.path(), OutputMode::None, None),
        xirius_session::ext::ExtExtras {
            io_settings: Some(settings.clone()),
            ..Default::default()
        },
    );
    let dry = RunOptions {
        dry_run: true,
        ..RunOptions::human()
    };
    let folders = |s: &mut Session| {
        commands::run(s, None, "plugin.rescan", &map(json!({})), &dry)
            .unwrap()
            .message
    };
    assert!(
        folders(&mut s).starts_with("0 個のフォルダー"),
        "{}",
        folders(&mut s)
    );
    {
        let mut st = settings.lock().unwrap();
        st.prefs
            .set("plugins.vst3.paths", &json!([r"D:\Plugins\VST3"]))
            .unwrap();
        st.prefs
            .set(
                "plugins.clap.paths",
                &json!([r"D:\Plugins\CLAP", r"D:\Plugins\VST3"]),
            )
            .unwrap();
    }
    // The same folder given for both kinds is looked in once.
    assert!(
        folders(&mut s).starts_with("2 個のフォルダー"),
        "{}",
        folders(&mut s)
    );
}
