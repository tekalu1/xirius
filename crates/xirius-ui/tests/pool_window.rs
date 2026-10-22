//! The song-files tab (BRW-004) through the real window: `--script pool` presses its rows and
//! menus, answers the folder window in its place, drags a file onto the lanes, moves with the
//! keys and fills the song's settings, on a song made in the test's own folder. `--script
//! pool-tidy` orders the list by name and puts the recordings nothing uses in a bin of the test's
//! own through 「使われていない録音を整理」.

#[test]
fn the_song_files_tab_follows_the_presses_drags_and_keys_of_a_real_window() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-pool-window-")
        .tempdir()
        .unwrap();
    let shots = tmp.path().join("shots");
    std::fs::create_dir_all(&shots).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "pool", "--shot"])
        .arg(&shots)
        .env("XIRIUS_POOL_DIR", tmp.path().join("song"))
        .env("XIRIUS_CONFIG_DIR", tmp.path().join("prefs"))
        .env("XIRIUS_CRASH_DIR", tmp.path().join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for stage in 0..9 {
        assert!(
            stderr.contains(&format!("pool-check stage={stage} ok=true")),
            "stage {stage}: {stderr}"
        );
    }
    let taken = std::fs::read_dir(&shots)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "bmp"))
        .count();
    assert_eq!(taken, 9);
}

#[test]
fn the_order_and_the_cleanup_dialog_follow_the_presses_and_keys_of_a_real_window() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-pool-tidy-window-")
        .tempdir()
        .unwrap();
    let shots = tmp.path().join("shots");
    std::fs::create_dir_all(&shots).unwrap();
    let bin = tmp.path().join("bin");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "pool-tidy", "--shot"])
        .arg(&shots)
        .env("XIRIUS_POOL_DIR", tmp.path().join("song"))
        .env("XIRIUS_TRASH_DIR", &bin)
        .env("XIRIUS_CONFIG_DIR", tmp.path().join("prefs"))
        .env("XIRIUS_CRASH_DIR", tmp.path().join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for stage in 0..5 {
        assert!(
            stderr.contains(&format!("pool-tidy-check stage={stage} ok=true")),
            "stage {stage}: {stderr}"
        );
    }
    assert_eq!(std::fs::read_dir(&bin).unwrap().count(), 3);
    let taken = std::fs::read_dir(&shots)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "bmp"))
        .count();
    assert_eq!(taken, 5);
}
