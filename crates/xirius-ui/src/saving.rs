//! UI-288: file writes leave the window thread; only their completed session comes back.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use xirius_core::error::{CmdError, CmdResult};
use xirius_core::session::{SaveProgress, Session};

use crate::studio::{FileRequest, Geometry, Rect, Studio};
use crate::toast::{Answer, Toast};

#[derive(Clone, Debug)]
pub(crate) enum Leave {
    File(FileRequest, PathBuf),
    New,
    Sample,
    Close,
}

pub(crate) struct Saving {
    worker: JoinHandle<(Session, CmdResult<()>)>,
    pub since: Instant,
    pub progress: Arc<SaveProgress>,
    pub target: Option<PathBuf>,
    leaving: Option<Leave>,
    draft: Option<PathBuf>,
    close_when_done: bool,
    stop_take: bool,
}

impl Saving {
    pub fn defers_input(&self) -> bool {
        self.target.is_none() && self.leaving.is_none() && !self.veil()
    }
    pub fn veil(&self) -> bool {
        self.since.elapsed() >= crate::wait::OVERLAY_AFTER
    }
    pub fn cancellable(&self) -> bool {
        self.target.is_some()
    }
    pub fn label(&self) -> &'static str {
        if self.cancellable() {
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
        if self.saving.is_some() {
            return false;
        }
        if self.song.dir().is_none() {
            self.file_request = Some(FileRequest::SaveAs);
            return true;
        }
        self.keep.flash_until = xirius_core::store::now_ms() + crate::keep::FLASH_MS;
        self.pull_plugins();
        self.launch_save(None, None);
        true
    }

    /// The folder dialog's answer. Immediate setup helpers use finish_file_request.
    pub fn start_file_request(&mut self, request: FileRequest, path: &std::path::Path) {
        if self.saving.is_some() {
            return;
        }
        if request == FileRequest::SaveAs {
            if self.transport.take.is_some() {
                self.error = Some("録音中は名前を付けて保存できません".into());
                return;
            }
            self.pull_plugins();
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
        if self.saving.is_some() {
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
        }
        if !matches!(
            self.window_ask,
            Some(crate::context::key_more::WindowAsk::Quit)
        ) {
            self.leaving_ready = false;
        }
    }

    fn launch_save(&mut self, target: Option<PathBuf>, leaving: Option<Leave>) {
        self.popup = None;
        self.palette = None;
        self.error = None;
        self.roll.cancel();
        self.pc_keys_release();
        self.sound_roll_audition();
        let mut session = self.song.take_session_for_save();
        let draft = self
            .song
            .is_draft()
            .then(|| self.song.record_dir())
            .flatten();
        let progress = Arc::new(SaveProgress::default());
        progress.simulate_slow_disk(self.save_test_delay);
        let worker_progress = progress.clone();
        let worker_target = target.clone();
        let only_wait = leaving.is_some();
        let delay = self.save_test_delay;
        #[cfg(test)]
        let failing = self.save_test_failure;
        #[cfg(not(test))]
        let failing = false;
        let worker = std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if !delay.is_zero() {
                    std::thread::sleep(delay);
                }
                if failing {
                    Err(CmdError::io("保存できません"))
                } else if only_wait {
                    session.flush_writes();
                    Ok(())
                } else if let Some(target) = worker_target {
                    session.save_as_cancellable(&target, worker_progress)
                } else {
                    session.save_collecting().map(|_| ())
                }
            }))
            .unwrap_or_else(|_| Err(CmdError::io("保存できません")));
            (session, result)
        });
        self.saving = Some(Saving {
            worker,
            since: Instant::now(),
            progress,
            target,
            leaving,
            draft,
            close_when_done: false,
            stop_take: false,
        });
    }

    pub(crate) fn poll_saving(&mut self) -> bool {
        if !self.saving.as_ref().is_some_and(|s| s.worker.is_finished()) {
            return false;
        }
        let save = self.saving.take().unwrap();
        let (session, result) = save.worker.join().expect("save worker catches panics");
        let saved_as = save.target.is_some() && result.is_ok();
        self.song.finish_session_save(session, saved_as);
        if save.stop_take && self.transport.take.is_some() {
            self.finish_take();
        }
        let failures = self.song.session().take_write_failures();
        for failure in &failures {
            self.save_failed(failure);
        }
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
                    if let Some(target) = save.target {
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
        true
    }

    pub(crate) fn saving_key(&mut self, key: &str) -> bool {
        let Some(save) = &self.saving else {
            return false;
        };
        if key == "escape" && save.cancellable() {
            save.progress.cancel();
        }
        if key == "space" {
            if self.transport.take.is_some() {
                if let Some(host) = self.transport.host_mut() {
                    host.record_stop();
                }
                self.transport.pause();
                self.saving.as_mut().unwrap().stop_take = true;
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
        if self.leaving_ready {
            return true;
        }
        if self.saving.is_none() {
            self.start_leaving(Leave::Close);
        } else if let Some(save) = &mut self.saving {
            save.close_when_done = true;
        }
        self.leaving_ready
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
