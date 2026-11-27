//! UI-039: on a real (parked, silent) window, a view that did not change is not painted again.
mod support;

#[test]
fn an_unchanged_overview_is_not_painted_again_and_a_changed_one_is_painted_once() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-repaint-shots-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("repaint-shots");
    std::fs::create_dir_all(&dir).unwrap();
    let output = support::app(dir_tmp.path())
        .args(["--park", "--silent", "--script", "repaint", "--shot"])
        .arg(&dir)
        .env("XIRIUS_CONFIG_DIR", dir.join("prefs"))
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for stage in [2, 3] {
        let line = stderr
            .lines()
            .find(|l| l.starts_with(&format!("repaint-check stage={stage} ")))
            .unwrap_or_else(|| panic!("no check for stage {stage}: {stderr}"));
        println!("{line}");
        assert!(line.ends_with("ok=true"), "{line}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
