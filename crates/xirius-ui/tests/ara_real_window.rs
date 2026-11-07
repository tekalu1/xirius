//! A real ARA plugin on a clip in a parked window (`--script ara-real`, EDT-023, PLG-036,
//! PLG-055). Needs `XIRIUS_ARA_REAL_PLUGIN`; run by `python scripts/ci_tests.py --plugins`.

fn copy_support(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_support(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

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
    for part in [
        "profile/Documents",
        "profile/Music",
        "profile/Desktop",
        "profile/Downloads",
        "profile/AppData/Local",
        "profile/AppData/Roaming",
        "local",
        "roaming",
        "temp",
        "common/VST3",
    ] {
        std::fs::create_dir_all(dir.join(part)).unwrap();
    }
    // Optional program-code dependencies, laid out relative to CommonProgramFiles.
    // The fixture contains no user settings, authorization or license data.
    if let Some(support) = std::env::var_os("XIRIUS_ARA_REAL_COMMON_FILES") {
        copy_support(std::path::Path::new(&support), &dir.join("common"));
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "ara-real", "--shot"])
        .arg(&dir)
        .env("USERPROFILE", dir.join("profile"))
        .env("LOCALAPPDATA", dir.join("local"))
        .env("APPDATA", dir.join("roaming"))
        .env("TEMP", dir.join("temp"))
        .env("TMP", dir.join("temp"))
        .env("CommonProgramFiles", dir.join("common"))
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
