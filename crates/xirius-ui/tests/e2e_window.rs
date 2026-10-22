//! One song made from start to end through the real window's presses (ARC-040): every stage of
//! the `e2e` script checks the screen and the song, and says so on stderr.

/// The stages of the `e2e` script.
const STAGES: u8 = 34;

#[test]
fn a_song_is_made_saved_opened_written_and_undone_through_the_real_window() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-e2e-window-")
        .tempdir()
        .unwrap();
    let root = tmp.path();
    let shots = root.join("shots");
    for d in [
        "config", "trash", "keep", "peaks", "presets", "crash", "song",
    ] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::fs::create_dir_all(&shots).unwrap();
    // The built-in instrument unless a plugin instrument is named for the run, so the result
    // does not depend on what this PC has installed.
    let instrument = std::env::var("XIRIUS_TEST_VST3_INSTRUMENT").unwrap_or_else(|_| "none".into());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "e2e", "--shot"])
        .arg(&shots)
        .env("XIRIUS_CONFIG_DIR", root.join("config"))
        .env("XIRIUS_TRASH_DIR", root.join("trash"))
        .env("XIRIUS_KEEP_DIR", root.join("keep"))
        .env("XIRIUS_PLUGIN_CACHE", root.join("plugins.json"))
        .env("XIRIUS_PEAK_CACHE", root.join("peaks"))
        .env("XIRIUS_TRACK_PRESETS", root.join("presets"))
        .env("XIRIUS_CRASH_DIR", root.join("crash"))
        .env("XIRIUS_E2E_DIR", root.join("song"))
        .env("XIRIUS_TEST_VST3_INSTRUMENT", instrument)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for stage in 0..STAGES {
        let line = stderr
            .lines()
            .find(|l| l.starts_with(&format!("e2e-check stage={stage} ")))
            .unwrap_or_else(|| panic!("no line for stage {stage}:\n{stderr}"));
        assert!(line.contains(" ok=true "), "{line}\n{stderr}");
    }
    let pictures = std::fs::read_dir(&shots)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "bmp"))
        .count();
    assert_eq!(pictures, usize::from(STAGES));
}
