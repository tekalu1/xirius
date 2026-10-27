//! The loader's value-only connection to one local VST3 owner (PRJ-030).
//! No method here calls the SDK or waits for a reply on the calling thread.

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use xirius_core::id::PluginId;
use xirius_core::model::Plugin;
use xirius_engine::server::{EditPhase, ParamEdit};
use xirius_vst3::owner::{Controller, Edits, Ticket};
use xirius_vst3::{Caption, Gesture, Placement};

use crate::plugins::RemoteEdit;
use xirius_engine::plugin_updates::{Field, Stamp, UpdateGate, new_instance};

#[derive(Clone)]
pub(crate) struct Capture(Arc<Mutex<CaptureInner>>);

struct CaptureInner {
    updates: Arc<Mutex<UpdateGate>>,
    stamp: Stamp,
    controller: Option<Controller>,
    due: Instant,
    task: Option<Ticket<Snapshot>>,
    completed: bool,
    value: Option<Result<Snapshot, String>>,
    error: Option<String>,
    validity: Arc<Mutex<Option<String>>>,
}

impl Capture {
    fn new(
        controller: Controller,
        due: Instant,
        validity: Arc<Mutex<Option<String>>>,
        updates: Arc<Mutex<UpdateGate>>,
    ) -> Self {
        let stamp = updates.lock().unwrap_or_else(|e| e.into_inner()).stamp();
        Self(Arc::new(Mutex::new(CaptureInner {
            updates,
            stamp,
            controller: Some(controller),
            due,
            task: None,
            completed: false,
            value: None,
            error: None,
            validity,
        })))
    }

    fn poll(&self) {
        let Ok(mut capture) = self.0.try_lock() else {
            return;
        };
        if capture.completed || Instant::now() < capture.due {
            return;
        }
        if let Some(controller) = capture.controller.take() {
            let validity = capture.validity.clone();
            let stamp = capture.stamp;
            let updates = capture.updates.clone();
            match controller.on_owner_versioned(move |plugin, serial| {
                let stamp = Stamp {
                    revision: serial,
                    ..stamp
                };
                #[cfg(feature = "test-io")]
                xirius_core::io_test::before(
                    "prepare_get_state",
                    std::path::Path::new("local-plugin-state"),
                )
                .map_err(|e| e.to_string())?;
                if let Some(error) = validity.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                    return Err(error);
                }
                if !updates
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .accepts(stamp, Field::Receipt)
                {
                    return Err(xirius_engine::plugin_updates::STALE_UPDATE.into());
                }
                let state = plugin.get_state().map_err(|e| e.to_string())?;
                let params = plugin
                    .params()
                    .iter()
                    .filter_map(|p| plugin.param(p.id).map(|v| (p.id, p.title.clone(), v)))
                    .collect();
                if !updates
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .accepts(stamp, Field::Receipt)
                {
                    return Err(xirius_engine::plugin_updates::STALE_UPDATE.into());
                }
                Ok(Snapshot {
                    stamp,
                    state: xirius_plugins::encode_state(&state),
                    params,
                })
            }) {
                Ok(task) => capture.task = Some(task),
                Err(error) => {
                    capture.error = Some(error.clone());
                    capture.value = Some(Err(error));
                    capture.completed = true;
                    return;
                }
            }
        }
        if let Some(result) = capture.task.as_ref().and_then(Ticket::poll) {
            if let Ok(snapshot) = &result {
                capture.stamp = snapshot.stamp;
            }
            capture.error = result.as_ref().err().cloned();
            capture.value = Some(result);
            capture.completed = true;
            capture.task = None;
        }
    }

    pub fn ready(&self) -> bool {
        self.poll();
        self.0.try_lock().is_ok_and(|c| c.completed)
    }

    pub fn error(&self) -> Option<String> {
        self.poll();
        self.0.try_lock().ok().and_then(|c| c.error.clone())
    }

    pub(crate) fn take(&self) -> Option<Result<Snapshot, String>> {
        self.poll();
        let mut capture = self.0.try_lock().ok()?;
        let value = capture.value.take()?;
        if !capture
            .updates
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .accepts(capture.stamp, Field::Receipt)
        {
            return Some(Err(xirius_engine::plugin_updates::STALE_UPDATE.into()));
        }
        Some(value)
    }

    pub(crate) fn cancel(&self) {
        if let Ok(mut c) = self.0.try_lock() {
            c.controller = None;
            c.task = None;
            c.completed = true;
            let error = "プラグインの状態要求が取り消されました".to_string();
            c.error = Some(error);
            // Cancellation belongs to this receipt; it is neither a plugin
            // edit to collect later nor a fault of the running instance.
            c.value = None;
        }
    }

    pub(crate) fn quarantine(&self) {
        if let Ok(c) = self.0.try_lock() {
            if let Some(task) = &c.task {
                task.quarantine();
            }
            if let Some(controller) = &c.controller {
                controller.quarantine();
            }
        }
        self.cancel();
    }
}

type PendingEdits = Option<(Stamp, Ticket<Edits>)>;
pub(crate) struct OwnedLocal {
    updates: Arc<Mutex<UpdateGate>>,
    controller: Controller,
    retirement: xirius_vst3::owner::Retirement,
    params: Vec<(u32, String)>,
    requests: RefCell<Vec<Ticket<()>>>,
    edits: Arc<Mutex<PendingEdits>>,
    poll_gui: Cell<bool>,
    captures: RefCell<Vec<Capture>>,
    error: RefCell<Option<String>>,
    validity: Arc<Mutex<Option<String>>>,
    flags: Cell<i32>,
    visible: Cell<bool>,
    editor_intent: Cell<Option<bool>>,
    editor_job: RefCell<Option<Ticket<()>>>,
    play: Cell<u32>,
    names: RefCell<Vec<(u8, String)>>,
    pub latency: Cell<u32>,
    due: Cell<Option<Instant>>,
}

pub(crate) struct Snapshot {
    pub stamp: Stamp,
    pub state: String,
    pub params: Vec<(u32, String, f64)>,
}
impl OwnedLocal {
    /// Value-only proxy for a background plan builder. The SDK stays on its
    /// existing owner; no plugin instance or audio part is moved here.
    pub fn fork(&self) -> Self {
        let mut copy = Self::new(
            self.controller.clone(),
            self.retirement.clone(),
            self.params.clone(),
            self.names.borrow().clone(),
            self.latency.get(),
        );
        copy.validity = self.validity.clone();
        copy.updates = self.updates.clone();
        copy.edits = self.edits.clone();
        copy.poll_gui.set(false);
        copy.visible.set(self.visible());
        copy
    }

    pub fn snapshot(&self) -> Result<Ticket<Snapshot>, String> {
        let updates = self.updates.clone();
        let stamp = self.update_gate().stamp();
        self.controller.on_owner_versioned(move |plugin, serial| {
            let stamp = Stamp {
                revision: serial,
                ..stamp
            };
            if !updates
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .accepts(stamp, Field::Receipt)
            {
                return Err(xirius_engine::plugin_updates::STALE_UPDATE.into());
            }
            let state = plugin.get_state().map_err(|e| e.to_string())?;
            let params = plugin
                .params()
                .iter()
                .filter_map(|p| {
                    plugin
                        .param(p.id)
                        .map(|value| (p.id, p.title.clone(), value))
                })
                .collect();
            if !updates
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .accepts(stamp, Field::Receipt)
            {
                return Err(xirius_engine::plugin_updates::STALE_UPDATE.into());
            }
            Ok(Snapshot {
                stamp,
                state: xirius_plugins::encode_state(&state),
                params,
            })
        })
    }
    pub fn new(
        controller: Controller,
        retirement: xirius_vst3::owner::Retirement,
        params: Vec<(u32, String)>,
        names: Vec<(u8, String)>,
        latency: u32,
    ) -> Self {
        Self {
            updates: Arc::new(Mutex::new(UpdateGate::new(Stamp {
                instance: new_instance(),
                generation: 0,
                revision: 0,
            }))),
            controller,
            retirement,
            params,
            requests: Default::default(),
            edits: Default::default(),
            poll_gui: Cell::new(true),
            captures: Default::default(),
            error: Default::default(),
            validity: Default::default(),
            flags: Cell::new(0),
            visible: Cell::new(false),
            editor_intent: Cell::new(None),
            editor_job: RefCell::new(None),
            play: Cell::new(0),
            names: RefCell::new(names),
            latency: Cell::new(latency),
            due: Cell::new(None),
        }
    }

    pub fn resume_edits(&self) {
        self.poll_gui.set(true);
    }
    pub fn update_gate(&self) -> UpdateGate {
        self.updates
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn inherit_updates(&self, basis: &UpdateGate, protected: Vec<String>) {
        let mut gate = self.updates.lock().unwrap_or_else(|e| e.into_inner());
        *gate = basis.successor(gate.stamp().instance, protected);
    }
    pub fn invalidate(&self, replace: bool, keys: &[String]) {
        let mut gate = self.updates.lock().unwrap_or_else(|e| e.into_inner());
        let next = gate.next_generation();
        gate.explicit(next, replace, keys);
    }
    pub fn retirement(&self) -> xirius_vst3::owner::Retirement {
        self.retirement.clone()
    }

    fn keep(&self, task: Result<Ticket<()>, String>) -> bool {
        match task {
            Ok(task) => {
                self.requests.borrow_mut().push(task);
                true
            }
            Err(error) => {
                *self.error.borrow_mut() = Some(error);
                false
            }
        }
    }

    fn poll_requests(&self) {
        self.requests.borrow_mut().retain(|task| match task.poll() {
            None => true,
            Some(Ok(())) => false,
            Some(Err(error)) => {
                *self.error.borrow_mut() = Some(error);
                false
            }
        });
    }

    pub fn capture(&self, delay: Duration) -> Capture {
        let capture = self.request_capture(delay);
        self.captures.borrow_mut().push(capture.clone());
        capture
    }

    pub fn request_capture(&self, delay: Duration) -> Capture {
        Capture::new(
            self.controller.clone(),
            Instant::now() + delay,
            self.validity.clone(),
            self.updates.clone(),
        )
    }

    pub fn apply(&self, value: &Plugin, previous: &Plugin) -> bool {
        let state_changed = value.state != previous.state;
        if state_changed && value.state.as_deref().is_none_or(str::is_empty) {
            return false;
        }
        for capture in self.captures.borrow().iter() {
            capture.cancel();
        }
        self.captures.borrow_mut().clear();
        self.due.set(None);
        let state = state_changed.then(|| value.state.clone()).flatten();
        let changed: Vec<_> = value
            .params
            .iter()
            .filter(|(key, value)| state_changed || previous.params.get(*key) != Some(*value))
            .map(|(key, value)| (key.clone(), *value))
            .collect();
        let validity = self.validity.clone();
        let updates = self.updates.clone();
        let stamp = self.update_gate().stamp();
        self.keep(self.controller.on_owner(move |plugin| {
            if !updates
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .accepts(stamp, Field::Receipt)
            {
                return Ok(());
            }
            let result = (|| {
                if let Some(text) = state.as_deref() {
                    let state = xirius_plugins::decode_state(text).map_err(|e| e.to_string())?;
                    plugin.set_state(&state).map_err(|e| e.to_string())?;
                }
                let params = plugin.params();
                for (key, v) in &changed {
                    let id = key
                        .parse::<u32>()
                        .ok()
                        .filter(|id| params.iter().any(|p| p.id == *id))
                        .or_else(|| {
                            params
                                .iter()
                                .find(|p| p.title.eq_ignore_ascii_case(key))
                                .map(|p| p.id)
                        })
                        .ok_or_else(|| format!("パラメーター {key} が見つかりません"))?;
                    if !plugin.set_param(id, v.clamp(0.0, 1.0)) {
                        return Err(format!("パラメーター {key} を変更できません"));
                    }
                }
                plugin.handler().take_gestures();
                Ok(())
            })();
            if updates
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .accepts(stamp, Field::Receipt)
            {
                *validity.lock().unwrap_or_else(|e| e.into_inner()) =
                    result.as_ref().err().cloned();
            } else {
                return Ok(());
            }
            result
        }))
    }

    pub fn editor(&self, title: String, caption: Option<Caption>, open: bool) -> bool {
        let job = if open {
            self.controller.editor(
                title,
                Placement {
                    caption,
                    ..Placement::shown(None)
                },
            )
        } else {
            self.controller.close_editor()
        };
        match job {
            Ok(job) => {
                // Reports queued before this command cannot close a newly opened
                // editor or erase the handoff intention.
                if let Ok(mut edits) = self.edits.try_lock() {
                    edits.take();
                }
                self.editor_intent.set(Some(open));
                *self.editor_job.borrow_mut() = Some(job);
                true
            }
            Err(error) => {
                *self.error.borrow_mut() = Some(error);
                false
            }
        }
    }

    fn poll_editor(&self) -> bool {
        let result = self.editor_job.borrow().as_ref().and_then(Ticket::poll);
        if let Some(result) = result {
            self.editor_job.borrow_mut().take();
            let open = self.editor_intent.take().unwrap_or(false);
            self.visible.set(open && result.is_ok());
            if let Err(error) = result {
                *self.error.borrow_mut() = Some(error);
            }
        }
        self.editor_job.borrow().is_none()
    }

    pub fn caption(&self, caption: Caption) {
        self.keep(self.controller.caption(caption));
    }

    pub fn error(&self) -> Option<String> {
        self.poll_requests();
        self.error.borrow().clone()
    }
    pub fn visible(&self) -> bool {
        self.editor_intent.get().unwrap_or(self.visible.get())
    }
    pub fn take_play_requests(&self) -> u32 {
        self.play.replace(0)
    }
    pub fn take_latency(&self) -> bool {
        let flags = self.flags.get();
        self.flags.set(flags & !8);
        flags & 8 != 0
    }
    pub fn names(&self) -> Vec<(u8, String)> {
        self.names.borrow().clone()
    }

    pub fn take(&self, id: PluginId) -> Option<RemoteEdit> {
        if !self.poll_gui.get() {
            return None;
        }
        self.poll_requests();
        if !self.poll_editor() {
            return None;
        }
        let result = {
            let Ok(mut pending) = self.edits.try_lock() else {
                return None;
            };
            let result = pending
                .as_ref()
                .and_then(|(stamp, ticket)| ticket.poll().map(|result| (*stamp, result)));
            if result.is_some() {
                pending.take();
            }
            result
        };
        let mut params = Vec::new();
        if let Some((stamp, result)) = result
            && self.update_gate().accepts(
                Stamp {
                    revision: result.as_ref().map_or(stamp.revision, |edits| edits.serial),
                    ..stamp
                },
                Field::Receipt,
            )
        {
            match result {
                Ok(edits) => {
                    let stamp = Stamp {
                        revision: edits.serial,
                        ..stamp
                    };
                    self.flags.set(self.flags.get() | edits.restart_flags);
                    self.visible.set(edits.editor_visible);
                    self.play
                        .set(self.play.get().saturating_add(edits.play_requests));
                    self.latency.set(edits.latency);
                    *self.names.borrow_mut() = edits.note_names;
                    if edits.restart_flags & 4 != 0 {
                        self.updates
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .observed(stamp, Field::State);
                        self.due
                            .set(Some(Instant::now() + super::plugins::CAPTURE_WAIT));
                    }
                    for gesture in edits.gestures {
                        let (pid, phase, value) = match gesture {
                            Gesture::Begin(pid) => (pid, EditPhase::Begin, None),
                            Gesture::Perform(pid, v) => (pid, EditPhase::Change, Some(v)),
                            Gesture::End(pid) => (pid, EditPhase::End, None),
                        };
                        if phase == EditPhase::End {
                            self.due
                                .set(Some(Instant::now() + super::plugins::CAPTURE_WAIT));
                        }
                        let title = self
                            .params
                            .iter()
                            .find(|(p, _)| *p == pid)
                            .map(|(_, t)| t.clone())
                            .unwrap_or_default();
                        self.updates
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .observed(
                                stamp,
                                Field::Parameter {
                                    id: pid,
                                    title: &title,
                                },
                            );
                        params.push(ParamEdit {
                            id: pid,
                            phase,
                            value,
                            title,
                            at: Instant::now(),
                        });
                    }
                }
                Err(error) => *self.error.borrow_mut() = Some(error),
            }
        }
        if let Ok(mut pending) = self.edits.try_lock()
            && pending.is_none()
        {
            let stamp = self.update_gate().stamp();
            match self.controller.edits() {
                Ok(task) => *pending = Some((stamp, task)),
                Err(error) => *self.error.borrow_mut() = Some(error),
            }
        }
        if self.due.get().is_some_and(|due| Instant::now() >= due) {
            self.due.set(None);
            self.capture(Duration::ZERO);
        }
        let mut state = None;
        self.captures.borrow_mut().retain(|capture| {
            if let Some(result) = capture.take() {
                match result {
                    Ok(value) => {
                        state = Some(value.state);
                        params.extend(value.params.into_iter().map(|(id, title, value)| {
                            ParamEdit {
                                id,
                                title,
                                value: Some(value),
                                phase: EditPhase::Snapshot,
                                at: Instant::now(),
                            }
                        }));
                        *self.error.borrow_mut() = None;
                    }
                    Err(error) if error == xirius_engine::plugin_updates::STALE_UPDATE => {}
                    Err(error) => *self.error.borrow_mut() = Some(error),
                }
            }
            !capture.ready()
        });
        (state.is_some() || !params.is_empty()).then_some(RemoteEdit {
            origin: xirius_engine::plugin_updates::Origin::Editor,
            stamp: self.update_gate().stamp(),
            plugin: id,
            state,
            params,
        })
    }
}
