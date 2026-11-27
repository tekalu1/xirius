mod support;

#[test]
fn playback_and_clicks_keep_the_parked_window_silent_and_without_os_focus() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-quiet-shots-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("quiet-shots");
    std::fs::create_dir_all(&dir).unwrap();
    support::sample_audio(&dir);
    let output = support::app(dir_tmp.path())
        .args(["--park", "--silent", "--script", "quiet", "--shot"])
        .arg(&dir)
        .current_dir(&dir)
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

// That screen scripts inject no OS input is read from the sources in `source_rules.rs`, which
// runs on CI.
