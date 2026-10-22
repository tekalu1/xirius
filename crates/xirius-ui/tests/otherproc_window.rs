//! UI-289: the real `xirius` program changes the song open in a real parked window, with a
//! silent output; the window is pressed only through its own input.

use std::path::PathBuf;
use std::process::Command;

/// The `xirius` program, built first when it is not there.
fn cli() -> PathBuf {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_xirius-ui"))
        .with_file_name(format!("xirius{}", std::env::consts::EXE_SUFFIX));
    let mut build = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    build.args(["build", "-p", "xirius-cli", "--bin", "xirius"]);
    if !cfg!(debug_assertions) {
        build.arg("--release");
    }
    assert!(build.status().unwrap().success(), "xirius builds");
    assert!(exe.is_file(), "{}", exe.display());
    exe
}

#[test]
fn another_app_s_saves_come_in_through_the_window_and_wait_for_a_drag() {
    let temporary = tempfile::tempdir().unwrap();
    let pictures = std::env::var_os("XIRIUS_OTHERPROC_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| temporary.path().join("shots"));
    std::fs::create_dir_all(&pictures).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "otherproc", "--shot"])
        .arg(&pictures)
        .env("XIRIUS_OTHERPROC_CLI", cli())
        .env("XIRIUS_OTHERPROC_ROOT", temporary.path().join("songs"))
        .env("XIRIUS_CONFIG_DIR", temporary.path().join("prefs"))
        .env("XIRIUS_CRASH_DIR", temporary.path().join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for stage in 0..=10 {
        let line = stderr
            .lines()
            .find(|line| line.starts_with(&format!("otherproc-check stage={stage} ")))
            .unwrap_or_else(|| panic!("stage {stage} missing: {stderr}"));
        println!("{line}");
        assert!(line.contains("ok=true"), "{line}");
    }
    let shots: Vec<_> = std::fs::read_dir(&pictures)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("otherproc") && name.ends_with(".bmp")
        })
        .collect();
    assert_eq!(shots.len(), 11);
    for entry in shots {
        assert!(entry.metadata().unwrap().len() > 54);
    }
}
