//! UI-061: on a real (parked, silent) window, a job that stops the person shows its veil only
//! after 0.2 s and Esc cancels it; work that goes on shows a thin line; a change waiting to
//! land says 「反映中…」 after 0.3 s.

#[test]
fn the_veil_comes_after_200_ms_and_esc_cancels_it() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-wait-shots-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("wait-shots");
    std::fs::create_dir_all(&dir).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "wait", "--shot"])
        .arg(&dir)
        .env("XIRIUS_CONFIG_DIR", dir.join("prefs"))
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for stage in 0..=5 {
        let line = stderr
            .lines()
            .find(|l| l.starts_with(&format!("wait-check stage={stage} ")))
            .unwrap_or_else(|| panic!("no check for stage {stage}: {stderr}"));
        println!("{line}");
        assert!(line.ends_with("ok=true"), "{line}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
