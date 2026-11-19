//! UI-337: the host's native preset button opens a popup above its editor.
//! Real parked windows; excluded from CI by the `_window` suffix.
#![cfg(windows)]

use serde_json::Value;
use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};
mod support;

#[test]
fn native_preset_popup_is_above_both_pinned_and_unpinned_editors() {
    for (pinned, topmost) in [(false, false), (true, false), (true, true)] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        support::sample_audio(dir);
        let log = fs::File::create(dir.join("run.log")).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_xirius-ui"));
        command
            .args([
                "--park",
                "--silent",
                "--script",
                "plugin-presets-front",
                "--shot",
            ])
            .arg(dir)
            .current_dir(dir)
            .env("XIRIUS_EDITOR_OFFSCREEN", "1")
            .env("XIRIUS_PRESET_PROBE_MODE", "dedicated")
            .env("XIRIUS_CONFIG_DIR", dir.join("prefs"))
            .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
            .env("LOCALAPPDATA", dir.join("local"))
            .env("APPDATA", dir.join("roaming"))
            .env_remove("XIRIUS_PRESET_PROBE_PLUGIN")
            .env_remove("XIRIUS_PRESET_PROBE_LONG")
            .env_remove("XIRIUS_PRESET_FRONT_PIN")
            .env_remove("XIRIUS_PRESET_FRONT_TOPMOST")
            .stdout(log.try_clone().unwrap())
            .stderr(log);
        if pinned {
            command.env("XIRIUS_PRESET_FRONT_PIN", "1");
        }
        if topmost {
            command.env("XIRIUS_PRESET_FRONT_TOPMOST", "1");
        }
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(120);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!(
                    "popup probe exceeded 120 seconds: {}",
                    fs::read_to_string(dir.join("run.log")).unwrap()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let log = fs::read_to_string(dir.join("run.log")).unwrap();
        assert!(status.success() && !log.contains("panicked"), "{log}");
        let data: Value =
            serde_json::from_slice(&fs::read(dir.join("front.json")).unwrap()).unwrap();
        eprintln!("pinned={pinned}: {data}");
        assert_eq!(data["before"]["topmost"], topmost, "{data}");
        assert_eq!(data["open"]["above_editor"], true, "{data}");
        assert_eq!(data["open"]["point_is_popup"], true, "{data}");
        assert_eq!(
            data["open"]["popup_origin"], data["open"]["button_bottom"],
            "{data}"
        );
        assert_eq!(
            data["after"]["topmost"], data["before"]["topmost"],
            "{data}"
        );
        assert_eq!(data["after"]["owner"], data["before"]["owner"], "{data}");
        assert_eq!(data["after"]["popup_closed"], true, "{data}");
        assert_eq!(data["after"]["selected"], "Clean", "{data}");
        assert_eq!(data["after"]["pinned"], pinned, "{data}");
        assert_eq!(data["after"]["order"], data["before"]["order"], "{data}");
        // Native hosts and file watchers can release their handles just after app exit.
        // Match the bounded cleanup used by the other UI scenario helpers.
        let deadline = Instant::now() + Duration::from_secs(10);
        while let Err(error) = fs::remove_dir_all(dir) {
            if !dir.exists() {
                break;
            }
            assert!(Instant::now() < deadline, "cleanup: {error}");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!dir.exists());
        drop(tmp);
    }
}
