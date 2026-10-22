//! The count-in before a recording (REC-003, ENG-036): the song's clock waits while the bars are
//! clicked, the recording starts where the song does, and a stop calls the count off. The output
//! is a buffer and the input a stand-in; no device is opened.

use std::path::PathBuf;
use std::time::Duration;

use xirius_engine::click::CountIn;
use xirius_engine::engine::{self, AudioEngine, Controller, EngineConfig};
use xirius_engine::record::{AudioLane, MockInput, RecordSpec, Recorder};
use xirius_engine::{
    AudioIo, Command, MixSpec, NoteEvent, Plan, ProcessSetup, Processor, RenderMode, TrackSlot,
    TrackSpec,
};

const RATE: u32 = 48_000;
const BLOCK: usize = 256;
/// A beat is 4 800 frames, so a bar of 4 is 19 200.
const BEAT: f64 = 4_800.0;

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

fn count_in(bars: u32) -> Option<Box<CountIn>> {
    Some(Box::new(CountIn::new(f64::from(RATE), BEAT, 4, bars, 0.5)))
}

#[test]
fn the_song_waits_while_the_bars_are_clicked_then_plays_from_where_it_was() {
    let (mut audio, mut c) = engine();
    c.send(Command::Seek(960)).ok().unwrap();
    c.send(Command::SetCountIn(count_in(1))).ok().unwrap();
    c.send(Command::Play).ok().unwrap();
    // Half the count: playing, not moving, and the click is heard.
    let out = render(&mut audio, 9_600);
    assert!(audio.is_playing());
    assert_eq!(audio.song_position(), 960);
    assert!(out.iter().any(|s| s.abs() > 0.1), "no click");
    // The rest of the count, and 4 800 frames of the song.
    let out = render(&mut audio, 9_600 + 4_800);
    assert_eq!(audio.song_position(), 960 + 4_800);
    // After the count nothing clicks: the metronome is off and the count is spent.
    assert!(out[9_600 * 2..].iter().all(|s| *s == 0.0));
    // A second play counts nothing in.
    c.send(Command::Stop).ok().unwrap();
    c.send(Command::Play).ok().unwrap();
    render(&mut audio, 1_000);
    assert_eq!(audio.song_position(), 960 + 4_800 + 1_000);
}

#[test]
fn a_stop_during_the_count_calls_it_off() {
    let (mut audio, mut c) = engine();
    c.send(Command::SetCountIn(count_in(2))).ok().unwrap();
    c.send(Command::Play).ok().unwrap();
    render(&mut audio, 5_000);
    c.send(Command::Stop).ok().unwrap();
    render(&mut audio, 500);
    assert!(!audio.is_playing());
    c.send(Command::Play).ok().unwrap();
    render(&mut audio, 1_000);
    assert_eq!(audio.song_position(), 1_000);
}

#[test]
fn the_recording_begins_where_the_song_does_and_holds_nothing_of_the_count() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-count-in-")
        .tempdir()
        .unwrap();
    let dir: PathBuf = tmp.path().to_path_buf();
    let (mut audio, mut c) = engine();
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
        ring_seconds: 4.0,
        thru: Vec::new(),
    })
    .unwrap();
    c.send(Command::SetInput(Some(Box::new(MockInput::counter()))))
        .ok()
        .unwrap();
    c.send(Command::Seek(960)).ok().unwrap();
    c.send(Command::SetCountIn(count_in(1))).ok().unwrap();
    c.send(Command::Record(Some(capture))).ok().unwrap();
    c.send(Command::Play).ok().unwrap();
    render(&mut audio, 19_200 + 6_000);
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
    assert_eq!(r.takes.len(), 1);
    let take = &r.takes[0];
    assert_eq!(take.song_start, 960);
    let a = &take.audio[0];
    assert_eq!(a.start, 960);
    let mut wav = hound::WavReader::open(&a.path).unwrap();
    let s: Vec<f32> = wav.samples::<f32>().map(Result::unwrap).collect();
    // The input frame that was heard as the song began: after the 19 200 of the count.
    assert_eq!(s[0], 19_200.0);
    assert_eq!(s[1], -19_200.0);
}
