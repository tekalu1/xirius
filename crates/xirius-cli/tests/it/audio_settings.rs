//! Audio/MIDI commands share the UI preferences without opening device streams.
use std::process::Command;

#[test]
fn audio_and_midi_settings_survive_separate_cli_processes() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-cli-audioset-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("cli-audioset");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_xirius"))
            .env_remove("XIRIUS_SONG")
            .env("XIRIUS_CONFIG_DIR", &dir)
            .current_dir(std::env::temp_dir())
            .args(args)
            .arg("--json")
            .output()
            .unwrap()
    };
    for args in [
        vec!["audio", "set", "buffer", "128"],
        vec!["audio", "set", "record-offset", "-7.5"],
        vec!["midi", "set", "follow-selection", "false"],
        vec!["midi", "set", "inputs.Test Keys", "false"],
        vec!["midi", "set", "input-offset", "4.5"],
    ] {
        let result = run(&args);
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let audio: serde_json::Value = serde_json::from_slice(&run(&["audio", "get"]).stdout).unwrap();
    assert_eq!(audio["data"]["settings"]["audio.buffer"], 128);
    assert_eq!(audio["data"]["settings"]["audio.record-offset"], -7.5);
    let midi: serde_json::Value = serde_json::from_slice(&run(&["midi", "get"]).stdout).unwrap();
    assert_eq!(midi["data"]["settings"]["midi.follow-selection"], false);
    assert_eq!(midi["data"]["settings"]["midi.inputs.Test Keys"], false);
    assert_eq!(midi["data"]["settings"]["midi.input-offset"], 4.5);
    assert!(!run(&["audio", "set", "buffer", "33"]).status.success());
    let before = std::fs::read(dir.join("prefs.toml")).unwrap();
    assert!(
        run(&["audio", "set", "buffer", "512", "--dry-run"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(dir.join("prefs.toml")).unwrap(), before);
    std::fs::remove_dir_all(dir).unwrap();
}
