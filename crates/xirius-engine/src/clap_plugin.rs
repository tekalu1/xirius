//! A CLAP plugin as a [`Processor`] (PLG-028), the counterpart of
//! [`Vst3Processor`](crate::plugin::Vst3Processor). The plugin's main-thread side stays with
//! the caller; the plan only owns its real-time half.

use xirius_clap::{AudioPart, Control, Note};

use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor, RenderMode};

/// Notes one block can carry to the plugin; the plugin's event list drops more too.
const MAX_NOTES: usize = 512;
/// Controller changes one block can carry to the plugin.
const MAX_CONTROLS: usize = 256;

pub struct ClapProcessor {
    name: String,
    part: AudioPart,
    notes: Vec<Note>,
    controls: Vec<Control>,
    latency: u32,
    /// Why `prepare` failed. The processor then passes audio through untouched.
    error: Option<String>,
}

impl ClapProcessor {
    pub fn new(part: AudioPart) -> ClapProcessor {
        ClapProcessor {
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

impl Processor for ClapProcessor {
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
            .process_with_params(io.left, io.right, &self.notes, &self.controls, &[]);
    }

    fn latency(&self) -> u32 {
        self.latency
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use xirius_clap::{Bundle, Plugin};

    use super::*;

    fn fixture() -> Plugin {
        xirius_test_clap::fixture_marker();
        let deps: PathBuf = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let bundle = Bundle::load(&deps.join("xirius_test_clap.dll")).expect("load");
        Plugin::new(&bundle, xirius_test_clap::ID).expect("create")
    }

    #[test]
    fn a_clap_plugin_runs_as_a_processor() {
        let mut plugin = fixture();
        plugin.set_param(xirius_test_clap::GAIN, 0.5);
        plugin.set_param(xirius_test_clap::LATENCY, 3.0 / 4096.0);
        let mut p = ClapProcessor::new(plugin.audio().expect("audio"));
        assert_eq!(p.name(), xirius_test_clap::NAME);
        p.prepare(&ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 32,
            mode: RenderMode::Offline,
        });
        assert_eq!(p.error(), None);
        assert_eq!(p.latency(), 3);
        let (mut l, mut r) = (vec![1.0f32; 32], vec![1.0f32; 32]);
        let on = NoteEvent {
            offset: 8,
            kind: NoteKind::On {
                pitch: 60,
                velocity: 1.0,
            },
        };
        p.process(
            &mut AudioIo {
                left: &mut l,
                right: &mut r,
            },
            &[on],
        );
        assert!(l[..8].iter().all(|&v| v == 0.5), "{l:?}");
        assert!(l[8..].iter().all(|&v| v == 0.75), "{l:?}");
        assert_eq!(l, r);
    }
}
