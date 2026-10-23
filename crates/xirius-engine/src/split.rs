//! The splitter (ENG-074): one insert that hands its input to parallel branches and sums them
//! back. Each branch is an insert chain of its own, then gain and pan at the merge.
//!
//! A branch whose chain is shorter than the longest is delayed by the difference after its
//! chain, so every branch reaches the merge at the same time, to the sample; the splitter
//! reports that longest latency as its own, and the plan compensates it like any insert's.
//! A muted branch is not processed at all (after a block's fade). When it is unmuted it runs
//! silent for the splitter's latency first, so its delay line fills and its inserts let go of
//! what they held from before the mute, and only then fades in. The branches run one after
//! another on the thread that runs the chain.
//!
//! Everything is allocated in `prepare`, which runs while the plan is built; `process` only
//! copies, calls the branches' inserts and sums. A branch's gain, pan and mute live in atomics
//! the plan shares ([`Cells`]), so [`crate::Command::BranchGain`] reaches a splitter wherever its
//! chain is, also while a prefetch worker has it (it is heard there as soon as the worker renders
//! the next block).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::mixer::pan_gains;
use crate::processor::{AudioIo, NoteEvent, ProcessSetup, Processor};

/// A branch's values at the merge: written by the plan, read by the splitter every block.
#[derive(Debug)]
pub struct BranchCell {
    gain: AtomicU32,
    pan: AtomicU32,
    mute: AtomicBool,
}

impl BranchCell {
    fn new(gain: f32, pan: f32, mute: bool) -> BranchCell {
        let cell = BranchCell {
            gain: AtomicU32::new(0),
            pan: AtomicU32::new(0),
            mute: AtomicBool::new(false),
        };
        cell.set(gain, pan, mute);
        cell
    }

    /// `gain` is linear; `pan` is -1 (left) ..= 1 (right).
    pub fn set(&self, gain: f32, pan: f32, mute: bool) {
        self.gain.store(gain.to_bits(), Ordering::Relaxed);
        self.pan
            .store(pan.clamp(-1.0, 1.0).to_bits(), Ordering::Relaxed);
        self.mute.store(mute, Ordering::Relaxed);
    }

    /// (gain, pan, mute).
    pub fn get(&self) -> (f32, f32, bool) {
        (
            f32::from_bits(self.gain.load(Ordering::Relaxed)),
            f32::from_bits(self.pan.load(Ordering::Relaxed)),
            self.mute.load(Ordering::Relaxed),
        )
    }
}

/// One branch as the builder hands it over.
pub struct BranchSpec {
    /// The branch's node id in the song.
    pub node: u64,
    /// Linear.
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    /// The branch's chain, in order: each insert with the id its automation goes by (a
    /// plug-in's id; for a nested splitter, its node id).
    pub inserts: Vec<(u64, Box<dyn Processor>)>,
}

impl BranchSpec {
    /// A branch at unity, centred and sounding.
    pub fn new(node: u64, inserts: Vec<(u64, Box<dyn Processor>)>) -> BranchSpec {
        BranchSpec {
            node,
            gain: 1.0,
            pan: 0.0,
            mute: false,
            inserts,
        }
    }
}

/// A value moved to its target across one block, as the mixer's faders are.
struct Ramp {
    current: f32,
    target: f32,
}

impl Ramp {
    fn new(v: f32) -> Ramp {
        Ramp {
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

/// A delay applied in place: a power-of-two ring sized in `prepare`.
#[derive(Default)]
struct Line {
    l: Vec<f32>,
    r: Vec<f32>,
    frames: usize,
    pos: usize,
}

impl Line {
    fn new(frames: u32, max_block: usize) -> Line {
        let len = if frames == 0 {
            0
        } else {
            (frames as usize + max_block).next_power_of_two()
        };
        Line {
            l: vec![0.0; len],
            r: vec![0.0; len],
            frames: frames as usize,
            pos: 0,
        }
    }

    fn apply(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.frames == 0 {
            return;
        }
        let mask = self.l.len() - 1;
        let n = left.len();
        for i in 0..n {
            let w = (self.pos + i) & mask;
            self.l[w] = left[i];
            self.r[w] = right[i];
            let r = w.wrapping_sub(self.frames) & mask;
            left[i] = self.l[r];
            right[i] = self.r[r];
        }
        self.pos = (self.pos + n) & mask;
    }

    fn clear(&mut self) {
        self.l.fill(0.0);
        self.r.fill(0.0);
    }
}

struct BranchProc {
    node: u64,
    cell: Arc<BranchCell>,
    /// The automation id of each insert.
    ids: Vec<u64>,
    inserts: Vec<Box<dyn Processor>>,
    l: Vec<f32>,
    r: Vec<f32>,
    /// What lines this branch up with the longest one.
    delay: Line,
    gain: Ramp,
    pan: Ramp,
    /// 1 sounding, 0 muted.
    on: Ramp,
    /// Skipped while muted: its chain and delay line did not run.
    asleep: bool,
    /// Frames still to run silent after waking, before it fades in.
    hold: u32,
}

impl BranchProc {
    fn latency(&self) -> u32 {
        self.inserts.iter().map(|p| p.latency()).sum()
    }
}

/// The splitter in [`crate::rack`]'s normal mode: every branch gets the same input.
pub struct SplitProc {
    node: u64,
    branches: Vec<BranchProc>,
    latency: u32,
}

impl SplitProc {
    pub fn new(node: u64, branches: Vec<BranchSpec>) -> SplitProc {
        SplitProc {
            node,
            branches: branches
                .into_iter()
                .map(|b| {
                    let (ids, inserts) = b.inserts.into_iter().unzip();
                    BranchProc {
                        node: b.node,
                        cell: Arc::new(BranchCell::new(b.gain, b.pan, b.mute)),
                        ids,
                        inserts,
                        l: Vec::new(),
                        r: Vec::new(),
                        delay: Line::default(),
                        gain: Ramp::new(b.gain),
                        pan: Ramp::new(b.pan.clamp(-1.0, 1.0)),
                        on: Ramp::new(if b.mute { 0.0 } else { 1.0 }),
                        asleep: false,
                        hold: 0,
                    }
                })
                .collect(),
            latency: 0,
        }
    }

    /// The splitter's node id in the song.
    pub fn node(&self) -> u64 {
        self.node
    }

    pub fn branches(&self) -> usize {
        self.branches.len()
    }

    /// Each branch's node id and shared values, nested splitters' too.
    pub fn cells(&self, out: &mut Vec<(u64, Arc<BranchCell>)>) {
        for b in &self.branches {
            out.push((b.node, b.cell.clone()));
            for p in &b.inserts {
                if let Some(s) = p.split() {
                    s.cells(out);
                }
            }
        }
    }

    /// For automation (ENG-074): every insert inside, as the insert `at` of the row that holds
    /// the splitter, with the frames from the splitter's input to that insert's plus `base`. An
    /// insert inside a branch is as late as what comes before it in its branch; the delay that
    /// lines the branch up comes after its chain.
    fn inner(&self, at: usize, base: u32, out: &mut Vec<Inner>) {
        for b in &self.branches {
            let mut offset = base;
            for (id, p) in b.ids.iter().zip(&b.inserts) {
                match p.split() {
                    Some(s) => s.inner(at, offset, out),
                    None => out.push(Inner {
                        id: *id,
                        insert: at,
                        offset,
                    }),
                }
                offset += p.latency();
            }
        }
    }

    fn size(&mut self, setup: &ProcessSetup) {
        self.latency = self
            .branches
            .iter()
            .map(BranchProc::latency)
            .max()
            .unwrap_or(0);
        for b in &mut self.branches {
            b.l = vec![0.0; setup.max_block];
            b.r = vec![0.0; setup.max_block];
            b.delay = Line::new(self.latency - b.latency(), setup.max_block);
        }
    }
}

impl Processor for SplitProc {
    fn name(&self) -> &str {
        "Splitter"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        for b in &mut self.branches {
            for p in &mut b.inserts {
                p.prepare(setup);
            }
        }
        self.size(setup);
    }

    fn begin_prepare(&mut self, setup: &ProcessSetup) {
        for b in &mut self.branches {
            for p in &mut b.inserts {
                p.begin_prepare(setup);
            }
        }
    }

    fn finish_prepare(&mut self, setup: &ProcessSetup) {
        for b in &mut self.branches {
            for p in &mut b.inserts {
                p.finish_prepare(setup);
            }
        }
        self.size(setup);
    }

    fn preparing(&self) -> bool {
        self.branches
            .iter()
            .any(|b| b.inserts.iter().any(|p| p.preparing()))
    }

    fn allow_late(&mut self) {
        for b in &mut self.branches {
            for p in &mut b.inserts {
                p.allow_late();
            }
        }
    }

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        let n = io.frames();
        let latency = self.latency;
        for b in &mut self.branches {
            let (gain, pan, mute) = b.cell.get();
            b.gain.target = gain;
            b.pan.target = pan;
            b.on.target = if mute { 0.0 } else { 1.0 };
            if b.on.current == 0.0 && b.on.target == 0.0 {
                b.asleep = true;
                continue;
            }
            if b.asleep {
                // What comes out is silence and then what the inserts held; it is all heard at
                // zero gain until the input has gone through the whole branch.
                b.delay.clear();
                b.asleep = false;
                b.hold = latency;
            }
            let (l, r) = (&mut b.l[..n], &mut b.r[..n]);
            l.copy_from_slice(&io.left[..n]);
            r.copy_from_slice(&io.right[..n]);
            for p in &mut b.inserts {
                p.process(&mut AudioIo::new(l, r), &[]);
            }
            b.delay.apply(l, r);
        }
        io.clear();
        for b in &mut self.branches {
            if b.hold > 0 {
                b.hold = b.hold.saturating_sub(n as u32);
                b.gain.settle();
                b.pan.settle();
                continue;
            }
            if !b.asleep {
                for i in 0..n {
                    let g = b.gain.at(i, n) * b.on.at(i, n);
                    let (pl, pr) = pan_gains(b.pan.at(i, n));
                    io.left[i] += b.l[i] * g * pl;
                    io.right[i] += b.r[i] * g * pr;
                }
            }
            b.gain.settle();
            b.pan.settle();
            b.on.settle();
        }
    }

    fn latency(&self) -> u32 {
        self.latency
    }

    fn split(&self) -> Option<&SplitProc> {
        Some(self)
    }
}

/// The branch values a plan node's splitters share, by branch node id (ENG-074). Collected
/// when the node is built; it moves with the node when a later plan keeps the track.
#[derive(Default)]
pub(crate) struct Cells(Vec<(u64, Arc<BranchCell>)>);

impl Cells {
    pub(crate) fn of(list: &[Box<dyn Processor>]) -> Cells {
        let mut out = Vec::new();
        for p in list {
            if let Some(s) = p.split() {
                s.cells(&mut out);
            }
        }
        Cells(out)
    }

    /// Sets a branch's values; false when no branch here has that id.
    pub(crate) fn set(&self, node: u64, gain: f32, pan: f32, mute: bool) -> bool {
        match self.0.iter().find(|(id, _)| *id == node) {
            Some((_, cell)) => {
                cell.set(gain, pan, mute);
                true
            }
            None => false,
        }
    }
}

/// An insert inside a splitter, for automation (ENG-074): it is as late as the row's insert
/// `insert` (the splitter) plus `offset` frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inner {
    /// The insert's automation id (its plug-in's id).
    pub id: u64,
    pub insert: usize,
    pub offset: u32,
}

/// The inserts inside each node's splitters (ENG-074), kept in the plan's shape beside
/// [`crate::automation_time::Timing`]: an inner insert's automation is as late as
/// `timing.insert(inner.insert) + inner.offset`, so it follows whatever moves the row's
/// inserts. Empty for a node without a splitter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Nested {
    pub tracks: Vec<Vec<Inner>>,
    pub buses: Vec<Vec<Inner>>,
}

impl Nested {
    /// The inner inserts of one row of inserts.
    pub fn of(list: &[Box<dyn Processor>]) -> Vec<Inner> {
        let mut out = Vec::new();
        for (at, p) in list.iter().enumerate() {
            if let Some(s) = p.split() {
                s.inner(at, 0, &mut out);
            }
        }
        out
    }

    /// The delay before the inner insert `id` of track `track`, by `timing`.
    pub fn track_delay(
        &self,
        timing: &crate::automation_time::Timing,
        track: usize,
        id: u64,
    ) -> Option<u32> {
        Self::delay(self.tracks.get(track)?, timing.tracks.get(track)?, id)
    }

    /// The delay before the inner insert `id` of bus `bus`, by `timing`.
    pub fn bus_delay(
        &self,
        timing: &crate::automation_time::Timing,
        bus: usize,
        id: u64,
    ) -> Option<u32> {
        Self::delay(self.buses.get(bus)?, timing.buses.get(bus)?, id)
    }

    fn delay(list: &[Inner], chain: &crate::automation_time::Chain, id: u64) -> Option<u32> {
        let x = list.iter().find(|x| x.id == id)?;
        Some(chain.insert(x.insert) + x.offset)
    }
}
