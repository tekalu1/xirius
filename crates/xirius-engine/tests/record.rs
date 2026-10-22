//! Recording (REC-001, REC-004, REC-005, ENG-020): what the input heard lands in the file at the
//! song frame it was played at, the round trip is taken off, loop laps become takes of their own,
//! MIDI lands on its frames, and a long recording at full speed never holds the audio thread up.
//! The inputs are stand-ins; no device is opened.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use xirius_engine::engine::{self, AudioEngine, Controller, EngineConfig};
use xirius_engine::record::{
    AudioLane, MockInput, MockMidi, RecordSpec, Recorder, Recording, TakeMidi,
};
use xirius_engine::{
    AudioData, AudioIo, Command, MixSpec, NoteEvent, Plan, ProcessSetup, Processor, Region,
    RenderMode, Timeline, TrackSlot, TrackSpec,
};

const RATE: u32 = 48_000;
const BLOCK: usize = 256;

struct Silence;

impl Processor for Silence {
    fn name(&self) -> &str {
        "silence"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.clear();
    }
}

fn setup(mode: RenderMode) -> ProcessSetup {
    ProcessSetup {
        sample_rate: f64::from(RATE),
        max_block: BLOCK,
        mode,
    }
}

/// One track playing `timeline`.
fn engine_with(timeline: Timeline, mode: RenderMode) -> (AudioEngine, Controller) {
    let mut t = TrackSpec::new("audio", Box::new(Silence));
    t.timeline = timeline;
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(t)],
        ..MixSpec::default()
    };
    engine::create(
        Plan::build(spec, setup(mode), None).unwrap(),
        EngineConfig::default(),
    )
}

fn send(c: &mut Controller, cmd: Command) {
    assert!(c.send(cmd).is_ok(), "the command queue is full");
}

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("xirius-record-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn spec(dir: &Path, latency: u64, align: u64) -> RecordSpec {
    let dir = dir.to_path_buf();
    RecordSpec {
        sample_rate: RATE,
        lanes: vec![AudioLane {
            channels: vec![0, 1],
            path: Box::new(move |k| dir.join(format!("take-{k}.wav"))),
        }],
        midi: false,
        latency,
        midi_latency: 0,
        align: Box::new(move |f| f.div_ceil(align) * align),
        ring_seconds: 2.0,
        thru: Vec::new(),
    }
}

/// Renders `frames` frames in device callbacks of `callback` frames; returns the interleaved
/// output.
fn run(audio: &mut AudioEngine, c: &mut Controller, frames: usize, callback: usize) -> Vec<f32> {
    let mut all = Vec::with_capacity(frames * 2);
    let mut out = vec![0.0f32; callback * 2];
    let mut left = frames;
    while left > 0 {
        let n = left.min(callback);
        audio.render(&mut out[..n * 2], 2);
        all.extend_from_slice(&out[..n * 2]);
        c.poll(|_| {});
        left -= n;
    }
    all
}

/// Stops the transport and the recording, renders until the capture lets go, and finishes.
fn stop(audio: &mut AudioEngine, c: &mut Controller, rec: Recorder) -> Recording {
    send(c, Command::Stop);
    send(c, Command::Record(None));
    for _ in 0..1000 {
        run(audio, c, BLOCK, BLOCK);
        if rec.detached() {
            break;
        }
    }
    let r = rec.finish(Duration::from_secs(5));
    assert!(r.detached, "the audio side let go");
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    r
}

fn read_wav(p: &Path) -> (Vec<f32>, Vec<f32>) {
    let mut r = hound::WavReader::open(p).unwrap();
    assert_eq!(r.spec().channels, 2);
    assert_eq!(r.spec().sample_rate, RATE);
    let s: Vec<f32> = r.samples::<f32>().map(Result::unwrap).collect();
    (
        s.iter().step_by(2).copied().collect(),
        s.iter().skip(1).step_by(2).copied().collect(),
    )
}

#[test]
fn a_writer_creation_failure_is_visible_while_capture_is_attached() {
    let dir = Dir::new("writer-failure");
    let (mut audio, mut controller) = engine_with(Timeline::default(), RenderMode::Offline);
    let (recorder, capture) = Recorder::start(spec(&dir.0.join("missing"), 0, 1)).unwrap();
    send(&mut controller, Command::Record(Some(capture)));
    send(&mut controller, Command::Play);
    run(&mut audio, &mut controller, BLOCK, BLOCK);
    let deadline = Instant::now() + Duration::from_secs(30);
    while recorder.writer_error().is_none() {
        assert!(
            Instant::now() < deadline,
            "the writer failure was not reported"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(recorder.writer_error().unwrap().contains("作成できません"));
    assert!(
        !recorder.disk_space_low(),
        "a missing directory is not a disk-full error"
    );
    send(&mut controller, Command::Record(None));
    run(&mut audio, &mut controller, BLOCK, BLOCK);
    let result = recorder.finish(Duration::from_secs(5));
    assert!(!result.errors.is_empty());
    assert!(result.takes.iter().all(|t| t.audio.is_empty()));
}

#[test]
fn the_file_starts_at_the_aligned_song_frame_and_holds_every_frame() {
    let d = Dir::new("counter");
    let (mut audio, mut c) = engine_with(Timeline::default(), RenderMode::Offline);
    let (rec, capture) = Recorder::start(spec(&d.0, 0, 7)).unwrap();
    send(
        &mut c,
        Command::SetInput(Some(Box::new(MockInput::counter()))),
    );
    send(&mut c, Command::Record(Some(capture)));
    send(&mut c, Command::Seek(1000));
    send(&mut c, Command::Play);
    // Callbacks that do not divide the block, so blocks are cut in odd places.
    run(&mut audio, &mut c, 5000, 333);
    let r = stop(&mut audio, &mut c, rec);

    assert_eq!(r.takes.len(), 1, "{:?}", r.takes);
    let t = &r.takes[0];
    assert_eq!((t.song_start, t.song_end), (1000, 6000));
    assert_eq!(t.audio.len(), 1);
    let a = &t.audio[0];
    // 1000 is not on the grid of 7; the take starts on 1001, one frame into the input.
    assert_eq!(a.start, 1001);
    assert_eq!(a.frames, 4999);
    let (l, rr) = read_wav(&a.path);
    assert_eq!(l.len(), 4999);
    for (j, (x, y)) in l.iter().zip(&rr).enumerate() {
        let want = (j + 1) as f32;
        assert_eq!((*x, *y), (want, -want), "file frame {j}");
    }
    assert_eq!(r.dropped, 0);
}

/// A ramp the track plays from song frame 0: frame `i` is `(i + 1) / 2^20`.
fn ramp(len: usize) -> Timeline {
    let v: Vec<f32> = (0..len).map(|i| (i + 1) as f32 / 1_048_576.0).collect();
    let data = Arc::new(AudioData {
        left: v.clone(),
        right: v,
    });
    Timeline::new(
        vec![],
        vec![Region {
            start: 0,
            len: len as u64,
            offset: 0,
            gain: 1.0,
            data,
            shape: Default::default(),
        }],
    )
}

/// The output goes back into the input `delay` frames later; recording with that latency, the
/// file matches what was played at the same song frames.
fn loopback(delay: usize, latency: u64) -> (Vec<f32>, Vec<f32>, u64) {
    let d = Dir::new(&format!("loop-{delay}-{latency}"));
    let (mut audio, mut c) = engine_with(ramp(20_000), RenderMode::Offline);
    let (rec, capture) = Recorder::start(spec(&d.0, latency, 1)).unwrap();
    send(
        &mut c,
        Command::SetInput(Some(Box::new(MockInput::loopback(delay, BLOCK)))),
    );
    send(&mut c, Command::Record(Some(capture)));
    send(&mut c, Command::Play);
    let out = run(&mut audio, &mut c, 10_000, 480);
    let r = stop(&mut audio, &mut c, rec);
    let a = r.takes[0].audio[0].clone();
    assert_eq!(a.start, 0);
    let (l, _) = read_wav(&a.path);
    let played: Vec<f32> = out.iter().step_by(2).copied().collect();
    (l, played, a.frames)
}

#[test]
fn the_round_trip_is_taken_off() {
    let delay = 333;
    let (file, played, frames) = loopback(delay, delay as u64);
    // The capture runs `latency` frames past the stop, so the take is as long as what was played.
    assert_eq!(frames, 10_000);
    assert_eq!(file.len(), 10_000);
    for i in 0..10_000 {
        assert_eq!(file[i], played[i], "song frame {i}");
    }

    // Without the correction the take is late by the round trip.
    let (file, played, _) = loopback(delay, 0);
    assert_eq!(file[..delay], vec![0.0; delay][..]);
    for i in 0..10_000 - delay {
        assert_eq!(file[i + delay], played[i], "song frame {i}");
    }
}

#[test]
fn each_loop_lap_is_a_take_of_its_own() {
    let d = Dir::new("laps");
    let (mut audio, mut c) = engine_with(Timeline::default(), RenderMode::Offline);
    let (rec, capture) = Recorder::start(spec(&d.0, 0, 1)).unwrap();
    send(
        &mut c,
        Command::SetInput(Some(Box::new(MockInput::counter()))),
    );
    send(&mut c, Command::Record(Some(capture)));
    send(&mut c, Command::SetLoop(Some((2000, 6000))));
    send(&mut c, Command::Seek(2000));
    send(&mut c, Command::Play);
    run(&mut audio, &mut c, 10_000, 512);
    let r = stop(&mut audio, &mut c, rec);

    let spans: Vec<_> = r.takes.iter().map(|t| (t.song_start, t.song_end)).collect();
    assert_eq!(spans, vec![(2000, 6000), (2000, 6000), (2000, 4000)]);
    let mut paths = Vec::new();
    for (k, t) in r.takes.iter().enumerate() {
        assert_eq!(t.index, k);
        let a = &t.audio[0];
        assert_eq!(a.start, 2000);
        let (l, _) = read_wav(&a.path);
        assert_eq!(l.len() as u64, t.song_end - t.song_start);
        // Lap k began at capture frame 4000 k.
        assert_eq!(l[0], (k * 4000) as f32);
        assert_eq!(*l.last().unwrap(), (k * 4000 + l.len() - 1) as f32);
        paths.push(a.path.clone());
    }
    paths.dedup();
    assert_eq!(paths.len(), 3, "one file per lap");
}

/// ENG-040: an input that stopped (unplugged) before the recording did. The takes end at the
/// last frame it delivered: the lap it stopped in is cut there, a lap after it is no take and
/// its file is gone, and the round trip is taken off the end as off the start.
#[test]
fn a_take_ends_at_the_last_frame_an_input_that_stopped_delivered() {
    let d = Dir::new("stopped-laps");
    let (mut audio, mut c) = engine_with(Timeline::default(), RenderMode::Offline);
    let (rec, capture) = Recorder::start(spec(&d.0, 0, 1)).unwrap();
    let input = MockInput::counter().stopping_after(5000);
    send(&mut c, Command::SetInput(Some(Box::new(input))));
    send(&mut c, Command::Record(Some(capture)));
    send(&mut c, Command::SetLoop(Some((2000, 6000))));
    send(&mut c, Command::Seek(2000));
    send(&mut c, Command::Play);
    run(&mut audio, &mut c, 10_000, 512);
    rec.end_at_last_input();
    let r = stop(&mut audio, &mut c, rec);
    let spans: Vec<_> = r.takes.iter().map(|t| (t.song_start, t.song_end)).collect();
    assert_eq!(spans, vec![(2000, 6000), (2000, 3000)]);
    let (l, _) = read_wav(&r.takes[1].audio[0].path);
    assert_eq!(r.takes[1].audio[0].frames, 1000);
    assert_eq!((l.len(), l[0], *l.last().unwrap()), (1000, 4000.0, 4999.0));
    assert!(!d.0.join("take-2.wav").exists(), "the lap after it is gone");

    let d = Dir::new("stopped-trip");
    let (mut audio, mut c) = engine_with(Timeline::default(), RenderMode::Offline);
    let (rec, capture) = Recorder::start(spec(&d.0, 300, 1)).unwrap();
    let input = MockInput::counter().stopping_after(5000);
    send(&mut c, Command::SetInput(Some(Box::new(input))));
    send(&mut c, Command::Record(Some(capture)));
    send(&mut c, Command::Play);
    run(&mut audio, &mut c, 10_000, 512);
    rec.end_at_last_input();
    let r = stop(&mut audio, &mut c, rec);
    assert_eq!(r.takes.len(), 1);
    assert_eq!((r.takes[0].song_start, r.takes[0].song_end), (0, 4700));
    let (l, _) = read_wav(&r.takes[0].audio[0].path);
    assert_eq!((l.len(), l[0], *l.last().unwrap()), (4700, 300.0, 4999.0));
}

#[test]
fn midi_lands_on_the_song_frames_it_was_played_at() {
    let d = Dir::new("midi");
    let (mut audio, mut c) = engine_with(Timeline::default(), RenderMode::Offline);
    let mut s = spec(&d.0, 0, 1);
    s.lanes.clear();
    s.midi = true;
    s.midi_latency = 100;
    let (rec, capture) = Recorder::start(s).unwrap();
    let events = vec![
        (600, [0x90, 60, 100]),
        (1100, [0x80, 60, 0]),
        (1200, [0x91, 64, 64]),
        (5000, [0x90, 64, 0]),
        (50, [0x90, 1, 1]), // before the take (100 frames of latency)
    ];
    send(
        &mut c,
        Command::SetMidiInput(Some(Box::new(MockMidi::new(events)))),
    );
    send(&mut c, Command::Record(Some(capture)));
    send(&mut c, Command::Seek(48_000));
    send(&mut c, Command::Play);
    run(&mut audio, &mut c, 6000, 256);
    let r = stop(&mut audio, &mut c, rec);

    assert_eq!(r.takes.len(), 1);
    let t = &r.takes[0];
    assert!(t.audio.is_empty());
    assert_eq!((t.song_start, t.song_end), (48_000, 54_000));
    // Each message keeps its frame inside the block; the latency is taken off.
    let at = |f: u64| 48_000 + f - 100;
    assert_eq!(
        t.midi.iter().map(|m| (m.at, m.msg)).collect::<Vec<_>>(),
        vec![
            (at(600), [0x90, 60, 100]),
            (at(1100), [0x80, 60, 0]),
            (at(1200), [0x91, 64, 64]),
            (at(5000), [0x90, 64, 0]),
        ]
    );
}

#[test]
fn ten_minutes_at_full_speed_never_hold_the_audio_thread() {
    let d = Dir::new("long");
    let (mut audio, mut c) = engine_with(Timeline::default(), RenderMode::Realtime);
    // The disk hangs for the first 20 seconds: the writer stops in the first file's name until
    // it is let go. A callback that waited for the writer would never get to the 20th second.
    let (go, held) = std::sync::mpsc::channel::<()>();
    let (stuck, writer_stuck) = std::sync::mpsc::channel::<()>();
    let mut gate = Some((stuck, held));
    let mut s = spec(&d.0, 0, 1);
    let dir = d.0.clone();
    s.lanes[0].path = Box::new(move |k| {
        if let Some((stuck, held)) = gate.take() {
            let _ = stuck.send(());
            let _ = held.recv();
        }
        dir.join(format!("take-{k}.wav"))
    });
    // If the render does stop, the watchdog lets the writer go so the test fails, not hangs.
    // It goes by the render's progress, not the clock: on a busy machine the ten minutes may take
    // long, but a callback that waits for the held writer never comes back at all.
    let (done, watch) = std::sync::mpsc::channel::<()>();
    let rescued = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicUsize::new(0));
    let watchdog = {
        let (go, rescued, progress) = (go.clone(), rescued.clone(), progress.clone());
        std::thread::spawn(move || {
            let mut seen = (0, Instant::now());
            while watch.recv_timeout(Duration::from_millis(100)).is_err() {
                let now = progress.load(Ordering::SeqCst);
                if now != seen.0 {
                    seen = (now, Instant::now());
                } else if seen.1.elapsed() > Duration::from_secs(60) {
                    rescued.store(true, Ordering::SeqCst);
                    let _ = go.send(());
                    return;
                }
            }
        })
    };
    let (rec, capture) = Recorder::start(s).unwrap();
    send(
        &mut c,
        Command::SetInput(Some(Box::new(MockInput::sine(f64::from(RATE), 440.0, 0.5)))),
    );
    send(&mut c, Command::Record(Some(capture)));
    send(&mut c, Command::Play);
    let frames = RATE as usize * 600;
    let hold = RATE as usize * 20;
    let calls = frames / BLOCK;
    let mut out = vec![0.0f32; BLOCK * 2];
    let budget = Duration::from_secs_f64(BLOCK as f64 / f64::from(RATE));
    let (mut slowest, mut over) = (Duration::ZERO, 0usize);
    let mut times = Vec::with_capacity(calls);
    // After the hang, the render waits (between callbacks) until one block gets into the ring
    // again: the writer is far slower than a render at full speed, so without the wait it might
    // never find room before the end.
    let mut back = None;
    let started = Instant::now();
    for k in 0..calls {
        if k == hold / BLOCK {
            go.send(()).unwrap();
        }
        let dropped = rec.dropped();
        let t = Instant::now();
        audio.render_timed(&mut out, 2);
        let took = t.elapsed();
        slowest = slowest.max(took);
        over += usize::from(took >= budget);
        times.push(took);
        progress.store(k + 1, Ordering::SeqCst);
        c.poll(|_| {});
        // The writer takes the first block and gets stuck in its file's name; from there the
        // ring fills and the rest of the 20 seconds is dropped, however late the writer started.
        if k == 0 {
            writer_stuck
                .recv_timeout(Duration::from_secs(60))
                .expect("the writer got to the file");
        }
        if k >= hold / BLOCK && back.is_none() {
            if rec.dropped() == dropped {
                back = Some(k * BLOCK);
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    let rendered = started.elapsed();
    let _ = done.send(());
    watchdog.join().unwrap();
    let r = stop(&mut audio, &mut c, rec);
    times.sort();
    let median = times[calls / 2];
    let misses = audio.stats().snapshot().deadline_misses;
    let a = r.takes[0].audio[0].clone();
    let mut wav = hound::WavReader::open(&a.path).unwrap();
    let file_frames = u64::from(wav.duration());
    eprintln!(
        "10 min: rendered in {rendered:?}, median callback {median:?}, slowest {slowest:?} \
         (budget {budget:?}), {over} of {calls} over budget, {misses} deadline misses, \
         dropped {} frames, back at {back:?}, file {file_frames} frames",
        r.dropped
    );
    // A callback never waits for the disk: all 20 held seconds were rendered while the writer
    // was stuck, and it was let go by the render, not by the watchdog.
    assert!(
        !rescued.load(Ordering::SeqCst),
        "the render stopped while the writer was held"
    );
    // While the writer was held, what did not fit in the 2-second ring was dropped; after it,
    // the input got in again.
    let ring = RATE as u64 * 2;
    let back = back.expect("a block got into the ring after the hang");
    assert!(
        r.dropped >= hold as u64 - ring - BLOCK as u64 * 2
            && r.dropped <= (frames - back) as u64 + hold as u64,
        "dropped {}",
        r.dropped
    );
    assert_eq!(r.lost_marks, 0);
    assert!(!r.dropouts.is_empty());
    assert_eq!(r.dropouts.iter().map(|d| d.frames).sum::<u64>(), r.dropped);
    assert_eq!(
        r.dropouts.iter().map(|d| d.blocks).sum::<u64>(),
        r.dropped_blocks
    );
    assert!(
        r.dropouts
            .iter()
            .all(|d| d.song_frame == Some(d.capture_frame))
    );
    // Dropped blocks are silence of the same length, so the file keeps its length and place:
    // the input in the ring, silence where it was dropped, the input again after the hang.
    assert_eq!(a.frames, frames as u64);
    assert_eq!(file_frames, frames as u64);
    let mut loudest = |at: usize| {
        wav.seek(at as u32).unwrap();
        wav.samples::<f32>()
            .take(BLOCK * 2)
            .map(Result::unwrap)
            .fold(0.0f32, |m, x| m.max(x.abs()))
    };
    let rate = RATE as usize;
    assert!(loudest(rate) > 0.4, "the first second is in the ring");
    assert_eq!(loudest(rate * 10), 0.0, "the 10th second was dropped");
    assert!(
        loudest(back) > 0.4,
        "the input after the hang is in its place"
    );
    // The callbacks keep inside their time. A callback that waited for the disk would be late
    // whenever the writer is busy, which is all the time here: the median would be. How many
    // are late on the clock is not asserted (only printed above): a callback measured on the
    // clock also holds the time the OS gives to other work, which on a busy machine lands on
    // one callback of every time slice, however short the callbacks are.
    assert!(median * 4 < budget, "median callback {median:?}");
    assert!(rendered < Duration::from_secs(600), "faster than real time");
}

/// Every channel of a file.
fn read_columns(p: &Path) -> Vec<Vec<f32>> {
    let mut r = hound::WavReader::open(p).unwrap();
    let ch = usize::from(r.spec().channels);
    let s: Vec<f32> = r.samples::<f32>().map(Result::unwrap).collect();
    (0..ch)
        .map(|c| s.iter().skip(c).step_by(ch).copied().collect())
        .collect()
}

#[test]
fn several_lanes_record_their_own_channels_on_the_same_frames() {
    let d = Dir::new("lanes");
    let (mut audio, mut c) = engine_with(Timeline::default(), RenderMode::Offline);
    let lane = |name: &str, channels: Vec<usize>| {
        let (dir, name) = (d.0.clone(), name.to_string());
        AudioLane {
            channels,
            path: Box::new(move |k| dir.join(format!("{name}-{k}.wav"))),
        }
    };
    let mut s = spec(&d.0, 50, 7);
    // In 1 (mono), In 3/4 (stereo), In 1 again (the same channel on a second track), and a
    // channel the interface does not have.
    s.lanes = vec![
        lane("vox", vec![0]),
        lane("keys", vec![2, 3]),
        lane("vox2", vec![0]),
        lane("none", vec![9]),
    ];
    s.midi = true;
    s.midi_latency = 50;
    let (rec, capture) = Recorder::start(s).unwrap();
    let ports = vec!["Keys".to_string(), "Pads".to_string()];
    let events = vec![(700, 0, [0x90, 60, 100]), (900, 1, [0x90, 72, 90])];
    send(
        &mut c,
        Command::SetInput(Some(Box::new(MockInput::counters(4)))),
    );
    send(
        &mut c,
        Command::SetMidiInput(Some(Box::new(MockMidi::with_ports(ports, events)))),
    );
    send(&mut c, Command::Record(Some(capture)));
    send(&mut c, Command::Seek(1000));
    send(&mut c, Command::Play);
    run(&mut audio, &mut c, 5000, 333);
    let r = stop(&mut audio, &mut c, rec);

    assert_eq!(r.takes.len(), 1, "{:?}", r.takes);
    let t = &r.takes[0];
    assert_eq!((t.song_start, t.song_end), (1000, 6000));
    let lanes: Vec<usize> = t.audio.iter().map(|a| a.lane).collect();
    assert_eq!(lanes, vec![0, 1, 2, 3]);
    let widths: Vec<usize> = t
        .audio
        .iter()
        .map(|a| read_columns(&a.path).len())
        .collect();
    assert_eq!(widths, vec![1, 2, 1, 1]);
    for a in &t.audio {
        // Every lane starts on the same aligned frame and is as long as the others.
        assert_eq!((a.start, a.frames), (1001, 4999), "{a:?}");
    }
    // Song frame 1001 is capture frame 1 + 50 frames of latency.
    let first = 51.0;
    let vox = read_columns(&t.audio[0].path);
    let keys = read_columns(&t.audio[1].path);
    let vox2 = read_columns(&t.audio[2].path);
    let none = read_columns(&t.audio[3].path);
    for j in 0..4999 {
        let n = first + j as f32;
        assert_eq!(vox[0][j], n, "vox frame {j}");
        assert_eq!(keys[0][j], n + 2_000_000.0, "keys left frame {j}");
        assert_eq!(keys[1][j], n + 3_000_000.0, "keys right frame {j}");
        assert_eq!(vox2[0][j], n, "vox2 frame {j}");
        assert_eq!(none[0][j], 0.0, "a channel the input lacks is silence");
    }
    let names: std::collections::HashSet<_> = t.audio.iter().map(|a| a.path.clone()).collect();
    assert_eq!(names.len(), 4, "one file per lane");
    // Each message keeps its port.
    assert_eq!(
        t.midi,
        vec![
            TakeMidi {
                at: 1000 + 700 - 50,
                port: 0,
                msg: [0x90, 60, 100],
            },
            TakeMidi {
                at: 1000 + 900 - 50,
                port: 1,
                msg: [0x90, 72, 90],
            },
        ]
    );
}

#[test]
fn every_lane_gets_a_file_per_loop_lap() {
    let d = Dir::new("lane-laps");
    let (mut audio, mut c) = engine_with(Timeline::default(), RenderMode::Offline);
    let mut s = spec(&d.0, 0, 1);
    let dir = d.0.clone();
    s.lanes.push(AudioLane {
        channels: vec![1],
        path: Box::new(move |k| dir.join(format!("mono-{k}.wav"))),
    });
    let (rec, capture) = Recorder::start(s).unwrap();
    send(
        &mut c,
        Command::SetInput(Some(Box::new(MockInput::counter()))),
    );
    send(&mut c, Command::Record(Some(capture)));
    send(&mut c, Command::SetLoop(Some((2000, 6000))));
    send(&mut c, Command::Seek(2000));
    send(&mut c, Command::Play);
    run(&mut audio, &mut c, 10_000, 512);
    let r = stop(&mut audio, &mut c, rec);

    assert_eq!(r.takes.len(), 3);
    for (k, t) in r.takes.iter().enumerate() {
        assert_eq!(t.audio.len(), 2, "lap {k}");
        let stereo = read_columns(&t.audio[0].path);
        let mono = read_columns(&t.audio[1].path);
        assert_eq!(mono.len(), 1);
        assert_eq!(stereo[0].len(), mono[0].len());
        // The mono lane recorded the right channel (-n) of the same frames.
        let first = (k * 4000) as f32;
        assert_eq!((stereo[0][0], mono[0][0]), (first, -first));
    }
}
