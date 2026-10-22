#[test]
fn acid_wav_import_and_undo_follow_palette_clicks_in_a_real_window() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-import-choice-window-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("import-choice-window");
    std::fs::create_dir_all(&dir).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "importchoice", "--shot"])
        .arg(&dir)
        .env("XIRIUS_CONFIG_DIR", dir.join("prefs"))
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for stage in 0..5 {
        assert!(
            stderr.contains(&format!("import-choice-check stage={stage} ok=true")),
            "{stderr}"
        );
    }
    assert_eq!(
        std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "bmp"))
            .count(),
        5
    );
    std::fs::remove_dir_all(dir).unwrap();
}
