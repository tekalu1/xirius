//! A device-free VST3 effect for latency/PDC integration tests (ENG-011).
#![allow(non_snake_case)]
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper};

pub mod editor;

pub const CID: TUID = [88, 73, 82, 73, 85, 83, 76, 65, 84, 69, 78, 67, 89, 0, 0, 1];
pub fn fixture_marker() {}
/// Momentary test trigger: a positive point exits the hosting process during processing.
pub const CRASH_PARAMETER: u32 = 3;
pub const CRASH_EXIT_CODE: i32 = 86;
/// The keys its one program names (`IUnitInfo` program pitch names), and what it calls them.
pub const NOTE_NAMES: [(u8, &str); 2] = [(36, "Fixture Kick"), (38, "Fixture Snare")];
const PROGRAM_LIST: i32 = 7;
/// A copied fixture can be held independently by a gate beside its own DLL.
/// Unlike process environment settings this never affects another test's plugin.
#[cfg(windows)]
fn opening_gate() {
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleFileNameW, GetModuleHandleExW,
    };
    let mut module = std::ptr::null_mut();
    let mut name = vec![0u16; 32768];
    // SAFETY: FROM_ADDRESS identifies the module containing this function, and
    // the output buffers remain valid for the calls. The reference count is unchanged.
    let path = unsafe {
        if GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            opening_gate as *const () as *const u16,
            &mut module,
        ) == 0
        {
            return;
        }
        let n = GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32);
        if n == 0 {
            return;
        }
        std::path::PathBuf::from(String::from_utf16_lossy(&name[..n as usize]))
    };
    let gate = path.with_extension("gate");
    if !gate.exists() {
        return;
    }
    let _ = std::fs::write(path.with_extension("entered"), b"opening");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while gate.exists() && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    if gate.exists() {
        let _ = std::fs::write(path.with_extension("timed-out"), b"gate timed out");
    }
}
struct Factory;
impl Class for Factory {
    type Interfaces = (IPluginFactory,);
}
impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the SDK supplies a writable factory record.
        unsafe {
            *info = std::mem::zeroed();
        }
        kResultOk
    }
    unsafe fn countClasses(&self) -> int32 {
        1
    }
    unsafe fn getClassInfo(&self, index: int32, info: *mut PClassInfo) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: checked output pointer; SDK fixed-size fields contain terminated strings.
        unsafe {
            *info = std::mem::zeroed();
            (*info).cid = CID;
            (*info).cardinality = i32::MAX;
            for (dst, src) in (*info).category.iter_mut().zip(b"Audio Module Class") {
                *dst = *src as i8;
            }
            for (dst, src) in (*info).name.iter_mut().zip(b"Xirius Test Latency") {
                *dst = *src as i8;
            }
        }
        kResultOk
    }
    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        obj: *mut *mut std::ffi::c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the factory ABI passes 16 readable UID bytes.
        if unsafe { std::slice::from_raw_parts(cid, 16) } != CID {
            return kResultFalse;
        }
        #[cfg(windows)]
        opening_gate();
        // A slow plugin, for the opening of a song's plugins together (ENG-064).
        if let Some(ms) = std::env::var("XIRIUS_TEST_VST3_OPEN_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        // An instance that cannot be made while the host already has others (ENG-067): the
        // opening fails while the named file exists.
        if std::env::var_os("XIRIUS_TEST_VST3_FAIL_FILE")
            .is_some_and(|p| std::path::Path::new(&p).exists())
        {
            return kResultFalse;
        }
        let plugin = ComWrapper::new(Latency {
            delay: Arc::new(AtomicU32::new(0)),
            number: MADE.fetch_add(1, Ordering::Relaxed) + 1,
            home: std::thread::current().id(),
            handler: UnsafeCell::new(None),
            buffers: UnsafeCell::new([[0.0; 512]; 2]),
            position: UnsafeCell::new(0),
        });
        let component = plugin.to_com_ptr::<IComponent>().unwrap();
        // SAFETY: the ABI supplies an interface UID and writable output; queryInterface
        // transfers one reference to the requested interface, or reports kNoInterface.
        let unknown = component.as_ptr().cast::<FUnknown>();
        unsafe { ((*(*unknown).vtbl).queryInterface)(unknown, iid.cast(), obj) }
    }
}

/// Instances made in this process so far.
static MADE: AtomicU32 = AtomicU32::new(0);

struct Latency {
    /// Shared with the editor's view, which shows it.
    delay: Arc<AtomicU32>,
    /// Which instance of this process this is (1 for the first), shown in the editor.
    number: u32,
    /// The thread that made it. Some plugins tie windows to it and break when another thread
    /// terminates them (ARC-044); [`FixtureTerminations`] counts those that were.
    home: std::thread::ThreadId,
    // Installed before processing, retained while processing, and replaced after it stops.
    handler: UnsafeCell<Option<ComPtr<IComponentHandler>>>,
    buffers: UnsafeCell<[[f32; 512]; 2]>,
    position: UnsafeCell<usize>,
}
impl Class for Latency {
    type Interfaces = (IComponent, IAudioProcessor, IEditController, IUnitInfo);
}
impl IPluginBaseTrait for Latency {
    unsafe fn initialize(&self, _: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        TERMINATED.fetch_add(1, Ordering::Relaxed);
        if std::thread::current().id() != self.home {
            TERMINATED_AWAY.fetch_add(1, Ordering::Relaxed);
        }
        kResultOk
    }
}
impl Latency {
    unsafe fn load(&self, state: *mut IBStream) -> tresult {
        let Some(stream) = (unsafe { vst3::ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        let mut bytes = [0u8; 4];
        let mut got = 0;
        // SAFETY: valid stream and fixed writable four-byte buffer.
        let result = unsafe { stream.read(bytes.as_mut_ptr().cast(), 4, &mut got) };
        if result == kResultOk && got == 4 {
            let delay = u32::from_le_bytes(bytes).min(256);
            if self.delay.swap(delay, Ordering::Relaxed) != delay {
                self.notify_latency();
            }
        }
        result
    }
    unsafe fn save(&self, state: *mut IBStream) -> tresult {
        if std::thread::current().id() != self.home {
            STATE_AWAY.fetch_add(1, Ordering::Relaxed);
        }
        state_gate();
        let Some(stream) = (unsafe { vst3::ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        let mut bytes = self.delay.load(Ordering::Relaxed).to_le_bytes();
        // SAFETY: valid stream and a four-byte buffer that outlives the call.
        unsafe { stream.write(bytes.as_mut_ptr().cast(), 4, std::ptr::null_mut()) }
    }
    fn notify_latency(&self) {
        // SAFETY: the host installs the handler before processing and retains it until processing ends.
        if let Some(handler) = unsafe { &*self.handler.get() } {
            unsafe {
                handler.restartComponent(8);
            }
        }
    }
}
impl IComponentTrait for Latency {
    unsafe fn getControllerClassId(&self, _: *mut TUID) -> tresult {
        kResultFalse
    }
    unsafe fn setIoMode(&self, _: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, media: MediaType, _: BusDirection) -> int32 {
        if media == 0 { 1 } else { 0 }
    }
    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: int32,
        bus: *mut BusInfo,
    ) -> tresult {
        if media != 0 || index != 0 || bus.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: checked output record.
        unsafe {
            *bus = std::mem::zeroed();
            (*bus).mediaType = media;
            (*bus).direction = dir;
            (*bus).channelCount = 2;
            (*bus).busType = 0;
            (*bus).flags = 1;
            for (dst, src) in (*bus).name.iter_mut().zip(b"Stereo") {
                *dst = *src as u16;
            }
        }
        kResultOk
    }
    unsafe fn getRoutingInfo(&self, _: *mut RoutingInfo, _: *mut RoutingInfo) -> tresult {
        kResultFalse
    }
    unsafe fn activateBus(&self, _: MediaType, _: BusDirection, _: int32, _: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setActive(&self, _: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        unsafe { self.load(state) }
    }
    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        unsafe { self.save(state) }
    }
}
impl IAudioProcessorTrait for Latency {
    unsafe fn setBusArrangements(
        &self,
        _: *mut SpeakerArrangement,
        _: int32,
        _: *mut SpeakerArrangement,
        _: int32,
    ) -> tresult {
        kResultOk
    }
    unsafe fn getBusArrangement(
        &self,
        _: BusDirection,
        _: int32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        if arr.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            *arr = 3;
        }
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, size: int32) -> tresult {
        if size == 0 { kResultOk } else { kResultFalse }
    }
    unsafe fn getLatencySamples(&self) -> uint32 {
        self.delay.load(Ordering::Relaxed)
    }
    unsafe fn setupProcessing(&self, _: *mut ProcessSetup) -> tresult {
        kResultOk
    }
    unsafe fn setProcessing(&self, _: TBool) -> tresult {
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> uint32 {
        0
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        if data.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the host supplies planar f32 stereo buses, and serializes this audio call.
        unsafe {
            let data = &mut *data;
            if let Some(changes) = vst3::ComRef::from_raw(data.inputParameterChanges) {
                for i in 0..changes.getParameterCount() {
                    if let Some(queue) = vst3::ComRef::from_raw(changes.getParameterData(i))
                        && queue.getParameterId() == CRASH_PARAMETER
                    {
                        for point in 0..queue.getPointCount() {
                            let (mut offset, mut value) = (0, 0.0);
                            if queue.getPoint(point, &mut offset, &mut value) == kResultOk
                                && value > 0.5
                            {
                                std::process::exit(CRASH_EXIT_CODE);
                            }
                        }
                    }
                    if let Some(queue) = vst3::ComRef::from_raw(changes.getParameterData(i))
                        && queue.getParameterId() == 1
                        && queue.getPointCount() > 0
                    {
                        let (mut offset, mut value) = (0, 0.0);
                        if queue.getPoint(queue.getPointCount() - 1, &mut offset, &mut value)
                            == kResultOk
                        {
                            let delay = (value.clamp(0.0, 1.0) * 256.0).round() as u32;
                            if self.delay.swap(delay, Ordering::Relaxed) != delay {
                                self.notify_latency();
                            }
                        }
                    }
                }
            }
            // A call without buffers only flushes parameters: nothing to do.
            if data.numInputs < 1
                || data.numOutputs < 1
                || data.inputs.is_null()
                || data.outputs.is_null()
                || (*data.inputs).numChannels < 2
                || (*data.outputs).numChannels < 2
            {
                return kResultOk;
            }
            let input = (*data.inputs).__field0.channelBuffers32;
            let output = (*data.outputs).__field0.channelBuffers32;
            if input.is_null() || output.is_null() {
                return kResultOk;
            }
            let delay = self.delay.load(Ordering::Relaxed) as usize;
            let buffers = &mut *self.buffers.get();
            let position = &mut *self.position.get();
            for i in 0..data.numSamples as usize {
                for (ch, buffer) in buffers.iter_mut().enumerate() {
                    let value = *(*input.add(ch)).add(i);
                    buffer[*position] = value;
                    *(*output.add(ch)).add(i) = buffer[(*position + 512 - delay) % 512];
                }
                *position = (*position + 1) % 512;
            }
        }
        kResultOk
    }
}
impl IEditControllerTrait for Latency {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        unsafe { self.load(state) }
    }
    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        unsafe { self.load(state) }
    }
    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        unsafe { self.save(state) }
    }
    unsafe fn getParameterCount(&self) -> int32 {
        2
    }
    unsafe fn getParameterInfo(&self, index: int32, info: *mut ParameterInfo) -> tresult {
        if !(0..2).contains(&index) || info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: checked writable SDK output; both parameters use bounded normalized values.
        unsafe {
            *info = std::mem::zeroed();
            (*info).id = if index == 0 { 1 } else { CRASH_PARAMETER };
            (*info).stepCount = if index == 0 { 256 } else { 1 };
            (*info).flags = 1;
            let title = if index == 0 { "Latency" } else { "Crash" };
            for (dst, src) in (*info).title.iter_mut().zip(title.encode_utf16()) {
                *dst = src;
            }
        }
        kResultOk
    }
    unsafe fn getParamStringByValue(
        &self,
        _: ParamID,
        _: ParamValue,
        _: *mut String128,
    ) -> tresult {
        kResultFalse
    }
    unsafe fn getParamValueByString(
        &self,
        _: ParamID,
        _: *mut TChar,
        _: *mut ParamValue,
    ) -> tresult {
        kResultFalse
    }
    unsafe fn normalizedParamToPlain(&self, _: ParamID, value: ParamValue) -> ParamValue {
        value * 256.0
    }
    unsafe fn plainParamToNormalized(&self, _: ParamID, value: ParamValue) -> ParamValue {
        value / 256.0
    }
    unsafe fn getParamNormalized(&self, id: ParamID) -> ParamValue {
        if id == 1 {
            f64::from(self.delay.load(Ordering::Relaxed)) / 256.0
        } else {
            0.0
        }
    }
    unsafe fn setParamNormalized(&self, id: ParamID, value: ParamValue) -> tresult {
        // The momentary trigger acts on processor events, and is never part of saved state.
        if id == CRASH_PARAMETER {
            return kResultOk;
        }
        if id != 1 {
            return kInvalidArgument;
        }
        self.delay.store(
            (value.clamp(0.0, 1.0) * 256.0).round() as u32,
            Ordering::Relaxed,
        );
        // SAFETY: the host owns controller calls, and keeps the handler alive until detach.
        self.notify_latency();
        kResultOk
    }
    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        // SAFETY: SDK controller-thread call. Add a reference before replacing the old handler.
        unsafe {
            *self.handler.get() = vst3::ComRef::from_raw(handler).map(|h| h.to_com_ptr());
        }
        kResultOk
    }
    unsafe fn createView(&self, _: FIDString) -> *mut IPlugView {
        editor::create(self.delay.clone(), self.number)
    }
}

fn write_name(dst: *mut String128, name: &str) -> tresult {
    if dst.is_null() {
        return kInvalidArgument;
    }
    // SAFETY: checked writable SDK output.
    unsafe {
        *dst = [0; 128];
        for (d, s) in (*dst).iter_mut().zip(name.encode_utf16()) {
            *d = s;
        }
    }
    kResultOk
}

/// One root unit with one program list of one program, which names a few keys.
impl IUnitInfoTrait for Latency {
    unsafe fn getUnitCount(&self) -> int32 {
        1
    }
    unsafe fn getUnitInfo(&self, index: int32, info: *mut UnitInfo) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: checked writable SDK output.
        unsafe {
            *info = std::mem::zeroed();
            (*info).id = 0;
            (*info).parentUnitId = -1;
            (*info).programListId = PROGRAM_LIST;
            write_name(&mut (*info).name, "Root")
        }
    }
    unsafe fn getProgramListCount(&self) -> int32 {
        1
    }
    unsafe fn getProgramListInfo(&self, index: int32, info: *mut ProgramListInfo) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: checked writable SDK output.
        unsafe {
            *info = std::mem::zeroed();
            (*info).id = PROGRAM_LIST;
            (*info).programCount = 1;
            write_name(&mut (*info).name, "Kits")
        }
    }
    unsafe fn getProgramName(
        &self,
        list: ProgramListID,
        index: int32,
        name: *mut String128,
    ) -> tresult {
        if list != PROGRAM_LIST || index != 0 {
            return kInvalidArgument;
        }
        write_name(name, "Fixture Kit")
    }
    unsafe fn getProgramInfo(
        &self,
        _: ProgramListID,
        _: int32,
        _: vst3::Steinberg::Vst::CString,
        _: *mut String128,
    ) -> tresult {
        kResultFalse
    }
    unsafe fn hasProgramPitchNames(&self, list: ProgramListID, index: int32) -> tresult {
        if list == PROGRAM_LIST && index == 0 {
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn getProgramPitchName(
        &self,
        list: ProgramListID,
        index: int32,
        pitch: int16,
        name: *mut String128,
    ) -> tresult {
        if list != PROGRAM_LIST || index != 0 {
            return kInvalidArgument;
        }
        match NOTE_NAMES.iter().find(|(key, _)| i16::from(*key) == pitch) {
            Some((_, n)) => write_name(name, n),
            None => kResultFalse,
        }
    }
    unsafe fn getSelectedUnit(&self) -> UnitID {
        0
    }
    unsafe fn selectUnit(&self, _: UnitID) -> tresult {
        kResultOk
    }
    unsafe fn getUnitByBus(
        &self,
        _: MediaType,
        _: BusDirection,
        _: int32,
        _: int32,
        unit: *mut UnitID,
    ) -> tresult {
        if unit.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: checked writable output.
        unsafe {
            *unit = 0;
        }
        kResultOk
    }
    unsafe fn setUnitProgramData(&self, _: int32, _: int32, _: *mut IBStream) -> tresult {
        kResultFalse
    }
}

/// # Safety
/// Called by a VST3 host; transfers one owned factory reference.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    #[cfg(windows)]
    opening_gate();
    ComWrapper::new(Factory)
        .to_com_ptr::<IPluginFactory>()
        .unwrap()
        .into_raw()
}
#[unsafe(no_mangle)]
pub extern "system" fn InitDll() -> bool {
    INITIALIZED.fetch_add(1, Ordering::Relaxed);
    std::env::var_os("XIRIUS_TEST_VST3_INIT_FAIL").is_none()
}
#[unsafe(no_mangle)]
pub extern "system" fn ExitDll() -> bool {
    EXITED.fetch_add(1, Ordering::Relaxed);
    true
}

static INITIALIZED: AtomicU32 = AtomicU32::new(0);
static EXITED: AtomicU32 = AtomicU32::new(0);
static TERMINATED: AtomicU32 = AtomicU32::new(0);
static TERMINATED_AWAY: AtomicU32 = AtomicU32::new(0);
static STATE_AWAY: AtomicU32 = AtomicU32::new(0);
static STATE_ENTERED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static STATE_RELEASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// # Safety
/// Handles must remain valid until getState returns. Arm only while no state call
/// is in flight; an instance in another test must use its own copy of this DLL.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn FixtureStateGate(entered: usize, release: usize) {
    STATE_RELEASE.store(release, Ordering::Release);
    STATE_ENTERED.store(entered, Ordering::Release);
}

#[unsafe(no_mangle)]
pub extern "system" fn FixtureStateAway() -> u32 {
    STATE_AWAY.load(Ordering::Relaxed)
}

fn state_gate() {
    let entered = STATE_ENTERED.load(Ordering::Acquire);
    if entered != 0 {
        let release = STATE_RELEASE.load(Ordering::Acquire);
        // SAFETY: the fixture caller retains these event handles until release.
        unsafe {
            windows_sys::Win32::System::Threading::SetEvent(entered as _);
            windows_sys::Win32::System::Threading::WaitForSingleObject(release as _, 30_000);
        }
    }
}

/// The instances terminated so far (low half), and of those, the ones terminated on a thread
/// other than the one that made them (high half).
#[unsafe(no_mangle)]
pub extern "system" fn FixtureTerminations() -> u64 {
    u64::from(TERMINATED.load(Ordering::Relaxed))
        | (u64::from(TERMINATED_AWAY.load(Ordering::Relaxed)) << 32)
}

#[unsafe(no_mangle)]
pub extern "system" fn FixtureModuleCalls() -> u64 {
    u64::from(INITIALIZED.load(Ordering::Relaxed))
        | (u64::from(EXITED.load(Ordering::Relaxed)) << 32)
}

// A deliberately lingering DLL callback for the module-lifetime regression. The caller
// owns the counter until the one-shot timer has fired. No editor or audio device is opened.
static TIMER_TARGET: std::sync::atomic::AtomicPtr<AtomicU32> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// # Safety
/// Dispatch on the thread that armed FixtureLingeringTimer, while its counter is valid.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn FixtureTimerCallback(
    _: windows_sys::Win32::Foundation::HWND,
    _: u32,
    timer: usize,
    _: u32,
) {
    // SAFETY: a thread timer created below, on this thread.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::KillTimer(std::ptr::null_mut(), timer) };
    let target = TIMER_TARGET.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !target.is_null() {
        // SAFETY: the fixture caller retains the counter through callback dispatch.
        unsafe { (*target).fetch_add(1, Ordering::Release) };
    }
}

/// # Safety
/// `counter` must remain valid on this thread until its timer fires. Call once per process.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn FixtureLingeringTimer(counter: *mut AtomicU32) -> usize {
    TIMER_TARGET.store(counter, Ordering::Release);
    // SAFETY: the callback has the TIMERPROC ABI and lives in this DLL.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetTimer(
            std::ptr::null_mut(),
            0,
            1,
            Some(FixtureTimerCallback),
        )
    }
}
