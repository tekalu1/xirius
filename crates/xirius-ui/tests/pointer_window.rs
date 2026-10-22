//! UI-184: on a real (parked, silent) window, what the window hands to the system as the
//! pointer is the tool's own picture over a part, the system's "not allowed" where the tool has
//! nothing to act on, and the system's crosshair when the setting says so.

#[cfg(windows)]
#[test]
fn the_window_hands_over_the_tools_pointer() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-pointer-shots-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("pointer-shots");
    std::fs::create_dir_all(&dir).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "pointer", "--shot"])
        .arg(&dir)
        .env("XIRIUS_CONFIG_DIR", dir.join("prefs"))
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let checks: Vec<_> = stderr
        .lines()
        .filter(|l| l.starts_with("pointer-check "))
        .collect();
    assert_eq!(checks.len(), 15, "{stderr}");
    for line in checks {
        println!("{line}");
        assert!(line.ends_with("ok=true"), "{line}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
