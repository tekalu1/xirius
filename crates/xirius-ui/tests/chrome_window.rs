#![cfg(windows)]

//! UI-115: the top bar of a parked window is the window's own caption (drag, double click to
//! maximise and restore, the edges to resize), asked of the window with messages and no OS input.

#[test]
fn the_top_bar_answers_as_the_caption_and_a_double_click_maximises_and_restores() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-chrome-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("chrome");
    std::fs::create_dir_all(&dir).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args([
            "--bench",
            "--park",
            "--chrome",
            "--scenario",
            "still",
            "--secs",
            "6",
            "--out",
        ])
        .arg(&dir)
        .env("XIRIUS_CONFIG_DIR", dir.join("prefs"))
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let md = std::fs::read_to_string(dir.join("chrome.md")).unwrap();
    assert!(md.contains("Hit tests: all as expected."), "{md}");
    assert!(md.contains("title bar, empty part"), "{md}");
    assert!(md.contains("| HTCAPTION | HTCAPTION | ok |"), "{md}");
    assert!(md.contains("SC_MAXIMIZE -> maximised: yes."), "{md}");
    assert!(md.contains("SC_RESTORE -> restored: yes."), "{md}");
    assert!(
        md.contains("Double click on the empty bar -> maximised: yes. Again -> restored: yes."),
        "{md}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
