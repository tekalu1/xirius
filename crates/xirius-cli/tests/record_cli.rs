//! Recording through the `xirius` program (REC-001, REC-016, ENG-028): `record.start` /
//! `record.stop` in `xirius play` and as MCP tools, both from the one definition in core's
//! command table. The output is silent (`--no-device`), so there is no input either: the takes
//! are silence of the right length, and nothing is played to the speakers. MIDI comes from a
//! file of messages (`--midi-script`), never from a device.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A song folder with an armed audio track "Vox", removed when dropped.
struct Song {
    dir: PathBuf,
}

impl Song {
    fn new() -> Song {
        let dir = std::env::temp_dir().join(format!(
            "xirius-cli-record-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = xirius_core::Session::create(&dir, xirius_core::Project::new("Rec")).unwrap();
        let human = xirius_core::session::RunOptions::human();
        let run = |s: &mut xirius_core::Session, name: &str, args: Value| {
            s.run(name, args.as_object().unwrap(), &human).unwrap()
        };
        run(&mut s, "ai.permission.set", json!({"level": "save"}));
        let r = run(&mut s, "track.add.audio", json!({"name": "Vox"}));
        let vox = r.data["id"].as_str().unwrap().to_string();
        run(&mut s, "track.arm", json!({"ids": [vox], "on": true}));
        s.save().unwrap();
        Song { dir }
    }

    /// Runs `xirius` on the song with settings of its own, so the person's settings (an input
    /// quantize, say) do not change what is recorded.
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_xirius"));
        c.env_remove("XIRIUS_SONG")
            .env("XIRIUS_CONFIG_DIR", self.dir.join("config"))
            .env("XIRIUS_PLUGIN_CACHE", self.dir.join("plugins.json"))
            .current_dir(std::env::temp_dir())
            .arg("--song")
            .arg(&self.dir)
            .args(args);
        c
    }

    /// Adds an armed audio track `name` (input `input`) or, with `input` `None`, an armed
    /// instrument track.
    fn arm(&self, name: &str, input: Option<&str>) {
        let mut s = xirius_core::Session::open(&self.dir).unwrap();
        let human = xirius_core::session::RunOptions::human();
        let run = |s: &mut xirius_core::Session, name: &str, args: Value| {
            s.run(name, args.as_object().unwrap(), &human).unwrap()
        };
        let kind = if input.is_some() {
            "track.add.audio"
        } else {
            "track.add.instrument"
        };
        let r = run(&mut s, kind, json!({"name": name}));
        let id = r.data["id"].as_str().unwrap().to_string();
        run(&mut s, "track.arm", json!({"ids": [id], "on": true}));
        if let Some(input) = input {
            run(
                &mut s,
                "track.input.set",
                json!({"ids": [id], "input": input}),
            );
        }
        s.save().unwrap();
    }

    /// A file of MIDI messages for `--midi-script`.
    fn midi_script(&self) -> PathBuf {
        let path = self.dir.join("keys.json");
        let msgs = json!([[2400, [0x90, 60, 100]], [26400, [0x80, 60, 0]]]);
        std::fs::write(&path, msgs.to_string()).unwrap();
        path
    }

    fn play(&self, lines: &str) -> Output {
        self.play_with(&[], lines)
    }

    fn play_with(&self, extra: &[&str], lines: &str) -> Output {
        let mut args = vec!["play", "--no-device", "--unpaced"];
        args.extend(extra);
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
            .unwrap()
            .write_all(lines.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    /// The clips of Vox in the saved song.
    fn clips(&self) -> Vec<Value> {
        self.clips_of("Vox")
    }

    /// The clips of the track `name` in the saved song.
    fn clips_of(&self, name: &str) -> Vec<Value> {
        let file: Value =
            serde_json::from_slice(&std::fs::read(self.dir.join("project.json")).unwrap()).unwrap();
        file["project"]["tracks"]
            .as_object()
            .expect("tracks in project.json")
            .values()
            .find(|t| t["name"] == name)
            .expect("the track in project.json")["clips"]
            .as_array()
            .cloned()
            .unwrap_or_default()
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

fn frames(path: &Path) -> u32 {
    hound::WavReader::open(path).unwrap().duration()
}

#[test]
fn play_records_the_armed_track_until_record_stop_and_saves_the_take() {
    let s = Song::new();
    let r = s.play("record.start --at 2.1.0\nrecord.status\nwait 3.1.0\nrecord.stop\n");
    let (out, err) = (text(&r.stdout), text(&r.stderr));
    assert!(r.status.success(), "{out}{err}");
    assert!(out.contains("録音を開始しました"), "{out}");
    assert!(out.contains("録音中"), "{out}");
    assert!(out.contains("録音を停止しました"), "{out}");
    let clips = s.clips();
    assert_eq!(clips.len(), 1, "{clips:?}");
    assert_eq!(clips[0]["start"], 3840, "2.1.0");
    assert!(clips[0]["length"].as_i64().unwrap() >= 3840, "{clips:?}");
    let file = s.dir.join("Media/Recordings/Vox-1.wav");
    assert!(frames(&file) >= 96_000, "a bar at least");

    // A recording still running when the input ends is placed and saved too; the song's end
    // does not stop it.
    let r = s.play("record.start --at 1.1.0\nwait 2.1.0\n");
    let (out, err) = (text(&r.stdout), text(&r.stderr));
    assert!(r.status.success(), "{out}{err}");
    assert_eq!(s.clips().len(), 2, "{out}{err}");
    assert!(s.dir.join("Media/Recordings/Vox-2.wav").exists());

    // `wait` without a position would wait for ever while recording.
    let r = s.play("record.start\nwait\nrecord.stop\n");
    assert_eq!(r.status.code(), Some(2), "{}", text(&r.stderr));
    assert!(text(&r.stderr).contains("録音中"), "{}", text(&r.stderr));
}

#[test]
fn record_commands_outside_play_say_where_to_use_them() {
    let s = Song::new();
    let out = s.cmd(&["record.start"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("xirius play"),
        "{}",
        text(&out.stderr)
    );
    let list = s.cmd(&["ops", "list", "--json"]).output().unwrap();
    let list = text(&list.stdout);
    for name in [
        "record.start",
        "record.stop",
        "record.status",
        "record.take.add",
    ] {
        assert!(list.contains(name), "{name} missing from ops list");
    }
}

#[test]
fn mcp_records_on_its_silent_player_and_one_undo_takes_it_out() {
    let s = Song::new();
    let mut child = s
        .cmd(&["mcp", "--no-device", "--autosave-sec", "0"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut replies = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut next = 0;
    let mut request = |method: &str, params: Value| -> Value {
        if method.starts_with("notifications/") {
            let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
            writeln!(input, "{msg}").unwrap();
            input.flush().unwrap();
            return Value::Null;
        }
        next += 1;
        let msg = json!({"jsonrpc": "2.0", "id": next, "method": method, "params": params});
        writeln!(input, "{msg}").unwrap();
        input.flush().unwrap();
        let r: Value = loop {
            let value: Value = serde_json::from_str(&replies.next().unwrap().unwrap()).unwrap();
            if value.get("id").is_some() {
                break value;
            }
        };
        assert_eq!(r["id"], next, "{r}");
        r
    };
    request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {},
               "clientInfo": {"name": "record-test", "version": "0"}}),
    );
    request("notifications/initialized", json!({}));
    let list = request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    for name in ["record_start", "record_stop", "record_status"] {
        assert!(tools.iter().any(|t| t["name"] == name), "{name}");
    }
    let mut call = |name: &str, args: Value| -> Value {
        let r = request("tools/call", json!({"name": name, "arguments": args}));
        assert_eq!(r["result"]["isError"], false, "{r}");
        r["result"]["structuredContent"].clone()
    };
    let started = call("record_start", json!({"at": "1.1.0"}));
    assert_eq!(started["data"]["recording"], true, "{started}");
    // The silent output does not make up for time its thread did not get (a busy machine), so
    // the wait is for the frames it played, not for a time.
    let begun = Instant::now();
    loop {
        let st = call("transport_status", json!({}));
        if st["data"]["frame"].as_u64().unwrap_or(0) >= 9_600 {
            break;
        }
        assert!(begun.elapsed() < Duration::from_secs(20), "{st}");
        std::thread::sleep(Duration::from_millis(10));
    }
    let stopped = call("record_stop", json!({}));
    assert_eq!(
        stopped["data"]["clips"].as_array().map(Vec::len),
        Some(1),
        "{stopped}"
    );
    let file = s.dir.join("Media/Recordings/Vox-1.wav");
    assert!(frames(&file) > 4_800, "silence while it ran");
    call("history_undo", json!({}));
    call("project_save", json!({}));
    assert!(s.clips().is_empty(), "one undo takes the recording out");
    assert!(file.exists(), "and leaves the file");
    drop(input);
    let _ = child.wait();
}

#[test]
fn play_records_audio_tracks_and_midi_from_a_script_at_once() {
    let s = Song::new();
    s.arm("Gtr", Some("In 1"));
    s.arm("Keys", None);
    let script = s.midi_script();
    let r = s.play_with(
        &["--midi-script", script.to_str().unwrap()],
        "record.start --at 2.1.0\nwait 3.1.0\nrecord.stop\n",
    );
    let (out, err) = (text(&r.stdout), text(&r.stderr));
    assert!(r.status.success(), "{out}{err}");
    assert!(err.contains("MIDI 入力: "), "{err}");
    assert!(
        out.contains("Vox") && out.contains("Gtr") && out.contains("Keys"),
        "{out}"
    );
    let (vox, gtr, keys) = (s.clips(), s.clips_of("Gtr"), s.clips_of("Keys"));
    assert_eq!((vox.len(), gtr.len(), keys.len()), (1, 1, 1), "{out}");
    assert_eq!(vox[0]["start"], 3840);
    assert_eq!(gtr[0]["start"], 3840);
    assert_eq!(
        gtr[0]["length"], vox[0]["length"],
        "recorded on the same frames"
    );
    assert_eq!(keys[0]["start"], 3840);
    let notes = keys[0]["notes"].as_array().expect("a MIDI clip");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0]["pitch"], 60);
    assert_eq!(notes[0]["length"], 960, "24 000 frames at 25 a tick");
    let read = |p: &str| {
        hound::WavReader::open(s.dir.join(p))
            .unwrap()
            .spec()
            .channels
    };
    assert_eq!(
        read("Media/Recordings/Vox-1.wav"),
        2,
        "no input named: stereo"
    );
    assert_eq!(read("Media/Recordings/Gtr-1.wav"), 1, "In 1: mono");

    // A MIDI option without its value, a script that is not one.
    let r = s.play_with(&["--midi-in"], "");
    assert_eq!(r.status.code(), Some(2), "{}", text(&r.stderr));
    std::fs::write(&script, "{}").unwrap();
    let r = s.play_with(&["--midi-script", script.to_str().unwrap()], "");
    assert_ne!(r.status.code(), Some(0));
    assert!(
        text(&r.stderr).contains("--midi-script"),
        "{}",
        text(&r.stderr)
    );
}

#[test]
fn mcp_records_midi_from_a_script() {
    let s = Song::new();
    s.arm("Keys", None);
    let script = s.midi_script();
    let mut child = s
        .cmd(&[
            "mcp",
            "--no-device",
            "--autosave-sec",
            "0",
            "--midi-script",
            script.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut replies = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut next = 0;
    let mut request = |method: &str, params: Value| -> Value {
        next += 1;
        let msg = json!({"jsonrpc": "2.0", "id": next, "method": method, "params": params});
        writeln!(input, "{msg}").unwrap();
        input.flush().unwrap();
        loop {
            let value: Value = serde_json::from_str(&replies.next().unwrap().unwrap()).unwrap();
            if value.get("id").is_some() {
                break value;
            }
        }
    };
    request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {},
               "clientInfo": {"name": "record-test", "version": "0"}}),
    );
    let mut call = |name: &str, args: Value| -> Value {
        let r = request("tools/call", json!({"name": name, "arguments": args}));
        assert_eq!(r["result"]["isError"], false, "{r}");
        r["result"]["structuredContent"].clone()
    };
    let started = call("record_start", json!({"at": "1.1.0"}));
    assert_eq!(
        started["data"]["midi_tracks"].as_array().map(Vec::len),
        Some(1)
    );
    // The note off comes 26 400 frames in (0.55 s). The silent output does not make up for
    // time its thread did not get, so the wait is for the player to read past it, not a time.
    let begun = Instant::now();
    loop {
        let st = call("transport_status", json!({}));
        if st["data"]["frame"].as_u64().unwrap_or(0) > 26_400 {
            break;
        }
        assert!(begun.elapsed() < Duration::from_secs(20), "{st}");
        std::thread::sleep(Duration::from_millis(10));
    }
    call("record_stop", json!({}));
    call("project_save", json!({}));
    let keys = s.clips_of("Keys");
    assert_eq!(keys.len(), 1, "{keys:?}");
    let notes = keys[0]["notes"].as_array().expect("a MIDI clip");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(
        (notes[0]["start"].as_i64(), notes[0]["length"].as_i64()),
        (Some(96), Some(960))
    );
    drop(input);
    let _ = child.wait();
}

#[test]
fn play_puts_the_recordings_nothing_uses_in_the_trash_on_quitting() {
    let s = Song::new();
    let bin = s.dir.join("bin");
    let quit = |lines: &str| {
        let mut child = s
            .cmd(&["play", "--no-device", "--unpaced"])
            .env("XIRIUS_TRASH_DIR", &bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run xirius play");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(lines.as_bytes())
            .unwrap();
        let r = child.wait_with_output().unwrap();
        assert!(r.status.success(), "{}{}", text(&r.stdout), text(&r.stderr));
        text(&r.stderr)
    };
    let take = "record.start --at 1.1.0\nwait 2.1.0\nrecord.stop\n";
    let rec = s.dir.join("Media/Recordings");

    // The undone take stays: a redo can bring it back. A file no state names goes.
    std::fs::create_dir_all(&rec).unwrap();
    std::fs::write(rec.join("stray.wav"), b"RIFF").unwrap();
    let err = quit(&format!("{take}{take}history.undo\n"));
    assert!(err.contains("ごみ箱に移動しました"), "{err}");
    assert!(bin.join("stray.wav").exists() && !rec.join("stray.wav").exists());
    assert!(rec.join("Vox-1.wav").exists() && rec.join("Vox-2.wav").exists());
    assert_eq!(s.clips().len(), 1);

    // The dry run of the command lists what would go; nothing does yet.
    let out = s
        .cmd(&["project.cleanup", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        !text(&out.stdout).contains("Vox-2.wav"),
        "{}",
        text(&out.stdout)
    );

    // Once a new edit drops the redo, the take is no longer needed.
    let err = quit("track.add.audio --name Gtr\n");
    assert!(err.contains("1 個"), "{err}");
    assert!(bin.join("Vox-2.wav").exists() && !rec.join("Vox-2.wav").exists());
    assert!(rec.join("Vox-1.wav").exists(), "the clip still uses it");
}

/// A script of two notes: the key 60 and the pad 61.
fn pad_script(s: &Song) -> PathBuf {
    let path = s.dir.join("pad.json");
    let msgs = json!([
        [2400, [0x90, 60, 100]],
        [4800, [0x90, 61, 100]],
        [10000, [0x80, 61, 0]],
        [26400, [0x80, 60, 0]]
    ]);
    std::fs::write(&path, msgs.to_string()).unwrap();
    path
}

/// `xirius play` into a MIDI track with the settings in `config`: the Keys clip.
fn play_pad(s: &Song, config: &Path) -> Value {
    let script = pad_script(s);
    let mut child = s
        .cmd(&[
            "play",
            "--no-device",
            "--unpaced",
            "--midi-script",
            script.to_str().unwrap(),
        ])
        .env("XIRIUS_CONFIG_DIR", config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run xirius play");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            b"record.start --at 2.1.0
wait 3.1.0
record.stop
",
        )
        .unwrap();
    let r = child.wait_with_output().unwrap();
    assert!(r.status.success(), "{}{}", text(&r.stdout), text(&r.stderr));
    let clips = s.clips_of("Keys");
    assert_eq!(clips.len(), 1);
    clips[0].clone()
}

#[test]
fn an_assigned_pad_is_left_out_of_the_recorded_clip() {
    let s = Song::new();
    s.arm("Keys", None);
    let config = s.dir.join("config");
    std::fs::create_dir_all(&config).unwrap();
    let pitches = |clip: &Value| -> Vec<i64> {
        let mut p: Vec<i64> = clip["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["pitch"].as_i64().unwrap())
            .collect();
        p.sort();
        p
    };
    assert_eq!(pitches(&play_pad(&s, &config)), [60, 61]);
    // The pad given to a command: the recording does not take it.
    let r = Command::new(env!("CARGO_BIN_EXE_xirius"))
        .env("XIRIUS_CONFIG_DIR", &config)
        .env("XIRIUS_PLUGIN_CACHE", s.dir.join("plugins.json"))
        .args([
            "midimap.set",
            "--message",
            "note:61",
            "--target",
            "transport.play",
        ])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}{}", text(&r.stdout), text(&r.stderr));
    let s2 = Song::new();
    s2.arm("Keys", None);
    assert_eq!(pitches(&play_pad(&s2, &config)), [60]);
}
