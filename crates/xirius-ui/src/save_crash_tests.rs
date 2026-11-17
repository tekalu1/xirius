// Windows-only: Save As keeps a hosted plugin playing without opening a device.
#![cfg(windows)]
//! Save As reaches the file picker's Studio entry, with no window or devices.
use crate::studio::{FileRequest, Studio};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CHILD: &str = "XIRIUS_SAVE_CRASH_CHILD";

#[test]
fn save_as_child() {
    let Ok(kind) = std::env::var(CHILD) else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("XIRIUS_SAVE_CRASH_ROOT").unwrap());
    xirius_engine::crash::install_at(&root.join("crashes"), "save-as test", true).unwrap();
    let mut s = Studio::sample();
    s.keep = crate::keep::Keep::at(root.join("keep"));
    crate::plugin_reload_shots::prepare(&mut s, 0);
    let id = crate::plugin_reload_shots::id(&s);
    if kind == "draft" {
        s.song
            .session_mut()
            .keep_as_draft(root.join("draft"))
            .unwrap();
        s.song.mark_draft(true);
    }
    if matches!(
        kind.as_str(),
        "saved"
            | "recording"
            | "capture"
            | "close"
            | "volatile_close"
            | "unsaved_close"
            | "sabotage_close"
    ) {
        s.song.save_as(&root.join("original")).unwrap();
    }
    let p = s.project();
    let dir = s.song.dir();
    s.transport.start_host(&p, dir).unwrap();
    crate::shot::run_for(&mut s, 10.0, |s| {
        s.transport
            .host()
            .and_then(|host| host.plugins().server_of(id))
            .and_then(|server| server.pid())
            .is_some()
    });
    let pid = s
        .transport
        .host()
        .unwrap()
        .plugins()
        .server_of(id)
        .and_then(|server| server.pid())
        .expect("fixture host");
    if kind == "capture" {
        assert!(s.start_save());
        s.wait_for_save();
        assert_eq!(s.error, None);
        let reopened = xirius_core::session::Session::open_with(
            root.join("original"),
            xirius_session::commands::shared_registry(),
        )
        .unwrap();
        let plugin = reopened
            .project()
            .tracks
            .values()
            .flat_map(|track| &track.inserts)
            .find(|plugin| plugin.id == id)
            .expect("saved hosted plugin");
        assert_eq!(plugin.params.get("1"), Some(&0.75));
        assert!(plugin.state.is_some(), "save pulls state from the host");
        return;
    }
    if matches!(
        kind.as_str(),
        "close" | "volatile_close" | "unsaved_close" | "sabotage_close"
    ) {
        use crate::context::key_more::WindowAsk;
        if kind != "close" {
            assert!(s.start_save());
            s.wait_for_save();
        }
        let previous = s
            .project()
            .tracks
            .values()
            .flat_map(|track| &track.inserts)
            .find(|plugin| plugin.id == id)
            .and_then(|plugin| plugin.state.clone());
        let steps = s.song.steps();
        let generation = s.keep.save_generation();
        if kind == "close" {
            let host = s.transport.host_mut().unwrap();
            let link = host.plugins().link(id).unwrap();
            host.plugins()
                .server_of(id)
                .unwrap()
                .set_param(&link, "Latency", 0.5);
        }
        if kind == "unsaved_close" {
            s.song
                .run(
                    "track.add.audio",
                    serde_json::json!({"name":"Unsaved take"}),
                )
                .unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        s.window_ask = Some(WindowAsk::Quit);
        let mut was_armed = false;
        let mut sabotages = 0;
        loop {
            s.idle();
            let armed = s.keep.recent_close_since().is_some();
            if kind == "sabotage_close" && armed && !was_armed && sabotages < 4 {
                sabotages += 1;
                s.song
                    .run(
                        "track.add.audio",
                        serde_json::json!({"name":format!("sabotage {sabotages}")}),
                    )
                    .unwrap();
            }
            was_armed = armed;
            if s.take_window_ask() == Some(WindowAsk::Quit) && s.close_now() {
                break;
            }
            assert!(Instant::now() < deadline, "hosted close watchdog");
            assert!(s.keep.save_generation() - generation <= 4);
            std::thread::sleep(Duration::from_millis(5));
        }
        if kind != "unsaved_close" && kind != "sabotage_close" {
            assert_eq!(s.song.steps(), steps, "close capture adds no edit step");
        }
        assert_eq!(
            s.keep.save_generation() - generation,
            if kind == "sabotage_close" { 4 } else { 1 }
        );
        let reopened = xirius_core::session::Session::open_with(
            root.join("original"),
            xirius_session::commands::shared_registry(),
        )
        .unwrap();
        let plugin = reopened
            .project()
            .tracks
            .values()
            .flat_map(|track| &track.inserts)
            .find(|plugin| plugin.id == id)
            .expect("closed hosted plugin was saved");
        let state = xirius_plugins::decode_state(plugin.state.as_deref().unwrap()).unwrap();
        if kind == "close" {
            assert_eq!(state.component, 128u32.to_le_bytes());
        }
        if kind == "volatile_close" {
            assert_ne!(plugin.state, previous);
        }
        if kind == "unsaved_close" {
            assert!(
                reopened
                    .project()
                    .tracks
                    .values()
                    .any(|track| track.name == "Unsaved take")
            );
        }
        return;
    }
    xirius_session::host::pull_plugin_states(
        s.song.session_mut(),
        s.transport.host_mut().unwrap(),
        &xirius_core::session::RunOptions::human(),
        xirius_session::host::PULL_WAIT,
    )
    .unwrap();
    let saved = root.join("saved");
    let mut take_files = Vec::new();
    if kind == "recording" {
        let vocal = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Vocal")
            .unwrap()
            .id;
        s.song.toggle_arm(vocal).unwrap();
        s.after_edit();
        s.seek(0);
        s.toggle_record();
        assert!(s.transport.recording, "{:?}", s.error);
        s.finish_file_request_sync(FileRequest::SaveAs, &saved);
        assert!(
            !saved.exists(),
            "an unfinished take must stay in its original folder"
        );
        assert!(s.transport.recording && s.transport.take.is_some());
        assert_eq!(s.song.dir(), Some(root.join("original")));
        assert_eq!(
            s.error.as_deref(),
            Some("録音中は名前を付けて保存できません")
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while s.transport.host_mut().unwrap().poll().frame < 4800 {
            assert!(Instant::now() < deadline, "silent recorder did not advance");
            std::thread::sleep(Duration::from_millis(5));
        }
        s.toggle_record();
        assert!(!s.transport.recording && s.transport.take.is_none());
        assert_eq!(s.error, None);
        for clip in &s.project().tracks[&vocal].clips {
            if !p.tracks[&vocal].clips.iter().any(|old| old.id == clip.id)
                && let xirius_core::model::ClipBody::Audio { file, .. } = &clip.body
            {
                take_files.push(file.clone());
            }
        }
        assert!(
            !take_files.is_empty(),
            "a recorded audio take must be in the song"
        );
    }
    s.finish_file_request_sync(FileRequest::SaveAs, &saved);
    assert_eq!(s.error, None);
    assert!(saved.join("manifest.toml").is_file());
    let reopened = xirius_core::session::Session::open_with(
        &saved,
        xirius_session::commands::shared_registry(),
    )
    .unwrap();
    let plugin = reopened
        .project()
        .tracks
        .values()
        .flat_map(|track| &track.inserts)
        .find(|plugin| plugin.id == id)
        .expect("hosted plugin survives Save As");
    assert_eq!(plugin.params.get("1"), Some(&0.75));
    assert!(
        plugin.state.is_some(),
        "Save As captures the hosted plugin state"
    );
    for file in take_files {
        assert!(
            saved.join(&file).is_file(),
            "the saved song must contain its recorded take"
        );
        assert!(root.join("original").join(&file).is_file());
    }
    assert!(s.transport.is_live(), "Save As preserves the player");
    let host = s.transport.host().unwrap();
    assert_eq!(
        host.plugins().server_of(id).and_then(|server| server.pid()),
        Some(pid)
    );
    s.transport.reset();
}

#[test]
fn save_as_keeps_the_player_and_hosted_plugin_alive() {
    crate::plugin_reload_shots::require_plugin_host();
    for kind in [
        "unsaved",
        "draft",
        "saved",
        "recording",
        "capture",
        "close",
        "volatile_close",
        "unsaved_close",
        "sabotage_close",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "save_crash_tests::save_as_child", "--nocapture"])
            .env(CHILD, kind)
            .env("XIRIUS_SAVE_CRASH_ROOT", root.path())
            .env(
                "XIRIUS_FIXTURE_VOLATILE_STATE",
                if matches!(kind, "volatile_close" | "unsaved_close" | "sabotage_close") {
                    "1"
                } else {
                    ""
                },
            )
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "{kind}: {status}");
                break;
            }
            if Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("Save As child timed out");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
