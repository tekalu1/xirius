//! UI-288: real parked window, silent output, input dispatched only to this window.

#[test]
fn saving_marks_copy_cancel_failure_retry_and_close_drain_through_the_window() {
    let temporary = tempfile::tempdir().unwrap();
    let pictures = std::env::var_os("XIRIUS_SAVING_SHOTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| temporary.path().join("shots"));
    std::fs::create_dir_all(&pictures).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "saving", "--shot"])
        .arg(&pictures)
        .env("XIRIUS_CONFIG_DIR", temporary.path().join("prefs"))
        .env("XIRIUS_CRASH_DIR", temporary.path().join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for stage in 0..=16 {
        let line = stderr
            .lines()
            .find(|line| line.starts_with(&format!("saving-check stage={stage} ")))
            .unwrap_or_else(|| panic!("stage {stage} missing: {stderr}"));
        println!("{line}");
        assert!(line.contains("ok=true"), "{line}");
    }
    assert_eq!(
        std::fs::read_dir(&pictures)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "bmp"))
            .count(),
        17
    );
    for entry in std::fs::read_dir(&pictures).unwrap().filter_map(Result::ok) {
        if entry.path().extension().is_some_and(|ext| ext == "bmp") {
            assert!(entry.metadata().unwrap().len() > 54);
        }
    }
}
