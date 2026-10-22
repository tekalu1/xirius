//! The built-in instruments and effects in a song (PLG-016, PLG-045): rendered offline, moved
//! by automation and by edits while the engine runs. Nothing here opens an audio device.

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::AudioEngine;
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::export;
use xirius_session::ext::ExtConfig;
use xirius_session::graph::{Select, song_end_tick};
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::tempo::TempoMap;

fn song(name: &str) -> Session {
    let mut s = Session::with_registry(Project::new(name), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    s
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    let args = match args {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    commands::run(s, None, name, &args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

/// An instrument track with one note of `pitch` through `bars` bars.
fn voice(s: &mut Session, pitch: i64, bars: f64) -> String {
    let t = id_of(&run(s, "track.add.instrument", json!({ "name": "A" })));
    let c = id_of(&run(
        s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": bars }),
    ));
    run(
        s,
        "note.add",
        json!({ "clip": c, "at": 0, "pitch": pitch, "length": (bars * 3840.0) as i64, "velocity": 100 }),
    );
    t
}

fn add(s: &mut Session, track: &str, name: &str) -> String {
    id_of(&run(
        s,
        "plugin.add",
        json!({ "track": track, "name": name }),
    ))
}

fn set(s: &mut Session, plugin: &str, param: &str, plain: &str) {
    run(
        s,
        "plugin.param.set",
        json!({ "id": plugin, "param": param, "plain": plain }),
    );
}

fn rms(l: &[f32], r: &[f32]) -> f64 {
    let n = (l.len() + r.len()).max(1) as f64;
    let sum: f64 = l.iter().chain(r).map(|x| f64::from(*x).powi(2)).sum();
    (sum / n).sqrt()
}

fn end_frame(p: &Project) -> u64 {
    let rate = f64::from(p.settings.sample_rate);
    TempoMap::from_project(p).frame_at(song_end_tick(p), rate)
}

/// The whole song offline.
fn render(s: &Session) -> (Vec<f32>, Vec<f32>) {
    let p = s.project();
    let mut b = Builder::new(p.settings.sample_rate, None);
    let r = export::render(&mut b, p, Select::All, 0, end_frame(p), 0).unwrap();
    (r.left, r.right)
}

fn level(s: &Session) -> f64 {
    let (l, r) = render(s);
    rms(&l, &r)
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1e-9)
}

/// A running engine that is never given a device: the test pulls the audio itself.
struct Rig {
    host: SessionHost,
    audio: AudioEngine,
}

impl Rig {
    fn new(s: &Session) -> Rig {
        let (host, audio, warnings) =
            SessionHost::start(s.project(), HostConfig::default()).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        Rig { host, audio }
    }

    fn pull(&mut self, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut out = vec![0.0f32; frames * 2];
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(480);
            self.audio.render(&mut out[done * 2..(done + n) * 2], 2);
            done += n;
        }
        let l = out.iter().step_by(2).copied().collect();
        let r = out.iter().skip(1).step_by(2).copied().collect();
        (l, r)
    }

    /// The song from start to end as the engine plays it now.
    fn level(&mut self, s: &Session) -> f64 {
        self.host.stop();
        self.pull(24000);
        let frames = end_frame(s.project()) as usize;
        self.host.seek_frame(0);
        self.host.play();
        let (l, r) = self.pull(frames);
        self.host.stop();
        self.pull(480);
        rms(&l, &r)
    }
}

#[test]
fn the_eq_shapes_the_offline_render() {
    let mut s = song("eq");
    let t = voice(&mut s, 96, 1.0);
    let base = level(&s);
    assert!(base > 0.0);
    let eq = add(&mut s, &t, "EQ");
    // Every value at its default: the sound goes through as it was.
    assert!(near(level(&s), base, 1e-3), "{} vs {base}", level(&s));
    set(&mut s, &eq, "high_cut_on", "on");
    set(&mut s, &eq, "high_cut_freq", "1000");
    set(&mut s, &eq, "high_cut_slope", "48 dB/oct");
    let cut = level(&s);
    assert!(cut < base * 0.05, "{cut} vs {base}");
    set(&mut s, &eq, "high_cut_on", "off");
    set(&mut s, &eq, "output", "-6");
    assert!(near(level(&s) / base, 0.501, 0.01));
}

#[test]
fn a_lane_moves_a_builtin_parameter() {
    let mut s = song("lane");
    let t = voice(&mut s, 60, 2.0);
    let eq = add(&mut s, &t, "EQ");
    let flat = render(&s);
    // The output at -24 dB through the first bar and at 0 dB in the second (as 0..1).
    let target = format!("param:{eq}:output");
    for (at, v) in [(0, 0.0), (3839, 0.0), (3840, 0.5)] {
        run(
            &mut s,
            "auto.point.add",
            json!({ "track": t, "target": target, "at": at, "value": v }),
        );
    }
    let (l, r) = render(&s);
    let split = l.len() / 2;
    // Past the glide to the new value (20 ms, about 2 dB left at 50 ms): the piano fades,
    // so its first moments in the bar weigh the most.
    let skip = 9600;
    let half = |l: &[f32], r: &[f32], second: bool| {
        let range = if second {
            split + skip..l.len()
        } else {
            skip..split
        };
        rms(&l[range.clone()], &r[range])
    };
    let first = half(&l, &r, false) / half(&flat.0, &flat.1, false);
    let second = half(&l, &r, true) / half(&flat.0, &flat.1, true);
    assert!(near(first, 0.0631, 0.02), "{first}");
    assert!(near(second, 1.0, 0.01), "{second}");
}

#[test]
fn a_builtin_value_moves_without_a_new_mix() {
    let mut s = song("live");
    let t = voice(&mut s, 64, 1.0);
    let eq = add(&mut s, &t, "EQ");
    let mut rig = Rig::new(&s);
    let base = rig.level(&s);
    assert!(near(base, level(&s), 1e-3));
    set(&mut s, &eq, "output", "-6");
    let rep = rig.host.sync(s.project());
    assert!(!rep.swapped, "{rep:?}");
    let live = rig.level(&s);
    assert!(near(live / base, 0.501, 0.01), "{live} vs {base}");
    assert!(near(live, level(&s), 1e-3));
}

#[test]
fn removing_or_bypassing_a_builtin_gives_the_sound_back() {
    let mut s = song("remove");
    let t = voice(&mut s, 67, 1.0);
    let base = level(&s);
    let eq = add(&mut s, &t, "EQ");
    set(&mut s, &eq, "mid1_gain", "12");
    set(&mut s, &eq, "low_cut_on", "on");
    assert!(!near(level(&s), base, 0.05));
    run(&mut s, "plugin.bypass", json!({ "ids": [eq] }));
    assert!(near(level(&s), base, 1e-6));
    run(&mut s, "plugin.bypass", json!({ "ids": [eq] }));
    run(&mut s, "plugin.remove", json!({ "ids": [eq] }));
    assert!(near(level(&s), base, 1e-6));
}

/// MIX-003: a Limiter on the master holds the exported file under its ceiling, and the file
/// starts where it did without it (its lookahead is made up for).
#[test]
fn a_limiter_on_the_master_holds_the_export_under_its_ceiling() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-builtin-mix003-")
        .tempdir()
        .unwrap();
    let dir = tmp.path();
    let mut s = song("master");
    for pitch in [48, 55, 60, 64] {
        let t = voice(&mut s, pitch, 1.0);
        run(&mut s, "track.volume.set", json!({ "ids": [t], "db": 6.0 }));
    }
    let o = export::Options {
        bit_depth: Some(32),
        ..export::Options::default()
    };
    let loud = export::mixdown(s.project(), &dir.join("loud.wav"), &o).unwrap();
    assert!(loud.peak > 1.0, "{}", loud.peak);
    // B-6: a clipped file says so in `warnings`, not only in `clipped` and the summary.
    assert!(loud.clipped > 0);
    assert!(
        loud.warnings
            .iter()
            .any(|w| w.contains("0 dBFS を超えたサンプルが")
                && w.contains(&loud.clipped.to_string())),
        "{:?}",
        loud.warnings
    );
    assert!(
        loud.true_peak_db.is_some_and(|tp| tp > 0.0),
        "{:?}",
        loud.true_peak_db
    );
    assert!(
        loud.warnings.iter().any(|w| w.contains("0 dBTP")),
        "{:?}",
        loud.warnings
    );
    let lim = add(&mut s, "master", "Limiter");
    set(&mut s, &lim, "ceiling", "-1");
    let path = dir.join("limited.wav");
    let held = export::mixdown(s.project(), &path, &o).unwrap();
    let ceiling = 10f32.powf(-1.0 / 20.0);
    assert!(held.peak <= ceiling + 1e-6, "{}", held.peak);
    assert!(held.peak > ceiling * 0.98, "{}", held.peak);
    assert_eq!(held.clipped, 0);
    assert_eq!(held.frames, loud.frames);
    let first = |p: &std::path::Path| {
        let mut r = hound::WavReader::open(p).unwrap();
        r.samples::<f32>().position(|x| x.unwrap() != 0.0)
    };
    assert_eq!(first(&path), first(&dir.join("loud.wav")));
}

/// PLG-011: the Delay's note lengths follow the song's tempo.
#[test]
fn the_delay_follows_the_song_tempo() {
    let mut s = song("delay");
    let t = voice(&mut s, 72, 1.0);
    // Above what is left of the neighbour when 0.45 s is not a whole number of frames in f64.
    let first = |s: &Session| render(s).0.iter().position(|x| x.abs() > 1e-6).unwrap();
    let dry = first(&s);
    let d = add(&mut s, &t, "Delay");
    set(&mut s, &d, "mix", "100");
    // A dotted eighth: 375 ms at 120 bpm, 450 ms at 100.
    assert_eq!(first(&s), dry + 18_000);
    run(&mut s, "tempo.set", json!({ "bpm": 100 }));
    assert_eq!(first(&s), dry + 21_600);
    set(&mut s, &d, "sync", "off");
    set(&mut s, &d, "time", "250");
    assert_eq!(first(&s), dry + 12_000);
}

/// PLG-046: effects on an audio track change what is heard, never the file. Taken off again,
/// the render is the same as before, sample for sample.
#[test]
fn inserts_leave_the_audio_file_as_it_was() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-builtin-plg046-")
        .tempdir()
        .unwrap();
    let dir = tmp.path();
    let file = dir.join("tone.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&file, spec).unwrap();
    for i in 0..48000 {
        let x = (i as f32 / 48000.0 * 330.0 * std::f32::consts::TAU).sin() * 0.4;
        w.write_sample((x * 32767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    let bytes = std::fs::read(&file).unwrap();
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();

    let mut s = song("insert");
    let t = id_of(&run(&mut s, "track.add.audio", json!({ "name": "Tone" })));
    run(
        &mut s,
        "clip.add.audio",
        json!({ "track": t, "at": 0, "bars": 1, "file": "tone.wav" }),
    );
    let render = |s: &Session| {
        let p = s.project();
        let mut b = Builder::new(p.settings.sample_rate, Some(dir.to_path_buf()));
        let r = export::render(&mut b, p, Select::All, 0, end_frame(p), 0).unwrap();
        (r.left, r.right)
    };
    let dry = render(&s);
    assert!(rms(&dry.0, &dry.1) > 0.1);
    let mut ids = Vec::new();
    for name in ["EQ", "Compressor", "Delay", "Reverb", "Limiter"] {
        ids.push(add(&mut s, &t, name));
    }
    set(&mut s, &ids[0], "mid1_gain", "12");
    set(&mut s, &ids[2], "mix", "50");
    let wet = render(&s);
    assert!(!near(rms(&wet.0, &wet.1), rms(&dry.0, &dry.1), 0.05));
    run(&mut s, "plugin.remove", json!({ "ids": ids }));
    assert_eq!(render(&s), dry);
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        modified
    );
}
