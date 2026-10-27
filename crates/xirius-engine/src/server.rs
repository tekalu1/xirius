//! Plugins in a separate process (OPEN-02, PLG-022/023/024/052).
//!
//! A [`PluginServer`] keeps one `xirius-plugin-host` process for all of its plugins and talks
//! to it over the process's stdin and stdout ([`crate::wire`]). A [`RemotePlugin`] is the
//! [`Processor`] the mix holds for one plugin: its audio and notes go through shared memory
//! ([`crate::sandbox`]), and it reaches the host's instance through its [`PluginLink`], which
//! also carries what the instance reports (edits in its editor, state, latency) and counters
//! the UI can read.
//!
//! The audio side never waits in real time. Each block it writes the input into the "in" ring,
//! wakes the host and reads the same number of frames from the "out" ring, which runs `L`
//! frames ahead. The host has that long for each block; the plugin is reported to the mix as
//! `L` plus its own latency, so delay compensation (ENG-010) lines the other tracks up. When
//! the host has not delivered, the block gets the dry input delayed by the same amount, so the
//! sound goes on in time. When the host process ends or stays late for half a second, every
//! plugin of the server plays dry until [`PluginServer::restart`], which starts a new host
//! and opens every instance again with its last state. Offline rendering waits for the host
//! instead, with `L` of one block, so the host works on one block while the mix works on the
//! next (PLG-024) and the result is exact.
//!
//! A host that goes down is put on the plugin that took it down, when that can be told: the
//! host marks the shared memory of the plugin it crashed in, and the audio side knows which
//! plugin it found stalled. [`PluginStatus::faults`] counts them, so a plugin that keeps
//! taking a shared host down can be given a process of its own (PLG-022).

use crate::plugin_updates::{Field, Origin, Stamp, UpdateGate};
use crate::rt_lock::Mutex;
use std::collections::HashMap;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, MutexGuard, OnceLock, Weak};
use std::time::{Duration, Instant};

use xirius_vst3::{Caption, ParamChange, PluginState};

use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor, RenderMode};
use crate::sandbox::{
    Event, NOTE_CAPACITY, NOTE_CONTROL, NOTE_PARAM, NoteRecord, Ring, SharedRings,
};
use crate::wire::{self, Fault, FromHost, Gesture, OpenSlot, Opened, ToHost};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CONTROL_QUEUE: usize = 256;
/// How long the host may take to open one plugin.
const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a starting host may take to say which protocol it speaks (a freshly built
/// executable is read slowly the first time while it is scanned, and a busy machine can hold
/// a starting process back for seconds).
const HELLO_TIMEOUT: Duration = Duration::from_secs(30);
/// Offline rendering gives up on a host that has not answered for this long. A busy machine
/// can hold a working host back for seconds; giving up on it would leave the rest of the
/// render dry.
const OFFLINE_TIMEOUT: Duration = Duration::from_secs(10);
/// Frames over which a restarted plugin takes over from the dry signal.
const FADE: u32 = 256;
/// Parameter edits kept for the caller before the oldest are dropped.
const MAX_PARAM_EDITS: usize = 65_536;
static NEXT_CAPTURE: AtomicU64 = AtomicU64::new(1);
/// The real-time `L` when none is given: two of the session's blocks.
pub const DEFAULT_LATENCY: u32 = 1024;

/// The `xirius-plugin-host` executable: `XIRIUS_PLUGIN_HOST`, else next to this program or
/// one directory up (where `cargo test` puts it).
pub fn find_host() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("XIRIUS_PLUGIN_HOST") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir, dir.parent()?]
        .iter()
        .map(|d| d.join("xirius-plugin-host.exe"))
        .find(|p| p.is_file())
}

// ---------------------------------------------------------------------------------------------
// What a plugin reports.

/// One step of a parameter edit in the plugin's editor, for writing automation (AUT-002):
/// `Begin` when the user grabs a control (Touch starts writing), `Change` for each value, `End`
/// when they let go (Touch returns to reading; Latch keeps the last value).
#[derive(Clone, Debug, PartialEq)]
pub struct ParamEdit {
    pub id: u32,
    /// The parameter's title, as the plugin names it.
    pub title: String,
    pub phase: EditPhase,
    /// Normalised 0..1; on `Change` only.
    pub value: Option<f64>,
    /// When the edit reached the engine.
    pub at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditPhase {
    /// State-capture value, never a user automation gesture.
    Snapshot,
    Begin,
    Change,
    End,
}

/// What the plugin changed since the last [`PluginLink::take_edits`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginEdits {
    pub origin: Origin,
    pub stamp: Stamp,
    /// The plugin's state after the user's last edit settled.
    pub state: Option<PluginState>,
    pub params: Vec<ParamEdit>,
}

/// What the UI can show about one remote plugin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginStatus {
    /// The host process went down (a crash, a hang or a kill); the plugin plays dry until
    /// [`PluginServer::restart`].
    pub crashed: bool,
    /// Why the plugin could not be opened; it then plays dry.
    pub error: Option<String>,
    /// The host's exit code after a crash.
    pub exit_code: Option<u32>,
    /// Host processes that went down since the server started.
    pub crashes: u64,
    /// Host processes this plugin took down: it crashed inside the plugin, or the plugin
    /// stalled the audio until the host was stopped.
    pub faults: u64,
    /// Frames the plugin adds to the mix: `L` plus the plugin's own latency.
    pub latency: u32,
    pub wet_blocks: u64,
    pub dry_blocks: u64,
    /// Blocks the host had not delivered in time while it was in use.
    pub misses: u64,
    pub editor_open: bool,
}

/// The connection between one plugin in the mix, the server and whoever keeps the song. Stays
/// the same across rebuilds of the mix, so edits are not lost.
#[derive(Default)]
pub struct PluginLink {
    /// Non-audio callers serialize access to the producers; the audio consumers never lock.
    /// One for each instance of the mix that is prepared, by [`RemotePlugin::feed`]: while a
    /// new mix opens (ENG-065) both the instance that plays and the one on its way take the
    /// automation (ENG-066).
    automation: Mutex<Vec<(u64, rtrb::Producer<ParamChange>)>>,
    /// The slot of the instance that plays, the one its window and what it reports belong to
    /// (0: none).
    slot: AtomicU32,
    /// The slot of the instance a new mix opens while the old one plays on (ENG-065), 0 when
    /// there is none. The values given to the plugin meanwhile go to both, and what the user
    /// changes in the playing one's window goes on to this one too, so a mix that is not
    /// swapped in after all leaves the playing one as it was given, and one that is has the
    /// same values (ENG-066). It becomes [`PluginLink::slot`] when the new mix is swapped in
    /// ([`PluginServer::promote`]), or when the old instance is closed first.
    next: AtomicU32,
    wet_blocks: AtomicU64,
    dry_blocks: AtomicU64,
    misses: AtomicU64,
    faults: AtomicU64,
    /// The host generation the audio side found stalled in this plugin.
    stalled_epoch: AtomicU64,
    /// How many states the host has sent for this plugin.
    states_seen: AtomicU64,
    /// The last block came from the plugin itself, not the dry signal (ENG-064).
    live: AtomicBool,
    /// How many openings the host has answered for this plugin (ENG-065).
    answers: AtomicU64,
    inner: Mutex<LinkInner>,
}

/// What one instance of the plugin said when it was opened and what the mix was built with
/// for it. The instance that plays and the one a new mix opens meanwhile have one each, so an
/// instance that cannot be opened does not change what the one heard shows (ENG-067).
#[derive(Clone, Debug, Default)]
struct Seat {
    error: Option<String>,
    /// The last answer that opened it; a new instance starts with the one of the instance
    /// that plays, as the parameters and latency to expect.
    opened: Option<Opened>,
    /// `opened` is the instance's own answer.
    answered: bool,
    /// Frames the instance adds to the mix: `L` plus the plugin's latency.
    latency: u32,
    /// The plugin latency the mix was built with, plus one (0: none yet). A plugin the audio
    /// side took before its answer came (ENG-065) is built with the last one known; when the
    /// answer says otherwise the mix has to be built again to compensate it.
    built_latency: u64,
    /// How long its opening took, in µs, plus one (0: not opened yet).
    open_us: u64,
}

impl Seat {
    /// A seat for an instance a new mix opens while `heard` plays.
    fn after(heard: &Seat) -> Seat {
        Seat {
            opened: heard.opened.clone(),
            ..Seat::default()
        }
    }
}

struct EditorState {
    revision: u64,
    source: Stamp,
    state: PluginState,
    values: Vec<(u32, String, f64)>,
}
#[derive(Default)]
struct LinkInner {
    receipts: UpdateGate,
    origins: HashMap<u32, u64>,
    updates: UpdateGate,
    captures: HashMap<u64, (Stamp, Option<Result<PluginEdits, String>>)>,
    live_revision: u64,
    live_values: Vec<(u64, Stamp, u32, String, f64)>,
    live_state: Option<EditorState>,
    /// The instance that plays, whose window and reports these are.
    heard: Seat,
    /// The instance a new mix opens while another plays (ENG-066), until it plays or is
    /// closed.
    coming: Option<Seat>,
    state: Option<(Stamp, PluginState)>,
    params: Vec<(Stamp, ParamEdit)>,
    latency_changed: Option<u32>,
    editor_open: bool,
    /// What the open window was asked with, to open it again on the plugin's next slot.
    editor: Option<(String, Option<Caption>)>,
    /// The window opens when this instance begins to play ([`PluginServer::promote`]): it
    /// takes over from an instance in another host, whose window is open until then.
    editor_later: bool,
    /// How many times the host said the window was shut (the person closed it).
    shut: u64,
    play_requests: u32,
}

impl LinkInner {
    /// The seat of the instance that plays, or of the one a new mix opens meanwhile.
    fn seat(&mut self, coming: bool) -> &mut Seat {
        if coming {
            self.coming.get_or_insert_with(|| Seat::after(&self.heard))
        } else {
            &mut self.heard
        }
    }

    /// The seat of the last instance opened.
    fn newest(&self) -> &Seat {
        self.coming.as_ref().unwrap_or(&self.heard)
    }
}

impl PluginLink {
    #[cfg(test)]
    fn begin_capture(&self) -> u64 {
        let stamp = self.lock().updates.stamp();
        self.begin_capture_at(stamp)
    }
    fn begin_capture_at(&self, stamp: Stamp) -> u64 {
        let request = NEXT_CAPTURE
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("capture request identifiers exhausted");
        self.lock().captures.insert(request, (stamp, None));
        request
    }

    fn complete_capture(&self, request: u64, result: Result<PluginState, String>) -> bool {
        self.complete_capture_values(request, result, &[])
    }

    fn complete_capture_values(
        &self,
        request: u64,
        result: Result<PluginState, String>,
        values: &[(u32, String, f64)],
    ) -> bool {
        let stamp = self
            .lock()
            .captures
            .get(&request)
            .map(|(stamp, _)| *stamp)
            .unwrap_or_default();
        self.complete_capture_at(request, stamp, result, values)
    }
    fn complete_capture_at(
        &self,
        request: u64,
        source: Stamp,
        mut result: Result<PluginState, String>,
        values: &[(u32, String, f64)],
    ) -> bool {
        let mut inner = self.lock();
        let Some((expected, capture)) = inner.captures.get(&request) else {
            return false;
        };
        if capture.is_some() {
            return false;
        }
        if expected.instance != source.instance
            || expected.generation != source.generation
            || source.revision < expected.revision
            || !inner.receipts.accepts(source, Field::Receipt)
        {
            result = Err(crate::plugin_updates::STALE_UPDATE.into());
        }
        if result.is_ok() {
            inner.captures.get_mut(&request).unwrap().0 = source;
            self.states_seen.fetch_add(1, Ordering::Release);
        }
        let params = values
            .iter()
            .map(|(id, title, value)| ParamEdit {
                id: *id,
                title: title.clone(),
                value: Some(*value),
                phase: EditPhase::Snapshot,
                at: Instant::now(),
            })
            .collect();
        inner.captures.get_mut(&request).unwrap().1 = Some(result.map(|state| PluginEdits {
            origin: Origin::Capture,
            stamp: source,
            state: Some(state),
            params,
        }));
        true
    }

    pub fn capture_finished(&self, request: u64) -> bool {
        self.lock()
            .captures
            .get(&request)
            .is_none_or(|(_, result)| result.is_some())
    }

    pub fn capture_error(&self, request: u64) -> Option<String> {
        let inner = self.lock();
        match inner.captures.get(&request) {
            None => Some("プラグインの状態要求が取り消されました".into()),
            Some((stamp, Some(_))) if !inner.receipts.accepts(*stamp, Field::Receipt) => {
                Some(crate::plugin_updates::STALE_UPDATE.into())
            }
            Some((_, Some(Err(error)))) => Some(error.clone()),
            _ => None,
        }
    }

    pub fn cancel_capture(&self, request: u64) {
        self.lock().captures.remove(&request);
    }

    pub fn take_capture_edits(&self, request: u64) -> Result<PluginEdits, String> {
        let inner = self.lock();
        let (stamp, value) = inner
            .captures
            .get(&request)
            .ok_or("プラグインの状態要求が取り消されました")?;
        if !inner.receipts.accepts(*stamp, Field::Receipt) {
            return Err(crate::plugin_updates::STALE_UPDATE.into());
        }
        value
            .clone()
            .unwrap_or_else(|| Err("プラグインの状態の返事がありません".into()))
    }
    /// Plain Space presses received from the hosted editor frame.
    pub fn take_play_requests(&self) -> u32 {
        std::mem::take(&mut self.lock().play_requests)
    }

    /// What the plugin's open window was asked with (its title and caption colours), or
    /// `None` when its window is not open.
    pub fn editor(&self) -> Option<(String, Option<Caption>)> {
        self.lock().editor.clone()
    }

    /// How many times the host said the plugin's window was shut: the person closed it, unlike
    /// a window closed with [`PluginServer::close_editor`] (ENG-071).
    pub fn windows_shut(&self) -> u64 {
        self.lock().shut
    }
    pub fn new() -> Arc<PluginLink> {
        Arc::new(PluginLink::default())
    }

    fn lock(&self) -> MutexGuard<'_, LinkInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Editor notifications have their own revision; draining automation edits
    /// or completing a getState receipt does not erase this handoff boundary.
    pub fn live_revision(&self) -> u64 {
        self.lock().live_revision
    }
    pub fn update_gate(&self) -> UpdateGate {
        self.lock().updates.clone()
    }
    pub fn current_gate(&self) -> UpdateGate {
        self.lock().receipts.clone()
    }

    /// Edits made in the plugin's editor since the last call.
    pub fn take_edits(&self) -> PluginEdits {
        let mut l = self.lock();
        let state = l
            .state
            .take()
            .filter(|(stamp, _)| l.updates.accepts(*stamp, Field::State))
            .map(|(_, state)| state);
        let params = std::mem::take(&mut l.params)
            .into_iter()
            .filter(|(stamp, p)| {
                l.updates.accepts(
                    *stamp,
                    Field::Parameter {
                        id: p.id,
                        title: &p.title,
                    },
                )
            })
            .map(|(_, p)| p)
            .collect();
        PluginEdits {
            origin: Origin::Editor,
            stamp: l.updates.stamp(),
            state,
            params,
        }
    }

    /// How many accepted states the host has sent. Explicit requests are identified
    /// by their receipt, independently of unsolicited editor state notifications.
    pub fn states_seen(&self) -> u64 {
        self.states_seen.load(Ordering::Acquire)
    }

    /// A state from the editor that nobody has taken yet.
    pub fn has_pending_state(&self) -> bool {
        self.lock().state.is_some()
    }

    /// The plugin's new latency, when it asked for it to be read again (ENG-011): the mix has
    /// to be rebuilt to compensate it.
    pub fn take_latency_change(&self) -> Option<u32> {
        let mut l = self.lock();
        if let Some(latency) = l.latency_changed.take() {
            return Some(latency);
        }
        // The answer of an opening the mix did not wait for (ENG-065) came with a latency other
        // than the one the mix assumed.
        let seat = &mut l.heard;
        let built = seat.built_latency;
        let opened = seat.opened.as_ref().filter(|_| seat.answered)?.latency;
        if built != 0 && u64::from(opened) + 1 != built {
            seat.built_latency = u64::from(opened) + 1;
            return Some(opened);
        }
        None
    }

    /// The latency of the plugin the last opening reported, if there was one.
    fn known_latency(&self) -> u32 {
        self.lock()
            .newest()
            .opened
            .as_ref()
            .map_or(0, |o| o.latency)
    }

    /// What the host reported when it opened the plugin that plays: its name, latency and
    /// parameters.
    pub fn opened(&self) -> Option<Opened> {
        self.lock().heard.opened.clone()
    }

    /// The names the plugin gives its keys, as last reported (EDT-134).
    pub fn note_names(&self) -> Vec<(u8, String)> {
        self.lock()
            .heard
            .opened
            .as_ref()
            .map(|o| o.note_names.clone())
            .unwrap_or_default()
    }

    /// Why the instance that plays could not be opened; it then plays dry. One a new mix
    /// opens meanwhile does not change it until it plays ([`PluginLink::opening_error`]).
    pub fn error(&self) -> Option<String> {
        self.lock().heard.error.clone()
    }

    /// Why the last opening failed: of the instance a new mix opens while another plays, or
    /// else of the one that plays (ENG-067). What to warn of when a mix is built.
    pub fn opening_error(&self) -> Option<String> {
        self.lock().newest().error.clone()
    }

    /// What the host reported for the last opening, as [`PluginLink::opening_error`].
    pub fn opening(&self) -> Option<Opened> {
        self.lock().newest().opened.clone()
    }

    /// The plugin is heard (ENG-064): its last block came from it, not from the dry signal
    /// that stands in while it opens, warms up after a restart or its host is down.
    pub fn live(&self) -> bool {
        self.live.load(Ordering::Relaxed)
    }

    /// How long the plugin's last opening in its host took, the answer included.
    pub fn open_took(&self) -> Option<Duration> {
        match self.lock().newest().open_us {
            0 => None,
            us => Some(Duration::from_micros(us - 1)),
        }
    }

    /// Gives the answer of an opening to the instance's seat (`None`: it is gone, and the
    /// answer only counts).
    fn set_open_result(&self, seat: Option<bool>, r: &Result<Opened, String>) {
        let mut l = self.lock();
        if let Some(s) = seat.map(|coming| l.seat(coming)) {
            match r {
                Ok(o) => {
                    s.opened = Some(o.clone());
                    s.answered = true;
                    s.error = None;
                }
                Err(e) => s.error = Some(e.clone()),
            }
        }
        self.answers.fetch_add(1, Ordering::Release);
    }

    /// How many openings the host has answered so far; an opening begun after a call has
    /// answered once this is above what that call returned (ENG-065).
    pub fn answers(&self) -> u64 {
        self.answers.load(Ordering::Acquire)
    }

    fn title(l: &LinkInner, id: u32) -> String {
        l.heard
            .opened
            .as_ref()
            .and_then(|o| o.params.iter().find(|p| p.0 == id))
            .map(|p| p.1.clone())
            .unwrap_or_default()
    }

    fn gesture(&self, stamp: Stamp, g: Gesture) {
        let mut l = self.lock();
        let (id, phase, value) = match g {
            Gesture::Begin(id) => (id, EditPhase::Begin, None),
            Gesture::Perform(id, v) => (id, EditPhase::Change, Some(v)),
            Gesture::End(id) => (id, EditPhase::End, None),
        };
        if l.params.len() >= MAX_PARAM_EDITS {
            l.params.remove(0);
        }
        let title = Self::title(&l, id);
        if let Some(value) = value {
            l.live_revision = l
                .live_revision
                .checked_add(1)
                .expect("editor revisions exhausted");
            let revision = l.live_revision;
            l.live_values
                .retain(|(_, source, param, _, _)| *source != stamp || *param != id);
            if l.live_values.len() >= MAX_PARAM_EDITS {
                l.live_values.remove(0);
            }
            l.live_values
                .push((revision, stamp, id, title.clone(), value));
        }
        l.params.push((
            stamp,
            ParamEdit {
                id,
                title,
                phase,
                value,
                at: Instant::now(),
            },
        ));
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    use crate::plugin_updates::new_instance;

    fn open(slot: u32, value: f64) -> OpenSlot {
        OpenSlot {
            slot,
            epoch: 0,
            generation: 0,
            revision: 0,
            shm: String::new(),
            req: String::new(),
            done: String::new(),
            bundle: String::new(),
            class: None,
            state: None,
            params: vec![("1".into(), value)],
        }
    }
    fn rec(link: &Arc<PluginLink>, slot: u32, updates: UpdateGate, value: f64) -> SlotRec {
        let mut open = open(slot, value);
        open.generation = updates.stamp().generation;
        link.lock().origins.insert(slot, updates.stamp().instance);
        SlotRec {
            updates,
            open,
            link: link.clone(),
            rings: None,
            poisoned: false,
        }
    }
    fn fixture() -> (PluginServer, Arc<PluginLink>, Stamp, mpsc::Receiver<ToHost>) {
        let server = PluginServer::new(None);
        let link = PluginLink::new();
        let gate = UpdateGate::new(Stamp {
            instance: new_instance(),
            generation: 0,
            revision: 0,
        });
        let old = gate.stamp();
        let next = gate.successor(new_instance(), Vec::new());
        link.slot.store(1, Ordering::Release);
        link.next.store(2, Ordering::Release);
        link.lock().updates = next.clone();
        link.lock().heard.opened = Some(Opened {
            params: vec![(1, "Delay".into()), (2, "Other".into())],
            ..Default::default()
        });
        let (writer, messages) = mpsc::sync_channel(256);
        {
            let mut st = server.inner.lock();
            st.next_slot = 3;
            st.proc = Some(Proc {
                writer,
                epoch: 0,
                stop: Arc::new(AtomicU8::new(KEEP)),
                pid: 0,
            });
            st.slots.insert(1, rec(&link, 1, gate, 0.25));
            st.slots.insert(2, rec(&link, 2, next, 0.25));
            st.publish(&link);
        }
        (server, link, old, messages)
    }
    fn late(server: &PluginServer, slot: u32, generation: u64, value: f64) {
        let revision = server
            .inner
            .lock()
            .source(slot)
            .map_or(1, |link| link.live_revision() + 1);
        server.inner.dispatch(
            0,
            FromHost::Gesture {
                slot,
                generation,
                revision,
                gesture: Gesture::Perform(1, value),
            },
        );
    }
    fn opaque(server: &PluginServer, slot: u32, generation: u64, value: u8) {
        let revision = server
            .inner
            .lock()
            .source(slot)
            .map_or(1, |link| link.live_revision() + 1);
        server.inner.dispatch(
            0,
            FromHost::State {
                slot,
                generation,
                revision,
                state: state(value),
                values: vec![(1, "Delay".into(), 0.5), (2, "Other".into(), 0.625)],
            },
        );
    }
    fn close_old(server: &PluginServer, link: &PluginLink, closing: bool) {
        server.promote(link);
        if closing {
            server.close(1);
        }
    }
    #[test]
    fn post_promotion_explicit_values_and_undo_reject_old_and_same_instance_old_generation_notifications()
     {
        for closing in [false, true] {
            for key in ["1", "delay"] {
                let (server, link, old, messages) = fixture();
                close_old(&server, &link, closing);
                server.set_param(&link, key, 0.75);
                late(&server, 1, old.generation, 0.5);
                late(&server, 2, old.generation, 0.375);
                assert_eq!(server.current(&link).unwrap().params[0].1, 0.75);
                assert!(link.take_edits().params.is_empty());
                let sent: Vec<_> = messages.try_iter().collect();
                assert!(
                    !sent
                        .iter()
                        .any(|m| matches!(m,ToHost::SetParam {slot:2,value,..} if *value==0.5))
                );
                let generation = link.update_gate().stamp().generation;
                late(&server, 2, generation, 0.875);
                assert_eq!(server.current(&link).unwrap().params[0].1, 0.875);
                assert_eq!(link.take_edits().params[0].value, Some(0.875));
                server.set_state(&link, state(9));
                server.invalidate(&link, true, &[]); // explicit reset needing a new graph
                server.set_state(&link, state(9)); // undo restores the same bytes, a new generation
                opaque(&server, 1, old.generation, 3);
                opaque(&server, 2, generation, 4);
                assert_eq!(server.current(&link).unwrap().state, Some(state(9)));
                assert!(link.take_edits().state.is_none());
            }
        }
    }
    #[test]
    fn opaque_edits_follow_promotion_and_close_into_both_seats_with_explicit_value_replay() {
        for closing in [false, true] {
            for protect in [false, true] {
                let (server, link, old, messages) = fixture();
                close_old(&server, &link, closing);
                if protect {
                    server.set_param(&link, "Delay", 0.75);
                }
                let current = server.inner.lock().slots[&2].updates.clone();
                let next = current.successor(new_instance(), Vec::new());
                server
                    .inner
                    .lock()
                    .slots
                    .insert(3, rec(&link, 3, next, if protect { 0.75 } else { 0.25 }));
                link.next.store(3, Ordering::Release);
                opaque(&server, 1, old.generation, 7);
                let st = server.inner.lock();
                for slot in [2, 3] {
                    assert_eq!(st.slots[&slot].open.state, Some(state(7)));
                    let values = &st.slots[&slot].open.params;
                    assert_eq!(
                        values.iter().find(|(key, _)| key == "1").unwrap().1,
                        if protect { 0.75 } else { 0.5 }
                    );
                    assert_eq!(values.iter().find(|(key, _)| key == "2").unwrap().1, 0.625);
                }
                drop(st);
                let edit = link.take_edits();
                assert_eq!(edit.state, Some(state(7)));
                assert!(edit.params.iter().all(|p| p.phase == EditPhase::Snapshot));
                assert_eq!(
                    edit.params.iter().rev().find(|p| p.id == 1).unwrap().value,
                    Some(if protect { 0.75 } else { 0.5 })
                );
                let sent: Vec<_> = messages.try_iter().collect();
                for target in [2, 3] {
                    let at=sent.iter().position(|m|matches!(m,ToHost::SetState {slot,state: s,..} if *slot==target && *s==state(7))).unwrap();
                    if protect {
                        assert!(sent[at+1..].iter().any(|m|matches!(m,ToHost::SetParam {slot,value,..} if *slot==target && *value==0.75)));
                    }
                }
            }
        }
    }
    #[test]
    fn current_editor_actions_supersede_ancestors_during_successive_replacements() {
        let (server, link, old, _messages) = fixture();
        close_old(&server, &link, true);
        let current = server.inner.lock().slots[&2].updates.clone();
        server.inner.lock().slots.insert(
            3,
            rec(
                &link,
                3,
                current.successor(new_instance(), Vec::new()),
                0.25,
            ),
        );
        link.next.store(3, Ordering::Release);
        late(&server, 2, 0, 0.75);
        late(&server, 1, old.generation, 0.5);
        for slot in [2, 3] {
            assert_eq!(server.inner.lock().slots[&slot].open.params[0].1, 0.75);
        }
        opaque(&server, 2, 0, 9);
        opaque(&server, 1, old.generation, 3);
        for slot in [2, 3] {
            assert_eq!(server.inner.lock().slots[&slot].open.state, Some(state(9)));
        }
        server.inner.dispatch(0, FromHost::Closed { slot: 1 });
        late(&server, 1, old.generation, 0.125);
        assert!(server.inner.lock().source(1).is_none());
    }
    #[test]
    fn opaque_and_parameter_edits_cross_the_registration_gap_in_order_and_captures_never_feed_it() {
        for protected in [Vec::new(), vec!["1".into()]] {
            for after_state in [false, true] {
                let (server, link, old, _messages) = fixture();
                server.inner.lock().slots.remove(&2);
                link.next.store(0, Ordering::Release);
                link.lock().updates = server.inner.lock().slots[&1].updates.clone();
                let since = link.live_revision();
                late(&server, 1, 0, 0.375);
                opaque(&server, 1, 0, 7);
                if after_state {
                    late(&server, 1, 0, 0.875);
                }
                link.take_edits();
                let request = link.begin_capture_at(old);
                link.complete_capture_values(request, Ok(state(99)), &[(1, "Delay".into(), 0.99)]);
                let mut processor =
                    RemotePlugin::new(server.clone(), link.clone(), RemoteSpec::default());
                let (connection, _) = processor
                    .connect(&ProcessSetup {
                        sample_rate: 48_000.0,
                        max_block: 512,
                        mode: RenderMode::Realtime,
                    })
                    .unwrap();
                server.inner.lock().crashed = true;
                let _ = server.open_slot(
                    &link,
                    open(0, 0.25),
                    connection.rings,
                    &connection.slot,
                    Some(LiveTransfer {
                        since,
                        protected: protected.clone(),
                        basis: None,
                    }),
                    true,
                );
                let st = server.inner.lock();
                let next = &st.slots[&link.next.load(Ordering::Acquire)];
                assert_eq!(next.open.state, Some(state(7)));
                assert_eq!(
                    next.open
                        .params
                        .iter()
                        .find(|(key, _)| key == "1")
                        .unwrap()
                        .1,
                    if !protected.is_empty() {
                        0.25
                    } else if after_state {
                        0.875
                    } else {
                        0.5
                    }
                );
            }
        }
    }
    #[test]
    fn an_editor_change_after_extraction_rejects_old_cache_writes_and_state_receipts() {
        let (server, link, _, _messages) = fixture();
        server.close(2); // keep the first, playing instance
        opaque(&server, 1, 0, 3);
        let extracted = link.take_edits();
        let request = server.request_state(&link);
        late(&server, 1, 0, 0.75);
        server.remember_snapshot(&link, extracted.stamp, state(3), &[("1".into(), 0.5)]);
        assert_eq!(
            server
                .current(&link)
                .unwrap()
                .params
                .iter()
                .find(|(key, _)| key == "1")
                .unwrap()
                .1,
            0.75
        );
        server.inner.dispatch(
            0,
            FromHost::CapturedState {
                slot: 1,
                generation: 0,
                revision: 0,
                request,
                result: Ok(state(3)),
                values: vec![(1, "Delay".into(), 0.5)],
            },
        );
        assert!(
            link.take_capture_edits(request).is_err(),
            "same intent/instance does not make an old observation current"
        );
        let current = server.request_state(&link);
        server.inner.dispatch(
            0,
            FromHost::CapturedState {
                slot: 1,
                generation: 0,
                revision: link.current_gate().stamp().revision,
                request: current,
                result: Ok(state(7)),
                values: vec![(1, "Delay".into(), 0.75)],
            },
        );
        assert_eq!(
            link.take_capture_edits(current).unwrap().params[0].value,
            Some(0.75)
        );
    }

    #[test]
    fn queued_state_replays_later_explicit_values_and_old_receipts_fail_after_promotion() {
        let (server, link, old, _messages) = fixture();
        let request = server.request_state(&link);
        server.inner.dispatch(
            0,
            FromHost::CapturedState {
                slot: 1,
                generation: old.generation,
                revision: 0,
                request,
                result: Ok(state(3)),
                values: Vec::new(),
            },
        );
        assert!(
            link.take_capture_edits(request).is_ok(),
            "playing receipt is valid while the successor prepares"
        );
        server.promote(&link);
        assert!(
            link.take_capture_edits(request).is_err(),
            "a completed old-instance receipt cannot save its successor"
        );
        opaque(&server, 1, old.generation, 7);
        server.set_param(&link, "Delay", 0.75);
        let edit = link.take_edits();
        assert_eq!(edit.state, Some(state(7)));
        assert_eq!(
            edit.params.iter().find(|p| p.id == 1).unwrap().value,
            Some(0.75)
        );
        assert!(
            !edit
                .params
                .iter()
                .any(|p| p.id == 1 && p.value == Some(0.5))
        );
    }

    #[test]
    fn receipts_are_stamped_and_never_become_editor_state_or_transfer_values() {
        let (server, link, _, _messages) = fixture();
        server.promote(&link);
        let request = server.request_state(&link);
        let old = link.update_gate().stamp();
        server.set_param(&link, "1", 0.75);
        server.inner.dispatch(
            0,
            FromHost::CapturedState {
                slot: 2,
                generation: old.generation,
                revision: 0,
                request,
                result: Ok(state(3)),
                values: vec![(1, "Delay".into(), 0.25)],
            },
        );
        assert!(link.capture_finished(request));
        assert!(link.take_capture_edits(request).is_err());
        assert!(link.take_edits().state.is_none());
        assert_eq!(server.current(&link).unwrap().params[0].1, 0.75);
        let current = server.request_state(&link);
        let generation = link.update_gate().stamp().generation;
        server.inner.dispatch(
            0,
            FromHost::CapturedState {
                slot: 2,
                generation,
                revision: link.current_gate().stamp().revision,
                request: current,
                result: Ok(state(7)),
                values: vec![(1, "Delay".into(), 0.75)],
            },
        );
        assert_eq!(
            link.take_capture_edits(current).unwrap().state,
            Some(state(7))
        );
        assert!(server.current(&link).unwrap().state.is_none());
    }

    #[test]
    fn blocked_writer_does_not_hold_server_or_wait_for_queue_space() {
        use super::*;
        struct Gate(mpsc::SyncSender<()>, mpsc::Receiver<()>);
        impl io::Write for Gate {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.send(()).unwrap();
                self.1.recv().unwrap();
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let (entered, received) = mpsc::sync_channel(1);
        let (release, held) = mpsc::channel();
        let stop = Arc::new(AtomicU8::new(KEEP));
        let writer = host_writer(Gate(entered, held), stop.clone()).unwrap();
        let mut state = State {
            proc: Some(Proc {
                writer,
                stop: stop.clone(),
                epoch: 1,
                pid: 0,
            }),
            ..State::default()
        };
        assert!(state.send(&ToHost::Close { slot: 1 }));
        received.recv_timeout(Duration::from_secs(10)).unwrap();
        // The writer is still inside Write. All calls below return without a
        // release, including overflow and a subsequent state request.
        for _ in 0..CONTROL_QUEUE {
            assert!(state.send(&ToHost::Close { slot: 1 }));
        }
        assert!(!state.send(&ToHost::Close { slot: 1 }));
        assert_eq!(stop.load(Ordering::Acquire), KILL);
        assert!(!state.send(&ToHost::CaptureState {
            slot: 1,
            request: 2
        }));
        drop(state);
        // Enough releases for the framing writes; no join is required on UI.
        for _ in 0..3 {
            let _ = release.send(());
        }
    }
    fn state(value: u8) -> PluginState {
        PluginState {
            component: vec![value],
            controller: vec![],
        }
    }

    #[test]
    fn unsolicited_and_abandoned_results_cannot_complete_a_new_capture() {
        let link = PluginLink::new();
        let old = link.begin_capture();
        link.cancel_capture(old);
        let current = link.begin_capture();
        link.lock().state = Some((Stamp::default(), state(1)));
        link.states_seen.fetch_add(1, Ordering::Release);
        assert!(!link.capture_finished(current));
        assert!(!link.complete_capture_values(old, Ok(state(2)), &[(1, "old".into(), 0.1)]));
        assert!(!link.capture_finished(current));
        assert!(link.complete_capture_values(
            current,
            Ok(state(3)),
            &[(1, "current".into(), 0.75)]
        ));
        link.lock().state = Some((Stamp::default(), state(4)));
        let captured = link.take_capture_edits(current).unwrap();
        assert_eq!(captured.state, Some(state(3)));
        assert_eq!(captured.params.len(), 1);
        assert_eq!(captured.params[0].value, Some(0.75));
        assert_eq!(captured.params[0].phase, EditPhase::Snapshot);
        assert_eq!(link.take_edits().state, Some(state(4)));
    }

    #[test]
    fn concurrent_captures_have_independent_results_and_cancellation() {
        let link = PluginLink::new();
        let old = link.begin_capture();
        let current = link.begin_capture();
        assert!(!link.capture_finished(old));
        assert!(link.complete_capture(old, Ok(state(1))));
        assert!(link.complete_capture(current, Err("SDK error".into())));
        assert_eq!(link.take_capture_edits(old).unwrap().state, Some(state(1)));
        link.cancel_capture(old);
        assert!(link.capture_finished(current));
        assert_eq!(link.capture_error(current).as_deref(), Some("SDK error"));
        assert_eq!(
            link.take_capture_edits(current).err().as_deref(),
            Some("SDK error")
        );
        assert!(
            link.take_edits().state.is_none(),
            "capture is not a passive edit"
        );
        link.cancel_capture(current);
        assert!(link.lock().captures.is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// The server.

/// Keeps the host process and the instances in it. Cheap to clone.
#[derive(Clone)]
pub struct PluginServer {
    inner: Arc<Inner>,
}

struct Inner {
    host: Option<PathBuf>,
    state: Mutex<State>,
    changed: Condvar,
    /// The newest host generation that has gone down; 0 when none has.
    down_epoch: AtomicU64,
    /// A host generation the audio side found stalled; its watcher kills it.
    stall_epoch: AtomicU64,
}

#[derive(Default)]
struct State {
    proc: Option<Proc>,
    epoch: u64,
    starting: bool,
    crashed: bool,
    exit_code: Option<u32>,
    crashes: u64,
    slots: HashMap<u32, SlotRec>,
    /// The instances asked to close whose host has not said so yet, with their links: what the
    /// host says about them until then still reaches the link (ENG-071).
    closing: HashMap<u32, Arc<PluginLink>>,
    next_slot: u32,
    replies: HashMap<u32, (u64, Result<Opened, String>)>,
}

/// Why the watcher stops a host.
const KEEP: u8 = 0;
/// Asked for (a restart or the server going away): not a crash.
const QUIT: u8 = 1;
/// Killed from outside, as a crash would end it.
const KILL: u8 = 2;

struct Proc {
    writer: mpsc::SyncSender<ToHost>,
    epoch: u64,
    stop: Arc<AtomicU8>,
    pid: u32,
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self
            .stop
            .compare_exchange(KEEP, QUIT, Ordering::AcqRel, Ordering::Acquire);
    }
}

#[derive(Clone)]
struct LiveTransfer {
    since: u64,
    protected: Vec<String>,
    basis: Option<UpdateGate>,
}

struct SlotRec {
    updates: UpdateGate,
    open: OpenSlot,
    link: Arc<PluginLink>,
    /// The instance's shared memory, where the host marks a crash inside the plugin.
    rings: Option<Arc<SharedRings>>,
    /// The host went down while opening this plugin; a restart leaves it out.
    poisoned: bool,
}
impl SlotRec {
    fn parameter_message(&mut self, key: String, value: f64) -> ToHost {
        let basis_revision = self.updates.stamp().revision;
        let stamp = self.updates.next_revision();
        self.open.revision = stamp.revision;
        ToHost::SetParam {
            slot: self.open.slot,
            generation: stamp.generation,
            revision: stamp.revision,
            basis_revision,
            key,
            value,
        }
    }
    fn state_message(&mut self, state: PluginState) -> ToHost {
        let basis_revision = self.updates.stamp().revision;
        let stamp = self.updates.next_revision();
        self.open.revision = stamp.revision;
        ToHost::SetState {
            slot: self.open.slot,
            generation: stamp.generation,
            revision: stamp.revision,
            basis_revision,
            state,
        }
    }
}

impl PluginServer {
    /// A server that runs `host`; `None` makes every plugin fail to open.
    pub fn new(host: Option<PathBuf>) -> PluginServer {
        PluginServer {
            inner: Arc::new(Inner {
                host,
                state: Mutex::new(State {
                    next_slot: 1,
                    ..State::default()
                }),
                changed: Condvar::new(),
                down_epoch: AtomicU64::new(0),
                stall_epoch: AtomicU64::new(0),
            }),
        }
    }

    /// The process-wide server, with the host from [`find_host`].
    pub fn shared() -> PluginServer {
        static SHARED: OnceLock<PluginServer> = OnceLock::new();
        SHARED
            .get_or_init(|| PluginServer::new(find_host()))
            .clone()
    }

    pub fn host(&self) -> Option<&Path> {
        self.inner.host.as_deref()
    }

    /// The running host's process id.
    pub fn pid(&self) -> Option<u32> {
        self.inner.lock().proc.as_ref().map(|p| p.pid)
    }

    /// The host has nothing more to say about `link` (ENG-071): no instance of it is open
    /// there, and the ones asked to close have been closed (or the host is gone).
    pub fn has_let_go(&self, link: &PluginLink) -> bool {
        let st = self.inner.lock();
        let is = |l: &Arc<PluginLink>| std::ptr::eq(Arc::as_ptr(l), link);
        !st.slots.values().any(|rec| is(&rec.link)) && !st.closing.values().any(is)
    }

    /// The host process is down and the plugins play dry.
    pub fn crashed(&self) -> bool {
        self.inner.lock().crashed
    }

    pub fn status(&self, link: &PluginLink) -> PluginStatus {
        let st = self.inner.lock();
        let slot = link.slot.load(Ordering::Acquire);
        let poisoned = st.slots.get(&slot).is_some_and(|s| s.poisoned);
        let l = link.lock();
        PluginStatus {
            crashed: st.crashed || poisoned,
            error: l.heard.error.clone(),
            exit_code: st.exit_code,
            crashes: st.crashes,
            faults: link.faults.load(Ordering::Relaxed),
            latency: l.heard.latency,
            wet_blocks: link.wet_blocks.load(Ordering::Relaxed),
            dry_blocks: link.dry_blocks.load(Ordering::Relaxed),
            misses: link.misses.load(Ordering::Relaxed),
            editor_open: l.editor_open,
        }
    }

    /// Starts the host process if it is not running, and checks that it speaks this engine's
    /// protocol. Opening a plugin does this by itself.
    pub fn start(&self) -> Result<(), String> {
        let st = self.inner.lock();
        if st.proc.is_some() {
            return Ok(());
        }
        self.inner.spawn(st).map(|_| ())
    }

    /// What the plugin would be opened with again: its latest state and parameters, with the
    /// edits made in its editor. `None` when it is not in this server.
    pub fn current(&self, link: &PluginLink) -> Option<OpenSlot> {
        let st = self.inner.lock();
        Some(
            st.slots
                .get(&link.slot.load(Ordering::Acquire))?
                .open
                .clone(),
        )
    }

    /// Opens a plugin in the host (starting the host if needed). The plugin stays registered
    /// even when this fails, so a restart tries again. Several threads may open at once: each
    /// waits for its own answer without holding the server (ENG-064).
    fn open(
        &self,
        link: &Arc<PluginLink>,
        open: OpenSlot,
        rings: Arc<SharedRings>,
        mine: &AtomicU32,
        transfer: Option<LiveTransfer>,
        intent_bound: bool,
    ) -> Result<Opened, String> {
        let began = Instant::now();
        let r = self.open_slot(link, open, rings, mine, transfer, intent_bound);
        let us = began.elapsed().as_micros().min(u128::from(u64::MAX - 1)) as u64;
        let st = self.inner.lock();
        if let Some(coming) = st.seat_of(link, mine.load(Ordering::Acquire)) {
            link.lock().seat(coming).open_us = us + 1;
        }
        r
    }

    /// What a [`RemotePlugin`] was built with: the frames it adds to the mix and the plugin
    /// latency assumed (plus one), on its instance's seat (ENG-067).
    fn built(&self, link: &PluginLink, slot: u32, latency: u32, built_latency: u64) {
        let st = self.inner.lock();
        if let Some(coming) = st.seat_of(link, slot) {
            let mut l = link.lock();
            let seat = l.seat(coming);
            seat.latency = latency;
            seat.built_latency = built_latency;
        }
    }

    /// The opening of a [`RemotePlugin`] failed before it reached the host.
    fn failed(&self, link: &PluginLink, e: String) {
        let st = self.inner.lock();
        link.set_open_result(st.seat_of(link, 0), &Err(e));
    }

    /// A [`RemotePlugin`] that never reached the host goes: a new instance it stood for while
    /// another played leaves no seat behind.
    fn forget_slotless(&self, link: &PluginLink) {
        let st = self.inner.lock();
        if link.next.load(Ordering::Acquire) == 0 && st.seat_of(link, 0) == Some(true) {
            link.lock().coming = None;
        }
    }

    fn open_slot(
        &self,
        link: &Arc<PluginLink>,
        mut open: OpenSlot,
        rings: Arc<SharedRings>,
        mine: &AtomicU32,
        transfer: Option<LiveTransfer>,
        intent_bound: bool,
    ) -> Result<Opened, String> {
        let inner = &self.inner;
        let mut st = inner.lock();
        if open.generation != link.update_gate().stamp().generation {
            return Err(crate::plugin_updates::STALE_UPDATE.into());
        }
        let playing = link.slot.load(Ordering::Acquire);
        let slot = st.next_slot;
        st.next_slot += 1;
        let instance = crate::plugin_updates::new_instance();
        let transfer = transfer.or_else(|| {
            (!intent_bound).then(|| LiveTransfer {
                since: link.live_revision(),
                protected: Vec::new(),
                basis: None,
            })
        });
        let basis = transfer
            .as_ref()
            .and_then(|transfer| transfer.basis.clone())
            .or_else(|| st.slots.get(&playing).map(|old| old.updates.clone()));
        let mut updates = match (&transfer, basis) {
            (Some(transfer), Some(old)) => old.successor(instance, transfer.protected.clone()),
            _ => link.update_gate().rebind(instance),
        };
        if transfer
            .as_ref()
            .is_none_or(|transfer| transfer.basis.is_none())
        {
            updates.retain_sources(|id| {
                st.slots
                    .values()
                    .any(|rec| rec.updates.stamp().instance == id)
                    || st
                        .closing
                        .values()
                        .any(|link| link.lock().origins.values().any(|origin| *origin == id))
            });
        }
        if let Some(transfer) = transfer {
            let inner = link.lock();
            let mut since = transfer.since;
            if let Some(edit) = &inner.live_state
                && edit.revision > transfer.since
                && updates.accepts(edit.source, Field::State)
            {
                merge_state(
                    &mut open,
                    &updates,
                    edit.source,
                    edit.state.clone(),
                    &edit.values,
                    &inner.heard.opened,
                );
                since = since.max(edit.revision);
            }
            for (revision, source, id, title, value) in &inner.live_values {
                if *revision > since
                    && updates.accepts(*source, Field::Parameter { id: *id, title })
                {
                    set_param(&mut open, &id.to_string(), title, *value);
                }
            }
        }
        open.generation = updates.stamp().generation;
        open.revision = updates.stamp().revision;
        link.lock().origins.insert(slot, instance);
        link.lock().updates = updates.clone();
        if playing == 0 {
            link.lock().receipts = updates.clone();
        }
        open.slot = slot;
        mine.store(slot, Ordering::Release);
        // With an instance playing, this one is on its way until that one is closed (ENG-066).
        let playing = link.slot.load(Ordering::Acquire);
        if playing != 0 && st.slots.contains_key(&playing) {
            // What it says has a seat of its own until it plays (ENG-067); one a mix opened
            // before, that this one takes over from, leaves its seat to it.
            let before = link.next.swap(slot, Ordering::AcqRel);
            let mut l = link.lock();
            if before != 0 || l.coming.is_none() {
                l.coming = Some(Seat::after(&l.heard));
            }
        } else {
            link.slot.store(slot, Ordering::Release);
        }
        st.slots.insert(
            slot,
            SlotRec {
                updates,
                open: open.clone(),
                link: link.clone(),
                rings: Some(rings.clone()),
                poisoned: false,
            },
        );
        st.publish(link);
        if st.crashed {
            let r = Err(
                "プラグインのプロセスがクラッシュしています（再起動すると復帰します）".to_string(),
            );
            link.set_open_result(st.seat_of(link, slot), &r);
            return r;
        }
        if st.proc.is_none() {
            match inner.spawn(st) {
                Ok(next) => st = next,
                Err(e) => {
                    let st = inner.lock();
                    let r = Err(e);
                    link.set_open_result(st.seat_of(link, slot), &r);
                    return r;
                }
            }
        }
        let epoch = st.epoch;
        open.epoch = epoch;
        if let Some(rec) = st.slots.get_mut(&slot) {
            rec.open.epoch = epoch;
        }
        st.send(&ToHost::Open(open));
        let (mut st, r) = inner.wait_opened(st, slot, epoch);
        if r.is_err() && st.epoch == epoch && st.proc.is_none() {
            // The host went down while opening this plugin: leave it out and bring the others
            // back at once.
            if let Some(rec) = st.slots.get_mut(&slot) {
                rec.poisoned = true;
            }
            // Counted when the host went down, if it marked the plugin.
            if rings.header().faulted.load(Ordering::Acquire) != epoch {
                link.faults.fetch_add(1, Ordering::Relaxed);
            }
            let r = Err("読み込むとプラグインのプロセスがクラッシュします".to_string());
            link.set_open_result(st.seat_of(link, slot), &r);
            let _ = inner.restart(st, true);
            return r;
        }
        link.set_open_result(st.seat_of(link, slot), &r);
        r
    }

    /// Takes the plugin out of the host. Returns what it was opened with, with the latest
    /// state and parameters.
    fn close(&self, slot: u32) -> Option<OpenSlot> {
        let mut st = self.inner.lock();
        let rec = st.slots.remove(&slot)?;
        if st.proc.is_some() {
            st.closing.insert(slot, rec.link.clone());
        }
        st.send(&ToHost::Close { slot });
        let link = &rec.link;
        if link.next.load(Ordering::Acquire) == slot {
            // The new mix was not swapped in: the instance that plays stays (ENG-066).
            link.next.store(0, Ordering::Release);
            link.lock().coming = None;
            st.publish(link);
            return Some(rec.open);
        }
        if link.slot.load(Ordering::Acquire) == slot {
            // The old mix is let go of before the new one was said to play: the instance it
            // opened plays now.
            st.seat_next(link, None);
        }
        Some(rec.open)
    }

    /// The new mix that opened the plugin again has been swapped in: the instance it opened
    /// is the one that plays from now on, though the old mix lets go of the old one only
    /// later (ENG-066). Nothing when no new instance is on its way.
    pub fn promote(&self, link: &PluginLink) {
        let mut st = self.inner.lock();
        let playing = link.slot.load(Ordering::Acquire);
        let leaving = st.slots.contains_key(&playing).then_some(playing);
        st.seat_next(link, leaving);
        // A window that waited for this instance to play opens on it.
        let waiting = {
            let mut l = link.lock();
            std::mem::take(&mut l.editor_later)
                .then(|| l.editor.clone())
                .flatten()
        };
        let slot = link.slot.load(Ordering::Acquire);
        if let Some((title, caption)) = waiting
            && st.slots.contains_key(&slot)
        {
            st.send(&ToHost::OpenEditor {
                slot,
                title,
                caption,
            });
        }
    }

    fn with_slot(&self, link: &PluginLink, f: impl FnOnce(&mut OpenSlot) -> Option<ToHost>) {
        let mut st = self.inner.lock();
        let slot = link.slot.load(Ordering::Acquire);
        if let Some(rec) = st.slots.get_mut(&slot)
            && let Some(msg) = f(&mut rec.open)
        {
            st.send(&msg);
        }
    }

    /// As [`PluginServer::with_slot`], for the instance that plays and the one a new mix opens
    /// meanwhile (ENG-066): a value given to the plugin reaches the one that is heard, stays in
    /// it when the new mix is not swapped in, and is in the new one when it is.
    fn with_seats(&self, link: &PluginLink, mut f: impl FnMut(&mut OpenSlot) -> Option<ToHost>) {
        let mut st = self.inner.lock();
        let playing = link.slot.load(Ordering::Acquire);
        let next = link.next.load(Ordering::Acquire);
        for slot in [playing, next] {
            if slot == 0 || (slot == next && next == playing) {
                continue;
            }
            let Some(rec) = st.slots.get_mut(&slot) else {
                continue;
            };
            if let Some(msg) = f(&mut rec.open) {
                st.send(&msg);
            }
        }
    }

    /// Replaces the plugin's state (an undo, or a state the song was given). The host does not
    /// report it back.
    pub fn set_state(&self, link: &PluginLink, state: PluginState) {
        let mut st = self.inner.lock();
        st.explicit(link, true, &[]);
        for slot in seats(link) {
            if let Some(rec) = st.slots.get_mut(&slot) {
                rec.open.state = Some(state.clone());
                rec.open.params.clear();
                let message = rec.state_message(state.clone());
                st.send(&message);
            }
        }
        st.publish(link);
    }
    /// Invalidate old replies even when reset/class/removal requires rebuilding.
    pub fn invalidate(&self, link: &PluginLink, replace: bool, keys: &[String]) {
        self.inner.lock().explicit(link, replace, keys);
    }

    /// Remembers an accepted snapshot for a later host restart. This changes no
    /// running instance and never overwrites a coming slot's explicit intent.
    pub fn remember_snapshot(
        &self,
        link: &PluginLink,
        stamp: Stamp,
        state: PluginState,
        params: &[(String, f64)],
    ) {
        let mut st = self.inner.lock();
        if !link.current_gate().accepts(stamp, Field::Receipt) {
            return;
        }
        let slot = link.slot.load(Ordering::Acquire);
        if let Some(rec) = st.slots.get_mut(&slot) {
            rec.open.state = Some(state);
            rec.open.params = params.to_vec();
        }
    }

    /// Asks for an independent snapshot receipt. Consume it with
    /// `take_capture_edits`, then release it with `cancel_capture`.
    pub fn request_state(&self, link: &PluginLink) -> u64 {
        let mut st = self.inner.lock();
        let slot = link.slot.load(Ordering::Acquire);
        let stamp = st
            .slots
            .get(&slot)
            .map(|rec| rec.updates.stamp())
            .unwrap_or_default();
        let request = link.begin_capture_at(stamp);
        if st.slots.contains_key(&slot) {
            if !st.send(&ToHost::CaptureState { slot, request }) {
                link.complete_capture(request, Err("プラグインへの送信ができません".into()));
            }
        } else {
            link.complete_capture(request, Err("プラグインが開かれていません".into()));
        }
        request
    }

    /// Sets a parameter by numeric id or title, normalised 0..1.
    pub fn set_param(&self, link: &PluginLink, key: &str, value: f64) {
        let mut st = self.inner.lock();
        let titles = link.lock().heard.opened.clone();
        let resolved = titles.as_ref().and_then(|o| {
            o.params.iter().find(|(id, title)| {
                key.parse::<u32>().ok() == Some(*id) || title.eq_ignore_ascii_case(key)
            })
        });
        let title = resolved.map(|(_, title)| title.as_str()).unwrap_or("");
        let canonical = resolved
            .map(|(id, _)| id.to_string())
            .unwrap_or_else(|| key.to_string());
        let mut keys = vec![key.to_string(), canonical.clone()];
        if !title.is_empty() {
            keys.push(title.to_string());
        }
        st.explicit(link, false, &keys);
        for slot in seats(link) {
            if let Some(rec) = st.slots.get_mut(&slot) {
                set_param(&mut rec.open, &canonical, title, value);
                let message = rec.parameter_message(canonical.clone(), value);
                st.send(&message);
            }
        }
        st.publish(link);
        let mut inner = link.lock();
        if inner.state.is_some() {
            let stamp = inner.updates.stamp();
            inner.params.push((
                stamp,
                ParamEdit {
                    id: resolved
                        .map(|(id, _)| *id)
                        .unwrap_or_else(|| key.parse().unwrap_or(u32::MAX)),
                    title: title.to_string(),
                    value: Some(value),
                    phase: EditPhase::Snapshot,
                    at: Instant::now(),
                },
            ));
        }
    }

    /// Queues automation for the next input block, at `offset` frames within that block.
    /// Only the audio thread writes the shared event ring, before publishing its audio.
    /// While a new mix opens, the instance that plays and the one on its way both take it
    /// (ENG-066). False when the parameter is unknown, no instance is prepared, or a queue is
    /// full (the others have taken it: sending it again is harmless).
    pub fn automate(&self, link: &PluginLink, key: &str, value: f64, offset: u32) -> bool {
        let id = {
            let inner = link.lock();
            let Some(opened) = &inner.heard.opened else {
                return false;
            };
            key.parse::<u32>()
                .ok()
                .filter(|id| opened.params.iter().any(|p| p.0 == *id))
                .or_else(|| {
                    opened
                        .params
                        .iter()
                        .find(|p| p.1.eq_ignore_ascii_case(key))
                        .map(|p| p.0)
                })
        };
        let Some(id) = id else { return false };
        let value = value.clamp(0.0, 1.0);
        let mut feeds = link.automation.lock().unwrap_or_else(|e| e.into_inner());
        let mut taken = 0;
        for (_, producer) in feeds.iter_mut() {
            taken += usize::from(producer.push(ParamChange { offset, id, value }).is_ok());
        }
        let all = taken == feeds.len();
        drop(feeds);
        if taken == 0 {
            return false;
        }
        // A restarted instance must keep the last value, without sending an untimed edit.
        self.with_seats(link, |o| {
            match o.params.iter_mut().find(|p| p.0 == key) {
                Some(p) => p.1 = value,
                None => o.params.push((key.to_string(), value)),
            }
            None
        });
        all
    }

    /// Opens the plugin's editor in the host process (behind other windows, not activated).
    pub fn open_editor(&self, link: &PluginLink, title: &str, caption: Option<Caption>) {
        self.with_slot(link, |o| {
            Some(ToHost::OpenEditor {
                slot: o.slot,
                title: title.to_string(),
                caption,
            })
        });
        let mut l = link.lock();
        l.editor_open = true;
        l.editor = Some((title.to_string(), caption));
        l.editor_later = false;
    }

    /// The plugin's window, open on an instance in another host, moves to this instance when
    /// the mix that opened it is swapped in ([`PluginServer::promote`]). Until then the window
    /// counts as open here.
    pub fn open_editor_later(&self, link: &PluginLink, title: &str, caption: Option<Caption>) {
        let mut l = link.lock();
        l.editor_open = true;
        l.editor = Some((title.to_string(), caption));
        l.editor_later = true;
    }

    /// Gives the open window of the plugin new caption colours (the app's theme changed).
    pub fn set_caption(&self, link: &PluginLink, caption: Caption) {
        self.with_slot(link, |o| {
            Some(ToHost::SetCaption {
                slot: o.slot,
                caption,
            })
        });
    }

    pub fn close_editor(&self, link: &PluginLink) {
        self.with_slot(link, |o| Some(ToHost::CloseEditor { slot: o.slot }));
        let mut l = link.lock();
        l.editor_open = false;
        l.editor = None;
        l.editor_later = false;
    }

    /// Moves a parameter the way the editor does. For tests of edit collection.
    pub fn simulate_edit(&self, link: &PluginLink, id: u32, values: &[f64]) {
        self.with_slot(link, |o| {
            Some(ToHost::SimulateEdit {
                slot: o.slot,
                id,
                values: values.to_vec(),
            })
        });
    }

    /// Makes the plugin's audio thread crash or hang after `after_blocks` more blocks. For
    /// tests of what happens when a plugin fails.
    pub fn fault(&self, link: &PluginLink, kind: Fault, after_blocks: u64) {
        self.with_slot(link, |o| {
            Some(ToHost::Fault {
                slot: o.slot,
                kind,
                after_blocks,
            })
        });
    }

    /// Kills the host process as a crash would end it.
    pub fn kill(&self) {
        if let Some(p) = &self.inner.lock().proc {
            p.stop.store(KILL, Ordering::Release);
        }
    }

    /// Waits until the host process has gone down (after [`PluginServer::kill`] or a fault).
    pub fn wait_down(&self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        let mut st = self.inner.lock();
        while !st.crashed {
            let now = Instant::now();
            if now >= end {
                return false;
            }
            st = self
                .inner
                .changed
                .wait_timeout(st, end - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }

    /// Starts a new host and opens every plugin again with its last state and parameters.
    /// The plugins take over from the dry signal as they come back.
    pub fn restart(&self) -> Result<(), String> {
        let st = self.inner.lock();
        self.inner.restart(st, true)
    }

    /// As [`PluginServer::restart`], without waiting for the plugins to open again (ENG-065):
    /// returns once the new host is started and asked for them. Each plugin plays dry until it
    /// is back, and a plugin that cannot be opened says so in its link, as it does when it is
    /// opened first.
    pub fn restart_later(&self) -> Result<(), String> {
        let inner = self.inner.clone();
        std::thread::Builder::new()
            .name("xirius-plugin-restart".into())
            .spawn(move || {
                let st = inner.lock();
                if let Err(error) = inner.restart(st, false) {
                    let mut st = inner.lock();
                    st.crashed = true;
                    for (&slot, rec) in &st.slots {
                        rec.link
                            .set_open_result(st.seat_of(&rec.link, slot), &Err(error.clone()));
                    }
                }
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn spawn<'a>(
        self: &'a Arc<Self>,
        mut st: MutexGuard<'a, State>,
    ) -> Result<MutexGuard<'a, State>, String> {
        while st.starting {
            st = self.changed.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        if st.proc.is_some() {
            return Ok(st);
        }
        st.starting = true;
        st.epoch += 1;
        let epoch = st.epoch;
        st.closing.clear();
        drop(st);
        let result = self.spawn_proc(epoch);
        let mut st = self.lock();
        st.starting = false;
        self.changed.notify_all();
        let proc = result?;
        if self.down_epoch.load(Ordering::Acquire) >= epoch {
            return Err("プラグインのプロセスが起動中に終了しました".into());
        }
        st.proc = Some(proc);
        st.crashed = false;
        st.exit_code = None;
        Ok(st)
    }

    fn spawn_proc(self: &Arc<Self>, epoch: u64) -> Result<Proc, String> {
        let host = self
            .host
            .as_ref()
            .ok_or("xirius-plugin-host が見つかりません")?;
        let stderr = if std::env::var_os("XIRIUS_PLUGIN_HOST_LOG").is_some() {
            Stdio::inherit()
        } else {
            Stdio::null()
        };
        let mut child = Command::new(host)
            .arg("--parent")
            .arg(std::process::id().to_string())
            .arg("--protocol")
            .arg(wire::PROTOCOL.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("{}: {e}", host.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let mut stdout = child.stdout.take().ok_or("no stdout")?;
        let stop = Arc::new(AtomicU8::new(KEEP));
        let pid = child.id();
        let weak: Weak<Inner> = Arc::downgrade(self);
        // The first message is the host's greeting; it goes to the check below.
        let (hello_tx, hello_rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("xirius-plugin-host-read".into())
            .spawn(move || {
                let mut hello = Some(hello_tx);
                while let Ok(Some(buf)) = wire::recv(&mut stdout) {
                    let msg = FromHost::decode(&buf);
                    if let Some(tx) = hello.take() {
                        let _ = tx.send(match msg {
                            Ok(FromHost::Hello { protocol, build }) => Some((protocol, build)),
                            _ => None,
                        });
                        continue;
                    }
                    let Some(inner) = weak.upgrade() else { return };
                    if let Ok(msg) = msg {
                        inner.dispatch(epoch, msg);
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let weak: Weak<Inner> = Arc::downgrade(self);
        let flag = stop.clone();
        std::thread::Builder::new()
            .name("xirius-plugin-host-watch".into())
            .spawn(move || {
                // Killed because the audio side found it stalled, not for another reason. A
                // host that is killed or ends on its own leaves every plugin of it stalled too
                // if that is seen before it is down, so only this puts a stall on the plugin.
                let mut hung = false;
                loop {
                    let stalled = weak
                        .upgrade()
                        .is_none_or(|i| i.stall_epoch.load(Ordering::Acquire) == epoch);
                    let stop = flag.load(Ordering::Acquire) != KEEP;
                    hung |= stalled && !stop;
                    if stop || stalled {
                        let _ = child.kill();
                    }
                    let code = match child.try_wait() {
                        Ok(None) => {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Ok(Some(c)) => c.code().map(|c| c as u32),
                        Err(_) => None,
                    };
                    if let Some(inner) = weak.upgrade() {
                        let asked = flag.load(Ordering::Acquire) == QUIT;
                        inner.went_down(epoch, code, asked, hung);
                    }
                    return;
                }
            })
            .map_err(|e| e.to_string())?;
        let rebuild = "エンジンと同じビルドの xirius-plugin-host を配置してください";
        let problem = match hello_rx.recv_timeout(HELLO_TIMEOUT) {
            Ok(Some((protocol, _))) if protocol == wire::PROTOCOL => None,
            Ok(Some((protocol, build))) => Some(format!(
                "通信プロトコルのバージョンが一致しません（xirius-plugin-host {build} は {protocol}、エンジンは {}）。{rebuild}",
                wire::PROTOCOL
            )),
            Ok(None) => Some(format!(
                "起動時にバージョンが通知されませんでした（xirius-plugin-host ではないか、バージョン確認に対応していない古いビルドです）。{rebuild}"
            )),
            Err(RecvTimeoutError::Disconnected) => Some(format!(
                "バージョンを通知せずに終了しました（バージョン確認に対応していない古い xirius-plugin-host です）。{rebuild}"
            )),
            Err(RecvTimeoutError::Timeout) => Some(format!(
                "{} 秒待ってもバージョンが通知されません（バージョン確認に対応していない古い xirius-plugin-host か、別のプログラムです）。{rebuild}",
                HELLO_TIMEOUT.as_secs()
            )),
        };
        if let Some(e) = problem {
            // Not a crash: the watcher ends it quietly.
            stop.store(QUIT, Ordering::Release);
            return Err(format!("{}: {e}", host.display()));
        }
        let writer = host_writer(stdin, stop.clone()).map_err(|e| e.to_string())?;
        Ok(Proc {
            writer,
            epoch,
            stop,
            pid,
        })
    }

    fn went_down(&self, epoch: u64, code: Option<u32>, asked: bool, hung: bool) {
        self.down_epoch.fetch_max(epoch, Ordering::AcqRel);
        let mut st = self.lock();
        if st.epoch == epoch {
            st.proc = None;
            st.closing.clear();
            if !asked {
                st.crashed = true;
                st.crashes += 1;
                st.exit_code = code;
                for rec in st.slots.values() {
                    let crashed_in = rec
                        .rings
                        .as_ref()
                        .is_some_and(|r| r.header().faulted.load(Ordering::Acquire) == epoch);
                    let stalled = hung && rec.link.stalled_epoch.load(Ordering::Acquire) == epoch;
                    if crashed_in || stalled {
                        rec.link.faults.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        drop(st);
        self.changed.notify_all();
    }

    fn dispatch(&self, epoch: u64, msg: FromHost) {
        let mut st = self.lock();
        if st.epoch != epoch {
            return;
        }
        match msg {
            FromHost::Opened {
                slot,
                epoch,
                result,
            } => {
                st.replies.insert(slot, (epoch, result));
                drop(st);
                self.changed.notify_all();
            }
            FromHost::Gesture {
                slot,
                generation,
                revision,
                gesture,
            } => st.accept_gesture(slot, generation, revision, gesture),
            FromHost::CapturedState {
                slot,
                generation,
                revision,
                request,
                result,
                values,
            } => {
                // Captures are observations belonging to a receipt. They never
                // mutate either slot or feed the passive editor channel.
                let link = st
                    .slots
                    .get(&slot)
                    .map(|rec| rec.link.clone())
                    .or_else(|| st.closing.get(&slot).cloned());
                if let Some(link) = link {
                    let instance = st.source_instance(slot, &link);
                    if let Some(rec) = st.slots.get_mut(&slot) {
                        rec.updates.received(Stamp {
                            instance,
                            generation,
                            revision,
                        });
                        rec.open.revision = rec.updates.stamp().revision;
                    }
                    st.publish(&link);
                    link.complete_capture_at(
                        request,
                        Stamp {
                            instance,
                            generation,
                            revision,
                        },
                        result,
                        &values,
                    );
                }
            }
            FromHost::State {
                slot,
                generation,
                revision,
                state,
                values,
            } => st.accept_editor_state(slot, generation, revision, state, &values),
            FromHost::Latency { slot, latency } => {
                // An instance that neither plays nor is about to has no mix to build again.
                if let Some(rec) = st.slots.get(&slot)
                    && st.seat_of(&rec.link, slot).is_some()
                {
                    rec.link.lock().latency_changed = Some(latency);
                }
            }
            FromHost::PlayKey { slot } => {
                // A press the window of an instance being let go of took is a press all the
                // same (ENG-071): its slot is in `closing` until the host confirms it is gone.
                let link = st
                    .slots
                    .get(&slot)
                    .map(|rec| rec.link.clone())
                    .or_else(|| st.closing.get(&slot).cloned());
                if let Some(link) = link {
                    let mut inner = link.lock();
                    inner.play_requests = inner.play_requests.saturating_add(1).min(32);
                }
            }
            FromHost::EditorClosed { slot } => {
                let Some(link) = st
                    .slots
                    .get(&slot)
                    .map(|rec| rec.link.clone())
                    .or_else(|| st.closing.get(&slot).cloned())
                else {
                    return;
                };
                let had = {
                    let mut l = link.lock();
                    l.editor_open = false;
                    l.editor_later = false;
                    l.shut += 1;
                    l.editor.take().is_some()
                };
                // Shut on the instance that played as its window moved to the one that plays
                // now (ENG-066): the moved window shuts too.
                let playing = link.slot.load(Ordering::Acquire);
                if had && playing != slot && st.slots.contains_key(&playing) {
                    st.send(&ToHost::CloseEditor { slot: playing });
                }
            }
            FromHost::Closed { slot } => {
                if let Some(link) = st.closing.remove(&slot) {
                    link.lock().origins.remove(&slot);
                }
            }
            FromHost::NoteNames { slot, names } => {
                if let Some(rec) = st.slots.get(&slot)
                    && let Some(coming) = st.seat_of(&rec.link, slot)
                    && let Some(opened) = rec.link.lock().seat(coming).opened.as_mut()
                {
                    opened.note_names = names;
                }
            }
            FromHost::Hello { .. } => {}
        }
    }

    /// Waits for the host to answer the opening of `slot`.
    fn wait_opened<'a>(
        &'a self,
        mut st: MutexGuard<'a, State>,
        slot: u32,
        epoch: u64,
    ) -> (MutexGuard<'a, State>, Result<Opened, String>) {
        let end = Instant::now() + OPEN_TIMEOUT;
        loop {
            if let Some((e, r)) = st.replies.remove(&slot)
                && e == epoch
            {
                return (st, r);
            }
            if st.epoch != epoch || st.proc.is_none() {
                return (st, Err("プラグインのプロセスが終了しました".into()));
            }
            let now = Instant::now();
            if now >= end {
                return (
                    st,
                    Err("プラグインが 20 秒以内に読み込まれませんでした".into()),
                );
            }
            st = self
                .changed
                .wait_timeout(st, end - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn restart<'a>(
        self: &'a Arc<Self>,
        mut st: MutexGuard<'a, State>,
        wait: bool,
    ) -> Result<(), String> {
        if let Some(p) = st.proc.take() {
            // Wait for the old host to be gone, so it cannot write into the rings the new one
            // lines up.
            let old = p.epoch;
            p.stop.store(QUIT, Ordering::Release);
            drop(p);
            let end = Instant::now() + Duration::from_secs(5);
            while self.down_epoch.load(Ordering::Acquire) < old {
                let now = Instant::now();
                if now >= end {
                    return Err("前のプラグインのプロセスが終了しません".into());
                }
                st = self
                    .changed
                    .wait_timeout(st, end - now)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        }
        st.crashed = false;
        st.exit_code = None;
        st.replies.clear();
        let mut slots: Vec<u32> = st
            .slots
            .iter()
            .filter(|(_, r)| !r.poisoned)
            .map(|(&s, _)| s)
            .collect();
        if slots.is_empty() {
            return Ok(());
        }
        slots.sort_unstable();
        st = self.spawn(st)?;
        let epoch = st.epoch;
        for &slot in &slots {
            let rec = st.slots.get_mut(&slot).unwrap();
            rec.open.epoch = epoch;
            let msg = ToHost::Open(rec.open.clone());
            st.send(&msg);
        }
        if wait {
            return self.reopened(st, epoch, slots);
        }
        drop(st);
        let inner = self.clone();
        // The answers are taken on a thread of their own; without one they are not taken and
        // the plugins stay dry.
        let _ = std::thread::Builder::new()
            .name("xirius-plugin-reopen".into())
            .spawn(move || {
                let st = inner.lock();
                let _ = inner.reopened(st, epoch, slots);
            });
        Ok(())
    }

    /// Takes the answers to the openings a restart asked for, and gives each plugin its own.
    fn reopened<'a>(
        self: &'a Arc<Self>,
        mut st: MutexGuard<'a, State>,
        epoch: u64,
        slots: Vec<u32>,
    ) -> Result<(), String> {
        let mut errors = Vec::new();
        for slot in slots {
            let r;
            (st, r) = self.wait_opened(st, slot, epoch);
            if let Some(rec) = st.slots.get(&slot) {
                rec.link.set_open_result(st.seat_of(&rec.link, slot), &r);
            }
            if let Err(e) = r {
                errors.push(e);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

/// Writes a value moved in a window into what an instance is opened with, under the name the
/// song gives it (its numeric id or its title).
fn set_param(open: &mut OpenSlot, key: &str, title: &str, value: f64) {
    open.params.retain(|(name, _)| {
        !name.eq_ignore_ascii_case(key) && (title.is_empty() || !name.eq_ignore_ascii_case(title))
    });
    open.params.push((key.to_string(), value));
}

fn seats(link: &PluginLink) -> [u32; 2] {
    [
        link.slot.load(Ordering::Acquire),
        link.next.load(Ordering::Acquire),
    ]
}

/// An opaque edit contains the runtime parameter values, not old descriptor
/// values. Retain/replay only keys the central gate protects against this source.
fn merge_state(
    open: &mut OpenSlot,
    gate: &UpdateGate,
    source: Stamp,
    state: PluginState,
    values: &[(u32, String, f64)],
    opened: &Option<Opened>,
) -> Vec<(String, f64)> {
    let replay: Vec<_> = open
        .params
        .iter()
        .filter(|(key, _)| {
            let (id, title) = opened
                .as_ref()
                .and_then(|o| {
                    o.params.iter().find(|(id, title)| {
                        key.parse::<u32>().ok() == Some(*id) || title.eq_ignore_ascii_case(key)
                    })
                })
                .map(|(id, title)| (*id, title.as_str()))
                .unwrap_or((key.parse().unwrap_or(u32::MAX), key.as_str()));
            !gate.accepts(source, Field::Parameter { id, title })
        })
        .cloned()
        .collect();
    open.state = Some(state);
    open.params.clear();
    for (id, title, value) in values {
        if gate.accepts(source, Field::Parameter { id: *id, title }) {
            set_param(open, &id.to_string(), title, *value);
        }
    }
    open.params.extend(replay.iter().cloned());
    replay
}
impl State {
    fn source_instance(&self, slot: u32, link: &PluginLink) -> u64 {
        self.slots
            .get(&slot)
            .map(|rec| rec.updates.stamp().instance)
            .or_else(|| link.lock().origins.get(&slot).copied())
            .unwrap_or(0)
    }
    fn source(&self, slot: u32) -> Option<Arc<PluginLink>> {
        self.slots
            .get(&slot)
            .map(|rec| rec.link.clone())
            .or_else(|| self.closing.get(&slot).cloned())
    }
    fn desired(&self, link: &PluginLink) -> Option<UpdateGate> {
        seats(link)
            .into_iter()
            .rev()
            .find_map(|id| self.slots.get(&id).map(|rec| rec.updates.clone()))
    }
    fn publish(&self, link: &PluginLink) {
        let desired = self.desired(link);
        let playing = self
            .slots
            .get(&link.slot.load(Ordering::Acquire))
            .map(|rec| rec.updates.clone());
        let mut inner = link.lock();
        if let Some(gate) = desired {
            inner.updates = gate;
        }
        inner.receipts = playing.unwrap_or_else(|| inner.updates.rebind(0));
    }
    fn explicit(&mut self, link: &PluginLink, replace: bool, keys: &[String]) {
        let generation = {
            let mut inner = link.lock();
            let next = inner.updates.next_generation();
            inner.updates.explicit(next, replace, keys);
            next
        };
        for slot in seats(link) {
            if let Some(rec) = self.slots.get_mut(&slot) {
                rec.updates.explicit(generation, replace, keys);
                rec.open.generation = generation;
            }
        }
        self.publish(link);
    }
    fn accept_gesture(&mut self, slot: u32, generation: u64, revision: u64, gesture: Gesture) {
        let Some(link) = self.source(slot) else {
            return;
        };
        let source = Stamp {
            instance: self.source_instance(slot, &link),
            generation,
            revision,
        };
        let id = match gesture {
            Gesture::Begin(id) | Gesture::End(id) | Gesture::Perform(id, _) => id,
        };
        let title = PluginLink::title(&link.lock(), id);
        let field = Field::Parameter { id, title: &title };
        let desired = self.desired(&link);
        let accepted = desired
            .as_ref()
            .is_some_and(|gate| gate.accepts(source, field));
        for target in seats(&link) {
            let Some(rec) = self.slots.get_mut(&target) else {
                continue;
            };
            rec.updates.received(source);
            rec.open.revision = rec.updates.stamp().revision;
            if !rec.updates.accepts(source, field) {
                continue;
            }
            rec.updates.observed(source, field);
            if let Gesture::Perform(_, value) = gesture {
                set_param(&mut rec.open, &id.to_string(), &title, value);
                if target != slot {
                    let message = rec.parameter_message(id.to_string(), value);
                    self.send(&message);
                }
            }
        }
        self.publish(&link);
        if accepted {
            link.gesture(source, gesture);
        }
    }
    fn accept_editor_state(
        &mut self,
        slot: u32,
        generation: u64,
        revision: u64,
        state: PluginState,
        values: &[(u32, String, f64)],
    ) {
        let Some(link) = self.source(slot) else {
            return;
        };
        let source = Stamp {
            instance: self.source_instance(slot, &link),
            generation,
            revision,
        };
        let desired = self.desired(&link);
        let accepted = desired
            .as_ref()
            .is_some_and(|gate| gate.accepts(source, Field::State));
        let opened = link.lock().heard.opened.clone();
        let mut protected = Vec::new();
        for target in seats(&link) {
            let Some(rec) = self.slots.get_mut(&target) else {
                continue;
            };
            rec.updates.received(source);
            rec.open.revision = rec.updates.stamp().revision;
            if !rec.updates.accepts(source, Field::State) {
                continue;
            }
            let replay = merge_state(
                &mut rec.open,
                &rec.updates,
                source,
                state.clone(),
                values,
                &opened,
            );
            rec.updates.observed(source, Field::State);
            if desired
                .as_ref()
                .is_some_and(|gate| gate.stamp().instance == rec.updates.stamp().instance)
            {
                protected = replay.clone();
            }
            if target != slot {
                let mut messages = vec![rec.state_message(state.clone())];
                for (key, value) in replay {
                    messages.push(rec.parameter_message(key, value));
                }
                for message in messages {
                    self.send(&message);
                }
            }
        }
        self.publish(&link);
        if !accepted {
            return;
        }
        let mut inner = link.lock();
        inner.live_revision = inner
            .live_revision
            .checked_add(1)
            .expect("editor revisions exhausted");
        inner.live_state = Some(EditorState {
            revision: inner.live_revision,
            source,
            state: state.clone(),
            values: values.to_vec(),
        });
        inner.state = Some((source, state));
        for (id, title, value) in values {
            if inner
                .updates
                .accepts(source, Field::Parameter { id: *id, title })
            {
                inner.params.push((
                    source,
                    ParamEdit {
                        id: *id,
                        title: title.clone(),
                        value: Some(*value),
                        phase: EditPhase::Snapshot,
                        at: Instant::now(),
                    },
                ));
            }
        }
        // Preserve explicit values when an accepted opaque ancestor state is
        // collected into the song. These are snapshots, never automation gestures.
        let stamp = inner.updates.stamp();
        for (key, value) in protected {
            let (id, title) = opened
                .as_ref()
                .and_then(|o| {
                    o.params.iter().find(|(id, title)| {
                        key.parse::<u32>().ok() == Some(*id) || title.eq_ignore_ascii_case(&key)
                    })
                })
                .map(|(id, title)| (*id, title.clone()))
                .unwrap_or((key.parse().unwrap_or(u32::MAX), key));
            inner.params.push((
                stamp,
                ParamEdit {
                    id,
                    title,
                    value: Some(value),
                    phase: EditPhase::Snapshot,
                    at: Instant::now(),
                },
            ));
        }
        link.states_seen.fetch_add(1, Ordering::Release);
    }
    /// Makes the instance on its way the one that plays (ENG-066). A mix built again while the
    /// window is open (a latency that came late, ENG-065) takes the plugin to a new instance:
    /// the window follows it, and the one of `leaving` is closed.
    fn seat_next(&mut self, link: &PluginLink, leaving: Option<u32>) {
        let next = link.next.load(Ordering::Acquire);
        if next == 0 || !self.slots.contains_key(&next) {
            // What a new instance said is what is heard now, though it never reached the host.
            {
                let mut l = link.lock();
                if let Some(coming) = l.coming.take() {
                    l.heard = coming;
                }
            }
            if leaving.is_none() {
                link.next.store(0, Ordering::Release);
                link.slot.store(0, Ordering::Release);
            }
            return;
        }
        link.next.store(0, Ordering::Release);
        link.slot.store(next, Ordering::Release);
        self.publish(link);
        let shown = {
            let mut l = link.lock();
            if let Some(coming) = l.coming.take() {
                l.heard = coming;
            }
            l.editor.clone()
        };
        if let Some((title, caption)) = shown {
            if let Some(slot) = leaving {
                self.send(&ToHost::CloseEditor { slot });
            }
            self.send(&ToHost::OpenEditor {
                slot: next,
                title,
                caption,
            });
        }
    }

    /// Which seat of `link` the instance in `slot` has (ENG-067): `Some(true)` for one a new mix
    /// opens while another plays, `Some(false)` for the one that plays, `None` for one that is
    /// neither any more. An instance whose opening has not reached the host (`slot` 0) is a new
    /// one when another plays.
    fn seat_of(&self, link: &PluginLink, slot: u32) -> Option<bool> {
        let playing = link.slot.load(Ordering::Acquire);
        let next = link.next.load(Ordering::Acquire);
        if slot == 0 {
            Some(playing != 0 && self.slots.contains_key(&playing))
        } else if next != 0 && slot == next {
            Some(true)
        } else {
            (slot == playing).then_some(false)
        }
    }

    /// Sends to the host; a host that has gone is noticed by its watcher.
    fn send(&mut self, msg: &ToHost) -> bool {
        self.proc.as_ref().is_some_and(|p| {
            if p.stop.load(Ordering::Acquire) != KEEP {
                return false;
            }
            if p.writer.try_send(msg.clone()).is_ok() {
                return true;
            }
            // Dropping a control message could silently lose a state/parameter.
            // Retire this generation instead; its watcher reports all pending work.
            p.stop.store(KILL, Ordering::Release);
            false
        })
    }
}

/// The pipe can block forever when the child stops reading. Only this detached
/// owner writes it; neither the server mutex nor the caller waits for that write.
fn host_writer(
    mut pipe: impl io::Write + Send + 'static,
    stop: Arc<AtomicU8>,
) -> io::Result<mpsc::SyncSender<ToHost>> {
    let (tx, rx) = mpsc::sync_channel::<ToHost>(CONTROL_QUEUE);
    std::thread::Builder::new()
        .name("xirius-plugin-host-write".into())
        .spawn(move || {
            while let Ok(message) = rx.recv() {
                if stop.load(Ordering::Acquire) != KEEP {
                    break;
                }
                if wire::send(&mut pipe, &message.encode()).is_err() {
                    stop.store(KILL, Ordering::Release);
                    break;
                }
            }
        })?;
    Ok(tx)
}

// ---------------------------------------------------------------------------------------------
// The processor.

/// What a [`RemotePlugin`] opens.
#[derive(Clone, Debug, Default)]
pub struct RemoteSpec {
    pub name: String,
    pub bundle: PathBuf,
    /// The class id; `None` takes the bundle's first audio class.
    pub class: Option<[u8; 16]>,
    pub state: Option<PluginState>,
    /// Parameters by numeric id or title, normalised, applied after the state.
    pub params: Vec<(String, f64)>,
    /// The plugin makes sound from notes; its input is ignored.
    pub instrument: bool,
    /// Frames the host has for each block when playing in real time (`L`); at least the
    /// largest block, and it has to cover the device's callback too, since one callback is cut
    /// into several blocks. Offline rendering uses one block.
    pub latency: u32,
}

struct Conn {
    rings: Arc<SharedRings>,
    req: Event,
    done: Event,
    /// The slot the opening takes in the host, set as soon as the opening begins.
    slot: Arc<AtomicU32>,
}

static NEXT_MAPPING: AtomicU64 = AtomicU64::new(0);
static NEXT_FEED: AtomicU64 = AtomicU64::new(1);

/// A plugin in the host process, as a [`Processor`].
pub struct RemotePlugin {
    intent_generation: u64,
    automation: Option<rtrb::Consumer<ParamChange>>,
    /// Which of the link's automation producers is this instance's (ENG-066).
    feed: u64,
    block_events: Vec<NoteRecord>,
    spec: RemoteSpec,
    transfer: Option<LiveTransfer>,
    intent_bound: bool,
    server: PluginServer,
    link: Arc<PluginLink>,
    conn: Option<Conn>,
    offline: bool,
    l: u32,
    plugin_latency: u32,
    /// The host generation whose instance was opened in `prepare`.
    first_epoch: u64,
    /// The host generation whose instance the audio currently comes from (0: none yet).
    epoch: u64,
    /// A generation found stalled; dry until the next.
    stalled: u64,
    stall_frames: u64,
    late_frames: u64,
    /// Out frames owed by blocks that already went dry; dropped when they arrive.
    debt: u64,
    out_read: u64,
    /// Frames of a restarted instance still to cover with the dry signal, then the fade.
    warm: u64,
    fade: u32,
    dry_l: Vec<f32>,
    dry_r: Vec<f32>,
    dry_pos: usize,
    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
    /// The opening `begin_prepare` asked for, not taken yet.
    opening: Option<Opening>,
    /// The mix may take the plugin before the host has answered (ENG-065).
    late_ok: bool,
    /// The opening of a plugin the mix took before its answer: the audio side plays dry until
    /// the host publishes the instance.
    pending: Option<std::thread::JoinHandle<Result<Opened, String>>>,
    /// The first instance is not known yet: the first one the audio side sees is not a restart.
    late_first: bool,
}

/// An opening in the host that runs on a thread of its own (ENG-064).
struct Opening {
    setup: ProcessSetup,
    /// None when the shared memory could not be made (the link has the error).
    conn: Option<Conn>,
    answer: Option<std::thread::JoinHandle<Result<Opened, String>>>,
}

impl RemotePlugin {
    /// The session's StateIntent supplies the protected keys. Only accepted
    /// same-intent editor changes follow a graph replacement; captures do not.
    pub fn preserve_live_edits_since(&mut self, since: u64, protected: Vec<String>) {
        self.intent_bound = true;
        self.transfer = Some(LiveTransfer {
            since,
            protected,
            basis: None,
        });
    }
    pub fn preserve_updates_from(&mut self, basis: UpdateGate, since: u64, protected: Vec<String>) {
        self.intent_bound = true;
        self.intent_generation = basis.stamp().generation;
        if self.link.slot.load(Ordering::Acquire) == 0 {
            self.link.lock().updates = basis.rebind(0);
        }
        self.transfer = Some(LiveTransfer {
            since,
            protected,
            basis: Some(basis),
        });
    }
    /// Explicit class/state/reset intent rejects observations from the old editor.
    pub fn discard_previous_editor_changes(&mut self) {
        self.intent_bound = true;
        self.transfer = None;
    }
    pub fn new(server: PluginServer, link: Arc<PluginLink>, spec: RemoteSpec) -> RemotePlugin {
        let intent_generation = link.update_gate().stamp().generation;
        RemotePlugin {
            intent_generation,
            automation: None,
            feed: NEXT_FEED.fetch_add(1, Ordering::Relaxed),
            block_events: Vec::with_capacity(NOTE_CAPACITY * 2),
            spec,
            transfer: None,
            intent_bound: false,
            server,
            link,
            opening: None,
            late_ok: false,
            pending: None,
            late_first: false,
            conn: None,
            offline: false,
            l: 0,
            plugin_latency: 0,
            first_epoch: 0,
            epoch: 0,
            stalled: 0,
            stall_frames: 0,
            late_frames: 0,
            debt: 0,
            out_read: 0,
            warm: 0,
            fade: FADE,
            dry_l: Vec::new(),
            dry_r: Vec::new(),
            dry_pos: 0,
            scratch_l: Vec::new(),
            scratch_r: Vec::new(),
        }
    }

    pub fn link(&self) -> &Arc<PluginLink> {
        &self.link
    }

    /// Why the plugin could not be opened; it then plays dry.
    pub fn error(&self) -> Option<String> {
        self.link.error()
    }

    fn connect(&mut self, setup: &ProcessSetup) -> io::Result<(Conn, OpenSlot)> {
        let id = NEXT_MAPPING.fetch_add(1, Ordering::Relaxed);
        let base = format!("Local\\xirius-plugin-{}-{id}", std::process::id());
        // The in ring holds the blocks of the half second a host may stay late (a full ring is
        // a stall too): a host that has not started its first block under load is not taken
        // down sooner than that.
        let late = self.stall_frames as usize + self.l as usize + 2 * setup.max_block;
        let capacity = ((self.l as usize + setup.max_block) * 4)
            .max(late)
            .next_power_of_two()
            .max(8192);
        let shm = format!("{base}-shm");
        let req = format!("{base}-req");
        let done = format!("{base}-done");
        let rings = Arc::new(SharedRings::create(
            &shm,
            capacity,
            setup,
            self.l,
            self.spec.instrument,
        )?);
        let conn = Conn {
            rings,
            req: Event::create(&req)?,
            done: Event::create(&done)?,
            slot: Arc::new(AtomicU32::new(0)),
        };
        let open = OpenSlot {
            slot: 0,
            epoch: 0,
            generation: self.intent_generation,
            revision: 0,
            shm,
            req,
            done,
            bundle: self.spec.bundle.to_string_lossy().into_owned(),
            class: self.spec.class,
            state: self.spec.state.clone(),
            params: self.spec.params.clone(),
        };
        Ok((conn, open))
    }

    /// Takes the plugin out of the host. An opening still under way ends first, so that its
    /// instance is closed too: `wait` waits for it, otherwise (a mix that is dropped) another
    /// thread does, and the caller goes on at once (ENG-065).
    fn release(&mut self, wait: bool) {
        let feed = self.feed;
        self.link
            .automation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|f| f.0 != feed);
        self.automation = None;
        let mut unfinished = self.pending.take();
        if let Some(o) = self.opening.take()
            && let Some(conn) = o.conn
        {
            self.conn = Some(conn);
            unfinished = o.answer;
        }
        self.late_first = false;
        let Some(c) = self.conn.take() else {
            self.server.forget_slotless(&self.link);
            return;
        };
        if let Some(answer) = unfinished {
            if !wait && !answer.is_finished() {
                let server = self.server.clone();
                // Without a thread to spare the instance stays in the host until it goes away.
                let _ = std::thread::Builder::new()
                    .name("xirius-plugin-close".into())
                    .spawn(move || {
                        let _ = answer.join();
                        server.close(c.slot.load(Ordering::Acquire));
                    });
                return;
            }
            let _ = answer.join();
        }
        if let Some(last) = self.server.close(c.slot.load(Ordering::Acquire)) {
            // Opened again (a new block size), the plugin keeps what it was changed to.
            self.spec.state = last.state;
            self.spec.params = last.params;
        }
    }

    fn go_dry(&mut self, io: &mut AudioIo<'_>) {
        let n = io.frames();
        io.left.copy_from_slice(&self.scratch_l[..n]);
        io.right.copy_from_slice(&self.scratch_r[..n]);
        self.link.dry_blocks.fetch_add(1, Ordering::Relaxed);
        self.link.live.store(false, Ordering::Relaxed);
    }

    fn stall(&mut self) {
        self.stalled = self.epoch;
        self.link.stalled_epoch.store(self.epoch, Ordering::Release);
        self.server
            .inner
            .stall_epoch
            .store(self.epoch, Ordering::Release);
    }

    /// Sends this block to the host and fills `io` with what it returned `L` frames ago. False
    /// when that is not there (the caller plays the dry signal).
    fn pull_wet(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) -> bool {
        let n = io.frames() as u64;
        let Some(conn) = &self.conn else {
            return false;
        };
        let h = conn.rings.header();
        let capacity = conn.rings.capacity() as u64;

        let w = h.in_write.load(Ordering::Relaxed);
        if w + n - h.in_read.load(Ordering::Acquire) > capacity {
            self.stall();
            return false;
        }
        // SAFETY: we are the only writer of the in ring and the frames are free.
        unsafe { conn.rings.write(Ring::In, w, io.left, io.right) };
        self.block_events.clear();
        if let Some(automation) = &mut self.automation {
            // Bound work even if a non-audio producer keeps filling the queue.
            // Leave room for notes, especially note-offs; accepted automation stays queued
            // when the host is behind instead of silently losing the last lane value.
            let notes = if self.spec.instrument {
                events.len()
            } else {
                0
            };
            for _ in 0..conn.rings.note_slots().saturating_sub(notes) {
                let Ok(p) = automation.pop() else { break };
                self.block_events.push(NoteRecord {
                    frame: w + u64::from(p.offset).min(n - 1),
                    on: NOTE_PARAM,
                    param_id: p.id,
                    param_value: p.value,
                    ..NoteRecord::default()
                });
            }
        }
        if self.spec.instrument {
            if events.len() > NOTE_CAPACITY {
                h.notes_dropped
                    .fetch_add((events.len() - NOTE_CAPACITY) as u64, Ordering::Relaxed);
            }
            for e in events.iter().take(NOTE_CAPACITY) {
                // `on` is 1 for a note-on, 0 for a note-off and `NOTE_CONTROL` for a controller
                // change, which carries its number in `pitch` and its value in `velocity`.
                let (pitch, on, velocity) = match e.kind {
                    NoteKind::On { pitch, velocity } => (pitch, 1, velocity),
                    NoteKind::Off { pitch } => (pitch, 0, 0.0),
                    NoteKind::Control { number, value } => (number, NOTE_CONTROL, value as f32),
                };
                self.block_events.push(NoteRecord {
                    frame: w + e.offset as u64,
                    pitch,
                    on,
                    velocity,
                    ..NoteRecord::default()
                });
            }
        }
        // A future event at the ring's head must never hide an earlier note or parameter.
        // Stable insertion sort keeps the last value of equal-offset edits last, without
        // the temporary allocation of a stable library sort.
        for i in 1..self.block_events.len() {
            let mut j = i;
            while j > 0 && self.block_events[j - 1].frame > self.block_events[j].frame {
                self.block_events.swap(j - 1, j);
                j -= 1;
            }
        }
        for &event in &self.block_events {
            conn.rings.push_note(event);
        }
        h.in_write.store(w + n, Ordering::Release);
        conn.req.set();

        let need = self.debt + n;
        let mut available = h.out_write.load(Ordering::Acquire) - self.out_read;
        if self.offline {
            // Rendering offline may wait; give up after a while so a hung host cannot hang it.
            let started = Instant::now();
            while available < need {
                if self.server.inner.down_epoch.load(Ordering::Acquire) >= self.epoch {
                    return false;
                }
                if started.elapsed() > OFFLINE_TIMEOUT {
                    self.stall();
                    return false;
                }
                conn.done.wait(10);
                available = h.out_write.load(Ordering::Acquire) - self.out_read;
            }
        }
        // Frames for blocks that already went dry are dropped as they arrive.
        let stale = self.debt.min(available);
        self.out_read += stale;
        self.debt -= stale;
        available -= stale;

        let wet = available >= n;
        let mut stalled = false;
        if wet {
            // SAFETY: we are the only reader of the out ring and the host published the frames.
            unsafe { conn.rings.read(Ring::Out, self.out_read, io.left, io.right) };
            self.out_read += n;
            self.late_frames = 0;
        } else {
            self.debt += n;
            self.late_frames += n;
            self.link.misses.fetch_add(1, Ordering::Relaxed);
            stalled = self.late_frames > self.stall_frames;
        }
        h.out_read.store(self.out_read, Ordering::Release);
        if stalled {
            self.stall();
        }
        wet
    }
}

impl Drop for RemotePlugin {
    fn drop(&mut self) {
        self.release(false);
    }
}

impl Processor for RemotePlugin {
    fn name(&self) -> &str {
        &self.spec.name
    }

    fn child_time(&self) -> Option<(u64, u64)> {
        let h = self.conn.as_ref()?.rings.header();
        // The frames first: their time was added before they were published.
        let frames = h.in_read.load(Ordering::Acquire);
        Some((frames, h.host_ns.load(Ordering::Relaxed)))
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.reset(setup);
        let (conn, answer) = match self.connect(setup) {
            Ok((conn, open)) => {
                let r = self.server.open(
                    &self.link,
                    open,
                    conn.rings.clone(),
                    &conn.slot,
                    self.transfer.clone(),
                    self.intent_bound,
                );
                (Some(conn), Some(r))
            }
            Err(e) => {
                self.server.failed(&self.link, e.to_string());
                (None, None)
            }
        };
        self.take_opening(setup, conn, answer);
    }

    /// Asks the host to open the plugin on a thread of its own and returns at once, so that
    /// the song's other plugins are asked for meanwhile (ENG-064).
    fn begin_prepare(&mut self, setup: &ProcessSetup) {
        self.reset(setup);
        let (conn, answer) = match self.connect(setup) {
            Ok((conn, open)) => {
                let (server, link, rings) =
                    (self.server.clone(), self.link.clone(), conn.rings.clone());
                let job = (
                    server,
                    link,
                    open.clone(),
                    rings.clone(),
                    conn.slot.clone(),
                    self.transfer.clone(),
                    self.intent_bound,
                );
                let spawned = std::thread::Builder::new()
                    .name("xirius-plugin-open".into())
                    .spawn(move || {
                        let (server, link, open, rings, slot, transfer, intent_bound) = job;
                        server.open(&link, open, rings, &slot, transfer, intent_bound)
                    });
                match spawned {
                    Ok(answer) => (Some(conn), Some(answer)),
                    Err(_) => {
                        let r = self.server.open(
                            &self.link,
                            open,
                            rings,
                            &conn.slot,
                            self.transfer.clone(),
                            self.intent_bound,
                        );
                        return self.take_opening(setup, Some(conn), Some(r));
                    }
                }
            }
            Err(e) => {
                self.server.failed(&self.link, e.to_string());
                (None, None)
            }
        };
        self.opening = Some(Opening {
            setup: *setup,
            conn,
            answer,
        });
    }

    fn finish_prepare(&mut self, setup: &ProcessSetup) {
        match self.opening.take() {
            Some(o) if o.setup == *setup => {
                if self.late_ok
                    && o.conn.is_some()
                    && o.answer.as_ref().is_some_and(|a| !a.is_finished())
                {
                    return self.attach_late(setup, o);
                }
                let r = o.answer.map(|a| {
                    a.join().unwrap_or_else(|_| {
                        Err("プラグインを開くスレッドが止まりました".to_string())
                    })
                });
                self.take_opening(setup, o.conn, r);
            }
            other => {
                self.opening = other;
                self.prepare(setup);
            }
        }
    }

    fn preparing(&self) -> bool {
        self.opening
            .as_ref()
            .and_then(|o| o.answer.as_ref())
            .is_some_and(|a| !a.is_finished())
    }

    fn allow_late(&mut self) {
        self.late_ok = true;
    }

    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        self.play(io, events);
    }

    fn latency(&self) -> u32 {
        self.l + self.plugin_latency
    }
}

impl RemotePlugin {
    /// Everything before the opening: the old instance goes and the counts start again.
    fn reset(&mut self, setup: &ProcessSetup) {
        self.release(true);
        self.link.live.store(false, Ordering::Relaxed);
        let (producer, consumer) = rtrb::RingBuffer::new(NOTE_CAPACITY);
        self.link
            .automation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((self.feed, producer));
        self.automation = Some(consumer);
        self.offline = setup.mode == RenderMode::Offline;
        self.l = if self.offline {
            setup.max_block as u32
        } else {
            self.spec.latency.max(setup.max_block as u32)
        };
        self.stall_frames = (setup.sample_rate / 2.0) as u64;
        self.late_frames = 0;
        self.debt = 0;
        self.out_read = 0;
        self.epoch = 0;
        self.stalled = 0;
        self.warm = 0;
        self.fade = FADE;
        self.plugin_latency = 0;
        self.first_epoch = 0;
    }

    /// Takes the answer of the opening (none when the shared memory could not be made) and
    /// sizes the dry path to the plugin's latency.
    fn take_opening(
        &mut self,
        setup: &ProcessSetup,
        conn: Option<Conn>,
        answer: Option<Result<Opened, String>>,
    ) {
        if let Some(conn) = conn {
            if let Some(Ok(o)) = &answer {
                self.plugin_latency = o.latency;
                self.first_epoch = conn.rings.header().ready_epoch.load(Ordering::Acquire);
            }
            self.conn = Some(conn);
        }
        self.size_dry(setup);
    }

    /// Takes the plugin into the mix before the host has answered (ENG-065): the audio side
    /// plays the dry signal until the host publishes the instance, and then the plugin sounds
    /// from the block it is ready in. The latency is the one last known; when the answer says
    /// otherwise, [`PluginLink::take_latency_change`] has the mix built again.
    fn attach_late(&mut self, setup: &ProcessSetup, o: Opening) {
        self.plugin_latency = self.link.known_latency();
        self.conn = o.conn;
        self.pending = o.answer;
        self.late_first = true;
        self.size_dry(setup);
    }

    /// The dry path and its scratch blocks, sized to the plugin's latency, which the link
    /// learns for this instance (ENG-067).
    fn size_dry(&mut self, setup: &ProcessSetup) {
        let total = self.l + self.plugin_latency;
        let slot = self
            .conn
            .as_ref()
            .map_or(0, |c| c.slot.load(Ordering::Acquire));
        self.server
            .built(&self.link, slot, total, u64::from(self.plugin_latency) + 1);
        let dry_len = (total as usize + setup.max_block).next_power_of_two();
        self.dry_l = vec![0.0; dry_len];
        self.dry_r = vec![0.0; dry_len];
        self.dry_pos = 0;
        self.scratch_l = vec![0.0; setup.max_block];
        self.scratch_r = vec![0.0; setup.max_block];
    }

    fn play(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        let n = io.frames();
        if self.spec.instrument {
            io.clear();
        }
        // The dry path: the input delayed as much as the wet one, for the blocks the host
        // does not deliver.
        let mask = self.dry_l.len() - 1;
        let back = self.dry_l.len() - (self.l + self.plugin_latency) as usize;
        for i in 0..n {
            let w = (self.dry_pos + i) & mask;
            self.dry_l[w] = io.left[i];
            self.dry_r[w] = io.right[i];
            let r = (w + back) & mask;
            self.scratch_l[i] = self.dry_l[r];
            self.scratch_r[i] = self.dry_r[r];
        }
        self.dry_pos = (self.dry_pos + n) & mask;

        let Some(conn) = &self.conn else {
            return self.go_dry(io);
        };
        let h = conn.rings.header();
        let ready = h.ready_epoch.load(Ordering::Acquire);
        let down = self.server.inner.down_epoch.load(Ordering::Acquire);
        // While the host is down the rings are left alone: a new instance lines them up.
        if ready <= down || ready == self.stalled {
            return self.go_dry(io);
        }
        if ready != self.epoch {
            // A fresh instance: the first one, or one after a restart.
            self.out_read = h.out_read.load(Ordering::Acquire);
            self.debt = 0;
            self.late_frames = 0;
            if self.late_first && self.epoch == 0 {
                self.late_first = false;
                self.first_epoch = ready;
            }
            if self.epoch != 0 || ready != self.first_epoch {
                // Its first `L` frames are silence and the plugin starts from nothing: keep
                // the dry signal over them and fade over to the plugin after.
                self.warm = (self.l + self.plugin_latency) as u64;
                self.fade = 0;
            }
            self.epoch = ready;
        }
        if !self.pull_wet(io, events) {
            return self.go_dry(io);
        }
        if self.warm > 0 || self.fade < FADE {
            for i in 0..n {
                if self.warm > 0 {
                    io.left[i] = self.scratch_l[i];
                    io.right[i] = self.scratch_r[i];
                    self.warm -= 1;
                } else if self.fade < FADE {
                    let g = self.fade as f32 / FADE as f32;
                    io.left[i] = self.scratch_l[i] * (1.0 - g) + io.left[i] * g;
                    io.right[i] = self.scratch_r[i] * (1.0 - g) + io.right[i] * g;
                    self.fade += 1;
                }
            }
        }
        self.link.wet_blocks.fetch_add(1, Ordering::Relaxed);
        self.link.live.store(self.warm == 0, Ordering::Relaxed);
    }
}
