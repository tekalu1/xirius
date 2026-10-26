//! Runs the VST3 and CLAP plugins of a [`xirius_engine::server::PluginServer`] in another process
//! (OPEN-02, PLG-023). Started by the engine, not by hand:
//!
//! ```text
//! xirius-plugin-host --parent PID --protocol N
//! ```
//!
//! The engine sends [`ToHost`] messages on stdin and reads [`FromHost`] messages on stdout.
//! The first message out is [`FromHost::Hello`] with this build's [`wire::PROTOCOL`]; a host
//! asked for another protocol says so and ends.
//! Each plugin gets its own shared memory and an audio thread that answers the engine's
//! blocks; this thread (the plugins' UI thread) opens and closes plugins, applies states and
//! parameters, runs editors and reports the user's edits. What plugins print goes to stderr,
//! so it cannot break the messages. A crash inside a plugin marks that plugin's shared memory
//! before the process goes down, so the engine knows which plugin it was.

use std::cell::Cell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Write};
use std::os::windows::io::FromRawHandle;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, EXCEPTION_ACCESS_VIOLATION, HANDLE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
};
use windows_sys::Win32::System::Diagnostics::Debug::{
    EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS, RaiseException, SEM_FAILCRITICALERRORS,
    SEM_NOGPFAULTERRORBOX, SetErrorMode, SetUnhandledExceptionFilter,
};
use windows_sys::Win32::System::ErrorReporting::{WER_FAULT_REPORTING_NO_UI, WerSetFlags};
use windows_sys::Win32::System::Threading::{
    AvSetMmThreadCharacteristicsW, GetCurrentProcess, OpenProcess, PROCESS_SYNCHRONIZE,
    WaitForSingleObject,
};
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use xirius_engine::sandbox::{Event, Header, NOTE_CONTROL, NOTE_PARAM, Ring, SharedRings};
use xirius_engine::wire::{self, Fault, FromHost, Gesture, OpenSlot, Opened, ToHost};
use xirius_vst3::{Control, Editor, Module, Note, ParamChange, Plugin};

#[path = "plugin_host/formats.rs"]
mod formats;

use formats::{Audio, Loaded};

/// After the user lets go of a control, the state is read once the processor has run this many
/// blocks (so it has the new value), or after `CAPTURE_WAIT` when it is not running.
const CAPTURE_BLOCKS: u64 = 2;
const CAPTURE_WAIT: Duration = Duration::from_millis(300);

unsafe extern "C" {
    fn _dup2(from: i32, to: i32) -> i32;
}

thread_local! {
    /// The instance this thread is calling into: its header and host generation.
    static IN_PLUGIN: Cell<(*const Header, u64)> = const { Cell::new((std::ptr::null(), 0)) };
}

/// While alive, a crash on this thread is put on one instance.
struct InPlugin;

impl InPlugin {
    fn enter(rings: &SharedRings, epoch: u64) -> InPlugin {
        IN_PLUGIN.set((rings.header(), epoch));
        InPlugin
    }
}

impl Drop for InPlugin {
    fn drop(&mut self) {
        IN_PLUGIN.set((std::ptr::null(), 0));
    }
}

/// Marks the instance the crashing thread was in, then lets the process go down.
unsafe extern "system" fn on_crash(exception: *const EXCEPTION_POINTERS) -> i32 {
    let (header, epoch) = IN_PLUGIN
        .try_with(Cell::get)
        .unwrap_or((std::ptr::null(), 0));
    if !header.is_null() {
        // SAFETY: the header belongs to a mapping its instance keeps while the guard is alive.
        unsafe { (*header).faulted.store(epoch, Ordering::Release) };
    }
    // SAFETY: Windows owns these exception pointers until this filter returns.
    unsafe {
        xirius_engine::crash::record_exception(exception);
    }
    EXCEPTION_CONTINUE_SEARCH
}

/// Takes stdout for the messages and points everything else that writes to stdout (the C
/// runtime's descriptor 1 and the standard handle plugins load with) at stderr.
fn take_stdout() -> File {
    // SAFETY: plain handle calls on this process's own handles.
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        let process = GetCurrentProcess();
        let mut mine: HANDLE = std::ptr::null_mut();
        DuplicateHandle(
            process,
            out,
            process,
            &mut mine,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        );
        _dup2(2, 1);
        SetStdHandle(STD_OUTPUT_HANDLE, GetStdHandle(STD_ERROR_HANDLE));
        File::from_raw_handle(mine as _)
    }
}

/// A test fault armed in an instance.
#[derive(Default)]
struct FaultCtl {
    /// 0: none, 1: crash, 2: hang.
    kind: AtomicU8,
    /// The block count at which it fires.
    at: AtomicU64,
}

struct Instance {
    plugin: Loaded,
    editor: Option<Editor>,
    audio: Option<JoinHandle<Audio>>,
    stop: Arc<AtomicBool>,
    fault: Arc<FaultCtl>,
    rings: Arc<SharedRings>,
    /// The host generation that opened it.
    epoch: u64,
    /// Read the state once the processor has run to this block, or at this time.
    capture: Option<(u64, Instant)>,
    capture_request: Option<u64>,
    /// The names of its keys last sent to the engine.
    note_names: Vec<(u8, String)>,
}

impl Instance {
    /// Reads the names of its keys again; the message for the engine when they changed.
    fn reread_note_names(&mut self, slot: u32) -> Option<FromHost> {
        let names = self.plugin.note_names();
        (names != self.note_names).then(|| {
            self.note_names = names.clone();
            FromHost::NoteNames { slot, names }
        })
    }

    fn blocks(&self) -> u64 {
        self.rings.header().host_blocks.load(Ordering::Acquire)
    }

    fn enter(&self) -> InPlugin {
        InPlugin::enter(&self.rings, self.epoch)
    }

    /// Stops the audio thread. A thread that does not stop (a hung plugin) is left behind with
    /// its plugin, since the plugin cannot be released under it.
    fn close(mut self) {
        self.editor = None;
        self.stop.store(true, Ordering::Release);
        let Some(audio) = self.audio.take() else {
            return;
        };
        let end = Instant::now() + Duration::from_secs(1);
        while !audio.is_finished() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(2));
        }
        if audio.is_finished() {
            drop(audio.join());
        } else {
            std::mem::forget(audio);
            std::mem::forget(self);
        }
    }
}

struct Host {
    out: File,
    modules: HashMap<String, Arc<Module>>,
    claps: HashMap<String, xirius_clap::Bundle>,
    instances: HashMap<u32, Instance>,
}

impl Host {
    fn send(&mut self, msg: &FromHost) {
        if wire::send(&mut self.out, &msg.encode()).is_err() {
            // The engine has gone.
            std::process::exit(0);
        }
    }

    fn handle(&mut self, msg: ToHost) {
        let slot = match &msg {
            ToHost::Open(o) => o.slot,
            ToHost::Close { slot }
            | ToHost::SetState { slot, .. }
            | ToHost::SetParam { slot, .. }
            | ToHost::OpenEditor { slot, .. }
            | ToHost::CloseEditor { slot }
            | ToHost::SetCaption { slot, .. }
            | ToHost::CaptureState { slot, .. }
            | ToHost::SimulateEdit { slot, .. }
            | ToHost::Fault { slot, .. } => *slot,
        };
        let _in = self.instances.get(&slot).map(Instance::enter);
        match msg {
            ToHost::Open(o) => {
                let (slot, epoch) = (o.slot, o.epoch);
                if let Some(old) = self.instances.remove(&slot) {
                    old.close();
                }
                let result = match self.open(o) {
                    Ok((inst, opened)) => {
                        self.instances.insert(slot, inst);
                        Ok(opened)
                    }
                    Err(e) => Err(e),
                };
                self.send(&FromHost::Opened {
                    slot,
                    epoch,
                    result,
                });
            }
            ToHost::Close { slot } => {
                if let Some(mut inst) = self.instances.remove(&slot) {
                    // A window the person shut since the last `poll` is said shut before the
                    // instance goes: the engine may have a window open elsewhere because of it
                    // (ENG-071).
                    if inst.editor.as_ref().is_some_and(|e| !e.is_visible()) {
                        self.send(&FromHost::EditorClosed { slot });
                    }
                    // Play presses the window took until now still count.
                    if let Some(editor) = inst.editor.take() {
                        for _ in 0..editor.take_play_requests() {
                            self.send(&FromHost::PlayKey { slot });
                        }
                    }
                    inst.close();
                }
                self.send(&FromHost::Closed { slot });
            }
            ToHost::SetState { slot, state } => {
                if let Some(inst) = self.instances.get_mut(&slot) {
                    if let Err(e) = inst.plugin.set_state(&state) {
                        eprintln!("xirius-plugin-host: slot {slot}: {e}");
                    }
                    // What the plugin reports about its own new state is not the user's.
                    inst.capture = None;
                    inst.plugin.take_gestures();
                    let latency_changed = inst.plugin.take_changes().latency;
                    let latency = inst.plugin.latency();
                    let names = inst.reread_note_names(slot);
                    if latency_changed {
                        self.send(&FromHost::Latency { slot, latency });
                    }
                    if let Some(msg) = names {
                        self.send(&msg);
                    }
                }
            }
            ToHost::SetParam { slot, key, value } => {
                if let Some(inst) = self.instances.get(&slot)
                    && let Some(id) = inst.plugin.resolve(&key)
                {
                    inst.plugin.set_param(id, value.clamp(0.0, 1.0));
                }
            }
            ToHost::OpenEditor {
                slot,
                title,
                caption,
            } => {
                let Some(inst) = self.instances.get_mut(&slot) else {
                    return;
                };
                if inst.editor.is_none() {
                    match inst.plugin.open_editor(&title, caption) {
                        Ok(e) => inst.editor = Some(e),
                        Err(e) => {
                            eprintln!("xirius-plugin-host: slot {slot}: {e}");
                            self.send(&FromHost::EditorClosed { slot });
                        }
                    }
                }
            }
            ToHost::SetCaption { slot, caption } => {
                if let Some(editor) = self
                    .instances
                    .get(&slot)
                    .and_then(|inst| inst.editor.as_ref())
                {
                    editor.set_caption(caption);
                }
            }
            ToHost::CloseEditor { slot } => {
                let mut shut = false;
                let mut keys = 0;
                if let Some(inst) = self.instances.get_mut(&slot)
                    && let Some(editor) = inst.editor.take()
                {
                    // Play presses the window took until now still count.
                    keys = editor.take_play_requests();
                    inst.capture = Some((0, Instant::now()));
                    // Shut by the person since the last `poll`, as with `Close`.
                    shut = !editor.is_visible();
                }
                for _ in 0..keys {
                    self.send(&FromHost::PlayKey { slot });
                }
                if shut {
                    self.send(&FromHost::EditorClosed { slot });
                }
            }
            ToHost::CaptureState { slot, request } => {
                // Read in the next `poll`, on this thread, where the plugin may be used.
                if let Some(inst) = self.instances.get_mut(&slot) {
                    inst.capture = Some((0, Instant::now()));
                    inst.capture_request = Some(request);
                } else {
                    self.send(&FromHost::CapturedState {
                        slot,
                        request,
                        result: Err("プラグインが開かれていません".into()),
                    });
                }
            }
            ToHost::SimulateEdit { slot, id, values } => {
                if let Some(inst) = self.instances.get(&slot) {
                    inst.plugin.edit_like_editor(id, &values);
                }
            }
            ToHost::Fault {
                slot,
                kind,
                after_blocks,
            } => {
                if let Some(inst) = self.instances.get(&slot) {
                    inst.fault
                        .at
                        .store(inst.blocks() + after_blocks, Ordering::Release);
                    let k = match kind {
                        Fault::Crash => 1,
                        Fault::Hang => 2,
                    };
                    inst.fault.kind.store(k, Ordering::Release);
                }
            }
        }
    }

    fn module(&mut self, bundle: &str) -> Result<Arc<Module>, String> {
        if let Some(m) = self.modules.get(bundle) {
            return Ok(m.clone());
        }
        let m = Arc::new(Module::load(bundle.as_ref()).map_err(|e| format!("{bundle}: {e}"))?);
        self.modules.insert(bundle.to_string(), m.clone());
        Ok(m)
    }

    fn clap_bundle(&mut self, bundle: &str) -> Result<xirius_clap::Bundle, String> {
        if let Some(b) = self.claps.get(bundle) {
            return Ok(b.clone());
        }
        let b = xirius_clap::Bundle::load(bundle.as_ref()).map_err(|e| e.to_string())?;
        self.claps.insert(bundle.to_string(), b.clone());
        Ok(b)
    }

    /// The plugin the slot names, and its name. A `.clap` bundle's plugin is found by the key
    /// of its id; the first plugin when none is named.
    fn create(&mut self, o: &OpenSlot) -> Result<(Loaded, String), String> {
        if xirius_clap::is_clap(o.bundle.as_ref()) {
            let bundle = self.clap_bundle(&o.bundle)?;
            let d = bundle
                .find_key(o.class)
                .ok_or_else(|| format!("{} に読み込めるプラグインがありません", o.bundle))?;
            let plugin = xirius_clap::Plugin::new(&bundle, &d.id).map_err(|e| e.to_string())?;
            return Ok((Loaded::Clap(Box::new(plugin)), d.name.clone()));
        }
        let module = self.module(&o.bundle)?;
        let class = module
            .classes()
            .into_iter()
            .find(|c| match o.class {
                Some(cid) => c.cid == cid,
                None => c.is_audio_processor(),
            })
            .ok_or_else(|| format!("{} に読み込めるクラスがありません", o.bundle))?;
        let plugin = Plugin::new(module, &class).map_err(|e| e.to_string())?;
        Ok((Loaded::Vst3(plugin), class.name))
    }

    fn open(&mut self, o: OpenSlot) -> Result<(Instance, Opened), String> {
        xirius_engine::crash::note(&format!(
            "slot={} bundle={} class={:?}",
            o.slot, o.bundle, o.class
        ));
        let rings = Arc::new(SharedRings::open(&o.shm).map_err(|e| e.to_string())?);
        xirius_engine::crash::note(&format!(
            "slot={} sample_rate={} max_block={} offline={}",
            o.slot,
            rings.header().sample_rate,
            rings.header().max_block,
            rings.header().offline
        ));
        let _in = InPlugin::enter(&rings, o.epoch);
        let req = Event::open(&o.req).map_err(|e| e.to_string())?;
        let done = Event::open(&o.done).map_err(|e| e.to_string())?;
        let (mut plugin, name) = self.create(&o)?;
        if let Some(state) = o.state.as_ref().filter(|s| !s.component.is_empty()) {
            plugin
                .set_state(state)
                .map_err(|e| format!("状態を復元できません: {e}"))?;
        }
        let mut warnings = Vec::new();
        for (key, value) in &o.params {
            match plugin.resolve(key) {
                Some(id) => plugin.set_param(id, value.clamp(0.0, 1.0)),
                None => warnings.push(format!("パラメーター {key} が見つかりません")),
            }
        }
        let h = rings.header();
        let offline = h.offline != 0;
        let max_block = h.max_block as usize;
        let (audio, latency) = plugin.audio(
            h.sample_rate,
            max_block,
            offline,
            xirius_engine::sandbox::NOTE_CAPACITY,
        )?;
        // Setting up is not the user's editing.
        plugin.take_gestures();
        plugin.take_changes();

        // Line the rings up: input from where the engine is now, and `L` frames of silence
        // ahead of where it reads.
        h.in_read
            .store(h.in_write.load(Ordering::Acquire), Ordering::Release);
        rings.skip_notes();
        let out_read = h.out_read.load(Ordering::Acquire);
        // SAFETY: the engine does not touch the rings until `ready_epoch` names this instance.
        unsafe { rings.write_silence(Ring::Out, out_read, h.latency as usize) };
        h.out_write
            .store(out_read + h.latency as u64, Ordering::Release);
        h.plugin_latency.store(latency, Ordering::Release);

        let stop = Arc::new(AtomicBool::new(false));
        let fault = Arc::new(FaultCtl::default());
        let (up, is_up) = mpsc::channel();
        let thread = {
            let (rings, stop, fault, epoch) = (rings.clone(), stop.clone(), fault.clone(), o.epoch);
            std::thread::Builder::new()
                .name(format!("xirius-plugin-audio-{}", o.slot))
                .spawn(move || {
                    let _in = InPlugin::enter(&rings, epoch);
                    run_audio(audio, &rings, &req, &done, &stop, &fault, up)
                })
                .map_err(|e| e.to_string())?
        };
        // The engine counts the time a ready instance is late from its first block. A new
        // thread can wait long on a busy machine before it first runs at the audio priority, and
        // that wait is not the plugin's: the instance is ready once its audio thread is.
        is_up
            .recv()
            .map_err(|_| "プラグインの音声のスレッドが始まりません".to_string())?;
        h.ready_epoch.store(o.epoch, Ordering::Release);

        let note_names = plugin.note_names();
        let opened = Opened {
            name,
            latency,
            params: plugin.params(),
            warnings,
            note_names: note_names.clone(),
        };
        let inst = Instance {
            plugin,
            editor: None,
            audio: Some(thread),
            stop,
            fault,
            rings,
            epoch: o.epoch,
            capture: None,
            capture_request: None,
            note_names,
        };
        Ok((inst, opened))
    }

    /// Reports what happened in the plugins since the last call.
    fn poll(&mut self) {
        let mut out = Vec::new();
        for (&slot, inst) in &mut self.instances {
            let _in = inst.enter();
            for g in inst.plugin.take_gestures() {
                if matches!(g, Gesture::End(_)) {
                    inst.capture = Some((
                        inst.blocks() + CAPTURE_BLOCKS,
                        Instant::now() + CAPTURE_WAIT,
                    ));
                }
                out.push(FromHost::Gesture { slot, gesture: g });
            }
            let changes = inst.plugin.take_changes();
            if changes.values && inst.capture.is_none() {
                inst.capture = Some((
                    inst.blocks() + CAPTURE_BLOCKS,
                    Instant::now() + CAPTURE_WAIT,
                ));
            }
            let latency_changed = changes.latency;
            let mut names_changed = changes.note_names;
            if inst.capture_request.is_some()
                || inst.capture.is_some_and(|(blocks, at)| {
                    latency_changed || inst.blocks() >= blocks || Instant::now() >= at
                })
            {
                inst.capture = None;
                let result = inst.plugin.get_state();
                if let Some(request) = inst.capture_request.take() {
                    out.push(FromHost::CapturedState {
                        slot,
                        request,
                        result,
                    });
                } else {
                    match result {
                        Ok(state) => out.push(FromHost::State { slot, state }),
                        Err(e) => eprintln!("xirius-plugin-host: slot {slot}: {e}"),
                    }
                }
                // A preset chosen in the plugin may name its keys anew.
                names_changed = true;
            }
            if names_changed && let Some(msg) = inst.reread_note_names(slot) {
                out.push(msg);
            }
            if latency_changed {
                out.push(FromHost::Latency {
                    slot,
                    latency: inst.plugin.latency(),
                });
            }
            if let Some(editor) = &inst.editor {
                for _ in 0..editor.take_play_requests() {
                    out.push(FromHost::PlayKey { slot });
                }
            }
            if inst.editor.as_ref().is_some_and(|e| !e.is_visible()) {
                inst.editor = None;
                // The patch may have been changed in the window without a word about it.
                inst.capture = Some((0, Instant::now()));
                out.push(FromHost::EditorClosed { slot });
            }
        }
        for msg in &out {
            self.send(msg);
        }
    }

    fn pump(&mut self) -> bool {
        let open = self
            .instances
            .values()
            .filter(|i| i.editor.is_some())
            .count();
        let Some(inst) = self.instances.values_mut().find(|i| i.editor.is_some()) else {
            return false;
        };
        // Pumping runs every editor's window; a crash in it is one plugin's only when one is open.
        let _in = (open == 1).then(|| inst.enter());
        if let Some(e) = inst.editor.as_mut() {
            e.pump(Duration::ZERO);
        }
        true
    }
}

/// One instance's audio thread: answers the engine's blocks until told to stop. Says on `up`
/// when it runs at its priority.
fn run_audio(
    mut audio: Audio,
    rings: &SharedRings,
    req: &Event,
    done: &Event,
    stop: &AtomicBool,
    fault: &FaultCtl,
    up: mpsc::Sender<()>,
) -> Audio {
    let h = rings.header();
    if h.offline == 0 {
        let mut task = 0u32;
        let name: Vec<u16> = "Pro Audio".encode_utf16().chain(Some(0)).collect();
        // SAFETY: the name is NUL-terminated. Failing leaves a normal priority.
        unsafe { AvSetMmThreadCharacteristicsW(name.as_ptr(), &mut task) };
    }
    let _ = up.send(());
    let max_block = h.max_block as usize;
    let capacity = rings.capacity() as u64;
    let mut l = vec![0.0f32; max_block];
    let mut r = vec![0.0f32; max_block];
    let mut notes: Vec<Note> = Vec::with_capacity(xirius_engine::sandbox::NOTE_CAPACITY);
    let mut controls: Vec<Control> = Vec::with_capacity(xirius_engine::sandbox::NOTE_CAPACITY);
    let mut params: Vec<ParamChange> = Vec::with_capacity(xirius_engine::sandbox::NOTE_CAPACITY);
    while !stop.load(Ordering::Acquire) {
        req.wait(50);
        loop {
            let read = h.in_read.load(Ordering::Relaxed);
            let n = (h.in_write.load(Ordering::Acquire) - read).min(max_block as u64) as usize;
            if n == 0 || stop.load(Ordering::Acquire) {
                break;
            }
            // SAFETY: we are the only reader of the in ring; the engine published the frames.
            unsafe { rings.read(Ring::In, read, &mut l[..n], &mut r[..n]) };
            notes.clear();
            controls.clear();
            params.clear();
            for _ in 0..xirius_engine::sandbox::NOTE_CAPACITY {
                let Some(nr) = rings.take_note_before(read + n as u64) else {
                    break;
                };
                let offset = nr.frame.saturating_sub(read).min(n as u64 - 1) as u32;
                if nr.on == NOTE_PARAM {
                    params.push(ParamChange {
                        offset,
                        id: nr.param_id,
                        value: nr.param_value,
                    });
                } else if nr.on == NOTE_CONTROL {
                    controls.push(Control {
                        offset,
                        number: nr.pitch,
                        value: nr.velocity as i32,
                    });
                } else {
                    notes.push(Note {
                        offset,
                        pitch: nr.pitch,
                        velocity: nr.velocity,
                        on: nr.on != 0,
                    });
                }
            }
            let start = Instant::now();
            audio.process(&mut l[..n], &mut r[..n], &notes, &controls, &params);
            let ns = start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
            h.host_ns.fetch_add(ns, Ordering::Relaxed);
            let write = h.out_write.load(Ordering::Relaxed);
            while write + n as u64 - h.out_read.load(Ordering::Acquire) > capacity {
                if stop.load(Ordering::Acquire) {
                    return audio;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            // SAFETY: we are the only writer of the out ring and the frames are free.
            unsafe { rings.write(Ring::Out, write, &l[..n], &r[..n]) };
            h.out_write.store(write + n as u64, Ordering::Release);
            h.in_read.store(read + n as u64, Ordering::Release);
            let blocks = h.host_blocks.fetch_add(1, Ordering::AcqRel) + 1;
            done.set();
            let kind = fault.kind.load(Ordering::Acquire);
            if kind != 0 && blocks >= fault.at.load(Ordering::Acquire) {
                if kind == 1 {
                    // SAFETY: raises a real access violation, as a crashing plugin would.
                    unsafe {
                        RaiseException(EXCEPTION_ACCESS_VIOLATION as u32, 1, 0, std::ptr::null())
                    };
                }
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    }
    audio
}

fn main() {
    if let Err(e) = xirius_engine::crash::install("plugin-host", false) {
        eprintln!("クラッシュの記録先を準備できません: {e}");
    }
    // A crash must not put a dialog in front of the user; the engine notices it by itself.
    // SAFETY: no preconditions.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
        WerSetFlags(WER_FAULT_REPORTING_NO_UI);
        SetUnhandledExceptionFilter(Some(on_crash));
        // A plugin's window is drawn at the screen's scale, as in the app, not stretched by
        // the system (blurred) from 100 %.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    // For tests of the engine's check: claim another protocol, or none at all ("none", as a
    // host from before the check, which did not know `--protocol`).
    let claim = std::env::var("XIRIUS_PLUGIN_HOST_PROTOCOL").ok();
    let old = claim.as_deref() == Some("none");
    let protocol = claim
        .as_deref()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(wire::PROTOCOL);
    let mut parent_pid = None;
    let mut asked = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--parent" => parent_pid = args.next().and_then(|v| v.parse::<u32>().ok()),
            "--protocol" if !old => asked = args.next().and_then(|v| v.parse::<u32>().ok()),
            _ => {
                eprintln!("xirius-plugin-host: unknown argument {a}");
                std::process::exit(2);
            }
        }
    }
    let Some(parent_pid) = parent_pid else {
        eprintln!("xirius-plugin-host: needs --parent PID");
        std::process::exit(2);
    };
    // SAFETY: no preconditions; a null handle is checked below.
    let parent = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, parent_pid) };
    if parent.is_null() {
        eprintln!("xirius-plugin-host: {}", io::Error::last_os_error());
        std::process::exit(2);
    }

    let Some(asked) = asked else {
        eprintln!("xirius-plugin-host: needs --protocol N (the engine is older than this host)");
        std::process::exit(2);
    };

    let out = take_stdout();
    let mut host = Host {
        out,
        modules: HashMap::new(),
        claps: HashMap::new(),
        instances: HashMap::new(),
    };
    host.send(&FromHost::Hello {
        protocol,
        build: env!("CARGO_PKG_VERSION").to_string(),
    });
    if asked != protocol {
        eprintln!("xirius-plugin-host: the engine speaks protocol {asked}, this host {protocol}");
        std::process::exit(3);
    }
    let (tx, rx) = mpsc::channel::<ToHost>();
    std::thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        while let Ok(Some(buf)) = wire::recv(&mut stdin) {
            match ToHost::decode(&buf) {
                Ok(msg) => {
                    if tx.send(msg).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    eprintln!("xirius-plugin-host: {e}");
                    return;
                }
            }
        }
    });

    loop {
        let wait = if host.pump() { 5 } else { 10 };
        match rx.recv_timeout(Duration::from_millis(wait)) {
            Ok(msg) => {
                host.handle(msg);
                while let Ok(msg) = rx.try_recv() {
                    host.handle(msg);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        host.poll();
        let _ = host.out.flush();
        // SAFETY: a valid process handle.
        if unsafe { WaitForSingleObject(parent, 0) } == WAIT_OBJECT_0 {
            break;
        }
    }
    for (_, inst) in host.instances.drain() {
        inst.close();
    }
}
