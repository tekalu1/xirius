//! The settings from the command line: no song needed, saved to `XIRIUS_CONFIG_DIR`, and
//! read back by the next run (and by the screen, which reads the same files).

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

struct Config {
    dir: PathBuf,
}

impl Config {
    fn new(name: &str) -> Config {
        let dir =
            std::env::temp_dir().join(format!("xirius-cli-prefs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Config { dir }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_xirius"))
            .env_remove("XIRIUS_SONG")
            .env("XIRIUS_CONFIG_DIR", &self.dir)
            .current_dir(std::env::temp_dir())
            .args(args)
            .output()
            .expect("run xirius")
    }

    fn ok(&self, args: &[&str]) -> Value {
        let mut a = args.to_vec();
        a.push("--json");
        let out = self.run(&a);
        assert!(
            out.status.success(),
            "{args:?} failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("json output")
    }

    fn file(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }
}

impl Drop for Config {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_setting_is_saved_without_a_song_and_read_back() {
    let c = Config::new("set");
    let r = c.ok(&["prefs", "set", "edit.automation.follows-clips", "false"]);
    assert_eq!(r["data"]["edit.automation.follows-clips"], false);
    assert!(
        c.file("prefs.toml").contains("follows-clips = false"),
        "{}",
        c.file("prefs.toml")
    );
    let r = c.ok(&["prefs.get", "edit.automation"]);
    assert_eq!(r["data"]["edit.automation.follows-clips"], false);
    assert_eq!(r["data"]["edit.automation.locks-clips"], true);

    // A dry run says what would change and leaves the file alone.
    let before = c.file("prefs.toml");
    let r = c.ok(&["prefs.set", "ui.theme", "light", "--dry-run"]);
    assert!(r["message"].as_str().unwrap().contains("試行"), "{r}");
    assert_eq!(c.file("prefs.toml"), before);

    // A bad value is a command failure (exit 1).
    let out = c.run(&["prefs.set", "audio.buffer", "5000"]);
    assert_eq!(out.status.code(), Some(1));

    let r = c.ok(&["prefs.reset"]);
    assert!(r["message"].as_str().unwrap().starts_with('1'), "{r}");
}

#[test]
fn keys_and_mouse_holds_from_the_command_line() {
    let c = Config::new("keys");
    c.ok(&[
        "keymap",
        "set",
        "Mod+Alt+V",
        "note.transpose --by 12",
        "--context",
        "roll",
    ]);
    assert!(c.file("keymap.toml").contains("note.transpose --by 12"));
    let r = c.ok(&["keymap.which", "Mod+Alt+V", "--context", "roll"]);
    assert_eq!(r["data"]["command"], "note.transpose --by 12");

    // Q is the quantize key in the roll: a second command on it is a conflict.
    c.ok(&["keymap.set", "Q", "note.delete", "--context", "roll"]);
    let r = c.ok(&["keymap.conflicts"]);
    let issues = r["data"].as_array().expect("issues");
    assert!(
        issues
            .iter()
            .any(|i| i["kind"] == "conflict" && i["key"] == "Q"),
        "{r}"
    );

    // Another preset keeps the changed rows.
    let r = c.ok(&["keymap.preset", "cubase"]);
    assert_eq!(r["data"]["kept"], 2);
    let r = c.ok(&["keymap.get", "--changed"]);
    assert_eq!(r["data"]["preset"], "cubase");

    c.ok(&["mousemap.set", "roll.select.hold", "Mod+Alt", "tool:mute"]);
    assert!(c.file("mousemap.toml").contains("tool:mute"));
    let r = c.ok(&["mousemap.get", "roll.select.hold"]);
    assert!(r["message"].as_str().unwrap().contains("tool:mute"), "{r}");

    let r = c.ok(&["keymap.export", "--format", "html"]);
    assert!(r["data"]["text"].as_str().unwrap().contains("<table>"));
}
