//! One CLAP instance. Its `[main-thread]` calls run on a thread the instance owns; the handle
//! sends them there and waits, so it can be used from any thread.

use std::ffi::CString;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use clack_extensions::audio_ports::PluginAudioPorts;
use clack_extensions::latency::PluginLatency;
use clack_extensions::note_name::{NoteNameBuffer, PluginNoteName};
use clack_extensions::note_ports::PluginNotePorts;
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags, PluginParams};
use clack_extensions::render::PluginRender;
use clack_extensions::state::PluginState;
use clack_host::events::Match;
use clack_host::prelude::*;
use clack_host::utils::Cookie;

use crate::audio::{AudioPart, value_event};
use crate::bundle::{Bundle, Descriptor};
use crate::host::{Flags, HostMain, HostShared, XiriusHost};
use crate::{Error, error};

/// Edits queued from other threads to the processor between two blocks.
const EDIT_QUEUE: usize = 1024;
/// Gestures kept until [`Plugin::take_gestures`] collects them; newer ones are dropped past this.
const MAX_GESTURES: usize = 4096;
/// How often the main thread looks at the plugin's requests when no call comes.
const IDLE: Duration = Duration::from_millis(10);

/// One step of a user's edit in the plugin's editor (a knob grabbed, moved and let go), with
/// normalized values as in `xirius-vst3`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    Begin(u32),
    Perform(u32, f64),
    End(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamInfo {
    pub id: u32,
    pub title: String,
    /// Where the plugin files the parameter, with `/` between levels.
    pub module: String,
    pub min: f64,
    pub max: f64,
    /// In the plugin's own range.
    pub default: f64,
    /// `CLAP_PARAM_*` flags.
    pub flags: u32,
    /// The plugin's own pointer for the parameter, handed back with each value event. Some
    /// plugins (clap-juce-extensions) read it without checking for null.
    pub(crate) cookie: Cookie,
}

impl ParamInfo {
    pub fn is_stepped(&self) -> bool {
        self.flags & ParamInfoFlags::IS_STEPPED.bits() != 0
    }

    pub fn is_hidden(&self) -> bool {
        self.flags & ParamInfoFlags::IS_HIDDEN.bits() != 0
    }

    pub fn is_read_only(&self) -> bool {
        self.flags & ParamInfoFlags::IS_READONLY.bits() != 0
    }

    pub fn is_automatable(&self) -> bool {
        self.flags & ParamInfoFlags::IS_AUTOMATABLE.bits() != 0
    }

    /// Like a VST3 step count: the steps between the ends, or 0 when continuous.
    pub fn step_count(&self) -> i32 {
        if self.is_stepped() {
            (self.max - self.min).round().max(0.0) as i32
        } else {
            0
        }
    }

    pub fn normalize(&self, plain: f64) -> f64 {
        if self.max > self.min {
            ((plain - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn plain(&self, normalized: f64) -> f64 {
        let v = self.min + normalized.clamp(0.0, 1.0) * (self.max - self.min);
        if self.is_stepped() { v.round() } else { v }
    }

    pub fn default_normalized(&self) -> f64 {
        self.normalize(self.default)
    }
}

pub(crate) type Job = Box<dyn FnOnce(&mut Main) + Send>;

/// What lives on the instance's main thread.
pub(crate) struct Main {
    pub(crate) instance: PluginInstance<XiriusHost>,
    pub(crate) params: Option<PluginParams>,
    pub(crate) state: Option<PluginState>,
    pub(crate) latency: Option<PluginLatency>,
    pub(crate) audio_ports: Option<PluginAudioPorts>,
    pub(crate) note_ports: Option<PluginNotePorts>,
    pub(crate) note_name: Option<PluginNoteName>,
    pub(crate) render: Option<PluginRender>,
    pub(crate) flags: Arc<Flags>,
}

impl Main {
    fn new(mut instance: PluginInstance<XiriusHost>, flags: Arc<Flags>) -> Main {
        let h = instance.plugin_handle();
        Main {
            params: h.get_extension(),
            state: h.get_extension(),
            latency: h.get_extension(),
            audio_ports: h.get_extension(),
            note_ports: h.get_extension(),
            note_name: h.get_extension(),
            render: h.get_extension(),
            instance,
            flags,
        }
    }

    pub(crate) fn param_infos(&mut self) -> Vec<ParamInfo> {
        let Some(params) = self.params else {
            return Vec::new();
        };
        let h = self.instance.plugin_handle();
        let mut buf = ParamInfoBuffer::new();
        let text = |b: &[u8]| {
            String::from_utf8_lossy(b)
                .trim_end_matches('\0')
                .to_string()
        };
        (0..params.count(&h))
            .filter_map(|i| {
                let info = params.get_info(&h, i, &mut buf)?;
                Some(ParamInfo {
                    id: info.id.get(),
                    title: text(info.name),
                    module: text(info.module),
                    min: info.min_value,
                    max: info.max_value,
                    default: info.default_value,
                    flags: info.flags.bits(),
                    cookie: info.cookie,
                })
            })
            .collect()
    }

    /// The names the plugin gives its keys (`clap.note-name`), by key, lowest first. A name
    /// for one port or channel only counts when it is the first port or channel; the first
    /// name of a key wins.
    pub(crate) fn note_names(&mut self) -> Vec<(u8, String)> {
        let Some(ext) = self.note_name else {
            return Vec::new();
        };
        let h = self.instance.plugin_handle();
        let mut buf = NoteNameBuffer::new();
        let mut out: Vec<(u8, String)> = Vec::new();
        for i in 0..ext.count(&h) {
            let Some(n) = ext.get(&h, i as u32, &mut buf) else {
                continue;
            };
            let first = |m: Match<u16>| m.into_specific().is_none_or(|v| v == 0);
            let Some(key) = n.key.into_specific().and_then(|k| u8::try_from(k).ok()) else {
                continue;
            };
            let name = String::from_utf8_lossy(n.name)
                .trim_end_matches('\0')
                .trim()
                .to_string();
            if key > 127 || name.is_empty() || !first(n.port) || !first(n.channel) {
                continue;
            }
            if !out.iter().any(|(k, _)| *k == key) {
                out.push((key, name));
            }
        }
        out.sort_by_key(|(k, _)| *k);
        out
    }

    /// Hands plain parameter values to an inactive plugin.
    fn flush(&mut self, values: &[(u32, f64, Cookie)]) -> bool {
        let Some(params) = self.params else {
            return false;
        };
        let Some(mut h) = self.instance.inactive_plugin_handle() else {
            return false;
        };
        let mut input = EventBuffer::with_capacity(values.len());
        for &(id, v, cookie) in values {
            input.push(&value_event(0, id, v, cookie));
        }
        let mut output = EventBuffer::new();
        params.flush(&mut h, &input.as_input(), &mut output.as_output());
        true
    }

    pub(crate) fn read_latency(&mut self) -> u32 {
        let Some(latency) = self.latency else {
            return 0;
        };
        // The spec only allows it while active (or being activated).
        if !self.instance.is_active() {
            return self.flags.latency.load(Ordering::Acquire);
        }
        let v = latency.get(&self.instance.plugin_handle());
        self.flags.latency.store(v, Ordering::Release);
        v
    }

    /// Answers what the plugin asked for from other threads.
    fn service(&mut self) {
        if Flags::take(&self.flags.callback) {
            self.instance.call_on_main_thread_callback();
        }
        if !self.instance.is_active() && Flags::take(&self.flags.flush) {
            self.flush(&[]);
        }
        if self.flags.latency_changed.load(Ordering::Acquire) {
            self.read_latency();
        }
    }
}

fn run(main: &mut Main, jobs: mpsc::Receiver<Job>) {
    loop {
        match jobs.recv_timeout(IDLE) {
            Ok(job) => job(main),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        main.service();
    }
    // Every handle is gone, so the processor came back (or was dropped) before this.
    if main.instance.is_active() {
        let _ = main.instance.try_deactivate();
    }
}

/// One plugin instance.
pub struct Plugin {
    descriptor: Descriptor,
    jobs: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
    flags: Arc<Flags>,
    params: Vec<ParamInfo>,
    edits: Mutex<Option<rtrb::Producer<(u32, f64)>>>,
    gestures: Mutex<Option<rtrb::Consumer<Gesture>>>,
    /// Gestures made on this side ([`edit_like_editor`](Self::edit_like_editor)).
    own_gestures: Mutex<Vec<Gesture>>,
}

impl Plugin {
    /// Creates the plugin `id` from `bundle`, on a new main thread.
    pub fn new(bundle: &Bundle, id: &str) -> Result<Plugin, Error> {
        let descriptor = bundle
            .find(id)
            .cloned()
            .ok_or_else(|| error(format!("{}: no plugin {id}", bundle.path().display())))?;
        let entry = bundle.entry.clone();
        let cid = CString::new(id).map_err(|_| error("the plugin id has a NUL"))?;
        let flags = Arc::new(Flags::default());
        let (jobs, rx) = mpsc::channel::<Job>();
        let (ready, ready_rx) = mpsc::sync_channel(1);
        let thread_flags = flags.clone();
        let thread = std::thread::Builder::new()
            .name(format!("clap main {}", descriptor.name))
            .spawn(move || {
                let made = HostInfo::new(
                    "Xirius",
                    "Xirius",
                    "https://github.com/tekalu1/xirius",
                    env!("CARGO_PKG_VERSION"),
                )
                .map_err(|e| error(e.to_string()))
                .and_then(|host| {
                    let (fs, fm) = (thread_flags.clone(), thread_flags.clone());
                    PluginInstance::<XiriusHost>::new(
                        |_| HostShared { flags: fs },
                        |_| HostMain { flags: fm },
                        &entry,
                        &cid,
                        &host,
                    )
                    .map_err(|e| error(format!("creating the plugin: {e}")))
                });
                match made {
                    Ok(instance) => {
                        let mut main = Main::new(instance, thread_flags);
                        let _ = ready.send(Ok(main.param_infos()));
                        run(&mut main, rx);
                    }
                    Err(e) => {
                        let _ = ready.send(Err(e));
                    }
                }
            })
            .map_err(|e| error(format!("starting the plugin's thread: {e}")))?;
        let params = match ready_rx.recv() {
            Ok(Ok(params)) => params,
            Ok(Err(e)) => {
                let _ = thread.join();
                return Err(e);
            }
            Err(_) => {
                let _ = thread.join();
                return Err(error("the plugin's thread ended while creating it"));
            }
        };
        Ok(Plugin {
            descriptor,
            jobs: Some(jobs),
            thread: Some(thread),
            flags,
            params,
            edits: Mutex::new(None),
            gestures: Mutex::new(None),
            own_gestures: Mutex::new(Vec::new()),
        })
    }

    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }

    pub fn name(&self) -> &str {
        &self.descriptor.name
    }

    /// Runs `f` on the instance's main thread and waits for it.
    pub(crate) fn call<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Main) -> R + Send + 'static,
    ) -> Result<R, Error> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.jobs
            .as_ref()
            .ok_or_else(|| error("the plugin is closed"))?
            .send(Box::new(move |m| {
                let _ = tx.send(f(m));
            }))
            .map_err(|_| error("the plugin's main thread is gone"))?;
        rx.recv()
            .map_err(|_| error("the plugin's main thread is gone"))
    }

    /// The parameters as read at creation, or at the last [`rescan_params`](Self::rescan_params).
    pub fn params(&self) -> &[ParamInfo] {
        &self.params
    }

    pub fn param_info(&self, id: u32) -> Option<&ParamInfo> {
        self.params.iter().find(|p| p.id == id)
    }

    /// Reads the parameter list again, after the plugin said it changed.
    pub fn rescan_params(&mut self) -> Result<(), Error> {
        self.params = self.call(|m| m.param_infos())?;
        Ok(())
    }

    /// The value, normalized to 0..1.
    pub fn param(&self, id: u32) -> Option<f64> {
        let info = self.param_info(id)?;
        self.param_plain(id).map(|v| info.normalize(v))
    }

    /// The value in the plugin's own range.
    pub fn param_plain(&self, id: u32) -> Option<f64> {
        self.call(move |m| {
            let params = m.params?;
            params.get_value(&m.instance.plugin_handle(), ClapId::new(id))
        })
        .ok()
        .flatten()
    }

    /// How the plugin writes a normalized value.
    pub fn param_text(&self, id: u32, normalized: f64) -> Option<String> {
        let plain = self.param_info(id)?.plain(normalized);
        self.call(move |m| {
            let params = m.params?;
            let mut buf = [0u8; 256];
            let text = params
                .value_to_text(
                    &m.instance.plugin_handle(),
                    ClapId::new(id),
                    plain,
                    &mut buf,
                )
                .ok()?;
            Some(String::from_utf8_lossy(text).into_owned())
        })
        .ok()
        .flatten()
    }

    /// Sets a parameter (normalized). While active it reaches the processor at its next
    /// block, else the plugin gets it through `flush` now.
    pub fn set_param(&self, id: u32, normalized: f64) -> bool {
        let Some(info) = self.param_info(id) else {
            return false;
        };
        let (plain, cookie) = (info.plain(normalized), info.cookie);
        // Once the audio part is gone its queue is no longer read, though the deactivation it
        // asked the main thread for may still be pending: then the edit is flushed after it.
        if self.flags.active.load(Ordering::Acquire)
            && self.flags.audio_alive.load(Ordering::Acquire)
            && let Ok(mut edits) = self.edits.lock()
            && let Some(p) = edits.as_mut()
        {
            return p.push((id, plain)).is_ok();
        }
        self.call(move |m| m.flush(&[(id, plain, cookie)]))
            .unwrap_or(false)
    }

    /// Moves a parameter the way the plugin's editor would: a begin, each value, an end, and
    /// the values set. For tests that need an editor's gesture without a window.
    pub fn edit_like_editor(&self, id: u32, values: &[f64]) {
        if let Ok(mut own) = self.own_gestures.lock() {
            own.push(Gesture::Begin(id));
            own.extend(values.iter().map(|&v| Gesture::Perform(id, v)));
            own.push(Gesture::End(id));
        }
        for &v in values {
            self.set_param(id, v);
        }
    }

    /// The plugin's latency in frames: as read at activation, or after it reported a change.
    pub fn latency(&self) -> u32 {
        self.call(|m| m.read_latency())
            .unwrap_or_else(|_| self.flags.latency.load(Ordering::Acquire))
    }

    pub fn get_state(&self) -> Result<Vec<u8>, Error> {
        self.call(|m| {
            let state = m.state.ok_or_else(|| error("the plugin keeps no state"))?;
            let mut out = Vec::new();
            state
                .save(&m.instance.plugin_handle(), &mut out)
                .map_err(|e| error(e.to_string()))?;
            Ok(out)
        })?
    }

    pub fn set_state(&self, data: &[u8]) -> Result<(), Error> {
        let data = data.to_vec();
        self.call(move |m| {
            let state = m.state.ok_or_else(|| error("the plugin keeps no state"))?;
            state
                .load(&m.instance.plugin_handle(), &mut data.as_slice())
                .map_err(|e| error(e.to_string()))
        })??;
        // A plugin asks for a rescan (or marks itself dirty) after a load, as CLAP wants; the
        // host made that change itself, so it is not an edit to report.
        Flags::take(&self.flags.params_changed);
        Flags::take(&self.flags.dirty);
        Ok(())
    }

    /// Edits the plugin's editor made since the last call, oldest first.
    pub fn take_gestures(&self) -> Vec<Gesture> {
        let mut out = self
            .own_gestures
            .lock()
            .map(|mut g| std::mem::take(&mut *g))
            .unwrap_or_default();
        if let Ok(mut ring) = self.gestures.lock()
            && let Some(c) = ring.as_mut()
        {
            while let Ok(g) = c.pop() {
                out.push(g);
            }
        }
        out
    }

    /// Whether the plugin said its state changed since the last call.
    pub fn take_dirty(&self) -> bool {
        Flags::take(&self.flags.dirty)
    }

    /// Whether the plugin said its latency changed since the last call.
    pub fn take_latency_changed(&self) -> bool {
        Flags::take(&self.flags.latency_changed)
    }

    /// Whether the plugin asked to be deactivated and activated again.
    pub fn take_restart(&self) -> bool {
        Flags::take(&self.flags.restart)
    }

    /// The names the plugin gives its keys (a drum kit's sounds), lowest key first; empty when
    /// it names none.
    pub fn note_names(&self) -> Vec<(u8, String)> {
        self.call(|m| m.note_names()).unwrap_or_default()
    }

    /// Whether the plugin said its note names changed since the last call.
    pub fn take_note_names_changed(&self) -> bool {
        Flags::take(&self.flags.note_names_changed)
    }

    /// Whether the plugin said its parameter list changed.
    pub fn take_params_changed(&self) -> bool {
        Flags::take(&self.flags.params_changed)
    }

    /// The real-time half. There is one at a time; it activates on
    /// [`AudioPart::prepare`].
    pub fn audio(&mut self) -> Result<AudioPart, Error> {
        if self.flags.audio_alive.swap(true, Ordering::AcqRel) {
            return Err(error("the plugin's audio part is already out"));
        }
        let jobs = self
            .jobs
            .clone()
            .ok_or_else(|| error("the plugin is closed"))?;
        let (edit_tx, edit_rx) = rtrb::RingBuffer::new(EDIT_QUEUE);
        let (gesture_tx, gesture_rx) = rtrb::RingBuffer::new(MAX_GESTURES);
        if let Ok(mut e) = self.edits.lock() {
            *e = Some(edit_tx);
        }
        if let Ok(mut g) = self.gestures.lock() {
            *g = Some(gesture_rx);
        }
        Ok(AudioPart::new(
            self.descriptor.name.clone(),
            jobs,
            self.flags.clone(),
            self.params.clone(),
            edit_rx,
            gesture_tx,
        ))
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        // The main thread ends when the audio part is gone too; wait for it unless that part
        // is still out (it then ends with the part).
        self.jobs = None;
        if let Some(t) = self.thread.take()
            && !self.flags.audio_alive.load(Ordering::Acquire)
        {
            let _ = t.join();
        }
    }
}
