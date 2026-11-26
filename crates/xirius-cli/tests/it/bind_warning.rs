#[test]
fn external_bind_prints_a_warning_before_refusing_to_start() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-bind-warning-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("bind-warning");
    let session = xirius_core::Session::create(&dir, xirius_core::Project::new("bind")).unwrap();
    drop(session);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius"))
        .args(["--song"])
        .arg(&dir)
        .args([
            "mcp",
            "--http",
            "--bind",
            "0.0.0.0",
            "--no-device",
            "--midi-in",
            "none",
            "--autosave-sec",
            "0",
        ])
        .env("XIRIUS_CONFIG_DIR", dir.join("prefs"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let warning = String::from_utf8(output.stderr).unwrap();
    assert!(
        warning.contains("警告:") && warning.contains("localhost"),
        "{warning}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
