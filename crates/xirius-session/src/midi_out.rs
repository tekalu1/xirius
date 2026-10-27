//! The MIDI output ports the tracks of a song send to (ENG-028): which ports the song names, and
//! the output the engine is given for them.

use std::collections::BTreeSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

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

type Opened = (MidiOutMaker, Result<Box<dyn MidiOutput>, String>);
struct Opening {
    generation: u64,
    want: BTreeSet<String>,
    work: crate::background::Job<Opened>,
}

/// The engine returns this box to its retirement worker. Its acknowledgement
/// follows the inner owner's real connection closure, including a slow driver.
struct Tracked {
    inner: Option<Box<dyn MidiOutput>>,
    done: Arc<AtomicBool>,
}
impl MidiOutput for Tracked {
    fn ports(&self) -> Vec<String> {
        self.inner.as_ref().unwrap().ports()
    }
    fn port_index(&self, name: &str) -> Option<usize> {
        self.inner.as_ref().unwrap().port_index(name)
    }
    fn send(&mut self, port: usize, offset: usize, msg: [u8; 3]) {
        self.inner.as_mut().unwrap().send(port, offset, msg);
    }
}
impl Drop for Tracked {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.close();
        }
        self.done.store(true, Ordering::Release);
    }
}

/// What the host keeps of the MIDI output.
#[derive(Default)]
pub struct MidiOutState {
    generation: u64,
    retiring: Vec<crate::background::Job<()>>,
    connection_end: Option<Arc<AtomicBool>>,
    detaching: bool,
    retirement_failed: bool,
    opening: Option<Opening>,
    pub(crate) warnings: Vec<String>,
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
        self.midi_out.generation = self
            .midi_out
            .generation
            .checked_add(1)
            .expect("MIDI generations exhausted");
        if let Some(previous) = self.midi_out.maker.take()
            && self.windowed
        {
            let retired = crate::background::Job::spawn("midi-maker-retire", move || {
                drop(previous);
                Ok(())
            });
            match retired {
                Ok(job) => {
                    job.cancel();
                    self.midi_out.retiring.push(job);
                }
                Err(_) => self.midi_out.retirement_failed = true,
            }
        }

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
        self.midi_out.retiring.retain(|job| !job.finished());
        if self.windowed {
            if self.midi_out.retirement_failed || !self.midi_out.retiring.is_empty() {
                return;
            }
            if self
                .midi_out
                .connection_end
                .as_ref()
                .is_some_and(|done| !done.load(Ordering::Acquire))
                && (want != self.midi_out.open || self.midi_out.detaching)
            {
                if !self.midi_out.detaching && self.send(Command::SetMidiOutput(None)) {
                    self.midi_out.detaching = true;
                }
                return;
            }
            if self.midi_out.detaching {
                self.midi_out.connection_end = None;
                self.midi_out.open.clear();
                self.midi_out.detaching = false;
            }
        }
        let mut prepared = None;
        if let Some(opening) = &self.midi_out.opening {
            if let Some(result) = opening.work.poll() {
                let opening = self.midi_out.opening.take().unwrap();
                match result {
                    Ok((maker, result)) => {
                        let current = opening.generation == self.midi_out.generation;
                        let previous = if current {
                            self.midi_out.maker = Some(maker);
                            None
                        } else {
                            Some(maker)
                        };
                        if current && opening.want == want {
                            prepared = Some(result);
                        } else {
                            let retired =
                                crate::background::Job::spawn("stale-midi-output", move || {
                                    if let Ok(out) = result {
                                        out.close();
                                    }
                                    drop(previous);
                                    Ok(())
                                });
                            match retired {
                                Ok(job) => {
                                    job.cancel();
                                    self.midi_out.retiring.push(job);
                                }
                                Err(error) => {
                                    self.midi_out.retirement_failed = true;
                                    report.warnings.push(error);
                                }
                            }
                            return;
                        }
                    }
                    Err(error) => report.warnings.push(error),
                }
            } else if !want.is_empty() {
                return;
            }
        }
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
        let made = if let Some(result) = prepared {
            result
        } else if self.windowed {
            if let Some(mut maker) = self.midi_out.maker.take() {
                let ports = names.clone();
                match crate::background::Job::spawn("midi-output-open", move || {
                    let result = maker(&ports, rate);
                    Ok((maker, result))
                }) {
                    Ok(work) => {
                        self.midi_out.opening = Some(Opening {
                            generation: self.midi_out.generation,
                            want,
                            work,
                        })
                    }
                    Err(error) => report.warnings.push(error),
                }
                return;
            }
            Err(format!("MIDI 出力を開けません: {}", names.join("、")))
        } else {
            match &mut self.midi_out.maker {
                Some(maker) => maker(&names, rate),
                None => Err(format!("MIDI 出力を開けません: {}", names.join("、"))),
            }
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
                let done = Arc::new(AtomicBool::new(false));
                let out = if self.windowed {
                    Box::new(Tracked {
                        inner: Some(out),
                        done: done.clone(),
                    }) as Box<dyn MidiOutput>
                } else {
                    out
                };
                if self.send(Command::SetMidiOutput(Some(out))) {
                    self.midi_out.connection_end = self.windowed.then_some(done);
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
