//! What differs between the plugin formats the host runs (VST3 and CLAP, PLG-028). The rest
//! of the host (the messages, the shared memory, crash marking) is the same for both.
//!
//! A CLAP plugin is named on the wire like a VST3 class: the bundle's path, and as the class
//! the 16 bytes of [`xirius_clap::class_key`] of its id. Its state travels as
//! [`PluginState::component`] with an empty controller part.

use xirius_engine::wire::Gesture;
use xirius_vst3::{
    AudioPart, Caption, Control, Editor, Note, ParamChange, Placement, Plugin, PluginState,
};

/// `kParamValuesChanged`: the plugin changed many values at once (a preset in its editor).
const PARAM_VALUES_CHANGED: i32 = 4;
/// `kLatencyChanged` (ENG-011).
const LATENCY_CHANGED: i32 = 8;

/// A plugin's main-thread side.
pub enum Loaded {
    Vst3(Plugin),
    Clap(Box<xirius_clap::Plugin>),
}

/// What a plugin asked for since the last look.
#[derive(Default)]
pub struct Changes {
    /// Its values changed without the user's gestures (a preset), so its state is worth
    /// reading.
    pub values: bool,
    pub latency: bool,
    /// It said the names of its keys changed.
    pub note_names: bool,
}

impl Loaded {
    pub fn set_state(&self, state: &PluginState) -> Result<(), String> {
        match self {
            Loaded::Vst3(p) => p.set_state(state).map_err(|e| e.to_string()),
            Loaded::Clap(p) => p.set_state(&state.component).map_err(|e| e.to_string()),
        }
    }

    pub fn get_state(&self) -> Result<PluginState, String> {
        match self {
            Loaded::Vst3(p) => p.get_state().map_err(|e| e.to_string()),
            Loaded::Clap(p) => p
                .get_state()
                .map(|component| PluginState {
                    component,
                    controller: Vec::new(),
                })
                .map_err(|e| e.to_string()),
        }
    }

    pub fn take_gestures(&self) -> Vec<Gesture> {
        match self {
            Loaded::Vst3(p) => {
                // Polled regularly: the values the processor reported reach the controller too.
                p.follow_processor();
                p.handler()
                    .take_gestures()
                    .into_iter()
                    .map(Gesture::from)
                    .collect()
            }
            Loaded::Clap(p) => p
                .take_gestures()
                .into_iter()
                .map(|g| match g {
                    xirius_clap::Gesture::Begin(id) => Gesture::Begin(id),
                    xirius_clap::Gesture::Perform(id, v) => Gesture::Perform(id, v),
                    xirius_clap::Gesture::End(id) => Gesture::End(id),
                })
                .collect(),
        }
    }

    pub fn take_changes(&self) -> Changes {
        match self {
            Loaded::Vst3(p) => {
                let flags = p.handler().take_restart_flags();
                Changes {
                    values: flags & PARAM_VALUES_CHANGED != 0,
                    latency: flags & LATENCY_CHANGED != 0,
                    // A VST3 plugin says nothing of it: the names are read again with its values.
                    note_names: false,
                }
            }
            Loaded::Clap(p) => {
                // A restart request is not followed: the engine's block size and rate have
                // not changed, and the processor is reactivated with the next `Open`.
                p.take_restart();
                let rescan = p.take_params_changed();
                Changes {
                    values: p.take_dirty() || rescan,
                    latency: p.take_latency_changed(),
                    note_names: p.take_note_names_changed(),
                }
            }
        }
    }

    pub fn latency(&self) -> u32 {
        match self {
            Loaded::Vst3(p) => p.latency(),
            Loaded::Clap(p) => p.latency(),
        }
    }

    /// The names the plugin gives its keys, by key (EDT-134).
    pub fn note_names(&self) -> Vec<(u8, String)> {
        match self {
            Loaded::Vst3(p) => p.note_names(),
            Loaded::Clap(p) => p.note_names(),
        }
    }

    /// Every parameter's id and title.
    pub fn params(&self) -> Vec<(u32, String)> {
        match self {
            Loaded::Vst3(p) => p.params().into_iter().map(|p| (p.id, p.title)).collect(),
            Loaded::Clap(p) => p.params().iter().map(|p| (p.id, p.title.clone())).collect(),
        }
    }

    /// A parameter by numeric id, else by title (case-insensitive), as the song names it.
    pub fn resolve(&self, key: &str) -> Option<u32> {
        let params = self.params();
        key.parse::<u32>()
            .ok()
            .filter(|id| params.iter().any(|(p, _)| p == id))
            .or_else(|| {
                params
                    .iter()
                    .find(|(_, title)| title.eq_ignore_ascii_case(key))
                    .map(|(id, _)| *id)
            })
    }

    pub fn set_param(&self, id: u32, normalized: f64) {
        match self {
            Loaded::Vst3(p) => {
                p.set_param(id, normalized);
            }
            Loaded::Clap(p) => {
                p.set_param(id, normalized);
            }
        }
    }

    pub fn edit_like_editor(&self, id: u32, values: &[f64]) {
        match self {
            Loaded::Vst3(p) => p.edit_like_editor(id, values),
            Loaded::Clap(p) => p.edit_like_editor(id, values),
        }
    }

    pub fn open_editor(&self, title: &str, caption: Option<Caption>) -> Result<Editor, String> {
        match self {
            Loaded::Vst3(p) => p
                .open_editor_at(
                    title,
                    Placement {
                        caption,
                        ..Placement::shown(None)
                    },
                )
                .map_err(|e| e.to_string()),
            Loaded::Clap(_) => Err("CLAP のエディターはまだ開けません".to_string()),
        }
    }
}

/// A plugin's real-time side, fed the engine's blocks.
pub enum Audio {
    Vst3(AudioPart),
    Clap(ClapBlock),
}

impl Audio {
    pub fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        notes: &[Note],
        controls: &[Control],
        params: &[ParamChange],
    ) {
        match self {
            Audio::Vst3(a) => a.process_with_params(left, right, notes, controls, params),
            Audio::Clap(a) => a.process(left, right, notes, controls, params),
        }
    }
}

impl Loaded {
    /// The real-time side, prepared for the engine's blocks, and its latency.
    pub fn audio(
        &mut self,
        sample_rate: f64,
        max_block: usize,
        offline: bool,
        events: usize,
    ) -> Result<(Audio, u32), String> {
        match self {
            Loaded::Vst3(p) => {
                let mut a = p.audio().map_err(|e| e.to_string())?;
                let latency = a
                    .prepare(sample_rate, max_block, offline)
                    .map_err(|e| e.to_string())?;
                Ok((Audio::Vst3(a), latency))
            }
            Loaded::Clap(p) => {
                let mut a = p.audio().map_err(|e| e.to_string())?;
                let latency = a
                    .prepare(sample_rate, max_block, offline)
                    .map_err(|e| e.to_string())?;
                Ok((Audio::Clap(ClapBlock::new(a, events)), latency))
            }
        }
    }
}

/// A CLAP processor with room for one block's events in its own types, so a block does not
/// allocate.
pub struct ClapBlock {
    part: xirius_clap::AudioPart,
    notes: Vec<xirius_clap::Note>,
    controls: Vec<xirius_clap::Control>,
    params: Vec<xirius_clap::ParamChange>,
}

impl ClapBlock {
    fn new(part: xirius_clap::AudioPart, capacity: usize) -> ClapBlock {
        ClapBlock {
            part,
            notes: Vec::with_capacity(capacity),
            controls: Vec::with_capacity(capacity),
            params: Vec::with_capacity(capacity),
        }
    }

    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        notes: &[Note],
        controls: &[Control],
        params: &[ParamChange],
    ) {
        let room = self.notes.capacity();
        self.notes.clear();
        self.controls.clear();
        self.params.clear();
        self.notes
            .extend(notes.iter().take(room).map(|n| xirius_clap::Note {
                offset: n.offset,
                pitch: n.pitch,
                velocity: n.velocity,
                on: n.on,
            }));
        self.controls
            .extend(controls.iter().take(room).map(|c| xirius_clap::Control {
                offset: c.offset,
                number: c.number,
                value: c.value,
            }));
        self.params
            .extend(params.iter().take(room).map(|p| xirius_clap::ParamChange {
                offset: p.offset,
                id: p.id,
                value: p.value,
            }));
        self.part
            .process_with_params(left, right, &self.notes, &self.controls, &self.params);
    }
}
