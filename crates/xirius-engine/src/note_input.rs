//! A fixed-capacity, latency-aligned note inlet for a receiving effect (MIX-051).
use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor};

const QUEUED: usize = 4096;

pub struct NoteInput {
    inner: Box<dyn Processor>,
    source: usize,
    delay: u32,
    start: u64,
    pending: Vec<(u64, NoteKind)>,
    block: Vec<NoteEvent>,
    sounding: [bool; 128],
    reset: bool,
}

impl NoteInput {
    pub fn new(inner: Box<dyn Processor>, source: usize) -> Self {
        Self {
            inner,
            source,
            delay: 0,
            start: 0,
            pending: Vec::new(),
            block: Vec::new(),
            sounding: [false; 128],
            reset: false,
        }
    }
}

impl Processor for NoteInput {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.inner.prepare(setup);
        self.pending = Vec::with_capacity(QUEUED);
        self.block = Vec::with_capacity(QUEUED + 128);
    }

    fn begin_prepare(&mut self, setup: &ProcessSetup) {
        self.inner.begin_prepare(setup);
    }

    fn finish_prepare(&mut self, setup: &ProcessSetup) {
        self.inner.finish_prepare(setup);
        self.pending = Vec::with_capacity(QUEUED);
        self.block = Vec::with_capacity(QUEUED + 128);
    }

    fn preparing(&self) -> bool {
        self.inner.preparing()
    }

    fn allow_late(&mut self) {
        self.inner.allow_late();
    }

    fn note_sources(&self, out: &mut Vec<usize>) {
        out.push(self.source);
    }

    fn note_delay(&mut self, before: u32) {
        self.delay = before;
    }

    fn note_block(&mut self, source: usize, events: &[NoteEvent], start: u64, reset: bool) {
        if source != self.source {
            return;
        }
        self.start = start;
        if reset {
            self.pending.clear();
            self.reset = true;
        }
        for e in events {
            if self.pending.len() < self.pending.capacity() {
                self.pending.push((
                    start.saturating_add(u64::from(e.offset) + u64::from(self.delay)),
                    e.kind,
                ));
            } else {
                // Bounded RT storage: abandon the backlog and release held notes on the
                // next process call rather than leaving an unmatched note-on sounding.
                self.pending.clear();
                self.reset = true;
                self.pending.push((
                    start.saturating_add(u64::from(e.offset) + u64::from(self.delay)),
                    e.kind,
                ));
            }
        }
    }

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        self.block.clear();
        if std::mem::take(&mut self.reset) {
            for (pitch, on) in self.sounding.iter_mut().enumerate() {
                if std::mem::take(on) && self.block.len() < self.block.capacity() {
                    self.block.push(NoteEvent {
                        offset: 0,
                        kind: NoteKind::Off { pitch: pitch as u8 },
                    });
                }
            }
        }
        let end = self.start.saturating_add(io.frames() as u64);
        let mut keep = 0;
        for i in 0..self.pending.len() {
            let (at, kind) = self.pending[i];
            if at < end {
                if self.block.len() < self.block.capacity() {
                    self.block.push(NoteEvent {
                        offset: at.saturating_sub(self.start) as u32,
                        kind,
                    });
                    match kind {
                        NoteKind::On { pitch, .. } => self.sounding[pitch as usize] = true,
                        NoteKind::Off { pitch } => self.sounding[pitch as usize] = false,
                        NoteKind::Control { .. } => {}
                    }
                }
            } else {
                self.pending[keep] = (at, kind);
                keep += 1;
            }
        }
        self.pending.truncate(keep);
        self.inner.process(io, &self.block);
    }

    fn latency(&self) -> u32 {
        self.inner.latency()
    }

    fn child_time(&self) -> Option<(u64, u64)> {
        self.inner.child_time()
    }
}
