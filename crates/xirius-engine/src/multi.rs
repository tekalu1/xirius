//! The multi-instrument (ENG-075): a track's source when its rack holds layers. Each layer is an
//! instrument and its own insert chain; a note goes to the layers whose key and velocity ranges
//! take it, moved by each layer's transpose, and its note-off goes only to the layers its
//! note-on reached. Controllers go to every layer. The layers are summed with their gain, pan and
//! mute, which live in the same shared cells as a splitter's branches
//! ([`crate::split::BranchCell`]), so [`crate::Command::BranchGain`] reaches a layer too.
//!
//! A layer whose chain is shorter than the longest is delayed by the difference, as a splitter's
//! branch is, and the multi reports the longest as its latency. Unlike a branch, a muted layer
//! keeps running unheard, so a note it holds still ends and nothing is cut when it comes back.
//!
//! Everything is allocated in `prepare`; `process` only routes, calls the layers and sums.

use std::sync::Arc;

use crate::mixer::pan_gains;
use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor};
use crate::split::{BranchCell, Line, Ramp};

/// A multi plays no more than this many layers; the rest are left out.
pub const MAX_LAYERS: usize = 64;

/// The note events a layer can be handed in one block; more are dropped.
const EVENTS: usize = 1024;

/// One layer as the builder hands it over.
pub struct LayerSpec {
    /// The layer's node id in the song.
    pub node: u64,
    pub instrument: Box<dyn Processor>,
    /// The layer's chain after its instrument, each insert with the id its automation goes by.
    pub inserts: Vec<(u64, Box<dyn Processor>)>,
    /// The notes it plays, `low..=high`.
    pub keys: (u8, u8),
    /// The velocities it plays, `low..=high` in 1..=127.
    pub velocity: (u8, u8),
    /// Semitones added to every note it plays.
    pub transpose: i8,
    /// Linear.
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
}

impl LayerSpec {
    /// A layer over every note and velocity, untransposed, at unity, centred and sounding.
    pub fn new(
        node: u64,
        instrument: Box<dyn Processor>,
        inserts: Vec<(u64, Box<dyn Processor>)>,
    ) -> LayerSpec {
        LayerSpec {
            node,
            instrument,
            inserts,
            keys: (0, 127),
            velocity: (1, 127),
            transpose: 0,
            gain: 1.0,
            pan: 0.0,
            mute: false,
        }
    }
}

struct LayerProc {
    node: u64,
    cell: Arc<BranchCell>,
    keys: (u8, u8),
    velocity: (u8, u8),
    transpose: i8,
    instrument: Box<dyn Processor>,
    ids: Vec<u64>,
    inserts: Vec<Box<dyn Processor>>,
    /// This block's events for the instrument.
    events: Vec<NoteEvent>,
    l: Vec<f32>,
    r: Vec<f32>,
    delay: Line,
    gain: Ramp,
    pan: Ramp,
    on: Ramp,
}

impl LayerProc {
    fn latency(&self) -> u32 {
        self.instrument.latency() + self.inserts.iter().map(|p| p.latency()).sum::<u32>()
    }

    /// Whether the note `pitch` at `velocity` (0..=1) is this layer's, and as which note.
    fn takes(&self, pitch: u8, velocity: f32) -> Option<u8> {
        layer_pitch(self.keys, self.velocity, self.transpose, pitch, velocity)
    }

    /// `pitch` moved by the transpose; `None` when that leaves 0..=127.
    fn moved(&self, pitch: u8) -> Option<u8> {
        u8::try_from(i16::from(pitch) + i16::from(self.transpose))
            .ok()
            .filter(|p| *p <= 127)
    }

    fn push(&mut self, e: NoteEvent) {
        if self.events.len() < self.events.capacity() {
            self.events.push(e);
        }
    }

    fn each(&mut self, f: &mut dyn FnMut(&mut Box<dyn Processor>)) {
        f(&mut self.instrument);
        for p in &mut self.inserts {
            f(p);
        }
    }
}

/// The same range and transpose decision used by the engine and pressed-key display.
pub fn layer_pitch(
    keys: (u8, u8),
    velocities: (u8, u8),
    transpose: i8,
    pitch: u8,
    velocity: f32,
) -> Option<u8> {
    let v = ((velocity * 127.0).round() as i32).clamp(1, 127) as u8;
    let keys = keys.0..=keys.1;
    let vels = velocities.0..=velocities.1;
    if !keys.contains(&pitch) || !vels.contains(&v) {
        return None;
    }
    u8::try_from(i16::from(pitch) + i16::from(transpose))
        .ok()
        .filter(|p| *p <= 127)
}

/// The multi-instrument source.
pub struct MultiProc {
    pub pressed: Arc<crate::performance::Pressed>,
    node: u64,
    layers: Vec<LayerProc>,
    /// For each note played in, the layers its note-on reached (bit `i`: layer `i`).
    held: [u64; 128],
    live_held: Vec<(u64, u8, u64)>,
    latency: u32,
}

impl MultiProc {
    pub fn new(node: u64, layers: Vec<LayerSpec>) -> MultiProc {
        MultiProc {
            pressed: Arc::default(),
            node,
            layers: layers
                .into_iter()
                .take(MAX_LAYERS)
                .map(|l| {
                    let (ids, inserts) = l.inserts.into_iter().unzip();
                    LayerProc {
                        node: l.node,
                        cell: Arc::new(BranchCell::new(l.gain, l.pan, l.mute)),
                        keys: (l.keys.0.min(l.keys.1), l.keys.0.max(l.keys.1).min(127)),
                        velocity: (l.velocity.0.max(1), l.velocity.1.min(127)),
                        transpose: l.transpose,
                        instrument: l.instrument,
                        ids,
                        inserts,
                        events: Vec::new(),
                        l: Vec::new(),
                        r: Vec::new(),
                        delay: Line::default(),
                        gain: Ramp::new(l.gain),
                        pan: Ramp::new(l.pan.clamp(-1.0, 1.0)),
                        on: Ramp::new(if l.mute { 0.0 } else { 1.0 }),
                    }
                })
                .collect(),
            held: [0; 128],
            live_held: Vec::with_capacity(EVENTS),
            latency: 0,
        }
    }

    /// The multi's node id in the song.
    pub fn node(&self) -> u64 {
        self.node
    }

    pub fn layers(&self) -> usize {
        self.layers.len()
    }

    /// Each layer's node id and shared values, and those of the splitters in the layers' chains.
    pub fn cells(&self, out: &mut Vec<(u64, Arc<BranchCell>)>) {
        for l in &self.layers {
            out.push((l.node, l.cell.clone()));
            for p in &l.inserts {
                if let Some(s) = p.split() {
                    s.cells(out);
                }
            }
        }
    }

    /// The automation ids of each layer's inserts, in order.
    pub fn insert_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.layers.iter().flat_map(|l| l.ids.iter().copied())
    }

    fn size(&mut self, setup: &ProcessSetup) {
        self.latency = self
            .layers
            .iter()
            .map(LayerProc::latency)
            .max()
            .unwrap_or(0);
        for l in &mut self.layers {
            l.l = vec![0.0; setup.max_block];
            l.r = vec![0.0; setup.max_block];
            l.events = Vec::with_capacity(EVENTS);
            l.delay = Line::new(self.latency - l.latency(), setup.max_block);
        }
        self.held = [0; 128];
    }

    /// Hands this block's events to the layers that take them.
    fn route(&mut self, events: &[NoteEvent]) {
        for l in &mut self.layers {
            l.events.clear();
        }
        for e in events {
            match e.kind {
                NoteKind::On { pitch, velocity } => {
                    let mut reached = 0u64;
                    for (i, l) in self.layers.iter_mut().enumerate() {
                        if let Some(p) = l.takes(pitch, velocity) {
                            l.push(NoteEvent {
                                offset: e.offset,
                                kind: NoteKind::On { pitch: p, velocity },
                            });
                            reached |= 1 << i;
                        }
                    }
                    if let Some(h) = self.held.get_mut(pitch as usize) {
                        *h |= reached;
                    }
                }
                NoteKind::Off { pitch } => {
                    let reached = self.held.get_mut(pitch as usize).map_or(0, std::mem::take);
                    for (i, l) in self.layers.iter_mut().enumerate() {
                        if reached & (1 << i) != 0
                            && let Some(p) = l.moved(pitch)
                        {
                            l.push(NoteEvent {
                                offset: e.offset,
                                kind: NoteKind::Off { pitch: p },
                            });
                        }
                    }
                }
                NoteKind::Control { .. } => {
                    for l in &mut self.layers {
                        l.push(*e);
                    }
                }
            }
        }
    }
}

impl Processor for MultiProc {
    fn release_live_inputs(&mut self) {
        self.live_held.clear();
        self.pressed.publish(std::iter::empty());
        for layer in &mut self.layers {
            layer.instrument.release_live_inputs();
        }
    }
    fn live_input(&mut self, input: crate::performance::Input) -> bool {
        if input.plugin.is_some() {
            return self
                .layers
                .iter_mut()
                .any(|l| l.instrument.live_input(input));
        }
        match input.kind {
            NoteKind::On { pitch, velocity } => {
                if self.live_held.len() == self.live_held.capacity() {
                    // Reset the bounded input history before accepting another press:
                    // an untracked on would let its off consume an older press.
                    self.release_live_inputs();
                }
                let mut reached = 0;
                for (i, l) in self.layers.iter_mut().enumerate() {
                    if let Some(pitch) = l.takes(pitch, velocity)
                        && l.instrument.live_input(crate::performance::Input {
                            kind: NoteKind::On { pitch, velocity },
                            ..input
                        })
                    {
                        reached |= 1 << i;
                    }
                }
                self.live_held.push((input.source, pitch, reached));
            }
            NoteKind::Off { pitch } => {
                let Some(at) = self
                    .live_held
                    .iter()
                    .rposition(|h| h.0 == input.source && h.1 == pitch)
                else {
                    return true;
                };
                let (_, _, reached) = self.live_held.swap_remove(at);
                for (i, l) in self.layers.iter_mut().enumerate() {
                    if reached & (1 << i) != 0
                        && let Some(pitch) = l.moved(pitch)
                    {
                        l.instrument.live_input(crate::performance::Input {
                            kind: NoteKind::Off { pitch },
                            ..input
                        });
                    }
                }
            }
            NoteKind::Control { number, .. } => {
                if matches!(number, 120 | 123) {
                    self.live_held.retain(|h| h.0 != input.source);
                }
                for l in &mut self.layers {
                    l.instrument.live_input(input);
                }
            }
        }
        self.pressed.publish(
            self.live_held
                .iter()
                .filter(|h| h.0 != 0 && h.0 & crate::performance::MOUSE == 0)
                .map(|h| h.1),
        );
        true
    }

    fn name(&self) -> &str {
        "Multi Instrument"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        for l in &mut self.layers {
            l.each(&mut |p| p.prepare(setup));
        }
        self.size(setup);
    }

    fn begin_prepare(&mut self, setup: &ProcessSetup) {
        for l in &mut self.layers {
            l.each(&mut |p| p.begin_prepare(setup));
        }
    }

    fn finish_prepare(&mut self, setup: &ProcessSetup) {
        for l in &mut self.layers {
            l.each(&mut |p| p.finish_prepare(setup));
        }
        self.size(setup);
    }

    fn preparing(&self) -> bool {
        self.layers
            .iter()
            .any(|l| l.instrument.preparing() || l.inserts.iter().any(|p| p.preparing()))
    }

    fn allow_late(&mut self) {
        for l in &mut self.layers {
            l.each(&mut |p| p.allow_late());
        }
    }

    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        let n = io.frames();
        self.route(events);
        for l in &mut self.layers {
            let (gain, pan, mute) = l.cell.get();
            l.gain.target = gain;
            l.pan.target = pan;
            l.on.target = if mute { 0.0 } else { 1.0 };
            let (bl, br) = (&mut l.l[..n], &mut l.r[..n]);
            bl.fill(0.0);
            br.fill(0.0);
            l.instrument.process(&mut AudioIo::new(bl, br), &l.events);
            for p in &mut l.inserts {
                p.process(&mut AudioIo::new(bl, br), &[]);
            }
            l.delay.apply(bl, br);
        }
        io.clear();
        for l in &mut self.layers {
            if l.on.current != 0.0 || l.on.target != 0.0 {
                for i in 0..n {
                    let g = l.gain.at(i, n) * l.on.at(i, n);
                    let (pl, pr) = pan_gains(l.pan.at(i, n));
                    io.left[i] += l.l[i] * g * pl;
                    io.right[i] += l.r[i] * g * pr;
                }
            }
            l.gain.settle();
            l.pan.settle();
            l.on.settle();
        }
    }

    fn latency(&self) -> u32 {
        self.latency
    }

    fn multi(&self) -> Option<&MultiProc> {
        Some(self)
    }
}
