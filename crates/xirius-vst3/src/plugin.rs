//! One plugin instance: the component with its processor, and its edit controller.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::sync::Arc;

use vst3::Steinberg::Vst::BusDirections_::{kInput, kOutput};
use vst3::Steinberg::Vst::Event_::EventTypes_::{
    kLegacyMIDICCOutEvent, kNoteOffEvent, kNoteOnEvent,
};
use vst3::Steinberg::Vst::MediaTypes_::{kAudio, kEvent};
use vst3::Steinberg::Vst::ProcessContext_::StatesAndFlags_::{
    kContTimeValid, kPlaying, kProjectTimeMusicValid, kTempoValid, kTimeSigValid,
};
use vst3::Steinberg::Vst::ProcessModes_::{kOffline, kRealtime};
use vst3::Steinberg::Vst::SymbolicSampleSizes_::kSample32;
use vst3::Steinberg::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, BusInfo, Event, IAudioProcessor, IAudioProcessorTrait,
    IComponent, IComponentHandler, IComponentTrait, IConnectionPoint, IConnectionPointTrait,
    IEditController, IEditControllerTrait, IEventList, IHostApplication, IMidiMapping,
    IMidiMappingTrait, IParameterChanges, IUnitInfo, IUnitInfoTrait, LegacyMIDICCOutEvent,
    NoteOffEvent, NoteOnEvent, ParamID, ParamValue, ParameterInfo, ProcessContext, ProcessData,
    ProcessSetup, ProgramListInfo, SpeakerArr, SpeakerArrangement, String128, UnitInfo,
};
use vst3::Steinberg::{FUnknown, IBStream, IPluginBaseTrait, TUID, kResultOk};
use vst3::{ComPtr, ComWrapper};

use crate::host::{ComponentHandler, HostApplication};
use crate::module::{ClassInfo, Module};
use crate::process::{EventList, ParameterChanges};
use crate::serial::{SETUP_LIMIT, turn, turn_within};
use crate::stream::MemoryStream;
use crate::{Error, error, utf16_str};

/// Edits queued from the UI thread to the processor between two blocks.
const EDIT_QUEUE: usize = 1024;
/// Distinct parameters and points per parameter that one block can carry.
const MAX_PARAMS_PER_BLOCK: usize = 128;
const MAX_POINTS_PER_PARAM: usize = 8;

/// A note for the plugin's first event input, at a frame offset within the block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub offset: u32,
    pub pitch: u8,
    /// 0..=1. Ignored for note-offs.
    pub velocity: f32,
    pub on: bool,
}

/// A normalized parameter value at a frame offset in the input audio block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamChange {
    pub offset: u32,
    pub id: ParamID,
    pub value: ParamValue,
}

/// The controller number of channel aftertouch, and of pitch bend, as VST3 and MIDI 2 number them
/// (the numbers below 128 are the MIDI CCs).
pub const CTRL_AFTERTOUCH: u8 = 128;
pub const CTRL_PITCH_BEND: u8 = 129;
const CTRL_NUMBERS: usize = 130;

/// A controller change for the plugin, at a frame offset within the block: a MIDI CC (`number`
/// 0..=127, `value` 0..=127), channel aftertouch (`CTRL_AFTERTOUCH`, 0..=127) or pitch bend
/// (`CTRL_PITCH_BEND`, -8192..=8191). The plugin gets it as a parameter change when its
/// controller maps the number (`IMidiMapping`), else as a legacy MIDI CC event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Control {
    pub offset: u32,
    pub number: u8,
    pub value: i32,
}

impl Control {
    /// The value as the 0..1 a parameter takes.
    fn normalized(&self) -> f64 {
        if self.number == CTRL_PITCH_BEND {
            f64::from((self.value + 8192).clamp(0, 16383)) / 16383.0
        } else {
            f64::from(self.value.clamp(0, 127)) / 127.0
        }
    }
}

/// How the controller changes reached the plugin in total, for tests and the log.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ControlCounts {
    pub as_params: u64,
    pub as_events: u64,
    pub dropped: u64,
}

#[derive(Clone, Debug)]
pub struct ParamInfo {
    pub id: ParamID,
    pub title: String,
    pub units: String,
    pub step_count: i32,
    pub default: ParamValue,
    pub flags: i32,
}

/// Saved state: the component's and the controller's streams, as the SDK keeps them apart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginState {
    pub component: Vec<u8>,
    pub controller: Vec<u8>,
}

/// What both the UI side and the audio side hold. It is terminated and released when the last
/// one goes, during a turn.
pub(crate) struct Shared {
    pub(crate) class: ClassInfo,
    pub(crate) controller: Option<ComPtr<IEditController>>,
    component: ManuallyDrop<ComPtr<IComponent>>,
    processor: ManuallyDrop<ComPtr<IAudioProcessor>>,
    separate_controller: bool,
    connection: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    _host: ComWrapper<HostApplication>,
    // Retains this instance's module handle through interface teardown.
    _module: Arc<Module>,
}

impl Drop for Shared {
    fn drop(&mut self) {
        let _turn = turn("release an instance of", self._module.path());
        // SAFETY: teardown in the SDK's order, on objects this struct created. The interfaces
        // are released here, during the turn; the module goes after, with the fields.
        unsafe {
            if let Some((a, b)) = self.connection.take() {
                a.disconnect(b.as_ptr());
                b.disconnect(a.as_ptr());
            }
            if let Some(c) = &self.controller {
                c.setComponentHandler(std::ptr::null_mut());
                if self.separate_controller {
                    c.terminate();
                }
            }
            self.controller = None;
            self.component.terminate();
            ManuallyDrop::drop(&mut self.processor);
            ManuallyDrop::drop(&mut self.component);
        }
    }
}

/// The UI-thread side of a plugin. Not `Send`: the SDK expects these calls on one thread.
pub struct Plugin {
    shared: Arc<Shared>,
    handler: ComWrapper<ComponentHandler>,
    edits: Option<rtrb::Consumer<(ParamID, ParamValue)>>,
    /// The values the processor reported in its blocks (`outputParameterChanges`), for
    /// [`Plugin::follow_processor`]. A new one comes with each audio part.
    reported: RefCell<Option<rtrb::Consumer<(ParamID, ParamValue)>>>,
    _ui_thread: PhantomData<*const ()>,
}

fn stream_ptr(s: &ComWrapper<MemoryStream>) -> *mut IBStream {
    s.as_com_ref::<IBStream>().unwrap().as_ptr()
}

impl Plugin {
    /// Creates and initialises `class` from `module`, with its controller connected and
    /// synchronised to the component's state. Any thread may call it; setups across the
    /// process take turns.
    pub fn new(module: Arc<Module>, class: &ClassInfo) -> Result<Plugin, Error> {
        // Held to the end, so a failed setup is also released during the turn.
        let _turn = turn_within("create an instance of", module.path(), SETUP_LIMIT)?;
        let host = ComWrapper::new(HostApplication);
        let context = host.as_com_ref::<IHostApplication>().unwrap().as_ptr() as *mut FUnknown;
        module.set_host_context(context);
        let component = module
            .create::<IComponent>(&class.cid)
            .ok_or_else(|| error(format!("could not create {}", class.name)))?;
        // SAFETY: `context` stays alive in `Shared`.
        if unsafe { component.initialize(context) } != kResultOk {
            return Err(error("IComponent::initialize failed"));
        }
        let Some(processor) = component.cast::<IAudioProcessor>() else {
            // SAFETY: undoes the initialize above.
            unsafe { component.terminate() };
            return Err(error("the class is not an IAudioProcessor"));
        };
        let mut shared = Shared {
            class: class.clone(),
            controller: None,
            processor: ManuallyDrop::new(processor),
            component: ManuallyDrop::new(component),
            separate_controller: false,
            connection: None,
            _host: host,
            _module: module.clone(),
        };

        let mut ccid: TUID = [0; 16];
        // SAFETY: `ccid` is a 16-byte out-buffer.
        let has_cid = unsafe { shared.component.getControllerClassId(&mut ccid) } == kResultOk
            && ccid.iter().any(|&b| b != 0);
        if has_cid
            && let Some(c) = module.create::<IEditController>(&ccid.map(|b| b as u8))
            // SAFETY: as for the component.
            && unsafe { c.initialize(context) } == kResultOk
        {
            shared.controller = Some(c);
            shared.separate_controller = true;
        }
        if shared.controller.is_none() {
            // A single-component plugin implements the controller on the component itself.
            shared.controller = shared.component.cast::<IEditController>();
        }

        let (tx, rx) = rtrb::RingBuffer::new(EDIT_QUEUE);
        let handler = ComWrapper::new(ComponentHandler::new(tx));
        if let Some(c) = &shared.controller {
            let h = handler.as_com_ref::<IComponentHandler>().unwrap().as_ptr();
            // SAFETY: the controller adds its own reference to the handler.
            unsafe { c.setComponentHandler(h) };
            if shared.separate_controller
                && let (Some(a), Some(b)) = (
                    shared.component.cast::<IConnectionPoint>(),
                    c.cast::<IConnectionPoint>(),
                )
            {
                // SAFETY: both points stay alive in `shared.connection` until disconnected.
                unsafe {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                }
                shared.connection = Some((a, b));
            }
            let s = MemoryStream::empty();
            // SAFETY: the stream outlives both calls.
            unsafe {
                if shared.component.getState(stream_ptr(&s)) == kResultOk {
                    s.rewind();
                    c.setComponentState(stream_ptr(&s));
                }
            }
        }
        Ok(Plugin {
            shared: Arc::new(shared),
            handler,
            edits: Some(rx),
            reported: RefCell::new(None),
            _ui_thread: PhantomData,
        })
    }

    pub fn class(&self) -> &ClassInfo {
        &self.shared.class
    }

    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    pub fn has_controller(&self) -> bool {
        self.shared.controller.is_some()
    }

    pub fn handler(&self) -> &ComponentHandler {
        &self.handler
    }

    pub fn params(&self) -> Vec<ParamInfo> {
        let Some(c) = &self.shared.controller else {
            return Vec::new();
        };
        // SAFETY: plain queries with out-structs owned here.
        let n = unsafe { c.getParameterCount() };
        (0..n)
            .filter_map(|i| {
                let mut p: ParameterInfo = unsafe { std::mem::zeroed() };
                (unsafe { c.getParameterInfo(i, &mut p) } == kResultOk).then(|| ParamInfo {
                    id: p.id,
                    title: utf16_str(&p.title),
                    units: utf16_str(&p.units),
                    step_count: p.stepCount,
                    default: p.defaultNormalizedValue,
                    flags: p.flags,
                })
            })
            .collect()
    }

    /// Passes the values the processor reported since the last call on to the controller, as
    /// the SDK asks of a host: a plugin that changes its own parameters while it plays, or
    /// after a state was loaded into the processor, shows them in its editor and in
    /// [`Plugin::param`]. Returns how many values were passed. Call it from the UI thread
    /// regularly.
    pub fn follow_processor(&self) -> usize {
        let mut reported = self.reported.borrow_mut();
        let (Some(rx), Some(c)) = (reported.as_mut(), &self.shared.controller) else {
            return 0;
        };
        let mut n = 0;
        while let Ok((id, value)) = rx.pop() {
            // SAFETY: a plain setter, on the controller's thread.
            unsafe { c.setParamNormalized(id, value) };
            n += 1;
        }
        n
    }

    /// The controller's normalised value of `id`.
    pub fn param(&self, id: ParamID) -> Option<ParamValue> {
        let c = self.shared.controller.as_ref()?;
        // SAFETY: a plain query.
        Some(unsafe { c.getParamNormalized(id) })
    }

    /// The plugin's own text for a normalised value, e.g. "-6.0 dB".
    pub fn param_text(&self, id: ParamID, value: ParamValue) -> Option<String> {
        let c = self.shared.controller.as_ref()?;
        let mut s: String128 = [0; 128];
        // SAFETY: `s` is a String128 owned here.
        (unsafe { c.getParamStringByValue(id, value, &mut s) } == kResultOk).then(|| utf16_str(&s))
    }

    /// Sets a parameter on the controller and queues it for the processor's next block.
    pub fn set_param(&self, id: ParamID, value: ParamValue) -> bool {
        if let Some(c) = &self.shared.controller {
            // SAFETY: a plain setter.
            unsafe { c.setParamNormalized(id, value) };
        }
        self.handler.push_edit(id, value)
    }

    /// Moves a parameter the way the plugin's editor does: `beginEdit`, a `performEdit` for
    /// each value, `endEdit`, with the controller following each value. For tests that need an
    /// editor's gesture without a window.
    pub fn edit_like_editor(&self, id: ParamID, values: &[ParamValue]) {
        use vst3::Steinberg::Vst::IComponentHandlerTrait;
        // SAFETY: the handler's methods only record and queue; the setter is plain.
        unsafe {
            self.handler.beginEdit(id);
            for &v in values {
                if let Some(c) = &self.shared.controller {
                    c.setParamNormalized(id, v);
                }
                self.handler.performEdit(id, v);
            }
            self.handler.endEdit(id);
        }
    }

    pub fn latency(&self) -> u32 {
        // SAFETY: a plain query.
        unsafe { self.shared.processor.getLatencySamples() }
    }

    pub fn get_state(&self) -> Result<PluginState, Error> {
        let comp = MemoryStream::empty();
        // SAFETY: the stream outlives the call.
        if unsafe { self.shared.component.getState(stream_ptr(&comp)) } != kResultOk {
            return Err(error("IComponent::getState failed"));
        }
        let mut state = PluginState {
            component: comp.bytes(),
            controller: Vec::new(),
        };
        if let Some(c) = &self.shared.controller {
            let s = MemoryStream::empty();
            // SAFETY: as above. Controllers without state of their own may refuse.
            if unsafe { c.getState(stream_ptr(&s)) } == kResultOk {
                state.controller = s.bytes();
            }
        }
        Ok(state)
    }

    pub fn set_state(&self, state: &PluginState) -> Result<(), Error> {
        let comp = MemoryStream::with_bytes(state.component.clone());
        // SAFETY: the streams outlive the calls.
        unsafe {
            if self.shared.component.setState(stream_ptr(&comp)) != kResultOk {
                return Err(error("IComponent::setState failed"));
            }
            if let Some(c) = &self.shared.controller {
                comp.rewind();
                c.setComponentState(stream_ptr(&comp));
                if !state.controller.is_empty() {
                    let s = MemoryStream::with_bytes(state.controller.clone());
                    c.setState(stream_ptr(&s));
                }
            }
        }
        Ok(())
    }

    /// The real-time side. There is one per plugin; it receives the edits made here and in the
    /// editor.
    /// A way to move parameters on the processor that outlives this side of the plugin and
    /// can be used from any thread (automation while the song plays, AUT-002).
    pub fn param_sender(&self) -> ParamSender {
        ParamSender {
            handler: self.handler.clone(),
            params: Arc::new(self.params().into_iter().map(|p| (p.id, p.title)).collect()),
        }
    }

    pub fn audio(&mut self) -> Result<AudioPart, Error> {
        let edits = self
            .edits
            .take()
            .ok_or_else(|| error("the audio part was already taken"))?;
        let (tx, rx) = rtrb::RingBuffer::new(EDIT_QUEUE);
        *self.reported.borrow_mut() = Some(rx);
        let mut part = AudioPart::new(self.shared.clone(), edits, tx);
        part.midi_map = self.midi_map();
        Ok(part)
    }

    /// The parameter the controller maps each controller number to (`IMidiMapping`, bus 0,
    /// channel 0), if it maps it at all.
    pub fn midi_map(&self) -> Box<[Option<ParamID>; CTRL_NUMBERS]> {
        let mut map = Box::new([None; CTRL_NUMBERS]);
        let Some(m) = self
            .shared
            .controller
            .as_ref()
            .and_then(|c| c.cast::<IMidiMapping>())
        else {
            return map;
        };
        for (number, slot) in map.iter_mut().enumerate() {
            let mut id: ParamID = 0;
            // SAFETY: a plain query with an out-value owned here.
            if unsafe { m.getMidiControllerAssignment(0, 0, number as i16, &mut id) } == kResultOk {
                *slot = Some(id);
            }
        }
        map
    }

    /// The names the plugin gives its keys (`IUnitInfo` program pitch names), sorted by key:
    /// those of the program the root unit has selected, or of the first program list when the
    /// root unit has none. Empty when the plugin names no keys.
    pub fn note_names(&self) -> Vec<(u8, String)> {
        let Some(c) = &self.shared.controller else {
            return Vec::new();
        };
        let Some(units) = c.cast::<IUnitInfo>() else {
            return Vec::new();
        };
        // SAFETY: plain queries with out-structs owned here.
        let root = (0..unsafe { units.getUnitCount() }).find_map(|i| {
            let mut u: UnitInfo = unsafe { std::mem::zeroed() };
            (unsafe { units.getUnitInfo(i, &mut u) } == kResultOk && u.id == K_ROOT_UNIT)
                .then_some(u.programListId)
        });
        let list = match root {
            Some(id) if id != K_NO_PROGRAM_LIST => id,
            _ => {
                let mut l: ProgramListInfo = unsafe { std::mem::zeroed() };
                if unsafe { units.getProgramListCount() } < 1
                    || unsafe { units.getProgramListInfo(0, &mut l) } != kResultOk
                {
                    return Vec::new();
                }
                l.id
            }
        };
        let program = self.selected_program(list);
        // SAFETY: as above.
        if unsafe { units.hasProgramPitchNames(list, program) } != kResultOk {
            return Vec::new();
        }
        (0..=127u8)
            .filter_map(|key| {
                let mut name: String128 = [0; 128];
                if unsafe { units.getProgramPitchName(list, program, i16::from(key), &mut name) }
                    != kResultOk
                {
                    return None;
                }
                let name = utf16_str(&name).trim().to_owned();
                (!name.is_empty()).then_some((key, name))
            })
            .collect()
    }

    /// The program selected in `list`, read from the plugin's program-change parameter; 0 when
    /// it has none.
    fn selected_program(&self, list: i32) -> i32 {
        let Some(c) = &self.shared.controller else {
            return 0;
        };
        let Some(p) = self
            .params()
            .into_iter()
            .find(|p| p.flags & K_IS_PROGRAM_CHANGE != 0)
        else {
            return 0;
        };
        let units = c.cast::<IUnitInfo>();
        let mut info: ProgramListInfo = unsafe { std::mem::zeroed() };
        let count = units.and_then(|u| {
            (0..unsafe { u.getProgramListCount() }).find_map(|i| {
                (unsafe { u.getProgramListInfo(i, &mut info) } == kResultOk && info.id == list)
                    .then_some(info.programCount)
            })
        });
        let Some(count) = count.filter(|&n| n > 0) else {
            return 0;
        };
        // SAFETY: a plain query.
        let value = unsafe { c.getParamNormalized(p.id) };
        ((value.clamp(0.0, 1.0) * f64::from(count - 1)).round() as i32).clamp(0, count - 1)
    }
}

/// The root unit's id, the id of no program list, and the program-change parameter flag
/// (`kRootUnitId`, `kNoProgramListId`, `ParameterInfo::kIsProgramChange`).
const K_ROOT_UNIT: i32 = 0;
const K_NO_PROGRAM_LIST: i32 = -1;
const K_IS_PROGRAM_CHANGE: i32 = 0x8000;

/// Queues parameter changes for a plugin's processor, for its next block. The controller is
/// left alone: it is only touched on its own thread.
#[derive(Clone)]
pub struct ParamSender {
    handler: ComWrapper<ComponentHandler>,
    params: Arc<Vec<(ParamID, String)>>,
}

impl ParamSender {
    /// The parameter a song names by its number or its title.
    pub fn id_of(&self, key: &str) -> Option<ParamID> {
        let by_id = key
            .parse::<u32>()
            .ok()
            .filter(|id| self.params.iter().any(|p| p.0 == *id));
        by_id.or_else(|| {
            self.params
                .iter()
                .find(|p| p.1.eq_ignore_ascii_case(key))
                .map(|p| p.0)
        })
    }

    /// Queues `value` (0..1) for the parameter `key`; false when there is no such parameter or
    /// the queue is full.
    pub fn set(&self, key: &str, value: f64) -> bool {
        match self.id_of(key) {
            Some(id) => self.handler.push_edit(id, value.clamp(0.0, 1.0)),
            None => false,
        }
    }
}

/// Channel buffers for every bus in one direction, allocated once in `prepare`.
struct Buses {
    storage: Vec<f32>,
    ptrs: Vec<*mut f32>,
    buffers: Vec<AudioBusBuffers>,
    channels: Vec<usize>,
}

impl Buses {
    fn empty() -> Buses {
        Buses {
            storage: Vec::new(),
            ptrs: Vec::new(),
            buffers: Vec::new(),
            channels: Vec::new(),
        }
    }

    fn new(channels: Vec<usize>, max_block: usize) -> Buses {
        let total: usize = channels.iter().sum();
        let mut b = Buses {
            storage: vec![0.0; total * max_block],
            ptrs: Vec::with_capacity(total),
            buffers: Vec::with_capacity(channels.len()),
            channels,
        };
        let base = b.storage.as_mut_ptr();
        for ch in 0..total {
            // SAFETY: within `storage`, which is never resized after this.
            b.ptrs.push(unsafe { base.add(ch * max_block) });
        }
        let mut first = 0;
        for &n in &b.channels {
            b.buffers.push(AudioBusBuffers {
                numChannels: n as i32,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    // SAFETY: `first <= total`, so this is within or one past `ptrs`.
                    channelBuffers32: unsafe { b.ptrs.as_mut_ptr().add(first) },
                },
            });
            first += n;
        }
        b
    }

    /// The first bus's channels, as `max_block`-long slices.
    fn main(&mut self, max_block: usize) -> &mut [f32] {
        let n = self.channels.first().copied().unwrap_or(0);
        &mut self.storage[..n * max_block]
    }

    fn data(&mut self) -> *mut AudioBusBuffers {
        if self.buffers.is_empty() {
            std::ptr::null_mut()
        } else {
            self.buffers.as_mut_ptr()
        }
    }
}

/// The real-time side of a plugin: calls `process` with preallocated buffers, events and
/// parameter changes. Dropping it stops processing and deactivates the component. Do not drop
/// it on the audio thread when the [`Plugin`] may already be gone: the last one tears the
/// instance down, which waits for a turn.
pub struct AudioPart {
    shared: Arc<Shared>,
    edits: rtrb::Consumer<(ParamID, ParamValue)>,
    /// Each block's `outputParameterChanges`, last value per parameter, to the UI side.
    reported: rtrb::Producer<(ParamID, ParamValue)>,
    in_events: ComWrapper<EventList>,
    out_events: ComWrapper<EventList>,
    in_params: ComWrapper<ParameterChanges>,
    out_params: ComWrapper<ParameterChanges>,
    inputs: Buses,
    outputs: Buses,
    context: Box<ProcessContext>,
    has_event_input: bool,
    midi_map: Box<[Option<ParamID>; CTRL_NUMBERS]>,
    control_counts: ControlCounts,
    active: bool,
    offline: bool,
    max_block: usize,
    process_errors: u64,
    edits_applied: u64,
}

// SAFETY: the raw pointers point into this struct's own heap buffers. The COM objects are
// used by one thread at a time; VST3 lets `process` run on another thread than the UI.
unsafe impl Send for AudioPart {}

impl AudioPart {
    fn new(
        shared: Arc<Shared>,
        edits: rtrb::Consumer<(ParamID, ParamValue)>,
        reported: rtrb::Producer<(ParamID, ParamValue)>,
    ) -> AudioPart {
        // The input pool can receive a full sandbox event ring in one block. Size the ID
        // pool for this plugin too, rather than losing values on plugins with many controls.
        let params = shared
            .controller
            .as_ref()
            .map_or(0, |c| {
                // SAFETY: this constructor runs on the plugin's controller thread.
                unsafe { c.getParameterCount().max(0) as usize }
            })
            .max(MAX_PARAMS_PER_BLOCK);
        AudioPart {
            shared,
            edits,
            reported,
            in_events: EventList::with_capacity(512),
            out_events: EventList::with_capacity(512),
            in_params: ParameterChanges::with_capacity(params, 1024),
            out_params: ParameterChanges::with_capacity(MAX_PARAMS_PER_BLOCK, MAX_POINTS_PER_PARAM),
            inputs: Buses::empty(),
            outputs: Buses::empty(),
            // SAFETY: ProcessContext is plain data; all-zero is valid.
            context: Box::new(unsafe { std::mem::zeroed() }),
            has_event_input: false,
            midi_map: Box::new([None; CTRL_NUMBERS]),
            control_counts: ControlCounts::default(),
            active: false,
            offline: false,
            max_block: 0,
            process_errors: 0,
            edits_applied: 0,
        }
    }

    pub fn name(&self) -> &str {
        &self.shared.class.name
    }

    /// Sets up buses (stereo where the plugin agrees), processing and activation. Returns the
    /// plugin's latency in frames. Call it off the audio thread; it allocates.
    pub fn prepare(
        &mut self,
        sample_rate: f64,
        max_block: usize,
        offline: bool,
    ) -> Result<u32, Error> {
        self.deactivate();
        let comp = &self.shared.component;
        let proc = &self.shared.processor;
        // SAFETY: plain calls with arrays and out-structs owned here.
        unsafe {
            let n_in = comp.getBusCount(kAudio, kInput).max(0);
            let n_out = comp.getBusCount(kAudio, kOutput).max(0);
            let mut ins: Vec<SpeakerArrangement> = vec![SpeakerArr::kStereo; n_in as usize];
            let mut outs: Vec<SpeakerArrangement> = vec![SpeakerArr::kStereo; n_out as usize];
            // Plugins may refuse; what they settle on is read back below.
            proc.setBusArrangements(ins.as_mut_ptr(), n_in, outs.as_mut_ptr(), n_out);
            let channels = |dir: i32, n: i32| -> Vec<usize> {
                (0..n)
                    .map(|i| {
                        let mut arr: SpeakerArrangement = 0;
                        if proc.getBusArrangement(dir, i, &mut arr) == kResultOk {
                            arr.count_ones() as usize
                        } else {
                            let mut info: BusInfo = std::mem::zeroed();
                            comp.getBusInfo(kAudio, dir, i, &mut info);
                            info.channelCount.max(0) as usize
                        }
                    })
                    .collect()
            };
            let in_ch = channels(kInput, n_in);
            let out_ch = channels(kOutput, n_out);
            for i in 0..n_in {
                comp.activateBus(kAudio, kInput, i, (i == 0) as u8);
            }
            for i in 0..n_out {
                comp.activateBus(kAudio, kOutput, i, (i == 0) as u8);
            }
            let n_ev = comp.getBusCount(kEvent, kInput);
            for i in 0..n_ev {
                comp.activateBus(kEvent, kInput, i, (i == 0) as u8);
            }
            self.has_event_input = n_ev > 0;
            if proc.canProcessSampleSize(kSample32) != kResultOk {
                return Err(error("the plugin cannot process 32-bit float"));
            }
            let mut setup = ProcessSetup {
                processMode: if offline { kOffline } else { kRealtime },
                symbolicSampleSize: kSample32,
                maxSamplesPerBlock: max_block as i32,
                sampleRate: sample_rate,
            };
            if proc.setupProcessing(&mut setup) != kResultOk {
                return Err(error("IAudioProcessor::setupProcessing failed"));
            }
            self.inputs = Buses::new(in_ch, max_block);
            self.outputs = Buses::new(out_ch, max_block);
            if comp.setActive(1) != kResultOk {
                return Err(error("IComponent::setActive failed"));
            }
            // Some plugins answer kNotImplemented here; that is allowed.
            proc.setProcessing(1);
        }
        self.active = true;
        self.offline = offline;
        self.max_block = max_block;
        *self.context = ProcessContext {
            state: (kPlaying
                | kTempoValid
                | kTimeSigValid
                | kProjectTimeMusicValid
                | kContTimeValid) as u32,
            sampleRate: sample_rate,
            tempo: 120.0,
            timeSigNumerator: 4,
            timeSigDenominator: 4,
            // SAFETY: the remaining fields are plain data; zero is valid.
            ..unsafe { std::mem::zeroed() }
        };
        Ok(self.latency())
    }

    fn deactivate(&mut self) {
        if self.active {
            // SAFETY: undoes `prepare`, in the SDK's order.
            unsafe {
                self.shared.processor.setProcessing(0);
                self.shared.component.setActive(0);
            }
            self.active = false;
        }
    }

    pub fn latency(&self) -> u32 {
        // SAFETY: a plain query.
        unsafe { self.shared.processor.getLatencySamples() }
    }

    pub fn input_channels(&self) -> &[usize] {
        &self.inputs.channels
    }

    pub fn output_channels(&self) -> &[usize] {
        &self.outputs.channels
    }

    pub fn has_event_input(&self) -> bool {
        self.has_event_input
    }

    /// Calls that returned an error, and edits handed to the processor.
    pub fn counts(&self) -> (u64, u64) {
        (self.process_errors, self.edits_applied)
    }

    /// Notes dropped because a block had more than the event list holds.
    pub fn dropped_events(&self) -> u64 {
        self.in_events.dropped()
    }

    /// How the controller changes passed to `process_with` reached the plugin.
    pub fn control_counts(&self) -> ControlCounts {
        self.control_counts
    }

    /// Whether the plugin's controller maps `number` (see [`Control`]) to a parameter.
    pub fn maps_control(&self, number: u8) -> bool {
        self.midi_map
            .get(usize::from(number))
            .is_some_and(Option::is_some)
    }

    /// Processes `left`/`right` in place (at most `max_block` frames). Instruments ignore the
    /// input and write their output. Does not allocate.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], notes: &[Note]) {
        self.process_with(left, right, notes, &[]);
    }

    /// [`process`](Self::process) with controller changes, sorted by `offset`.
    pub fn process_with(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        notes: &[Note],
        controls: &[Control],
    ) {
        self.process_with_params(left, right, notes, controls, &[]);
    }

    /// Processes block-timed automation together with audio, notes and controllers.
    /// Parameter points must be sorted by offset. Does not allocate or touch the controller.
    pub fn process_with_params(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        notes: &[Note],
        controls: &[Control],
        params: &[ParamChange],
    ) {
        let n = left.len().min(right.len()).min(self.max_block);
        if !self.active || n == 0 {
            return;
        }
        let mb = self.max_block;
        self.inputs.storage.fill(0.0);
        self.outputs.storage.fill(0.0);
        let main_in = self.inputs.channels.first().copied().unwrap_or(0);
        let inp = self.inputs.main(mb);
        match main_in {
            0 => {}
            1 => {
                for i in 0..n {
                    inp[i] = 0.5 * (left[i] + right[i]);
                }
            }
            _ => {
                inp[..n].copy_from_slice(&left[..n]);
                inp[mb..mb + n].copy_from_slice(&right[..n]);
            }
        }

        self.in_events.clear();
        self.out_events.clear();
        if self.has_event_input {
            for note in notes {
                self.in_events.push(note_event(note, n));
            }
        }
        self.in_params.clear();
        self.out_params.clear();
        while let Ok((id, value)) = self.edits.pop() {
            self.in_params.add(id, 0, value);
            self.edits_applied += 1;
        }
        for p in params {
            self.in_params.add(
                p.id,
                p.offset.min(n as u32 - 1) as i32,
                p.value.clamp(0.0, 1.0),
            );
        }
        for c in controls {
            let offset = (c.offset as usize).min(n - 1) as i32;
            let mapped = self.midi_map.get(usize::from(c.number)).copied().flatten();
            if let Some(id) = mapped {
                if self.in_params.add(id, offset, c.normalized()) {
                    self.control_counts.as_params += 1;
                } else {
                    self.control_counts.dropped += 1;
                }
            } else if self.has_event_input {
                if self.in_events.push(control_event(c, n)) {
                    self.control_counts.as_events += 1;
                } else {
                    self.control_counts.dropped += 1;
                }
            }
        }

        let mut data = ProcessData {
            processMode: if self.offline { kOffline } else { kRealtime },
            symbolicSampleSize: kSample32,
            numSamples: n as i32,
            numInputs: self.inputs.buffers.len() as i32,
            numOutputs: self.outputs.buffers.len() as i32,
            inputs: self.inputs.data(),
            outputs: self.outputs.data(),
            inputParameterChanges: com_ptr::<_, IParameterChanges>(&self.in_params),
            outputParameterChanges: com_ptr::<_, IParameterChanges>(&self.out_params),
            inputEvents: if self.has_event_input {
                com_ptr::<_, IEventList>(&self.in_events)
            } else {
                std::ptr::null_mut()
            },
            outputEvents: com_ptr::<_, IEventList>(&self.out_events),
            processContext: &mut *self.context,
        };
        for b in self
            .inputs
            .buffers
            .iter_mut()
            .chain(self.outputs.buffers.iter_mut())
        {
            b.silenceFlags = 0;
        }
        // SAFETY: every pointer in `data` points into buffers owned by `self`, sized for
        // `max_block` >= n frames.
        if unsafe { self.shared.processor.process(&mut data) } != kResultOk {
            self.process_errors += 1;
        }
        // A full queue drops the rest; the next block reports again.
        for i in 0..self.out_params.len() {
            if let Some(v) = self.out_params.last(i)
                && self.reported.push(v).is_err()
            {
                break;
            }
        }

        let main_out = self.outputs.channels.first().copied().unwrap_or(0);
        let out = self.outputs.main(mb);
        match main_out {
            0 => {}
            1 => {
                left[..n].copy_from_slice(&out[..n]);
                right[..n].copy_from_slice(&out[..n]);
            }
            _ => {
                left[..n].copy_from_slice(&out[..n]);
                right[..n].copy_from_slice(&out[mb..mb + n]);
            }
        }
        let c = &mut *self.context;
        c.projectTimeSamples += n as i64;
        c.continousTimeSamples += n as i64;
        c.projectTimeMusic += n as f64 / c.sampleRate * c.tempo / 60.0;
    }
}

fn com_ptr<C: vst3::Class, I: vst3::Interface>(w: &ComWrapper<C>) -> *mut I {
    w.as_com_ref::<I>()
        .map(|r| r.as_ptr())
        .unwrap_or(std::ptr::null_mut())
}

fn note_event(note: &Note, frames: usize) -> Event {
    // SAFETY: Event is plain data; all-zero is valid before the fields are set.
    let mut e: Event = unsafe { std::mem::zeroed() };
    e.busIndex = 0;
    e.sampleOffset = (note.offset as usize).min(frames.saturating_sub(1)) as i32;
    e.flags = 1; // kIsLive
    if note.on {
        e.r#type = kNoteOnEvent as u16;
        e.__field0.noteOn = NoteOnEvent {
            channel: 0,
            pitch: note.pitch as i16,
            tuning: 0.0,
            velocity: note.velocity,
            length: 0,
            noteId: -1,
        };
    } else {
        e.r#type = kNoteOffEvent as u16;
        e.__field0.noteOff = NoteOffEvent {
            channel: 0,
            pitch: note.pitch as i16,
            velocity: 0.0,
            noteId: -1,
            tuning: 0.0,
        };
    }
    e
}

/// The legacy MIDI CC event for a plugin that does not map the number to a parameter.
fn control_event(c: &Control, frames: usize) -> Event {
    // SAFETY: Event is plain data; all-zero is valid before the fields are set.
    let mut e: Event = unsafe { std::mem::zeroed() };
    e.busIndex = 0;
    e.sampleOffset = (c.offset as usize).min(frames.saturating_sub(1)) as i32;
    e.flags = 1; // kIsLive
    e.r#type = kLegacyMIDICCOutEvent as u16;
    let (value, value2) = if c.number == CTRL_PITCH_BEND {
        // 14 bits: the low 7 in `value`, the high 7 in `value2`.
        let raw = (c.value + 8192).clamp(0, 16383);
        ((raw & 0x7f) as i8, (raw >> 7) as i8)
    } else {
        (c.value.clamp(0, 127) as i8, 0)
    };
    e.__field0.midiCCOut = LegacyMIDICCOutEvent {
        controlNumber: c.number,
        channel: 0,
        value,
        value2,
    };
    e
}

impl Drop for AudioPart {
    fn drop(&mut self) {
        self.deactivate();
    }
}
