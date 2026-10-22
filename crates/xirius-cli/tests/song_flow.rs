//! One song made twice, through the `xirius` program (one process per command) and through
//! its MCP server over stdio (ARC-041): tempo and meter, an instrument track, notes drawn,
//! moved and quantized, drum hits, an audio file, split, loop and fade, the mixer, automation,
//! a marker, bounce and freeze, save and reopen, the exports, and every step undone and redone.
//! Each same-named command must answer the same on both sides. No device is opened: the server
//! runs with `--no-device`, and the program never plays.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{Map, Value, json};

/// A song folder with the AI allowed to save, and a tone to import.
fn song(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    let mut s = xirius_core::Session::create(dir, xirius_core::Project::new("Flow")).unwrap();
    s.run(
        "ai.permission.set",
        json!({"level": "save"}).as_object().unwrap(),
        &xirius_core::session::RunOptions::human(),
    )
    .unwrap();
    s.save().unwrap();
    drop(s);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    std::fs::create_dir_all(dir.join("Samples")).unwrap();
    let mut w = hound::WavWriter::create(dir.join("Samples").join("tone.wav"), spec).unwrap();
    // Two seconds of a quiet 440 Hz tone: a little over a bar at 128 BPM.
    for i in 0..96_000 {
        let x = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.25;
        w.write_sample((x * 32767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

trait Endpoint {
    fn dir(&self) -> &Path;
    /// The whole answer (`ok`, `data`, `changes`…), or the error's answer.
    fn run(&mut self, name: &str, args: Value) -> Result<Value, Value>;
    /// Closes the song and opens it again.
    fn reopen(&mut self);
}

struct Cli(PathBuf);

impl Endpoint for Cli {
    fn dir(&self) -> &Path {
        &self.0
    }

    fn run(&mut self, name: &str, args: Value) -> Result<Value, Value> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xirius"));
        command
            .env_remove("XIRIUS_SONG")
            .env("XIRIUS_PLUGIN_CACHE", self.0.join("plugins.json"))
            .env("XIRIUS_CONFIG_DIR", self.0.join("config"))
            .current_dir(std::env::temp_dir())
            .arg("--song")
            .arg(&self.0)
            .arg(name)
            .arg("--json");
        for (key, value) in args.as_object().unwrap() {
            command.arg(format!("--{key}"));
            command.arg(match value {
                Value::String(text) => text.clone(),
                Value::Bool(true) => "true".into(),
                Value::Array(values) if values.iter().all(Value::is_string) => values
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(","),
                other => other.to_string(),
            });
        }
        let output = command.output().unwrap();
        let answer: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            json!({"stdout": String::from_utf8_lossy(&output.stdout),
                   "stderr": String::from_utf8_lossy(&output.stderr)})
        });
        if output.status.success() {
            Ok(answer)
        } else {
            Err(answer)
        }
    }

    fn reopen(&mut self) {}
}

struct Mcp {
    dir: PathBuf,
    child: Option<Child>,
    input: Option<ChildStdin>,
    replies: Option<Receiver<Value>>,
    next: u64,
    events: Vec<Value>,
}

impl Mcp {
    fn new(dir: PathBuf) -> Self {
        let mut mcp = Self {
            dir,
            child: None,
            input: None,
            replies: None,
            next: 1,
            events: Vec::new(),
        };
        mcp.start();
        mcp
    }

    fn start(&mut self) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_xirius"))
            .args(["mcp", "--no-device", "--autosave-sec", "0", "--song"])
            .arg(&self.dir)
            .env_remove("XIRIUS_SONG")
            .env("XIRIUS_PLUGIN_CACHE", self.dir.join("plugins.json"))
            .env("XIRIUS_CONFIG_DIR", self.dir.join("config"))
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        self.input = child.stdin.take();
        let output = child.stdout.take().unwrap();
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                let Ok(value) = serde_json::from_str(&line) else {
                    break;
                };
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        self.child = Some(child);
        self.replies = Some(replies);
        self.events.clear();
        let r = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "song-flow", "version": "0"}
            }),
        );
        assert_eq!(r["result"]["serverInfo"]["name"], "xirius");
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    }

    fn stop(&mut self) {
        self.input.take();
        if let Some(mut child) = self.child.take() {
            let start = std::time::Instant::now();
            while child.try_wait().ok().flatten().is_none() {
                if start.elapsed() > Duration::from_secs(10) {
                    let _ = child.kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = child.wait();
        }
    }

    fn send(&mut self, v: Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{v}").unwrap();
        input.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        self.replies
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_secs(60))
            .expect("MCP reply")
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let r = self.recv();
            if r.get("id").is_none() {
                self.events.push(r);
                continue;
            }
            assert_eq!(r["id"], id, "{r}");
            assert!(r.get("error").is_none(), "{r}");
            return r;
        }
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Endpoint for Mcp {
    fn dir(&self) -> &Path {
        &self.dir
    }

    fn run(&mut self, name: &str, args: Value) -> Result<Value, Value> {
        let r = self.request(
            "tools/call",
            json!({"name": name.replace('.', "_"), "arguments": args}),
        );
        let mut result = r["result"]["structuredContent"].clone();
        if r["result"]["isError"] == true {
            return Err(result);
        }
        // A long command answers with a receipt; its result comes as an event when it is
        // done. The server runs every command that has `background` in the background.
        let receipt = result["receipt"]["id"].clone();
        if result["receipt"]["state"] != "completed" && !receipt.is_null() {
            loop {
                let found = self.events.iter().position(|event| {
                    event["params"]["receipt"]["id"] == receipt
                        && event["params"]["receipt"]["state"] == "completed"
                        && event["params"]["result"].is_object()
                });
                if let Some(i) = found {
                    result = self.events.remove(i)["params"]["result"].clone();
                    break;
                }
                let event = self.recv();
                assert!(event.get("id").is_none(), "{event}");
                self.events.push(event);
            }
            if result["ok"] == false {
                return Err(result);
            }
        }
        if let Some(id) = result["result_id"].as_str().map(str::to_owned) {
            let mut text = String::new();
            for page in 0..result["pages"].as_u64().unwrap() {
                let part = self.request(
                    "tools/call",
                    json!({"name": "ops_get-result", "arguments": {"id": id, "page": page}}),
                );
                text.push_str(
                    part["result"]["structuredContent"]["text"]
                        .as_str()
                        .unwrap(),
                );
            }
            result = serde_json::from_str(&text).unwrap();
        }
        Ok(result)
    }

    fn reopen(&mut self) {
        self.stop();
        self.start();
    }
}

/// Keys that say how or when a command ran, not what it did.
const HOW: [&str; 6] = [
    "receipt",
    "elapsed_ms",
    "duration_ms",
    "result_id",
    "pages",
    "state_hash",
];

/// The answer without what differs by the way it was asked: the song folder's path and the
/// keys in [`HOW`].
fn plain(value: &Value, dir: &Path) -> Value {
    let text = value.to_string();
    let back = dir.to_string_lossy().to_string();
    let forward = back.replace('\\', "/");
    let escaped = back.replace('\\', "\\\\");
    let text = text
        .replace(&escaped, "<song>")
        .replace(&forward, "<song>")
        .replace(&back, "<song>");
    let mut value: Value = serde_json::from_str(&text).unwrap();
    strip(&mut value);
    value
}

fn strip(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in HOW {
                map.remove(key);
            }
            map.values_mut().for_each(strip);
        }
        Value::Array(items) => items.iter_mut().for_each(strip),
        _ => {}
    }
}

/// What one side answered at each step, by step name.
type Log = Vec<(String, Result<Value, Value>)>;

fn step(endpoint: &mut dyn Endpoint, log: &mut Log, label: &str, name: &str, args: Value) -> Value {
    let answer = endpoint.run(name, args);
    let dir = endpoint.dir().to_path_buf();
    let kept = match &answer {
        Ok(v) => Ok(plain(v, &dir)),
        Err(v) => Err(plain(v, &dir)),
    };
    log.push((label.to_string(), kept));
    answer.unwrap_or_else(|e| panic!("{label} ({name}): {e}"))
}

fn id(v: &Value) -> String {
    v["data"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no id: {v}"))
        .to_string()
}

fn project(endpoint: &mut dyn Endpoint) -> Value {
    let dir = endpoint.dir().to_path_buf();
    plain(
        &endpoint.run("project.get", json!({})).unwrap()["data"],
        &dir,
    )
}

/// What the flow leaves to check, besides the answers.
struct Made {
    log: Log,
    empty: Value,
    saved: Value,
    undone: Value,
    redone: Value,
    wav: PathBuf,
    mp3: Result<Value, Value>,
    dawproject: PathBuf,
}

fn flow(e: &mut dyn Endpoint) -> Made {
    let mut log = Log::new();
    let empty = project(e);
    let l = &mut log;
    let mut steps = 0;
    let mut edit = |e: &mut dyn Endpoint, l: &mut Log, label: &str, name: &str, args: Value| {
        steps += 1;
        step(e, l, label, name, args)
    };

    // 1. Tempo and meter, an instrument track.
    edit(e, l, "tempo", "tempo.set", json!({"bpm": 128}));
    edit(
        e,
        l,
        "meter",
        "timesig.set",
        json!({"num": 7, "den": 8, "bar": 9}),
    );
    let keys = id(&edit(
        e,
        l,
        "instrument track",
        "track.add.with-instrument",
        json!({"instrument": "Sampler", "name": "Keys"}),
    ));

    // 2. Notes drawn, moved and quantized; drum hits; an audio file.
    let clip = id(&edit(
        e,
        l,
        "midi clip",
        "clip.add.midi",
        json!({"track": keys, "at": 0, "bars": 2}),
    ));
    edit(
        e,
        l,
        "notes",
        "note.put",
        json!({"clip": clip, "notes": [
            {"at": 10, "pitch": 60, "length": 240, "velocity": 90},
            {"at": 490, "pitch": 64, "length": 240, "velocity": 90},
            {"at": 970, "pitch": 67, "length": 480, "velocity": 90}
        ]}),
    );
    let notes = step(e, l, "note list", "note.list", json!({"clip": clip}));
    let second = notes["data"][1]["id"].as_str().unwrap().to_string();
    edit(
        e,
        l,
        "note move",
        "note.move",
        json!({"ids": [second], "by": 480}),
    );
    edit(
        e,
        l,
        "quantize",
        "note.quantize",
        json!({"ids": [clip], "grid": "1/8"}),
    );
    step(e, l, "note list after", "note.list", json!({"clip": clip}));
    let drums = id(&edit(
        e,
        l,
        "drum track",
        "track.add.with-instrument",
        json!({"instrument": "Sampler", "name": "Drums"}),
    ));
    let beat = id(&edit(
        e,
        l,
        "drum clip",
        "clip.add.midi",
        json!({"track": drums, "at": 0, "bars": 1}),
    ));
    edit(
        e,
        l,
        "drum hits",
        "note.put",
        json!({"clip": beat, "notes": [
            {"at": 0, "pitch": 36, "length": 120, "velocity": 100},
            {"at": 480, "pitch": 37, "length": 120, "velocity": 100},
            {"at": 960, "pitch": 36, "length": 120, "velocity": 100},
            {"at": 1440, "pitch": 37, "length": 120, "velocity": 100}
        ]}),
    );
    let import = edit(
        e,
        l,
        "audio file",
        "clip.import",
        json!({"file": "Samples/tone.wav", "at": "2.1.0", "tempo": "keep"}),
    );
    let audio = import["data"]["clip"]
        .as_str()
        .or_else(|| import["data"]["clips"][0].as_str())
        .or_else(|| import["data"]["clips"][0]["id"].as_str())
        .unwrap_or_else(|| panic!("no clip: {import}"))
        .to_string();

    // 3. Split, loop and fade; the mixer; automation; a marker; bounce and freeze.
    edit(
        e,
        l,
        "split",
        "clip.split",
        json!({"ids": [audio], "at": "2.3.0"}),
    );
    edit(
        e,
        l,
        "loop",
        "clip.loop.set",
        json!({"ids": [clip], "start": 0, "length": 1920}),
    );
    edit(
        e,
        l,
        "fade",
        "clip.fade.set",
        json!({"ids": [audio], "in": 480, "out": 240}),
    );
    edit(
        e,
        l,
        "volume",
        "track.volume.set",
        json!({"ids": [keys], "db": -8.8}),
    );
    edit(
        e,
        l,
        "pan",
        "track.pan.set",
        json!({"ids": [keys], "pan": 0.4}),
    );
    edit(
        e,
        l,
        "insert",
        "plugin.add",
        json!({"track": keys, "name": "Reverb"}),
    );
    edit(
        e,
        l,
        "send",
        "send.add.bus",
        json!({"ids": [keys], "level": -6}),
    );
    for (at, value) in [("2.1.0", -12), ("3.1.0", -3)] {
        edit(
            e,
            l,
            "automation",
            "auto.point.add",
            json!({"track": keys, "target": "volume", "at": at, "value": value}),
        );
    }
    edit(
        e,
        l,
        "marker",
        "marker.add",
        json!({"at": "3.1.0", "name": "サビ"}),
    );
    edit(
        e,
        l,
        "bounce",
        "clip.bounce",
        json!({"ids": [clip], "new-track": true}),
    );
    edit(e, l, "freeze", "track.freeze", json!({"ids": [drums]}));

    // 4. Save, close and reopen.
    step(e, l, "save", "project.save", json!({}));
    let saved = project(e);
    e.reopen();
    assert_eq!(project(e), saved, "reopened");

    // 5. The exports.
    let dir = e.dir().to_path_buf();
    let wav = dir.join("exports").join("flow.wav");
    step(
        e,
        l,
        "wav",
        "export.mixdown",
        json!({"out": wav.to_string_lossy(), "background": false}),
    );
    let mp3 = e.run(
        "export.mixdown",
        json!({"out": dir.join("exports").join("flow.mp3").to_string_lossy(), "background": false}),
    );
    l.push((
        "mp3".into(),
        match &mp3 {
            Ok(v) => Ok(plain(v, &dir)),
            Err(v) => Err(plain(v, &dir)),
        },
    ));
    let dawproject = dir.join("exports").join("flow.dawproject");
    step(
        e,
        l,
        "dawproject",
        "export.dawproject",
        json!({"out": dawproject.to_string_lossy()}),
    );

    // 6. Every step undone, then redone: each edit above is one step.
    let undo = step(e, l, "undo", "history.undo", json!({"count": steps}));
    assert_eq!(undo["data"]["cursor"], 0, "{undo}");
    let undone = project(e);
    let redo = step(e, l, "redo", "history.redo", json!({"count": steps}));
    assert_eq!(redo["data"]["cursor"], steps, "{redo}");
    let redone = project(e);
    Made {
        log,
        empty,
        saved,
        undone,
        redone,
        wav,
        mp3,
        dawproject,
    }
}

/// Seconds and the loudest sample of a WAV.
fn wav_measure(path: &Path) -> (f64, f32) {
    let mut r = hound::WavReader::open(path).unwrap();
    let spec = r.spec();
    let seconds = r.duration() as f64 / spec.sample_rate as f64;
    let peak = match spec.sample_format {
        hound::SampleFormat::Float => r
            .samples::<f32>()
            .map(|s| s.unwrap().abs())
            .fold(0.0, f32::max),
        hound::SampleFormat::Int => {
            let full = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.unwrap().unsigned_abs() as f32 / full)
                .fold(0.0, f32::max)
        }
    };
    (seconds, peak)
}

fn differences(a: &Value, b: &Value, at: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            let none = Value::Null;
            for k in keys {
                differences(
                    x.get(k).unwrap_or(&none),
                    y.get(k).unwrap_or(&none),
                    &format!("{at}.{k}"),
                    out,
                );
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                differences(p, q, &format!("{at}[{i}]"), out);
            }
        }
        _ if a != b => out.push(format!("{at}: {a} | {b}")),
        _ => {}
    }
}

/// Takes out of the answers what differs because the server's caller is an AI: the notes it
/// writes are marked as the AI's, and its save keeps a copy of the file it overwrote.
fn by_design(label: &str, cli: &mut Value, mcp: &mut Value) {
    let notes = label.starts_with("note list");
    for note in mcp["ok"]["data"]
        .as_array_mut()
        .filter(|_| notes)
        .into_iter()
        .flatten()
    {
        assert_eq!(note["ai"], true, "{label}: {note}");
        note.as_object_mut().unwrap().remove("ai");
    }
    for note in cli["ok"]["data"]
        .as_array_mut()
        .filter(|_| notes)
        .into_iter()
        .flatten()
    {
        assert_eq!(note["ai"], false, "{label}: {note}");
        note.as_object_mut().unwrap().remove("ai");
    }
    // The server ran the export as a job: its answer is the job's state, with the file's
    // measures under `result` and the program's line at the end of the message.
    if mcp["ok"]["data"]["state"] == "done" && mcp["ok"]["data"]["result"].is_object() {
        let line = cli["ok"]["message"].as_str().unwrap().to_string();
        assert!(
            mcp["ok"]["message"].as_str().unwrap().ends_with(&line),
            "{label}: {mcp}"
        );
        let mut result = mcp["ok"]["data"]["result"].take();
        let cli = cli["ok"].as_object_mut().unwrap();
        cli.remove("message");
        let data = cli["data"].as_object_mut().unwrap();
        // The program's answer also names the export's row in the list of exports.
        data.remove("record");
        result
            .as_object_mut()
            .unwrap()
            .retain(|key, _| data.contains_key(key));
        mcp["ok"] = json!({"data": result});
        cli.retain(|key, _| key == "data");
    }
    // The export's row in the list of exports is named after the moment it ran, on either
    // side (a `background: false` export answers with the file's measures, not a job).
    for side in [&mut *cli, &mut *mcp] {
        if let Some(data) = side["ok"]["data"].as_object_mut() {
            data.remove("record");
        }
    }
    if label == "save" {
        assert!(mcp["ok"]["data"]["backup"].is_string(), "{mcp}");
        assert!(cli["ok"]["data"]["backup"].is_null(), "{cli}");
        for side in [cli, mcp] {
            let ok = side["ok"].as_object_mut().unwrap();
            ok.remove("message");
            let data = ok["data"].as_object_mut().unwrap();
            data.remove("backup");
            data.remove("file_hash");
        }
    }
}

#[test]
fn arc_041_one_song_through_the_program_and_the_mcp_server_answers_the_same() {
    let base = std::env::temp_dir().join(format!("xirius-song-flow-{}", std::process::id()));
    let (cli_dir, mcp_dir) = (base.join("cli"), base.join("mcp"));
    song(&cli_dir);
    song(&mcp_dir);
    let cli = flow(&mut Cli(cli_dir));
    let mcp = {
        let mut server = Mcp::new(mcp_dir);
        flow(&mut server)
    };

    let mut out = Vec::new();
    assert_eq!(cli.log.len(), mcp.log.len());
    for ((label, a), (_, b)) in cli.log.iter().zip(&mcp.log) {
        let wrap = |r: &Result<Value, Value>| match r {
            Ok(v) => json!({"ok": v}),
            Err(v) => json!({"error": v}),
        };
        let (mut a, mut b) = (wrap(a), wrap(b));
        by_design(label, &mut a, &mut b);
        differences(&a, &b, label, &mut out);
    }
    differences(&cli.saved, &mcp.saved, "saved", &mut out);
    differences(&cli.redone, &mcp.redone, "redone", &mut out);
    if let Ok(path) = std::env::var("XIRIUS_SONG_FLOW_LOG") {
        let log: Map<String, Value> = cli
            .log
            .iter()
            .zip(&mcp.log)
            .enumerate()
            .map(|(i, ((label, a), (_, b)))| {
                let side = |r: &Result<Value, Value>| match r {
                    Ok(v) => json!({"ok": v}),
                    Err(v) => json!({"error": v}),
                };
                (
                    format!("{i:02} {label}"),
                    json!({"cli": side(a), "mcp": side(b)}),
                )
            })
            .collect();
        std::fs::write(path, serde_json::to_string_pretty(&log).unwrap()).unwrap();
    }
    assert!(out.is_empty(), "{}", out.join("\n"));

    for made in [&cli, &mcp] {
        let mut changed = Vec::new();
        differences(&made.empty, &made.undone, "undone", &mut changed);
        assert!(changed.is_empty(), "{}", changed.join("\n"));
        assert_eq!(made.redone, made.saved);
        let (seconds, peak) = wav_measure(&made.wav);
        assert!(seconds > 4.0, "{seconds}");
        assert!(peak > 0.01, "{peak}");
        assert!(made.dawproject.is_file());
        if let Err(e) = &made.mp3 {
            eprintln!("skip-mp3: {e}");
        }
    }
    let _ = std::fs::remove_dir_all(&base);
}
