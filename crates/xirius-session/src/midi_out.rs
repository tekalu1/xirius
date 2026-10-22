//! The MIDI output ports the tracks of a song send to (ENG-028): which ports the song names, and
//! the output the engine is given for them.

use std::collections::BTreeSet;

use xirius_engine::Command;
use xirius_engine::midi_out::{MidiOutput, MidirOutput};

use crate::graph::Structure;
use crate::host::{SessionHost, SyncReport};

/// Makes the output for the ports a song names, for an engine at the given sample rate: the
/// system's ports, or a stand-in in tests.
pub type MidiOutMaker =
    Box<dyn FnMut(&[String], f64) -> Result<Box<dyn MidiOutput>, String> + Send>;

/// The maker that opens the system's MIDI output ports.
pub fn system_maker() -> MidiOutMaker {
    Box::new(|ports, rate| {
        MidirOutput::open(ports, rate).map(|o| Box::new(o) as Box<dyn MidiOutput>)
    })
}

/// What the host keeps of the MIDI output.
#[derive(Default)]
pub struct MidiOutState {
    maker: Option<MidiOutMaker>,
    /// The ports the engine's output was made for.
    open: BTreeSet<String>,
    /// The ports that could not be opened, so the warning is given once, not at every sync.
    failed: Option<BTreeSet<String>>,
}

/// The ports the tracks of `s` send to.
fn wanted(s: &Structure) -> BTreeSet<String> {
    s.tracks
        .iter()
        .filter_map(|t| t.midi.as_ref().map(|m| m.port.clone()))
        .collect()
}

impl SessionHost {
    /// Where the song's MIDI output ports come from (`None`: tracks that play to a MIDI port
    /// make no sound, and a warning says so). Returns the warnings of opening them.
    pub fn set_midi_out(&mut self, maker: Option<MidiOutMaker>) -> Vec<String> {
        self.midi_out.maker = maker;
        self.midi_out.open.clear();
        self.midi_out.failed = None;
        let mut report = SyncReport::default();
        self.sync_midi_out(&mut report);
        report.warnings
    }

    /// The ports the engine's MIDI output is open for.
    pub fn midi_out_ports(&self) -> Vec<String> {
        self.midi_out.open.iter().cloned().collect()
    }

    /// Gives the engine an output for the ports the song names now, when they are not the ones
    /// it has. Ports that cannot be opened are named in the report's warnings.
    pub(crate) fn sync_midi_out(&mut self, report: &mut SyncReport) {
        let want = wanted(&self.structure);
        if want == self.midi_out.open {
            return;
        }
        if want.is_empty() {
            if self.send(Command::SetMidiOutput(None)) {
                self.midi_out.open.clear();
            }
            return;
        }
        if self.midi_out.failed.as_ref() == Some(&want) {
            return;
        }
        let names: Vec<String> = want.iter().cloned().collect();
        let rate = f64::from(self.rate());
        let made = match &mut self.midi_out.maker {
            Some(maker) => maker(&names, rate),
            None => Err(format!("MIDI 出力を開けません: {}", names.join("、"))),
        };
        match made {
            Ok(out) => {
                for name in &names {
                    if out.port_index(name).is_none() {
                        report
                            .warnings
                            .push(format!("MIDI 出力「{name}」が見つかりません"));
                    }
                }
                if self.send(Command::SetMidiOutput(Some(out))) {
                    self.midi_out.open = want;
                    self.midi_out.failed = None;
                } else {
                    report.deferred += 1;
                }
            }
            Err(e) => {
                report.warnings.push(e);
                self.midi_out.failed = Some(want);
            }
        }
    }
}
