//! The input monitor, the input level and the input trim (REC-002, REC-011, REC-012): what the
//! armed track's input hears is added to the device output, measured per channel, and scaled by
//! the trim before it is recorded. The input is a stand-in and the output a buffer; no device is
//! opened.

use std::path::PathBuf;
use std::time::Duration;

use xirius_engine::engine::{self, AudioEngine, Controller, EngineConfig};
use xirius_engine::record::{AudioLane, MockInput, RecordSpec, Recorder};
use xirius_engine::{
    AudioIo, Command, MixSpec, NoteEvent, Plan, ProcessSetup, Processor, RenderMode, TrackSlot,
    TrackSpec,
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

fn engine() -> (AudioEngine, Controller) {
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(TrackSpec::new("audio", Box::new(Silence)))],
        ..MixSpec::default()
    };
    let setup = ProcessSetup {
        sample_rate: f64::from(RATE),
        max_block: BLOCK,
        mode: RenderMode::Offline,
    };
    engine::create(
        Plan::build(spec, setup, None).unwrap(),
        EngineConfig::default(),
    )
}

fn render(audio: &mut AudioEngine, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    for chunk in out.chunks_mut(480 * 2) {
        audio.render(chunk, 2);
    }
    out
}

fn peak(out: &[f32], side: usize) -> f32 {
    out.iter()
        .skip(side)
        .step_by(2)
        .fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn a_monitored_input_is_heard_and_one_that_is_not_stays_out() {
    let (mut audio, mut c) = engine();
    let st = c.input_state();
    c.send(Command::SetInput(Some(Box::new(MockInput::sine(
        f64::from(RATE),
        440.0,
        0.5,
    )))))
    .ok()
    .unwrap();
    // Not monitored: the output is silent, though the input is measured.
    let out = render(&mut audio, 4800);
    assert_eq!(peak(&out, 0).max(peak(&out, 1)), 0.0);
    assert!((st.take_peak(0) - 0.5).abs() < 0.01, "{}", st.take_peak(0));

    // Input channel 0 in the middle: both sides hear it.
    st.set_monitor(0b1, 0b1);
    let out = render(&mut audio, 4800);
    assert!((peak(&out, 0) - 0.5).abs() < 0.01);
    assert!((peak(&out, 1) - 0.5).abs() < 0.01);
    let m = st.take_monitor_peak();
    assert!(
        (m[0] - 0.5).abs() < 0.01 && (m[1] - 0.5).abs() < 0.01,
        "{m:?}"
    );

    // Only the right side.
    st.set_monitor(0, 0b1);
    let out = render(&mut audio, 4800);
    assert_eq!(peak(&out, 0), 0.0);
    assert!(peak(&out, 1) > 0.4);

    // Off again.
    st.set_monitor(0, 0);
    let out = render(&mut audio, 4800);
    assert_eq!(peak(&out, 1), 0.0);
}

#[test]
fn the_trim_changes_what_is_heard_and_measured() {
    let (mut audio, mut c) = engine();
    let st = c.input_state();
    c.send(Command::SetInput(Some(Box::new(MockInput::sine(
        f64::from(RATE),
        440.0,
        0.5,
    )))))
    .ok()
    .unwrap();
    st.set_monitor(0b1, 0b1);
    st.set_gain(0, 0.5);
    let out = render(&mut audio, 4800);
    assert!((peak(&out, 0) - 0.25).abs() < 0.01);
    assert!((st.take_peak(0) - 0.25).abs() < 0.01);
    // The other channel is not trimmed.
    assert!((st.take_peak(1) - 0.5).abs() < 0.01);
}

#[test]
fn a_silent_output_stays_silent_but_the_monitor_is_measured() {
    let (mut audio, mut c) = engine();
    audio.set_silent(true);
    let st = c.input_state();
    c.send(Command::SetInput(Some(Box::new(MockInput::sine(
        f64::from(RATE),
        440.0,
        0.5,
    )))))
    .ok()
    .unwrap();
    st.set_monitor(0b1, 0b10);
    let out = render(&mut audio, 4800);
    assert_eq!(peak(&out, 0).max(peak(&out, 1)), 0.0);
    let m = st.take_monitor_peak();
    assert!(m[0] > 0.4 && m[1] > 0.4, "{m:?}");
}

#[test]
fn the_recording_holds_the_trimmed_input_and_not_the_monitor() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-monitor-")
        .tempdir()
        .unwrap();
    let dir: PathBuf = tmp.path().to_path_buf();
    let (mut audio, mut c) = engine();
    let st = c.input_state();
    st.set_gain(0, 0.5);
    st.set_gain(1, 0.5);
    st.set_monitor(0b1, 0b10);
    let d = dir.clone();
    let (rec, capture) = Recorder::start(RecordSpec {
        sample_rate: RATE,
        lanes: vec![AudioLane {
            channels: vec![0, 1],
            path: Box::new(move |k| d.join(format!("take-{k}.wav"))),
        }],
        midi: false,
        latency: 0,
        midi_latency: 0,
        align: Box::new(|f| f),
        ring_seconds: 2.0,
        thru: Vec::new(),
    })
    .unwrap();
    c.send(Command::SetInput(Some(Box::new(MockInput::counter()))))
        .ok()
        .unwrap();
    c.send(Command::Record(Some(capture))).ok().unwrap();
    c.send(Command::Play).ok().unwrap();
    render(&mut audio, 4000);
    c.send(Command::Stop).ok().unwrap();
    c.send(Command::Record(None)).ok().unwrap();
    for _ in 0..100 {
        render(&mut audio, BLOCK);
        if rec.detached() {
            break;
        }
    }
    let r = rec.finish(Duration::from_secs(5));
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let a = &r.takes[0].audio[0];
    let mut wav = hound::WavReader::open(&a.path).unwrap();
    let s: Vec<f32> = wav.samples::<f32>().map(Result::unwrap).collect();
    for (j, f) in s.as_chunks::<2>().0.iter().enumerate().take(3000) {
        assert_eq!(*f, [j as f32 * 0.5, -(j as f32) * 0.5], "frame {j}");
    }
}
