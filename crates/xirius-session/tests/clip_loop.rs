//! A clip's loop reaches the sound (EDT-041): the clip plays up to the loop's end and then the
//! loop again until the clip ends, for notes and for audio. Nothing here opens an audio device.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::NoteKind;
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, Options};
use xirius_session::ext::ExtConfig;
use xirius_session::tempo::TempoMap;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-clip-loop-test-{}-{n}-{tag}",
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

fn song(name: &str) -> Session {
    let mut s = Session::with_registry(Project::new(name), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    run(&mut s, "tempo.set", json!({ "bpm": 120 }));
    s
}

/// Where each note of `pitch` starts and stops, in ticks.
fn notes_of(s: &Session, track: &str, pitch: u8) -> Vec<(i64, i64)> {
    let p = s.project();
    let rate = p.settings.sample_rate;
    let tmap = TempoMap::from_project(p);
    let t = &p.tracks[&track.parse().unwrap()];
    let tl = Builder::new(rate, None).timeline(p, t, &tmap, &mut Vec::new());
    // 120 bpm, 960 ticks to the beat.
    let tick = |frame: u64| (frame as f64 / f64::from(rate) * 1920.0).round() as i64;
    let mut ons = Vec::new();
    let mut offs = Vec::new();
    for n in tl.notes() {
        match n.kind {
            NoteKind::On { pitch: p, .. } if p == pitch => ons.push(tick(n.frame)),
            NoteKind::Off { pitch: p } if p == pitch => offs.push(tick(n.frame)),
            _ => {}
        }
    }
    ons.sort();
    offs.sort();
    ons.into_iter().zip(offs).collect()
}

#[test]
fn a_looped_midi_clip_plays_up_to_the_loop_end_and_then_the_loop_until_it_ends() {
    let mut s = song("midi");
    let t = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Lead" }),
    ));
    let c = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": 2 }),
    ));
    // One note before the loop, one in it that runs past its end, one after it.
    for (at, pitch, length) in [(0, 60, 480), (960, 62, 1440), (2400, 64, 240)] {
        run(
            &mut s,
            "note.add",
            json!({ "clip": c, "at": at, "pitch": pitch, "length": length }),
        );
    }
    assert_eq!(notes_of(&s, &t, 62), vec![(960, 2400)]);
    assert_eq!(notes_of(&s, &t, 64), vec![(2400, 2640)]);

    // The second beat repeats: 960..1920 and then again from 1920 to the end at 7680.
    run(
        &mut s,
        "clip.loop.set",
        json!({ "ids": [c], "start": 960, "length": 960 }),
    );
    assert_eq!(
        notes_of(&s, &t, 60),
        vec![(0, 480)],
        "before the loop: once"
    );
    let want: Vec<(i64, i64)> = (1..8).map(|k| (k * 960, k * 960 + 960)).collect();
    assert_eq!(
        notes_of(&s, &t, 62),
        want,
        "cut at the loop's end each pass"
    );
    assert!(notes_of(&s, &t, 64).is_empty(), "after the loop: never");

    // --length alone keeps the head; --start alone keeps the end.
    run(
        &mut s,
        "clip.loop.set",
        json!({ "ids": [c], "length": 1920 }),
    );
    let l = s.project().clip_loop(c.parse().unwrap()).unwrap();
    assert_eq!((l.start, l.length), (960, 1920));
    assert_eq!(
        notes_of(&s, &t, 64),
        vec![(2400, 2640), (4320, 4560), (6240, 6480)]
    );
    run(
        &mut s,
        "clip.loop.set",
        json!({ "ids": [c], "start": 1920 }),
    );
    let l = s.project().clip_loop(c.parse().unwrap()).unwrap();
    assert_eq!((l.start, l.length), (1920, 960));

    // Undo takes it back a step at a time; --clear plays the clip as it was.
    run(&mut s, "history.undo", json!({}));
    assert_eq!(
        s.project().clip_loop(c.parse().unwrap()).unwrap().start,
        960
    );
    run(
        &mut s,
        "clip.loop.set",
        json!({ "ids": [c], "clear": true }),
    );
    assert_eq!(s.project().clip_loop(c.parse().unwrap()), None);
    assert!(s.project().clip_fx.is_empty());
    assert_eq!(notes_of(&s, &t, 62), vec![(960, 2400)]);
}

#[test]
fn a_bad_loop_is_refused() {
    let mut s = song("bad");
    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let c = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": 1 }),
    ));
    for args in [
        json!({ "ids": [c] }),
        json!({ "ids": [c], "start": -1 }),
        json!({ "ids": [c], "length": 0 }),
        json!({ "ids": [c], "start": 3840 }),
    ] {
        let r = commands::run(
            &mut s,
            None,
            "clip.loop.set",
            &map(args.clone()),
            &RunOptions::human(),
        );
        assert!(r.is_err(), "{args}");
    }
}

/// A file whose every sample says which tick of it it is (25 frames to the tick at 48 kHz and
/// 120 bpm), so the mix shows which part of the file plays where.
fn write_ramp(path: &Path, ticks: u32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for f in 0..ticks * 25 {
        w.write_sample((f / 25) as i16).unwrap();
    }
    w.finalize().unwrap();
}

fn mix(s: &Session, dir: &Path, name: &str) -> Vec<f32> {
    let out = dir.join(format!("{name}.wav"));
    let o = Options {
        dir: Some(dir.to_path_buf()),
        bit_depth: Some(32),
        ..Options::default()
    };
    export::mixdown(s.project(), &out, &o).unwrap();
    let mut r = hound::WavReader::open(&out).unwrap();
    r.samples::<f32>().map(|x| x.unwrap()).step_by(2).collect()
}

#[test]
fn a_looped_audio_clip_plays_the_loop_of_its_file_again() {
    let dir = Dir::new("audio");
    write_ramp(&dir.path().join("ramp.wav"), 8000);
    let mut s = song("audio");
    let t = id_of(&run(&mut s, "track.add.audio", json!({ "name": "A" })));
    let c = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({ "track": t, "at": 0, "bars": 2, "file": "ramp.wav" }),
    ));
    let plain = mix(&s, dir.path(), "plain");
    run(
        &mut s,
        "clip.loop.set",
        json!({ "ids": [c], "start": 960, "length": 960 }),
    );
    let looped = mix(&s, dir.path(), "looped");
    // The middle of a tick, away from where the ramp steps.
    let at = |tick: usize| tick * 25 + 12;
    let same = |a: f32, b: f32| (a - b).abs() < 1e-6;
    // Up to the loop's end it is the file as it was.
    for tick in [100, 1000, 1900] {
        assert!(same(looped[at(tick)], plain[at(tick)]), "{tick}");
    }
    // Then each pass plays ticks 960.. of the file again.
    for (tick, from) in [(1920, 960), (2000, 1040), (2900, 980), (7600, 1840)] {
        assert!(plain[at(from)].abs() > 1e-4, "the ramp sounds at {from}");
        assert!(
            same(looped[at(tick)], plain[at(from)]),
            "{tick}: {} vs {}",
            looped[at(tick)],
            plain[at(from)]
        );
    }
}
