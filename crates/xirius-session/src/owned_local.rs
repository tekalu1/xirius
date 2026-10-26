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

#[derive(Clone)]
pub(crate) struct Capture(Arc<Mutex<CaptureInner>>);

struct CaptureInner {
    controller: Option<Controller>,
    due: Instant,
    task: Option<Ticket<String>>,
    completed: bool,
    value: Option<Result<String, String>>,
    error: Option<String>,
    validity: Arc<Mutex<Option<String>>>,
}

impl Capture {
    fn new(controller: Controller, due: Instant, validity: Arc<Mutex<Option<String>>>) -> Self {
        Self(Arc::new(Mutex::new(CaptureInner {
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
            match controller.on_owner(move |plugin| {
                if let Some(error) = validity.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                    return Err(error);
                }
                plugin
                    .get_state()
                    .map(|s| xirius_plugins::encode_state(&s))
                    .map_err(|e| e.to_string())
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

    fn take(&self) -> Option<Result<String, String>> {
        self.poll();
        self.0.try_lock().ok()?.value.take()
    }

    fn cancel(&self) {
        if let Ok(mut c) = self.0.try_lock() {
            c.controller = None;
            c.task = None;
            c.completed = true;
            let error = "プラグインの状態要求が変更により無効になりました".to_string();
            c.error = Some(error.clone());
            c.value = Some(Err(error));
        }
    }
}

pub(crate) struct OwnedLocal {
    controller: Controller,
    params: Vec<(u32, String)>,
    requests: RefCell<Vec<Ticket<()>>>,
    edits: RefCell<Option<Ticket<Edits>>>,
    captures: RefCell<Vec<Capture>>,
    error: RefCell<Option<String>>,
    validity: Arc<Mutex<Option<String>>>,
    flags: Cell<i32>,
    visible: Cell<bool>,
    play: Cell<u32>,
    names: RefCell<Vec<(u8, String)>>,
    pub latency: Cell<u32>,
    due: Cell<Option<Instant>>,
}

impl OwnedLocal {
    pub fn new(
        controller: Controller,
        params: Vec<(u32, String)>,
        names: Vec<(u8, String)>,
        latency: u32,
    ) -> Self {
        Self {
            controller,
            params,
            requests: Default::default(),
            edits: Default::default(),
            captures: Default::default(),
            error: Default::default(),
            validity: Default::default(),
            flags: Cell::new(0),
            visible: Cell::new(false),
            play: Cell::new(0),
            names: RefCell::new(names),
            latency: Cell::new(latency),
            due: Cell::new(None),
        }
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
        let capture = Capture::new(
            self.controller.clone(),
            Instant::now() + delay,
            self.validity.clone(),
        );
        self.captures.borrow_mut().push(capture.clone());
        capture
    }

    pub fn apply(&self, value: &Plugin) -> bool {
        for capture in self.captures.borrow().iter() {
            capture.cancel();
        }
        self.captures.borrow_mut().clear();
        self.due.set(None);
        let value = value.clone();
        let validity = self.validity.clone();
        self.keep(self.controller.on_owner(move |plugin| {
            let result = (|| {
                if let Some(text) = value.state.as_deref().filter(|s| !s.is_empty()) {
                    let state = xirius_plugins::decode_state(text).map_err(|e| e.to_string())?;
                    plugin.set_state(&state).map_err(|e| e.to_string())?;
                }
                let params = plugin.params();
                for (key, v) in &value.params {
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
            *validity.lock().unwrap_or_else(|e| e.into_inner()) = result.as_ref().err().cloned();
            result
        }))
    }

    pub fn editor(&self, title: String, caption: Option<Caption>, open: bool) -> bool {
        if open {
            let place = Placement {
                caption,
                ..Placement::shown(None)
            };
            self.keep(self.controller.editor(title, place))
        } else {
            self.keep(self.controller.close_editor())
        }
    }

    pub fn caption(&self, caption: Caption) {
        self.keep(self.controller.caption(caption));
    }

    pub fn error(&self) -> Option<String> {
        self.poll_requests();
        self.error.borrow().clone()
    }
    pub fn visible(&self) -> bool {
        self.visible.get()
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
        self.poll_requests();
        let result = self.edits.borrow().as_ref().and_then(Ticket::poll);
        let mut params = Vec::new();
        if let Some(result) = result {
            *self.edits.borrow_mut() = None;
            match result {
                Ok(edits) => {
                    self.flags.set(self.flags.get() | edits.restart_flags);
                    self.visible.set(edits.editor_visible);
                    self.play
                        .set(self.play.get().saturating_add(edits.play_requests));
                    self.latency.set(edits.latency);
                    *self.names.borrow_mut() = edits.note_names;
                    if edits.restart_flags & 4 != 0 {
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
        if self.edits.borrow().is_none() {
            match self.controller.edits() {
                Ok(task) => *self.edits.borrow_mut() = Some(task),
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
                        state = Some(value);
                        *self.error.borrow_mut() = None;
                    }
                    Err(error) => *self.error.borrow_mut() = Some(error),
                }
            }
            !capture.ready()
        });
        (state.is_some() || !params.is_empty()).then_some(RemoteEdit {
            plugin: id,
            state,
            params,
        })
    }
}
