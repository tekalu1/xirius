//! File-backed commands hand the actual Session to the existing completion
//! boundary. The running audio/editor host is retained, and no command result
//! (including MCP) reports successful persistence before the worker returns.
use crate::studio::Studio;

pub(crate) fn handles(name: &str) -> bool {
    name != "macro.run"
        && (matches!(
            name,
            "plugin.rescan" | "plugin.scan.status" | "browser.plugin.list" | "project.cleanup"
        ) || xirius_session::track_preset::NAMES.contains(&name)
            || xirius_session::preset::NAMES.contains(&name)
            || xirius_session::xpreset::NAMES.contains(&name)
            || xirius_prefs::is_settings_command(name)
            || [
                "export.",
                "audio.",
                "midi.",
                "prefs.",
                "keymap.",
                "mousemap.",
                "midimap.",
                "macro.",
                "file.",
                "version.",
                "history.",
            ]
            .iter()
            .any(|prefix| name.starts_with(prefix)))
}

pub(crate) fn captures(name: &str) -> bool {
    matches!(
        name,
        "plugin.preset.save"
            | "track.preset.save"
            | "track.preset.update"
            | "preset.save"
            | "version.save"
    )
}

pub(crate) fn label(name: &str) -> &'static str {
    if name.starts_with("export.") {
        "書き出しを準備しています"
    } else if name.starts_with("file.")
        || name.starts_with("history.")
        || name.starts_with("version.")
        || name == "project.cleanup"
        || name == "@keep-draft"
    {
        "保存しています"
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
        ((self.transport.recording || self.transport.take.is_some())
            && xirius_prefs::targets_audio_device(name, raw))
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
        if name.starts_with("file.")
            || name.starts_with("history.")
            || name.starts_with("version.")
            || name == "project.cleanup"
        {
            if name.starts_with("version.") {
                self.song.invalidate_file_reads();
            }
            if name.starts_with("history.") {
                let points = self.view_read.history_points.take();
                if !result.changes.is_empty() {
                    self.restore_history_points(points);
                }
            }
            self.after_file_command(name, &result.data);
            self.say_command(name, &serde_json::Value::Object(raw.clone()), &result.data);
            self.follow_selection();
            self.refresh_popup();
            return;
        }
        if name == "plugin.rescan" {
            if result.data["state"] == "running" {
                if let Err(error) = self.track_scan(&result.data) {
                    self.error = Some(error);
                }
            } else {
                self.reload_catalog_later();
            }
        }
        if name.starts_with("export.") {
            if name == "export.delete"
                && self.export.preview.as_deref() == raw.get("job").and_then(|v| v.as_str())
            {
                self.export.preview = None;
                self.transport.auditioning_export = false;
            }
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

    fn recording(root: &std::path::Path) -> (Studio, xirius_core::id::TrackId) {
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&root.join("song")).unwrap();
        s.song.load_settings(&root.join("prefs"));
        s.song
            .set(
                "prefs.set",
                serde_json::json!({"key":"audio.device","value":"Fixture"}),
            )
            .unwrap();
        let track = s
            .song
            .run("track.add.audio", serde_json::json!({"name":"Audio"}))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        s.song.toggle_arm(track).unwrap();
        s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
        s.bind_window_io();
        s.prepare_song();
        let end = std::time::Instant::now() + Duration::from_secs(20);
        while s.preparing.is_some() {
            s.poll_preparing();
            assert!(std::time::Instant::now() < end);
            std::thread::yield_now();
        }
        (s, track)
    }

    #[test]
    fn all_device_reset_scopes_are_rejected_before_memory_or_disk_changes_from_ui_and_mcp() {
        use xirius_mcp::Host;
        let root = tempfile::tempdir().unwrap();
        let (mut s, track) = recording(root.path());
        s.toggle_record();
        assert!(s.transport.recording, "{:?}", s.error);
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while s.transport.host().unwrap().tick_now().unwrap_or(0) < i64::from(s.project().ppq) {
            s.transport.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let before = s.song.settings().lock().unwrap().prefs.values.clone();
        let file = root.path().join("prefs/prefs.toml");
        let bytes = std::fs::read(&file).unwrap();
        for retained in [false, true] {
            if retained {
                s.song
                    .run("track.delete", serde_json::json!({"ids":track.to_string()}))
                    .unwrap();
                s.finish_take();
                let end = std::time::Instant::now() + Duration::from_secs(20);
                while s.transport.finishing_take {
                    s.poll_finished_take();
                    assert!(std::time::Instant::now() < end);
                    std::thread::yield_now();
                }
                assert!(s.transport.take.is_some());
            }
            for raw in [
                serde_json::json!({"key":"audio.device"}),
                serde_json::json!({"key":"audio.driver"}),
                serde_json::json!({"key":"audio.buffer"}),
                serde_json::json!({"key":"audio"}),
                serde_json::json!({"key":"audio."}),
                serde_json::json!({}),
            ] {
                s.error = None;
                s.run_command("prefs.reset", raw.clone());
                assert!(s.saving.is_none() && s.error.is_some());
                let (bridge, mut jobs) =
                    xirius_mcp::bridge(xirius_session::commands::shared_registry());
                let call = std::thread::spawn(move || {
                    bridge.run(
                        "prefs.reset",
                        raw.as_object().unwrap(),
                        &xirius_core::session::RunOptions::human(),
                        false,
                    )
                });
                let end = std::time::Instant::now() + Duration::from_secs(20);
                let job = loop {
                    if let Ok(job) = jobs.try_recv() {
                        break job;
                    }
                    assert!(std::time::Instant::now() < end);
                    std::thread::yield_now();
                };
                s.ai_job(job);
                assert!(call.join().unwrap().is_err());
                assert_eq!(s.song.settings().lock().unwrap().prefs.values, before);
                assert_eq!(std::fs::read(&file).unwrap(), bytes);
            }
        }
    }

    #[test]
    fn automatic_record_stop_and_placement_continue_while_settings_io_is_held() {
        for idle in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (mut s, track) = recording(root.path());
            let end_tick = i64::from(s.project().ppq) * 4;
            s.song
                .run("loop.set", serde_json::json!({"start":0,"end":end_tick}))
                .unwrap();
            s.song
                .settings()
                .lock()
                .unwrap()
                .prefs
                .set("record.punch", &serde_json::json!(true))
                .unwrap();
            s.toggle_record();
            assert!(s.transport.recording, "{:?}", s.error);
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while s.transport.host().unwrap().tick_now().unwrap_or(0) < i64::from(s.project().ppq) {
                s.transport.poll();
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            let before = s.project().tracks[&track].clips.len();
            let release = Release::new();
            let gate = release.0.clone();
            let path = root.path().to_owned();
            let (tx, entered) = mpsc::channel();
            let _observe = xirius_core::io_test::observe(Arc::new(move |op, file| {
                if op == "settings_write" && file.starts_with(&path) {
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
                serde_json::json!({"key":"ui.theme","value":"light"})
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            entered.recv_timeout(Duration::from_secs(20)).unwrap();
            assert!(s.transport.host_mut().unwrap().seek_tick(end_tick + 1));
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while s.transport.recording {
                if idle {
                    s.idle();
                } else {
                    s.tick();
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(!s.transport.playing && s.saving.is_some());
            assert!(s.transport.take.is_some());
            for _ in 0..8 {
                s.tick();
                s.idle();
                assert!(!s.transport.playing);
            }
            release.release();
            s.wait_for_save();
            while s.transport.finishing_take {
                s.poll_finished_take();
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(s.transport.take.is_none(), "{:?}", s.error);
            assert_eq!(s.project().tracks[&track].clips.len(), before + 1);
            s.tick();
            s.idle();
            assert_eq!(s.project().tracks[&track].clips.len(), before + 1);
        }
    }

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
        std::fs::write(root.path().join("prefs.toml"), "[ui]\ntheme = 'dark'\n").unwrap();
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
