//! Recording into the song through the host (REC-001, REC-004, REC-005, REC-014, REC-016): a
//! take lands on the song frame it was played at once the round trip is taken off, MIDI becomes
//! notes, loop laps become takes of which only the last plays, one undo takes a recording out
//! and leaves its files, every armed track records at once on the same frames (OPEN-07), and
//! the commands drive it all. The inputs are stand-ins; no device is opened and nothing is
//! played to the speakers.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use xirius_core::model::{Clip, ClipBody, Project, Track};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::AudioEngine;
use xirius_engine::record::{MockInput, MockMidi};
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::output::OutputMode;
use xirius_session::record::{self, Input, RecordOptions, Source, Sources};

const RATE: f64 = 48_000.0;

// ---- helpers ------------------------------------------------------------------------------

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-record-test-{}-{n}-{tag}",
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

/// Waits until the player is `frames` in. The silent output does not make up for time its
/// thread did not get (a busy machine), so a wait for a time may find nothing recorded.
fn played(s: &mut Session, frames: u64) {
    let begun = Instant::now();
    loop {
        let st = run(s, "transport.status", json!({}));
        if st.data["frame"].as_u64().unwrap_or(0) >= frames {
            return;
        }
        assert!(begun.elapsed() < Duration::from_secs(20), "{:?}", st.data);
        std::thread::sleep(Duration::from_millis(10));
    }
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

/// A song saved in `<dir>/song`, so takes have a folder to go to.
fn saved_song(dir: &Dir, config: ExtConfig) -> Session {
    let mut s = Session::with_registry(Project::new("rec"), shared_registry());
    commands::attach(&mut s, config);
    s.save_as(dir.0.join("song")).unwrap();
    s
}

fn track<'a>(s: &'a Session, id: &str) -> &'a Track {
    s.project()
        .tracks
        .values()
        .find(|t| t.id.to_string() == id)
        .unwrap_or_else(|| panic!("no track {id}"))
}

fn clips<'a>(s: &'a Session, id: &str) -> &'a [Clip] {
    &track(s, id).clips
}

fn armed_audio(s: &mut Session, name: &str) -> String {
    let t = id_of(&run(s, "track.add.audio", json!({ "name": name })));
    run(s, "track.arm", json!({ "ids": [t], "on": true }));
    t
}

fn write_sine(path: &Path, secs: f32, amp: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(secs * RATE as f32) as usize {
        w.write_sample((i as f32 / RATE as f32 * 440.0 * std::f32::consts::TAU).sin() * amp)
            .unwrap();
    }
    w.finalize().unwrap();
}

fn read_wav(path: &Path) -> (Vec<f32>, Vec<f32>) {
    let mut r = hound::WavReader::open(path).unwrap();
    assert_eq!(r.spec().channels, 2);
    assert_eq!(f64::from(r.spec().sample_rate), RATE);
    let s: Vec<f32> = r.samples::<f32>().map(Result::unwrap).collect();
    (
        s.iter().step_by(2).copied().collect(),
        s.iter().skip(1).step_by(2).copied().collect(),
    )
}

/// A running engine that is never given a device: the test renders it.
struct Rig {
    host: SessionHost,
    audio: AudioEngine,
}

impl Rig {
    fn new(s: &Session) -> Rig {
        let (host, audio, warnings) = SessionHost::start(
            s.project(),
            HostConfig {
                dir: s.dir().map(Path::to_path_buf),
                ..HostConfig::default()
            },
        )
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(f64::from(host.rate()), RATE);
        Rig { host, audio }
    }

    fn max_block(&mut self) -> usize {
        self.host.controller().shape().setup.max_block
    }

    /// Renders `frames` frames; gives the left channel.
    fn pull(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; 960];
        let mut left = Vec::with_capacity(frames);
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(480);
            self.audio.render(&mut out[..n * 2], 2);
            left.extend(out[..n * 2].iter().step_by(2));
            done += n;
            self.host.poll();
        }
        left
    }

    /// Stops, renders while the capture catches up and lets go, and places the takes.
    fn finish(&mut self, s: &mut Session) -> RunResult {
        assert!(self.host.record_stop());
        self.pull(24_000);
        record::finish_recording(
            s,
            &mut self.host,
            &RunOptions::human(),
            Duration::from_secs(5),
        )
        .unwrap()
    }

    fn frame_of(&self, tick: i64) -> u64 {
        self.host.tempo_map().frame_at(tick, RATE)
    }
}

fn audio_file(s: &Session, c: &Clip) -> PathBuf {
    match &c.body {
        ClipBody::Audio { file, .. } => s.dir().unwrap().join(file),
        other => panic!("not an audio clip: {other:?}"),
    }
}

// ---- audio --------------------------------------------------------------------------------

/// Records the song's own sound through a cable with a round trip of `trip` frames, telling
/// the host `told`. Gives what was played (from the song frame recording started at), the
/// file's left channel, the clip's first frame and the song frame recording started at.
fn record_through_a_cable(trip: usize, told: u64) -> (Vec<f32>, Vec<f32>, u64, u64) {
    let dir = Dir::new("cable");
    let mut s = saved_song(&dir, ExtConfig::default());
    write_sine(&s.dir().unwrap().join("tone.wav"), 8.0, 0.5);
    let tone = id_of(&run(&mut s, "track.add.audio", json!({ "name": "Tone" })));
    run(
        &mut s,
        "clip.add.audio",
        json!({ "track": tone, "at": 0, "bars": 4, "file": "tone.wav" }),
    );
    let vox = armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    let block = rig.max_block();
    let started = rig
        .host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(1920),
                latency: Some(told),
                input: Input::Given(Box::new(MockInput::loopback(trip, block))),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    assert_eq!(started.audio.map(|t| t.to_string()), Some(vox.clone()));
    assert_eq!(started.latency, told);
    assert!(rig.host.is_recording());
    let played = rig.pull(96_000);
    let r = rig.finish(&mut s);
    assert!(!rig.host.is_recording());
    assert!(r.step.is_some(), "{r:?}");
    assert_eq!(r.data["files"].as_array().unwrap().len(), 1, "{:?}", r.data);

    let c = &clips(&s, &vox)[0];
    assert_eq!(clips(&s, &vox).len(), 1);
    assert!(!c.muted);
    let path = audio_file(&s, c);
    assert!(
        path.starts_with(s.dir().unwrap().join("Media").join("Recordings")),
        "{path:?}"
    );
    assert_eq!(path.file_name().unwrap(), "Vox-1.wav");
    let (file, _) = read_wav(&path);
    // The clip is as long as the whole ticks the file holds.
    let (from, to) = (rig.frame_of(c.start), rig.frame_of(c.start + c.length));
    assert!(
        to - from <= file.len() as u64,
        "{} > {}",
        to - from,
        file.len()
    );
    assert!(
        file.len() as u64 - (to - from) < 25,
        "less than a tick left over"
    );
    (played, file, from, rig.frame_of(1920))
}

#[test]
fn a_take_lands_on_the_song_frame_it_was_played_at() {
    let trip = 333;
    let (played, file, clip_frame, at) = record_through_a_cable(trip, trip as u64);
    assert_eq!(at, 48_000);
    assert_eq!(clip_frame, at, "the take starts where recording started");
    // Frame k of the file is what was played at song frame clip_frame + k, to the sample.
    let off = (clip_frame - at) as usize;
    let n = file.len().min(played.len() - off);
    assert!(n > 90_000, "{n}");
    for k in 0..n {
        assert_eq!(file[k], played[off + k], "frame {k}");
    }
    assert!(
        played[..n].iter().any(|x| x.abs() > 0.1),
        "the tone was heard"
    );
}

#[test]
fn without_the_latency_the_take_is_late_by_the_round_trip() {
    let trip = 333;
    let (played, file, clip_frame, at) = record_through_a_cable(trip, 0);
    let off = (clip_frame - at) as usize;
    let n = file.len().min(played.len() - off);
    let mismatched = (0..n).filter(|&k| file[k] != played[off + k]).count();
    assert!(mismatched > n / 2, "{mismatched} of {n}");
    // It is the same sound, `trip` frames late.
    assert!(file[..trip].iter().all(|x| *x == 0.0));
    for k in trip..n {
        assert_eq!(file[k], played[off + k - trip], "frame {k}");
    }
}

/// Records from a stand-in device that delivers the counter (`stops`: only its first frames),
/// renders a second, unplugs it, and places the take. Gives the take's clip and the file's left
/// channel.
fn record_from_a_device_that_is_unplugged(name: &str, stops: Option<u64>) -> Vec<f32> {
    use std::sync::{Arc, Mutex};
    use xirius_session::output::{DeviceInput, Drivers, Output, SilentOutput};
    let dir = Dir::new(name);
    let mut s = saved_song(&dir, ExtConfig::default());
    let vox = armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    let held = Arc::new(Mutex::new(None));
    let h = held.clone();
    rig.host.drivers = Drivers {
        exclusive_out: None,
        shared_out: Arc::new(|audio, _, _| Ok(Output::Silent(SilentOutput::start(audio, false)))),
        exclusive_in: None,
        shared_in: Arc::new(move |_, _, _| {
            let counter = MockInput::counter();
            let input = DeviceInput::new(
                "Interface".into(),
                false,
                Arc::new(std::sync::atomic::AtomicU64::new(0)),
                Box::new(match stops {
                    Some(frames) => counter.stopping_after(frames),
                    None => counter,
                }),
            );
            *h.lock().unwrap() = Some(input.lost());
            Ok(input)
        }),
        devices: Arc::new(Vec::new),
    };
    rig.host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                input: Input::Device,
                ..RecordOptions::default()
            },
        )
        .unwrap();
    rig.pull(48_000);
    assert_eq!(rig.host.input_lost(), None);
    let lost: xirius_engine::device::Lost = held.lock().unwrap().take().expect("opened");
    lost.set("the device was unplugged".into());
    assert_eq!(
        rig.host.input_lost().as_deref(),
        Some("the device was unplugged")
    );
    rig.finish(&mut s);
    let takes = clips(&s, &vox);
    assert_eq!(takes.len(), 1, "the take is kept");
    let (l, _) = read_wav(&audio_file(&s, &takes[0]));
    assert!(l.windows(2).all(|w| w[1] == w[0] + 1.0), "contiguous");
    // The input was let go with the take: nothing is said of it any more.
    assert_eq!(rig.host.input_lost(), None);
    let c = &takes[0];
    // The clip ends with the file, to the tick (a clip ends on a whole tick at or before it).
    let file_end = rig.frame_of(c.start) + l.len() as u64;
    let clip_end = rig.frame_of(c.end());
    assert!(
        clip_end <= file_end && file_end - clip_end < rig.frame_of(1),
        "the clip ends at frame {clip_end}, the file at {file_end}"
    );
    l
}

/// ENG-040: the input device stops in the middle of a take (unplugged). The host says so, and
/// what was recorded before is placed as any take.
#[test]
fn a_take_whose_input_device_stops_keeps_what_was_recorded() {
    let l = record_from_a_device_that_is_unplugged("lost", None);
    // A device that went on delivering keeps every frame up to the stop.
    assert!(*l.last().unwrap() >= 47_999.0, "{}", l.last().unwrap());
}

/// ENG-040: a device that stops delivering is noticed late (a second without a callback). The
/// take ends at the last frame it delivered, not at the moment that was noticed: the silence in
/// between is cut off, from the file and from the clip.
#[test]
fn a_take_whose_input_device_stops_ends_at_the_last_frame_it_delivered() {
    const CAME: u64 = 30_000;
    let l = record_from_a_device_that_is_unplugged("lost-cut", Some(CAME));
    let first = l[0] as u64;
    assert_eq!(
        *l.last().unwrap(),
        (CAME - 1) as f32,
        "the last frame is the last one that came"
    );
    assert_eq!(l.len() as u64, CAME - first, "no silence after it");
}

#[test]
fn undo_takes_a_recording_out_and_leaves_the_file_for_redo() {
    let dir = Dir::new("undo");
    let mut s = saved_song(&dir, ExtConfig::default());
    let vox = armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    rig.host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                input: Input::Given(Box::new(MockInput::counter())),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    rig.pull(48_000);
    rig.finish(&mut s);
    let path = audio_file(&s, &clips(&s, &vox)[0]);
    let (l, r) = read_wav(&path);
    // The counter's frames, in order: nothing lost or repeated.
    assert!(l.windows(2).all(|w| w[1] == w[0] + 1.0), "contiguous");
    assert!(l.iter().zip(&r).all(|(a, b)| *b == -*a));

    run(&mut s, "history.undo", json!({}));
    assert!(clips(&s, &vox).is_empty(), "one undo takes the take out");
    assert!(path.exists(), "the file stays");
    run(&mut s, "history.redo", json!({}));
    assert_eq!(audio_file(&s, &clips(&s, &vox)[0]), path);

    // The next take never writes over the first.
    rig.host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    rig.pull(24_000);
    rig.finish(&mut s);
    let second = audio_file(&s, &clips(&s, &vox)[1]);
    assert_eq!(second.file_name().unwrap(), "Vox-2.wav");
    let (silence, _) = read_wav(&second);
    assert!(silence.iter().all(|x| *x == 0.0), "no input: silence");
    // A recording leaves the clips already there alone; only its own laps mute each other.
    assert!(!clips(&s, &vox)[0].muted);
}

#[test]
fn loop_laps_become_takes_and_the_last_whole_lap_plays_not_the_one_stopped_midway() {
    let dir = Dir::new("loop");
    let mut s = saved_song(&dir, ExtConfig::default());
    let vox = armed_audio(&mut s, "Vox");
    run(&mut s, "loop.set", json!({ "start": 0, "end": "2.1.0" }));
    let mut rig = Rig::new(&s);
    rig.host.set_looping(s.project(), true);
    rig.host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                input: Input::Given(Box::new(MockInput::counter())),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    // Two laps of 2 s and half of a third.
    rig.pull(96_000 * 2 + 48_000);
    let r = rig.finish(&mut s);
    assert_eq!(r.data["clips"].as_array().unwrap().len(), 3, "{:?}", r.data);
    let cs = clips(&s, &vox);
    assert_eq!(
        cs.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>(),
        [(0, 3840), (0, 3840), (0, 1920)]
    );
    assert_eq!(
        cs.iter().map(|c| c.muted).collect::<Vec<_>>(),
        [true, false, true],
        "the half lap stays as a take that is not selected"
    );
    assert_eq!(
        cs.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["Vox", "Vox", "Vox"],
        "each lap is named after its track (UI-296)"
    );
    // Each lap's file goes on from where the last one ended.
    let files: Vec<PathBuf> = cs.iter().map(|c| audio_file(&s, c)).collect();
    let (first, _) = read_wav(&files[0]);
    let (second, _) = read_wav(&files[1]);
    assert_eq!(first.len(), 96_000);
    assert_eq!(second[0], first[95_999] + 1.0);

    run(&mut s, "history.undo", json!({}));
    assert!(clips(&s, &vox).is_empty(), "all laps go in one undo");
    assert!(files.iter().all(|f| f.exists()));
}

/// Each channel of a WAV file.
fn read_columns(path: &Path) -> Vec<Vec<f32>> {
    let mut r = hound::WavReader::open(path).unwrap();
    let ch = usize::from(r.spec().channels);
    let s: Vec<f32> = r.samples::<f32>().map(Result::unwrap).collect();
    (0..ch)
        .map(|c| s.iter().skip(c).step_by(ch).copied().collect())
        .collect()
}

fn notes(s: &Session, id: &str) -> Vec<(i64, i64, u8)> {
    let cs = clips(s, id);
    assert_eq!(cs.len(), 1, "one take on {id}");
    let ClipBody::Midi { notes, .. } = &cs[0].body else {
        panic!("not a MIDI clip")
    };
    notes.iter().map(|n| (n.start, n.length, n.pitch)).collect()
}

#[test]
fn every_armed_track_records_its_own_input_on_the_same_frames() {
    let dir = Dir::new("many");
    let mut s = saved_song(&dir, ExtConfig::default());
    let vox = armed_audio(&mut s, "Vox");
    let gtr = armed_audio(&mut s, "Gtr");
    // The same channel on two tracks (a dry and a wet take of one microphone).
    let dbl = armed_audio(&mut s, "Vox");
    let far = armed_audio(&mut s, "Far");
    let keys = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    let pads = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Pads" }),
    ));
    run(
        &mut s,
        "track.arm",
        json!({ "ids": [keys, pads], "on": true }),
    );
    run(
        &mut s,
        "track.input.set",
        json!({ "ids": [vox, dbl], "input": "In 1" }),
    );
    run(
        &mut s,
        "track.input.set",
        json!({ "ids": [gtr], "input": "In 3+4" }),
    );
    run(
        &mut s,
        "track.input.set",
        json!({ "ids": [far], "input": "In 9" }),
    );
    run(
        &mut s,
        "track.input.set",
        json!({ "ids": [keys], "input": "keys" }),
    );
    let mut rig = Rig::new(&s);
    let midi = MockMidi::with_ports(
        vec!["Keys".into(), "Pads".into()],
        vec![
            (2_400, 0, [0x90, 60, 100]),
            (4_800, 1, [0x90, 72, 90]),
            (26_400, 0, [0x80, 60, 0]),
            (28_800, 1, [0x80, 72, 0]),
        ],
    );
    let started = rig
        .host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                input: Input::Given(Box::new(MockInput::counters(4))),
                midi: Some(Box::new(midi)),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    let ids = |v: &[xirius_core::id::TrackId]| v.iter().map(|t| t.to_string()).collect::<Vec<_>>();
    assert_eq!(
        ids(&started.audio_tracks),
        [vox.clone(), gtr.clone(), dbl.clone()]
    );
    assert_eq!(ids(&started.midi_tracks), [keys.clone(), pads.clone()]);
    assert!(
        started
            .warnings
            .iter()
            .any(|w| w.contains("Far") && w.contains("In 9")),
        "{:?}",
        started.warnings
    );
    rig.pull(72_000);
    let r = rig.finish(&mut s);
    assert!(r.step.is_some(), "{r:?}");
    assert_eq!(r.data["clips"].as_array().unwrap().len(), 5, "{:?}", r.data);
    assert!(clips(&s, &far).is_empty());

    let (v, g, d) = (
        &clips(&s, &vox)[0],
        &clips(&s, &gtr)[0],
        &clips(&s, &dbl)[0],
    );
    let names: Vec<String> = [v, g, d]
        .iter()
        .map(|c| {
            audio_file(&s, c)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["Vox-1.wav", "Gtr-1.wav", "Vox-2.wav"]);
    // The clips start and end together.
    assert_eq!((g.start, g.length), (v.start, v.length));
    assert_eq!((d.start, d.length), (v.start, v.length));
    let vf = read_columns(&audio_file(&s, v));
    let gf = read_columns(&audio_file(&s, g));
    let df = read_columns(&audio_file(&s, d));
    assert_eq!(
        (vf.len(), gf.len(), df.len()),
        (1, 2, 1),
        "mono, stereo, mono"
    );
    assert_eq!(gf[0].len(), vf[0].len());
    assert_eq!(df[0], vf[0], "the same channel twice is the same sound");
    // Frame k of every file is input frame base + k of its own channel: to the sample.
    let base = vf[0][0];
    assert!(vf[0].len() > 70_000, "{}", vf[0].len());
    for k in 0..vf[0].len() {
        let n = base + k as f32;
        assert_eq!(vf[0][k], n, "In 1, frame {k}");
        assert_eq!(gf[0][k], n + 2_000_000.0, "In 3, frame {k}");
        assert_eq!(gf[1][k], n + 3_000_000.0, "In 4, frame {k}");
    }

    // The MIDI takes run on the same frames: Keys has only its port, Pads every port.
    let k = &clips(&s, &keys)[0];
    assert_eq!(k.start, v.start);
    let tick = |frame: f32| ((frame - base) / 25.0) as i64;
    assert_eq!(notes(&s, &keys), [(tick(2_400.0), 960, 60)]);
    assert_eq!(
        notes(&s, &pads),
        [(tick(2_400.0), 960, 60), (tick(4_800.0), 960, 72)]
    );
}

// ---- MIDI ---------------------------------------------------------------------------------

#[test]
fn midi_on_an_armed_instrument_track_becomes_a_note_clip() {
    let mut s = Session::with_registry(Project::new("midi"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    let keys = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    run(&mut s, "track.arm", json!({ "ids": [keys], "on": true }));
    let mut rig = Rig::new(&s);
    // 25 frames a tick at 120 bpm: 2400 frames is tick 96.
    let midi = MockMidi::new(vec![
        (2_400, [0x90, 60, 100]),
        (26_400, [0x80, 60, 0]),
        (48_000, [0x90, 64, 80]),
    ]);
    let started = rig
        .host
        .record_start(
            s.project(),
            None,
            RecordOptions {
                at: Some(3840),
                midi: Some(Box::new(midi)),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    assert_eq!(started.midi.map(|t| t.to_string()), Some(keys.clone()));
    assert_eq!(started.audio, None);
    rig.pull(72_000);
    let r = rig.finish(&mut s);
    assert!(r.step.is_some());

    let c = &clips(&s, &keys)[0];
    assert_eq!(c.start, 3840);
    assert_eq!(c.length, 72_000 / 25);
    let ClipBody::Midi { notes, .. } = &c.body else {
        panic!("not a MIDI clip")
    };
    let got: Vec<(i64, i64, u8, u8)> = notes
        .iter()
        .map(|n| (n.start, n.length, n.pitch, n.velocity))
        .collect();
    // The held note ends with the clip.
    assert_eq!(got, [(96, 960, 60, 100), (1920, 2880 - 1920, 64, 80)]);

    run(&mut s, "history.undo", json!({}));
    assert!(clips(&s, &keys).is_empty());
}

#[test]
fn an_armed_instrument_track_without_a_midi_input_is_refused() {
    let mut s = Session::with_registry(Project::new("nomidi"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    let keys = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    let mut rig = Rig::new(&s);
    let e = rig
        .host
        .record_start(s.project(), None, RecordOptions::default())
        .unwrap_err();
    assert!(e.message.contains("track.arm"), "{}", e.message);
    run(&mut s, "track.arm", json!({ "ids": [keys], "on": true }));
    let e = rig
        .host
        .record_start(s.project(), None, RecordOptions::default())
        .unwrap_err();
    assert!(e.message.contains("MIDI"), "{}", e.message);
    // An armed audio track in a song never saved has nowhere to write.
    armed_audio(&mut s, "Vox");
    let e = rig
        .host
        .record_start(s.project(), None, RecordOptions::default())
        .unwrap_err();
    assert!(e.message.contains("保存"), "{}", e.message);
    assert!(!rig.host.is_recording());
}

/// ENG-001: a keyboard pulled out. The track that names it is left out of the next recording
/// with a word, keeps the name, and the other armed instrument tracks record from the ports
/// there are; once the port is back, the track records from it again.
#[test]
fn a_track_naming_a_port_that_went_is_left_out_and_keeps_the_name() {
    let mut s = Session::with_registry(Project::new("unplugged"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    let keys = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    let pads = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Pads" }),
    ));
    run(
        &mut s,
        "track.arm",
        json!({ "ids": [keys, pads], "on": true }),
    );
    run(
        &mut s,
        "track.input.set",
        json!({ "ids": [keys], "input": "KeyStep 37" }),
    );
    let mut rig = Rig::new(&s);
    let take = |rig: &mut Rig, s: &mut Session, ports: Vec<String>| {
        // A note on every port.
        let notes = (0..ports.len())
            .map(|i| (2_400, i, [0x90, 60, 100]))
            .collect();
        let midi = MockMidi::with_ports(ports, notes);
        let started = rig
            .host
            .record_start(
                s.project(),
                None,
                RecordOptions {
                    at: Some(0),
                    midi: Some(Box::new(midi)),
                    ..RecordOptions::default()
                },
            )
            .unwrap();
        rig.pull(24_000);
        let r = rig.finish(s);
        assert!(r.step.is_some(), "{r:?}");
        started
    };
    // Only the loopMIDI port is left.
    let started = take(&mut rig, &mut s, vec!["loopMIDI Port".into()]);
    let ids: Vec<String> = started.midi_tracks.iter().map(|t| t.to_string()).collect();
    assert_eq!(ids, std::slice::from_ref(&pads));
    assert!(
        started
            .warnings
            .iter()
            .any(|w| w.contains("Keys") && w.contains("KeyStep 37")),
        "{:?}",
        started.warnings
    );
    assert_eq!(track(&s, &keys).input.as_deref(), Some("KeyStep 37"));
    assert!(clips(&s, &keys).is_empty());
    assert_eq!(clips(&s, &pads).len(), 1);
    // Plugged in again: Keys records from it with no setting.
    let started = take(
        &mut rig,
        &mut s,
        vec!["loopMIDI Port".into(), "KeyStep 37".into()],
    );
    assert_eq!(started.midi_tracks.len(), 2, "{:?}", started.warnings);
    assert!(started.warnings.is_empty(), "{:?}", started.warnings);
    assert_eq!(clips(&s, &keys).len(), 1);
}

// ---- frozen tracks (REC-025) ---------------------------------------------------------------

/// A frozen track cannot take a clip (its take would be turned away by freeze::check, and the
/// whole `record.take.add` with it). An armed one is left out of the recording with a word,
/// and the other armed tracks' takes go in.
#[test]
fn an_armed_frozen_track_is_left_out_and_the_other_takes_go_in() {
    let dir = Dir::new("frozen");
    let mut s = saved_song(&dir, ExtConfig::default());
    let vox = armed_audio(&mut s, "Vox");
    let keys = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    run(
        &mut s,
        "clip.add.midi",
        json!({ "track": keys, "at": 0, "bars": 1 }),
    );
    let frozen_dir = s.dir().unwrap().join("Freeze");
    std::fs::create_dir_all(&frozen_dir).unwrap();
    write_sine(&frozen_dir.join("Keys.wav"), 1.0, 0.5);
    run(
        &mut s,
        "track.freeze.apply",
        json!({ "tracks": [{ "id": keys, "file": "Freeze/Keys.wav", "start": 0 }] }),
    );
    run(&mut s, "track.arm", json!({ "ids": [keys], "on": true }));
    let mut rig = Rig::new(&s);
    let midi = MockMidi::new(vec![(2_400, [0x90, 60, 100]), (26_400, [0x80, 60, 0])]);
    let started = rig
        .host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                input: Input::Given(Box::new(MockInput::counter())),
                midi: Some(Box::new(midi)),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    assert!(
        started.midi_tracks.is_empty(),
        "the frozen track does not record: {:?}",
        started.midi_tracks
    );
    assert!(
        started
            .warnings
            .iter()
            .any(|w| w.contains("Keys") && w.contains("フリーズ")),
        "{:?}",
        started.warnings
    );
    rig.pull(48_000);
    let r = rig.finish(&mut s);
    assert!(r.step.is_some(), "{r:?}");
    assert_eq!(clips(&s, &vox).len(), 1, "the audio take went in");
    assert_eq!(
        clips(&s, &keys).len(),
        1,
        "the frozen track keeps only the clip it had"
    );
}

/// Every armed track frozen: nothing is left to record, so the recording is refused with a
/// word the same way as with nothing armed.
#[test]
fn a_recording_with_only_frozen_tracks_armed_is_refused() {
    let dir = Dir::new("frozen-only");
    let mut s = saved_song(&dir, ExtConfig::default());
    let keys = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    run(
        &mut s,
        "clip.add.midi",
        json!({ "track": keys, "at": 0, "bars": 1 }),
    );
    let frozen_dir = s.dir().unwrap().join("Freeze");
    std::fs::create_dir_all(&frozen_dir).unwrap();
    write_sine(&frozen_dir.join("Keys.wav"), 1.0, 0.5);
    run(
        &mut s,
        "track.freeze.apply",
        json!({ "tracks": [{ "id": keys, "file": "Freeze/Keys.wav", "start": 0 }] }),
    );
    run(&mut s, "track.arm", json!({ "ids": [keys], "on": true }));
    let mut rig = Rig::new(&s);
    let e = rig
        .host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                midi: Some(Box::new(MockMidi::new(vec![(0, [0x90, 60, 100])]))),
                ..RecordOptions::default()
            },
        )
        .unwrap_err();
    assert!(e.message.contains("フリーズ"), "{}", e.message);
    assert!(!rig.host.is_recording());
}

// ---- the commands -------------------------------------------------------------------------

#[test]
fn the_record_commands_drive_the_host() {
    let dir = Dir::new("commands");
    let mut s = saved_song(&dir, ExtConfig::default());
    let mut rig = Rig::new(&s);
    let e = commands::run(
        &mut s,
        Some(&mut rig.host),
        "record.start",
        &Map::new(),
        &RunOptions::human(),
    )
    .unwrap_err();
    assert!(e.message.contains("録音準備"), "{}", e.message);

    let vox = armed_audio(&mut s, "Vox");
    rig.host.set_record_sources(Sources {
        audio: Source::Make(Box::new(|| Box::new(MockInput::counter()))),
        midi: None,
    });
    let r = run_host(
        &mut s,
        &mut rig.host,
        "record.start",
        json!({ "at": "3.1.0", "latency": 5 }),
    );
    assert_eq!(r.data["latency"], 240, "5 ms at 48 kHz");
    assert_eq!(r.data["audio_track"], vox.as_str());
    assert!(r.step.is_none(), "starting changes nothing in the song");
    rig.pull(48_000);
    let st = run_host(&mut s, &mut rig.host, "record.status", json!({}));
    assert_eq!(st.data["recording"], true);
    assert!(st.message.starts_with("録音中"), "{}", st.message);

    rig.host.record_stop();
    rig.pull(24_000);
    let r = run_host(&mut s, &mut rig.host, "record.stop", json!({}));
    assert_eq!(r.command, "record.stop");
    assert!(r.step.is_some());
    assert_eq!(r.data["clips"].as_array().unwrap().len(), 1, "{:?}", r.data);
    let c = &clips(&s, &vox)[0];
    assert_eq!(c.start, 7680, "3.1.0");
    let (l, _) = read_wav(&audio_file(&s, c));
    // The first 240 frames of the input went to the song before recording started.
    assert_eq!(l[0], 240.0);

    let st = run_host(&mut s, &mut rig.host, "record.status", json!({}));
    assert_eq!(st.data["recording"], false);
    let e = commands::run(
        &mut s,
        Some(&mut rig.host),
        "record.stop",
        &Map::new(),
        &RunOptions::human(),
    )
    .unwrap_err();
    assert!(e.message.contains("録音していな"), "{}", e.message);
}

/// `xirius mcp`: the extension's own player with a silent output records silence, and the
/// takes it leaves go in as one step.
#[test]
fn the_extension_records_on_its_player_and_places_the_takes_in_one_step() {
    let dir = Dir::new("ext");
    let mut s = saved_song(
        &dir,
        ExtConfig {
            output: OutputMode::Silent { paced: true },
            ..ExtConfig::default()
        },
    );
    let vox = armed_audio(&mut s, "Vox");
    let steps = s.steps().len();
    let r = s
        .run("record.start", &Map::new(), &RunOptions::human())
        .unwrap();
    assert!(r.message.contains("Vox"), "{}", r.message);
    played(&mut s, 9_600);
    let r = s
        .run("record.stop", &Map::new(), &RunOptions::human())
        .unwrap();
    assert!(r.step.is_some(), "{r:?}");
    assert_eq!(s.steps().len(), steps + 1, "one step");
    assert_eq!(r.data["clips"].as_array().unwrap().len(), 1, "{:?}", r.data);
    let path = audio_file(&s, &clips(&s, &vox)[0]);
    let (l, _) = read_wav(&path);
    assert!(l.len() > 4_800, "{}", l.len());
    assert!(l.iter().all(|x| *x == 0.0));

    s.run("history.undo", &Map::new(), &RunOptions::human())
        .unwrap();
    assert!(clips(&s, &vox).is_empty());
    assert!(path.exists());
}

// ---- inputs -------------------------------------------------------------------------------

#[test]
fn a_track_input_names_channels_or_a_midi_port() {
    use record::{input_channels, midi_port};
    assert_eq!(input_channels(None, 8), Ok(vec![0, 1]));
    assert_eq!(input_channels(None, 1), Ok(vec![0]));
    assert_eq!(input_channels(Some("In 3"), 8), Ok(vec![2]));
    assert_eq!(input_channels(Some("input 3+4"), 8), Ok(vec![2, 3]));
    assert_eq!(input_channels(Some("入力 5/6"), 8), Ok(vec![4, 5]));
    assert_eq!(input_channels(Some("7-8"), 8), Ok(vec![6, 7]));
    assert!(input_channels(Some("In 9"), 8).is_err());
    assert!(input_channels(Some("In 0"), 8).is_err());
    assert!(input_channels(Some("In 1+2+3"), 8).is_err());
    assert!(input_channels(Some("Mic"), 8).is_err());

    let ports = vec![
        "Keystation 49".to_string(),
        "Launchpad".to_string(),
        "Launchkey".to_string(),
    ];
    assert_eq!(midi_port(None, &ports), Ok(None));
    assert_eq!(midi_port(Some("all"), &ports), Ok(None));
    assert_eq!(midi_port(Some("Launchpad"), &ports), Ok(Some(1)));
    assert_eq!(midi_port(Some("launchkey"), &ports), Ok(Some(2)));
    assert_eq!(midi_port(Some("station"), &ports), Ok(Some(0)));
    assert!(midi_port(Some("launch"), &ports).is_err(), "two ports");
    assert!(midi_port(Some("Pads"), &ports).is_err());
    assert_eq!(
        midi_port(Some("Pads"), &[]),
        Ok(None),
        "ports without names"
    );
}

#[test]
fn a_count_in_waits_the_bars_out_and_the_take_starts_with_the_song() {
    let dir = Dir::new("count-in");
    let mut s = saved_song(&dir, ExtConfig::default());
    let vox = armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    let started = rig
        .host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(1920),
                latency: Some(0),
                input: Input::Given(Box::new(MockInput::counter())),
                count_in_bars: 1,
                ..RecordOptions::default()
            },
        )
        .unwrap();
    // 120 BPM in 4/4: a bar is two seconds.
    assert_eq!(started.count_in, 96_000);
    let at = rig.frame_of(1920);
    let played = rig.pull(48_000);
    // The count clicks on the device output; the song's clock has not moved.
    assert!(played.iter().any(|x| x.abs() > 0.05), "the click is heard");
    assert_eq!(rig.audio.song_position(), at);
    rig.pull(96_000);
    assert!(rig.audio.song_position() > at);
    rig.finish(&mut s);
    let c = &clips(&s, &vox)[0];
    assert_eq!(c.start, 1920, "the take starts where the song began");
    let (l, _) = read_wav(&audio_file(&s, c));
    // The first frame is the input of the moment the count was over.
    assert_eq!(l[0], 96_000.0);
}

#[test]
fn a_recording_without_a_count_in_does_not_wait() {
    let dir = Dir::new("no-count-in");
    let mut s = saved_song(&dir, ExtConfig::default());
    armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    let started = rig
        .host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                input: Input::Given(Box::new(MockInput::counter())),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    assert_eq!(started.count_in, 0);
    rig.pull(4_800);
    assert!(rig.audio.song_position() >= 4_800);
    rig.finish(&mut s);
}

// ---- punch and input quantize (REC-007, REC-009) ------------------------------------------

#[test]
fn only_the_punch_range_of_a_take_goes_into_the_song_and_it_names_where_in_its_file() {
    let dir = Dir::new("punch");
    let mut s = saved_song(&dir, ExtConfig::default());
    let vox = armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    rig.host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                input: Input::Given(Box::new(MockInput::counter())),
                keep: Some((1920, Some(3840))),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    // 2 s: the song is at tick 3840, the end of the range.
    rig.pull(96_000);
    let r = rig.finish(&mut s);
    assert!(r.step.is_some(), "{r:?}");
    let cs = clips(&s, &vox);
    assert_eq!(cs.len(), 1);
    assert_eq!((cs[0].start, cs[0].length), (1920, 1920));
    let ClipBody::Audio { offset, .. } = &cs[0].body else {
        panic!("not audio")
    };
    assert_eq!(*offset, 1920);
    // The file holds the whole capture, and one undo takes the clip out.
    let (file, _) = read_wav(&audio_file(&s, &cs[0]));
    assert!(file.len() >= 96_000, "{}", file.len());
    run(&mut s, "history.undo", json!({}));
    assert!(clips(&s, &vox).is_empty());
}

#[test]
fn a_take_with_nothing_inside_the_range_is_not_a_clip() {
    let dir = Dir::new("punch-out");
    let mut s = saved_song(&dir, ExtConfig::default());
    let vox = armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    rig.host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                input: Input::Given(Box::new(MockInput::counter())),
                keep: Some((7680, Some(9600))),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    rig.pull(48_000);
    let r = rig.finish(&mut s);
    assert!(r.step.is_none(), "{r:?}");
    assert!(clips(&s, &vox).is_empty());
    assert!(
        r.message.contains("範囲の外"),
        "the outcome says why: {}",
        r.message
    );
}

#[test]
fn notes_snap_their_heads_to_the_grid_and_keep_their_lengths_and_one_undo_removes_them() {
    let mut s = Session::with_registry(Project::new("quantize"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    let keys = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    run(&mut s, "track.arm", json!({ "ids": [keys], "on": true }));
    let mut rig = Rig::new(&s);
    // Ticks of 25 frames: the heads are at ticks 96 and 600, the lengths 960 and 600.
    let midi = MockMidi::new(vec![
        (2_400, [0x90, 60, 100]),
        (26_400, [0x80, 60, 0]),
        (15_000, [0x90, 64, 80]),
        (30_000, [0x80, 64, 0]),
    ]);
    rig.host
        .record_start(
            s.project(),
            None,
            RecordOptions {
                at: Some(3840),
                midi: Some(Box::new(midi)),
                quantize: Some(240),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    rig.pull(72_000);
    rig.finish(&mut s);
    let c = &clips(&s, &keys)[0];
    assert_eq!(c.start, 3840);
    let ClipBody::Midi { notes, .. } = &c.body else {
        panic!("not a MIDI clip")
    };
    let got: Vec<(i64, i64, u8)> = notes.iter().map(|n| (n.start, n.length, n.pitch)).collect();
    assert_eq!(got, [(0, 960, 60), (720, 600, 64)]);
    run(&mut s, "history.undo", json!({}));
    assert!(
        clips(&s, &keys).is_empty(),
        "the whole take goes in one step"
    );
}

#[test]
fn record_status_tells_the_panel_and_where_the_recording_stands() {
    let dir = Dir::new("status");
    let mut s = saved_song(&dir, ExtConfig::default());
    let _vox = armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    let r = run_host(&mut s, &mut rig.host, "record.status", json!({}));
    assert_eq!(r.data["recording"], false);
    assert_eq!(r.data["phase"], "idle");
    rig.host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                input: Input::Given(Box::new(MockInput::counter())),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    rig.pull(4_800);
    let r = run_host(&mut s, &mut rig.host, "record.status", json!({}));
    assert_eq!(r.data["phase"], "recording");
    rig.finish(&mut s);
}

#[test]
fn the_post_roll_is_due_once_the_song_has_played_to_its_end() {
    let dir = Dir::new("due");
    let mut s = saved_song(&dir, ExtConfig::default());
    let _vox = armed_audio(&mut s, "Vox");
    let mut rig = Rig::new(&s);
    rig.host
        .record_start(
            s.project(),
            s.dir(),
            RecordOptions {
                at: Some(0),
                input: Input::Given(Box::new(MockInput::counter())),
                stop_at: Some(1920),
                ..RecordOptions::default()
            },
        )
        .unwrap();
    rig.pull(24_000);
    assert!(!rig.host.record_due());
    rig.pull(48_000);
    assert!(rig.host.record_due());
    rig.finish(&mut s);
}

#[test]
fn the_record_offsets_in_the_settings_move_the_takes() {
    let dir = Dir::new("offsets");
    let mut s = saved_song(&dir, ExtConfig::default());
    let mut rig = Rig::new(&s);
    let mut settings = xirius_prefs::Settings::memory();
    settings
        .prefs
        .set("audio.record-offset", &json!("2"))
        .unwrap();
    settings
        .prefs
        .set("midi.input-offset", &json!("20"))
        .unwrap();
    rig.host.io_settings = Some(settings.shared());

    // Audio: 2 ms (96 frames) of the 5 ms latency is taken back, so the take starts 96 frames
    // of the input later than without the setting (240).
    let vox = armed_audio(&mut s, "Vox");
    rig.host.set_record_sources(Sources {
        audio: Source::Make(Box::new(|| Box::new(MockInput::counter()))),
        midi: None,
    });
    run_host(
        &mut s,
        &mut rig.host,
        "record.start",
        json!({ "at": "3.1.0", "latency": 5 }),
    );
    rig.pull(48_000);
    rig.host.record_stop();
    rig.pull(24_000);
    run_host(&mut s, &mut rig.host, "record.stop", json!({}));
    let (l, _) = read_wav(&audio_file(&s, &clips(&s, &vox)[0]));
    assert_eq!(l[0], 144.0);

    // MIDI: 20 ms (960 frames, 38.4 ticks of 25 frames) later than where it was played.
    run(&mut s, "track.arm", json!({ "ids": [vox], "on": false }));
    let keys = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    run(&mut s, "track.arm", json!({ "ids": [keys], "on": true }));
    rig.host.set_record_sources(Sources {
        audio: Source::None,
        midi: Some(Box::new(|| {
            Box::new(MockMidi::new(vec![
                (2_400, [0x90, 60, 100]),
                (26_400, [0x80, 60, 0]),
            ]))
        })),
    });
    run_host(
        &mut s,
        &mut rig.host,
        "record.start",
        json!({ "at": "2.1.0" }),
    );
    rig.pull(48_000);
    rig.host.record_stop();
    rig.pull(24_000);
    run_host(&mut s, &mut rig.host, "record.stop", json!({}));
    assert_eq!(clips(&s, &keys)[0].start, 3840 + 38);
}
