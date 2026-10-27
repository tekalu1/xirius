//! UI-288: file writes leave the window thread; only their completed session comes back.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use xirius_core::error::{CmdError, CmdResult};
use xirius_core::session::{RunOptions, RunResult, SaveProgress, Session};

use crate::studio::{FileRequest, Geometry, Rect, Studio};
use crate::toast::{Answer, Toast};

#[derive(Clone, Debug)]
pub(crate) enum Leave {
    File(FileRequest, PathBuf),
    New,
    Sample,
    Close,
    Dawproject(PathBuf),
    Version(String),
}

pub(crate) struct Saving {
    pub(crate) device_entry: Option<Geometry>,
    operation: Option<&'static str>,
    mcp: Option<McpSave>,
    command: Option<SaveCommand>,
    work: Work,
    pub since: Instant,
    pub progress: Arc<SaveProgress>,
    pub target: Option<PathBuf>,
    leaving: Option<Leave>,
    draft: Option<PathBuf>,
    close_when_done: bool,
    stop_take: bool,
    record_error: Option<String>,
}

struct McpSave {
    job: xirius_mcp::Job,
    result: Option<RunResult>,
}
type SaveCommand = (
    String,
    serde_json::Map<String, serde_json::Value>,
    RunOptions,
    bool,
);
impl McpSave {
    fn command(&self) -> Option<SaveCommand> {
        self.result.is_none().then(|| {
            (
                self.job.command.clone(),
                self.job.raw.clone(),
                self.job.opts.clone(),
                self.job.save,
            )
        })
    }
}

enum Work {
    States(xirius_session::plugins::StatePull),
    Writing(JoinHandle<(Session, CmdResult<Option<RunResult>>)>),
}

impl Saving {
    pub fn defers_input(&self) -> bool {
        self.target.is_none() && self.leaving.is_none() && !self.veil()
    }
    pub fn veil(&self) -> bool {
        self.since.elapsed() >= crate::wait::OVERLAY_AFTER
    }
    pub fn cancellable(&self) -> bool {
        self.target.is_some() && self.mcp.is_none() && self.command.is_none()
    }
    pub fn label(&self) -> &'static str {
        if let Some(label) = self.operation {
            label
        } else if self.cancellable() {
            "別名で保存しています"
        } else {
            "保存しています"
        }
    }
}

pub fn failure_text(error: &CmdError) -> String {
    use std::io::ErrorKind::*;
    match error.io_kind {
        Some(StorageFull | QuotaExceeded) => "ディスクの空きが足りないため保存できません".into(),
        Some(PermissionDenied | ReadOnlyFilesystem) => {
            "このフォルダーには書き込めないため保存できません".into()
        }
        Some(NotFound) => "保存先のフォルダーが見つからないため保存できません".into(),
        _ => error.os_message.as_ref().map_or_else(
            || "保存できません".into(),
            |os| format!("保存できません（{os}）"),
        ),
    }
}

impl Studio {
    #[cfg(test)]
    pub(crate) fn wait_for_save(&mut self) {
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        while self.saving.is_some() && Instant::now() < deadline {
            self.poll_saving();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(self.saving.is_none(), "save timed out");
    }
    pub(crate) fn save_failed(&mut self, error: &CmdError) {
        let first = self.keep.failed.is_none();
        let text = failure_text(error);
        self.keep.failed = Some(text.clone());
        if first {
            self.toast = Some(Toast {
                alert: true,
                button: Some(("別名で保存…".into(), Answer::SaveAs)),
                also: Some(("もう一度".into(), Answer::SaveAgain)),
                ..Toast::plain(text)
            });
        } else if let Some(toast) = &mut self.toast
            && toast
                .also
                .as_ref()
                .is_some_and(|(_, answer)| *answer == Answer::SaveAgain)
        {
            toast.text = text;
        }
    }

    pub(crate) fn save_succeeded(&mut self) {
        if self.keep.plugin_state_failed {
            return;
        }
        self.keep.failed = None;
        if self.toast.as_ref().is_some_and(|t| {
            t.also
                .as_ref()
                .is_some_and(|(_, a)| *a == Answer::SaveAgain)
        }) {
            self.toast = None;
        }
    }

    pub fn start_save(&mut self) -> bool {
        if self.input_blocked() {
            return false;
        }
        if self.song.dir().is_none() {
            self.file_request = Some(FileRequest::SaveAs);
            return true;
        }
        self.keep.flash_until = xirius_core::store::now_ms() + crate::keep::FLASH_MS;
        self.launch_save(None, None);
        true
    }

    /// The folder dialog's answer. Immediate setup helpers use finish_file_request.
    pub fn start_file_request(&mut self, request: FileRequest, path: &std::path::Path) {
        if self.opening.is_some()
            && matches!(
                request,
                FileRequest::Open | FileRequest::OpenVersion(_) | FileRequest::OpenDawproject
            )
        {
            self.request_open_file(request, path);
            return;
        }
        if self.input_blocked() {
            return;
        }
        if request == FileRequest::SaveAs {
            if self.transport.take.is_some() {
                self.error = Some("録音中は名前を付けて保存できません".into());
                return;
            }
            self.launch_save(Some(path.to_path_buf()), None);
        } else if matches!(
            request,
            FileRequest::Open | FileRequest::OpenVersion(_) | FileRequest::OpenDawproject
        ) {
            self.start_leaving(Leave::File(request, path.to_path_buf()));
        } else {
            self.finish_file_request(request, path);
        }
    }

    pub(crate) fn start_leaving(&mut self, leave: Leave) {
        match &leave {
            Leave::File(request, path) => {
                self.request_open_file(request.clone(), path);
                return;
            }
            Leave::New => {
                self.request_open(crate::opening::Request::New);
                return;
            }
            Leave::Sample => {
                self.request_open(crate::opening::Request::Sample);
                return;
            }
            Leave::Close | Leave::Dawproject(_) | Leave::Version(_) => {}
        }
        if self.input_blocked() {
            return;
        }
        if self.transport.take.is_some() {
            self.warn(
                "録音テイクの配置が終わっていないため、まだ閉じられません".into(),
                None,
            );
            return;
        }
        if self.keep.plugin_state_failed {
            self.ask_unsaved(crate::unsaved::Pending::Close);
            return;
        }
        if self.transport.host().is_some() {
            self.launch_save(None, Some(leave));
            return;
        }
        self.keep_leaving();
        if !self.song.writes_pending() && self.save_test_delay.is_zero() {
            self.finish_leaving(leave);
            return;
        }
        self.launch_save(None, Some(leave));
    }

    fn finish_leaving(&mut self, leave: Leave) {
        self.leaving_ready = true;
        match leave {
            Leave::File(request, path) => self.finish_file_request(request, &path),
            Leave::New => self.new_song(),
            Leave::Sample => self.open_sample(),
            Leave::Close => self.window_ask = Some(crate::context::key_more::WindowAsk::Quit),
            Leave::Dawproject(path) => {
                self.dawproject_export_ready(&path);
            }
            Leave::Version(name) => {
                self.command(|song| song.save_version(&name));
            }
        }
        if !matches!(
            self.window_ask,
            Some(crate::context::key_more::WindowAsk::Quit)
        ) {
            self.leaving_ready = false;
        }
    }

    fn launch_save(&mut self, target: Option<PathBuf>, leaving: Option<Leave>) {
        self.launch_save_with_capture(target, leaving, true, None, None);
    }

    pub(crate) fn save_for_mcp(&mut self, job: xirius_mcp::Job, result: Option<RunResult>) {
        let target = result
            .is_none()
            .then(|| {
                self.song
                    .session()
                    .save_request(&job.command, &job.raw)
                    .flatten()
            })
            .flatten();
        self.launch_save_with_capture(target, None, true, Some(McpSave { job, result }), None);
    }

    pub(crate) fn save_command_on_worker(
        &mut self,
        name: &str,
        raw: serde_json::Map<String, serde_json::Value>,
    ) {
        let target = self.song.session().save_request(name, &raw).flatten();
        self.launch_save_with_capture(
            target,
            None,
            true,
            None,
            Some((name.into(), raw, RunOptions::human(), false)),
        );
    }

    pub(crate) fn io_command_on_worker(
        &mut self,
        name: &str,
        raw: serde_json::Map<String, serde_json::Value>,
    ) {
        if let Some(error) = self.io_device_change_error(name, &raw) {
            self.error = Some(error);
            return;
        }
        if let Some(ui) = &mut self.settings {
            ui.toast = None;
        }
        let popup = self.popup.take();
        self.launch_save_with_capture(
            None,
            None,
            false,
            None,
            Some((name.into(), raw, RunOptions::human(), false)),
        );
        self.saving.as_mut().unwrap().operation = Some(crate::io_command::label(name));
        self.popup = popup;
    }

    pub(crate) fn io_command_for_mcp(&mut self, job: xirius_mcp::Job) {
        if let Some(error) = self.io_device_change_error(&job.command, &job.raw) {
            job.answer(Err(CmdError::conflict(error)));
            return;
        }
        if let Some(ui) = &mut self.settings {
            ui.toast = None;
        }
        let name = job.command.clone();
        let capture = job.save;
        self.launch_save_with_capture(
            None,
            None,
            capture,
            Some(McpSave { job, result: None }),
            None,
        );
        self.saving.as_mut().unwrap().operation = Some(crate::io_command::label(&name));
    }

    pub(crate) fn export_with_plugin_states(&mut self, path: PathBuf) {
        self.launch_save(None, Some(Leave::Dawproject(path)));
    }

    pub(crate) fn version_with_plugin_states(&mut self, name: String) {
        if self.transport.is_windowed() {
            self.save_command_on_worker(
                "version.save",
                serde_json::json!({"name":name})
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            return;
        }
        if self.transport.host().is_some() {
            self.launch_save(None, Some(Leave::Version(name)));
        } else {
            self.command(|song| song.save_version(&name));
            if self.error.is_none()
                && let Some(popup) = &mut self.popup
            {
                popup.naming = None;
            }
        }
    }

    pub(crate) fn close_without_plugin_state(&mut self) {
        self.leaving_ready = true;
        self.keep_leaving();
        self.launch_save_with_capture(None, Some(Leave::Close), false, None, None);
    }

    fn launch_save_with_capture(
        &mut self,
        target: Option<PathBuf>,
        leaving: Option<Leave>,
        capture: bool,
        mcp: Option<McpSave>,
        command: Option<SaveCommand>,
    ) {
        let close_when_done = self.keep.save_started();
        self.leaving_ready = false;
        if close_when_done {
            self.window_ask = None;
        }
        self.popup = None;
        self.palette = None;
        self.error = None;
        self.roll.cancel();
        self.pc_keys_release();
        self.sound_roll_audition();
        let draft = self
            .song
            .is_draft()
            .then(|| self.song.record_dir())
            .flatten();
        let progress = Arc::new(SaveProgress::default());
        progress.simulate_slow_disk(self.save_test_delay);
        let pull = capture
            .then(|| {
                self.transport
                    .host_mut()
                    .map(|host| host.begin_pull_states())
            })
            .flatten();
        let work = if let Some(pull) = pull {
            Work::States(pull)
        } else {
            Work::Writing(
                self.save_worker(
                    target.clone(),
                    leaving.is_some(),
                    progress.clone(),
                    command
                        .clone()
                        .or_else(|| mcp.as_ref().and_then(McpSave::command)),
                ),
            )
        };
        self.saving = Some(Saving {
            device_entry: None,
            operation: None,
            mcp,
            command,
            work,
            since: crate::wait::now(),
            progress,
            target,
            leaving,
            draft,
            close_when_done,
            stop_take: false,
            record_error: None,
        });
    }

    fn save_worker(
        &mut self,
        worker_target: Option<PathBuf>,
        only_wait: bool,
        worker_progress: Arc<SaveProgress>,
        command: Option<SaveCommand>,
    ) -> JoinHandle<(Session, CmdResult<Option<RunResult>>)> {
        let mut session = self.song.take_session_for_save();
        let delay = self.save_test_delay;
        #[cfg(test)]
        let failing = self.save_test_failure;
        #[cfg(not(test))]
        let failing = false;
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if !delay.is_zero() {
                    std::thread::sleep(delay);
                }
                if failing {
                    Err(CmdError::io("保存できません"))
                } else if let Some((command, args, opts, persist)) = command {
                    xirius_mcp::persist::run_and_persist(
                        &mut session,
                        &command,
                        &args,
                        &opts,
                        persist,
                    )
                    .map(Some)
                } else if only_wait {
                    session.flush_writes();
                    Ok(None)
                } else if let Some(target) = worker_target {
                    session
                        .save_as_cancellable(&target, worker_progress)
                        .map(|_| None)
                } else {
                    session.save_collecting().map(|_| None)
                }
            }))
            .unwrap_or_else(|_| Err(CmdError::io("保存できません")));
            (session, result)
        })
    }

    pub(crate) fn poll_saving(&mut self) -> bool {
        let Some(save) = self.saving.as_ref() else {
            return false;
        };
        if let Work::States(pull) = &save.work {
            if save.progress.cancelled() {
                pull.cancel();
                if let Some(mcp) = self.saving.take().unwrap().mcp {
                    mcp.job
                        .answer(Err(CmdError::conflict("保存を中止しました")));
                }
                self.update.pending = None;
                return true;
            }
            pull.reconcile(&self.song.project());
            let error = if !pull.ready() {
                if crate::wait::now().saturating_duration_since(save.since)
                    < xirius_session::host::PULL_WAIT
                {
                    return true;
                }
                pull.quarantine();
                Some("プラグインの状態を取得できません。未保存の変更が残っています".into())
            } else {
                pull.error()
            };
            let collected = if let Some(error) = error {
                Err(CmdError::io(error))
            } else if let Some(host) = self.transport.host_mut() {
                xirius_session::host::collect_pulled_states(
                    self.song.session_mut(),
                    host,
                    pull,
                    &xirius_core::session::RunOptions::human(),
                )
                .map(|_| ())
            } else {
                Err(CmdError::io("プラグインの状態を取得できません"))
            };
            let mut save = self.saving.take().unwrap();
            if let Err(error) = collected {
                if let Work::States(pull) = &save.work {
                    pull.cancel();
                }
                self.keep.plugin_state_failed = true;
                if let Some(mcp) = save.mcp.take() {
                    mcp.job.answer(Err(error.clone()));
                }
                self.save_failed(&error);
                self.error = Some(error.to_string());
                self.update.pending = None;
                if matches!(save.leaving, Some(Leave::Close)) || save.close_when_done {
                    self.ask_unsaved(crate::unsaved::Pending::Close);
                }
                return true;
            }
            self.keep.plugin_state_failed = false;
            save.work = Work::Writing(
                self.save_worker(
                    save.target.clone(),
                    save.leaving.is_some(),
                    save.progress.clone(),
                    save.command
                        .clone()
                        .or_else(|| save.mcp.as_ref().and_then(McpSave::command)),
                ),
            );
            self.saving = Some(save);
            return true;
        }
        let Work::Writing(worker) = &save.work else {
            unreachable!()
        };
        if !worker.is_finished() {
            return false;
        }
        let mut save = self.saving.take().unwrap();
        let Work::Writing(worker) = save.work else {
            unreachable!()
        };
        let (session, result) = worker.join().expect("save worker catches panics");
        let operation_command = save.operation.and_then(|_| {
            save.command
                .clone()
                .or_else(|| save.mcp.as_ref().and_then(McpSave::command))
        });
        let committed = result.as_ref().is_ok_and(|r| {
            r.as_ref()
                .is_none_or(|r| !r.dry_run && r.decision.is_none())
        });
        let saved_as = save.target.is_some() && committed;
        self.song.finish_session_save(session, saved_as);
        if save.stop_take && self.transport.take.is_some() {
            self.finish_take();
        }
        let failures = self.song.session().take_write_failures();
        for failure in &failures {
            self.save_failed(failure);
        }
        if let Some(mcp) = save.mcp.take() {
            let reply = match (&result, failures.first()) {
                (Err(error), _) | (_, Some(error)) => Err(error.clone()),
                (Ok(result), None) => Ok(result
                    .clone()
                    .or(mcp.result)
                    .expect("MCP save returns its command result")),
            };
            mcp.job.answer(reply);
        }
        if let Some((name, raw, _, _)) = operation_command {
            match result {
                Ok(Some(result)) => {
                    self.io_command_finished(&name, &raw, &result);
                    if let Some(g) = save.device_entry {
                        self.start_audio(&g);
                    }
                }
                Err(error) => {
                    self.error = Some(error.to_string());
                    if let Some(ui) = &mut self.settings {
                        ui.tell(error.to_string(), None);
                    }
                }
                Ok(None) => {}
            }
            if save.close_when_done {
                self.request_close();
            }
            if let Some(error) = save.record_error {
                self.error = Some(error);
            }
            return true;
        }
        let result = result.map(|_| ());
        match result {
            Err(error) if error.io_kind == Some(std::io::ErrorKind::Interrupted) => {
                // The save was cancelled: the close is not going on, so the update is not
                // waiting for it any more (UI-282).
                self.update.pending = None;
            }
            Err(error) => {
                self.save_failed(&error);
                // UI-282: the close was stopped, so nothing is put in later.
                self.update.pending = None;
            }
            Ok(()) => {
                if let Some(leave) = save.leaving {
                    if !failures.is_empty() {
                        self.update.pending = None;
                        return true;
                    }
                    self.finish_leaving(leave);
                } else {
                    if failures.is_empty() {
                        self.save_succeeded();
                    }
                    if saved_as && let Some(target) = save.target {
                        self.transport.relocate(self.song.dir());
                        if let Some(draft) = save.draft {
                            self.keep.delete_draft(&draft);
                        }
                        self.keep.remember(
                            &target,
                            &self.project().name,
                            false,
                            xirius_core::store::now_ms(),
                        );
                        self.keep.flash_until = 0;
                    }
                    self.after_edit();
                    if saved_as {
                        self.keep.flash_until = 0;
                    }
                    if save.close_when_done && failures.is_empty() {
                        self.start_leaving(Leave::Close);
                    }
                }
            }
        }
        if let Some(error) = save.record_error {
            self.error = Some(error);
        }
        true
    }

    /// The actual Session may be on a file worker; recording control still owns
    /// its running host. Only placing the closed take needs that Session back.
    pub(crate) fn poll_saving_transport(&mut self) {
        if let Some(at) = self.transport.poll() {
            self.playhead = at;
        }
        if self.transport.recording {
            let input_lost = self.transport.input_lost();
            let output_lost = self.transport.is_lost();
            let due = self.transport.host().is_some_and(|host| host.record_due());
            if input_lost.is_some() || output_lost || due {
                self.stop_record_during_save();
                if let Some(save) = &mut self.saving {
                    save.record_error = input_lost.or_else(|| {
                        output_lost.then(|| "オーディオ出力が停止したため録音を止めました".into())
                    });
                }
            }
        }
    }

    fn stop_record_during_save(&mut self) {
        if let Some(host) = self.transport.host_mut() {
            host.record_stop();
        }
        self.transport.pause();
        self.transport.finishing_take = true;
        if let Some(save) = &mut self.saving {
            save.stop_take = true;
        }
    }

    pub(crate) fn saving_key(&mut self, key: &str) -> bool {
        let Some(save) = &self.saving else {
            return false;
        };
        if key == "escape" && save.cancellable() {
            save.progress.cancel();
        }
        if key == "space" {
            if save.operation.is_some() && self.transport.take.is_none() {
                return true;
            }
            if self.transport.take.is_some() {
                self.stop_record_during_save();
            } else {
                self.toggle_play();
            }
        }
        true
    }

    pub(crate) fn saving_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(save) = &self.saving else {
            return false;
        };
        if save.veil() && save.cancellable() && crate::wait::save_cancel_rect(g).contains(x, y) {
            save.progress.cancel();
        }
        true
    }

    pub(crate) fn request_close(&mut self) -> bool {
        if self.unsaved.is_some() {
            return false;
        }
        // A placeholder Session can retain the old Arc and path during Save As.
        // Always wait for the actual job before consulting any cached close state.
        if let Some(save) = &mut self.saving {
            save.close_when_done = true;
            return false;
        }
        if self.prefs_closing {
            return false;
        }
        if let Some(file) = self.browser.prefs_file.as_deref() {
            match crate::browser_prefs_save::status(file) {
                None => {
                    self.prefs_closing = true;
                    return false;
                }
                Some(Err(e)) => {
                    self.error = Some(e);
                    return false;
                }
                Some(Ok(())) => {}
            }
        }
        if self.open_closing.is_some() {
            return self.request_recent_close();
        }
        if let Some(open) = &mut self.opening {
            open.confirm_close = true;
            return false;
        }
        if self.leaving_ready {
            return self.request_recent_close();
        }
        if self.saving.is_none() {
            self.start_leaving(Leave::Close);
        } else if let Some(save) = &mut self.saving {
            save.close_when_done = true;
        }
        self.leaving_ready && self.request_recent_close()
    }
}

pub(crate) fn card(g: &Geometry, progress: bool) -> Rect {
    let height = if progress { 108.0 } else { 84.0 };
    Rect::new(
        (g.size.0 - 280.0) / 2.0,
        (g.size.1 - height) / 2.0,
        280.0,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keep::SaveMark;
    use crate::song::Song;
    use std::time::Duration;

    #[test]
    fn connected_mcp_save_as_waits_for_worker_io_and_returns_the_original_command_result() {
        use std::sync::mpsc;
        use xirius_mcp::Host;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("copy");
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&root.path().join("original")).unwrap();
        s.transport.bind_window();
        let release = crate::browser_io_tests::Release::new();
        let gate = release.0.clone();
        let watch = target.clone();
        let once = std::sync::atomic::AtomicBool::new(false);
        let (tx, entered) = mpsc::channel();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if op == "write"
                && path.starts_with(&watch)
                && !once.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                tx.send(std::thread::current().id()).unwrap();
                let held = gate.0.lock().unwrap();
                drop(gate.1.wait_while(held, |done| !*done).unwrap());
            }
            Ok(())
        }));
        let (bridge, mut jobs) = xirius_mcp::bridge(xirius_session::commands::shared_registry());
        let path = target.clone();
        let call = std::thread::spawn(move || {
            bridge.run(
                "project.save-as",
                serde_json::json!({"path":path}).as_object().unwrap(),
                &RunOptions::human(),
                true,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        let job = loop {
            if let Ok(job) = jobs.try_recv() {
                break job;
            }
            assert!(Instant::now() < deadline, "MCP job watchdog");
            std::thread::yield_now();
        };
        s.ai_job(job);
        assert_ne!(
            entered.recv_timeout(Duration::from_secs(20)).unwrap(),
            std::thread::current().id()
        );
        assert!(!call.is_finished());
        assert!(s.input_blocked());
        assert!(!s.poll_saving(), "a held disk operation is only polled");
        release.release();
        s.wait_for_save();
        let result = call.join().unwrap().unwrap();
        assert_eq!(result.command, "project.save-as");
        assert!(!result.dry_run && result.decision.is_none());
        assert_eq!(s.song.dir().as_deref(), Some(target.as_path()));
        assert!(target.join("project.json").exists());
        assert!(s.transport.host().is_none());
    }

    fn finish(s: &mut Studio) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while s.saving.is_some() && Instant::now() < deadline {
            s.poll_saving();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(s.saving.is_none());
    }

    #[test]
    fn ui_288_failure_reasons_keep_only_the_os_reason_and_no_path_context() {
        use std::io::ErrorKind::*;
        for (kinds, text) in [
            (
                vec![StorageFull, QuotaExceeded],
                "ディスクの空きが足りないため保存できません",
            ),
            (
                vec![PermissionDenied, ReadOnlyFilesystem],
                "このフォルダーには書き込めないため保存できません",
            ),
            (
                vec![NotFound],
                "保存先のフォルダーが見つからないため保存できません",
            ),
        ] {
            for kind in kinds {
                let mut error = CmdError::from(std::io::Error::from(kind));
                error.message = "C:/private/song/project.json".into();
                assert_eq!(failure_text(&error), text);
            }
        }
        let mut error = CmdError::from(std::io::Error::new(Other, "OS reason"));
        error.message = "C:/private/song: OS reason".into();
        assert_eq!(failure_text(&error), "保存できません（OS reason）");
    }

    #[test]
    fn ui_288_ctrl_s_flashes_at_least_450_ms_and_short_saves_have_no_toast_or_veil() {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&root.path().join("song")).unwrap();
        let g = s.geometry(1440.0, 820.0);
        let before = xirius_core::store::now_ms();
        s.key(&g, "s", true, false);
        assert_eq!(s.save_mark(), SaveMark::Saving);
        assert!(!s.saving.as_ref().unwrap().veil());
        let until = s.keep.flash_until;
        assert!(until >= before + 450);
        finish(&mut s);
        assert_eq!(s.keep.mark(&s.song, until - 1), SaveMark::Saving);
        assert_eq!(s.keep.mark(&s.song, until), SaveMark::Saved);
        assert!(s.toast.is_none());
    }

    #[test]
    fn ui_288_failure_is_shown_once_dismissal_keeps_mark_and_retry_recovers() {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&root.path().join("song")).unwrap();
        let error = CmdError::from(std::io::Error::from(std::io::ErrorKind::StorageFull));
        s.save_failed(&error);
        let since = s.toast.as_ref().unwrap().since;
        s.save_failed(&error);
        assert_eq!(s.toast.as_ref().unwrap().since, since);
        let g = s.geometry(1440.0, 820.0);
        s.press(
            &g,
            crate::studio::Hit::Toast(crate::toast::ToastPart::Close),
        );
        s.save_failed(&error);
        assert!(s.toast.is_none());
        assert_eq!(s.save_mark(), SaveMark::Failed);
        s.save_succeeded();
        s.save_failed(&error);
        s.toast_button(Some(&g), true);
        finish(&mut s);
        assert!(s.keep.failed.is_none());
        assert!(s.toast.is_none());
    }

    #[test]
    fn ui_288_save_as_can_be_cancelled_and_blocks_edits_but_accepts_space() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("copy");
        let mut s = Studio::new(Song::empty());
        s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
        s.save_test_delay = Duration::from_millis(250);
        let original = s.project();
        s.start_file_request(FileRequest::SaveAs, &target);
        assert!(!s.run_command("track.add.audio", serde_json::json!({"name":"blocked"})));
        assert!(s.saving_key("space"));
        assert!(s.transport.playing);
        s.saving_key("space");
        assert!(!s.transport.playing);
        assert_eq!(*s.project(), *original);
        s.saving_key("escape");
        finish(&mut s);
        assert!(!target.exists());
        assert_eq!(*s.project(), *original);
        assert!(s.song.dir().is_none());
    }

    #[test]
    fn ui_288_a_close_keeps_the_window_until_the_history_writer_has_finished() {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&root.path().join("song")).unwrap();
        s.song.simulate_slow_disk(Duration::from_millis(350));
        s.run_command("track.add.audio", serde_json::json!({"name":"pending"}));
        let start = Instant::now();
        assert!(!s.request_close());
        assert!(start.elapsed() < Duration::from_millis(200));
        std::thread::sleep(Duration::from_millis(220));
        assert!(s.saving.as_ref().unwrap().veil());
        assert_eq!(s.saving.as_ref().unwrap().label(), "保存しています");
        finish(&mut s);
        assert!(s.leaving_ready);
        assert!(s.request_close());
    }

    #[test]
    fn ui_288_stopping_a_recording_during_save_keeps_the_take_in_the_restored_session() {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::sample();
        s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
        s.song.save_as(&root.path().join("song")).unwrap();
        let track = s
            .project()
            .tracks
            .values()
            .find(|track| track.kind == xirius_core::model::TrackKind::Audio)
            .unwrap()
            .id;
        s.song.toggle_arm(track).unwrap();
        let before = s.project().tracks[&track].clips.len();
        s.toggle_record();
        assert!(s.transport.take.is_some(), "{:?}", s.error);
        std::thread::sleep(Duration::from_millis(60));
        s.save_test_delay = Duration::from_millis(250);
        s.start_save();
        s.saving_key("space");
        assert!(!s.transport.recording);
        assert!(
            s.transport.take.is_some(),
            "the take waits for its real session"
        );
        s.wait_for_save();
        assert!(s.transport.take.is_none());
        assert_eq!(
            s.project().tracks[&track].clips.len(),
            before + 1,
            "{:?}",
            s.error
        );
        s.transport.reset();
    }

    #[test]
    fn ui_288_a_failure_polled_before_the_rest_of_an_autosave_batch_is_not_cleared() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("song");
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&dir).unwrap();
        s.song.simulate_slow_disk(Duration::from_millis(100));
        std::fs::create_dir(dir.join("history/1.json")).unwrap();
        s.run_command("track.add.audio", serde_json::json!({"name":"first"}));
        s.run_command("track.add.audio", serde_json::json!({"name":"second"}));
        let now = xirius_core::store::now_ms() + 31_000;
        s.keep_beat(now);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen_early = false;
        while s.song.writes_pending() && Instant::now() < deadline {
            s.keep_beat(now + 1);
            seen_early |= s.keep.failed.is_some() && s.song.writes_pending();
            std::thread::sleep(Duration::from_millis(5));
        }
        s.keep_beat(now + 2);
        assert!(seen_early);
        assert_eq!(s.save_mark(), SaveMark::Failed);
    }

    /// UI-282: when the close is stopped because the song could not be saved, the update that
    /// was waiting for the end of the window is dropped; it must not install hours later.
    #[test]
    fn ui_282_a_stopped_close_drops_the_waiting_install() {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&root.path().join("song")).unwrap();
        s.save_test_delay = Duration::from_millis(20);
        s.save_test_failure = true;
        s.update.pending = Some(xirius_session::update::Install {
            program: root.path().join("setup.exe"),
            args: vec!["/S".into(), "/XRUN".into()],
        });
        assert!(!s.request_close());
        finish(&mut s);
        assert!(!s.leaving_ready, "the window stays");
        assert!(s.update.pending.is_none(), "nothing is installed later");
    }

    /// UI-282: cancelling a 「別名で保存」 (the only save that can be cancelled) also drops the
    /// waiting install; the window stays, and nothing is put in later.
    #[test]
    fn ui_282_cancelling_save_as_drops_the_waiting_install() {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::new(Song::empty());
        s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
        s.save_test_delay = Duration::from_millis(25);
        s.update.pending = Some(xirius_session::update::Install {
            program: root.path().join("setup.exe"),
            args: vec!["/S".into(), "/XRUN".into()],
        });
        s.start_file_request(FileRequest::SaveAs, &root.path().join("copy"));
        s.saving_key("escape");
        finish(&mut s);
        assert!(!s.leaving_ready);
        assert!(s.update.pending.is_none(), "nothing is installed later");
    }
}
