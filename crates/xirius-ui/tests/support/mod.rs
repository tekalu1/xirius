//! Shared by the window tests: the app started with folders of the test's own, and the files the
//! built-in sample uses.
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

/// The app, with everything a run would read or write of the person's under `root/profile`:
/// their profile (`LOCALAPPDATA\Xirius` holds the browser's places, the plugin list and the crash
/// records), the temporary folder, the shared plugin folders (no plugin of this PC is scanned or
/// loaded), the settings and the crash records. A test sets its own values after this one.
/// Every window test starts the app this way (`tests/it/window_sandbox.rs`).
pub fn app(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xirius-ui"));
    let profile = root.join("profile");
    #[cfg(windows)]
    let names = [
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
        "CommonProgramFiles",
        "CommonProgramW6432",
        "CommonProgramFiles(x86)",
    ];
    #[cfg(not(windows))]
    let names = ["HOME", "TMPDIR"];
    for name in names {
        let directory = profile.join(name);
        std::fs::create_dir_all(&directory).unwrap();
        command.env(name, directory);
    }
    command
        .env("XIRIUS_CONFIG_DIR", profile.join("config"))
        .env("XIRIUS_CRASH_DIR", profile.join("crashes"))
        .env_remove("XIRIUS_VST3_PATH")
        .env_remove("CLAP_PATH");
    command
}

/// Files used by the built-in sample, under each window test's own working directory.
pub fn sample_audio(root: &Path) {
    let silence = vec![0.0; 48_000];
    for name in ["vocal_take1.wav", "guitar_di.wav"] {
        xirius_session::export::write_wav(&root.join(name), &silence, &silence, 48_000, 16)
            .unwrap();
    }
}
