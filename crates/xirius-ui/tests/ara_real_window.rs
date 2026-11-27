//! A real ARA plugin on a clip in a parked window (`--script ara-real`, EDT-023, PLG-036,
//! PLG-055). Needs `XIRIUS_ARA_REAL_PLUGIN`; run by `python scripts/ci_tests.py --plugins`.
mod support;

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
    run(false);
}

#[cfg(all(windows, debug_assertions))]
#[test]
fn ara_fixture_restores_raises_and_tracks_edits_in_a_parked_window() {
    run(true);
}

fn run(fixture: bool) {
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
    if !fixture && let Some(support) = std::env::var_os("XIRIUS_ARA_REAL_COMMON_FILES") {
        copy_support(std::path::Path::new(&support), &dir.join("common"));
    }
    let run = |script: &str| {
        let mut command = support::app(&dir);
        if fixture {
            command.env("XIRIUS_ARA_NATIVE_FIXTURE", "1");
        } else {
            command.env_remove("XIRIUS_ARA_NATIVE_FIXTURE");
        }
        let output = command
            .args(["--park", "--silent", "--script", script, "--shot"])
            .arg(dir.join(script))
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
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        eprint!("{stderr}");
        assert!(output.status.success(), "{script}: {stderr}");
        stderr
    };
    let stderr = run("ara-real") + &run("ara-real-restart");
    let mut checks = vec![
        "picking=true",
        "attached=true",
        "badge=ready",
        "editing=true",
        "offscreen=true",
        "foreground=false",
        "closed=true",
        "saved=true",
        "detached=true",
        "undo_ready=true",
        "reopen painted_ready=true",
        "restart painted_ready=true",
        "undo painted_ready=true",
        "detach_unsaved=true",
        "close_prompt=true",
    ];
    if cfg!(windows) {
        checks.extend([
            "independent_raise=true",
            "channel_open raised=true",
            "main_click raised=true",
            "channel_close ordered=true",
            "modal_front ordered=true",
            "modal_restore ordered=true",
        ]);
    }
    if fixture {
        checks.push("edit_unsaved=true");
    }
    let missing: Vec<_> = checks
        .into_iter()
        .filter(|check| {
            !stderr
                .lines()
                .any(|line| line.starts_with("ara-check") && line.contains(*check))
        })
        .collect();
    assert!(missing.is_empty(), "failed checks: {missing:?}\n{stderr}");
    assert!(!stderr.contains("ara-skip"), "{stderr}");
}
