//! PrintWindow and own-window messages only; no foreground/global input/device.
#![cfg(all(windows, feature = "loading-probe"))]
use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn live_edit_recording_and_editor_survive_held_work_and_native_window_operations() {
    for operation in [
        "prepare_decode",
        "prepare_convert",
        "prepare_prime",
        "prepare_get_state",
        "input_open",
        "record_writer",
        "record_directory",
        "live_midi_open",
        "settings_write",
        "export_metadata",
        "live_midi_close",
        "input_close",
        "view_read",
        "version_list",
        "cleanup_list",
        "groove_read",
        "notice_read",
        "watch_register",
        "external_project_read",
        "crash_reports",
        "update_startup",
        "log_write",
    ] {
        for size in ["1200,800", "880,600"] {
            let root = tempfile::tempdir().unwrap();
            let log = root.path().join("probe.log");
            let output = fs::File::create(&log).unwrap();
            let mut child = Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
                .args([
                    "--park",
                    "--silent",
                    "--script",
                    "live-edit",
                    "--size",
                    size,
                    "--shot",
                ])
                .arg(root.path())
                .env("XIRIUS_CONFIG_DIR", root.path().join("prefs"))
                .env("XIRIUS_CRASH_DIR", root.path().join("crashes"))
                .env("XIRIUS_LIVE_SHOTS_DIR", root.path())
                .env("XIRIUS_LIVE_GATE", operation)
                .env(xirius_vst3::OFFSCREEN_ENV, "1")
                .stdout(output.try_clone().unwrap())
                .stderr(output)
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(120);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!(
                        "{operation} {size} watchdog: {}",
                        fs::read_to_string(&log).unwrap()
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            let log = fs::read_to_string(log).unwrap();
            assert!(status.success(), "{operation} {size}: {log}");
            if operation == "prepare_decode" {
                assert!(log.contains("live-check stage=handoff ok=true"), "{log}");
            }
            for check in [
                "live-check stage=held ok=true",
                "live-check stage=finished ok=true",
                "prepared-check stage=moved ok=true",
                "prepared-check stage=resized ok=true",
                "prepared-check stage=restored ok=true",
                "6 saved, 0 failed",
            ] {
                assert!(log.contains(check), "{operation} {size}: {log}");
            }
            for name in [
                "live-00-before",
                "live-01-held",
                "live-02-moved",
                "live-03-resized",
                "live-04-restored",
                "live-05-finished",
            ] {
                let bmp = fs::read(root.path().join(format!("{name}.bmp"))).unwrap();
                assert!(bmp.len() > 1024 && &bmp[..2] == b"BM");
            }
        }
    }
}
