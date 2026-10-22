#[test]
fn playback_and_clicks_keep_the_parked_window_silent_and_without_os_focus() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-quiet-shots-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("quiet-shots");
    std::fs::create_dir_all(&dir).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "quiet", "--shot"])
        .arg(&dir)
        .env("XIRIUS_CONFIG_DIR", dir.join("prefs"))
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for stage in 0..3 {
        assert!(
            stderr.contains(&format!("quiet-check stage={stage} ok=true")),
            "{stderr}"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn screen_scripts_cannot_inject_os_keyboard_or_mouse_input() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    fn scan(dir: &std::path::Path) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                for forbidden in [
                    "SendInput",
                    "keybd_event",
                    "mouse_event",
                    "SendKeys",
                    "SetForegroundWindow",
                    "SetFocus(",
                ] {
                    assert!(
                        !source.contains(forbidden),
                        "{} uses {forbidden}",
                        path.display()
                    );
                }
            }
        }
    }
    scan(&root);
}
