//! A small CLAP plugin for the host's tests (PLG-028, PLG-042). No window, no device.
//!
//! - One stereo in and out port: the input times `Gain` (parameter 1).
//! - `Latency` (parameter 2, whole samples) is the latency it reports; nothing is delayed.
//! - A note input: while a key is held, `0.25 × velocity` is added to both channels from the
//!   note's sample on, so a test sees where a note landed.
//! - Key [`EDIT_KEY`] acts as if the user moved `Gain` to 0.5 in an editor: the plugin sends
//!   the gesture and the value to the host.
//! - The state is `XTCL`, the gain (f32) and the latency (u32), little-endian. Anything else
//!   is refused. After loading it the plugin asks the host to read the values again.
//! - It names some keys (`clap.note-name`, [`NOTE_NAMES`]); the kick is [`SOFT_KICK`] while the
//!   gain is below 0.5, and after a state is loaded it tells the host the names changed. Names
//!   for another port or for every key are there too, for a host to leave out.
//! - Each parameter has a cookie, and a value event without it is ignored, as plugins built
//!   with clap-juce-extensions read the cookie without checking it (Vital crashed on a null one).

use std::ffi::CStr;
use std::fmt::Write as _;
use std::io::{Read, Write as _};
use std::sync::atomic::{AtomicU32, Ordering};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::latency::{PluginLatency, PluginLatencyImpl};
use clack_extensions::note_name::{
    HostNoteName, NoteName, NoteNameWriter, PluginNoteName, PluginNoteNameImpl,
};
use clack_extensions::note_ports::{
    NoteDialect, NoteDialects, NotePortInfo, NotePortInfoWriter, PluginNotePorts,
    PluginNotePortsImpl,
};
use clack_extensions::params::{
    HostParams, ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, ParamRescanFlags,
    PluginAudioProcessorParams, PluginMainThreadParams, PluginParams,
};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::events::event_types::{
    ParamGestureBeginEvent, ParamGestureEndEvent, ParamValueEvent,
};
use clack_plugin::events::spaces::CoreEventSpace;
use clack_plugin::events::{Match, Pckn};
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};
use clack_plugin::utils::Cookie;

pub const ID: &str = "com.xirius.test-gain";
pub const NAME: &str = "Xirius Test Gain";
pub const GAIN: u32 = 1;
pub const LATENCY: u32 = 2;
pub const MAX_LATENCY: u32 = 4096;
/// The key that acts as an edit in the editor.
pub const EDIT_KEY: u16 = 127;
/// What that edit sets `Gain` to.
pub const EDIT_GAIN: f64 = 0.5;
/// The keys it names on its note port, and what it calls them.
pub const NOTE_NAMES: [(u8, &str); 3] = [
    (36, "Fixture Kick"),
    (38, "Fixture Snare"),
    (42, "Fixture Hat"),
];
/// The kick's name while the gain is below 0.5.
pub const SOFT_KICK: &str = "Fixture Kick Soft";
const MAGIC: &[u8; 4] = b"XTCL";
/// What the parameters' cookies point at: one byte each for `Gain` and `Latency`.
static COOKIES: [u8; 2] = [0; 2];

fn cookie(id: ClapId) -> Cookie {
    let i = if id == GAIN { 0 } else { 1 };
    Cookie::from_raw(std::ptr::from_ref(&COOKIES[i]).cast_mut().cast())
}

/// Referenced by tests so the cdylib is built next to them.
pub fn fixture_marker() {}

struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

pub struct TestShared {
    gain: AtomicF32,
    latency: AtomicU32,
}

impl TestShared {
    fn set(&self, id: ClapId, value: f64) {
        if id == GAIN {
            self.gain.set(value.clamp(0.0, 1.0) as f32);
        } else if id == LATENCY {
            self.latency.store(
                value.clamp(0.0, MAX_LATENCY as f64).round() as u32,
                Ordering::Relaxed,
            );
        }
    }

    fn get(&self, id: ClapId) -> Option<f64> {
        if id == GAIN {
            Some(self.gain.get() as f64)
        } else if id == LATENCY {
            Some(self.latency.load(Ordering::Relaxed) as f64)
        } else {
            None
        }
    }

    fn handle(&self, event: &UnknownEvent) {
        if let Some(CoreEventSpace::ParamValue(e)) = event.as_core_event()
            && let Some(id) = e.param_id()
            && e.cookie() == cookie(id)
        {
            self.set(id, e.value());
        }
    }
}

impl PluginShared<'_> for TestShared {}

pub struct TestMainThread<'a> {
    shared: &'a TestShared,
    host: HostMainThreadHandle<'a>,
}

impl<'a> PluginMainThread<'a, TestShared> for TestMainThread<'a> {}

pub struct TestPlugin;

impl Plugin for TestPlugin {
    type AudioProcessor<'a> = TestProcessor<'a>;
    type Shared<'a> = TestShared;
    type MainThread<'a> = TestMainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&TestShared>) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginNotePorts>()
            .register::<PluginParams>()
            .register::<PluginState>()
            .register::<PluginLatency>()
            .register::<PluginNoteName>();
    }
}

impl DefaultPluginFactory for TestPlugin {
    fn get_descriptor() -> PluginDescriptor {
        use clack_plugin::plugin::features::{AUDIO_EFFECT, STEREO};
        PluginDescriptor::new(ID, NAME)
            .with_vendor("Xirius")
            .with_version("1.0.0")
            .with_features([AUDIO_EFFECT, STEREO])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<TestShared, PluginError> {
        Ok(TestShared {
            gain: AtomicF32::new(1.0),
            latency: AtomicU32::new(0),
        })
    }

    fn new_main_thread<'a>(
        host: HostMainThreadHandle<'a>,
        shared: &'a TestShared,
    ) -> Result<TestMainThread<'a>, PluginError> {
        Ok(TestMainThread { shared, host })
    }
}

pub struct TestProcessor<'a> {
    shared: &'a TestShared,
    /// The velocity of each held key.
    held: [f32; 128],
}

impl<'a> PluginAudioProcessor<'a, TestShared, TestMainThread<'a>> for TestProcessor<'a> {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &TestMainThread<'a>,
        shared: &'a TestShared,
        _audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(TestProcessor {
            shared,
            held: [0.0; 128],
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        let mut pair = audio
            .port_pair(0)
            .ok_or(PluginError::Message("no audio port"))?;
        let mut channels = pair
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("not f32"))?;
        let mut bufs = [None, None];
        for (pair, buf) in channels.iter_mut().zip(&mut bufs) {
            *buf = match pair {
                ChannelPair::InPlace(b) => Some(b),
                ChannelPair::InputOutput(i, o) => {
                    o.copy_from_slice(i);
                    Some(o)
                }
                ChannelPair::OutputOnly(o) => {
                    o.fill(0.0);
                    Some(o)
                }
                ChannelPair::InputOnly(_) => None,
            };
        }
        let mut edited = false;
        for batch in events.input.batch() {
            for event in batch.events() {
                self.shared.handle(event);
                match event.as_core_event() {
                    Some(CoreEventSpace::NoteOn(e)) => match e.pckn().key {
                        Match::Specific(EDIT_KEY) => edited = true,
                        Match::Specific(k) if k < 128 => {
                            self.held[usize::from(k)] = e.velocity() as f32;
                        }
                        _ => {}
                    },
                    Some(CoreEventSpace::NoteOff(e)) => match e.pckn().key {
                        Match::Specific(k) if k < 128 => self.held[usize::from(k)] = 0.0,
                        Match::All => self.held = [0.0; 128],
                        _ => {}
                    },
                    _ => {}
                }
            }
            let gain = self.shared.gain.get();
            let add = 0.25 * self.held.iter().sum::<f32>();
            for buf in bufs.iter_mut().flatten() {
                let len = buf.len();
                let start = batch.first_sample().min(len);
                let end = batch.next_batch_first_sample().unwrap_or(len).min(len);
                for x in &mut buf[start..end] {
                    *x = *x * gain + add;
                }
            }
        }
        if edited {
            let id = ClapId::new(GAIN);
            let out = events.output;
            let _ = out.try_push(ParamGestureBeginEvent::new(0, id));
            let _ = out.try_push(ParamValueEvent::new(0, id, Pckn::match_all(), EDIT_GAIN));
            let _ = out.try_push(ParamGestureEndEvent::new(0, id));
            self.shared.set(id, EDIT_GAIN);
        }
        Ok(ProcessStatus::ContinueIfNotQuiet)
    }
}

impl PluginAudioProcessorParams for TestProcessor<'_> {
    fn flush(&mut self, input: &InputEvents, _output: &mut OutputEvents) {
        for event in input {
            self.shared.handle(event);
        }
    }
}

impl PluginAudioPortsImpl for TestMainThread<'_> {
    fn count(&self, _is_input: bool) -> u32 {
        1
    }

    fn get(&self, index: u32, _is_input: bool, writer: &mut AudioPortInfoWriter) {
        if index == 0 {
            writer.set(&AudioPortInfo {
                id: ClapId::new(0),
                name: b"main",
                channel_count: 2,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: None,
            });
        }
    }
}

impl PluginNotePortsImpl for TestMainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        u32::from(is_input)
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut NotePortInfoWriter) {
        if is_input && index == 0 {
            writer.set(&NotePortInfo {
                id: ClapId::new(0),
                name: b"notes",
                supported_dialects: NoteDialects::CLAP,
                preferred_dialect: Some(NoteDialect::Clap),
            });
        }
    }
}

impl PluginLatencyImpl for TestMainThread<'_> {
    fn get(&self) -> u32 {
        self.shared.latency.load(Ordering::Relaxed)
    }
}

impl PluginStateImpl for TestMainThread<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        output.write_all(MAGIC)?;
        output.write_all(&self.shared.gain.get().to_le_bytes())?;
        output.write_all(&self.shared.latency.load(Ordering::Relaxed).to_le_bytes())?;
        Ok(())
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        let mut buf = [0u8; 12];
        input.read_exact(&mut buf)?;
        if &buf[..4] != MAGIC {
            return Err(PluginError::Message("not this plugin's state"));
        }
        let gain = f32::from_le_bytes(buf[4..8].try_into().unwrap_or_default());
        let latency = u32::from_le_bytes(buf[8..12].try_into().unwrap_or_default());
        self.shared.set(ClapId::new(GAIN), gain as f64);
        self.shared.set(ClapId::new(LATENCY), latency as f64);
        // The values changed under the host: it reads them again (clap-validator checks this).
        if let Some(params) = self.host.get_extension::<HostParams>() {
            params.rescan(&self.host, ParamRescanFlags::VALUES);
        }
        if let Some(names) = self.host.get_extension::<HostNoteName>() {
            names.changed(&self.host);
        }
        Ok(())
    }
}

impl PluginMainThreadParams for TestMainThread<'_> {
    fn count(&self) -> u32 {
        2
    }

    fn get_info(&self, index: u32, info: &mut ParamInfoWriter) {
        let (id, name, max, default, flags): (u32, &[u8], f64, f64, ParamInfoFlags) = match index {
            0 => (GAIN, b"Gain", 1.0, 1.0, ParamInfoFlags::IS_AUTOMATABLE),
            1 => (
                LATENCY,
                b"Latency",
                MAX_LATENCY as f64,
                0.0,
                ParamInfoFlags::IS_STEPPED,
            ),
            _ => return,
        };
        info.set(&ParamInfo {
            id: ClapId::new(id),
            flags,
            cookie: cookie(ClapId::new(id)),
            name,
            module: b"",
            min_value: 0.0,
            max_value: max,
            default_value: default,
        });
    }

    fn get_value(&self, id: ClapId) -> Option<f64> {
        self.shared.get(id)
    }

    fn value_to_text(
        &self,
        id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        if id == GAIN {
            write!(writer, "{:.1} %", value * 100.0)
        } else if id == LATENCY {
            write!(writer, "{value:.0} smp")
        } else {
            Err(std::fmt::Error)
        }
    }

    fn text_to_value(&self, id: ClapId, text: &CStr) -> Option<f64> {
        let text = text.to_str().ok()?.trim();
        if id == GAIN {
            let v: f64 = text.trim_end_matches('%').trim().parse().ok()?;
            Some(v / 100.0)
        } else if id == LATENCY {
            text.trim_end_matches("smp").trim().parse().ok()
        } else {
            None
        }
    }

    fn flush(&self, input: &InputEvents, _output: &mut OutputEvents) {
        for event in input {
            self.shared.handle(event);
        }
    }
}

impl PluginNoteNameImpl for TestMainThread<'_> {
    /// Its own names, then one for a second port and one for every key.
    fn count(&self) -> u32 {
        NOTE_NAMES.len() as u32 + 2
    }

    fn get(&self, index: u32, writer: &mut NoteNameWriter) {
        let i = index as usize;
        let (name, port, key) = match NOTE_NAMES.get(i) {
            Some(&(key, name)) => {
                let soft = key == NOTE_NAMES[0].0 && self.shared.gain.get() < 0.5;
                (
                    if soft { SOFT_KICK } else { name },
                    Match::Specific(0),
                    Match::Specific(u16::from(key)),
                )
            }
            None if i == NOTE_NAMES.len() => {
                ("Other Port", Match::Specific(1), Match::Specific(40))
            }
            None if i == NOTE_NAMES.len() + 1 => ("Every Key", Match::All, Match::All),
            None => return,
        };
        writer.write(&NoteName {
            name: name.as_bytes(),
            port,
            channel: Match::All,
            key,
        });
    }
}

clack_export_entry!(SinglePluginEntry<TestPlugin>);
