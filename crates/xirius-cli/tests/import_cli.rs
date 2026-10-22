//! `xirius clip.import`, run as a user would: the file is read, placed in one step, and its
//! waveform's peaks are in the song's `Peaks/` when the program has ended.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

struct Song {
    dir: PathBuf,
}

impl Song {
    fn new(tag: &str) -> Song {
        let dir =
            std::env::temp_dir().join(format!("xirius-cli-import-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = Song { dir };
        let out = Command::new(env!("CARGO_BIN_EXE_xirius"))
            .args(["init"])
            .arg(&s.dir)
            .args(["--name", "Import"])
            .output()
            .expect("run xirius init");
        assert!(out.status.success(), "{}", text(&out.stderr));
        s
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_xirius"))
            .env_remove("XIRIUS_SONG")
            .current_dir(std::env::temp_dir())
            .arg("--song")
            .arg(&self.dir)
            .args(args)
            .arg("--json")
            .output()
            .expect("run xirius")
    }

    fn ok(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}{}",
            text(&out.stdout),
            text(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("json output")
    }

    fn project(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.dir.join("project.json")).unwrap()).unwrap()
    }
}

impl Drop for Song {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn wav(path: &Path, rate: u32, frames: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..frames {
        let x = (i as f32 * 0.05).sin() * 0.5;
        w.write_sample((x * 32767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

#[test]
fn a_file_is_imported_from_the_command_line_with_its_peaks_kept_in_the_song() {
    let s = Song::new("cli");
    let file = s.dir.join("Samples").join("Loop 100bpm.wav");
    wav(&file, 44_100, 44_100 * 12 / 5);

    // --dry-run reads it and changes nothing.
    let before = std::fs::read(s.dir.join("project.json")).unwrap();
    let dry = s.ok(&[
        "clip.import",
        "--file",
        "Samples/Loop 100bpm.wav",
        "--dry-run",
    ]);
    assert_eq!(dry["dry_run"], true);
    assert_eq!(std::fs::read(s.dir.join("project.json")).unwrap(), before);

    let done = s.ok(&["clip.import", "--file", "Samples/Loop 100bpm.wav"]);
    let msg = done["message"].as_str().unwrap_or_default();
    assert!(msg.contains("Loop 100bpm.wav"), "{done}");
    assert!(msg.contains("100 BPM"), "{done}");
    assert_eq!(done["data"]["rate"], 44_100);
    assert_eq!(done["data"]["file_bpm"], 100.0);
    let saved = s.project();
    assert!(
        saved.to_string().contains("Samples/Loop 100bpm.wav"),
        "{saved}"
    );
    // The program waited for the peaks before it ended.
    let peaks = std::fs::read_dir(s.dir.join("Peaks"))
        .expect("Peaks/")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(
        peaks
            .iter()
            .any(|n| n.starts_with("Loop_100bpm-") && n.ends_with(".xpk")),
        "{peaks:?}"
    );

    // One undo takes it away.
    s.ok(&["history.undo"]);
    assert!(!s.project().to_string().contains("Loop 100bpm.wav"));

    // A file that is not there is an error, and nothing changes.
    let out = s.run(&["clip.import", "--file", "Samples/None.wav"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stdout).contains("見つかりません")
            || text(&out.stderr).contains("見つかりません")
    );
}

#[test]
fn brw_004_the_pools_commands_run_from_the_command_line() {
    let s = Song::new("pool");
    let outside = tempfile::Builder::new()
        .prefix("xirius-cli-pool-outside-")
        .tempdir()
        .unwrap();
    let kick = outside.path().join("Kick.wav");
    wav(&kick, 48_000, 4_800);
    let kick_text = kick.to_string_lossy().into_owned();
    let bytes = std::fs::metadata(&kick).unwrap().len();

    // With the copy off, the file read in stays where it is and is listed as outside.
    s.ok(&["project.settings", "--copy_imports", "false"]);
    s.ok(&["clip.import", "--file", &kick_text]);
    let list = s.ok(&["file.list"]);
    let row = &list["data"]["files"][0];
    assert_eq!(row["kind"], "import", "{list}");
    assert_eq!(row["external"], true, "{list}");
    assert_eq!(row["bytes"], bytes, "{list}");

    let before = std::fs::read(s.dir.join("project.json")).unwrap();
    s.ok(&["file.collect", "--dry-run"]);
    assert_eq!(std::fs::read(s.dir.join("project.json")).unwrap(), before);
    let done = s.ok(&["file.collect"]);
    assert!(
        done["message"].as_str().unwrap().contains("コピーしました"),
        "{done}"
    );
    assert!(s.dir.join("Media/Imports/Kick.wav").is_file());
    assert!(s.project().to_string().contains("Media/Imports/Kick.wav"));

    let picked = s.ok(&["select.using_file", "--file", "Media/Imports/Kick.wav"]);
    assert!(
        picked["message"]
            .as_str()
            .unwrap()
            .contains("クリップ 1 個"),
        "{picked}"
    );

    // The copy moved away: the clip is pointed at where it is now.
    let moved = outside.path().join("moved");
    std::fs::create_dir_all(&moved).unwrap();
    std::fs::rename(s.dir.join("Media/Imports/Kick.wav"), moved.join("Kick.wav")).unwrap();
    let found = s.ok(&["file.locate", "--folder", &moved.to_string_lossy()]);
    assert_eq!(found["data"]["missing"], 0, "{found}");
    let list = s.ok(&["file.list"]);
    assert_eq!(list["data"]["files"][0]["exists"], true, "{list}");

    // Without a path the song's folder is what would be shown.
    let shown = s.ok(&["file.reveal_os", "--dry-run"]);
    let path = shown["data"]["path"].as_str().unwrap().to_string();
    assert_eq!(
        std::fs::canonicalize(path).unwrap(),
        std::fs::canonicalize(&s.dir).unwrap()
    );
}
