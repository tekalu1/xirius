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
    /// whatever this says; an offline plan never prefetches.
    pub prefetch: bool,
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

pub struct BusSpec {
    pub name: String,
    pub inserts: Vec<Box<dyn Processor>>,
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    /// The master, or a bus with a higher index than this one.
    pub output: Output,
    /// To buses with a higher index than this one.
    pub sends: Vec<SendSpec>,
}

impl BusSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            inserts: Vec::new(),
            gain: 1.0,
            pan: 0.0,
            mute: false,
            output: Output::Master,
            sends: Vec::new(),
        }
    }
}

/// A track in a new plan: either built fresh, or carried over from the plan being replaced so
/// that its processors keep their state (oscillator phase, plugin instance) across the swap.
pub enum TrackSlot {
    New(TrackSpec),
    Keep(usize),
}

pub struct MixSpec {
    pub tracks: Vec<TrackSlot>,
    pub buses: Vec<BusSpec>,
    pub master_inserts: Vec<Box<dyn Processor>>,
    pub master_gain: f32,
}

impl Default for MixSpec {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            buses: Vec::new(),
            master_inserts: Vec::new(),
            master_gain: 1.0,
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
    live: bool,
    name: String,
    /// What makes the track's signal: home with the node, or away with a prefetch worker.
    chain: Option<Box<Chain>>,
    prefetch: Option<Box<Prefetch<Chain>>>,
    strip: Strip,
    sends: Vec<SendNode>,
    output: Output,
    /// The chain's block, which the fader, pan and sends go on from.
    buf: Stereo,
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
    name: String,
    inserts: Vec<Box<dyn Processor>>,
    strip: Strip,
    sends: Vec<SendNode>,
    output: Output,
    buf: Stereo,
}

struct MasterNode {
    inserts: Vec<Box<dyn Processor>>,
    gain: Smoothed,
    buf: Stereo,
    meter: MeterAcc,
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
    routes: Vec<Route>,
    automation: crate::automation_time::Timing,
    track_delays: Vec<NodeDelays>,
    bus_delays: Vec<NodeDelays>,
    compensation: (Vec<Compensation>, Vec<Compensation>),
    latency: u32,
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
        let mut tracks = Vec::with_capacity(spec.tracks.len());
        let mut keep_from = Vec::with_capacity(spec.tracks.len());
        let mut routes = Vec::with_capacity(spec.tracks.len());
        let mut track_timing = Vec::with_capacity(spec.tracks.len());
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
                        && !pure
                        && t.midi.is_none()
                        && setup.mode == crate::processor::RenderMode::Realtime)
                        .then(|| Prefetch::new(n))
                        .flatten()
                        .map(Box::new);
                    tracks.push(Some(Box::new(TrackNode {
                        live: t.live,
                        name: t.name,
                        prefetch,
                        strip: Strip::new(t.gain, t.pan, t.mute),
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
                        })),
                    })));
                    keep_from.push(None);
                }
            }
        }
        let mut buses = Vec::with_capacity(bus_count);
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
            prepare_all(&mut b.inserts, &setup);
            buses.push(BusNode {
                name: b.name,
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
            });
        }
        let mut master_inserts = spec.master_inserts;
        prepare_all(&mut master_inserts, &setup);
        let bus_routes: Vec<Route> = buses
            .iter()
            .map(|b| Route {
                latency: latency_of(&b.inserts),
                output: b.output,
                sends: sends_of(&b.sends),
            })
            .collect();
        let (track_comp, bus_comp, latency) =
            compensate(&routes, &bus_routes, latency_of(&master_inserts));
        let (arrive, _) = crate::automation_time::arrivals(&routes, &bus_routes);
        let automation = crate::automation_time::Timing {
            tracks: track_timing,
            buses: buses
                .iter()
                .enumerate()
                .map(|(i, b)| crate::automation_time::Chain {
                    input: arrive[i],
                    inserts: b.inserts.iter().map(|p| p.latency()).collect(),
                })
                .collect(),
        };
        let mut plan = Plan {
            pool: None,
            track_delays: track_comp.iter().map(|c| NodeDelays::new(c, n)).collect(),
            bus_delays: bus_comp.iter().map(|c| NodeDelays::new(c, n)).collect(),
            compensation: (track_comp, bus_comp),
            latency,
            automation,
            routes,
            setup,
            tracks,
            keep_from,
            buses,
            master: MasterNode {
                inserts: master_inserts,
                gain: Smoothed::new(spec.master_gain),
                buf: Stereo::new(n),
                meter: MeterAcc::default(),
            },
            pre: Stereo::new(n),
            #[cfg(test)]
            pushed: Vec::new(),
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
            }
        }
    }

    /// How late the mix comes out of the master, in frames: the longest path from a source
    /// through its inserts, buses and the master inserts. Every other path is delayed to match.
    pub fn latency(&self) -> u32 {
        self.latency
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
        self.master.buf.clear(n);

        if let Some(pool) = &mut self.pool {
            pool.render(&mut self.tracks, n, pos);
        } else {
            for t in self.tracks.iter_mut().flatten() {
                Self::render_track(t, n, pos);
            }
        }

        for (t, delays) in self.tracks.iter_mut().zip(&mut self.track_delays) {
            let Some(t) = t else { continue };
            let has_pre = t.sends.iter().any(|s| s.pre_fader);
            if has_pre {
                // Pre-fader sends still follow the mute.
                for i in 0..n {
                    let m = t.strip.mute.at(i, n);
                    self.pre.l[i] = t.buf.l[i] * m;
                    self.pre.r[i] = t.buf.r[i] * m;
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
            t.strip.settle();
            t.strip.meter.add(&t.buf, n);
        }

        for i in 0..self.buses.len() {
            // Everything a bus feeds comes after it, so the rest of the list can be borrowed.
            let (head, later) = self.buses.split_at_mut(i + 1);
            let b = &mut head[i];
            for p in &mut b.inserts {
                p.process(&mut b.buf.io(n), &[]);
            }
            let has_pre = b.sends.iter().any(|s| s.pre_fader);
            if has_pre {
                for k in 0..n {
                    let m = b.strip.mute.at(k, n);
                    self.pre.l[k] = b.buf.l[k] * m;
                    self.pre.r[k] = b.buf.r[k] * m;
                }
            }
            b.strip.apply(&mut b.buf, n);
            let delays = &mut self.bus_delays[i];
            for (s, d) in b.sends.iter_mut().zip(&mut delays.sends) {
                let src = if s.pre_fader { &self.pre } else { &b.buf };
                d.add(&mut later[s.bus - i - 1].buf, src, n, &s.level);
                s.level.settle();
            }
            b.strip.settle();
            b.strip.meter.add(&b.buf, n);
            let dest = match b.output {
                Output::Master => &mut self.master.buf,
                Output::Bus(to) => &mut later[to - i - 1].buf,
            };
            delays.output.add(dest, &b.buf, n, &Smoothed::new(1.0));
        }

        let m = &mut self.master;
        for p in &mut m.inserts {
            p.process(&mut m.buf.io(n), &[]);
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
        for p in &mut c.inserts {
            p.process(
                &mut AudioIo {
                    left: &mut left[..n],
                    right: &mut right[..n],
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
