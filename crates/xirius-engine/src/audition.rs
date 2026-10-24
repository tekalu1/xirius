//! The engine half of the file browser's audition (BRW-020): a plan of one track that plays a
//! decoded file from its start and stops by itself at its end, and the commands that put a file
//! on it. The plan is built once and kept; each file is a new timeline on the same track, so a
//! press does not build a player.

use std::sync::Arc;

use crate::engine::{Command, Controller};
use crate::mixer::{MixSpec, Plan, PlanError, TrackSlot, TrackSpec};
use crate::processor::{AudioIo, NoteEvent, ProcessSetup, Processor};
use crate::timeline::{AudioData, Region, Timeline};

/// The rate the audition plays at; a file of another rate is converted to it.
pub const RATE: u32 = 48_000;

/// Frames per block of the audition's plan.
pub const MAX_BLOCK: usize = 512;

/// The track's source: silence, so the file on the timeline is all it plays.
struct Quiet;

impl Processor for Quiet {
    fn name(&self) -> &str {
        "audition"
    }

    fn prepare(&mut self, _setup: &ProcessSetup) {}

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        io.clear();
    }

    fn timeline_only(&self) -> bool {
        true
    }
}

/// The plan of the audition: one track, nothing on it yet.
pub fn plan(setup: ProcessSetup) -> Result<Plan, PlanError> {
    Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(TrackSpec::new("audition", Box::new(Quiet)))],
            ..MixSpec::default()
        },
        setup,
        None,
    )
}

/// The timeline that holds the whole of `data` from the start.
pub fn timeline(data: Arc<AudioData>) -> Timeline {
    let len = data.left.len() as u64;
    Timeline::new(
        Vec::new(),
        vec![Region {
            start: 0,
            len,
            offset: 0,
            gain: 1.0,
            data,
            shape: Default::default(),
        }],
    )
}

/// Puts `data` on the audition's track and plays it from the start to its end. The engine's
/// transport events carry `serial` from then on, so what it said of the file before is told
/// apart. False when the command queue is full.
pub fn cue(control: &mut Controller, data: Arc<AudioData>, serial: u64) -> bool {
    let len = data.left.len() as u64;
    [
        Command::Stop,
        Command::SetTimeline {
            track: 0,
            timeline: Box::new(timeline(data)),
        },
        Command::SetEnd(Some(len)),
        Command::SeekTracked { frame: 0, serial },
        Command::Play,
    ]
    .into_iter()
    .all(|c| control.send(c).is_ok())
}

/// Stops the audition and lets go of the file it held.
pub fn halt(control: &mut Controller) -> bool {
    [
        Command::Stop,
        Command::SetTimeline {
            track: 0,
            timeline: Box::new(Timeline::new(Vec::new(), Vec::new())),
        },
    ]
    .into_iter()
    .all(|c| control.send(c).is_ok())
}

pub mod bus;
