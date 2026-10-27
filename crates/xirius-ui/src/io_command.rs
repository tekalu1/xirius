//! File-backed commands hand the actual Session to the existing completion
//! boundary. The running audio/editor host is retained, and no command result
//! (including MCP) reports successful persistence before the worker returns.
use crate::studio::Studio;

pub(crate) fn handles(name: &str) -> bool {
    name != "macro.run"
        && (xirius_prefs::is_settings_command(name)
            || [
                "export.",
                "audio.",
                "midi.",
                "prefs.",
                "keymap.",
                "mousemap.",
                "midimap.",
                "macro.",
            ]
            .iter()
            .any(|prefix| name.starts_with(prefix)))
}

pub(crate) fn label(name: &str) -> &'static str {
    if name.starts_with("export.") {
        "書き出しを準備しています"
    } else {
        "設定を更新しています"
    }
}
impl Studio {
    pub(crate) fn io_device_change_error(
        &self,
        name: &str,
        raw: &serde_json::Map<String, serde_json::Value>,
    ) -> Option<String> {
        let key = raw.get("key").and_then(|v| v.as_str()).unwrap_or_default();
        ((self.transport.recording || self.transport.take.is_some())
            && matches!(name, "audio.set" | "prefs.set")
            && matches!(
                key,
                "driver" | "device" | "buffer" | "audio.driver" | "audio.device" | "audio.buffer"
            ))
        .then(|| "録音またはテイクの配置が未完了のため、オーディオデバイスを変更できません".into())
    }
    pub(crate) fn io_command_finished(
        &mut self,
        name: &str,
        raw: &serde_json::Map<String, serde_json::Value>,
        result: &xirius_core::session::RunResult,
    ) {
        self.error = None;
        self.after_edit();
        if name.starts_with("export.") {
            if name == "export.preview" {
                self.export.preview = (!raw.get("stop").and_then(|v| v.as_bool()).unwrap_or(false))
                    .then(|| {
                        raw.get("job")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_owned()
                    });
                self.transport.auditioning_export = self.export.preview.is_some();
            }
            self.reload_exports();
            self.refresh_popup();
        } else {
            self.poll_io();
            if let Some(ui) = &mut self.settings {
                let undo = ui.toast.take().and_then(|toast| toast.undo);
                ui.tell(result.message.clone(), undo);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{browser_io_tests::Release, song::Song};
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };

    #[test]
    fn memory_only_macro_edit_keeps_the_normal_command_path() {
        let mut s = Studio::new(Song::empty());
        s.song
            .set(
                "macro.set",
                serde_json::json!({"name":"Fixture", "steps":[
                    {"command":"track.add.audio --name One"},
                    {"command":"track.add.audio --name Two"}
                ]}),
            )
            .unwrap();
        s.bind_window_io();
        let tracks = s.project().tracks.len();
        assert!(s.run_command("macro.run", serde_json::json!({"name":"Fixture"})));
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(s.saving.is_none() && s.preparing.is_none());
        assert_eq!(s.project().tracks.len(), tracks + 2);
    }

    #[test]
    fn settings_write_never_holds_shared_locks_or_marks_a_failed_plugin_saved() {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&root.path().join("song")).unwrap();
        s.song.settings().lock().unwrap().dir = Some(root.path().join("prefs"));
        s.bind_window_io();
        s.keep.plugin_state_failed = true;
        let ui = std::thread::current().id();
        let release = Release::new();
        let gate = release.0.clone();
        let path = root.path().to_owned();
        let (tx, entered) = mpsc::channel();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, file| {
            if op == "settings_write" && file.starts_with(&path) {
                assert_ne!(std::thread::current().id(), ui);
                let _ = tx.send(());
                drop(
                    gate.1
                        .wait_while(gate.0.lock().unwrap(), |done| !*done)
                        .unwrap(),
                );
            }
            Ok(())
        }));
        s.io_command_on_worker(
            "prefs.set",
            serde_json::json!({"key":"ui.theme", "value":"light"})
                .as_object()
                .unwrap()
                .clone(),
        );
        entered.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(s.song.settings().try_lock().is_ok());
        assert!(s.song.io.try_lock().is_ok());
        assert!(!root.path().join("prefs/prefs.toml").exists());
        for _ in 0..8 {
            s.idle();
            s.tick();
            s.geometry(880.0, 600.0);
            s.geometry(1200.0, 800.0);
            assert!(s.saving.is_some());
            assert!(s.keep.plugin_state_failed);
        }
        release.release();
        s.wait_for_save();
        assert!(s.error.is_none(), "{:?}", s.error);
        assert_eq!(
            s.song.settings().lock().unwrap().prefs.text("ui.theme"),
            "light"
        );
        assert!(root.path().join("prefs/prefs.toml").exists());
        assert!(s.keep.plugin_state_failed);
    }

    #[test]
    fn session_drop_returns_before_history_and_export_retirement() {
        let root = tempfile::tempdir().unwrap();
        let mut song = Song::empty();
        song.save_as(&root.path().join("song")).unwrap();
        song.bind_window_resources();
        let ui = std::thread::current().id();
        let release = Release::new();
        let gate = release.0.clone();
        let path = root.path().to_owned();
        let (tx, entered) = mpsc::channel();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, file| {
            if op == "session_retire" && file.starts_with(&path) {
                assert_ne!(std::thread::current().id(), ui);
                let _ = tx.send(());
                drop(
                    gate.1
                        .wait_while(gate.0.lock().unwrap(), |done| !*done)
                        .unwrap(),
                );
            }
            Ok(())
        }));
        drop(song);
        entered.recv_timeout(Duration::from_secs(20)).unwrap();
        let mut next = Studio::new(Song::empty());
        next.geometry(880.0, 600.0);
        next.idle();
        release.release();
    }

    #[test]
    fn late_settings_read_cannot_undo_an_explicit_persisted_command() {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::new(Song::empty());
        s.song.settings().lock().unwrap().dir = Some(root.path().to_owned());
        s.bind_window_io();
        let release = Release::new();
        let gate = release.0.clone();
        let path = root.path().to_owned();
        let ui = std::thread::current().id();
        let (tx, entered) = mpsc::channel();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, file| {
            if op == "settings_read_ready" && file == path {
                assert_ne!(std::thread::current().id(), ui);
                let _ = tx.send(());
                drop(
                    gate.1
                        .wait_while(gate.0.lock().unwrap(), |done| !*done)
                        .unwrap(),
                );
            }
            Ok(())
        }));
        s.song.settings().lock().unwrap().reload_if_changed();
        entered.recv_timeout(Duration::from_secs(20)).unwrap();
        s.io_command_on_worker(
            "prefs.set",
            serde_json::json!({"key":"ui.theme", "value":"light"})
                .as_object()
                .unwrap()
                .clone(),
        );
        s.wait_for_save();
        assert!(s.error.is_none(), "{:?}", s.error);
        release.release();
        for _ in 0..128 {
            s.song.settings().lock().unwrap().reload_if_changed();
            assert_eq!(
                s.song.settings().lock().unwrap().prefs.text("ui.theme"),
                "light"
            );
            std::thread::yield_now();
        }
    }
}
