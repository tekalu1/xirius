//! `mix.listen` (AI-004, AI-013): a range of the song rendered offline and measured. The same
//! song and range give the same JSON, the range and the track decide what is measured, each bar
//! has its own levels, and a tone's band stands out of the spectrum. Nothing opens a device.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::write_wav;
use xirius_session::ext::ExtConfig;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-listen-test-{}-{n}-{tag}",
            std::process::id()
        ));
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

const RATE: u32 = 48_000;

/// `secs` of a tone at `hz`, the right channel `right` times the left.
fn tone(path: &Path, hz: f32, secs: f32, right: f32) {
    let n = (RATE as f32 * secs) as usize;
    let l: Vec<f32> = (0..n)
        .map(|i| 0.25 * (i as f32 / RATE as f32 * hz * std::f32::consts::TAU).sin())
        .collect();
    let r: Vec<f32> = l.iter().map(|x| x * right).collect();
    write_wav(path, &l, &r, RATE, 32).unwrap();
}

/// 120 bpm, 4/4: a bar is 2 s. "Tone" plays 1 kHz from bar 1 for 2 bars, "Wide" plays 200 Hz
/// with the right channel upside down over bars 1 and 2 as well; bar 3 is silent up to the end
/// of a quiet clip on "Tail" in bar 4.
fn song(tag: &str) -> (Dir, Session, String, String) {
    let dir = Dir::new(tag);
    let (k, w, t) = (
        dir.path().join("khz.wav"),
        dir.path().join("wide.wav"),
        dir.path().join("tail.wav"),
    );
    tone(&k, 1000.0, 4.0, 1.0);
    tone(&w, 200.0, 4.0, -1.0);
    tone(&t, 50.0, 2.0, 1.0);
    let mut s = Session::with_registry(Project::new(tag), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    run(&mut s, "tempo.set", json!({ "bpm": 120 }));
    let add = |s: &mut Session, name: &str, file: &Path, at: &str, bars: i64| {
        let track = id_of(&run(s, "track.add.audio", json!({ "name": name })));
        run(
            s,
            "clip.add.audio",
            json!({ "track": track, "at": at, "bars": bars, "file": file.to_string_lossy() }),
        );
        track
    };
    let tone_track = add(&mut s, "Tone", &k, "1.1.0", 2);
    let wide = add(&mut s, "Wide", &w, "1.1.0", 2);
    add(&mut s, "Tail", &t, "4.1.0", 1);
    (dir, s, tone_track, wide)
}

fn listen(s: &mut Session, args: Value) -> Value {
    run(s, "mix.listen", args).data
}

#[test]
fn the_same_song_and_range_give_the_same_numbers() {
    let (_dir, mut s, _, _) = song("same");
    let a = listen(&mut s, json!({ "range": "1.1.0..5.1.0" }));
    let b = listen(&mut s, json!({ "range": "1.1.0..5.1.0" }));
    assert_eq!(a, b);
    // Not a song left as it was by luck: something was heard.
    assert!(a["levels"]["rms_db"].as_f64().unwrap() > -40.0, "{a}");
    assert!(a["levels"]["lufs"].is_number(), "{a}");
    assert!(a["levels"]["true_peak_db"].is_number(), "{a}");
    assert!(a["levels"]["lra"].is_number(), "{a}");
    assert_eq!(a["sample_rate"], RATE);
    assert_eq!(a["frames"], 4 * 2 * RATE);
    assert_eq!(a["range"]["from"], "1.1.0");
    assert_eq!(a["range"]["to"], "5.1.0");
    assert_eq!(a["target"]["kind"], "master");
}

#[test]
fn each_bar_of_the_range_has_its_own_levels() {
    let (_dir, mut s, _, _) = song("bars");
    let v = listen(&mut s, json!({ "range": "1.3.0..5.1.0" }));
    let bars = v["bars"].as_array().unwrap();
    let numbers: Vec<u64> = bars.iter().map(|b| b["bar"].as_u64().unwrap()).collect();
    assert_eq!(numbers, [1, 2, 3, 4]);
    // The first bar is cut where the range starts.
    assert_eq!(bars[0]["from_tick"], 1920);
    assert_eq!(bars[0]["to_tick"], 3840);
    for b in &bars[..2] {
        assert!(b["rms_db"].as_f64().unwrap() > -30.0, "{b}");
        assert!(b["lufs"].is_number(), "{b}");
    }
    // Bar 3 is silent; bar 4 has the quiet clip.
    assert!(bars[2]["rms_db"].is_null(), "{}", bars[2]);
    assert!(bars[2]["lufs"].is_null(), "{}", bars[2]);
    assert!(bars[3]["rms_db"].is_number(), "{}", bars[3]);
}

#[test]
fn the_track_decides_what_is_heard() {
    let (_dir, mut s, tone_track, wide) = song("track");
    let range = "1.1.0..3.1.0";
    let t = listen(&mut s, json!({ "range": range, "track": tone_track }));
    assert_eq!(t["target"]["kind"], "track");
    assert_eq!(t["target"]["name"], "Tone");
    // The tone is the same on both sides: one channel, no width.
    assert_eq!(t["levels"]["correlation"], 1.0);
    assert_eq!(t["levels"]["width"], 0.0);
    let items = t["spectrum"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 10);
    let db = |hz: f64| {
        items
            .iter()
            .find(|i| i["hz"] == hz)
            .and_then(|i| i["db"].as_f64())
            .unwrap_or(f64::NEG_INFINITY)
    };
    for other in [125.0, 250.0, 4000.0] {
        assert!(db(1000.0) > db(other) + 40.0, "{t}");
    }
    let w = listen(&mut s, json!({ "range": range, "track": wide }));
    // Upside down on the right: all side and no middle.
    assert_eq!(w["levels"]["correlation"], -1.0);
    assert_eq!(w["levels"]["width"], 1.0);
    // The mix of both sits between.
    let m = listen(&mut s, json!({ "range": range }));
    let c = m["levels"]["correlation"].as_f64().unwrap();
    assert!(-1.0 < c && c < 1.0, "{m}");
}

#[test]
fn third_octave_bands_find_the_tone_too() {
    let (_dir, mut s, tone_track, _) = song("thirds");
    let v = listen(
        &mut s,
        json!({ "range": "1.1.0..2.1.0", "track": tone_track, "bands": "third" }),
    );
    assert_eq!(v["spectrum"]["bands"], "third");
    let items = v["spectrum"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 30);
    let top = items
        .iter()
        .max_by(|a, b| {
            let d = |x: &Value| x["db"].as_f64().unwrap_or(f64::NEG_INFINITY);
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    assert_eq!(top["hz"], 1000.0, "{v}");
    let centroid = v["spectrum"]["centroid_hz"].as_f64().unwrap();
    assert!((centroid - 1000.0).abs() < 10.0, "{centroid}");
}

#[test]
fn a_range_that_cannot_be_heard_is_refused() {
    let (_dir, mut s, _, _) = song("refused");
    let e = try_run(&mut s, "mix.listen", json!({ "range": "loop" })).unwrap_err();
    assert!(e.contains("ループ範囲がありません"), "{e}");
    let e = try_run(&mut s, "mix.listen", json!({ "range": "3.1.0..2.1.0" })).unwrap_err();
    assert!(e.contains("範囲が空"), "{e}");
}

#[test]
fn listening_leaves_the_song_as_it_was() {
    let (_dir, mut s, _, _) = song("read");
    let before = s.project().content_hash();
    let r = run(&mut s, "mix.listen", json!({ "range": "1.1.0..2.1.0" }));
    assert!(r.changes.is_empty(), "{:?}", r.changes);
    assert!(r.step.is_none());
    assert_eq!(s.project().content_hash(), before);
}
