//! A `xirius` process killed while it had a song open (PRJ-012): the next one
//! says how many operations came back and which was the last.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::session::{RunOptions, Session};

/// A song folder named `tag`, not made yet, in a folder of the test's own that goes when the
/// guard is dropped (also when the test fails).
fn temp_dir(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("xirius-cli-recovery-")
        .tempdir()
        .unwrap();
    let dir = base.path().join(tag);
    (base, dir)
}

/// A child process that is killed when dropped, so a failed test does not leave it holding
/// the song's folder.
struct Player(std::process::Child);

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_killed_player_leaves_a_song_that_opens_with_the_count_and_the_last_operation() {
    let (_tmp, dir) = temp_dir("killed");
    let mut s = Session::new("落ちた曲");
    s.save_as(&dir).unwrap();
    for name in ["one", "two"] {
        s.run_value(
            "track.add.audio",
            json!({"name": name}),
            &RunOptions::human(),
        )
        .unwrap();
    }
    drop(s);

    // The player holds the song (and its lock) until it is killed.
    let mut player = Player(
        Command::new(env!("CARGO_BIN_EXE_xirius"))
            .env_remove("XIRIUS_SONG")
            .arg("--song")
            .arg(&dir)
            .args(["play", "--no-device"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let lock = dir.join("session.lock");
    let started = Instant::now();
    while !lock.exists() {
        assert!(started.elapsed() < Duration::from_secs(30), "no lock");
        std::thread::sleep(Duration::from_millis(50));
    }
    player.0.kill().unwrap();
    player.0.wait().unwrap();
    assert!(lock.exists(), "a killed process does not clean up");

    let out = Command::new(env!("CARGO_BIN_EXE_xirius"))
        .env_remove("XIRIUS_SONG")
        .arg("--song")
        .arg(&dir)
        .args(["track.list", "--json"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("お知らせ: 前回は正常に終了しませんでした"),
        "{err}"
    );
    assert!(
        err.contains("お知らせ: 記録から 2 件の操作を戻しました。最後は「track.add.audio」です"),
        "{err}"
    );
}
