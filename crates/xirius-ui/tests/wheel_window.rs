//! EDT-136: on a real (parked, silent) window, a wheel turned with Ctrl and Shift held, in the
//! shape Windows sends it (sideways, nothing upright), widens the roll and the arrangement.

#[cfg(windows)]
#[test]
fn ctrl_shift_and_the_wheel_widen_time_as_windows_sends_it() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-wheel-shots-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("wheel-shots");
    std::fs::create_dir_all(&dir).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "wheel", "--shot"])
        .arg(&dir)
        .env("XIRIUS_CONFIG_DIR", dir.join("prefs"))
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let checks: Vec<_> = stderr
        .lines()
        .filter(|l| l.starts_with("wheel-check "))
        .collect();
    assert_eq!(checks.len(), 2, "{stderr}");
    for line in checks {
        println!("{line}");
        assert!(line.ends_with("ok=true"), "{line}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
