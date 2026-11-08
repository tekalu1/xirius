//! Approved context menus on the actual parked window. Captures use PrintWindow;
//! input is dispatched only to that window, never to the OS input queue.
#[cfg(windows)]
#[test]
fn six_targets_cascade_and_search_in_both_themes() {
    for light in [false, true] {
        let sandbox = tempfile::Builder::new()
            .prefix("xirius-context-window-")
            .tempdir()
            .unwrap();
        let root = sandbox.path();
        let shots = root.join("shots");
        std::fs::create_dir_all(&shots).unwrap();
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"));
        command
            .args([
                "--park",
                "--silent",
                "--size",
                "1440,1000",
                "--script",
                "ctx-v2",
                "--shot",
            ])
            .arg(&shots);
        if light {
            command.arg("--light");
        }
        for name in [
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "TEMP",
            "TMP",
            "CommonProgramFiles",
            "CommonProgramW6432",
            "CommonProgramFiles(x86)",
            "XIRIUS_CONFIG_DIR",
            "XIRIUS_CRASH_DIR",
        ] {
            let directory = root.join(name);
            std::fs::create_dir_all(&directory).unwrap();
            command.env(name, directory);
        }
        let output = command.output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "light={light}: {stderr}");
        for target in [
            "audio", "midi", "track", "insert", "note", "empty", "cascade", "search",
        ] {
            let image = shots.join(format!("v2-{target}.bmp"));
            assert!(
                image.metadata().is_ok_and(|m| m.len() > 54),
                "light={light}, {target}: {stderr}"
            );
        }
    }
}
