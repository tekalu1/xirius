//! PRJ-029: one nonblocking replacement boundary for the window and its MCP connection.
use crate::studio::{FileRequest, Geometry, Studio};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};
use xirius_core::CmdError;
use xirius_core::session::RunResult;
use xirius_core::session::opening::{OpenId, OpenJob, OpenRequest, OpenStatus};

thread_local! { static CACHED_PAINT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
/// While the old song is only a preview, painting may consult existing caches
/// but must not revalidate files or request fresh directory/peak reads.
pub(crate) struct PaintScope(bool);
impl PaintScope {
    pub(crate) fn enter(active: bool) -> Self {
        let old = CACHED_PAINT.get();
        CACHED_PAINT.set(old || active);
        Self(old)
    }
}
impl Drop for PaintScope {
    fn drop(&mut self) {
        CACHED_PAINT.set(self.0);
    }
}
pub(crate) fn cached_paint() -> bool {
    CACHED_PAINT.get()
}

pub const TITLE: &str = "曲を開いています";
pub const CANCELLING: &str = "中止しています";
pub const BUSY: &str = "読み込み中は別の曲を開けません";
pub const CLOSE: &str = "中止して閉じますか";

pub(crate) enum Request {
    File(FileRequest, PathBuf),
    New,
    Sample,
    Startup(PathBuf),
    Core(OpenRequest),
}

type AfterOpen = Box<dyn FnOnce(&mut Studio)>;

pub(crate) struct Opening {
    pub job: Option<OpenJob>,
    id: OpenId,
    pending: Option<OpenRequest>,
    pull: Option<xirius_session::plugins::StatePull>,
    pub(crate) after: Option<AfterOpen>,
    pub since: Instant,
    pub status: OpenStatus,
    pub cancelled: bool,
    pub confirm_close: bool,
    pub close_focus: bool,
    pub full_name: bool,
    pub name_focus: bool,
    pub cancel_hover: bool,
    last_display: Instant,
    last_spoken: Instant,
    pub announcement: String,
    sample: bool,
    startup: bool,
    closing: Result<Option<xirius_session::cleanup::Closed>, String>,
    mcp: Option<xirius_mcp::Job>,
}

pub(crate) struct CloseJob {
    rx: std::sync::mpsc::Receiver<Result<usize, String>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Opening {
    pub fn visible(&self, now: Instant) -> bool {
        self.confirm_close
            || now.saturating_duration_since(self.since) >= crate::wait::OVERLAY_AFTER
    }
    pub fn title(&self) -> &'static str {
        if self.cancelled { CANCELLING } else { TITLE }
    }
    pub fn state(&self) -> &'static str {
        if self.cancelled {
            return "読み込み";
        }
        match self.status.stage {
            "writer" => "保存待ち",
            "lock" => "ロック待ち",
            _ => "読み込み",
        }
    }
    pub fn detail(&self) -> String {
        format!("{} {} ファイル", self.state(), count(self.status.reads))
    }
    fn update(&mut self, now: Instant, reduced: bool) {
        if self.cancelled {
            return;
        }
        let Some(latest) = self.job.as_ref().and_then(|job| job.progress.latest()) else {
            return;
        };
        self.display(latest, now, reduced);
    }
    fn display(&mut self, latest: OpenStatus, now: Instant, reduced: bool) {
        if self.cancelled {
            return;
        }
        let stage_changed = state_of(&latest) != state_of(&self.status);
        let interval = Duration::from_millis(if reduced { 1000 } else { 250 });
        if stage_changed || now.saturating_duration_since(self.last_display) >= interval {
            self.status = latest;
            self.last_display = now;
        }
        if stage_changed
            || now.saturating_duration_since(self.last_spoken) >= Duration::from_secs(5)
        {
            self.announcement = self.detail();
            self.last_spoken = now;
        }
    }
}
fn state_of(s: &OpenStatus) -> &str {
    match s.stage {
        "writer" => "save",
        "lock" => "lock",
        _ => "read",
    }
}
pub fn count(n: u64) -> String {
    let s = n.to_string();
    s.chars()
        .enumerate()
        .flat_map(|(i, c)| {
            if i > 0 && (s.len() - i).is_multiple_of(3) {
                vec![',', c]
            } else {
                vec![c]
            }
        })
        .collect()
}
fn empty() -> xirius_core::model::Project {
    xirius_core::model::Project::new(crate::song::EMPTY_NAME)
}
fn startup(root: &Path) -> xirius_core::CmdResult<OpenRequest> {
    let keep = crate::keep::Keep::at(root.to_path_buf());
    if let Some(r) = keep.crashed() {
        return Ok(OpenRequest::Song(r.dir));
    }
    Ok(match keep.last_open() {
        Some(crate::keep::LastOpen::Song(path)) => OpenRequest::Song(path),
        Some(crate::keep::LastOpen::Sample) => {
            OpenRequest::Build(crate::song::Song::sample_project)
        }
        _ => OpenRequest::Build(empty),
    })
}
impl Studio {
    #[cfg(test)]
    pub(crate) fn wait_for_open_for_test(&mut self) {
        let end = Instant::now() + Duration::from_secs(30);
        while self.opening.is_some() {
            assert!(Instant::now() < end);
            self.poll_opening();
            std::thread::yield_now();
        }
    }

    pub(crate) fn input_blocked(&self) -> bool {
        self.opening.is_some() || self.saving.is_some()
    }
    pub(crate) fn request_open_file(&mut self, request: FileRequest, path: &Path) -> bool {
        self.request_open(Request::File(request, path.to_path_buf()))
    }
    pub(crate) fn request_open(&mut self, request: Request) -> bool {
        if self.opening.is_some() {
            self.warn(BUSY.into(), None);
            return false;
        }
        if self.saving.is_some()
            || self.transport.take.is_some()
            || self.export.running()
            || self.dawproject.is_some()
        {
            self.warn("録音・保存・書き出しの途中は曲を開けません".into(), None);
            return false;
        }
        let is_startup = matches!(&request, Request::Startup(_));
        let (request, sample) = match request {
            Request::File(FileRequest::OpenVersion(id), destination) => {
                (OpenRequest::Version { id, destination }, false)
            }
            Request::File(FileRequest::OpenDawproject, path) => {
                let media = crate::dawproject_ui::media_dir(self.os_files, &path);
                (OpenRequest::Dawproject { path, media }, false)
            }
            Request::File(_, path) => {
                if xirius_core::dawproject::is_dawproject(&path) {
                    let media = crate::dawproject_ui::media_dir(self.os_files, &path);
                    (OpenRequest::Dawproject { path, media }, false)
                } else {
                    (OpenRequest::Song(path), false)
                }
            }
            Request::New => (OpenRequest::Build(empty), false),
            Request::Sample => (OpenRequest::Build(crate::song::Song::sample_project), true),
            Request::Startup(path) => (
                OpenRequest::Resolve {
                    path,
                    resolve: startup,
                },
                false,
            ),
            Request::Core(request) => (request, false),
        };
        let closing = self.closing();
        if let Err(e) = &closing {
            self.warn(e.clone(), None);
            return false;
        }
        if let OpenRequest::Song(path) = &request {
            self.stop_tidy(path);
        }
        self.transport.pause();
        self.pc_keys_release();
        self.roll.cancel();
        self.popup = None;
        self.palette = None;
        self.preparing = None;
        self.error = None;
        // Plugin state collection has its existing nonblocking mailbox; the writer
        // flush and all candidate construction belong to OpenJob.
        let pull = self.transport.host_mut().map(|h| h.begin_pull_states());
        self.open_generation += 1;
        let id = OpenId {
            job: self.open_generation,
            generation: self.open_generation,
        };
        let now = crate::wait::now();
        self.opening = Some(Opening {
            job: None,
            id,
            pending: Some(request),
            pull,
            after: None,
            since: now,
            status: OpenStatus {
                stage: "writer",
                ..Default::default()
            },
            cancelled: false,
            confirm_close: false,
            close_focus: false,
            full_name: false,
            name_focus: false,
            cancel_hover: false,
            last_display: now,
            last_spoken: now,
            announcement: TITLE.into(),
            sample,
            startup: is_startup,
            closing,
            mcp: None,
        });
        true
    }
    pub(crate) fn after_open(&mut self, after: impl FnOnce(&mut Studio) + 'static) {
        self.opening.as_mut().expect("an open request").after = Some(Box::new(after));
    }

    pub(crate) fn cancel_opening(&mut self) {
        if let Some(open) = &mut self.opening
            && !open.cancelled
            && open.job.as_ref().is_none_or(|j| j.cancel())
        {
            open.cancelled = true;
            open.announcement = CANCELLING.into();
            self.open_generation += 1;
        }
    }
    pub(crate) fn opening_key(&mut self, key: &str) -> bool {
        let Some(open) = &mut self.opening else {
            return false;
        };
        if open.confirm_close {
            match key {
                "escape" => open.confirm_close = false,
                "tab" | "left" | "right" => open.close_focus = !open.close_focus,
                "enter" if open.close_focus => self.accept_open_close(),
                "enter" => open.confirm_close = false,
                _ => {}
            }
        } else if key == "escape" {
            self.cancel_opening();
        } else if key == "tab" {
            open.name_focus = !open.name_focus;
        } else if key == "enter" && !open.name_focus {
            self.cancel_opening();
        }
        true
    }
    pub(crate) fn accept_open_close(&mut self) {
        // Disconnecting the receivers delegates both sessions' destruction to
        // the owner worker. No join or session Drop occurs on the window thread.
        if let Some(open) = self.opening.take() {
            use std::sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            };
            let last = match self.song.record_dir() {
                Some(dir) => crate::keep::LastOpen::Song(dir),
                None if self.song.is_sample() => crate::keep::LastOpen::Sample,
                None => crate::keep::LastOpen::Empty,
            };
            let root = self.keep.root().map(Path::to_path_buf);
            let later = self.os_files.then(crate::notice::later_path);
            let done = if let Some(job) = &open.job {
                job.cancel();
                job.finished.clone()
            } else {
                let session = self.song.take_session_for_save();
                let done = Arc::new(AtomicBool::new(false));
                let ended = done.clone();
                std::thread::spawn(move || {
                    drop(session);
                    ended.store(true, Ordering::Release);
                });
                done
            };
            let closing = open.closing;
            drop(open.job);
            let stop = Arc::new(AtomicBool::new(false));
            let halted = stop.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            // No Studio/Window is captured. A close keeps the previous last-open
            // record and records deferred cleanup before waiting for owner retirement.
            std::thread::spawn(move || {
                if let Some(root) = root {
                    crate::keep::Keep::at(root).closed_with(&last);
                    if let (Some(later), Ok(Some(closed))) = (&later, &closing) {
                        let _ = crate::notice::leave_later(later, &[closed.dir().to_path_buf()]);
                    }
                }
                while !done.load(Ordering::Acquire) && !halted.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(2));
                }
                let result = if done.load(Ordering::Acquire) && !halted.load(Ordering::Acquire) {
                    let _apartment = xirius_session::os::apartment();
                    closing.and_then(|closed| {
                        closed.map_or(Ok(0), |c| {
                            c.run(&halted)
                                .map(|out| out.data["moved"].as_array().map_or(0, Vec::len))
                                .map_err(|e| e.to_string())
                        })
                    })
                } else {
                    Ok(0)
                };
                let _ = tx.send(result);
            });
            self.open_closing = Some(CloseJob { rx, stop });
            self.song.session_mut().fail_closed();
        }
        self.open_generation += 1;
        self.leaving_ready = true;
        self.window_ask = Some(crate::context::key_more::WindowAsk::Quit);
    }
    /// Only called after the real window has gone, sharing its existing shutdown budget.
    pub(crate) fn wait_open_close(&mut self, within: Duration) -> usize {
        let Some(close) = self.open_closing.take() else {
            return 0;
        };
        let n = match close.rx.recv_timeout(within) {
            Ok(Ok(n)) => n,
            _ => 0,
        };
        close.stop.store(true, std::sync::atomic::Ordering::Release);
        n
    }

    pub(crate) fn opening_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(open) = &self.opening else {
            return false;
        };
        if open.confirm_close {
            let (back, close) = crate::opening_view::close_buttons(g);
            if back.contains(x, y) {
                self.opening.as_mut().unwrap().confirm_close = false;
            } else if close.contains(x, y) {
                self.accept_open_close();
            }
        } else if open.visible(crate::wait::now())
            && crate::opening_view::cancel_rect(g).contains(x, y)
        {
            self.cancel_opening();
        }
        true
    }
    pub(crate) fn poll_opening(&mut self) -> bool {
        let Some(open) = &mut self.opening else {
            return false;
        };
        if open.pending.is_some() {
            if open.cancelled {
                let mut open = self.opening.take().unwrap();
                if let Some(job) = open.mcp.take() {
                    job.answer(Err(
                        std::io::Error::from(std::io::ErrorKind::Interrupted).into()
                    ));
                }
                return true;
            }
            if open.pull.as_ref().is_some_and(|p| !p.ready()) {
                // Preserve the existing timeout, but never spend it inside a callback.
                if crate::wait::now().saturating_duration_since(open.since)
                    < xirius_session::host::PULL_WAIT
                {
                    return true;
                }
            }
            if let Some(host) = self.transport.host_mut()
                && let Err(e) = xirius_session::host::collect_plugin_states(
                    self.song.session_mut(),
                    host,
                    &xirius_core::session::RunOptions::human(),
                )
            {
                self.opening = None;
                self.warn(e.to_string(), None);
                return true;
            }
            let session = self.song.take_session_for_save();
            match OpenJob::start(session, open.pending.take().unwrap(), open.id) {
                Ok(job) => open.job = Some(job),
                Err(e) => {
                    self.song.finish_session_save(*e.session, false);
                    self.opening = None;
                    self.warn(e.error.to_string(), None);
                    return true;
                }
            }
        }
        let open = self.opening.as_mut().unwrap();
        open.update(crate::wait::now(), self.ai.motion.reduced);
        let job = open.job.as_ref().unwrap();
        if let Ok(ready) = job.try_ready() {
            let valid = ready.id == open.id
                && ready.id.generation == self.open_generation
                && !open.cancelled;
            job.decide(ready.id, valid);
        }
        let result = match job.try_finish() {
            Err(TryRecvError::Empty) => return true,
            other => other,
        };
        let mut open = self.opening.take().unwrap();
        let result = match result {
            Err(_) => {
                self.song.session_mut().fail_closed();
                Err(CmdError::io("曲の読み込みとの接続が切れました"))
            }
            Ok(outcome) => {
                let valid = outcome.id == open.id
                    && outcome.id.generation == self.open_generation
                    && !open.cancelled;
                if outcome.result.is_ok() && !valid {
                    // A stale successful payload must never be installed or dropped here.
                    std::thread::spawn(move || drop(outcome));
                    self.song.session_mut().fail_closed();
                    Err(CmdError::conflict("古い読み込みの結果を破棄しました"))
                } else {
                    let result = outcome.result;
                    self.song.finish_session_save(outcome.session, false);
                    if result.is_ok() {
                        let sample = open.sample
                            || (open.startup
                                && self.song.dir().is_none()
                                && self.project().name == crate::song::SAMPLE_NAME);
                        let draft = self
                            .song
                            .record_dir()
                            .is_some_and(|dir| self.keep.is_draft_dir(&dir));
                        self.song.opened(sample, draft);
                        self.open_adopted(std::mem::replace(&mut open.closing, Ok(None)));
                        if let Some(skipped) = outcome.skipped {
                            self.dawproject_skipped(skipped);
                        }
                    }
                    result
                }
            }
        };
        if let Err(error) = &result
            && error.io_kind != Some(std::io::ErrorKind::Interrupted)
        {
            self.warn(format!("曲を開けません（{error}）"), None);
        }
        if result.is_ok()
            && let Some(after) = open.after.take()
        {
            after(self);
        }
        if let Some(job) = open.mcp.take() {
            let command = job.command.clone();
            job.answer(result.map(|_| {
                RunResult::new(
                    &command,
                    xirius_core::Output::with(
                        "曲を開きました",
                        serde_json::json!({"name":self.project().name}),
                    ),
                    xirius_core::diff::ChangeSet::default(),
                )
            }));
        }
        true
    }
    fn open_adopted(&mut self, closing: Result<Option<xirius_session::cleanup::Closed>, String>) {
        // The regular logical switch is shared with setup helpers. Recent-list
        // filesystem work is deferred separately, never part of this callback.
        self.song_replaced_with(closing, true);
    }
    pub(crate) fn open_command(&mut self, command: &str, args: serde_json::Value) -> bool {
        if self.input_blocked() {
            self.warn(BUSY.into(), None);
            return false;
        }
        let Some(raw) = args.as_object() else {
            return false;
        };
        match self.song.session_mut().prepare_open(
            command,
            raw,
            &xirius_core::session::RunOptions::human(),
        ) {
            Ok(Ok(request)) => self.request_open(Request::Core(request)),
            Ok(Err(_)) => true,
            Err(e) => {
                self.warn(e.to_string(), None);
                false
            }
        }
    }
    pub(crate) fn open_mcp(&mut self, job: xirius_mcp::Job) {
        match self
            .song
            .session_mut()
            .prepare_open(&job.command, &job.raw, &job.opts)
        {
            Ok(Ok(request)) => {
                if self.request_open(Request::Core(request)) {
                    self.opening.as_mut().unwrap().mcp = Some(job);
                } else {
                    job.answer(Err(CmdError::conflict(BUSY)));
                }
            }
            Ok(Err(reply)) => job.answer(Ok(reply)),
            Err(e) => job.answer(Err(e)),
        }
    }
}
#[cfg(test)]
mod tests;
