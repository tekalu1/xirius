//! One controller thread per local instance (PRJ-030).
//!
//! SDK objects are constructed inside the thread and never leave it. Requests and
//! replies contain only owned, Send data. Dropping a ticket abandons its result;
//! it never joins or attempts to terminate third-party code. The audio lease is
//! returned through a preallocated single-item queue before the plugin is freed.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::time::Duration;

use crate::{AudioPart, Caption, Editor, Gesture, Module, ParamInfo, ParamSender, Placement};
use crate::{Plugin, PluginState};

const QUEUED: usize = 64;

/// No SDK objects, module handles or thread-affine guards may be put here.
pub struct Open {
    pub bundle: PathBuf,
    pub class: [u8; 16],
    pub state: Option<PluginState>,
    pub params: Vec<(String, f64)>,
    /// Compatibility information supplied by the host, never inferred from STA.
    pub requires_process_main: bool,
    pub sample_rate: f64,
    pub max_block: usize,
    pub offline: bool,
}

pub struct Opened {
    pub controller: Controller,
    pub audio: AudioLease,
    pub params: Vec<ParamInfo>,
    pub note_names: Vec<(u8, String)>,
    pub sender: ParamSender,
    pub retirement: Retirement,
}

#[derive(Clone)]
pub struct Retirement(Arc<AtomicBool>);

impl Retirement {
    pub fn finished(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

struct Finished(Retirement);

impl Drop for Finished {
    fn drop(&mut self) {
        self.0.0.store(true, Ordering::Release);
    }
}

/// Each request has its own reply channel. A late reply cannot satisfy a newer
/// capture or a request to another instance with the same song plugin id.
pub struct Ticket<T> {
    reply: Receiver<Result<T, String>>,
    abandoned: Arc<AtomicBool>,
    consumed: Cell<bool>,
}

impl<T> Ticket<T> {
    /// Nonblocking, including while initialize/getState/editor/terminate hangs.
    pub fn poll(&self) -> Option<Result<T, String>> {
        if self.consumed.get() {
            return None;
        }
        if self.abandoned.load(Ordering::Acquire) {
            self.consumed.set(true);
            return Some(Err("プラグインへの要求を中止しました".into()));
        }
        let result = match self.reply.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err("プラグインの所有スレッドが停止しました".into()))
            }
        };
        self.consumed.set(result.is_some());
        result
    }

    pub fn cancel(&self) {
        self.abandoned.store(true, Ordering::Release);
    }
}

impl<T> Drop for Ticket<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}

type Job = Box<dyn FnOnce(&mut Instance) + Send>;

#[derive(Clone)]
pub struct Controller {
    commands: SyncSender<Job>,
}

/// The processor borrows this part. Returning it performs no allocation or lock.
/// The engine must retire its plan off RT before dropping the processor's other
/// buffers. This lease alone is not permission to destroy a plan on RT.
pub struct AudioLease {
    part: Option<AudioPart>,
    returned: rtrb::Producer<AudioPart>,
    prepared: (f64, usize, bool, u32),
}

impl AudioLease {
    pub fn prepared(&self, rate: f64, block: usize, offline: bool) -> Result<u32, crate::Error> {
        let (r, b, o, latency) = self.prepared;
        if (rate, block, offline) == (r, b, o) {
            Ok(latency)
        } else {
            Err(crate::error(
                "the owner's prepared audio setup does not match",
            ))
        }
    }
    pub fn name(&self) -> &str {
        self.part
            .as_ref()
            .expect("audio lease returned only on drop")
            .name()
    }

    /// Only the RT processing API crosses the owner boundary. In particular the
    /// caller cannot reconfigure, deactivate or extract the underlying AudioPart.
    pub fn process_with(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        notes: &[crate::Note],
        controls: &[crate::Control],
    ) {
        self.part
            .as_mut()
            .expect("audio lease returned only on drop")
            .process_with(left, right, notes, controls);
    }
}

impl Drop for AudioLease {
    fn drop(&mut self) {
        if let Some(part) = self.part.take()
            && let Err(rtrb::PushError::Full(part)) = self.returned.push(part)
        {
            // One part and one reserved slot: unreachable in normal operation.
            // Never fall back to releasing SDK objects on the calling thread.
            std::mem::forget(part);
        }
    }
}

pub struct Edits {
    pub gestures: Vec<Gesture>,
    pub restart_flags: i32,
    pub note_names: Vec<(u8, String)>,
    pub editor_visible: bool,
    pub play_requests: u32,
}

struct Instance {
    // Drop the editor before the plugin, on this thread.
    editor: Option<Editor>,
    plugin: Plugin,
}

struct ReturnGuard {
    returning: rtrb::Consumer<AudioPart>,
    received: bool,
}

impl ReturnGuard {
    fn poll(&mut self) {
        if let Ok(part) = self.returning.pop() {
            drop(part);
            self.received = true;
        }
    }
}

impl Drop for ReturnGuard {
    fn drop(&mut self) {
        // Also runs during a Rust unwind in a command. Keep the plugin's owning
        // reference alive until the audio half returns. No other thread joins us.
        while !self.received {
            self.poll();
            if !self.received {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

impl Controller {
    /// Starts the owner and returns immediately. Thread creation is the only
    /// synchronous fallible work; DLL loading and initial getState happen there.
    pub fn open(open: Open) -> Result<Ticket<Opened>, String> {
        let (reply, ticket) = reply();
        let cancelled = ticket.abandoned.clone();
        std::thread::Builder::new()
            .name("vst3-owner".into())
            .spawn(move || {
                let finished = Finished(Retirement(Arc::new(AtomicBool::new(false))));
                let apartment = match Apartment::new() {
                    Ok(a) => a,
                    Err(e) => {
                        let _ = reply.send(Err(e));
                        return;
                    }
                };
                if open.requires_process_main {
                    let _ = reply.send(Err(
                        "主スレッドが必要なプラグインです。別プロセスで開く設定を選んでください"
                            .into(),
                    ));
                    return;
                }
                if cancelled.load(Ordering::Acquire) {
                    return;
                }
                let (rate, block, offline) = (open.sample_rate, open.max_block, open.offline);
                let plugin = load(open);
                let mut instance = match plugin {
                    Ok(plugin) => Instance {
                        editor: None,
                        plugin,
                    },
                    Err(e) => {
                        let _ = reply.send(Err(e));
                        return;
                    }
                };
                let mut part = match instance.plugin.audio() {
                    Ok(a) => a,
                    Err(e) => {
                        let _ = reply.send(Err(e.to_string()));
                        return;
                    }
                };
                let latency = match part.prepare(rate, block, offline) {
                    Ok(latency) => latency,
                    Err(e) => {
                        let _ = reply.send(Err(e.to_string()));
                        return;
                    }
                };
                let (commands, incoming) = mpsc::sync_channel(QUEUED);
                let (returned, returning) = rtrb::RingBuffer::new(1);
                // Declared after instance: unwinding returns audio before
                // dropping the last controller-side SDK reference.
                let mut returning = ReturnGuard {
                    returning,
                    received: false,
                };
                let opened = Opened {
                    controller: Controller { commands },
                    params: instance.plugin.params(),
                    note_names: instance.plugin.note_names(),
                    sender: instance.plugin.param_sender(),
                    retirement: finished.0.clone(),
                    audio: AudioLease {
                        part: Some(part),
                        returned,
                        prepared: (rate, block, offline, latency),
                    },
                };
                // Even a ticket abandoned during load returns its audio through
                // the same queue; final release never happens in the receiver.
                let _ = reply.send(Ok(opened));
                drop(reply);
                let mut disconnected = false;
                loop {
                    returning.poll();
                    // Bound the work per turn so hidden HWNDs and OLE keep
                    // receiving messages even with a full command queue.
                    for _ in 0..8 {
                        match incoming.try_recv() {
                            Ok(job) => job(&mut instance),
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                disconnected = true;
                                instance.editor = None;
                                break;
                            }
                        }
                    }
                    pump();
                    if disconnected && returning.received {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                drop(instance);
                drop(apartment);
            })
            .map_err(|e| format!("プラグインの所有スレッドを開始できません: {e}"))?;
        Ok(ticket)
    }

    fn ask<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Instance) -> Result<T, String> + Send + 'static,
    ) -> Result<Ticket<T>, String> {
        let (reply, ticket) = reply();
        let abandoned = ticket.abandoned.clone();
        self.commands
            .try_send(Box::new(move |instance| {
                if !abandoned.load(Ordering::Acquire) {
                    let result = f(instance);
                    if !abandoned.load(Ordering::Acquire) {
                        let _ = reply.send(result);
                    }
                }
            }))
            .map_err(|_| "プラグインが応答しないか要求が満杯です".to_string())?;
        Ok(ticket)
    }

    pub fn capture(&self) -> Result<Ticket<PluginState>, String> {
        self.ask(|i| i.plugin.get_state().map_err(|e| e.to_string()))
    }

    pub fn apply(
        &self,
        state: Option<PluginState>,
        params: Vec<(u32, f64)>,
    ) -> Result<Ticket<()>, String> {
        self.ask(move |i| {
            if let Some(state) = state {
                i.plugin.set_state(&state).map_err(|e| e.to_string())?;
            }
            for (id, value) in params {
                if !i.plugin.set_param(id, value.clamp(0.0, 1.0)) {
                    return Err(format!("パラメーター {id} を変更できません"));
                }
            }
            Ok(())
        })
    }

    pub fn edits(&self) -> Result<Ticket<Edits>, String> {
        self.ask(|i| {
            i.plugin.follow_processor();
            Ok(Edits {
                gestures: i.plugin.handler().take_gestures(),
                restart_flags: i.plugin.handler().take_restart_flags(),
                note_names: i.plugin.note_names(),
                editor_visible: i.editor.as_ref().is_some_and(Editor::is_visible),
                play_requests: i.editor.as_ref().map_or(0, Editor::take_play_requests),
            })
        })
    }

    pub fn editor(&self, title: String, mut place: Placement) -> Result<Ticket<()>, String> {
        // A cross-thread owner HWND can make a hung plugin wait on GPUI. Each
        // instance has an independent top-level frame on its owner thread.
        place.owner = None;
        self.ask(move |i| {
            if let Some(editor) = &i.editor {
                editor.raise();
            } else {
                i.editor = Some(
                    i.plugin
                        .open_editor_at(&title, place)
                        .map_err(|e| e.to_string())?,
                );
            }
            Ok(())
        })
    }

    pub fn close_editor(&self) -> Result<Ticket<()>, String> {
        self.ask(|i| {
            i.editor = None;
            Ok(())
        })
    }

    pub fn caption(&self, caption: Caption) -> Result<Ticket<()>, String> {
        self.ask(move |i| {
            if let Some(editor) = &i.editor {
                editor.set_caption(caption);
            }
            Ok(())
        })
    }
}

fn reply<T>() -> (SyncSender<Result<T, String>>, Ticket<T>) {
    let (sender, reply) = mpsc::sync_channel(1);
    (
        sender,
        Ticket {
            reply,
            abandoned: Arc::new(AtomicBool::new(false)),
            consumed: Cell::new(false),
        },
    )
}

fn load(open: Open) -> Result<Plugin, String> {
    let module = Arc::new(Module::load(&open.bundle).map_err(|e| e.to_string())?);
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.cid == open.class)
        .ok_or_else(|| "プラグインのクラスが見つかりません".to_string())?;
    let plugin = Plugin::new(module, &class).map_err(|e| e.to_string())?;
    if let Some(state) = open.state {
        plugin.set_state(&state).map_err(|e| e.to_string())?;
    }
    let params = plugin.params();
    for (key, value) in open.params {
        let id = key
            .parse::<u32>()
            .ok()
            .filter(|id| params.iter().any(|p| p.id == *id))
            .or_else(|| {
                params
                    .iter()
                    .find(|p| p.title.eq_ignore_ascii_case(&key))
                    .map(|p| p.id)
            })
            .ok_or_else(|| format!("パラメーター {key} が見つかりません"))?;
        plugin.set_param(id, value.clamp(0.0, 1.0));
    }
    plugin.handler().take_gestures();
    plugin.handler().take_restart_flags();
    Ok(plugin)
}

/// OLE is initialized before loading any DLL, and released after all SDK objects.
/// The marker prevents moving the guard to another thread.
pub(crate) struct Apartment(std::marker::PhantomData<*const ()>);

impl Apartment {
    pub(crate) fn new() -> Result<Self, String> {
        #[cfg(windows)]
        {
            // SAFETY: initializes this thread; Drop balances each successful call.
            let result =
                unsafe { windows_sys::Win32::System::Ole::OleInitialize(std::ptr::null()) };
            if result < 0 {
                return Err(format!("プラグインの OLE を初期化できません: {result:#x}"));
            }
        }
        Ok(Self(std::marker::PhantomData))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        #[cfg(windows)]
        // SAFETY: balances the successful initialization on this same thread.
        unsafe {
            windows_sys::Win32::System::Ole::OleUninitialize()
        };
    }
}

fn pump() {
    #[cfg(windows)]
    // SAFETY: drains only the calling owner thread's messages, including hidden
    // windows and thread timers; no GPUI windows are dispatched here.
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let mut message = std::mem::zeroed();
        for _ in 0..64 {
            if PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) == 0 {
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
