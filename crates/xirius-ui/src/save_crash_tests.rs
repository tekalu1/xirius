//! Save As reaches the file picker's Studio entry, with no window or devices.
use crate::studio::{FileRequest, Studio};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW};

const CHILD: &str = "XIRIUS_SAVE_CRASH_CHILD";

#[test]
fn save_as_child() {
    let Ok(kind) = std::env::var(CHILD) else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("XIRIUS_SAVE_CRASH_ROOT").unwrap());
    xirius_engine::crash::install_at(&root.join("crashes"), "save-as test", true).unwrap();
    let mut s = Studio::sample();
    crate::plugin_reload_shots::prepare(&mut s, 0);
    let id = crate::plugin_reload_shots::id(&s);
    assert!(s.run_command(
        "plugin.isolation.set",
        serde_json::json!({
            "ids":[id.to_string()], "mode":"inprocess"
        })
    ));
    if kind == "draft" {
        s.song
            .session_mut()
            .keep_as_draft(root.join("draft"))
            .unwrap();
        s.song.mark_draft(true);
    }
    if kind == "saved" || kind == "recording" {
        s.song.save_as(&root.join("original")).unwrap();
    }
    let p = s.project();
    let dir = s.song.dir();
    let host = s.transport.start_host(&p, dir).unwrap();
    let (plugin, generation) = host.plugins().local(id).expect("in-process fixture");
    let instance = plugin as *const _ as usize;
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
        s.finish_file_request(FileRequest::SaveAs, &saved);
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
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = dll.as_os_str().encode_wide().chain([0]).collect();
    let mut counter = AtomicU32::new(0);
    // SAFETY: the fixture export takes a counter kept on this stack through dispatch.
    unsafe {
        let handle = GetModuleHandleW(wide.as_ptr());
        assert!(!handle.is_null());
        let arm: unsafe extern "system" fn(*mut AtomicU32) -> usize = std::mem::transmute(
            GetProcAddress(handle, c"FixtureLingeringTimer".as_ptr().cast()).unwrap(),
        );
        assert_ne!(arm(&mut counter), 0);
    }
    s.finish_file_request(FileRequest::SaveAs, &saved);
    assert_eq!(s.error, None);
    assert!(saved.join("manifest.toml").is_file());
    for file in take_files {
        assert!(
            saved.join(&file).is_file(),
            "the saved song must contain its recorded take"
        );
        assert!(root.join("original").join(&file).is_file());
    }
    assert!(s.transport.is_live(), "Save As preserves the player");
    let deadline = Instant::now() + Duration::from_secs(10);
    while counter.load(Ordering::Acquire) == 0 {
        assert!(Instant::now() < deadline, "fixture timer never fired");
        // SAFETY: dispatch only this headless child's messages; never send OS input.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let host = s.transport.host().unwrap();
    let (plugin, now) = host.plugins().local(id).unwrap();
    assert_eq!((plugin as *const _ as usize, now), (instance, generation));
    assert_eq!(counter.load(Ordering::Acquire), 1);
    s.transport.reset();
    // SAFETY: a NUL-terminated module path, only inspecting the handle.
    assert!(!unsafe { GetModuleHandleW(wide.as_ptr()) }.is_null());
}

#[test]
fn save_as_keeps_the_player_and_a_lingering_dll_timer_alive() {
    crate::plugin_reload_shots::require_plugin_host();
    for kind in ["unsaved", "draft", "saved", "recording"] {
        let root = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "save_crash_tests::save_as_child", "--nocapture"])
            .env(CHILD, kind)
            .env("XIRIUS_SAVE_CRASH_ROOT", root.path())
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
