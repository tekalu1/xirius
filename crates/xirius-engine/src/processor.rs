//! The interface every sound-making or sound-changing node implements.
//!
//! A [`Processor`] is prepared on a non-real-time thread and then moved to the audio thread,
//! where only [`Processor::process`] is called. `process` must not allocate, lock or do I/O.

/// A note event inside one block. `offset` is the frame index inside the block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteEvent {
    pub offset: u32,
    pub kind: NoteKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteKind {
    On {
        pitch: u8,
        velocity: f32,
    },
    Off {
        pitch: u8,
    },
    /// A controller change on the note's channel: a MIDI CC (`number` 0..=127, `value`
    /// 0..=127), channel aftertouch ([`CTRL_AFTERTOUCH`], 0..=127) or pitch bend
    /// ([`CTRL_PITCH_BEND`], -8192..=8191). An instrument that wants them keeps them with
    /// [`ControlState`](crate::control::ControlState).
    Control {
        number: u8,
        value: i32,
    },
}

/// The `number` of a [`NoteKind::Control`] that is channel aftertouch.
pub const CTRL_AFTERTOUCH: u8 = 128;
/// The `number` of a [`NoteKind::Control`] that is pitch bend.
pub const CTRL_PITCH_BEND: u8 = 129;

impl NoteKind {
    pub fn control(number: u8, value: i32) -> NoteKind {
        NoteKind::Control { number, value }
    }
}

/// One block of stereo audio, processed in place.
///
/// Sources (instruments, oscillators) overwrite it; effects read and write it.
pub struct AudioIo<'a> {
    pub left: &'a mut [f32],
    pub right: &'a mut [f32],
}

impl AudioIo<'_> {
    pub fn frames(&self) -> usize {
        self.left.len()
    }

    pub fn clear(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
    }
}

/// Whether a block is rendered for the device or faster/slower than real time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    Realtime,
    Offline,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessSetup {
    pub sample_rate: f64,
    /// The engine never calls `process` with more frames than this (ENG-023).
    pub max_block: usize,
    pub mode: RenderMode,
}

pub trait Processor: Send {
    fn name(&self) -> &str;

    /// Called on a non-real-time thread before the processor joins a plan.
    fn prepare(&mut self, setup: &ProcessSetup);

    /// Called on the audio thread. `io.frames() <= setup.max_block`.
    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]);

    /// Latency the processor adds, in frames.
    fn latency(&self) -> u32 {
        0
    }
    /// True only for a silent source whose signal comes wholly from its timeline.
    /// Mixing that signal ahead does not advance any processor state.
    fn timeline_only(&self) -> bool {
        false
    }
    /// For a processor whose work runs in another process (ENG-060): the frames processed
    /// there and the time that took, in ns, both counted from a point of its own. `None` for
    /// one that works where it is called.
    fn child_time(&self) -> Option<(u64, u64)> {
        None
    }

    /// Starts getting ready for `setup` before the plan is built (ENG-064), so that a song's
    /// plugins open one by one while its window keeps drawing. A processor whose work runs in
    /// another process asks for the opening there and returns at once, and
    /// [`Processor::preparing`] is true until the answer has come; [`Processor::finish_prepare`]
    /// then takes it. By default the processor is prepared here.
    fn begin_prepare(&mut self, setup: &ProcessSetup) {
        self.prepare(setup);
    }

    /// Ends what [`Processor::begin_prepare`] started for the same `setup`, waiting for it if
    /// need be. By default there is nothing left.
    fn finish_prepare(&mut self, _setup: &ProcessSetup) {}

    /// True while what [`Processor::begin_prepare`] started is still going on.
    fn preparing(&self) -> bool {
        false
    }

    /// Lets [`Processor::finish_prepare`] return before the work in another process is done
    /// (ENG-065): the processor then stands in with the dry signal (silence, for an
    /// instrument) and sounds from the block it is ready in. By default there is nothing to
    /// wait for.
    fn allow_late(&mut self) {}
}

/// A processor made ready before its plan (ENG-064): `prepare` with the setup it was readied
/// for only ends that ([`Processor::finish_prepare`]); with another one it prepares anew.
pub struct Readied {
    inner: Box<dyn Processor>,
    setup: Option<ProcessSetup>,
}

impl Readied {
    /// Begins readying `inner` for `setup`.
    pub fn new(mut inner: Box<dyn Processor>, setup: &ProcessSetup) -> Readied {
        inner.begin_prepare(setup);
        Readied {
            inner,
            setup: Some(*setup),
        }
    }
}

impl Processor for Readied {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        if self.setup.take() == Some(*setup) {
            self.inner.finish_prepare(setup);
        } else {
            self.inner.prepare(setup);
        }
    }

    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        self.inner.process(io, events);
    }

    fn latency(&self) -> u32 {
        self.inner.latency()
    }

    fn timeline_only(&self) -> bool {
        self.inner.timeline_only()
    }

    fn child_time(&self) -> Option<(u64, u64)> {
        self.inner.child_time()
    }

    fn preparing(&self) -> bool {
        self.setup.is_some() && self.inner.preparing()
    }

    fn allow_late(&mut self) {
        self.inner.allow_late();
    }
}
