//! The mixer graph: track → inserts → fader/pan → sends → bus → master (ENG-009).
//!
//! A bus outputs to the master or to a bus with a higher index, and sends go from a track or a
//! bus to a bus with a higher index (ENG-034), so the graph cannot contain a cycle and the
//! processing order is fixed: tracks, buses in index order, master. The caller numbers the buses
//! so that every bus comes before the buses it feeds.
//!
//! A [`Plan`] is built and prepared on a non-real-time thread with every buffer allocated up
//! front. The audio thread only calls [`Plan::process`] and the setters, which do not allocate.
//!
//! Plugin delay compensation (ENG-010): when the plan is built, the latency of every path is
//! added up, and each connection into a bus or the master gets a delay line that makes all the
//! paths meeting there equally late. Sends and pre-fader sends are compensated the same way. The
//! whole mix is then [`Plan::latency`] frames late, which offline rendering skips.

use std::collections::{BTreeSet, HashMap};
use std::f32::consts::FRAC_PI_2;

use crate::control::{CONTROL_NUMBERS, index as control_index, rest_value};
use crate::midi_out::{MidiOutput, midi_message};
use crate::prefetch::{Chained, Prefetch, PrefetchStatus, Take};
use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor};
use crate::timeline::Timeline;

/// Most note events a track buffers between two blocks; more are dropped and counted.
pub const MAX_EVENTS_PER_BLOCK: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Master,
    Bus(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeId {
    Track(usize),
    Bus(usize),
    Master,
}

pub struct TrackSpec {
    /// A failed preparation stays silent, independently of later mute/monitor commands.
    pub unavailable: bool,
    /// Recording or monitoring; always rendered synchronously.
    pub live: bool,
    pub name: String,
    pub source: Box<dyn Processor>,
    pub inserts: Vec<Box<dyn Processor>>,
    /// Linear amplitude.
    pub gain: f32,
    /// -1.0 (left) ..= 1.0 (right).
    pub pan: f32,
    pub mute: bool,
    pub sends: Vec<SendSpec>,
    pub output: Output,
    /// Played while the transport runs.
    pub timeline: Timeline,
    /// A MIDI port the track's notes and controllers go to (ENG-028).
    pub midi: Option<Box<MidiTarget>>,
    /// Whether the instrument and inserts may be rendered ahead of the transport by a worker
    /// (ENG-015). A track that is `live`, or has a MIDI port, is rendered on the audio thread
    /// whatever this says; an offline plan never prefetches. A keyed track is never lent out:
    /// its inserts need this block's key, not the key a worker rendered ahead.
    pub prefetch: bool,
    /// The sidechain keys the track's inserts take, in insert order (ENG-078).
    pub keys: Vec<KeySpec>,
}

/// A MIDI output port and the channel a track sends on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MidiTarget {
    pub port: String,
    /// 0..=15.
    pub channel: u8,
}

/// A track's MIDI route, with the port resolved against the output the engine holds.
struct MidiRoute {
    target: MidiTarget,
    port: Option<usize>,
    /// Messages of the block: the frame in it and the bytes.
    out: Vec<(u32, [u8; 3])>,
    /// Pitches that were sent a note-on and no note-off.
    sounding: [bool; 128],
}

impl MidiRoute {
    /// Turns the block's note events into messages, keeping track of the notes that sound.
    fn take_events(&mut self, events: &[NoteEvent]) {
        for e in events {
            let Some(msg) = midi_message(e.kind, self.target.channel) else {
                continue;
            };
            match e.kind {
                NoteKind::On { pitch, .. } => self.sounding[(pitch & 0x7f) as usize] = true,
                NoteKind::Off { pitch } => self.sounding[(pitch & 0x7f) as usize] = false,
                NoteKind::Control { .. } => {}
            }
            if self.out.len() < self.out.capacity() {
                self.out.push((e.offset, msg));
            }
        }
    }

    /// Calls `send` with a note-off for every note that sounds.
    fn release_all(&mut self, mut send: impl FnMut([u8; 3])) {
        let ch = self.target.channel & 0x0f;
        for (pitch, on) in self.sounding.iter_mut().enumerate() {
            if std::mem::take(on) {
                send([0x80 | ch, pitch as u8, 0]);
            }
        }
    }
}

impl TrackSpec {
    pub fn new(name: impl Into<String>, source: Box<dyn Processor>) -> Self {
        Self {
            unavailable: false,
            live: false,
            name: name.into(),
            source,
            inserts: Vec::new(),
            gain: 1.0,
            pan: 0.0,
            mute: false,
            sends: Vec::new(),
            output: Output::Master,
            timeline: Timeline::default(),
            midi: None,
            prefetch: true,
            keys: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SendSpec {
    pub bus: usize,
    pub level: f32,
    /// Taken after the inserts and before the fader; otherwise after fader and pan.
    pub pre_fader: bool,
}

/// The node a sidechain key's signal is taken from (ENG-078).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyFrom {
    Track(usize),
    Bus(usize),
}

/// One signal summed into a sidechain key (ENG-078).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeySourceSpec {
    /// The send row's id, for [`Plan::set_key_level`].
    pub id: u64,
    /// The node the signal is taken from.
    pub from: KeyFrom,
    /// `true`: taken after the inserts and before the fader (the mute still
    /// applies); `false`: after fader and pan.
    pub pre: bool,
    /// Linear amplitude.
    pub level: f32,
}

/// A sidechain key one insert takes: the signals summed into it, and whether
/// the insert listens to them at all (ENG-078).
#[derive(Clone, Debug, PartialEq)]
pub struct KeySpec {
    /// The insert in the node's `inserts` that hears the key.
    pub insert: usize,
    /// The receiving plug-in's id, for [`Plan::set_key_on`].
    pub plugin: u64,
    /// `false`: the insert takes its own signal as its key (the model's Int).
    pub on: bool,
    /// The signals the key is summed from, in order.
    pub sources: Vec<KeySourceSpec>,
}

pub struct BusSpec {
    /// A failed bus never passes incoming audio through to another route.
    pub unavailable: bool,
    pub name: String,
    pub inserts: Vec<Box<dyn Processor>>,
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    /// The master, or a bus with a higher index than this one.
    pub output: Output,
    /// To buses with a higher index than this one.
    pub sends: Vec<SendSpec>,
    /// The sidechain keys the bus's inserts take, in insert order (ENG-078).
    pub keys: Vec<KeySpec>,
}

impl BusSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            unavailable: false,
            name: name.into(),
            inserts: Vec::new(),
            gain: 1.0,
            pan: 0.0,
            mute: false,
            output: Output::Master,
            sends: Vec::new(),
            keys: Vec::new(),
        }
    }
}

/// A track in a new plan: either built fresh, or carried over from the plan being replaced so
/// that its processors keep their state (oscillator phase, plugin instance) across the swap.
#[allow(clippy::large_enum_variant)] // The spec is moved once at build; `Keep` is the rare case.
pub enum TrackSlot {
    New(TrackSpec),
    Keep(usize),
}

pub struct MixSpec {
    pub tracks: Vec<TrackSlot>,
    pub buses: Vec<BusSpec>,
    pub master_inserts: Vec<Box<dyn Processor>>,
    pub master_gain: f32,
    /// The sidechain keys the master's inserts take, in insert order (ENG-078).
    pub master_keys: Vec<KeySpec>,
}

impl Default for MixSpec {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            buses: Vec::new(),
            master_inserts: Vec::new(),
            master_gain: 1.0,
            master_keys: Vec::new(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PlanError {
    UnknownBus {
        track: usize,
        bus: usize,
    },
    /// `Keep` needs the previous plan's shape and the same setup. Buses may only be appended,
    /// because a kept track's sends still point at the old bus indices.
    CannotKeep {
        track: usize,
    },
    /// A bus feeds a bus that does not come after it.
    BusOrder {
        bus: usize,
        to: usize,
    },
    /// A key sits on an insert that is not in the node's inserts (ENG-078).
    /// Nodes number tracks first, then buses; `usize::MAX` is the master.
    KeyInsert {
        node: usize,
        insert: usize,
    },
    /// A key's source is not a node in the plan (ENG-078); `from` is the
    /// source it asked for, numbered as `node`.
    UnknownKey {
        node: usize,
        from: usize,
    },
    /// The keys and the audio paths make a loop; no order renders them all
    /// first (ENG-078). `node` is a node in the loop.
    KeyLoop {
        node: usize,
    },
    MaxBlockZero,
    /// The command queue to the audio thread is full.
    QueueFull,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownBus { track, bus } => write!(f, "track {track} refers to bus {bus}"),
            Self::CannotKeep { track } => write!(f, "track {track} cannot be kept"),
            Self::BusOrder { bus, to } => {
                write!(f, "bus {bus} feeds bus {to}, which does not come after it")
            }
            Self::KeyInsert { node, insert } => {
                write!(
                    f,
                    "node {node} has a key on insert {insert} it does not have"
                )
            }
            Self::UnknownKey { node, from } => {
                write!(
                    f,
                    "node {node} listens to key source {from}, which is not a node"
                )
            }
            Self::KeyLoop { node } => write!(f, "node {node} is in a key routing loop"),
            Self::MaxBlockZero => write!(f, "max_block is 0"),
            Self::QueueFull => write!(f, "the command queue is full"),
        }
    }
}

impl std::error::Error for PlanError {}

/// What the controller remembers about the plan running on the audio thread.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanShape {
    pub tracks: usize,
    pub buses: usize,
    pub setup: ProcessSetup,
    /// Each track's latency and connections, for compensating kept tracks in the next plan.
    pub routes: Vec<Route>,
    pub automation: crate::automation_time::Timing,
    /// The inserts inside splitters, for their automation (ENG-074).
    pub nested: crate::split::Nested,
}

/// A node's own latency (source and inserts) and where its signal goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub latency: u32,
    pub output: Output,
    /// The bus of each send, in order.
    pub sends: Vec<usize>,
}

/// The compensation delays of one node's connections.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Compensation {
    pub output: u32,
    pub sends: Vec<u32>,
}

/// Delays for every connection so that all the paths meeting at a bus or the master arrive
/// equally late (ENG-010). Buses only feed buses after them. Returns the delays of the tracks and
/// buses and the latency of the whole mix, master inserts included.
pub fn compensate(
    tracks: &[Route],
    buses: &[Route],
    master_latency: u32,
) -> (Vec<Compensation>, Vec<Compensation>, u32) {
    let (arrive, master) = crate::automation_time::arrivals(tracks, buses);
    let at = |to: Output| match to {
        Output::Master => master,
        Output::Bus(b) => arrive[b],
    };
    let delays = |r: &Route, late: u32| Compensation {
        output: at(r.output) - late,
        sends: r.sends.iter().map(|&b| arrive[b] - late).collect(),
    };
    let track_delays = tracks.iter().map(|t| delays(t, t.latency)).collect();
    let bus_delays = buses
        .iter()
        .enumerate()
        .map(|(i, b)| delays(b, arrive[i] + b.latency))
        .collect();
    (track_delays, bus_delays, master + master_latency)
}

/// A fixed stereo delay on one connection, allocated when the plan is built.
struct Delay {
    l: Vec<f32>,
    r: Vec<f32>,
    frames: usize,
    pos: usize,
}

impl Delay {
    fn new(frames: u32, max_block: usize) -> Delay {
        let len = if frames == 0 {
            0
        } else {
            (frames as usize + max_block).next_power_of_two()
        };
        Delay {
            l: vec![0.0; len],
            r: vec![0.0; len],
            frames: frames as usize,
            pos: 0,
        }
    }

    /// Adds `src` delayed by this line's frames, times `level`, to `dest`.
    fn add(&mut self, dest: &mut Stereo, src: &Stereo, n: usize, level: &Smoothed) {
        if self.frames == 0 {
            dest.add_from(src, n, level);
            return;
        }
        let mask = self.l.len() - 1;
        for i in 0..n {
            let w = (self.pos + i) & mask;
            self.l[w] = src.l[i];
            self.r[w] = src.r[i];
            let r = w.wrapping_sub(self.frames) & mask;
            let g = level.at(i, n);
            dest.l[i] += self.l[r] * g;
            dest.r[i] += self.r[r] * g;
        }
        self.pos = (self.pos + n) & mask;
    }

    /// Delays the signal in place by this line's frames (a main signal waiting
    /// for its key, or a keyed insert's own). No-op for a zero delay.
    fn apply(&mut self, l: &mut [f32], r: &mut [f32], n: usize) {
        if self.frames == 0 {
            return;
        }
        let mask = self.l.len() - 1;
        for i in 0..n {
            let w = (self.pos + i) & mask;
            let read = w.wrapping_sub(self.frames) & mask;
            let (sl, sr) = (l[i], r[i]);
            l[i] = self.l[read];
            r[i] = self.r[read];
            self.l[w] = sl;
            self.r[w] = sr;
        }
        self.pos = (self.pos + n) & mask;
    }
}

struct NodeDelays {
    output: Delay,
    sends: Vec<Delay>,
}

impl NodeDelays {
    fn new(c: &Compensation, max_block: usize) -> NodeDelays {
        NodeDelays {
            output: Delay::new(c.output, max_block),
            sends: c.sends.iter().map(|&d| Delay::new(d, max_block)).collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeterReading {
    pub peak: [f32; 2],
    pub rms: [f32; 2],
}

/// A value moved linearly to its target over one block, so fader moves do not click.
#[derive(Clone, Copy, Debug)]
struct Smoothed {
    current: f32,
    target: f32,
}

impl Smoothed {
    fn new(v: f32) -> Self {
        Self {
            current: v,
            target: v,
        }
    }

    fn at(&self, i: usize, n: usize) -> f32 {
        if self.current == self.target {
            self.current
        } else {
            self.current + (self.target - self.current) * (i + 1) as f32 / n as f32
        }
    }

    fn settle(&mut self) {
        self.current = self.target;
    }
}

/// Constant-power balance with unity at the centre: the far side fades out along a cosine.
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let p = pan.clamp(-1.0, 1.0);
    if p <= 0.0 {
        (1.0, (-p * FRAC_PI_2).cos())
    } else {
        ((p * FRAC_PI_2).cos(), 1.0)
    }
}

struct Stereo {
    l: Vec<f32>,
    r: Vec<f32>,
}

impl Stereo {
    fn new(frames: usize) -> Self {
        Self {
            l: vec![0.0; frames],
            r: vec![0.0; frames],
        }
    }

    fn io(&mut self, n: usize) -> AudioIo<'_> {
        AudioIo {
            left: &mut self.l[..n],
            right: &mut self.r[..n],
            key: None,
        }
    }

    fn clear(&mut self, n: usize) {
        self.l[..n].fill(0.0);
        self.r[..n].fill(0.0);
    }

    fn add_from(&mut self, src: &Stereo, n: usize, level: &Smoothed) {
        for i in 0..n {
            let g = level.at(i, n);
            self.l[i] += src.l[i] * g;
            self.r[i] += src.r[i] * g;
        }
    }
}

#[derive(Default)]
struct MeterAcc {
    peak: [f32; 2],
    sumsq: [f64; 2],
    frames: usize,
}

impl MeterAcc {
    fn add(&mut self, buf: &Stereo, n: usize) {
        for (ch, data) in [&buf.l[..n], &buf.r[..n]].into_iter().enumerate() {
            for &s in data {
                self.peak[ch] = self.peak[ch].max(s.abs());
                self.sumsq[ch] += (s as f64) * (s as f64);
            }
        }
        self.frames += n;
    }

    fn take(&mut self) -> MeterReading {
        let n = self.frames.max(1) as f64;
        let reading = MeterReading {
            peak: self.peak,
            rms: [
                (self.sumsq[0] / n).sqrt() as f32,
                (self.sumsq[1] / n).sqrt() as f32,
            ],
        };
        *self = Self::default();
        reading
    }
}

struct Strip {
    gain: Smoothed,
    pan: Smoothed,
    mute: Smoothed,
    meter: MeterAcc,
}

impl Strip {
    fn new(gain: f32, pan: f32, mute: bool) -> Self {
        Self {
            gain: Smoothed::new(gain),
            pan: Smoothed::new(pan),
            mute: Smoothed::new(if mute { 0.0 } else { 1.0 }),
            meter: MeterAcc::default(),
        }
    }

    /// Applies mute, gain and pan in place.
    fn apply(&self, buf: &mut Stereo, n: usize) {
        for i in 0..n {
            let g = self.gain.at(i, n) * self.mute.at(i, n);
            let (pl, pr) = pan_gains(self.pan.at(i, n));
            buf.l[i] *= g * pl;
            buf.r[i] *= g * pr;
        }
    }

    fn settle(&mut self) {
        self.gain.settle();
        self.pan.settle();
        self.mute.settle();
    }
}

struct SendNode {
    bus: usize,
    level: Smoothed,
    pre_fader: bool,
}

pub struct TrackNode {
    unavailable: bool,
    live: bool,
    name: String,
    /// What makes the track's signal: home with the node, or away with a prefetch worker.
    chain: Option<Box<Chain>>,
    prefetch: Option<Box<Prefetch<Chain>>>,
    strip: Strip,
    /// The values of the chain's splitter branches, also while a prefetch worker has the chain.
    branches: crate::split::Cells,
    sends: Vec<SendNode>,
    output: Output,
    /// The chain's block, which the fader, pan and sends go on from.
    buf: Stereo,
}

/// A keyed insert's input (ENG-078): the summed key for the block, swapped in
/// from the plan's accumulators before the node renders, and the delay that
/// lines the main signal up with it.
struct KeyIn {
    /// The insert in the node's `inserts` this key belongs to.
    insert: usize,
    /// The summed key, ready for the insert when the node renders.
    buf: Stereo,
    /// The delay on the main signal that puts it at the same time as the key.
    main_delay: Delay,
    /// `false`: the insert takes its own signal as its key.
    on: bool,
    /// The slot in the plan's `key_acc` this receiver sums into; `usize::MAX`
    /// for a kept node whose key wiring is gone.
    acc: usize,
}

impl KeyIn {
    /// Delays the main signal in place and returns the key for this block
    /// (`None` when the insert takes its own signal).
    fn feed<'a>(
        &'a mut self,
        left: &mut [f32],
        right: &mut [f32],
        n: usize,
    ) -> Option<(&'a [f32], &'a [f32])> {
        self.main_delay.apply(&mut left[..n], &mut right[..n], n);
        (self.on && self.acc != usize::MAX).then_some((&self.buf.l[..n], &self.buf.r[..n]))
    }
}

/// One outgoing key tap on a node (ENG-078): the receiver's accumulator, the
/// tap point, the level and the compensation delay.
struct KeySend {
    /// The slot in the plan's `key_acc` this tap adds into.
    acc: usize,
    /// `true`: taken after the inserts and before the fader.
    pre: bool,
    delay: Delay,
    level: Smoothed,
}

/// The nodes rendered at the same height (ENG-078): every key source and
/// audio feeder of a level's nodes is in an earlier level, so a level's
/// tracks can render in any order (the pool parallelizes them) and its buses
/// after them.
struct Level {
    tracks: Vec<usize>,
    buses: Vec<usize>,
}

/// The part of a track that keeps state between blocks: the instrument, the inserts, and what
/// the timeline has started. It is what a prefetch worker renders ahead (ENG-015).
struct Chain {
    ahead: Option<Box<crate::anticipation::Ahead>>,
    source: Box<dyn Processor>,
    inserts: Vec<Box<dyn Processor>>,
    events: Vec<NoteEvent>,
    dropped_events: u64,
    timeline: Timeline,
    /// Pitches the timeline has started and not yet released.
    held: [bool; 128],
    /// Controllers the timeline has moved and not yet put back to rest.
    moved: [bool; CONTROL_NUMBERS],
    /// The next block that plays the timeline first brings every controller to its value at that
    /// position (the transport jumped, or the timeline changed).
    chase: bool,
    midi: Option<Box<MidiRoute>>,
    /// The keyed inserts' summed keys and main delays, in insert order (ENG-078).
    keys: Vec<KeyIn>,
}

impl Chained for Chain {
    fn render_ahead(&mut self, pos: u64, left: &mut [f32], right: &mut [f32]) {
        Plan::render_chain(self, false, left, right, Some(pos));
    }
}

impl TrackNode {
    pub fn name(&self) -> &str {
        &self.name
    }
}

struct BusNode {
    unavailable: bool,
    name: String,
    inserts: Vec<Box<dyn Processor>>,
    strip: Strip,
    branches: crate::split::Cells,
    sends: Vec<SendNode>,
    output: Output,
    buf: Stereo,
    /// The keyed inserts' summed keys and main delays, in insert order (ENG-078).
    keys: Vec<KeyIn>,
}

struct MasterNode {
    branches: crate::split::Cells,
    inserts: Vec<Box<dyn Processor>>,
    gain: Smoothed,
    buf: Stereo,
    meter: MeterAcc,
    /// The keyed inserts' summed keys and main delays, in insert order (ENG-078).
    keys: Vec<KeyIn>,
}

pub struct Plan {
    pool: Option<crate::parallel::TrackPool>,
    setup: ProcessSetup,
    tracks: Vec<Option<Box<TrackNode>>>,
    /// For each track slot, the index in the previous plan to take the node from on swap.
    keep_from: Vec<Option<usize>>,
    buses: Vec<BusNode>,
    master: MasterNode,
    /// Scratch for pre-fader sends: holds the post-insert signal while the fader is applied.
    pre: Stereo,
    /// Render heights (ENG-078): every node's key sources and audio feeders
    /// rendered in an earlier level, so a level's tracks are independent.
    levels: Vec<Level>,
    /// The summed key buffers, one per keyed insert in the plan. A source adds
    /// its delayed signal during its own routing; the receiver swaps the slot
    /// for its `KeyIn` buffer before it renders.
    key_acc: Vec<Stereo>,
    /// The outgoing key taps of each node, tracks first then buses.
    key_sends: Vec<Vec<KeySend>>,
    /// plug-in id → (receiving node, its key) for `set_key_on`.
    key_on: HashMap<u64, (NodeId, usize)>,
    /// send id → (the source's node index in `key_sends`, its tap) for `set_key_level`.
    key_send: HashMap<u64, (usize, usize)>,
    routes: Vec<Route>,
    automation: crate::automation_time::Timing,
    track_delays: Vec<NodeDelays>,
    bus_delays: Vec<NodeDelays>,
    compensation: (Vec<Compensation>, Vec<Compensation>),
    latency: u32,
    nested: crate::split::Nested,
    /// The events pushed, for the tests (UI-312).
    #[cfg(test)]
    pub(crate) pushed: Vec<(usize, NoteEvent)>,
}

fn latency_of(list: &[Box<dyn Processor>]) -> u32 {
    list.iter().map(|p| p.latency()).sum()
}

fn sends_of(sends: &[SendNode]) -> Vec<usize> {
    sends.iter().map(|s| s.bus).collect()
}

fn prepare_all(list: &mut [Box<dyn Processor>], setup: &ProcessSetup) {
    for p in list {
        p.prepare(setup);
    }
}

/// A key's insert index must be in the node's inserts, and every source a
/// node in the plan (ENG-078). `node` is the receiver numbered the
/// `PlanError` way: tracks first, then buses, `usize::MAX` for the master.
fn check_keys(
    node: usize,
    keys: &[KeySpec],
    inserts: usize,
    tracks: usize,
    buses: usize,
) -> Result<(), PlanError> {
    let mut last = None;
    for k in keys {
        if k.insert >= inserts || last.is_some_and(|l| k.insert <= l) {
            return Err(PlanError::KeyInsert {
                node,
                insert: k.insert,
            });
        }
        last = Some(k.insert);
        for s in &k.sources {
            let ok = match s.from {
                KeyFrom::Track(f) => f < tracks,
                KeyFrom::Bus(f) => f < buses,
            };
            if !ok {
                let from = match s.from {
                    KeyFrom::Track(f) => f,
                    KeyFrom::Bus(f) => tracks + f,
                };
                return Err(PlanError::UnknownKey { node, from });
            }
        }
    }
    Ok(())
}

/// One node's keyed inserts (ENG-078). For each key, the main signal takes a
/// delay so it meets the key at the same time as the slowest source; the
/// faster sources take the matching delay, and the taps are pushed onto the
/// sources' `sends` rows.
///
/// `input`: the latency the node's signal arrives with (a track's source
/// latency, a bus's input arrival). `lats`: each insert's latency. `eff`:
/// the finished latency of every node that can feed a key — the level
/// ordering puts a key's sources in an earlier level. Returns the node's
/// `KeyIn`s, the delay to apply before each insert, and the node's finished
/// latency.
#[allow(clippy::too_many_arguments)]
fn keys_for(
    node_id: NodeId,
    keys: &[KeySpec],
    input: u32,
    lats: &[u32],
    eff: &[u32],
    node_of: impl Fn(KeyFrom) -> usize,
    n: usize,
    accs: &mut Vec<Stereo>,
    sends: &mut [Vec<KeySend>],
    on: &mut HashMap<u64, (NodeId, usize)>,
    send_ids: &mut HashMap<u64, (usize, usize)>,
) -> (Vec<KeyIn>, Vec<u32>, u32) {
    let mut out = Vec::with_capacity(keys.len());
    let mut d = vec![0u32; lats.len()];
    let mut at = input;
    let mut keyed = keys.iter().peekable();
    for (i, &lat) in lats.iter().enumerate() {
        if keyed.peek().is_some_and(|k| k.insert == i) {
            let k = keyed.next().expect("peeked");
            // The main signal waits for the slowest key; the faster keys wait.
            let target = k
                .sources
                .iter()
                .map(|s| eff[node_of(s.from)])
                .fold(at, u32::max);
            d[i] = target - at;
            let acc = accs.len();
            accs.push(Stereo::new(n));
            on.insert(k.plugin, (node_id, out.len()));
            for s in &k.sources {
                let from = node_of(s.from);
                send_ids.insert(s.id, (from, sends[from].len()));
                sends[from].push(KeySend {
                    acc,
                    pre: s.pre,
                    delay: Delay::new(target - eff[from], n),
                    level: Smoothed::new(s.level),
                });
            }
            out.push(KeyIn {
                insert: i,
                buf: Stereo::new(n),
                main_delay: Delay::new(d[i], n),
                on: k.on,
                acc,
            });
            at = target;
        }
        at += lat;
    }
    (out, d, at)
}

impl Plan {
    /// Builds and prepares a plan. `previous` is the shape of the plan it will replace and is
    /// required when any track is [`TrackSlot::Keep`].
    pub fn build(
        spec: MixSpec,
        setup: ProcessSetup,
        previous: Option<PlanShape>,
    ) -> Result<Plan, PlanError> {
        if setup.max_block == 0 {
            return Err(PlanError::MaxBlockZero);
        }
        let n = setup.max_block;
        let bus_count = spec.buses.len();
        let track_count = spec.tracks.len();
        let mut tracks = Vec::with_capacity(track_count);
        let mut keep_from = Vec::with_capacity(track_count);
        let mut routes = Vec::with_capacity(track_count);
        let mut track_timing = Vec::with_capacity(track_count);
        let mut track_keys = Vec::with_capacity(track_count);
        for (i, slot) in spec.tracks.into_iter().enumerate() {
            match slot {
                TrackSlot::Keep(old) => {
                    let route = previous.as_ref().and_then(|p| {
                        (old < p.tracks && bus_count >= p.buses && p.setup == setup)
                            .then(|| p.routes.get(old).cloned())
                            .flatten()
                    });
                    let Some(route) = route else {
                        return Err(PlanError::CannotKeep { track: i });
                    };
                    routes.push(route);
                    track_timing.push(
                        previous
                            .as_ref()
                            .and_then(|p| p.automation.tracks.get(old))
                            .cloned()
                            .ok_or(PlanError::CannotKeep { track: i })?,
                    );
                    tracks.push(None);
                    keep_from.push(Some(old));
                    track_keys.push(Vec::new());
                }
                TrackSlot::New(mut t) => {
                    let bad_bus = t
                        .sends
                        .iter()
                        .map(|s| s.bus)
                        .chain(match t.output {
                            Output::Bus(b) => Some(b),
                            Output::Master => None,
                        })
                        .find(|&b| b >= bus_count);
                    if let Some(bus) = bad_bus {
                        return Err(PlanError::UnknownBus { track: i, bus });
                    }
                    check_keys(i, &t.keys, t.inserts.len(), track_count, bus_count)?;
                    let keyed = !t.keys.is_empty();
                    track_keys.push(std::mem::take(&mut t.keys));
                    t.source.prepare(&setup);
                    t.timeline
                        .set_offline(setup.mode == crate::processor::RenderMode::Offline);
                    prepare_all(&mut t.inserts, &setup);
                    track_timing.push(crate::automation_time::Chain {
                        input: t.source.latency(),
                        inserts: t.inserts.iter().map(|p| p.latency()).collect(),
                    });
                    routes.push(Route {
                        latency: t.source.latency() + latency_of(&t.inserts),
                        output: t.output,
                        sends: t.sends.iter().map(|s| s.bus).collect(),
                    });
                    let pure = t.source.timeline_only() && t.inserts.is_empty();
                    // Only a track with state to keep ahead is lent out; a pure timeline has its
                    // own cache, and a MIDI track makes no audio.
                    let prefetch = (t.prefetch
                        && !keyed
                        && !pure
                        && t.midi.is_none()
                        && setup.mode == crate::processor::RenderMode::Realtime)
                        .then(|| Prefetch::new(n))
                        .flatten()
                        .map(Box::new);
                    tracks.push(Some(Box::new(TrackNode {
                        unavailable: t.unavailable,
                        live: t.live,
                        name: t.name,
                        prefetch,
                        strip: Strip::new(t.gain, t.pan, t.mute),
                        branches: crate::split::Cells::of(&t.inserts),
                        sends: t
                            .sends
                            .iter()
                            .map(|s| SendNode {
                                bus: s.bus,
                                level: Smoothed::new(s.level),
                                pre_fader: s.pre_fader,
                            })
                            .collect(),
                        output: t.output,
                        buf: Stereo::new(n),
                        chain: Some(Box::new(Chain {
                            ahead: pure.then(|| Box::new(crate::anticipation::Ahead::new(n))),
                            source: t.source,
                            inserts: t.inserts,
                            events: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
                            dropped_events: 0,
                            timeline: t.timeline,
                            held: [false; 128],
                            moved: [false; CONTROL_NUMBERS],
                            chase: false,
                            midi: t.midi.map(|target| {
                                Box::new(MidiRoute {
                                    target: *target,
                                    port: None,
                                    out: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
                                    sounding: [false; 128],
                                })
                            }),
                            keys: Vec::new(),
                        })),
                    })));
                    keep_from.push(None);
                }
            }
        }
        let mut buses = Vec::with_capacity(bus_count);
        let mut bus_keys = Vec::with_capacity(bus_count);
        let mut bus_lats = Vec::with_capacity(bus_count);
        for (i, mut b) in spec.buses.into_iter().enumerate() {
            let feeds = b.sends.iter().map(|s| s.bus).chain(match b.output {
                Output::Bus(to) => Some(to),
                Output::Master => None,
            });
            for to in feeds {
                if to >= bus_count {
                    return Err(PlanError::UnknownBus { track: i, bus: to });
                }
                if to <= i {
                    return Err(PlanError::BusOrder { bus: i, to });
                }
            }
            check_keys(
                track_count + i,
                &b.keys,
                b.inserts.len(),
                track_count,
                bus_count,
            )?;
            prepare_all(&mut b.inserts, &setup);
            bus_lats.push(b.inserts.iter().map(|p| p.latency()).collect::<Vec<u32>>());
            bus_keys.push(std::mem::take(&mut b.keys));
            buses.push(BusNode {
                unavailable: b.unavailable,
                name: b.name,
                branches: crate::split::Cells::of(&b.inserts),
                inserts: b.inserts,
                strip: Strip::new(b.gain, b.pan, b.mute),
                sends: b
                    .sends
                    .iter()
                    .map(|s| SendNode {
                        bus: s.bus,
                        level: Smoothed::new(s.level),
                        pre_fader: s.pre_fader,
                    })
                    .collect(),
                output: b.output,
                buf: Stereo::new(n),
                keys: Vec::new(),
            });
        }
        let mut master_inserts = spec.master_inserts;
        prepare_all(&mut master_inserts, &setup);
        let master_keys = spec.master_keys;
        check_keys(
            usize::MAX,
            &master_keys,
            master_inserts.len(),
            track_count,
            bus_count,
        )?;
        let mut bus_routes: Vec<Route> = buses
            .iter()
            .map(|b| Route {
                latency: latency_of(&b.inserts),
                output: b.output,
                sends: sends_of(&b.sends),
            })
            .collect();

        // Key taps are dependencies: a receiver renders after the nodes it
        // listens to (ENG-078). Order the nodes into levels so a level's
        // sources are always in an earlier level; within a level the tracks
        // are independent and the buses come after them.
        let node_count = track_count + bus_count;
        let node_of = |f: KeyFrom| match f {
            KeyFrom::Track(i) => i,
            KeyFrom::Bus(j) => track_count + j,
        };
        let mut deps: Vec<BTreeSet<usize>> = std::iter::repeat_with(BTreeSet::new)
            .take(node_count)
            .collect();
        let mut feeds: Vec<BTreeSet<usize>> = std::iter::repeat_with(BTreeSet::new)
            .take(bus_count)
            .collect();
        for (i, r) in routes.iter().enumerate() {
            if let Output::Bus(b) = r.output {
                deps[track_count + b].insert(i);
                feeds[b].insert(i);
            }
            for &b in &r.sends {
                deps[track_count + b].insert(i);
                feeds[b].insert(i);
            }
        }
        for (j, b) in buses.iter().enumerate() {
            if let Output::Bus(to) = b.output {
                deps[track_count + to].insert(track_count + j);
                feeds[to].insert(track_count + j);
            }
            for s in &b.sends {
                deps[track_count + s.bus].insert(track_count + j);
                feeds[s.bus].insert(track_count + j);
            }
        }
        for (i, keys) in track_keys.iter().enumerate() {
            for s in keys.iter().flat_map(|k| &k.sources) {
                deps[i].insert(node_of(s.from));
            }
        }
        for (j, keys) in bus_keys.iter().enumerate() {
            for s in keys.iter().flat_map(|k| &k.sources) {
                deps[track_count + j].insert(node_of(s.from));
            }
        }
        // The height of each node: 0 for none, else 1 + the highest dependency.
        let mut height = vec![0usize; node_count];
        let mut waiting: Vec<usize> = deps.iter().map(BTreeSet::len).collect();
        let mut ready: Vec<usize> = (0..node_count).filter(|&i| waiting[i] == 0).collect();
        let mut placed = 0usize;
        while !ready.is_empty() {
            let mut next = Vec::new();
            for &i in &ready {
                placed += 1;
                for (m, d) in deps.iter().enumerate() {
                    if d.contains(&i) {
                        waiting[m] -= 1;
                        height[m] = height[m].max(height[i] + 1);
                        if waiting[m] == 0 {
                            next.push(m);
                        }
                    }
                }
            }
            ready = next;
        }
        if placed < node_count {
            let node = waiting.iter().position(|&w| w > 0).unwrap_or(0);
            return Err(PlanError::KeyLoop { node });
        }
        // With no keys the mix renders as before: every track, then the buses
        // in index order, so a key-less song keeps its sums bit for bit.
        let any_keys =
            track_keys.iter().chain(&bus_keys).any(|k| !k.is_empty()) || !master_keys.is_empty();
        let levels: Vec<Level> = if !any_keys {
            vec![Level {
                tracks: (0..track_count).collect(),
                buses: (0..bus_count).collect(),
            }]
        } else {
            (0..=height.iter().copied().max().unwrap_or(0))
                .map(|l| Level {
                    tracks: (0..track_count).filter(|&i| height[i] == l).collect(),
                    buses: (0..bus_count)
                        .filter(|&j| height[track_count + j] == l)
                        .collect(),
                })
                .collect()
        };

        // The finished latency of each node, in level order: a key's `eff`
        // lookup only ever sees nodes that render before it. Key taps and the
        // keyed inserts' main delays come out of this pass, and the main
        // delays are folded into the automation timings.
        let mut eff = vec![0u32; node_count];
        let mut arrive = vec![0u32; bus_count];
        let mut key_acc: Vec<Stereo> = Vec::new();
        let mut key_sends: Vec<Vec<KeySend>> =
            std::iter::repeat_with(Vec::new).take(node_count).collect();
        let mut key_on = HashMap::new();
        let mut key_send = HashMap::new();
        let mut bus_timing: Vec<Option<crate::automation_time::Chain>> = vec![None; bus_count];
        for level in &levels {
            for &i in &level.tracks {
                if keep_from[i].is_some() {
                    eff[i] = routes[i].latency;
                    continue;
                }
                let lats = track_timing[i].inserts.clone();
                let (keys, d, e) = keys_for(
                    NodeId::Track(i),
                    &track_keys[i],
                    track_timing[i].input,
                    &lats,
                    &eff,
                    node_of,
                    n,
                    &mut key_acc,
                    &mut key_sends,
                    &mut key_on,
                    &mut key_send,
                );
                if let Some(&d0) = d.first() {
                    track_timing[i].input += d0;
                }
                for (k, &dj) in d.iter().skip(1).enumerate() {
                    track_timing[i].inserts[k] += dj;
                }
                if let Some(c) = tracks[i].as_mut().and_then(|t| t.chain.as_mut()) {
                    c.keys = keys;
                }
                eff[i] = e;
                routes[i].latency = e;
            }
            for &j in &level.buses {
                arrive[j] = feeds[j].iter().map(|&f| eff[f]).max().unwrap_or(0);
                let (keys, d, e) = keys_for(
                    NodeId::Bus(j),
                    &bus_keys[j],
                    arrive[j],
                    &bus_lats[j],
                    &eff,
                    node_of,
                    n,
                    &mut key_acc,
                    &mut key_sends,
                    &mut key_on,
                    &mut key_send,
                );
                buses[j].keys = keys;
                eff[track_count + j] = e;
                bus_routes[j].latency = e - arrive[j];
                bus_timing[j] = Some(crate::automation_time::Chain {
                    input: arrive[j] + d.first().copied().unwrap_or(0),
                    inserts: bus_lats[j]
                        .iter()
                        .enumerate()
                        .map(|(k, &l)| l + d.get(k + 1).copied().unwrap_or(0))
                        .collect(),
                });
            }
        }
        let master_lats: Vec<u32> = master_inserts.iter().map(|p| p.latency()).collect();
        let (_, master_arrive) = crate::automation_time::arrivals(&routes, &bus_routes);
        let (master_key_ins, _d, master_eff) = keys_for(
            NodeId::Master,
            &master_keys,
            master_arrive,
            &master_lats,
            &eff,
            node_of,
            n,
            &mut key_acc,
            &mut key_sends,
            &mut key_on,
            &mut key_send,
        );
        let (track_comp, bus_comp, latency) =
            compensate(&routes, &bus_routes, master_eff - master_arrive);
        let automation = crate::automation_time::Timing {
            tracks: track_timing,
            buses: bus_timing
                .into_iter()
                .map(Option::unwrap_or_default)
                .collect(),
        };
        let mut plan = Plan {
            pool: None,
            levels,
            key_acc,
            key_sends,
            key_on,
            key_send,
            track_delays: track_comp.iter().map(|c| NodeDelays::new(c, n)).collect(),
            bus_delays: bus_comp.iter().map(|c| NodeDelays::new(c, n)).collect(),
            compensation: (track_comp, bus_comp),
            latency,
            automation,
            nested: crate::split::Nested::default(),
            routes,
            setup,
            tracks,
            keep_from,
            buses,
            master: MasterNode {
                branches: crate::split::Cells::of(&master_inserts),
                inserts: master_inserts,
                gain: Smoothed::new(spec.master_gain),
                buf: Stereo::new(n),
                meter: MeterAcc::default(),
                keys: master_key_ins,
            },
            pre: Stereo::new(n),
            #[cfg(test)]
            pushed: Vec::new(),
        };
        // A kept track's inner inserts are the ones its node had in the previous plan.
        plan.nested = crate::split::Nested {
            tracks: plan
                .tracks
                .iter()
                .zip(&plan.keep_from)
                .map(|(t, from)| match from {
                    Some(old) => previous
                        .as_ref()
                        .and_then(|p| p.nested.tracks.get(*old))
                        .cloned()
                        .unwrap_or_default(),
                    None => t
                        .as_ref()
                        .and_then(|t| t.chain.as_ref())
                        .map_or_else(Vec::new, |c| crate::split::Nested::of(&c.inserts)),
                })
                .collect(),
            buses: plan
                .buses
                .iter()
                .map(|b| crate::split::Nested::of(&b.inserts))
                .collect(),
        };
        if plan.tracks.len() >= 8 {
            let workers = std::thread::available_parallelism()
                .map_or(1, |n| n.get())
                .min(8);
            // Resource exhaustion leaves a valid serial plan.
            let _ = plan.set_workers(workers);
        }
        Ok(plan)
    }

    pub fn setup(&self) -> ProcessSetup {
        self.setup
    }

    /// Builds persistent track workers on the control thread. Routing always joins their
    /// outputs in track order, so parallel rendering has the same summation as one worker.
    pub fn set_workers(&mut self, count: usize) -> std::io::Result<()> {
        self.pool = if count <= 1 || self.tracks.len() <= 1 {
            None
        } else {
            Some(crate::parallel::TrackPool::new(
                count.min(self.tracks.len()),
            )?)
        };
        Ok(())
    }

    pub fn workers(&self) -> usize {
        self.pool
            .as_ref()
            .map_or(1, crate::parallel::TrackPool::count)
    }

    pub fn shape(&self) -> PlanShape {
        PlanShape {
            tracks: self.tracks.len(),
            buses: self.buses.len(),
            setup: self.setup,
            routes: self.routes.clone(),
            automation: self.automation.clone(),
            nested: self.nested.clone(),
        }
    }

    /// The compensation delays of each track's and each bus's connections (ENG-010).
    pub fn compensation(&self) -> (&[Compensation], &[Compensation]) {
        (&self.compensation.0, &self.compensation.1)
    }

    /// A seek starts at the controls queued for its new position, without a ramp from the old one.
    pub(crate) fn settle_controls(&mut self) {
        for t in self.tracks.iter_mut().flatten() {
            t.strip.settle();
            for send in &mut t.sends {
                send.level.settle();
            }
        }
        for b in &mut self.buses {
            b.strip.settle();
            for send in &mut b.sends {
                send.level.settle();
            }
        }
        for sends in &mut self.key_sends {
            for s in sends {
                s.level.settle();
            }
        }
        self.master.gain.settle();
    }

    pub fn track_name(&self, i: usize) -> Option<&str> {
        self.tracks.get(i)?.as_deref().map(TrackNode::name)
    }

    pub fn bus_name(&self, i: usize) -> Option<&str> {
        self.buses.get(i).map(|b| b.name.as_str())
    }

    /// Moves the kept tracks out of `old` into this plan. Runs on the audio thread: it only
    /// moves boxes between slots that already exist.
    pub fn take_kept_from(&mut self, old: &mut Plan) {
        for (slot, from) in self.tracks.iter_mut().zip(self.keep_from.iter()) {
            if let Some(from) = *from {
                *slot = old.tracks.get_mut(from).and_then(Option::take);
                // A carried node's keys are not wired into this plan: their
                // accumulator slots were the old plan's. The main-signal
                // delays keep running; `feed` gives no key.
                if let Some(c) = slot.as_mut().and_then(|t| t.chain.as_mut()) {
                    for k in &mut c.keys {
                        k.acc = usize::MAX;
                    }
                }
            }
        }
    }

    /// How late the mix comes out of the master, in frames: the longest path from a source
    /// through its inserts, buses and the master inserts. Every other path is delayed to match.
    pub fn latency(&self) -> u32 {
        self.latency
    }

    /// Sets a splitter branch's gain (linear), pan and mute (ENG-074), by the branch's node id,
    /// in whichever node's chain the splitter is.
    pub fn set_branch_gain(&mut self, node: u64, gain: f32, pan: f32, mute: bool) {
        let _ = self
            .tracks
            .iter()
            .flatten()
            .map(|t| &t.branches)
            .chain(self.buses.iter().map(|b| &b.branches))
            .chain([&self.master.branches])
            .any(|c| c.set(node, gain, pan, mute));
    }

    pub fn set_track_gain(&mut self, track: usize, gain: f32) {
        if let Some(Some(t)) = self.tracks.get_mut(track) {
            // The fader is after the chain: only the pure timeline cache looks at it.
            if let Some(ahead) = t.chain.as_mut().and_then(|c| c.ahead.as_mut()) {
                ahead.invalidate();
            }
            t.strip.gain.target = gain;
        }
    }

    pub fn set_track_pan(&mut self, track: usize, pan: f32) {
        if let Some(Some(t)) = self.tracks.get_mut(track) {
            t.strip.pan.target = pan.clamp(-1.0, 1.0);
        }
    }

    pub fn set_track_mute(&mut self, track: usize, mute: bool) {
        if let Some(Some(t)) = self.tracks.get_mut(track) {
            t.strip.mute.target = if mute { 0.0 } else { 1.0 };
        }
    }

    pub fn set_send_level(&mut self, track: usize, send: usize, level: f32) {
        if let Some(Some(t)) = self.tracks.get_mut(track)
            && let Some(s) = t.sends.get_mut(send)
        {
            s.level.target = level;
        }
    }

    pub fn set_bus_gain(&mut self, bus: usize, gain: f32) {
        if let Some(b) = self.buses.get_mut(bus) {
            b.strip.gain.target = gain;
        }
    }

    pub fn set_bus_pan(&mut self, bus: usize, pan: f32) {
        if let Some(b) = self.buses.get_mut(bus) {
            b.strip.pan.target = pan.clamp(-1.0, 1.0);
        }
    }

    pub fn set_bus_mute(&mut self, bus: usize, mute: bool) {
        if let Some(b) = self.buses.get_mut(bus) {
            b.strip.mute.target = if mute { 0.0 } else { 1.0 };
        }
    }

    pub fn set_bus_send_level(&mut self, bus: usize, send: usize, level: f32) {
        if let Some(b) = self.buses.get_mut(bus)
            && let Some(s) = b.sends.get_mut(send)
        {
            s.level.target = level;
        }
    }

    /// Turns a keyed insert's external sidechain on or off (ENG-078).
    /// `plugin` is the receiving plug-in's id.
    pub fn set_key_on(&mut self, plugin: u64, on: bool) {
        let Some(&(node, i)) = self.key_on.get(&plugin) else {
            return;
        };
        let keys: Option<&mut Vec<KeyIn>> = match node {
            NodeId::Track(t) => self
                .tracks
                .get_mut(t)
                .and_then(Option::as_mut)
                .and_then(|t| t.chain.as_mut())
                .map(|c| &mut c.keys),
            NodeId::Bus(b) => self.buses.get_mut(b).map(|b| &mut b.keys),
            NodeId::Master => Some(&mut self.master.keys),
        };
        if let Some(k) = keys.and_then(|ks| ks.get_mut(i)) {
            k.on = on;
        }
    }

    /// The level of one key source, by the send row's id (ENG-078).
    pub fn set_key_level(&mut self, send: u64, level: f32) {
        if let Some(&(node, i)) = self.key_send.get(&send)
            && let Some(s) = self.key_sends.get_mut(node).and_then(|v| v.get_mut(i))
        {
            s.level.target = level;
        }
    }

    /// Swaps in a new timeline for a track and returns the old one, to be freed elsewhere. The
    /// notes the old one is holding are released at the start of the next block. Gives the new
    /// timeline back when there is no such track.
    pub fn set_timeline(
        &mut self,
        track: usize,
        mut timeline: Box<Timeline>,
    ) -> Result<Box<Timeline>, Box<Timeline>> {
        let Some(Some(t)) = self.tracks.get_mut(track) else {
            return Err(timeline);
        };
        Self::release_track(t);
        timeline.set_offline(self.setup.mode == crate::processor::RenderMode::Offline);
        if let Some(c) = t.chain.as_mut() {
            std::mem::swap(&mut c.timeline, &mut *timeline);
        }
        Ok(timeline)
    }

    /// Ends every note the timelines have started, at the start of the next block. Used when the
    /// transport stops or jumps. A track that took live input goes back to being prefetched.
    pub fn release_notes(&mut self) {
        for t in self.tracks.iter_mut().flatten() {
            Self::release_track(t);
            if let Some(p) = &mut t.prefetch {
                p.set_hot(false);
            }
        }
    }

    /// Calls a lent chain back and drops what was rendered ahead (an edit, a jump). The chain's
    /// state is then ahead of the transport, which [`Chain::release`] puts right.
    fn discard_prefetch(t: &mut TrackNode) {
        if let Some(p) = &mut t.prefetch
            && let Some(chain) = p.recall()
        {
            t.chain = Some(chain);
        }
    }

    fn release_track(t: &mut TrackNode) {
        Self::discard_prefetch(t);
        if let Some(c) = t.chain.as_mut() {
            if let Some(ahead) = &mut c.ahead {
                ahead.invalidate();
            }
            c.release();
        }
    }

    pub fn set_master_gain(&mut self, gain: f32) {
        self.master.gain.target = gain;
    }

    /// Queues a note for the track's source in the next block. Returns false if dropped.
    pub fn push_event(&mut self, track: usize, event: NoteEvent) -> bool {
        #[cfg(test)]
        self.pushed.push((track, event));
        let Some(Some(t)) = self.tracks.get_mut(track) else {
            return false;
        };
        // A note played by hand is live input: what was rendered ahead cannot have it, and the
        // track renders on the audio thread until the transport stops or jumps.
        if let Some(p) = &mut t.prefetch {
            p.set_hot(true);
            if p.is_lent() {
                Self::release_track(t);
            }
        }
        let Some(c) = t.chain.as_mut() else {
            return false;
        };
        if c.events.len() < c.events.capacity() {
            c.events.push(event);
            true
        } else {
            c.dropped_events += 1;
            false
        }
    }

    /// Puts the notes and controller changes of the block `[pos, pos + n)` into the chain's
    /// events, in time order: at one frame the note-offs, then the controllers, then the
    /// note-ons. After a jump the controllers first take the values they have at `pos`.
    fn queue_timeline(c: &mut Chain, pos: u64, n: u64) {
        let Chain {
            timeline,
            events,
            dropped_events,
            held,
            moved,
            chase,
            ..
        } = c;
        let mut push = |offset: u32, kind: NoteKind| {
            if events.len() < events.capacity() {
                events.push(NoteEvent { offset, kind });
            } else {
                *dropped_events += 1;
            }
        };
        if std::mem::take(chase) {
            timeline.controls_before(pos, |c| {
                if let NoteKind::Control { number, .. } = c.kind {
                    moved[control_index(number)] = true;
                }
                push(0, c.kind);
            });
        }
        let notes = timeline.notes_in(pos, pos + n);
        let controls = timeline.controls_in(pos, pos + n);
        let (mut i, mut j) = (0, 0);
        while i < notes.len() || j < controls.len() {
            // A control goes before the next note unless that note is an off at or before it.
            let control_first = match (notes.get(i), controls.get(j)) {
                (Some(note), Some(c)) => {
                    c.frame < note.frame
                        || (c.frame == note.frame && matches!(note.kind, NoteKind::On { .. }))
                }
                (None, Some(_)) => true,
                _ => false,
            };
            if control_first {
                let c = controls[j];
                j += 1;
                if let NoteKind::Control { number, .. } = c.kind {
                    moved[control_index(number)] = true;
                }
                push((c.frame - pos) as u32, c.kind);
            } else {
                let note = notes[i];
                i += 1;
                match note.kind {
                    NoteKind::On { pitch, .. } => held[(pitch & 0x7f) as usize] = true,
                    NoteKind::Off { pitch } => held[(pitch & 0x7f) as usize] = false,
                    NoteKind::Control { .. } => {}
                }
                push((note.frame - pos) as u32, note.kind);
            }
        }
    }

    /// Renders one block with the transport stopped: only the notes sent by hand play.
    /// `out_l.len() == out_r.len() <= max_block`.
    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        self.process_at(out_l, out_r, None);
    }

    /// Renders one block. With `Some(pos)` the transport runs and the block starts `pos` frames
    /// into the song, so the timelines play; with `None` they stay silent.
    pub fn process_at(&mut self, out_l: &mut [f32], out_r: &mut [f32], pos: Option<u64>) {
        let n = out_l.len().min(out_r.len()).min(self.setup.max_block);
        for b in &mut self.buses {
            b.buf.clear(n);
        }
        for a in &mut self.key_acc {
            a.clear(n);
        }
        self.master.buf.clear(n);

        let track_count = self.tracks.len();
        for level in 0..self.levels.len() {
            // Feed each receiver the key its sources pushed in earlier levels.
            for &i in &self.levels[level].tracks {
                if let Some(c) = self.tracks[i].as_mut().and_then(|t| t.chain.as_mut()) {
                    for k in &mut c.keys {
                        if k.acc != usize::MAX {
                            std::mem::swap(&mut k.buf, &mut self.key_acc[k.acc]);
                        }
                    }
                }
            }
            for &j in &self.levels[level].buses {
                for k in &mut self.buses[j].keys {
                    if k.acc != usize::MAX {
                        std::mem::swap(&mut k.buf, &mut self.key_acc[k.acc]);
                    }
                }
            }

            if let Some(pool) = &mut self.pool {
                pool.render(&mut self.tracks, &self.levels[level].tracks, n, pos);
            } else {
                for &i in &self.levels[level].tracks {
                    if let Some(t) = &mut self.tracks[i] {
                        Self::render_track(t, n, pos);
                    }
                }
            }

            for &i in &self.levels[level].tracks {
                let delays = &mut self.track_delays[i];
                let Some(t) = &mut self.tracks[i] else {
                    continue;
                };
                let has_pre =
                    t.sends.iter().any(|s| s.pre_fader) || self.key_sends[i].iter().any(|s| s.pre);
                if has_pre {
                    // Pre-fader sends and key taps still follow the mute.
                    for k in 0..n {
                        let m = t.strip.mute.at(k, n);
                        self.pre.l[k] = t.buf.l[k] * m;
                        self.pre.r[k] = t.buf.r[k] * m;
                    }
                }
                t.strip.apply(&mut t.buf, n);
                for (s, d) in t.sends.iter_mut().zip(&mut delays.sends) {
                    let src = if s.pre_fader { &self.pre } else { &t.buf };
                    d.add(&mut self.buses[s.bus].buf, src, n, &s.level);
                    s.level.settle();
                }
                let dest = match t.output {
                    Output::Master => &mut self.master.buf,
                    Output::Bus(b) => &mut self.buses[b].buf,
                };
                delays.output.add(dest, &t.buf, n, &Smoothed::new(1.0));
                for s in &mut self.key_sends[i] {
                    let src = if s.pre { &self.pre } else { &t.buf };
                    s.delay.add(&mut self.key_acc[s.acc], src, n, &s.level);
                    s.level.settle();
                }
                t.strip.settle();
                t.strip.meter.add(&t.buf, n);
            }

            for &j in &self.levels[level].buses {
                // Everything a bus feeds comes after it, so the rest of the list can be borrowed.
                let (head, later) = self.buses.split_at_mut(j + 1);
                let b = &mut head[j];
                if b.unavailable {
                    b.buf.clear(n);
                    continue;
                }
                let mut key = 0;
                for (i, p) in b.inserts.iter_mut().enumerate() {
                    let mut io = b.buf.io(n);
                    if key < b.keys.len() && b.keys[key].insert == i {
                        io.key = b.keys[key].feed(&mut io.left[..], &mut io.right[..], n);
                        key += 1;
                    }
                    p.process(&mut io, &[]);
                }
                let has_pre = b.sends.iter().any(|s| s.pre_fader)
                    || self.key_sends[track_count + j].iter().any(|s| s.pre);
                if has_pre {
                    for k in 0..n {
                        let m = b.strip.mute.at(k, n);
                        self.pre.l[k] = b.buf.l[k] * m;
                        self.pre.r[k] = b.buf.r[k] * m;
                    }
                }
                b.strip.apply(&mut b.buf, n);
                let delays = &mut self.bus_delays[j];
                for (s, d) in b.sends.iter_mut().zip(&mut delays.sends) {
                    let src = if s.pre_fader { &self.pre } else { &b.buf };
                    d.add(&mut later[s.bus - j - 1].buf, src, n, &s.level);
                    s.level.settle();
                }
                b.strip.settle();
                b.strip.meter.add(&b.buf, n);
                let dest = match b.output {
                    Output::Master => &mut self.master.buf,
                    Output::Bus(to) => &mut later[to - j - 1].buf,
                };
                delays.output.add(dest, &b.buf, n, &Smoothed::new(1.0));
                for s in &mut self.key_sends[track_count + j] {
                    let src = if s.pre { &self.pre } else { &b.buf };
                    s.delay.add(&mut self.key_acc[s.acc], src, n, &s.level);
                    s.level.settle();
                }
            }
        }

        let m = &mut self.master;
        for k in &mut m.keys {
            if k.acc != usize::MAX {
                std::mem::swap(&mut k.buf, &mut self.key_acc[k.acc]);
            }
        }
        let mut key = 0;
        for (i, p) in m.inserts.iter_mut().enumerate() {
            let mut io = m.buf.io(n);
            if key < m.keys.len() && m.keys[key].insert == i {
                io.key = m.keys[key].feed(&mut io.left[..], &mut io.right[..], n);
                key += 1;
            }
            p.process(&mut io, &[]);
        }
        for i in 0..n {
            let g = m.gain.at(i, n);
            m.buf.l[i] *= g;
            m.buf.r[i] *= g;
        }
        m.gain.settle();
        m.meter.add(&m.buf, n);
        out_l[..n].copy_from_slice(&m.buf.l[..n]);
        out_r[..n].copy_from_slice(&m.buf.r[..n]);
    }

    /// Renders a track's block into `t.buf`. A track with a prefetch takes the block from its
    /// worker's ring while the transport runs and nothing is live; otherwise the audio thread
    /// renders the chain itself and, when the transport runs, lends it out for the next blocks.
    pub(crate) fn render_track(t: &mut TrackNode, n: usize, pos: Option<u64>) {
        if t.unavailable {
            t.buf.clear(n);
            if let Some(chain) = &mut t.chain {
                chain.events.clear();
            }
            return;
        }
        let TrackNode {
            live,
            chain,
            prefetch,
            buf,
            ..
        } = t;
        let (left, right) = (&mut buf.l[..n], &mut buf.r[..n]);
        let Some(p) = prefetch.as_deref_mut() else {
            if let Some(c) = chain.as_mut() {
                Self::render_chain(c, *live, left, right, pos);
            }
            return;
        };
        match pos {
            Some(pos) if !*live && !p.hot() && !p.failed() => {
                if p.is_lent() {
                    if p.take(pos, left, right) == Take::Hit {
                        return;
                    }
                    // The ring does not go on from here: call the chain back, as for a jump.
                    if let Some(mut c) = p.recall() {
                        c.release();
                        *chain = Some(c);
                    }
                }
                let Some(c) = chain.as_mut() else { return };
                Self::render_chain(c, false, left, right, Some(pos));
                // The disk streams of a clip fill between callbacks: they stay on this thread.
                if c.timeline.streams().is_empty()
                    && let Some(c) = chain.take()
                {
                    p.lend(c, pos + n as u64);
                }
            }
            _ => {
                if let Some(mut c) = p.recall() {
                    c.release();
                    *chain = Some(c);
                }
                if let Some(c) = chain.as_mut() {
                    Self::render_chain(c, *live, left, right, pos);
                }
            }
        }
    }

    /// The chain's block: the instrument with the notes of the timeline and of the hand, the
    /// timeline's audio, then the inserts. Writes the whole of `left` and `right`.
    fn render_chain(
        c: &mut Chain,
        live: bool,
        left: &mut [f32],
        right: &mut [f32],
        pos: Option<u64>,
    ) {
        left.fill(0.0);
        right.fill(0.0);
        let n = left.len().min(right.len());
        if let Some(pos) = pos {
            Self::queue_timeline(c, pos, n as u64);
        }
        if let Some(m) = &mut c.midi {
            m.take_events(&c.events);
        }
        c.source.process(
            &mut AudioIo {
                left: &mut left[..n],
                right: &mut right[..n],
                key: None,
            },
            &c.events,
        );
        c.events.clear();
        if let Some(pos) = pos {
            if !live && let Some(ahead) = &mut c.ahead {
                ahead.mix(&c.timeline, pos, &mut left[..n], &mut right[..n]);
            } else {
                c.timeline.mix_audio(pos, &mut left[..n], &mut right[..n]);
            }
        }
        let mut key = 0;
        for (i, p) in c.inserts.iter_mut().enumerate() {
            let io_key = if key < c.keys.len() && c.keys[key].insert == i {
                let k = &mut c.keys[key];
                key += 1;
                k.feed(&mut left[..n], &mut right[..n], n)
            } else {
                None
            };
            p.process(
                &mut AudioIo {
                    left: &mut left[..n],
                    right: &mut right[..n],
                    key: io_key,
                },
                &[],
            );
        }
    }

    /// Points every track's MIDI route at the port of `out` that has its name (`None`: no port).
    /// Runs on the audio thread: it compares names and allocates nothing.
    pub fn bind_midi(&mut self, out: Option<&dyn MidiOutput>) {
        for t in self.tracks.iter_mut().flatten() {
            if let Some(m) = t.chain.as_mut().and_then(|c| c.midi.as_mut()) {
                m.port = out.and_then(|o| o.port_index(&m.target.port));
            }
        }
    }

    /// Sends the MIDI messages of the block just rendered to `out`, `base` frames into the device
    /// block, and clears them. Without an output they are dropped.
    pub fn flush_midi(&mut self, mut out: Option<&mut (dyn MidiOutput + '_)>, base: usize) {
        for t in self.tracks.iter_mut().flatten() {
            if let Some(m) = t.chain.as_mut().and_then(|c| c.midi.as_mut()) {
                if let (Some(out), Some(port)) = (out.as_deref_mut(), m.port) {
                    for &(offset, msg) in &m.out {
                        out.send(port, base + offset as usize, msg);
                    }
                }
                m.out.clear();
            }
        }
    }

    /// For a plan that is being replaced: lets go the notes its tracks that did not move into
    /// the new plan still hold on a MIDI port.
    pub fn release_midi(&mut self, out: Option<&mut (dyn MidiOutput + '_)>) {
        let Some(out) = out else { return };
        for t in self.tracks.iter_mut().flatten() {
            if let Some(m) = t.chain.as_mut().and_then(|c| c.midi.as_mut())
                && let Some(port) = m.port
            {
                m.release_all(|msg| out.send(port, 0, msg));
            }
        }
    }

    /// Hands every node's meter reading since the last call to `f` and resets them.
    pub fn drain_meters(&mut self, mut f: impl FnMut(NodeId, MeterReading)) {
        for (i, t) in self.tracks.iter_mut().enumerate() {
            if let Some(t) = t {
                f(NodeId::Track(i), t.strip.meter.take());
            }
        }
        for (i, b) in self.buses.iter_mut().enumerate() {
            f(NodeId::Bus(i), b.strip.meter.take());
        }
        f(NodeId::Master, self.master.meter.take());
    }

    /// Events dropped for want of room, on the tracks whose chain is on this thread.
    pub fn dropped_events(&self) -> u64 {
        self.tracks
            .iter()
            .flatten()
            .filter_map(|t| t.chain.as_deref())
            .map(|c| c.dropped_events)
            .sum()
    }
    pub fn set_track_live(&mut self, track: usize, live: bool) {
        if let Some(Some(t)) = self.tracks.get_mut(track)
            && t.live != live
        {
            // Armed or monitored tracks are rendered on the audio thread: what a worker has
            // rendered ahead goes, and the chain comes home.
            if t.prefetch.as_ref().is_some_and(|p| p.is_lent()) {
                Self::release_track(t);
            } else if let Some(ahead) = t.chain.as_mut().and_then(|c| c.ahead.as_mut()) {
                ahead.invalidate();
            }
            t.live = live;
        }
    }
    pub fn lookahead(&self, track: usize) -> Option<crate::anticipation::AheadStatus> {
        self.tracks
            .get(track)?
            .as_ref()?
            .chain
            .as_ref()?
            .ahead
            .as_ref()
            .map(|a| a.status())
    }
    /// How a track's chain is being prefetched by a worker (ENG-015); `None` for a track that is
    /// never lent (a pure timeline, a MIDI track, an offline plan, or switched off).
    pub fn prefetch_status(&self, track: usize) -> Option<PrefetchStatus> {
        Some(
            self.tracks
                .get(track)?
                .as_ref()?
                .prefetch
                .as_ref()?
                .status(),
        )
    }
    pub fn stream_health(&self) -> (u64, bool) {
        self.tracks
            .iter()
            .flatten()
            .filter_map(|t| t.chain.as_deref())
            .fold((0, false), |(missed, failed), c| {
                let (m, f) = c.timeline.stream_health();
                (missed + m, failed || f)
            })
    }
}

impl Chain {
    /// Puts the chain right after a jump or an edit: the notes and controllers its timeline has
    /// started end at the start of the next block, and the next block that plays the timeline
    /// brings every controller to its value there.
    fn release(&mut self) {
        for pitch in 0..128u8 {
            if self.held[pitch as usize] {
                self.held[pitch as usize] = false;
                self.push_now(NoteKind::Off { pitch });
            }
        }
        for number in 0..CONTROL_NUMBERS {
            if self.moved[number] {
                self.moved[number] = false;
                let number = number as u8;
                self.push_now(NoteKind::Control {
                    number,
                    value: rest_value(number),
                });
            }
        }
        self.chase = true;
    }

    fn push_now(&mut self, kind: NoteKind) {
        if self.events.len() < self.events.capacity() {
            self.events.push(NoteEvent { offset: 0, kind });
        } else {
            self.dropped_events += 1;
        }
    }
}
