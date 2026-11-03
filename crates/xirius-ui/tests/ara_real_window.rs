//! A real ARA plugin on a clip in a parked window (`--script ara-real`, EDT-023, PLG-036,
//! PLG-055). Needs `XIRIUS_ARA_REAL_PLUGIN`; run by `python scripts/ci_tests.py --plugins`.

#[test]
#[ignore = "needs XIRIUS_ARA_REAL_PLUGIN and a real window"]
fn a_real_ara_plugin_is_attached_edited_kept_and_detached_in_a_parked_window() {
    assert!(
        std::env::var_os("XIRIUS_ARA_REAL_PLUGIN").is_some(),
        "XIRIUS_ARA_REAL_PLUGIN names no plugin"
    );
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-ara-real-shots-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("ara-real-shots");
    std::fs::create_dir_all(&dir).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "ara-real", "--shot"])
        .arg(&dir)
        .env("XIRIUS_EDITOR_OFFSCREEN", "1")
        .env("XIRIUS_CONFIG_DIR", dir.join("prefs"))
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for check in [
        "picking=true",
        "attached=true",
        "badge=ready",
        "asked=true",
        "editing=true",
        "offscreen=true",
        "foreground=false",
        "closed=true",
        "saved=true",
        "opened=true",
        "detached=true",
    ] {
        assert!(
            stderr
                .lines()
                .any(|l| l.starts_with("ara-check") && l.contains(check)),
            "{check}: {stderr}"
        );
    }
    assert!(!stderr.contains("ara-skip"), "{stderr}");
}
