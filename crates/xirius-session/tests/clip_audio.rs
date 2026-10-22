//! `clip.reverse` and `clip.normalize` (EDT-016): one step each, the source never rewritten.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::id::ClipId;
use xirius_core::model::{ClipBody, Project};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, Options, RangeSpec, write_wav};
use xirius_session::ext::ExtConfig;

const RATE: u32 = 48_000;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-clip-audio-{}-{n}-{tag}",
            std::process::id()
        ));
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

fn song() -> Session {
    let mut s = Session::with_registry(Project::new("clip-audio"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
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

/// Two seconds of 440 Hz: the first second at `first`, the second at `second`.
fn two_levels(path: &Path, first: f32, second: f32) {
    let x: Vec<f32> = (0..2 * RATE as usize)
        .map(|i| {
            let amp = if i < RATE as usize { first } else { second };
            amp * (i as f32 / RATE as f32 * 440.0 * std::f32::consts::TAU).sin()
        })
        .collect();
    write_wav(path, &x, &x, RATE, 32).unwrap();
}

/// An audio clip at bar 2 playing `file` for `beats` beats, `skip` beats into it (at 120 a
/// beat is half a second).
fn clip(s: &mut Session, file: &Path, beats: i64, skip: i64) -> String {
    let t = id_of(&run(s, "track.add.audio", json!({})));
    let id = id_of(&run(
        s,
        "clip.add.audio",
        json!({"track": t, "at": 3840, "length": beats * 960, "file": file.to_string_lossy()}),
    ));
    if skip > 0 {
        run(s, "clip.slip", json!({"ids": [id], "by": -skip * 960}));
    }
    id
}

/// (reverse, offset, gain_db) of an audio clip.
fn state(p: &Project, id: &str) -> (bool, i64, f64) {
    let (_, c) = p.clip(id.parse::<ClipId>().unwrap()).unwrap();
    let ClipBody::Audio {
        process,
        offset,
        gain_db,
        ..
    } = &c.body
    else {
        panic!("not audio");
    };
    (process.reverse, *offset, *gain_db)
}

/// The highest sample of an export of the clip's bar.
fn export_peak(p: &Project, dir: &Path) -> f32 {
    let out = dir.join("mix.wav");
    export::mixdown(
        p,
        &out,
        &Options {
            range: RangeSpec::Ticks(3840, 3840 + 1920),
            bit_depth: Some(32),
            ..Options::default()
        },
    )
    .unwrap();
    let mut r = hound::WavReader::open(&out).unwrap();
    r.samples::<f32>()
        .map(|v| v.unwrap().abs())
        .fold(0.0, f32::max)
}

#[test]
fn reversing_keeps_the_same_part_of_the_source_and_is_one_step() {
    let dir = Dir::new("reverse");
    let file = dir.0.join("a.wav");
    two_levels(&file, 0.5, 0.5);
    let mut s = song();
    // Plays 0.5 s to 1.5 s of the two seconds.
    let a = clip(&mut s, &file, 2, 1);
    assert_eq!(state(s.project(), &a), (false, 960, 0.0));
    let n = s.history().len();
    run(&mut s, "clip.reverse", json!({"ids": [a]}));
    assert_eq!(s.history().len(), n + 1, "one step");
    // Reversed, the same part is 0.5 s from the end of the played sound.
    assert_eq!(state(s.project(), &a), (true, 960, 0.0));
    // Off again, and with a window that is not in the middle.
    run(&mut s, "clip.reverse", json!({"ids": [a]}));
    assert_eq!(state(s.project(), &a), (false, 960, 0.0));
    run(&mut s, "clip.slip", json!({"ids": [a], "by": 480}));
    run(&mut s, "clip.reverse", json!({"ids": [a], "on": true}));
    assert_eq!(state(s.project(), &a), (true, 1440, 0.0));
    run(&mut s, "clip.reverse", json!({"ids": [a], "on": true}));
    assert_eq!(state(s.project(), &a), (true, 1440, 0.0), "already on");
    run(&mut s, "clip.reverse", json!({"ids": [a], "off": true}));
    assert_eq!(state(s.project(), &a), (false, 480, 0.0));
    s.undo(1).unwrap();
    assert_eq!(state(s.project(), &a), (true, 1440, 0.0));
    // The source is as it was.
    let mut r = hound::WavReader::open(&file).unwrap();
    let first: f32 = r.samples::<f32>().nth(2).unwrap().unwrap();
    assert!(first > 0.0);
}

#[test]
fn a_reversed_clip_is_kept_through_saving_and_opening() {
    let dir = Dir::new("reverse-save");
    let file = dir.0.join("a.wav");
    two_levels(&file, 0.5, 0.5);
    let mut s = song();
    let a = clip(&mut s, &file, 2, 1);
    run(&mut s, "clip.reverse", json!({"ids": [a], "on": true}));
    let want = state(s.project(), &a);
    let folder = dir.0.join("song");
    s.save_as(&folder).unwrap();
    drop(s);
    let again = Session::open_with(&folder, shared_registry()).unwrap();
    assert_eq!(state(again.project(), &a), want);
    assert!(want.0);
}

#[test]
fn normalizing_brings_the_played_part_to_full_scale() {
    let dir = Dir::new("normalize");
    let file = dir.0.join("a.wav");
    two_levels(&file, 0.4, 0.8);
    let mut s = song();
    // The first second only: its peak is 0.4.
    let a = clip(&mut s, &file, 2, 0);
    let plain = export_peak(s.project(), &dir.0);
    let n = s.history().len();
    let r = run(&mut s, "clip.normalize", json!({"ids": [a]}));
    assert_eq!(s.history().len(), n + 1, "one step");
    let want = -20.0 * 0.4f64.log10();
    let (_, _, gain) = state(s.project(), &a);
    assert!((gain - want).abs() < 0.02, "{gain} dB, want {want}");
    assert!(r.message.contains("+8.0 dB"), "{}", r.message);
    let louder = export_peak(s.project(), &dir.0);
    assert!((louder / plain - 2.5).abs() < 0.02, "{plain} then {louder}");

    // Reversed it plays the same part backwards; slipped to the start of the reversed sound it
    // plays the second, louder second.
    s.undo(1).unwrap();
    run(&mut s, "clip.reverse", json!({"ids": [a]}));
    run(&mut s, "clip.normalize", json!({"ids": [a]}));
    let (_, offset, gain) = state(s.project(), &a);
    assert_eq!(offset, 1920);
    assert!((gain - want).abs() < 0.02, "{gain} dB, want {want}");
    run(&mut s, "clip.slip", json!({"ids": [a], "by": 1920}));
    run(&mut s, "clip.normalize", json!({"ids": [a]}));
    let (_, _, gain) = state(s.project(), &a);
    let want = -20.0 * 0.8f64.log10();
    assert!((gain - want).abs() < 0.02, "{gain} dB, want {want}");

    // A MIDI clip is turned away, and nothing changes.
    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let m = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "length": 960}),
    ));
    let n = s.history().len();
    assert!(try_run(&mut s, "clip.normalize", json!({"ids": [a, m]})).is_err());
    assert!(try_run(&mut s, "clip.reverse", json!({"ids": [m]})).is_err());
    assert_eq!(s.history().len(), n);
}

#[test]
fn a_silent_clip_keeps_its_gain() {
    let dir = Dir::new("silent");
    let file = dir.0.join("a.wav");
    two_levels(&file, 0.0, 0.0);
    let mut s = song();
    let a = clip(&mut s, &file, 2, 0);
    run(&mut s, "clip.gain.set", json!({"ids": [a], "db": -3}));
    run(&mut s, "clip.normalize", json!({"ids": [a]}));
    assert_eq!(state(s.project(), &a).2, -3.0);
}

/// Ticks at `first` BPM for `seconds`, then at `second` BPM for `seconds`.
fn ticks(path: &Path, first: f64, second: f64, seconds: f64) {
    let rate = f64::from(RATE);
    let x: Vec<f32> = (0..(2.0 * seconds * rate) as usize)
        .map(|i| {
            let t = i as f64 / rate;
            let (t, bpm) = if t < seconds {
                (t, first)
            } else {
                (t - seconds, second)
            };
            let since = t % (60.0 / bpm);
            (0.8 * (-since * 60.0).exp() * (since * 1000.0 * std::f64::consts::TAU).sin()) as f32
        })
        .collect();
    write_wav(path, &x, &x, RATE, 32).unwrap();
}

/// The clip's tempo and its length in ticks.
fn tempo(p: &Project, id: &str) -> (Option<f64>, i64) {
    let (_, c) = p.clip(id.parse::<ClipId>().unwrap()).unwrap();
    let ClipBody::Audio { process, .. } = &c.body else {
        panic!("not audio");
    };
    (process.tempo, c.length)
}

#[test]
fn detecting_the_tempo_reads_the_played_part_and_is_one_step() {
    let dir = Dir::new("detect");
    let file = dir.0.join("a.wav");
    // 15 s at 100 BPM, then 15 s at 128 BPM.
    ticks(&file, 100.0, 128.0, 15.0);
    let mut s = song();
    // At 120 a beat is half a second: 20 beats from the start play the first 10 s.
    let a = clip(&mut s, &file, 20, 0);
    let b = clip(&mut s, &file, 20, 32);
    let n = s.history().len();
    let r = run(&mut s, "clip.tempo.detect", json!({"ids": [a]}));
    assert_eq!(s.history().len(), n + 1);
    assert!(r.message.contains("BPM"), "{}", r.message);
    let got = tempo(s.project(), &a).0.unwrap();
    assert!((got - 100.0).abs() < 0.05, "{got}");
    run(&mut s, "clip.tempo.detect", json!({"ids": [b]}));
    let got = tempo(s.project(), &b).0.unwrap();
    assert!((got - 128.0).abs() < 0.05, "{got}");
    // Unfollowed, the clip keeps its length.
    assert_eq!(tempo(s.project(), &b).1, 20 * 960);

    // Following the song, the clip found at 100 BPM plays at 120 and ends sooner.
    run(&mut s, "clip.stretch", json!({"ids": [a], "follow": "on"}));
    let (_, len) = tempo(s.project(), &a);
    assert!((len - 20 * 960 * 100 / 120).abs() <= 2, "{len}");
    s.undo(3).unwrap();
    assert_eq!(tempo(s.project(), &a), (None, 20 * 960));
}

#[test]
fn a_loop_of_whole_bars_gets_its_exact_tempo() {
    let dir = Dir::new("loop");
    let file = dir.0.join("loop.wav");
    // Two bars at 128 BPM, twice over (the second half is the same tempo).
    ticks(&file, 128.0, 128.0, 3.75);
    let mut s = song();
    let r = run(
        &mut s,
        "clip.import",
        json!({"at": 0, "file": file.to_string_lossy()}),
    );
    let a = id_of(&r);
    let r = run(&mut s, "clip.tempo.detect", json!({"ids": [a]}));
    assert_eq!(tempo(s.project(), &a).0, Some(128.0));
    assert_eq!(r.message, "テンポを検出しました（128 BPM）");
}

#[test]
fn silence_has_no_tempo() {
    let dir = Dir::new("detect-silent");
    let file = dir.0.join("a.wav");
    two_levels(&file, 0.0, 0.0);
    let mut s = song();
    let a = clip(&mut s, &file, 4, 0);
    let n = s.history().len();
    let e = try_run(&mut s, "clip.tempo.detect", json!({"ids": [a]})).unwrap_err();
    assert_eq!(e, "テンポを検出できません");
    assert_eq!(s.history().len(), n);
    assert_eq!(tempo(s.project(), &a).0, None);
}
