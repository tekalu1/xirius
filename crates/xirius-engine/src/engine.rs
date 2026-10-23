//! The real-time side and the control side of the engine, joined only by lock-free queues
//! (ENG-005, ENG-006).
//!
//! - [`Controller`] lives on the UI/control thread. It sends [`Command`]s, builds new plans off
//!   the audio thread, receives [`EngineEvent`]s (meters, position) and frees replaced plans.
//! - [`AudioEngine`] lives on the audio thread. [`AudioEngine::render`] drains commands, chops
//!   the device buffer into blocks no longer than `max_block` (ENG-023) and runs the plan. It never
//!   allocates, locks, waits or frees a plan (ENG-004).
//!
//! The transport (play, stop, seek, loop) is part of the audio side: while it plays, the position
//! in the song moves with the frames rendered, the plan plays each track's [`Timeline`] at that
//! position, and a block that would cross the loop end or the song end is cut there.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::click::{Click, CountIn};
use crate::midi_out::MidiOutput;
use crate::mixer::{MeterReading, MixSpec, NodeId, Plan, PlanError, PlanShape};
use crate::monitor::InputState;
use crate::processor::{NoteEvent, NoteKind, ProcessSetup};
use crate::record::{AudioInput, Capture, MAX_INPUT_CHANNELS, MidiInput, note_of};
use crate::stats::Stats;
use crate::timeline::Timeline;
use crate::tone::Tone;

pub enum Command {
    TrackLive {
        track: usize,
        live: bool,
    },
    TrackGain {
        track: usize,
        gain: f32,
    },
    TrackPan {
        track: usize,
        pan: f32,
    },
    TrackMute {
        track: usize,
        mute: bool,
    },
    SendLevel {
        track: usize,
        send: usize,
        level: f32,
    },
    BusGain {
        bus: usize,
        gain: f32,
    },
    BusPan {
        bus: usize,
        pan: f32,
    },
    MasterGain(f32),
    /// Physical destination of the master, zero-based.
    OutputChannels([usize; 2]),
    BusMute {
        bus: usize,
        mute: bool,
    },
    BusSendLevel {
        bus: usize,
        send: usize,
        level: f32,
    },
    /// Turns a keyed insert's external sidechain on or off (ENG-078). `plugin`
    /// is the receiving plug-in's id.
    KeyOn {
        plugin: u64,
        on: bool,
    },
    /// The level of one key source (its send row's id).
    KeyLevel {
        send: u64,
        level: f32,
    },
    /// Replaces what a track plays on the timeline. The old one goes back through the garbage
    /// queue.
    SetTimeline {
        track: usize,
        timeline: Box<Timeline>,
    },
    /// Starts moving the song position and playing the timelines.
    Play,
    /// Stops where it is and ends the notes that sound.
    Stop,
    /// Moves the song position, in frames.
    Seek(u64),
    /// Host seek generation, returned with transport notifications.
    SeekTracked {
        frame: u64,
        serial: u64,
    },
    /// Repeat `start..end` (frames) while playing; `None` plays on.
    SetLoop(Option<(u64, u64)>),
    /// Stop by itself when the position reaches this frame (not while looping).
    SetEnd(Option<u64>),
    /// Played at the start of the next block.
    Note {
        track: usize,
        kind: NoteKind,
    },
    SwapPlan(Box<Plan>),
    /// The audio input read every block (`None`: none). The old one goes back through the
    /// garbage queue.
    SetInput(Option<Box<dyn AudioInput>>),
    /// The MIDI input read every block.
    SetMidiInput(Option<Box<dyn MidiInput>>),
    /// The MIDI output the tracks with a MIDI port send to (ENG-028). The old one goes back
    /// through the garbage queue.
    SetMidiOutput(Option<Box<dyn MidiOutput>>),
    /// `Some`: start capturing the inputs into this recording (one that was running is let go).
    /// `None`: stop once the input has caught up with what was played (see [`crate::record`]).
    Record(Option<Box<Capture>>),
    /// What the recording's tracks play as the MIDI input comes in, changed while it records
    /// (UI-312). Ignored when no recording runs.
    Thru(Vec<crate::record::Thru>),
    /// The metronome, heard on the device only (`None`: off). The old one goes back through the
    /// garbage queue.
    SetClick(Option<Box<Click>>),
    /// Counts in before the next [`Command::Play`]: the song waits this long, with the
    /// transport playing and the position where it is. A stop or a seek calls it off. The old
    /// one goes back through the garbage queue.
    SetCountIn(Option<Box<CountIn>>),
    /// Plays the test tone once on the device (UI-260); `None` stops one that sounds. The
    /// old one goes back through the garbage queue.
    SetTone(Option<Box<Tone>>),
    /// A splitter branch's gain (linear), pan and mute at the merge (ENG-074), by the branch's
    /// node id, wherever its splitter is. A muted branch is not processed.
    BranchGain {
        node: u64,
        gain: f32,
        pan: f32,
        mute: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EngineEvent {
    Meter {
        node: NodeId,
        reading: MeterReading,
    },
    PlanSwapped,
    /// Frames rendered since the engine started.
    Position(u64),
    /// The transport started, stopped or jumped, and every `meter_interval` while it plays.
    /// `position` is in frames from the start of the song.
    Transport {
        playing: bool,
        position: u64,
        serial: u64,
    },
}

/// What the audio thread hands back to be freed elsewhere. The values are only held, so that
/// dropping them happens on the control side.
#[allow(dead_code)]
pub(crate) enum Garbage {
    Plan(Box<Plan>),
    Timeline(Box<Timeline>),
    Input(Box<dyn AudioInput>),
    MidiInput(Box<dyn MidiInput>),
    MidiOutput(Box<dyn MidiOutput>),
    Capture(Box<Capture>),
    Click(Box<Click>),
    CountIn(Box<CountIn>),
    Tone(Box<Tone>),
    /// The old values of a recording's monitored tracks (UI-312).
    Thru(Vec<crate::record::Thru>),
}

#[derive(Clone, Copy, Debug, Default)]
struct Transport {
    sought: bool,
    serial: u64,
    playing: bool,
    pos: u64,
    loop_range: Option<(u64, u64)>,
    end: Option<u64>,
    /// Something changed that the control side has not been told about.
    changed: bool,
}

/// How many frames of a block of `want` can run before the loop end or the song end, and the song
/// position they start at (`None` when stopped). Wraps or stops first if the position is already
/// there.
fn next_segment(tr: &mut Transport, plan: &mut Plan, want: usize) -> (usize, Option<u64>) {
    if !tr.playing {
        return (want, None);
    }
    match (tr.loop_range, tr.end) {
        (Some((start, end)), _) => {
            if tr.pos >= end {
                tr.pos = start;
                plan.release_notes();
            }
            (want.min((end - tr.pos) as usize), Some(tr.pos))
        }
        (None, Some(end)) => {
            if tr.pos >= end {
                tr.playing = false;
                tr.changed = true;
                plan.release_notes();
                return (want, None);
            }
            (want.min((end - tr.pos) as usize), Some(tr.pos))
        }
        (None, None) => (want, Some(tr.pos)),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EngineConfig {
    pub command_capacity: usize,
    pub event_capacity: usize,
    /// How often meters and the position are reported.
    pub meter_interval: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            command_capacity: 1024,
            event_capacity: 4096,
            meter_interval: Duration::from_millis(20),
        }
    }
}

/// Optional copy of the master output for a writer thread (see [`crate::tap`]).
pub type TapProducer = Producer<f32>;

pub struct AudioEngine {
    plan: Box<Plan>,
    commands: Consumer<Command>,
    events: Producer<EngineEvent>,
    garbage: Producer<Garbage>,
    transport: Transport,
    left: Vec<f32>,
    right: Vec<f32>,
    position: u64,
    meter_every: u64,
    since_meter: u64,
    stats: Arc<Stats>,
    tap: Option<TapProducer>,
    last_callback: Option<Instant>,
    /// Shortest callback seen, taken as the device period for spotting late callbacks.
    period: Option<Duration>,
    /// Plans that could not be handed back because the garbage queue was full; leaked, not freed.
    leaked_plans: u64,
    silent: bool,
    output_channels: [usize; 2],
    input: Option<Box<dyn AudioInput>>,
    midi_in: Option<Box<dyn MidiInput>>,
    midi_out: Option<Box<dyn MidiOutput>>,
    capture: Option<Box<Capture>>,
    /// The input of one block, channel after channel (see [`AudioInput::read_channels`]).
    in_buf: Vec<f32>,
    click: Option<Box<Click>>,
    count_in: Option<Box<CountIn>>,
    click_buf: Vec<f32>,
    tone: Option<Box<Tone>>,
    /// Levels, trims and the monitor of the input (REC-002, REC-011).
    input_state: Arc<InputState>,
    /// The monitored input of one block, left and right.
    mon_l: Vec<f32>,
    mon_r: Vec<f32>,
}

pub struct Controller {
    input_state: Arc<InputState>,
    commands: Producer<Command>,
    events: Consumer<EngineEvent>,
    garbage: Consumer<Garbage>,
    shape: PlanShape,
    stats: Arc<Stats>,
}

/// Splits a prepared plan into the audio-thread half and the control half.
pub fn create(plan: Plan, config: EngineConfig) -> (AudioEngine, Controller) {
    let setup = plan.setup();
    let shape = plan.shape();
    let (cmd_tx, cmd_rx) = RingBuffer::new(config.command_capacity);
    let (ev_tx, ev_rx) = RingBuffer::new(config.event_capacity);
    let (gb_tx, gb_rx) = RingBuffer::new(1024);
    let stats = Arc::new(Stats::default());
    let input_state = Arc::new(InputState::default());
    let meter_every =
        ((config.meter_interval.as_secs_f64() * setup.sample_rate).round() as u64).max(1);
    let audio = AudioEngine {
        plan: Box::new(plan),
        commands: cmd_rx,
        events: ev_tx,
        garbage: gb_tx,
        transport: Transport::default(),
        left: vec![0.0; setup.max_block],
        right: vec![0.0; setup.max_block],
        position: 0,
        meter_every,
        since_meter: 0,
        stats: stats.clone(),
        tap: None,
        last_callback: None,
        period: None,
        leaked_plans: 0,
        silent: false,
        output_channels: [0, 1],
        input: None,
        midi_in: None,
        midi_out: None,
        capture: None,
        in_buf: vec![0.0; setup.max_block * MAX_INPUT_CHANNELS],
        click: None,
        count_in: None,
        click_buf: vec![0.0; setup.max_block],
        tone: None,
        input_state: input_state.clone(),
        mon_l: vec![0.0; setup.max_block],
        mon_r: vec![0.0; setup.max_block],
    };
    let control = Controller {
        input_state,
        commands: cmd_tx,
        events: ev_rx,
        garbage: gb_rx,
        shape,
        stats,
    };
    (audio, control)
}

impl AudioEngine {
    pub fn setup(&self) -> ProcessSetup {
        self.plan.setup()
    }

    pub fn stats(&self) -> Arc<Stats> {
        self.stats.clone()
    }

    /// The levels, trims and monitor of the input, shared with the control side.
    pub fn input_state(&self) -> Arc<InputState> {
        self.input_state.clone()
    }

    pub fn set_tap(&mut self, tap: TapProducer) {
        self.tap = Some(tap);
    }

    /// Renders, meters and taps as usual but hands the device zeros, so tests on a machine in
    /// use make no sound. The work per callback is the same.
    pub fn set_silent(&mut self, silent: bool) {
        self.silent = silent;
    }

    pub fn leaked_plans(&self) -> u64 {
        self.leaked_plans
    }

    pub fn is_playing(&self) -> bool {
        self.transport.playing
    }
    /// Disk underruns and reader failures, for offline callers to refuse a damaged export.
    pub fn stream_health(&self) -> (u64, bool) {
        self.plan.stream_health()
    }
    pub fn latency(&self) -> u32 {
        self.plan.latency()
    }
    pub fn lookahead(&self, track: usize) -> Option<crate::anticipation::AheadStatus> {
        self.plan.lookahead(track)
    }
    /// How the worker prefetch of a track's instrument and inserts is doing (ENG-015).
    pub fn prefetch(&self, track: usize) -> Option<crate::prefetch::PrefetchStatus> {
        self.plan.prefetch_status(track)
    }

    /// The song position in frames.
    pub fn song_position(&self) -> u64 {
        self.transport.pos
    }

    /// Whether a recording is being captured.
    pub fn is_recording(&self) -> bool {
        self.capture.is_some()
    }

    fn let_go_capture(&mut self) {
        if let Some(mut c) = self.capture.take() {
            c.finish();
            self.hand_back(Garbage::Capture(c));
        }
    }

    fn hand_back(&mut self, old: Garbage) {
        if let Err(rtrb::PushError::Full(old)) = self.garbage.push(old) {
            // Freeing here could take a lock in the allocator; leaking is the lesser harm.
            std::mem::forget(old);
            self.leaked_plans += 1;
        }
    }

    fn apply(&mut self, cmd: Command) {
        let serial = match &cmd {
            Command::SeekTracked { serial, .. } => Some(*serial),
            _ => None,
        };
        let plan = &mut self.plan;
        match cmd {
            Command::TrackGain { track, gain } => plan.set_track_gain(track, gain),
            Command::TrackLive { track, live } => plan.set_track_live(track, live),
            Command::TrackPan { track, pan } => plan.set_track_pan(track, pan),
            Command::TrackMute { track, mute } => plan.set_track_mute(track, mute),
            Command::SendLevel { track, send, level } => plan.set_send_level(track, send, level),
            Command::BusGain { bus, gain } => plan.set_bus_gain(bus, gain),
            Command::BusPan { bus, pan } => plan.set_bus_pan(bus, pan),
            Command::MasterGain(gain) => plan.set_master_gain(gain),
            Command::OutputChannels(pair) => self.output_channels = pair,
            Command::BusMute { bus, mute } => plan.set_bus_mute(bus, mute),
            Command::BusSendLevel { bus, send, level } => plan.set_bus_send_level(bus, send, level),
            Command::KeyOn { plugin, on } => plan.set_key_on(plugin, on),
            Command::KeyLevel { send, level } => plan.set_key_level(send, level),
            Command::SetTimeline { track, timeline } => {
                let old = match plan.set_timeline(track, timeline) {
                    Ok(old) | Err(old) => old,
                };
                self.hand_back(Garbage::Timeline(old));
            }
            Command::Play => {
                self.transport.playing = true;
                self.transport.changed = true;
            }
            Command::Stop => {
                self.end_count_in();
                let plan = &mut self.plan;
                if self.transport.playing {
                    plan.release_notes();
                }
                self.transport.playing = false;
                self.transport.changed = true;
            }
            Command::Seek(frame) | Command::SeekTracked { frame, .. } => {
                if let Some(serial) = serial {
                    self.transport.serial = serial;
                }
                self.end_count_in();
                // Also when stopped: the next play starts with the controllers at their values here.
                let plan = &mut self.plan;
                plan.release_notes();
                self.transport.pos = frame;
                self.transport.sought = true;
                self.transport.changed = true;
            }
            Command::SetLoop(range) => {
                self.transport.loop_range = range.filter(|&(s, e)| e > s);
                self.transport.changed = true;
            }
            Command::SetEnd(end) => self.transport.end = end,
            Command::Note { track, kind } => {
                plan.push_event(track, NoteEvent { offset: 0, kind });
            }
            Command::SwapPlan(mut new) => {
                new.take_kept_from(plan);
                // Notes the tracks that were not carried over still hold on a MIDI port end.
                plan.release_midi(self.midi_out.as_deref_mut());
                new.bind_midi(self.midi_out.as_deref());
                let old = std::mem::replace(plan, new);
                self.hand_back(Garbage::Plan(old));
                let _ = self.events.push(EngineEvent::PlanSwapped);
            }
            Command::SetInput(input) => {
                if let Some(old) = std::mem::replace(&mut self.input, input) {
                    self.hand_back(Garbage::Input(old));
                }
            }
            Command::SetMidiInput(input) => {
                if let Some(old) = std::mem::replace(&mut self.midi_in, input) {
                    self.hand_back(Garbage::MidiInput(old));
                }
            }
            Command::SetMidiOutput(mut output) => {
                plan.release_midi(self.midi_out.as_deref_mut());
                plan.bind_midi(output.as_deref());
                if let Some(old) = std::mem::replace(&mut self.midi_out, output.take()) {
                    self.hand_back(Garbage::MidiOutput(old));
                }
            }
            Command::Record(Some(new)) => {
                self.let_go_capture();
                self.capture = Some(new);
            }
            Command::Record(None) => {
                if self.capture.as_mut().is_some_and(|c| c.close()) {
                    self.let_go_capture();
                }
            }
            Command::Thru(thru) => {
                let old = match &mut self.capture {
                    Some(c) => c.set_thru(thru, plan),
                    None => thru,
                };
                self.hand_back(Garbage::Thru(old));
            }
            Command::SetClick(click) => {
                if let Some(old) = std::mem::replace(&mut self.click, click) {
                    self.hand_back(Garbage::Click(old));
                }
            }
            Command::SetCountIn(count) => {
                if let Some(old) = std::mem::replace(&mut self.count_in, count) {
                    self.hand_back(Garbage::CountIn(old));
                }
            }
            Command::SetTone(tone) => {
                self.stats
                    .set_tone_left(tone.as_ref().map_or(0, |t| u64::from(t.left())));
                if let Some(old) = std::mem::replace(&mut self.tone, tone) {
                    self.hand_back(Garbage::Tone(old));
                }
            }
            Command::BranchGain {
                node,
                gain,
                pan,
                mute,
            } => plan.set_branch_gain(node, gain, pan, mute),
        }
    }

    /// Calls off the count-in that waits or runs.
    fn end_count_in(&mut self) {
        if let Some(old) = self.count_in.take() {
            self.hand_back(Garbage::CountIn(old));
            self.stats.set_count_in_left(0);
        }
    }

    /// Fills an interleaved device buffer. Channel 0 gets the left mix and channel 1 the right;
    /// a mono device gets their average and further channels get silence.
    pub fn render(&mut self, out: &mut [f32], channels: usize) {
        let _audio = crate::rt_lock::AudioScope::enter();
        assert_no_alloc::assert_no_alloc(|| self.render_inner(out, channels));
    }

    /// [`Self::render`] plus timing statistics for one device callback.
    pub fn render_timed(&mut self, out: &mut [f32], channels: usize) {
        let start = Instant::now();
        self.render(out, channels);
        let busy = start.elapsed();
        let frames = out.len() / channels.max(1);
        let budget = Duration::from_secs_f64(frames as f64 / self.plan.setup().sample_rate);
        let late = match (self.last_callback, self.period) {
            (Some(last), Some(period)) => (start - last) * 2 > period * 3,
            _ => false,
        };
        self.last_callback = Some(start);
        if frames > 0 {
            self.period = Some(self.period.map_or(budget, |p| p.min(budget)));
        }
        self.stats.record_callback(frames, busy, budget, late);
    }

    fn notify_transport(&mut self) {
        if self.transport.changed
            && self
                .events
                .push(EngineEvent::Transport {
                    playing: self.transport.playing,
                    position: self.transport.pos,
                    serial: self.transport.serial,
                })
                .is_ok()
        {
            self.transport.changed = false;
        }
    }

    fn render_inner(&mut self, out: &mut [f32], channels: usize) {
        while let Ok(cmd) = self.commands.pop() {
            self.apply(cmd);
        }
        if std::mem::take(&mut self.transport.sought) {
            self.plan.settle_controls();
        }
        self.notify_transport();
        if channels == 0 {
            return;
        }
        let max_block = self.plan.setup().max_block;
        let mut tap_dropped = 0;
        let silent = self.silent;
        for (chunk_index, chunk) in out.chunks_mut(max_block * channels).enumerate() {
            let n = chunk.len() / channels;
            if let Some(midi) = &mut self.midi_in {
                let (capture, plan) = (&mut self.capture, &mut self.plan);
                midi.read_ports(n, &mut |offset, port, msg| {
                    if let Some(c) = capture {
                        c.midi(offset, port, msg);
                        if let Some(kind) = note_of(msg) {
                            c.thru_note(kind, port, plan);
                        }
                    }
                });
            }
            let click = &mut self.click_buf[..n];
            click.fill(0.0);
            let mut done = 0;
            while done < n {
                // The song waits while the count-in clicks: playing, but not moving.
                let (seg, at) = match &mut self.count_in {
                    Some(c) if self.transport.playing => {
                        let seg = c.render(&mut self.click_buf[done..n]);
                        (seg, None)
                    }
                    _ => next_segment(&mut self.transport, &mut self.plan, n - done),
                };
                if let Some(c) = &mut self.capture {
                    c.segment(done, seg, at);
                }
                self.plan.process_at(
                    &mut self.left[done..done + seg],
                    &mut self.right[done..done + seg],
                    at,
                );
                // Messages are timed from the start of the device block.
                let at_frame = chunk_index * max_block + done;
                self.plan.flush_midi(self.midi_out.as_deref_mut(), at_frame);
                if self.count_in.as_ref().is_some_and(|c| c.is_over()) {
                    self.end_count_in();
                } else if let Some(c) = &self.count_in {
                    self.stats.set_count_in_left(c.left());
                }
                if let (Some(c), Some(at)) = (&mut self.click, at) {
                    c.render(&mut self.click_buf[done..done + seg], at);
                }
                if at.is_some() {
                    self.transport.pos += seg as u64;
                }
                done += seg;
            }
            let (l, r) = (&self.left[..n], &self.right[..n]);
            // The input is read after the mix was rendered (a loopback hears it), and before the
            // device is written, so the monitor can join it.
            let (heard, delivered) = match &mut self.input {
                Some(input) => {
                    input.played(l, r);
                    let ch = input.channels().clamp(1, MAX_INPUT_CHANNELS);
                    let buf = &mut self.in_buf[..ch * n];
                    input.read_channels(buf, n);
                    let delivered = input.delivered(n);
                    self.input_state.process(buf, n);
                    (&*buf, delivered)
                }
                None => (&[][..], 0),
            };
            let (mon_l, mon_r) = (&mut self.mon_l[..n], &mut self.mon_r[..n]);
            mon_l.fill(0.0);
            mon_r.fill(0.0);
            self.input_state.mix(heard, n, mon_l, mon_r);
            // The click, the test tone and the monitor go to the device only: not into the
            // meters, the tap or the recording. The output's peak is kept before a silent output
            // drops it, so a test reads what would have been heard.
            let click = &self.click_buf[..n];
            let mut peak = [0f32; 2];
            for (i, frame) in chunk.chunks_exact_mut(channels).enumerate() {
                let tone = self.tone.as_mut().map_or([0.0; 2], |t| t.next_frame());
                let pair = [
                    l[i] + mon_l[i] + click[i] + tone[0],
                    r[i] + mon_r[i] + click[i] + tone[1],
                ];
                peak = [peak[0].max(pair[0].abs()), peak[1].max(pair[1].abs())];
                if silent {
                    frame.fill(0.0);
                } else if channels == 1 {
                    frame[0] =
                        0.5 * (l[i] + r[i] + mon_l[i] + mon_r[i]) + click[i] + tone[0] + tone[1];
                } else {
                    frame.fill(0.0);
                    for (channel, sample) in self.output_channels.into_iter().zip(pair) {
                        if let Some(out) = frame.get_mut(channel) {
                            *out += sample;
                        }
                    }
                }
            }
            self.stats.record_output_peak(peak);
            if let Some(tap) = &mut self.tap {
                for i in 0..n {
                    for s in [l[i], r[i]] {
                        if tap.push(s).is_err() {
                            tap_dropped += 1;
                        }
                    }
                }
            }
            if self
                .capture
                .as_mut()
                .is_some_and(|c| c.block(heard, n, delivered))
            {
                self.let_go_capture();
            }
            if let Some(t) = &self.tone {
                self.stats.set_tone_left(u64::from(t.left()));
                if t.is_over()
                    && let Some(old) = self.tone.take()
                {
                    self.hand_back(Garbage::Tone(old));
                }
            }
            self.position += n as u64;
            self.since_meter += n as u64;
            if self.since_meter >= self.meter_every {
                self.since_meter = 0;
                let events = &mut self.events;
                self.plan.drain_meters(|node, reading| {
                    let _ = events.push(EngineEvent::Meter { node, reading });
                });
                let _ = events.push(EngineEvent::Position(self.position));
                if self.transport.playing {
                    self.transport.changed = true;
                }
            }
        }
        self.notify_transport();
        if tap_dropped > 0 {
            self.stats.record_tap_dropped(tap_dropped);
        }
    }
}

impl Controller {
    pub fn stats(&self) -> Arc<Stats> {
        self.stats.clone()
    }

    /// The levels, trims and monitor of the input, shared with the audio thread.
    pub fn input_state(&self) -> Arc<InputState> {
        self.input_state.clone()
    }

    /// The shape of the last plan sent, which the next [`Self::swap`] is checked against.
    pub fn shape(&self) -> PlanShape {
        self.shape.clone()
    }

    /// Queues a command. Gives it back if the queue is full.
    pub fn send(&mut self, cmd: Command) -> Result<(), Command> {
        if let Command::SwapPlan(plan) = &cmd {
            let shape = plan.shape();
            self.commands
                .push(cmd)
                .map_err(|rtrb::PushError::Full(c)| c)?;
            self.shape = shape;
            return Ok(());
        }
        self.commands
            .push(cmd)
            .map_err(|rtrb::PushError::Full(c)| c)
    }

    /// Builds and prepares a plan here, then hands it to the audio thread to swap in at the
    /// start of its next callback.
    pub fn swap(&mut self, spec: MixSpec) -> Result<(), PlanError> {
        let plan = Plan::build(spec, self.shape.setup, Some(self.shape.clone()))?;
        if self.send(Command::SwapPlan(Box::new(plan))).is_err() {
            // The queue is full; dropping the plan here is fine (we are not on the audio thread).
            return Err(PlanError::QueueFull);
        }
        Ok(())
    }

    /// Delivers pending events to `f` and frees plans the audio thread has handed back.
    pub fn poll(&mut self, mut f: impl FnMut(EngineEvent)) {
        while let Ok(ev) = self.events.pop() {
            f(ev);
        }
        while let Ok(old) = self.garbage.pop() {
            drop(old);
        }
    }
}

/// Renders `frames` frames of a plan without a device (ENG-024), returning left and right.
pub fn render_offline(plan: Plan, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut audio, _control) = create(plan, EngineConfig::default());
    let mut out = vec![0.0; frames * 2];
    audio.render(&mut out, 2);
    let left = out.iter().step_by(2).copied().collect();
    let right = out.iter().skip(1).step_by(2).copied().collect();
    (left, right)
}
