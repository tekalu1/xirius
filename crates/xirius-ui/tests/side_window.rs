//! UI-048: on a real (parked, silent) window, the pointer drags the browser's panel and the
//! track head to the widths asked for.

#[test]
fn the_browser_and_the_track_head_are_resized_by_their_edges() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-side-shots-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("side-shots");
    std::fs::create_dir_all(&dir).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "side", "--shot"])
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
            .find(|l| l.starts_with(&format!("side-check stage={stage} ")))
            .unwrap_or_else(|| panic!("no check for stage {stage}: {stderr}"));
        println!("{line}");
        assert!(line.ends_with("ok=true"), "{line}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
