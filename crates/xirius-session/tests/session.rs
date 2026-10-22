//! The session layer end to end: a song made with core's commands, played by the engine with no
//! device. Nothing here opens an audio device; every render is offline or silent.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::AudioEngine;
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, Options, RangeSpec, StemSet};
use xirius_session::ext::ExtConfig;
use xirius_session::graph::{Select, song_end_tick};
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::tempo::TempoMap;

// ---- helpers ------------------------------------------------------------------------------

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh folder under the temp dir, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-session-test-{}-{n}-{tag}",
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

fn song(name: &str) -> Session {
    let mut s = Session::with_registry(Project::new(name), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
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

fn run_host(s: &mut Session, host: &mut SessionHost, name: &str, args: Value) -> RunResult {
    commands::run(s, Some(host), name, &map(args), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

/// An instrument track with one MIDI clip of `bars` bars holding one long note of `pitch`.
fn voice(s: &mut Session, name: &str, pitch: i64, bars: f64) -> (String, String) {
    let t = id_of(&run(s, "track.add.instrument", json!({ "name": name })));
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
    (t, c)
}

struct Stats {
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Stats {
    fn rms(&self) -> f64 {
        rms_of(&self.left, &self.right)
    }
    fn peak(&self) -> f32 {
        self.left
            .iter()
            .chain(&self.right)
            .fold(0.0f32, |m, x| m.max(x.abs()))
    }
}

fn rms_of(l: &[f32], r: &[f32]) -> f64 {
    let n = (l.len() + r.len()).max(1) as f64;
    let sum: f64 = l.iter().chain(r).map(|x| f64::from(*x).powi(2)).sum();
    (sum / n).sqrt()
}

fn end_frame(p: &Project, rate: u32) -> u64 {
    TempoMap::from_project(p).frame_at(song_end_tick(p), f64::from(rate))
}

/// The whole song offline, `select` as the mix.
fn measure_sel(s: &Session, dir: Option<&Path>, select: Select) -> Stats {
    let p = s.project();
    let rate = p.settings.sample_rate;
    let mut b = Builder::new(rate, dir.map(Path::to_path_buf));
    let r = export::render(&mut b, p, select, 0, end_frame(p, rate), 0).unwrap();
    Stats {
        left: r.left,
        right: r.right,
    }
}

fn measure(s: &Session) -> Stats {
    measure_sel(s, None, Select::All)
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1e-9)
}

fn assert_ratio(a: f64, b: f64, want: f64, what: &str) {
    assert!(
        close(a / b, want, 0.02),
        "{what}: {a} / {b} = {} but {want} was expected",
        a / b
    );
}

fn read_wav(path: &Path) -> (hound::WavSpec, Vec<i32>, Vec<f32>) {
    let mut r = hound::WavReader::open(path).unwrap();
    let spec = r.spec();
    match spec.sample_format {
        hound::SampleFormat::Int => (
            spec,
            r.samples::<i32>().map(|x| x.unwrap()).collect(),
            vec![],
        ),
        hound::SampleFormat::Float => (
            spec,
            vec![],
            r.samples::<f32>().map(|x| x.unwrap()).collect(),
        ),
    }
}

fn read_f32(path: &Path) -> Vec<f32> {
    let (_, _, f) = read_wav(path);
    f
}

// ---- note times across tempo changes (ENG-030) -------------------------------------------

#[test]
fn note_times_follow_the_tempo_map() {
    let mut s = song("tempo");
    let (t, _) = voice(&mut s, "Lead", 60, 2.0);
    // 120 bpm for the first bar, 128 from the second.
    run(&mut s, "tempo.set", json!({ "bpm": 120 }));
    run(&mut s, "tempo.set", json!({ "bpm": 128, "at": 3840 }));
    let clip = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({ "track": t, "at": 7680, "bars": 1 }),
    ));
    // A second clip starts at tick 7680 with a half-beat note on its first tick.
    run(
        &mut s,
        "note.add",
        json!({ "clip": clip, "at": 7680, "pitch": 62, "length": 480 }),
    );

    let p = s.project();
    let rate = p.settings.sample_rate;
    let tmap = TempoMap::from_project(p);
    let track = &p.tracks[&t.parse().unwrap()];
    let mut b = Builder::new(rate, None);
    let mut warnings = Vec::new();
    let tl = b.timeline(p, track, &tmap, &mut warnings);
    assert!(warnings.is_empty(), "{warnings:?}");

    let sec = |ticks_120: f64, ticks_128: f64| {
        ticks_120 / 960.0 * 0.5 + ticks_128 / 960.0 * (60.0 / 128.0)
    };
    let at = |secs: f64| (secs * f64::from(rate)).round() as u64;
    let frames: Vec<u64> = tl.notes().iter().map(|n| n.frame).collect();

    // The 2-bar note: on at 0, off at the clip's end 7680 ticks = 3840 at 120 + 3840 at 128.
    assert_eq!(frames[0], 0);
    let end = at(sec(3840.0, 3840.0));
    assert!(
        frames.contains(&end) || frames.iter().any(|f| f.abs_diff(end) <= 1),
        "off at {end} in {frames:?}"
    );
    // Tick 7680 is bar 1 at 120 bpm (2 s) plus bar 2 at 128 bpm (1.875 s) in.
    let second_on = at(sec(3840.0, 3840.0));
    assert!(
        frames.iter().any(|f| f.abs_diff(second_on) <= 1),
        "{frames:?}"
    );
    // 480 ticks later, at 128 bpm.
    let second_off = at(sec(3840.0, 4320.0));
    assert!(
        frames.iter().any(|f| f.abs_diff(second_off) <= 1),
        "{frames:?}"
    );
    // The tempo change moved it from the 4.0 s a steady 120 bpm would give.
    assert!(second_on < at(4.0));
}

#[test]
fn a_note_across_a_tempo_change_changes_length_in_frames() {
    let mut s = song("across");
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
    // Starts half a bar before the change and lasts a whole beat.
    run(
        &mut s,
        "note.add",
        json!({ "clip": c, "at": 3360, "pitch": 60, "length": 960 }),
    );
    run(&mut s, "tempo.set", json!({ "bpm": 120 }));
    run(&mut s, "tempo.set", json!({ "bpm": 128, "at": 3840 }));
    let p = s.project();
    let rate = p.settings.sample_rate;
    let tmap = TempoMap::from_project(p);
    let track = &p.tracks[&t.parse().unwrap()];
    let tl = Builder::new(rate, None).timeline(p, track, &tmap, &mut Vec::new());
    let (on, off) = (tl.notes()[0].frame, tl.notes()[1].frame);
    // 480 ticks at 120 bpm (0.25 s) plus 480 ticks at 128 bpm (0.234375 s).
    let want = (0.25 + 0.234375) * f64::from(rate);
    assert_eq!(on, (1.75 * f64::from(rate)).round() as u64);
    assert!(
        ((off - on) as f64 - want).abs() <= 1.0,
        "{} vs {want}",
        off - on
    );
    // At a steady 120 bpm the same note would be 0.5 s.
    assert!(((off - on) as f64) < 0.5 * f64::from(rate));
}

// ---- routing: mute, solo, send, bus, folder ----------------------------------------------

#[test]
fn volume_and_pan_scale_the_sound() {
    let mut s = song("vol");
    let (t, _) = voice(&mut s, "A", 60, 1.0);
    let base = measure(&s);
    assert!(base.rms() > 0.01, "the track must sound: {}", base.rms());

    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [t], "db": -6.0 }),
    );
    assert_ratio(measure(&s).rms(), base.rms(), 0.501, "-6 dB");

    run(&mut s, "track.volume.set", json!({ "ids": [t], "db": 0.0 }));
    run(&mut s, "track.pan.set", json!({ "ids": [t], "pan": -1.0 }));
    let left = measure(&s);
    assert!(left.left.iter().any(|x| x.abs() > 0.01));
    assert!(
        left.right.iter().all(|x| x.abs() < 1e-6),
        "hard left must leave the right empty"
    );
}

#[test]
fn mute_wins_over_solo() {
    let mut s = song("mute");
    let (a, _) = voice(&mut s, "A", 60, 1.0);
    let (b, _) = voice(&mut s, "B", 67, 1.0);
    let both = measure(&s).rms();

    run(&mut s, "track.mute", json!({ "ids": [a], "on": true }));
    let only_b = measure(&s).rms();
    assert!(only_b > 0.0 && only_b < both);

    // Solo on B changes nothing; B is the only one sounding already.
    run(&mut s, "track.solo", json!({ "ids": [b], "on": true }));
    assert!(close(measure(&s).rms(), only_b, 1e-4));

    // Muted and soloed: silent.
    run(&mut s, "track.mute", json!({ "ids": [b], "on": true }));
    assert_eq!(measure(&s).peak(), 0.0);
}

#[test]
fn solo_silences_the_others_but_not_what_the_soloed_feeds() {
    let mut s = song("solo");
    let (a, _) = voice(&mut s, "A", 60, 1.0);
    let (_b, _) = voice(&mut s, "B", 67, 1.0);
    let a_alone = {
        let mut t = song("solo-a");
        voice(&mut t, "A", 60, 1.0);
        measure(&t).rms()
    };
    run(&mut s, "track.solo", json!({ "ids": [a], "on": true }));
    assert!(close(measure(&s).rms(), a_alone, 1e-4), "only A sounds");

    // A fed into a bus: soloing A keeps the bus it feeds.
    let bus = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Sub" })));
    run(&mut s, "track.output.set", json!({ "ids": [a], "to": bus }));
    assert!(
        close(measure(&s).rms(), a_alone, 1e-4),
        "the bus stays audible"
    );

    // Soloing the bus keeps what feeds it.
    run(&mut s, "track.solo", json!({ "ids": [a], "off": true }));
    run(&mut s, "track.solo", json!({ "ids": [bus], "on": true }));
    assert!(
        close(measure(&s).rms(), a_alone, 1e-4),
        "the feeder stays audible"
    );
}

#[test]
fn a_bus_sums_its_tracks_and_has_its_own_fader_and_mute() {
    let mut s = song("bus");
    let (a, _) = voice(&mut s, "A", 60, 1.0);
    let (b, _) = voice(&mut s, "B", 67, 1.0);
    let direct = measure(&s).rms();

    let bus = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Sub" })));
    run(
        &mut s,
        "track.output.set",
        json!({ "ids": [a, b], "to": bus }),
    );
    assert!(
        close(measure(&s).rms(), direct, 1e-4),
        "through a bus at 0 dB is the same"
    );

    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [bus], "db": -6.0 }),
    );
    assert_ratio(measure(&s).rms(), direct, 0.501, "bus fader");

    run(&mut s, "track.mute", json!({ "ids": [bus], "on": true }));
    assert_eq!(measure(&s).peak(), 0.0, "a muted bus passes nothing");
}

#[test]
fn a_send_adds_the_track_to_a_bus() {
    let mut s = song("send");
    let (a, _) = voice(&mut s, "A", 60, 1.0);
    let direct = measure(&s).rms();
    let bus = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Fx" })));

    // A 0 dB post-fader send to a bus that goes to the master doubles the amplitude.
    run(
        &mut s,
        "send.add",
        json!({ "ids": [a], "to": bus, "level": 0.0 }),
    );
    assert_ratio(measure(&s).rms(), direct, 2.0, "0 dB send");

    // Muting the track silences the send too.
    run(&mut s, "track.mute", json!({ "ids": [a], "on": true }));
    assert_eq!(measure(&s).peak(), 0.0);
    run(&mut s, "track.mute", json!({ "ids": [a], "off": true }));

    // Muting the bus leaves the direct sound.
    run(&mut s, "track.mute", json!({ "ids": [bus], "on": true }));
    assert_ratio(measure(&s).rms(), direct, 1.0, "bus muted");
}

#[test]
fn a_folder_passes_no_sound_but_its_bus_collects_its_tracks() {
    let mut s = song("folder");
    let f = id_of(&run(&mut s, "folder.add", json!({ "name": "Drums" })));
    let a = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "A", "into": f }),
    ));
    let c = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({ "track": a, "at": 0, "bars": 1 }),
    ));
    run(
        &mut s,
        "note.add",
        json!({ "clip": c, "at": 0, "pitch": 60, "length": 3840 }),
    );
    let (_b, _) = voice(&mut s, "B", 67, 1.0);
    let direct = measure(&s).rms();
    assert!(direct > 0.0);

    // A folder with no bus changes nothing.
    // With a bus, its tracks go there; the bus fader then scales A but not B.
    let bus = id_of(&run(&mut s, "track.add.bus", json!({ "name": "DrumBus" })));
    run(&mut s, "folder.bus.set", json!({ "id": f, "bus": bus }));
    assert!(close(measure(&s).rms(), direct, 1e-4));
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [bus], "db": -120.0 }),
    );
    let only_b = measure(&s).rms();
    assert!(
        only_b > 0.0 && only_b < direct,
        "the folder's track is gone, B stays"
    );

    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [bus], "db": 0.0 }),
    );
    run(&mut s, "track.mute", json!({ "ids": [bus], "on": true }));
    assert!(close(measure(&s).rms(), only_b, 1e-4));
}

// ---- inserts -----------------------------------------------------------------------------

#[test]
fn the_built_in_gain_insert_scales_the_track() {
    let mut s = song("gain");
    let (t, _) = voice(&mut s, "A", 60, 1.0);
    let base = measure(&s).rms();
    let r = run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "Gain", "kind": "effect", "format": "builtin", "uid": "gain" }),
    );
    let plugin = id_of(&r);
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": plugin, "param": "gain_db", "value": -6.0 }),
    );
    assert_ratio(measure(&s).rms(), base, 0.501, "gain insert");
}

// ---- audio clips -------------------------------------------------------------------------

fn write_sine(path: &Path, rate: u32, secs: f32, amp: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let n = (secs * rate as f32) as usize;
    for i in 0..n {
        let x = (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * amp;
        w.write_sample((x * 32767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

#[test]
fn audio_clips_play_from_a_wav_file() {
    let dir = Dir::new("audio");
    // A different rate from the song's, so the load resamples.
    write_sine(&dir.path().join("tone.wav"), 44100, 1.0, 0.5);
    let mut s = song("audio");
    let t = id_of(&run(&mut s, "track.add.audio", json!({ "name": "Tone" })));
    run(
        &mut s,
        "clip.add.audio",
        json!({ "track": t, "at": 0, "bars": 1, "file": "tone.wav" }),
    );

    let m = measure_sel(&s, Some(dir.path()), Select::All);
    let rate = s.project().settings.sample_rate as usize;
    assert_eq!(m.left.len(), 2 * rate, "one bar at 120 bpm is 2 s");
    // A sine of amplitude 0.5 has an rms of 0.3536 per channel while the file lasts.
    let first_half = rms_of(&m.left[..rate / 2], &m.right[..rate / 2]);
    assert!(close(first_half, 0.5 / 2f64.sqrt(), 0.03), "{first_half}");
    // The file is 1 s; after that the clip is silent.
    let after = &m.left[rate + rate / 20..];
    assert!(
        after.iter().all(|x| x.abs() < 1e-6),
        "silent after the file ends"
    );

    // A missing file is left out with a warning.
    run(
        &mut s,
        "clip.add.audio",
        json!({ "track": t, "at": 3840, "bars": 1, "file": "gone.wav" }),
    );
    let mut b = Builder::new(48000, Some(dir.path().to_path_buf()));
    let built = b.build(s.project(), Select::All);
    assert!(
        built.warnings.iter().any(|w| w.contains("gone")),
        "{:?}",
        built.warnings
    );
}

// ---- export ------------------------------------------------------------------------------

/// A fixed song: two voices, a bus with a send and a gain, a tempo change and a WAV clip.
fn fixed_song(dir: &Path) -> Session {
    write_sine(&dir.join("pad.wav"), 48000, 1.0, 0.25);
    let mut s = song("fixed");
    let (lead, _) = voice(&mut s, "Lead", 72, 2.0);
    let (bass, _) = voice(&mut s, "Bass", 48, 2.0);
    let pad = id_of(&run(&mut s, "track.add.audio", json!({ "name": "Pad" })));
    run(
        &mut s,
        "clip.add.audio",
        json!({ "track": pad, "at": 0, "bars": 1, "file": "pad.wav" }),
    );
    let bus = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Verb" })));
    run(
        &mut s,
        "send.add",
        json!({ "ids": [lead], "to": bus, "level": -9.0 }),
    );
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [bass], "db": -4.5 }),
    );
    run(
        &mut s,
        "track.pan.set",
        json!({ "ids": [lead], "pan": 0.3 }),
    );
    run(&mut s, "tempo.set", json!({ "bpm": 120 }));
    run(&mut s, "tempo.set", json!({ "bpm": 128, "at": 3840 }));
    s
}

#[test]
fn a_fixed_song_exports_a_fixed_wav() {
    let dir = Dir::new("fixed");
    let s = fixed_song(dir.path());
    let p = s.project();
    let out = dir.path().join("mix.wav");
    let o = Options {
        dir: Some(dir.path().to_path_buf()),
        bit_depth: Some(32),
        ..Options::default()
    };
    let rep = export::mixdown(p, &out, &o).unwrap();

    // 2 bars, the first at 120 bpm (2 s) and the second at 128 (1.875 s).
    let rate = 48000u64;
    let want_frames = (3.875 * rate as f64).round() as u64;
    assert!(
        rep.frames.abs_diff(want_frames) <= 1,
        "{} vs {want_frames}",
        rep.frames
    );
    assert_eq!((rep.sample_rate, rep.bit_depth), (48000, 32));
    assert_eq!(rep.clipped, 0);

    let (spec, _, samples) = read_wav(&out);
    assert_eq!(
        (spec.channels, spec.sample_rate, spec.bits_per_sample),
        (2, 48000, 32)
    );
    assert_eq!(spec.sample_format, hound::SampleFormat::Float);
    assert_eq!(samples.len() as u64, rep.frames * 2);
    let l: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let r: Vec<f32> = samples.iter().skip(1).step_by(2).copied().collect();
    let rms = rms_of(&l, &r);
    let peak = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(close(f64::from(rep.rms), rms, 1e-3), "{} vs {rms}", rep.rms);
    assert!(close(f64::from(rep.peak), f64::from(peak), 1e-6));

    // The numbers of this song, pinned. They move only if the mix itself changes.
    eprintln!("fixed song: frames {} peak {peak} rms {rms}", rep.frames);
    assert!(close(rms, EXPECT_RMS, 0.01), "rms {rms}");
    assert!(close(f64::from(peak), EXPECT_PEAK, 0.01), "peak {peak}");

    // The same song again gives the same bytes.
    let again = dir.path().join("mix2.wav");
    export::mixdown(p, &again, &o).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&again).unwrap());
}

// The lead and the bass are played by the built-in Sampler's piano (PLG-004), which starts
// louder and fades, where the placeholder held a sine: a lower RMS and a higher peak.
const EXPECT_RMS: f64 = 0.107_587;
const EXPECT_PEAK: f64 = 0.782_016;

#[test]
fn the_export_format_follows_the_song_settings_and_the_options() {
    let dir = Dir::new("format");
    let mut s = song("format");
    voice(&mut s, "A", 60, 1.0);
    run(
        &mut s,
        "project.settings",
        json!({ "sample_rate": 44100, "bit_depth": 16 }),
    );
    let o = Options::default();

    let a = dir.path().join("a.wav");
    let rep = export::mixdown(s.project(), &a, &o).unwrap();
    let (spec, ints, _) = read_wav(&a);
    assert_eq!((spec.sample_rate, spec.bits_per_sample), (44100, 16));
    assert_eq!(spec.sample_format, hound::SampleFormat::Int);
    assert_eq!(rep.frames, 88200, "one bar at 120 bpm");
    assert_eq!(ints.len(), 88200 * 2);
    assert!(ints.iter().any(|x| x.abs() > 100));

    let b = dir.path().join("b.wav");
    let o24 = Options {
        sample_rate: Some(96000),
        bit_depth: Some(24),
        ..Options::default()
    };
    let rep = export::mixdown(s.project(), &b, &o24).unwrap();
    let (spec, _, _) = read_wav(&b);
    assert_eq!((spec.sample_rate, spec.bits_per_sample), (96000, 24));
    assert_eq!(rep.frames, 192000);

    // A tail adds seconds after the end.
    let c = dir.path().join("c.wav");
    let rep = export::mixdown(
        s.project(),
        &c,
        &Options {
            tail_secs: 0.5,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(rep.frames, 88200 + 22050);
}

#[test]
fn ranges_pick_the_song_the_loop_or_a_section() {
    let dir = Dir::new("range");
    let mut s = song("range");
    voice(&mut s, "A", 60, 4.0);
    run(&mut s, "loop.set", json!({ "start": 3840, "end": 7680 }));
    run(
        &mut s,
        "section.add",
        json!({ "name": "Chorus", "at": 7680, "bars": 2 }),
    );
    let p = s.project();
    let rate = 48000u64;
    let frames = |range: RangeSpec| {
        let path = dir.path().join("r.wav");
        export::mixdown(
            p,
            &path,
            &Options {
                range,
                ..Options::default()
            },
        )
        .unwrap()
        .frames
    };
    assert_eq!(frames(RangeSpec::Song), 8 * rate);
    assert_eq!(frames(RangeSpec::Loop), 2 * rate);
    assert_eq!(frames(RangeSpec::Section("Chorus".into())), 4 * rate);
    assert_eq!(frames(RangeSpec::Ticks(0, 960)), rate / 2);
    assert!(
        export::mixdown(
            p,
            &dir.path().join("x.wav"),
            &Options {
                range: RangeSpec::Section("Nope".into()),
                ..Options::default()
            }
        )
        .is_err()
    );
}

#[test]
fn stems_hold_one_strip_each_and_add_up_to_the_mix() {
    let dir = Dir::new("stems");
    let mut s = song("stems");
    let (a, _) = voice(&mut s, "A", 60, 1.0);
    let (_b, _) = voice(&mut s, "B", 67, 1.0);
    let (c, _) = voice(&mut s, "C/muted", 72, 1.0);
    run(&mut s, "track.mute", json!({ "ids": [c], "on": true }));
    let bus = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Verb" })));
    run(
        &mut s,
        "send.add",
        json!({ "ids": [a], "to": bus, "level": -6.0 }),
    );

    let o = Options {
        bit_depth: Some(32),
        ..Options::default()
    };
    let out = dir.path().join("Stems");
    std::fs::create_dir_all(&out).unwrap();
    let rep = export::stems(s.project(), &out, StemSet::All, &o).unwrap();
    let names: Vec<&str> = rep.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["A", "B", "Verb"]);
    assert_eq!(rep.skipped.len(), 1);
    assert_eq!(rep.skipped[0].name, "C/muted");
    for f in &rep.files {
        assert!(f.path.exists(), "{:?}", f.path);
        assert_eq!(f.sample_rate, 48000);
    }

    // Every stem reaches the master through its own output, so A + B (+ the send's bus) is the mix.
    let mix_path = dir.path().join("mix.wav");
    export::mixdown(s.project(), &mix_path, &o).unwrap();
    let mix = read_f32(&mix_path);
    let sa = read_f32(&rep.files[0].path);
    let sb = read_f32(&rep.files[1].path);
    let sv = read_f32(&rep.files[2].path);
    assert_eq!(mix.len(), sa.len());
    let worst = (0..mix.len())
        .map(|i| (mix[i] - (sa[i] + sb[i] + sv[i])).abs())
        .fold(0.0f32, f32::max);
    // A's stem leaves its send out and the bus stem holds it, so the sum is exact.
    assert!(worst < 1e-4, "stems must add up to the mix: {worst}");
    // The bus stem alone is the send of A at -6 dB.
    let verb_rms = rms_of(&sv, &[]);
    let a_rms = rms_of(&sa, &[]);
    assert!(verb_rms > 0.0 && verb_rms < a_rms);

    // Only tracks, or only buses.
    let only_buses = dir.path().join("Buses");
    std::fs::create_dir_all(&only_buses).unwrap();
    let rep = export::stems(s.project(), &only_buses, StemSet::Buses, &o).unwrap();
    assert_eq!(rep.files.len(), 1);
}

#[test]
fn file_names_are_made_safe_and_unique() {
    assert_eq!(export::sanitize("a/b:c"), "a_b_c");
    assert_eq!(export::sanitize("CON"), "_CON");
    assert_eq!(export::sanitize("end."), "end");
    assert_eq!(export::sanitize(""), "_");

    let dir = Dir::new("names");
    let mut s = song("names");
    voice(&mut s, "Same", 60, 1.0);
    voice(&mut s, "Same", 64, 1.0);
    let rep = export::stems(
        s.project(),
        dir.path(),
        StemSet::Tracks,
        &Options::default(),
    )
    .unwrap();
    let files: Vec<String> = rep
        .files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files, vec!["Same.wav", "Same-2.wav"]);
}

#[test]
fn export_commands_run_through_the_command_table() {
    let dir = Dir::new("cmd");
    let mut s = song("cmd");
    voice(&mut s, "A", 60, 1.0);
    let out = dir.path().join("mix.wav");
    let r = run(
        &mut s,
        "export.mixdown",
        json!({ "range": "song", "out": out.display().to_string(), "bit_depth": 16 }),
    );
    assert!(out.exists());
    assert_eq!(r.data["frames"], 96000);
    assert_eq!(r.data["bit_depth"], 16);
    assert!(r.step.is_none(), "an export is not an edit");

    // A custom range in bar.beat.tick form.
    let custom = dir.path().join("custom.wav");
    let r = run(
        &mut s,
        "export.mixdown",
        json!({ "range": "custom", "from": "1.1.0", "to": "1.3.0", "out": custom.display().to_string() }),
    );
    assert_eq!(r.data["frames"], 48000);

    let stems = dir.path().join("st");
    let r = run(
        &mut s,
        "export.stems",
        json!({ "dir": stems.display().to_string(), "what": "tracks" }),
    );
    // Named {曲}_{トラック} unless another pattern is given (EXP-015).
    assert!(stems.join("cmd_A.wav").exists(), "{:?}", r.data);

    // The registry lists them next to core's commands.
    assert!(s.registry().get("export.mixdown").is_some());
    assert!(s.registry().get("track.add.bus").is_some());

    // The AI may not export (it writes files).
    let denied = commands::run(
        &mut s,
        None,
        "export.mixdown",
        &map(json!({ "out": out.display().to_string() })),
        &RunOptions::ai("tester"),
    );
    match denied {
        Err(_) => {}
        Ok(r) => assert!(r.forced_dry_run || r.decision.is_some(), "{r:?}"),
    }
}

#[test]
fn transport_commands_need_a_running_engine() {
    let mut s = song("t");
    voice(&mut s, "A", 60, 1.0);
    let r = commands::run(
        &mut s,
        None,
        "transport.play",
        &Map::new(),
        &RunOptions::human(),
    );
    assert!(r.is_err());
}

// ---- the running engine: diffs, undo, transport ------------------------------------------

/// A running engine that is never given a device: the test pulls the audio itself.
struct Rig {
    host: SessionHost,
    audio: AudioEngine,
}

impl Rig {
    fn new(s: &Session, dir: Option<&Path>) -> Rig {
        let (host, audio, warnings) = SessionHost::start(
            s.project(),
            HostConfig {
                dir: dir.map(Path::to_path_buf),
                ..HostConfig::default()
            },
        )
        .unwrap();
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

    /// Lets the smoothing of any changed value finish, with the transport stopped.
    fn settle(&mut self) {
        self.host.stop();
        self.pull(24000);
    }

    /// The song from the start to its end, as the engine plays it now.
    fn play_song(&mut self, s: &Session) -> Stats {
        self.settle();
        let frames = end_frame(s.project(), self.host.rate()) as usize;
        self.host.seek_frame(0);
        self.host.play();
        let (left, right) = self.pull(frames);
        self.host.stop();
        self.pull(480);
        Stats { left, right }
    }
}

#[test]
fn the_running_engine_plays_what_the_offline_render_does() {
    let mut s = song("live");
    voice(&mut s, "A", 60, 1.0);
    voice(&mut s, "B", 67, 1.0);
    let offline = measure(&s);
    let mut rig = Rig::new(&s, None);
    let live = rig.play_song(&s);
    assert_eq!(live.left.len(), offline.left.len());
    assert!(
        close(live.rms(), offline.rms(), 1e-3),
        "{} vs {}",
        live.rms(),
        offline.rms()
    );
}

#[test]
fn a_core_command_reaches_the_engine_as_a_small_diff() {
    let mut s = song("diff");
    let (a, _) = voice(&mut s, "A", 60, 1.0);
    let (b, _) = voice(&mut s, "B", 67, 1.0);
    let bus = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Fx" })));
    let mut rig = Rig::new(&s, None);
    let base = rig.play_song(&s).rms();
    assert!(base > 0.0);

    // A fader: one command, no new mix.
    s.run_value(
        "track.volume.set",
        json!({ "ids": [a], "db": -6.0 }),
        &RunOptions::human(),
    )
    .unwrap();
    let rep = rig.host.sync(s.project());
    assert!(!rep.swapped, "{rep:?}");
    assert_eq!(rep.commands, 1, "{rep:?}");
    assert_eq!(rep.deferred, 0);
    let after = rig.play_song(&s);
    assert!(after.rms() < base);
    assert!(close(after.rms(), measure(&s).rms(), 1e-3));
    // Nothing else changed, so there is nothing more to send.
    let again = rig.host.sync(s.project());
    assert_eq!(
        (again.swapped, again.commands, again.deferred),
        (false, 0, 0)
    );

    // A pan, a mute, a send level: each a command.
    for (name, args) in [
        ("track.pan.set", json!({ "ids": [b], "pan": 0.5 })),
        ("track.mute", json!({ "ids": [b], "on": true })),
        ("send.add", json!({ "ids": [a], "to": bus, "level": -3.0 })),
    ] {
        s.run_value(name, args, &RunOptions::human()).unwrap();
        let rep = rig.host.sync(s.project());
        // A new send changes the routing, so that one is a new mix; the others are not.
        assert_eq!(rep.swapped, name == "send.add", "{name}: {rep:?}");
        if !rep.swapped {
            assert!(rep.commands >= 1, "{name}: {rep:?}");
        }
        let live = rig.play_song(&s);
        assert!(
            close(live.rms(), measure(&s).rms(), 1e-3),
            "{name}: live {} vs offline {}",
            live.rms(),
            measure(&s).rms()
        );
    }

    // A send level afterwards is a small command again.
    let send = s.project().tracks[&a.parse().unwrap()].sends[0]
        .id
        .to_string();
    s.run_value(
        "send.level.set",
        json!({ "id": send, "level": -12.0 }),
        &RunOptions::human(),
    )
    .ok();
    let rep = rig.host.sync(s.project());
    assert!(!rep.swapped);

    // A note edit rebuilds that track's timeline only.
    let clip = s.project().tracks[&a.parse().unwrap()].clips[0]
        .id
        .to_string();
    s.run_value(
        "note.add",
        json!({ "clip": clip, "at": 960, "pitch": 64, "length": 480 }),
        &RunOptions::human(),
    )
    .unwrap();
    let rep = rig.host.sync(s.project());
    assert!(!rep.swapped);
    assert_eq!(rep.commands, 1, "{rep:?}");
    let live = rig.play_song(&s);
    assert!(close(live.rms(), measure(&s).rms(), 1e-3));
}

#[test]
fn a_routing_change_swaps_in_a_new_mix() {
    let mut s = song("swap");
    let (a, _) = voice(&mut s, "A", 60, 1.0);
    let mut rig = Rig::new(&s, None);
    let base = rig.play_song(&s).rms();
    let bus = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Sub" })));
    run(&mut s, "track.output.set", json!({ "ids": [a], "to": bus }));
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [bus], "db": -6.0 }),
    );
    let rep = rig.host.sync(s.project());
    assert!(rep.swapped, "{rep:?}");
    let live = rig.play_song(&s).rms();
    assert_ratio(live, base, 0.501, "through the bus");
    // The old mix came back to the control side to be freed, not on the audio thread.
    rig.host.poll();
    assert_eq!(rig.audio.leaked_plans(), 0);
}

#[test]
fn undo_restores_the_sound() {
    let mut s = song("undo");
    let (a, _) = voice(&mut s, "A", 60, 1.0);
    let mut rig = Rig::new(&s, None);
    let base = rig.play_song(&s);

    // A fader and undo.
    run_host(
        &mut s,
        &mut rig.host,
        "track.volume.set",
        json!({ "ids": [a], "db": -12.0 }),
    );
    let low = rig.play_song(&s).rms();
    assert_ratio(low, base.rms(), 0.251, "-12 dB");
    run_host(&mut s, &mut rig.host, "history.undo", json!({}));
    let back = rig.play_song(&s);
    assert!(
        close(back.rms(), base.rms(), 1e-3),
        "{} vs {}",
        back.rms(),
        base.rms()
    );
    assert!(close(f64::from(back.peak()), f64::from(base.peak()), 1e-3));

    // Deleting the notes and undo.
    let clip = s.project().tracks[&a.parse().unwrap()].clips[0]
        .id
        .to_string();
    run_host(
        &mut s,
        &mut rig.host,
        "note.delete",
        json!({ "ids": [clip] }),
    );
    assert_eq!(rig.play_song(&s).peak(), 0.0);
    run_host(&mut s, &mut rig.host, "history.undo", json!({}));
    assert!(close(rig.play_song(&s).rms(), base.rms(), 1e-3));

    // A routing change and undo (a new mix both ways), then redo.
    let bus = id_of(&run_host(
        &mut s,
        &mut rig.host,
        "track.add.bus",
        json!({ "name": "Sub" }),
    ));
    run_host(
        &mut s,
        &mut rig.host,
        "track.volume.set",
        json!({ "ids": [bus], "db": -6.0 }),
    );
    run_host(
        &mut s,
        &mut rig.host,
        "track.output.set",
        json!({ "ids": [a], "to": bus }),
    );
    let routed = rig.play_song(&s).rms();
    assert_ratio(routed, base.rms(), 0.501, "routed");
    run_host(&mut s, &mut rig.host, "history.undo", json!({ "count": 3 }));
    assert!(
        close(rig.play_song(&s).rms(), base.rms(), 1e-3),
        "undo of the routing"
    );
    run_host(&mut s, &mut rig.host, "history.redo", json!({ "count": 3 }));
    assert!(
        close(rig.play_song(&s).rms(), routed, 1e-3),
        "redo of the routing"
    );
}

#[test]
fn the_transport_plays_stops_seeks_and_loops_by_command() {
    let mut s = song("transport");
    voice(&mut s, "A", 60, 4.0);
    run(&mut s, "loop.set", json!({ "start": 3840, "end": 7680 }));
    let mut rig = Rig::new(&s, None);
    let rate = rig.host.rate() as usize;

    // Play from bar 2 (2 s).
    let r = commands::run(
        &mut s,
        Some(&mut rig.host),
        "transport.play",
        &map(json!({ "from": 3840 })),
        &RunOptions::human(),
    )
    .unwrap();
    assert!(r.step.is_none());
    rig.pull(rate / 2);
    let st = rig.host.poll();
    assert!(st.playing);
    assert!(
        st.frame.abs_diff(2 * rate as u64 + rate as u64 / 2) <= 480,
        "{}",
        st.frame
    );

    // Stop keeps the position.
    commands::run(
        &mut s,
        Some(&mut rig.host),
        "transport.stop",
        &Map::new(),
        &RunOptions::human(),
    )
    .unwrap();
    rig.pull(480);
    let st = rig.host.poll();
    assert!(!st.playing);
    let held = st.frame;
    rig.pull(4800);
    assert_eq!(
        rig.host.poll().frame,
        held,
        "stopped means the position stays"
    );

    // Seek.
    commands::run(
        &mut s,
        Some(&mut rig.host),
        "transport.seek",
        &map(json!({ "to": 960 })),
        &RunOptions::human(),
    )
    .unwrap();
    rig.pull(480);
    assert_eq!(rig.host.poll().frame, (rate / 2) as u64);

    // Loop: the position wraps inside the loop range (2 s to 4 s).
    commands::run(
        &mut s,
        Some(&mut rig.host),
        "transport.loop",
        &map(json!({ "on": true })),
        &RunOptions::human(),
    )
    .unwrap();
    commands::run(
        &mut s,
        Some(&mut rig.host),
        "transport.play",
        &map(json!({ "from": 3840 })),
        &RunOptions::human(),
    )
    .unwrap();
    rig.pull(3 * rate);
    let st = rig.host.poll();
    assert!(st.playing);
    assert!(
        (2 * rate as u64..4 * rate as u64).contains(&st.frame),
        "the loop keeps the position in 2..4 s: {}",
        st.frame
    );
    // The loop is 2 s long and 3 s were played from its start, so it is 1 s in.
    assert!(st.frame.abs_diff(3 * rate as u64) <= 960, "{}", st.frame);
}

#[test]
fn a_song_change_moves_the_end_the_engine_stops_at() {
    let mut s = song("end");
    let (a, _) = voice(&mut s, "A", 60, 1.0);
    let mut rig = Rig::new(&s, None);
    assert_eq!(rig.host.end_frame(), 96000);
    run_host(
        &mut s,
        &mut rig.host,
        "clip.add.midi",
        json!({ "track": a, "at": 3840, "bars": 1 }),
    );
    assert_eq!(rig.host.end_frame(), 192000);
}

#[test]
fn the_audio_thread_gets_no_allocation_for_a_sync() {
    // `AudioEngine::render` runs under `assert_no_alloc`; a sync with every kind of change must
    // pass through it without aborting.
    let dir = Dir::new("noalloc");
    write_sine(&dir.path().join("pad.wav"), 48000, 0.5, 0.2);
    let mut s = song("noalloc");
    let (a, c) = voice(&mut s, "A", 60, 1.0);
    let pad = id_of(&run(&mut s, "track.add.audio", json!({ "name": "Pad" })));
    let mut rig = Rig::new(&s, Some(dir.path()));
    rig.host.play();
    for step in 0..6 {
        match step {
            0 => drop(run_host(
                &mut s,
                &mut rig.host,
                "track.volume.set",
                json!({ "ids": [a], "db": -3.0 }),
            )),
            1 => drop(run_host(
                &mut s,
                &mut rig.host,
                "note.add",
                json!({ "clip": c, "at": 960, "pitch": 62, "length": 480 }),
            )),
            2 => drop(run_host(
                &mut s,
                &mut rig.host,
                "clip.add.audio",
                json!({ "track": pad, "at": 0, "bars": 1, "file": "pad.wav" }),
            )),
            3 => drop(run_host(
                &mut s,
                &mut rig.host,
                "track.mute",
                json!({ "ids": [a], "on": true }),
            )),
            4 => drop(run_host(
                &mut s,
                &mut rig.host,
                "track.add.bus",
                json!({ "name": "Fx" }),
            )),
            _ => drop(run_host(&mut s, &mut rig.host, "history.undo", json!({}))),
        }
        rig.pull(2400);
    }
    rig.host.poll();
}

// ---- automation lanes play (AUT-001, AUT-011) ---------------------------------------------

/// A volume lane that holds -40 dB through the first bar and 0 dB in the second.
fn ducked_first_bar(s: &mut Session, t: &str) {
    for (at, db) in [(0, -40.0), (3839, -40.0), (3840, 0.0)] {
        run(
            s,
            "auto.point.add",
            json!({ "track": t, "target": "volume", "at": at, "value": db }),
        );
    }
}

fn halves(left: &[f32], right: &[f32], split: usize) -> (f64, f64) {
    // Skip the first 50 ms of each half, where the smoothing of a jump is still moving.
    let skip = 2400;
    (
        rms_of(&left[skip..split], &right[skip..split]),
        rms_of(&left[split + skip..], &right[split + skip..]),
    )
}

#[test]
fn a_volume_lane_shapes_the_offline_render() {
    let mut s = song("autovol");
    let (t, _) = voice(&mut s, "A", 60, 2.0);
    let flat = measure(&s);
    ducked_first_bar(&mut s, &t);
    let r = measure(&s);
    assert_eq!(r.left.len(), flat.left.len());
    let split = end_frame(s.project(), 48000) as usize / 2;
    let (a, b) = halves(&r.left, &r.right, split);
    let (fa, fb) = halves(&flat.left, &flat.right, split);
    assert_ratio(b, fb, 1.0, "the second bar is at 0 dB");
    assert_ratio(a, fa, 0.01, "the first bar is 40 dB down");

    // Off mode ignores the lane; the fader's own value plays.
    run(&mut s, "auto.mode", json!({ "ids": [t], "mode": "off" }));
    assert!(close(measure(&s).rms(), flat.rms(), 1e-3));
}

#[test]
fn a_stair_in_a_volume_lane_holds_through_the_offline_render() {
    let mut s = song("autostair");
    let (t, _) = voice(&mut s, "A", 60, 2.0);
    let flat = measure(&s);
    // Two points only: a straight line would rise through the first bar; a stair holds.
    for (at, db) in [(0, -40.0), (3840, 0.0)] {
        run(
            &mut s,
            "auto.point.add",
            json!({ "track": t, "target": "volume", "at": at, "value": db }),
        );
    }
    let ramp = measure(&s);
    run(
        &mut s,
        "auto.segment.shape",
        json!({ "track": t, "target": "volume", "index": 0, "curve": 0.0, "step": true }),
    );
    let r = measure(&s);
    let split = end_frame(s.project(), 48000) as usize / 2;
    let (a, b) = halves(&r.left, &r.right, split);
    let (fa, fb) = halves(&flat.left, &flat.right, split);
    let (ra, _) = halves(&ramp.left, &ramp.right, split);
    assert_ratio(b, fb, 1.0, "the second bar is at 0 dB");
    assert_ratio(a, fa, 0.01, "the first bar holds 40 dB down");
    assert!(
        ra > a * 3.0,
        "the straight line rises in the first bar ({ra} vs {a})"
    );
}

#[test]
fn the_running_engine_follows_a_pan_lane() {
    let mut s = song("autopan");
    let (t, _) = voice(&mut s, "A", 60, 2.0);
    for (at, pan) in [(0, -1.0), (3839, -1.0), (3840, 1.0)] {
        run(
            &mut s,
            "auto.point.add",
            json!({ "track": t, "target": "pan", "at": at, "value": pan }),
        );
    }
    let mut rig = Rig::new(&s, None);
    rig.settle();
    rig.host.seek_frame(0);
    rig.host.play();
    let frames = end_frame(s.project(), rig.host.rate()) as usize;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    while left.len() < frames {
        let (l, r) = rig.pull(480.min(frames - left.len()));
        left.extend(l);
        right.extend(r);
        rig.host.poll();
    }
    let split = frames / 2;
    let side = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(side(&left[2400..split]) > 0.01);
    assert!(
        side(&right[2400..split]) < 1e-4,
        "the first bar is hard left"
    );
    assert!(side(&right[split + 2400..]) > 0.01);
    assert!(
        side(&left[split + 2400..]) < 1e-4,
        "the second bar is hard right"
    );

    // An edit while it plays keeps the lane followed.
    s.run_value(
        "track.volume.set",
        json!({ "ids": [t], "db": -6.0 }),
        &RunOptions::human(),
    )
    .unwrap();
    let rep = rig.host.sync(s.project());
    assert!(!rep.swapped, "{rep:?}");
}

#[test]
fn levels_reach_the_screen_per_strip_and_reset_when_taken() {
    use xirius_session::host::Strip;
    let mut s = song("levels");
    let (a, _) = voice(&mut s, "A", 60, 2.0);
    let (b, _) = voice(&mut s, "B", 67, 2.0);
    let bus = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Fx" })));
    run(&mut s, "track.output.set", json!({ "ids": [b], "to": bus }));
    run(&mut s, "track.mute", json!({ "ids": [a], "on": true }));
    let mut rig = Rig::new(&s, None);
    rig.settle();
    rig.host.poll();
    rig.host.take_levels();

    rig.host.seek_frame(0);
    rig.host.play();
    rig.pull(9600);
    rig.host.poll();
    let levels = rig.host.take_levels();
    let id = |t: &str| Strip::Track(t.parse().unwrap());
    let peak = |k: Strip| levels.get(&k).map_or(0.0, |r| r.peak[0].max(r.peak[1]));
    assert!(peak(id(&b)) > 0.01, "{levels:?}");
    assert!(peak(id(&bus)) > 0.01, "{levels:?}");
    assert!(peak(Strip::Master) > 0.01, "{levels:?}");
    assert_eq!(peak(id(&a)), 0.0, "a muted track shows no level");
    let rms = levels[&Strip::Master].rms[0];
    assert!(rms > 0.0 && rms <= levels[&Strip::Master].peak[0]);
    // Taken once: the next look holds only what came after.
    assert!(rig.host.take_levels().is_empty());

    // A new mix: indices change, and nothing stale lands on the wrong strip.
    let c = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Fx2" })));
    run(
        &mut s,
        "send.add",
        json!({ "ids": [b], "to": c, "level": 0.0 }),
    );
    assert!(rig.host.sync(s.project()).swapped);
    // The new mix has new instruments, which did not hear the note start: play it again.
    rig.host.seek_frame(0);
    rig.pull(9600);
    rig.host.poll();
    rig.host.take_levels();
    rig.pull(9600);
    rig.host.poll();
    let levels = rig.host.take_levels();
    let peak = |k: Strip| levels.get(&k).map_or(0.0, |r| r.peak[0].max(r.peak[1]));
    assert!(
        peak(id(&c)) > 0.01,
        "the new bus reports after the swap: {levels:?}"
    );
    assert_eq!(peak(id(&a)), 0.0);
}
