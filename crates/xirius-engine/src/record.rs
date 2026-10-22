//! Recording: what an input hears while the transport plays, written to WAV files on a thread
//! of its own (REC-001, REC-004, REC-005, ENG-017, ENG-020, OPEN-07).
//!
//! - The audio thread reads the input (an [`AudioInput`], one or more channels) once per block,
//!   after the block was rendered, and the MIDI input (a [`MidiInput`], one or more ports) before
//!   it. While a [`Capture`] is attached it pushes the channels the recording picked into a
//!   lock-free ring, and marks into another one: where the song position stands (when the
//!   transport starts, stops, jumps or wraps at the loop end) and where a block was dropped
//!   because the ring was full. It never waits. A dropped block is counted and becomes silence
//!   of the same length in every file, so what follows stays in place.
//! - The [`Recorder`] thread reads the rings and writes one WAV file per lane and take: a lane
//!   is one armed audio track with the input channels it records (mono or stereo), a take one
//!   run of the song between two marks (one per lap when looping). All lanes share the ring, so
//!   their takes start and end on the same frames. The input is shifted back by the round-trip
//!   latency, so a take lines up with what the player heard, and a take starts on the first
//!   frame where a clip can start (a whole tick).
//!
//! The stand-ins [`MockInput`] and [`MockMidi`] feed fixed input, so the tests open no
//! microphone and no MIDI device; [`CpalInput`] is the device's default input.

use crate::device::Lost;
use crate::rt_lock::Mutex;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::processor::{NoteEvent, NoteKind};

/// The most input channels the engine reads (the rest of a larger interface is not read).
pub const MAX_INPUT_CHANNELS: usize = 32;

/// An audio input the engine reads on the audio thread.
pub trait AudioInput: Send {
    /// Fills `left` and `right` (same length, at most the plan's `max_block`) with the next
    /// frames. Called once per block, after the block was rendered, while the engine runs,
    /// recording or not; it must not allocate, lock or wait. Frames that are not there yet stay
    /// zero.
    fn read(&mut self, left: &mut [f32], right: &mut [f32]);

    /// The mix the engine just rendered for the same frames, given before `read`. A loopback
    /// stand-in hears it.
    fn played(&mut self, _left: &[f32], _right: &[f32]) {}

    /// How many channels the input has (1: mono). An input with other than 2 also implements
    /// [`AudioInput::read_channels`].
    fn channels(&self) -> usize {
        2
    }

    /// Like [`AudioInput::read`], for every channel: channel `c` goes to
    /// `buf[c * frames..(c + 1) * frames]`, for the `buf.len() / frames` channels `buf` holds
    /// (at most [`AudioInput::channels`]). The engine calls this one.
    fn read_channels(&mut self, buf: &mut [f32], frames: usize) {
        let (left, rest) = buf.split_at_mut(frames.min(buf.len()));
        if rest.len() >= frames {
            self.read(left, &mut rest[..frames]);
        } else {
            left.fill(0.0);
            rest.fill(0.0);
        }
    }

    /// How many of the `frames` frames the last read filled came from the device, from the
    /// first on: the rest were not there and are zeros. A take cut short by a device that
    /// stopped ends at the last frame that came.
    fn delivered(&self, frames: usize) -> usize {
        frames
    }
}

/// A MIDI input the engine reads on the audio thread.
pub trait MidiInput: Send {
    /// Calls `f(offset, message)` for each message that arrived for the next `frames` frames,
    /// `offset` within them. Called once per block, before it is rendered; must not allocate,
    /// lock or wait.
    fn read(&mut self, frames: usize, f: &mut dyn FnMut(usize, [u8; 3]));

    /// Like [`MidiInput::read`], with the port each message came in on (an index into
    /// [`MidiInput::ports`]). The engine calls this one.
    fn read_ports(&mut self, frames: usize, f: &mut dyn FnMut(usize, usize, [u8; 3])) {
        self.read(frames, &mut |offset, msg| f(offset, 0, msg));
    }

    /// The names of the ports, in order. Empty: one port without a name.
    fn ports(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The note a channel message plays, if it is a note on or off.
pub fn note_of(msg: [u8; 3]) -> Option<NoteKind> {
    let pitch = msg[1] & 0x7f;
    match msg[0] & 0xf0 {
        0x90 if msg[2] > 0 => Some(NoteKind::On {
            pitch,
            velocity: f32::from(msg[2] & 0x7f) / 127.0,
        }),
        0x80 | 0x90 => Some(NoteKind::Off { pitch }),
        _ => None,
    }
}

// ---- stand-ins -------------------------------------------------------------------------------

enum Signal {
    /// Each frame holds its number since the input was made: left `n`, right `-n`.
    Counter,
    /// Channel `c` of frame `n` holds `n + c × 1 000 000`.
    Counters,
    Sine {
        step: f64,
        amp: f32,
    },
    /// What the engine played, `delay` frames later.
    Loopback {
        buf: Vec<[f32; 2]>,
        write: usize,
        read: usize,
    },
}

/// A fixed input for tests and for playing without a device: a counter, a sine or a loopback.
pub struct MockInput {
    signal: Signal,
    frame: u64,
    channels: usize,
    /// The frames it delivers before it stops (a device unplugged); `None`: it never stops.
    until: Option<u64>,
    /// Frames the last read delivered.
    got: usize,
}

impl MockInput {
    /// Frame `n` (counted from the first `read`) is `n` on the left and `-n` on the right. Exact
    /// in a 32-bit float file up to 2^24 frames.
    pub fn counter() -> MockInput {
        MockInput {
            signal: Signal::Counter,
            frame: 0,
            channels: 2,
            until: None,
            got: 0,
        }
    }

    /// An interface with `channels` inputs: channel `c` of frame `n` is `n + c × 1 000 000`, so
    /// a file tells which channel it recorded and from which frame. Exact in a 32-bit float file
    /// while that stays under 2^24 (16 channels, 770 000 frames).
    pub fn counters(channels: usize) -> MockInput {
        MockInput {
            signal: Signal::Counters,
            frame: 0,
            channels: channels.clamp(1, MAX_INPUT_CHANNELS),
            until: None,
            got: 0,
        }
    }

    pub fn sine(sample_rate: f64, freq: f64, amp: f32) -> MockInput {
        MockInput {
            signal: Signal::Sine {
                step: freq / sample_rate,
                amp,
            },
            frame: 0,
            channels: 2,
            until: None,
            got: 0,
        }
    }

    /// Hears the engine's own mix `delay` frames later: a cable from the output to the input
    /// with a known round trip. `max_block` is the engine's; the buffer is made here, not on
    /// the audio thread.
    pub fn loopback(delay: usize, max_block: usize) -> MockInput {
        let len = delay + max_block + 1;
        MockInput {
            signal: Signal::Loopback {
                buf: vec![[0.0; 2]; len],
                write: delay,
                read: 0,
            },
            frame: 0,
            channels: 2,
            until: None,
            got: 0,
        }
    }

    /// Stops delivering after `frames` frames, as an unplugged device does: what is read after
    /// that is silence (not for the loopback).
    pub fn stopping_after(mut self, frames: u64) -> MockInput {
        self.until = Some(frames);
        self
    }

    /// Of the next `n` frames, how many it delivers.
    fn coming(&self, n: usize) -> usize {
        match self.until {
            Some(u) if !matches!(self.signal, Signal::Loopback { .. }) => {
                u.saturating_sub(self.frame).min(n as u64) as usize
            }
            _ => n,
        }
    }

    /// Channel `ch` of frame `i` of the next block (not for the loopback).
    fn value(&self, ch: usize, i: usize) -> f32 {
        let n = self.frame + i as u64;
        match &self.signal {
            Signal::Counter if ch == 1 => -(n as f32),
            Signal::Counter => n as f32,
            Signal::Counters => (n + ch as u64 * 1_000_000) as f32,
            Signal::Sine { step, amp } => {
                let phase = (n as f64 * *step).fract();
                (phase * std::f64::consts::TAU).sin() as f32 * *amp
            }
            Signal::Loopback { .. } => 0.0,
        }
    }
}

impl AudioInput for MockInput {
    fn read(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        if let Signal::Loopback { buf, read, .. } = &mut self.signal {
            let len = buf.len();
            for i in 0..n {
                let [l, r] = buf[*read];
                left[i] = l;
                right[i] = r;
                *read = (*read + 1) % len;
            }
        } else {
            let real = self.coming(n);
            for i in 0..real {
                left[i] = self.value(0, i);
                right[i] = self.value(1, i);
            }
            left[real..n].fill(0.0);
            right[real..n].fill(0.0);
        }
        self.got = self.coming(n);
        self.frame += n as u64;
    }

    fn played(&mut self, left: &[f32], right: &[f32]) {
        if let Signal::Loopback { buf, write, .. } = &mut self.signal {
            let len = buf.len();
            for (l, r) in left.iter().zip(right) {
                buf[*write] = [*l, *r];
                *write = (*write + 1) % len;
            }
        }
    }

    fn channels(&self) -> usize {
        self.channels
    }

    fn delivered(&self, frames: usize) -> usize {
        self.got.min(frames)
    }

    fn read_channels(&mut self, buf: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        let have = (buf.len() / frames).min(self.channels);
        if let Signal::Loopback {
            buf: ring, read, ..
        } = &mut self.signal
        {
            let len = ring.len();
            for i in 0..frames {
                let [l, r] = ring[*read];
                buf[i] = l;
                if have > 1 {
                    buf[frames + i] = r;
                }
                *read = (*read + 1) % len;
            }
        } else {
            let real = self.coming(frames);
            for c in 0..have {
                for i in 0..real {
                    buf[c * frames + i] = self.value(c, i);
                }
                buf[c * frames + real..(c + 1) * frames].fill(0.0);
            }
        }
        self.got = self.coming(frames);
        buf[have * frames..].fill(0.0);
        self.frame += frames as u64;
    }
}

/// MIDI messages at fixed frames, counted from the first `read`.
pub struct MockMidi {
    /// (frame, port, message), oldest first.
    events: Vec<(u64, usize, [u8; 3])>,
    ports: Vec<String>,
    next: usize,
    frame: u64,
}

impl MockMidi {
    /// Messages on one port.
    pub fn new(events: Vec<(u64, [u8; 3])>) -> MockMidi {
        MockMidi::with_ports(
            Vec::new(),
            events.into_iter().map(|(at, msg)| (at, 0, msg)).collect(),
        )
    }

    /// Messages on the ports named `ports`: `(frame, port, message)`, `port` an index into them.
    pub fn with_ports(ports: Vec<String>, mut events: Vec<(u64, usize, [u8; 3])>) -> MockMidi {
        events.sort_by_key(|e| e.0);
        MockMidi {
            events,
            ports,
            next: 0,
            frame: 0,
        }
    }
}

impl MidiInput for MockMidi {
    fn read(&mut self, frames: usize, f: &mut dyn FnMut(usize, [u8; 3])) {
        self.read_ports(frames, &mut |offset, _, msg| f(offset, msg));
    }

    fn read_ports(&mut self, frames: usize, f: &mut dyn FnMut(usize, usize, [u8; 3])) {
        let end = self.frame + frames as u64;
        while let Some(&(at, port, msg)) = self.events.get(self.next) {
            if at >= end {
                break;
            }
            f(at.saturating_sub(self.frame) as usize, port, msg);
            self.next += 1;
        }
        self.frame = end;
    }

    fn ports(&self) -> Vec<String> {
        self.ports.clone()
    }
}

/// MIDI messages pushed from another thread (a MIDI device's callback, an on-screen keyboard):
/// each lands at the start of the next block.
pub struct MidiFeed {
    rx: Consumer<[u8; 3]>,
}

/// The pushing end of a [`MidiFeed`].
pub struct MidiSender {
    tx: Producer<[u8; 3]>,
}

impl MidiFeed {
    /// A feed holding up to `capacity` messages between two blocks.
    pub fn new(capacity: usize) -> (MidiSender, MidiFeed) {
        let (tx, rx) = RingBuffer::new(capacity.max(16));
        (MidiSender { tx }, MidiFeed { rx })
    }
}

impl MidiSender {
    /// The same as [`MidiFeed::new`]: the sender and the feed to give the engine.
    pub fn new(capacity: usize) -> (MidiSender, MidiFeed) {
        MidiFeed::new(capacity)
    }

    /// Queues a message. `false` when the feed is full (the message is lost).
    pub fn send(&mut self, msg: [u8; 3]) -> bool {
        self.tx.push(msg).is_ok()
    }
}

impl MidiInput for MidiFeed {
    fn read(&mut self, _frames: usize, f: &mut dyn FnMut(usize, [u8; 3])) {
        while let Ok(msg) = self.rx.pop() {
            f(0, msg);
        }
    }
}

// ---- the device's input ------------------------------------------------------------------------

/// The default input device, opened in shared mode on a thread of its own. Its callback pushes
/// every channel's frames into a ring the audio thread reads; frames the audio thread finds
/// missing are silence.
pub struct CpalInput {
    rx: Consumer<f32>,
    stop: Arc<AtomicBool>,
    pub name: String,
    pub channels: u16,
    /// Capture time to callback time, as the device reports it ([`crate::stats::Steady`] over the
    /// last callbacks).
    latency_ns: Arc<AtomicU64>,
    /// Why the device stopped by itself, once it has: what it does not deliver reads as silence,
    /// so whoever records from it asks here.
    lost: Lost,
    /// Frames the last read took from the ring.
    got: usize,
}

/// A cpal error as the text of this crate's input errors, saying [`crate::device::IN_USE`] when
/// another program holds the device.
fn busy(e: cpal::Error) -> String {
    match e.kind() {
        cpal::ErrorKind::DeviceBusy => crate::device::IN_USE.to_string(),
        _ => e.to_string(),
    }
}

/// What an input that delivers nothing is said to do: one that never starts (another program
/// has the hardware) or stops (unplugged).
pub const NO_INPUT: &str = "入力デバイスから 1 秒間データが届きません";

/// How long an input may deliver nothing before it is taken as stopped.
pub(crate) const INPUT_WAIT: Duration = Duration::from_secs(1);

/// When an input last delivered, from a count of its callbacks.
struct Heard {
    beats: u64,
    at: Instant,
}

impl Heard {
    fn new(beats: u64, now: Instant) -> Heard {
        Heard { beats, at: now }
    }

    /// Whether the input has delivered nothing for [`INPUT_WAIT`] now that `beats` callbacks
    /// came.
    fn quiet(&mut self, beats: u64, now: Instant) -> bool {
        if beats != self.beats {
            *self = Heard::new(beats, now);
        }
        now.duration_since(self.at) >= INPUT_WAIT
    }
}

impl CpalInput {
    /// Opens the default input at `sample_rate` (the engine's; the device must run at it).
    pub fn open(sample_rate: u32) -> Result<CpalInput, String> {
        Self::open_named(sample_rate, "", None)
    }

    pub fn open_named(
        sample_rate: u32,
        name: &str,
        buffer: Option<u32>,
    ) -> Result<CpalInput, String> {
        use cpal::traits::{DeviceTrait, StreamTrait};
        let wanted = name.to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let latency_ns = Arc::new(AtomicU64::new(0));
        let lost = Lost::default();
        // Callbacks so far: the thread below watches it move.
        let beats = Arc::new(AtomicU64::new(0));
        type Ready = Result<(String, u16, Consumer<f32>), String>;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Ready>();
        let (stop2, lat2) = (stop.clone(), latency_ns.clone());
        let (lost2, beats2) = (lost.clone(), beats.clone());
        std::thread::Builder::new()
            .name("xirius-input".into())
            .spawn(move || {
                let opened = (|| {
                    let device = crate::devices::endpoint(&wanted, true)?;
                    let config = device
                        .default_input_config()
                        .map_err(|e| format!("入力デバイスの形式を取得できません: {e}"))?;
                    if config.sample_rate() != sample_rate {
                        return Err(format!(
                            "入力デバイスのサンプルレート {} Hz がエンジンの {sample_rate} Hz と異なるため、録音できません",
                            config.sample_rate()
                        ));
                    }
                    let channels = config.channels();
                    let name = device
                        .description()
                        .map(|d| d.name().to_string())
                        .unwrap_or_else(|_| "(名前なし)".into());
                    let ch = usize::from(channels).max(1);
                    let (mut tx, rx) = RingBuffer::<f32>::new(sample_rate as usize * ch);
                    let mut steady = crate::stats::Steady::new();
                    let (lost_err, beats_cb) = (lost2.clone(), beats2.clone());
                    let stream = device
                        .build_input_stream::<f32, _, _>(
                            cpal::StreamConfig {
                                channels,
                                sample_rate,
                                buffer_size: buffer.map_or(cpal::BufferSize::Default, cpal::BufferSize::Fixed),
                            },
                            move |data: &[f32], info: &cpal::InputCallbackInfo| {
                                beats_cb.fetch_add(1, Ordering::Relaxed);
                                let ts = info.timestamp();
                                if let Some(d) = ts.callback.checked_duration_since(ts.capture) {
                                    let ns = d.as_nanos().min(u128::from(u64::MAX)) as u64;
                                    lat2.store(steady.push(ns), Ordering::Relaxed);
                                }
                                for frame in data.chunks_exact(ch) {
                                    if tx.slots() >= ch {
                                        for s in frame {
                                            let _ = tx.push(*s);
                                        }
                                    }
                                }
                            },
                            move |e: cpal::Error| {
                                if crate::device::ends_stream(e.kind()) {
                                    lost_err.set(crate::device::why_ended(&e));
                                }
                            },
                            None,
                        )
                        .map_err(|e| format!("入力デバイスを開けません: {}", busy(e)))?;
                    stream
                        .play()
                        .map_err(|e| format!("入力デバイスを開始できません: {}", busy(e)))?;
                    // The device is open once it has delivered: one that starts but never
                    // calls back (another program has the hardware) is not.
                    let asked = Instant::now();
                    while beats2.load(Ordering::Relaxed) == 0 {
                        if let Some(why) = lost2.get() {
                            return Err(format!("入力デバイスを開始できません: {why}"));
                        }
                        if asked.elapsed() >= INPUT_WAIT {
                            return Err(NO_INPUT.to_string());
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok((stream, name, channels, rx))
                })();
                match opened {
                    Ok((stream, name, channels, rx)) => {
                        let _ = ready_tx.send(Ok((name, channels, rx)));
                        let mut heard = Heard::new(beats2.load(Ordering::Relaxed), Instant::now());
                        while !stop2.load(Ordering::Acquire) {
                            std::thread::sleep(Duration::from_millis(20));
                            if heard.quiet(beats2.load(Ordering::Relaxed), Instant::now()) {
                                lost2.set(NO_INPUT.to_string());
                            }
                        }
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| format!("入力のスレッドを作成できません: {e}"))?;
        let (name, channels, rx) = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "入力デバイスが応答しません".to_string())??;
        Ok(CpalInput {
            rx,
            stop,
            name,
            channels,
            latency_ns,
            lost,
            got: 0,
        })
    }

    /// An input fed by a thread of someone else's: `ring` holds `channels`-interleaved frames,
    /// and `stop` ends that thread when this goes.
    pub(crate) fn from_parts(
        rx: Consumer<f32>,
        stop: Arc<AtomicBool>,
        name: String,
        channels: u16,
        latency_ns: Arc<AtomicU64>,
        lost: Lost,
    ) -> CpalInput {
        CpalInput {
            rx,
            stop,
            name,
            channels,
            latency_ns,
            lost,
            got: 0,
        }
    }

    /// Says why the device stopped by itself, once it has.
    pub fn lost(&self) -> Lost {
        self.lost.clone()
    }

    pub(crate) fn stop_now(&self) {
        self.stop.store(true, Ordering::Release);
    }

    /// A handle that reads the device's input latency after the input went to the engine.
    pub fn latency_probe(&self) -> Arc<AtomicU64> {
        self.latency_ns.clone()
    }

    /// Reads up to `frames` frames from the ring, calling `put(channel, frame, sample)` for each
    /// sample; returns how many frames there were.
    fn take(&mut self, frames: usize, mut put: impl FnMut(usize, usize, f32)) -> usize {
        let ch = usize::from(self.channels).max(1);
        let have = (self.rx.slots() / ch).min(frames);
        if have > 0
            && let Ok(chunk) = self.rx.read_chunk(have * ch)
        {
            let (a, b) = chunk.as_slices();
            for (k, s) in a.iter().chain(b).enumerate() {
                put(k % ch, k / ch, *s);
            }
            chunk.commit_all();
        }
        have
    }
}

impl AudioInput for CpalInput {
    fn read(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mono = self.channels < 2;
        let have = self.take(n, |c, i, s| match c {
            0 => {
                left[i] = s;
                if mono {
                    right[i] = s;
                }
            }
            1 => right[i] = s,
            _ => {}
        });
        left[have..n].fill(0.0);
        right[have..n].fill(0.0);
        self.got = have;
    }

    fn channels(&self) -> usize {
        usize::from(self.channels).clamp(1, MAX_INPUT_CHANNELS)
    }

    fn read_channels(&mut self, buf: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        let out = buf.len() / frames;
        let have = self.take(frames, |c, i, s| {
            if c < out {
                buf[c * frames + i] = s;
            }
        });
        for c in 0..out {
            buf[c * frames + have..(c + 1) * frames].fill(0.0);
        }
        self.got = have;
    }

    fn delivered(&self, frames: usize) -> usize {
        self.got.min(frames)
    }
}

impl Drop for CpalInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

// ---- the audio side -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    /// From capture frame `index` on, the song plays from `song` (`None`: stopped).
    Song { index: u64, song: Option<u64> },
    /// The `frames` frames at `index` were dropped (one block, or several in a row).
    Gap {
        index: u64,
        frames: u64,
        blocks: u64,
    },
}

#[derive(Clone, Copy, Debug)]
struct MidiMark {
    index: u64,
    port: usize,
    msg: [u8; 3],
}

#[derive(Default)]
struct Shared {
    writer_error: Mutex<Option<String>>,
    dropped_blocks: AtomicU64,
    dropouts: Mutex<Vec<Dropout>>,
    disk_space_low: AtomicBool,
    disk_space_unknown: AtomicBool,
    /// Frames dropped because the ring was full.
    dropped: AtomicU64,
    /// Marks or MIDI messages lost because their ring was full.
    lost_marks: AtomicU64,
    lost_midi: AtomicU64,
    /// Capture frames so far, as the audio side last stored them.
    frames: AtomicU64,
    /// The capture frame after the last one the input delivered.
    heard: AtomicU64,
    /// End the take at `heard`: the input stopped, and what came after is silence it did not
    /// deliver.
    cut: AtomicBool,
    /// The audio side let go of the capture: everything it pushed is in the rings.
    done: AtomicBool,
    /// The note messages so far, at song frames (the latency taken off), for a screen to show
    /// the notes while they are played.
    live_midi: Mutex<Vec<(u64, [u8; 3])>>,
}

/// A track that plays the MIDI input as it comes in, while recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thru {
    /// The engine's track.
    pub track: usize,
    /// The port it hears (an index into [`MidiInput::ports`]); `None`: every port.
    pub port: Option<usize>,
    /// The semitones the track plays above (or below) the keys played (-64..=64), and the
    /// percent its velocity is boosted or attenuated by (-100..=100, UI-312).
    pub transpose: i8,
    pub velocity: i8,
}

impl Thru {
    /// The note the keys played sound as on this track: the transpose may take it off the
    /// keyboard (then it does not sound), and the velocity is the track's.
    pub fn heard(self, kind: NoteKind) -> Option<NoteKind> {
        match kind {
            NoteKind::On { pitch, velocity } => {
                let pitch = i16::from(pitch) + i16::from(self.transpose);
                let pitch = u8::try_from(pitch).ok().filter(|p| *p <= 127)?;
                // The same whole-number velocity the playing tracks use: 1..=127 first, then
                // normalized. No allocation on the audio thread (UI-312).
                let note = (velocity * 127.0).round().clamp(1.0, 127.0) as u8;
                let scaled = f64::from(note) * (1.0 + f64::from(self.velocity) / 100.0);
                let v = (scaled.round() as i64).clamp(1, 127) as f32 / 127.0;
                Some(NoteKind::On { pitch, velocity: v })
            }
            NoteKind::Off { pitch } => {
                let pitch = i16::from(pitch) + i16::from(self.transpose);
                Some(NoteKind::Off {
                    pitch: u8::try_from(pitch).ok().filter(|p| *p <= 127)?,
                })
            }
            other => Some(other),
        }
    }
}

/// The audio thread's end of a recording: give it to the engine with
/// [`crate::Command::Record`].
pub struct Capture {
    /// The picked channels, interleaved: one frame is `picks.len()` samples.
    audio: Option<Producer<f32>>,
    /// The input channel of each sample of a frame.
    picks: Vec<usize>,
    marks: Producer<Mark>,
    midi: Option<Producer<MidiMark>>,
    shared: Arc<Shared>,
    /// Capture frames so far (pushed or dropped).
    index: u64,
    /// Blocks dropped in a row, not yet told to the writer: `(index, frames, failed callbacks)`. One mark for the
    /// whole run, sent before the next block that fits, so a writer that falls far behind costs
    /// one mark, not one per block, and never reads audio past a gap it has not been told of.
    gap: Option<(u64, u64, u64)>,
    /// The song position the next segment is expected at; `None` before the first.
    expect: Option<Option<u64>>,
    /// Frames still captured after the recording was asked to stop, so the input catches up
    /// with what was played (the round trip).
    tail: u64,
    closing: Option<u64>,
    /// The engine's tracks that play the MIDI input as it comes in.
    pub(crate) thru: Vec<Thru>,
    /// The notes the thru sent, by `(track, the key's own pitch)`: the pitch that was really
    /// sent. The off uses it, and a value changed while the key is held stops it first
    /// (UI-312). A fixed array: the audio thread must not allocate.
    held: [(usize, u8, u8); 128],
    held_len: usize,
    /// The notes a value change already stopped, by `(track, the key's own pitch)`: their
    /// later off is not sent again at the new value's pitch (UI-312).
    stopped: [(usize, u8); 128],
    stopped_len: usize,
}

impl Capture {
    fn note_off(&mut self, track: usize, pitch: u8, plan: &mut crate::mixer::Plan) {
        plan.push_event(
            track,
            NoteEvent {
                offset: 0,
                kind: NoteKind::Off { pitch },
            },
        );
    }

    fn remember_stopped(&mut self, track: usize, own: u8) {
        if self.stopped_len < self.stopped.len() {
            self.stopped[self.stopped_len] = (track, own);
            self.stopped_len += 1;
        }
    }

    fn take_stopped(&mut self, track: usize, own: u8) -> bool {
        let mut i = 0;
        while i < self.stopped_len {
            if self.stopped[i] == (track, own) {
                self.stopped_len -= 1;
                self.stopped[i] = self.stopped[self.stopped_len];
                return true;
            }
            i += 1;
        }
        false
    }

    /// Stops the notes `track` holds at the pitch that was sent there (UI-312).
    fn stop_held(&mut self, track: usize, plan: &mut crate::mixer::Plan) {
        let mut i = 0;
        while i < self.held_len {
            if self.held[i].0 == track {
                let (_, own, sent) = self.held[i];
                self.note_off(track, sent, plan);
                self.remember_stopped(track, own);
                self.held_len -= 1;
                self.held[i] = self.held[self.held_len];
                continue;
            }
            i += 1;
        }
    }

    /// Replaces what the tracks play as the input comes in, and gives the old values back so
    /// the audio thread does not free them. Notes held whose track's value or port changed are
    /// let go at the pitch that was sent before the new value takes over (UI-312).
    pub(crate) fn set_thru(&mut self, thru: Vec<Thru>, plan: &mut crate::mixer::Plan) -> Vec<Thru> {
        for i in 0..self.thru.len() {
            let old = self.thru[i];
            let new = thru.iter().find(|t| t.track == old.track);
            if new.is_some_and(|n| {
                n.port == old.port && n.transpose == old.transpose && n.velocity == old.velocity
            }) {
                continue;
            }
            self.stop_held(old.track, plan);
        }
        std::mem::replace(&mut self.thru, thru)
    }

    /// One message from the input, through the tracks that monitor it: the note sent to each is
    /// kept so its off uses the same pitch (UI-312).
    pub(crate) fn thru_note(&mut self, kind: NoteKind, port: usize, plan: &mut crate::mixer::Plan) {
        for i in 0..self.thru.len() {
            let t = self.thru[i];
            if t.port.is_some_and(|p| p != port) {
                continue;
            }
            match kind {
                NoteKind::On { pitch, .. } => {
                    let Some(NoteKind::On {
                        pitch: heard,
                        velocity,
                    }) = t.heard(kind)
                    else {
                        continue;
                    };
                    // A new press of a key a value change stopped is a new note.
                    self.take_stopped(t.track, pitch);
                    if self.held_len >= self.held.len() {
                        // It could never be let go: do not sound it, and keep its off silent.
                        self.remember_stopped(t.track, pitch);
                        continue;
                    }
                    self.held[self.held_len] = (t.track, pitch, heard);
                    self.held_len += 1;
                    plan.push_event(
                        t.track,
                        NoteEvent {
                            offset: 0,
                            kind: NoteKind::On {
                                pitch: heard,
                                velocity,
                            },
                        },
                    );
                }
                NoteKind::Off { pitch } => {
                    let mut sent = None;
                    let mut j = 0;
                    while j < self.held_len {
                        if self.held[j].0 == t.track && self.held[j].1 == pitch {
                            sent = Some(self.held[j].2);
                            self.held_len -= 1;
                            self.held[j] = self.held[self.held_len];
                            break;
                        }
                        j += 1;
                    }
                    if let Some(pitch) = sent {
                        self.note_off(t.track, pitch, plan);
                    } else if self.take_stopped(t.track, pitch) {
                        // Already stopped when the value changed: not again at the new height.
                    } else if let Some(NoteKind::Off { pitch }) = t.heard(kind) {
                        self.note_off(t.track, pitch, plan);
                    }
                }
                other => {
                    if let Some(kind) = t.heard(other) {
                        plan.push_event(t.track, NoteEvent { offset: 0, kind });
                    }
                }
            }
        }
    }
}

impl Capture {
    /// A run of `frames` frames at `offset` in the block plays the song from `at`.
    pub(crate) fn segment(&mut self, offset: usize, frames: usize, at: Option<u64>) {
        if self.expect != Some(at) {
            let mark = Mark::Song {
                index: self.index + offset as u64,
                song: at,
            };
            if self.marks.push(mark).is_err() {
                self.shared.lost_marks.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.expect = Some(at.map(|a| a + frames as u64));
    }

    pub(crate) fn midi(&mut self, offset: usize, port: usize, msg: [u8; 3]) {
        if let Some(tx) = &mut self.midi {
            let mark = MidiMark {
                index: self.index + offset as u64,
                port,
                msg,
            };
            if tx.push(mark).is_err() {
                self.shared.lost_midi.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// The input of one block of `frames` frames, channel `c` at `input[c * frames..]` (a
    /// channel `input` does not hold is silence), of which the first `delivered` came from the
    /// device. Returns whether the capture is finished and can be handed back.
    pub(crate) fn block(&mut self, input: &[f32], frames: usize, delivered: usize) -> bool {
        let n = frames;
        if delivered > 0 {
            let heard = self.index + delivered.min(n) as u64;
            self.shared.heard.store(heard, Ordering::Relaxed);
        }
        let have = input.len().checked_div(n).unwrap_or(0);
        if let Some(tx) = &mut self.audio {
            let stride = self.picks.len();
            if tx.slots() >= n * stride && flush_gap(&mut self.gap, &mut self.marks) {
                let mut chunk = tx.write_chunk_uninit(n * stride).expect("slots checked");
                let (a, b) = chunk.as_mut_slices();
                for (k, slot) in a.iter_mut().chain(b.iter_mut()).enumerate() {
                    let (i, ch) = (k / stride, self.picks[k % stride]);
                    slot.write(if ch < have { input[ch * n + i] } else { 0.0 });
                }
                // SAFETY: every slot of the chunk was written above.
                unsafe { chunk.commit_all() };
            } else {
                self.shared.dropped.fetch_add(n as u64, Ordering::Relaxed);
                self.shared.dropped_blocks.fetch_add(1, Ordering::Relaxed);
                match &mut self.gap {
                    Some((_, frames, blocks)) => {
                        *frames += n as u64;
                        *blocks += 1;
                    }
                    None => self.gap = Some((self.index, n as u64, 1)),
                }
            }
        }
        self.index += n as u64;
        self.shared.frames.store(self.index, Ordering::Release);
        match &mut self.closing {
            Some(left) => {
                *left = left.saturating_sub(n as u64);
                *left == 0
            }
            None => false,
        }
    }

    /// Asked to stop: finished at once without a tail, else after it.
    pub(crate) fn close(&mut self) -> bool {
        if self.closing.is_none() {
            self.closing = Some(self.tail);
        }
        self.tail == 0
    }

    /// Tells the writer the audio side let go. The rings' own release orders the data first.
    pub(crate) fn finish(&mut self) {
        if !flush_gap(&mut self.gap, &mut self.marks) {
            self.shared.lost_marks.fetch_add(1, Ordering::Relaxed);
        }
        self.shared.done.store(true, Ordering::Release);
    }
}

/// Tells the writer of the blocks dropped in a row, if any; `false` while the marks are full.
fn flush_gap(gap: &mut Option<(u64, u64, u64)>, marks: &mut Producer<Mark>) -> bool {
    let Some((index, frames, blocks)) = *gap else {
        return true;
    };
    if marks
        .push(Mark::Gap {
            index,
            frames,
            blocks,
        })
        .is_err()
    {
        return false;
    }
    *gap = None;
    true
}

// ---- the writer ---------------------------------------------------------------------------------

/// One audio file per take: an armed audio track and the input channels it records.
pub struct AudioLane {
    /// The input channels (from 0), one per channel of the file: one for mono, two for stereo.
    /// A channel the input does not have records silence. Several lanes may pick the same one.
    pub channels: Vec<usize>,
    /// The file of take `k` (from 0). Called just before the file is made, lane after lane, so
    /// a name it finds free is still free.
    pub path: Box<dyn FnMut(usize) -> PathBuf + Send>,
}

/// What a recording is made with.
pub struct RecordSpec {
    pub sample_rate: u32,
    /// The audio to capture, one lane per armed audio track. Empty: no audio.
    pub lanes: Vec<AudioLane>,
    /// Capture MIDI (an armed instrument track).
    pub midi: bool,
    /// Frames from a frame leaving for the device to the input hearing it (output latency +
    /// input latency): the audio is shifted back by this much.
    pub latency: u64,
    /// The same for MIDI: how late a note played along reaches the engine.
    pub midi_latency: u64,
    /// The first song frame at or after the given one where a take may start (a whole tick,
    /// so the clip starts on exactly that frame).
    pub align: Box<dyn Fn(u64) -> u64 + Send>,
    /// How long the writer may stall before blocks are dropped.
    pub ring_seconds: f64,
    /// The engine's tracks that play the MIDI input as it comes in.
    pub thru: Vec<Thru>,
}

/// One take: a run of the song between two marks.
#[derive(Clone, Debug, PartialEq)]
pub struct Take {
    /// Which run of the recording (0, 1, …; one per lap when looping).
    pub index: usize,
    /// Song frames the run covered, before alignment.
    pub song_start: u64,
    pub song_end: u64,
    /// One file per lane that heard the run, in lane order.
    pub audio: Vec<AudioTake>,
    /// MIDI messages at song frames, oldest first.
    pub midi: Vec<TakeMidi>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioTake {
    /// The lane (an index into [`RecordSpec::lanes`]).
    pub lane: usize,
    pub path: PathBuf,
    /// The song frame of the file's first frame (aligned).
    pub start: u64,
    pub frames: u64,
}

/// A MIDI message of a take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TakeMidi {
    /// The song frame.
    pub at: u64,
    /// The port it came in on (an index into [`MidiInput::ports`]).
    pub port: usize,
    pub msg: [u8; 3],
}

/// What a finished recording made.
#[derive(Clone, Debug, Default)]
pub struct Recording {
    pub disk_space_unknown: bool,
    pub dropped_blocks: u64,
    pub dropouts: Vec<Dropout>,
    pub disk_space_low: bool,
    pub takes: Vec<Take>,
    /// Capture frames read from the input, recorded or not.
    pub frames: u64,
    /// Frames dropped because the writer fell behind (silence in the files).
    pub dropped: u64,
    pub lost_marks: u64,
    pub lost_midi: u64,
    /// The audio side let go before [`Recorder::finish`] gave up waiting.
    pub detached: bool,
    pub errors: Vec<String>,
}

/// A continuous gap in capture time. `song_frame` is the start after input-latency correction;
/// a gap spanning a loop wrap still reports its full capture length and first song position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropout {
    pub capture_frame: u64,
    pub song_frame: Option<u64>,
    pub frames: u64,
    pub blocks: u64,
}

pub struct Recorder {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Recording>>,
    /// Frames the audio side may render after [`Recorder::finish`] begins before it is taken
    /// for one that was not told to stop: the tail and a second.
    overrun: u64,
}

type Wav = crate::wavfile::SafeWav;

impl Recorder {
    /// Starts the writer thread. Give the capture to the engine with
    /// [`crate::Command::Record`]`(Some(..))`; stop it with `Record(None)`.
    pub fn start(spec: RecordSpec) -> std::io::Result<(Recorder, Box<Capture>)> {
        let frames = ((f64::from(spec.sample_rate) * spec.ring_seconds) as usize).max(8192);
        let shared = Arc::new(Shared::default());
        let picks: Vec<usize> = spec
            .lanes
            .iter()
            .flat_map(|l| l.channels.iter().copied())
            .collect();
        let audio = !picks.is_empty();
        let (audio_tx, audio_rx) = if audio {
            let (tx, rx) = RingBuffer::new(frames * picks.len());
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let (midi_tx, midi_rx) = if spec.midi {
            let (tx, rx) = RingBuffer::new(8192);
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let (marks_tx, marks_rx) = RingBuffer::new(8192);
        let tail =
            if audio { spec.latency } else { 0 }.max(if spec.midi { spec.midi_latency } else { 0 });
        let stride = picks.len();
        let capture = Box::new(Capture {
            audio: audio_tx,
            picks,
            marks: marks_tx,
            midi: midi_tx,
            shared: shared.clone(),
            index: 0,
            gap: None,
            expect: None,
            tail,
            closing: None,
            thru: spec.thru.clone(),
            held: [(0, 0, 0); 128],
            held_len: 0,
            stopped: [(0, 0); 128],
            stopped_len: 0,
        });
        let mut spec = spec;
        let mut first = 0;
        let lanes = std::mem::take(&mut spec.lanes)
            .into_iter()
            .map(|l| {
                let lane = Lane {
                    failed_seg: None,
                    first,
                    count: l.channels.len(),
                    path: l.path,
                    file: None,
                };
                first += lane.count;
                lane
            })
            .collect();
        let stop = Arc::new(AtomicBool::new(false));
        let overrun = tail + u64::from(spec.sample_rate);
        let writer = Writer {
            audio: audio_rx,
            stride,
            lanes,
            spec,
            marks: marks_rx,
            midi: midi_rx,
            songs: Vec::new(),
            gaps: VecDeque::new(),
            messages: Vec::new(),
            live_wait: Vec::new(),
            idx: 0,
            seg: 0,
            files: Vec::new(),
            errors: Vec::new(),
        };
        let (sh, st) = (shared.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("xirius-recorder".into())
            .spawn(move || writer.run(&sh, &st))?;
        Ok((
            Recorder {
                shared,
                stop,
                thread: Some(thread),
                overrun,
            },
            capture,
        ))
    }

    /// Ends the takes at the last frame the input delivered, cutting off the silence of an
    /// input that stopped (unplugged) before the recording did. Asked before the capture is let
    /// go ([`crate::Command::Record`]`(None)`).
    pub fn end_at_last_input(&self) {
        self.shared.cut.store(true, Ordering::Release);
    }

    /// The audio side let go of the capture.
    pub fn detached(&self) -> bool {
        self.shared.done.load(Ordering::Acquire)
    }

    /// Frames dropped so far.
    pub fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }
    pub fn disk_space_low(&self) -> bool {
        self.shared.disk_space_low.load(Ordering::Relaxed)
    }
    pub fn disk_space_unknown(&self) -> bool {
        self.shared.disk_space_unknown.load(Ordering::Relaxed)
    }
    /// The latest disk writer failure, available while capture is still attached.
    pub fn writer_error(&self) -> Option<String> {
        self.shared
            .writer_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn dropped_blocks(&self) -> u64 {
        self.shared.dropped_blocks.load(Ordering::Relaxed)
    }
    /// The note messages played so far, each at the song frame it fell on.
    pub fn live_midi(&self) -> Vec<(u64, [u8; 3])> {
        self.shared
            .live_midi
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn dropouts(&self) -> Vec<Dropout> {
        self.shared
            .dropouts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Waits for the audio side to let go (send `Record(None)` first), writes what is left and
    /// closes the files. A busy machine only makes the wait longer: it is given up on once the
    /// audio side has rendered nothing for `wait` (a device that stopped), or has rendered on
    /// past its tail without letting go (it was not told to stop).
    pub fn finish(mut self, wait: Duration) -> Recording {
        let begun = self.shared.frames.load(Ordering::Relaxed);
        let mut seen = (begun, Instant::now());
        while !self.detached() {
            let frames = self.shared.frames.load(Ordering::Relaxed);
            if frames != seen.0 {
                seen = (frames, Instant::now());
            } else if seen.1.elapsed() >= wait {
                break;
            }
            if frames - begun > self.overrun {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let detached = self.detached();
        self.stop.store(true, Ordering::Release);
        let mut r = match self.thread.take().map(JoinHandle::join) {
            Some(Ok(r)) => r,
            _ => Recording {
                errors: vec!["録音の書き込みスレッドがクラッシュしました".into()],
                ..Recording::default()
            },
        };
        r.detached = detached;
        r.disk_space_low = self.disk_space_low();
        r.disk_space_unknown = self.disk_space_unknown();
        r.dropped = self.shared.dropped.load(Ordering::Relaxed);
        r.dropped_blocks = self.dropped_blocks();
        r.dropouts = self.dropouts();
        r.lost_marks = self.shared.lost_marks.load(Ordering::Relaxed);
        r.lost_midi = self.shared.lost_midi.load(Ordering::Relaxed);
        r
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The song marks, in capture frames.
#[derive(Clone, Copy, Debug)]
struct SongMark {
    index: u64,
    song: Option<u64>,
}

struct OpenFile {
    failed: bool,
    seg: usize,
    start: u64,
    /// The capture frame of the file's first frame.
    capture: u64,
    path: PathBuf,
    wav: Wav,
    frames: u64,
}

/// A lane on the writer's side: its columns of the ring's frames and its open file.
struct Lane {
    failed_seg: Option<usize>,
    first: usize,
    count: usize,
    path: Box<dyn FnMut(usize) -> PathBuf + Send>,
    file: Option<OpenFile>,
}

/// A finished file.
struct Written {
    seg: usize,
    lane: usize,
    path: PathBuf,
    start: u64,
    capture: u64,
    frames: u64,
}

/// The song frame a MIDI message at capture frame `index` falls on, if a song mark is known for
/// it.
fn song_frame_of(songs: &[SongMark], latency: u64, index: u64) -> Option<u64> {
    let e = index.checked_sub(latency)?;
    let k = songs.partition_point(|m| m.index <= e).checked_sub(1)?;
    let m = songs[k];
    Some(m.song? + (e - m.index))
}

struct Writer {
    spec: RecordSpec,
    audio: Option<Consumer<f32>>,
    /// Samples per frame in the ring.
    stride: usize,
    lanes: Vec<Lane>,
    marks: Consumer<Mark>,
    midi: Option<Consumer<MidiMark>>,
    songs: Vec<SongMark>,
    gaps: VecDeque<(u64, u64)>,
    messages: Vec<MidiMark>,
    /// The note messages the screen is not shown yet: the engine pushes a message before the
    /// mark of its block, so one is shown once the capture has gone past it and every mark that
    /// covers it is known.
    live_wait: Vec<MidiMark>,
    /// Capture frame of the next audio frame.
    idx: u64,
    /// The song mark the audio is in.
    seg: usize,
    files: Vec<Written>,
    errors: Vec<String>,
}

impl Writer {
    fn run(mut self, shared: &Shared, stop: &AtomicBool) -> Recording {
        // One column per picked channel.
        let mut cols: Vec<Vec<f32>> = vec![Vec::new(); self.stride];
        loop {
            // Read the flags first: what was pushed before they were set is then in the rings.
            let ending = shared.done.load(Ordering::Acquire) || stop.load(Ordering::Acquire);
            let avail = match &self.audio {
                Some(a) if self.stride > 0 => a.slots() / self.stride,
                _ => 0,
            };
            self.drain_marks(shared);
            let mut busy = false;
            while let Some(&(at, frames)) = self.gaps.front() {
                if at != self.idx {
                    break;
                }
                self.gaps.pop_front();
                for c in &mut cols {
                    c.clear();
                    c.resize(frames as usize, 0.0);
                }
                self.feed(&cols, frames, shared);
                busy = true;
            }
            let until_gap = self
                .gaps
                .front()
                .map_or(u64::MAX, |&(at, _)| at.saturating_sub(self.idx));
            let take = (avail as u64).min(until_gap) as usize;
            if take > 0 {
                let stride = self.stride;
                let chunk = self
                    .audio
                    .as_mut()
                    .expect("frames are available")
                    .read_chunk(take * stride)
                    .expect("slots() were available");
                let (a, b) = chunk.as_slices();
                for c in &mut cols {
                    c.clear();
                }
                for (k, s) in a.iter().chain(b).enumerate() {
                    cols[k % stride].push(*s);
                }
                chunk.commit_all();
                self.feed(&cols, take as u64, shared);
                busy = true;
            }
            if self
                .lanes
                .iter()
                .any(|l| l.file.as_ref().is_some_and(|f| f.wav.disk_space_low()))
            {
                shared.disk_space_low.store(true, Ordering::Relaxed);
            }
            shared.disk_space_unknown.store(
                self.lanes
                    .iter()
                    .any(|l| l.file.as_ref().is_some_and(|f| f.wav.disk_space_unknown())),
                Ordering::Relaxed,
            );
            if !busy {
                if ending && self.gaps.front().is_none_or(|&(at, _)| at > self.idx) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        self.drain_marks(shared);
        self.close_files();
        let total = shared.frames.load(Ordering::Relaxed).max(self.idx);
        let cut = (shared.cut.load(Ordering::Acquire) && !self.lanes.is_empty())
            .then(|| shared.heard.load(Ordering::Relaxed));
        if let Some(heard) = cut {
            self.cut_files(heard);
        }
        self.result(total, cut)
    }

    /// Cuts every file to the frames captured before `heard`; a file that holds none of them is
    /// removed.
    fn cut_files(&mut self, heard: u64) {
        let errors = &mut self.errors;
        self.files.retain_mut(|f| {
            let keep = heard.saturating_sub(f.capture).min(f.frames);
            if keep == f.frames {
                return true;
            }
            if keep == 0 {
                if let Err(e) = std::fs::remove_file(&f.path) {
                    errors.push(format!("{} を削除できません: {e}", f.path.display()));
                }
                return false;
            }
            match crate::wavfile::shorten(&f.path, keep) {
                Ok(()) => f.frames = keep,
                Err(e) => errors.push(format!("{} を短くできません: {e}", f.path.display())),
            }
            true
        });
    }

    fn drain_marks(&mut self, shared: &Shared) {
        // How far the capture had got before the marks were read: the marks of every block up to
        // there are in the ring (each block's marks are pushed before its frames are counted).
        let seen = shared.frames.load(Ordering::Acquire);
        while let Ok(m) = self.marks.pop() {
            match m {
                Mark::Song { index, song } => self.songs.push(SongMark { index, song }),
                Mark::Gap {
                    index,
                    frames,
                    blocks,
                } => {
                    self.gaps.push_back((index, frames));
                    let song_frame = dropout_song_frame(&self.songs, self.spec.latency, index);
                    shared
                        .dropouts
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(Dropout {
                            capture_frame: index,
                            song_frame,
                            frames,
                            blocks,
                        });
                }
            }
        }
        if let Some(rx) = &mut self.midi {
            while let Ok(m) = rx.pop() {
                self.messages.push(m);
                if note_of(m.msg).is_some() {
                    self.live_wait.push(m);
                }
            }
        }
        // A message the capture has gone past is mapped by the marks that are known now: the
        // very first note of a run is no longer lost to a mark that came a moment later.
        let (ready, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.live_wait)
            .into_iter()
            .partition(|m| m.index < seen);
        self.live_wait = wait;
        if !ready.is_empty() {
            let mut live = shared.live_midi.lock().unwrap_or_else(|e| e.into_inner());
            for m in ready {
                if let Some(frame) = song_frame_of(&self.songs, self.spec.midi_latency, m.index) {
                    live.push((frame, m.msg));
                }
            }
        }
    }

    /// `n` audio frames at capture frames `idx..` (one column per picked channel), shifted back
    /// by the latency onto the song.
    fn feed(&mut self, cols: &[Vec<f32>], n: u64, shared: &Shared) {
        let lat = self.spec.latency;
        let mut k = 0u64;
        while k < n {
            let i = self.idx + k;
            if i < lat {
                k += (lat - i).min(n - k);
                continue;
            }
            let e = i - lat;
            while self.seg + 1 < self.songs.len() && self.songs[self.seg + 1].index <= e {
                self.seg += 1;
            }
            let Some(mark) = self.songs.get(self.seg).copied() else {
                k = n;
                continue;
            };
            let run_end = self
                .songs
                .get(self.seg + 1)
                .map_or(u64::MAX, |m| m.index.saturating_sub(e));
            let run = run_end.min(n - k);
            if e < mark.index {
                // Before the first mark: nothing was playing yet.
                k += (mark.index - e).min(n - k);
                continue;
            }
            let Some(song) = mark.song else {
                k += run;
                continue;
            };
            let start = (self.spec.align)(song);
            let at = song + (e - mark.index);
            let skip = start.saturating_sub(at).min(run);
            let (from, to) = ((k + skip) as usize, (k + run) as usize);
            if from < to {
                self.write(self.seg, start, cols, from, to, shared);
            }
            k += run;
        }
        self.idx += n;
    }

    /// Frames `from..to` of the columns, into every lane's file of song mark `seg`.
    fn write(
        &mut self,
        seg: usize,
        start: u64,
        cols: &[Vec<f32>],
        from: usize,
        to: usize,
        shared: &Shared,
    ) {
        let take = self.songs[..=seg]
            .iter()
            .filter(|m| m.song.is_some())
            .count()
            - 1;
        for (index, lane) in self.lanes.iter_mut().enumerate() {
            if lane.count == 0 || lane.failed_seg == Some(seg) {
                continue;
            }
            if lane.file.as_ref().is_some_and(|f| f.seg != seg) {
                close(lane, index, &mut self.files, &mut self.errors);
            }
            if lane.file.is_none() {
                let path = (lane.path)(take);
                match Wav::create(&path, lane.count as u16, self.spec.sample_rate) {
                    Ok(wav) => {
                        lane.file = Some(OpenFile {
                            failed: false,
                            seg,
                            start,
                            capture: self.idx + from as u64,
                            path,
                            wav,
                            frames: 0,
                        });
                    }
                    Err(e) => {
                        lane.failed_seg = Some(seg);
                        writer_error(
                            shared,
                            &mut self.errors,
                            &e,
                            format!("{} を作成できません: {e}", path.display()),
                        );
                        continue;
                    }
                }
            }
            let f = lane.file.as_mut().expect("opened above");
            if f.failed {
                continue;
            }
            let mine = &cols[lane.first..lane.first + lane.count];
            'frames: for i in from..to {
                for c in mine {
                    if let Err(e) = f.wav.write_sample(c[i]) {
                        f.failed = true;
                        writer_error(
                            shared,
                            &mut self.errors,
                            &e,
                            format!("{} に書き込めません: {e}", f.path.display()),
                        );
                        break 'frames;
                    }
                }
            }
            f.frames = f.wav.frames();
        }
    }

    fn close_files(&mut self) {
        for (index, lane) in self.lanes.iter_mut().enumerate() {
            close(lane, index, &mut self.files, &mut self.errors);
        }
    }

    /// The takes of a capture of `total` frames, ended at capture frame `cut` if one is given.
    fn result(mut self, total: u64, cut: Option<u64>) -> Recording {
        // Where the capture ended, on the song's side of the shift.
        let latency = if self.lanes.is_empty() {
            self.spec.midi_latency
        } else {
            self.spec.latency
        };
        let end = total.min(cut.unwrap_or(u64::MAX)).saturating_sub(latency);
        let mut takes = Vec::new();
        // The take of each song mark.
        let mut of_mark = vec![None; self.songs.len()];
        let mut n = 0;
        for (k, m) in self.songs.iter().enumerate() {
            let Some(song) = m.song else { continue };
            let next = self.songs.get(k + 1).map_or(end, |x| x.index.min(end));
            let len = next.saturating_sub(m.index);
            let mut audio: Vec<AudioTake> = self
                .files
                .iter()
                .filter(|f| f.seg == k)
                .map(|f| AudioTake {
                    lane: f.lane,
                    path: f.path.clone(),
                    start: f.start,
                    frames: f.frames,
                })
                .collect();
            audio.sort_by_key(|a| a.lane);
            if len > 0 || !audio.is_empty() {
                of_mark[k] = Some(takes.len());
                takes.push(Take {
                    index: n,
                    song_start: song,
                    song_end: song + len,
                    audio,
                    midi: Vec::new(),
                });
            }
            n += 1;
        }
        let lat = self.spec.midi_latency;
        for msg in std::mem::take(&mut self.messages) {
            let Some(e) = msg.index.checked_sub(lat) else {
                continue;
            };
            let Some(k) = self.songs.partition_point(|m| m.index <= e).checked_sub(1) else {
                continue;
            };
            let m = self.songs[k];
            let (Some(song), Some(t)) = (m.song, of_mark[k]) else {
                continue;
            };
            let at = song + (e - m.index);
            if at < takes[t].song_end {
                takes[t].midi.push(TakeMidi {
                    at,
                    port: msg.port,
                    msg: msg.msg,
                });
            }
        }
        Recording {
            takes,
            frames: total,
            errors: self.errors,
            ..Recording::default()
        }
    }
}

fn writer_error(
    shared: &Shared,
    errors: &mut Vec<String>,
    error: &std::io::Error,
    message: String,
) {
    if crate::storage::is_disk_full(error) {
        shared.disk_space_low.store(true, Ordering::Relaxed);
    }
    *shared
        .writer_error
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(message.clone());
    errors.push(message);
}

fn dropout_song_frame(songs: &[SongMark], latency: u64, index: u64) -> Option<u64> {
    index.checked_sub(latency).and_then(|corrected| {
        songs
            .iter()
            .rev()
            .find(|m| m.index <= corrected)
            .and_then(|m| m.song.map(|s| s + corrected - m.index))
    })
}

/// Closes a lane's open file and keeps what it wrote.
fn close(lane: &mut Lane, index: usize, files: &mut Vec<Written>, errors: &mut Vec<String>) {
    if let Some(f) = lane.file.take() {
        if let Err(e) = f.wav.finalize() {
            errors.push(format!("{} を閉じられません: {e}", f.path.display()));
        }
        files.push(Written {
            seg: f.seg,
            lane: index,
            path: f.path,
            start: f.start,
            capture: f.capture,
            frames: f.frames,
        });
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    fn midi_only() -> RecordSpec {
        RecordSpec {
            sample_rate: 48_000,
            lanes: Vec::new(),
            midi: true,
            latency: 0,
            midi_latency: 0,
            align: Box::new(|f| f),
            ring_seconds: 1.0,
            thru: Vec::new(),
        }
    }

    /// The engine pushes a block's MIDI messages before the block's mark. A note on the very
    /// first frame therefore reaches the writer with no mark yet: the screen waits for the mark
    /// instead of dropping it, and the finished take holds it too (REC-001).
    #[test]
    fn a_note_on_the_first_frame_is_shown_and_recorded_even_if_its_mark_comes_late() {
        let (rec, mut capture) = Recorder::start(midi_only()).unwrap();
        capture.midi(0, 0, [0x90, 60, 100]);
        // The writer reads the message with no mark in sight: nothing can be placed yet.
        std::thread::sleep(Duration::from_millis(60));
        assert!(rec.live_midi().is_empty());
        // The block's mark and frames come: the note falls on song frame 1000.
        capture.segment(0, 64, Some(1000));
        assert!(!capture.block(&[], 64, 0));
        let t = Instant::now();
        while rec.live_midi().is_empty() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(rec.live_midi(), vec![(1000, [0x90, 60, 100])]);
        capture.close();
        capture.finish();
        let r = rec.finish(Duration::from_secs(5));
        assert_eq!(r.takes.len(), 1, "{r:?}");
        assert_eq!(
            r.takes[0].midi,
            vec![TakeMidi {
                at: 1000,
                port: 0,
                msg: [0x90, 60, 100]
            }]
        );
    }

    /// A run that was not playing (the count-in) places no note: nothing is shown for it.
    #[test]
    fn a_note_while_the_song_waits_is_not_shown() {
        let (rec, mut capture) = Recorder::start(midi_only()).unwrap();
        capture.midi(0, 0, [0x90, 60, 100]);
        capture.segment(0, 64, None);
        capture.block(&[], 64, 0);
        capture.segment(0, 64, Some(500));
        capture.midi(10, 0, [0x90, 64, 100]);
        capture.block(&[], 64, 0);
        let t = Instant::now();
        while rec.live_midi().is_empty() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(rec.live_midi(), vec![(500 + 10, [0x90, 64, 100])]);
        capture.close();
        capture.finish();
        let _ = rec.finish(Duration::from_secs(5));
    }
}

#[cfg(test)]
mod dropout_tests {
    use super::*;
    #[test]
    fn dropout_positions_follow_latency_seeks_loops_and_stopped_capture() {
        let songs = [
            SongMark {
                index: 0,
                song: Some(1000),
            },
            SongMark {
                index: 200,
                song: Some(5000),
            },
            SongMark {
                index: 400,
                song: Some(1000),
            },
            SongMark {
                index: 600,
                song: None,
            },
        ];
        assert_eq!(dropout_song_frame(&songs, 100, 50), None);
        assert_eq!(dropout_song_frame(&songs, 100, 150), Some(1050));
        assert_eq!(dropout_song_frame(&songs, 100, 350), Some(5050));
        assert_eq!(dropout_song_frame(&songs, 100, 550), Some(1050));
        assert_eq!(dropout_song_frame(&songs, 100, 750), None);
    }
}

#[cfg(test)]
mod heard_tests {
    use super::*;

    #[test]
    fn an_input_is_quiet_once_it_has_not_called_back_for_a_second() {
        let t0 = Instant::now();
        let mut heard = Heard::new(3, t0);
        assert!(!heard.quiet(3, t0 + Duration::from_millis(900)));
        // A callback came: the second counts from it.
        assert!(!heard.quiet(4, t0 + Duration::from_millis(950)));
        assert!(!heard.quiet(4, t0 + Duration::from_millis(1900)));
        assert!(heard.quiet(4, t0 + Duration::from_millis(1950)));
    }
}

#[cfg(test)]
mod thru_tests {
    use super::*;

    #[test]
    fn the_thru_plays_the_tracks_transpose_and_velocity() {
        let thru = Thru {
            track: 0,
            port: None,
            transpose: 2,
            velocity: 10,
        };
        let heard = thru
            .heard(NoteKind::On {
                pitch: 60,
                velocity: 100.0 / 127.0,
            })
            .expect("on the keyboard");
        let NoteKind::On { pitch, velocity } = heard else {
            panic!("not a note on");
        };
        assert_eq!(pitch, 62);
        assert!((velocity - 110.0 / 127.0).abs() < 1e-6, "{velocity}");
        // The note off goes to the same key the on played.
        assert_eq!(
            thru.heard(NoteKind::Off { pitch: 60 }),
            Some(NoteKind::Off { pitch: 62 })
        );
        // A key the transpose takes off the keyboard makes no sound, on or off.
        let low = Thru {
            transpose: -64,
            ..thru
        };
        assert_eq!(
            low.heard(NoteKind::On {
                pitch: 60,
                velocity: 0.5
            }),
            None
        );
        assert_eq!(low.heard(NoteKind::Off { pitch: 60 }), None);
        // Controllers are not moved.
        let ctrl = NoteKind::control(64, 0);
        assert_eq!(thru.heard(ctrl), Some(ctrl));
    }

    /// The monitor rounds to the same whole velocity the playing tracks use: 64 with +10% is
    /// 70, not 70.4 (UI-312).
    #[test]
    fn the_thru_rounds_the_velocity_to_a_whole_number_first() {
        let thru = Thru {
            track: 0,
            port: None,
            transpose: 0,
            velocity: 10,
        };
        let heard = thru
            .heard(NoteKind::On {
                pitch: 60,
                velocity: 64.0 / 127.0,
            })
            .expect("on the keyboard");
        assert_eq!(
            heard,
            NoteKind::On {
                pitch: 60,
                velocity: 70.0 / 127.0,
            }
        );
    }

    fn empty_plan() -> crate::mixer::Plan {
        crate::mixer::Plan::build(
            crate::mixer::MixSpec::default(),
            crate::processor::ProcessSetup {
                sample_rate: 48_000.0,
                max_block: 256,
                mode: crate::processor::RenderMode::Realtime,
            },
            None,
        )
        .expect("an empty plan")
    }

    fn capture_with(thru: Vec<Thru>) -> (Recorder, Box<Capture>) {
        Recorder::start(RecordSpec {
            sample_rate: 48_000,
            lanes: Vec::new(),
            midi: false,
            latency: 0,
            midi_latency: 0,
            align: Box::new(|f| f),
            ring_seconds: 1.0,
            thru,
        })
        .expect("a capture")
    }

    /// A value changed while the key is held stops the note at the pitch that was sent, then
    /// the new value takes over (UI-312).
    #[test]
    fn a_value_changed_mid_key_stops_the_note_that_was_sent() {
        let (_rec, mut capture) = capture_with(vec![Thru {
            track: 0,
            port: None,
            transpose: 2,
            velocity: 0,
        }]);
        let mut plan = empty_plan();
        capture.thru_note(
            NoteKind::On {
                pitch: 60,
                velocity: 100.0 / 127.0,
            },
            0,
            &mut plan,
        );
        assert_eq!(
            plan.pushed,
            [(
                0,
                NoteEvent {
                    offset: 0,
                    kind: NoteKind::On {
                        pitch: 62,
                        velocity: 100.0 / 127.0,
                    },
                }
            )]
        );
        capture.set_thru(
            vec![Thru {
                track: 0,
                port: None,
                transpose: 5,
                velocity: 0,
            }],
            &mut plan,
        );
        assert_eq!(
            plan.pushed.last(),
            Some(&(
                0,
                NoteEvent {
                    offset: 0,
                    kind: NoteKind::Off { pitch: 62 },
                }
            )),
            "the held note is stopped at the pitch that was sent"
        );
    }

    /// The off that follows a value change is not sent again at the new value's height: the note
    /// was already stopped (UI-312).
    #[test]
    fn a_stopped_note_is_not_sent_off_again() {
        let (_rec, mut capture) = capture_with(vec![Thru {
            track: 0,
            port: None,
            transpose: 2,
            velocity: 0,
        }]);
        let mut plan = empty_plan();
        capture.thru_note(
            NoteKind::On {
                pitch: 60,
                velocity: 100.0 / 127.0,
            },
            0,
            &mut plan,
        );
        capture.set_thru(
            vec![Thru {
                track: 0,
                port: None,
                transpose: 5,
                velocity: 0,
            }],
            &mut plan,
        );
        let after_stop = plan.pushed.len();
        capture.thru_note(NoteKind::Off { pitch: 60 }, 0, &mut plan);
        assert_eq!(plan.pushed.len(), after_stop, "{:?}", plan.pushed);
    }

    /// A port alone changing stops the notes held on that track: the off may come from a port
    /// the track no longer listens to (UI-312).
    #[test]
    fn a_port_change_stops_the_held_note() {
        let (_rec, mut capture) = capture_with(vec![Thru {
            track: 0,
            port: None,
            transpose: 2,
            velocity: 0,
        }]);
        let mut plan = empty_plan();
        capture.thru_note(
            NoteKind::On {
                pitch: 60,
                velocity: 100.0 / 127.0,
            },
            0,
            &mut plan,
        );
        let old = capture.set_thru(
            vec![Thru {
                track: 0,
                port: Some(1),
                transpose: 2,
                velocity: 0,
            }],
            &mut plan,
        );
        assert_eq!(
            old.len(),
            1,
            "the old values come back to be freed elsewhere"
        );
        assert_eq!(
            plan.pushed.last(),
            Some(&(
                0,
                NoteEvent {
                    offset: 0,
                    kind: NoteKind::Off { pitch: 62 },
                }
            ))
        );
    }

    /// More notes held than the fixed array takes: the one that does not fit is not sounded, so
    /// nothing can be left with no way to stop it (UI-312).
    #[test]
    fn a_note_that_does_not_fit_is_not_sounded() {
        let (_rec, mut capture) = capture_with(vec![
            Thru {
                track: 0,
                port: Some(0),
                transpose: 0,
                velocity: 0,
            },
            Thru {
                track: 1,
                port: Some(1),
                transpose: 0,
                velocity: 0,
            },
        ]);
        let mut plan = empty_plan();
        for pitch in 0..=127u8 {
            capture.thru_note(
                NoteKind::On {
                    pitch,
                    velocity: 100.0 / 127.0,
                },
                0,
                &mut plan,
            );
        }
        let ons = plan
            .pushed
            .iter()
            .filter(|(_, e)| matches!(e.kind, NoteKind::On { .. }))
            .count();
        assert_eq!(ons, 128);
        capture.thru_note(
            NoteKind::On {
                pitch: 60,
                velocity: 100.0 / 127.0,
            },
            0,
            &mut plan,
        );
        capture.thru_note(NoteKind::Off { pitch: 60 }, 0, &mut plan);
        assert_eq!(
            plan.pushed
                .iter()
                .filter(|(t, e)| *t == 1 && matches!(e.kind, NoteKind::On { .. }))
                .count(),
            0,
            "the note that did not fit is silent"
        );
        assert_eq!(
            plan.pushed
                .iter()
                .filter(|(t, e)| *t == 1 && matches!(e.kind, NoteKind::Off { .. }))
                .count(),
            0,
            "and its off is not sent either"
        );
    }
}
