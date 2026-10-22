//! Exporting and playing through the `xirius` program: a real process, a real song folder.
//! Playing goes to a silent output (`--no-device`), so the tests make no sound.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Song {
    dir: PathBuf,
}

impl Song {
    fn new() -> Song {
        let dir = std::env::temp_dir().join(format!(
            "xirius-cli-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let out = Command::new(env!("CARGO_BIN_EXE_xirius"))
            .arg("init")
            .arg(&dir)
            .args(["--name", "Sketch"])
            .output()
            .expect("run xirius init");
        assert!(out.status.success(), "{}", text(&out.stderr));
        Song { dir }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_xirius"));
        c.env_remove("XIRIUS_SONG")
            .env("XIRIUS_TRASH_DIR", self.bin())
            .current_dir(std::env::temp_dir())
            .arg("--song")
            .arg(&self.dir)
            .args(args);
        c
    }

    /// Where the files the program sends to the recycle bin go instead.
    fn bin(&self) -> PathBuf {
        PathBuf::from(format!("{}-bin", self.dir.display()))
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().expect("run xirius")
    }

    fn ok(&self, args: &[&str]) -> Value {
        let mut a = args.to_vec();
        a.push("--json");
        let out = self.run(&a);
        assert!(
            out.status.success(),
            "{args:?} failed: {}{}",
            text(&out.stdout),
            text(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("json output")
    }

    fn id(&self, args: &[&str]) -> String {
        self.ok(args)["data"]["id"]
            .as_str()
            .expect("data.id")
            .to_string()
    }

    /// Feeds the lines to `xirius play --no-device --unpaced` and waits for it to end.
    fn play(&self, flags: &[&str], lines: &str) -> Output {
        let mut args = vec!["play", "--no-device", "--unpaced"];
        args.extend_from_slice(flags);
        let mut child = self
            .cmd(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run xirius play");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(lines.as_bytes())
            .expect("write stdin");
        child.wait_with_output().expect("wait for xirius play")
    }

    fn project_json(&self) -> Vec<u8> {
        std::fs::read(self.dir.join("project.json")).expect("project.json")
    }
}

impl Drop for Song {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_dir_all(self.bin());
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// An instrument track with one MIDI clip of two notes, two bars from the start.
fn song_with_notes() -> Song {
    let s = Song::new();
    let trk = s.id(&["track.add.instrument", "--name", "Keys"]);
    let clp = s.id(&[
        "clip.add.midi",
        "--track",
        &trk,
        "--at",
        "1.1.0",
        "--bars",
        "2",
    ]);
    for (at, pitch) in [("0", "60"), ("960", "64")] {
        s.id(&[
            "note.add", "--clip", &clp, "--at", at, "--pitch", pitch, "--length", "960",
        ]);
    }
    s
}

struct Wav {
    spec: hound::WavSpec,
    frames: usize,
    peak: f32,
}

fn read_wav(path: &Path) -> Wav {
    let mut r = hound::WavReader::open(path).expect("open wav");
    let spec = r.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(Result::unwrap).collect(),
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|v| v.unwrap() as f32 / scale)
                .collect()
        }
    };
    Wav {
        spec,
        frames: samples.len() / spec.channels as usize,
        peak: samples.iter().fold(0.0, |m, v| m.max(v.abs())),
    }
}

#[test]
fn the_mixdown_command_writes_the_song_as_a_wav() {
    let s = song_with_notes();
    let out = s.dir.join("mix.wav");
    let before = s.project_json();
    let v = s.ok(&[
        "export.mixdown",
        "--range",
        "song",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(v["ok"], true);
    let wav = read_wav(&out);
    assert_eq!(wav.spec.channels, 2);
    assert_eq!(wav.spec.sample_rate, 48000);
    assert_eq!(wav.spec.bits_per_sample, 24);
    // Two bars of 4/4 at 120 bpm are four seconds.
    assert_eq!(wav.frames, 4 * 48000);
    assert!(wav.peak > 0.01, "the mix is silent: {}", wav.peak);
    assert_eq!(s.project_json(), before, "an export leaves the song alone");

    let background = s.run(&["export.mixdown", "--background", "--json"]);
    assert_eq!(background.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&background.stdout).unwrap();
    assert_eq!(error["error"]["code"], "invalid_args");
}

#[test]
fn the_export_follows_the_flags_and_the_dry_run_writes_nothing() {
    let s = song_with_notes();
    let out = s.dir.join("short.wav");
    s.ok(&[
        "export.mixdown",
        "--range",
        "custom",
        "--from",
        "1.1.0",
        "--to",
        "2.1.0",
        "--bit_depth",
        "16",
        "--sample_rate",
        "44100",
        "--out",
        out.to_str().unwrap(),
    ]);
    let wav = read_wav(&out);
    assert_eq!(wav.spec.bits_per_sample, 16);
    assert_eq!(wav.spec.sample_rate, 44100);
    assert_eq!(wav.frames, 2 * 44100);

    let none = s.dir.join("none.wav");
    let v = s.ok(&[
        "export.mixdown",
        "--dry-run",
        "--out",
        none.to_str().unwrap(),
    ]);
    assert_eq!(v["dry_run"], true);
    assert!(!none.exists());
}

#[test]
fn stems_go_to_a_folder_one_file_each() {
    let s = song_with_notes();
    let dir = s.dir.join("stems");
    let v = s.ok(&["export.stems", "--dir", dir.to_str().unwrap()]);
    assert_eq!(v["ok"], true);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(names.iter().any(|n| n.ends_with("_Keys.wav")), "{names:?}");
    assert!(names.iter().all(|n| n.ends_with(".wav")), "{names:?}");
}

#[test]
fn an_ai_without_the_save_permission_cannot_export() {
    let s = song_with_notes();
    let out = s.dir.join("ai.wav");
    let r = s.run(&[
        "--as",
        "helper",
        "export.mixdown",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(1), "{}", text(&r.stderr));
    assert!(!out.exists());
}

#[test]
fn the_new_commands_are_in_the_command_list() {
    let s = song_with_notes();
    let v = s.ok(&["ops", "list"]);
    let text = v.to_string();
    for name in ["export.mixdown", "export.stems", "transport.play"] {
        assert!(text.contains(name), "{name} missing from ops list");
    }
}

#[test]
fn transport_commands_need_xirius_play() {
    let s = song_with_notes();
    let r = s.run(&["transport.stop"]);
    assert_eq!(r.status.code(), Some(1));
    assert!(
        text(&r.stderr).contains("xirius play"),
        "{}",
        text(&r.stderr)
    );
}

#[test]
fn play_plays_the_song_out_and_reports_where_it_is() {
    let s = song_with_notes();
    let r = s.play(&[], "wait\nstatus\n");
    assert!(r.status.success(), "{}", text(&r.stderr));
    let out = text(&r.stdout);
    assert!(out.contains("停止中"), "{out}");
    assert!(text(&r.stderr).contains("無音"), "{}", text(&r.stderr));
}

#[test]
fn play_takes_seek_stop_and_loop_by_command() {
    let s = song_with_notes();
    let range = ["loop.set", "--start", "1.1.0", "--end", "2.1.0"];
    s.ok(&range);
    // Stop, move, and read the position back.
    let r = s.play(&["--json"], "stop\nseek --to 2.1.0\nstatus\nquit\n");
    assert!(r.status.success(), "{}", text(&r.stderr));
    let out = text(&r.stdout);
    let line = out
        .lines()
        .find(|l| l.contains("\"frame\""))
        .unwrap_or_else(|| panic!("no status line in {out}"));
    let v: Value = serde_json::from_str(line).unwrap();
    assert_eq!(v["playing"], false);
    assert_eq!(v["frame"], 2 * 48000, "bar 2 is two seconds in at 120 bpm");

    // With the loop on and the song set to repeat, the input's end does not wait for ever.
    let r = s.play(&["--loop"], "status\n");
    assert!(r.status.success(), "{}", text(&r.stderr));
    assert!(text(&r.stdout).contains("ループ"), "{}", text(&r.stdout));
}

#[test]
fn an_edit_while_playing_is_saved_and_undo_takes_it_back() {
    let s = song_with_notes();
    let before = s.project_json();
    let tracks = s.ok(&["track.list"]);
    let id = tracks["data"][0]["id"]
        .as_str()
        .expect("track id")
        .to_string();
    let r = s.play(
        &[],
        &format!("track.volume.set --ids {id} --db -6\nhistory.undo\n"),
    );
    assert!(r.status.success(), "{}{}", text(&r.stdout), text(&r.stderr));
    assert_eq!(s.project_json(), before, "the undo restored the saved song");
    let out = text(&r.stdout);
    assert!(out.contains("-6"), "{out}");
}

#[test]
fn a_line_that_fails_is_reported_and_the_exit_code_says_so() {
    let s = song_with_notes();
    let r = s.play(&[], "no.such.command\nstatus\n");
    assert_eq!(r.status.code(), Some(1));
    assert!(text(&r.stdout).contains("停止中") || text(&r.stdout).contains("再生中"));
}

#[test]
fn play_needs_a_song_and_knows_its_flags() {
    let out = Command::new(env!("CARGO_BIN_EXE_xirius"))
        .env_remove("XIRIUS_SONG")
        .current_dir(std::env::temp_dir())
        .args(["play", "--no-device"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = Command::new(env!("CARGO_BIN_EXE_xirius"))
        .args(["play", "--bogus"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn each_one_shot_export_leaves_a_record_the_next_process_reads() {
    let s = song_with_notes();
    let r = s.ok(&["export.mixdown", "--format", "flac", "--bit_depth", "24"]);
    let id = r["data"]["record"]
        .as_str()
        .expect("data.record")
        .to_string();
    let flac = s.dir.join("Mixdown").join("Sketch.flac");
    assert_eq!(r["data"]["path"], flac.display().to_string());
    assert_eq!(&std::fs::read(&flac).unwrap()[..4], b"fLaC");

    // A second process: the list, the record and its settings.
    let all = s.ok(&["export.status"]);
    let jobs = all["data"]["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1, "{all}");
    assert_eq!(jobs[0]["job"], id.as_str());
    assert_eq!(jobs[0]["state"], "done");
    let open = s.ok(&["export.open", "--job", &id]);
    assert_eq!(open["data"]["args"]["format"], "flac");
    assert_eq!(open["data"]["args"]["bit_depth"], 24);

    // A third: the same export again over the same file, one line in the list.
    let again = s.ok(&["export.update", "--job", &id]);
    assert_eq!(again["data"]["path"], flac.display().to_string());
    let jobs = s.ok(&["export.status"])["data"]["jobs"].clone();
    assert_eq!(jobs.as_array().unwrap().len(), 1, "{jobs}");
    assert_eq!(jobs[0]["replaces"], id.as_str());
    let names: Vec<String> = std::fs::read_dir(s.dir.join("Mixdown"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["Sketch.flac"]);
    let binned: Vec<String> = std::fs::read_dir(s.bin())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        binned,
        vec!["Sketch.flac"],
        "the first file went to the recycle bin"
    );
}
