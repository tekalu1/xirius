//! A VST3 plugin as a [`Processor`] (ARC-011). The plugin's UI side stays with the caller;
//! the plan only owns its real-time half.

use xirius_vst3::{AudioPart, Control, Note};

use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor, RenderMode};

/// Notes one block can carry to the plugin; more are dropped by the plugin's event list too.
const MAX_NOTES: usize = 512;
/// Controller changes one block can carry to the plugin.
const MAX_CONTROLS: usize = 256;

pub struct Vst3Processor {
    name: String,
    part: AudioPart,
    notes: Vec<Note>,
    controls: Vec<Control>,
    latency: u32,
    /// Why `prepare` failed. The processor then passes audio through untouched.
    error: Option<String>,
}

impl Vst3Processor {
    pub fn new(part: AudioPart) -> Vst3Processor {
        Vst3Processor {
            name: part.name().to_string(),
            part,
            notes: Vec::with_capacity(MAX_NOTES),
            controls: Vec::with_capacity(MAX_CONTROLS),
            latency: 0,
            error: None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl Processor for Vst3Processor {
    fn name(&self) -> &str {
        &self.name
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        let offline = setup.mode == RenderMode::Offline;
        match self
            .part
            .prepare(setup.sample_rate, setup.max_block, offline)
        {
            Ok(latency) => {
                self.latency = latency;
                self.error = None;
            }
            Err(e) => {
                self.latency = 0;
                self.error = Some(e.to_string());
            }
        }
    }

    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        if self.error.is_some() {
            return;
        }
        self.notes.clear();
        self.controls.clear();
        for e in events {
            match e.kind {
                NoteKind::On { pitch, velocity } if self.notes.len() < MAX_NOTES => {
                    self.notes.push(Note {
                        offset: e.offset,
                        pitch,
                        velocity,
                        on: true,
                    });
                }
                NoteKind::Off { pitch } if self.notes.len() < MAX_NOTES => {
                    self.notes.push(Note {
                        offset: e.offset,
                        pitch,
                        velocity: 0.0,
                        on: false,
                    });
                }
                NoteKind::Control { number, value } if self.controls.len() < MAX_CONTROLS => {
                    self.controls.push(Control {
                        offset: e.offset,
                        number,
                        value,
                    });
                }
                _ => {}
            }
        }
        self.part
            .process_with(io.left, io.right, &self.notes, &self.controls);
    }

    fn latency(&self) -> u32 {
        self.latency
    }
}
