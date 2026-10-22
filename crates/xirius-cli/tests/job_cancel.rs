//! `job.cancel` (EXP-047): a bounce running in one `xirius` (the CLI or the MCP server) stops
//! from another process, and the song is left as it was.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use xirius_core::Project;
use xirius_core::session::{RunOptions, Session};

/// 400 bars at 120 BPM: long enough to be stopped while it renders.
const LONG: i64 = 3840 * 400;

/// A saved song with one audio clip on track 「A」; returns the track's id and the number of
/// tracks.
fn song(root: &Path) -> (PathBuf, String, usize) {
    let wav = root.join("a.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(&wav, spec).unwrap();
    for i in 0..96_000 {
        let x = 0.2 * (i as f32 * 0.03).sin();
        w.write_sample(x).unwrap();
        w.write_sample(x).unwrap();
    }
    w.finalize().unwrap();
    let dir = root.join("song");
    let mut s = Session::create(&dir, Project::new("Cancel")).unwrap();
    let mut run = |name: &str, args: Value| {
        s.run(name, args.as_object().unwrap(), &RunOptions::human())
            .unwrap_or_else(|e| panic!("{name}: {e:?}"))
    };
    run("ai.permission.set", json!({"level": "save"}));
    let t = run("track.add.audio", json!({"name": "A"})).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 3840, "file": wav.to_string_lossy()}),
    );
    s.save().unwrap();
    let n = s.project().tracks.len();
    (dir, t, n)
}

fn xirius(root: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_xirius"));
    c.env_remove("XIRIUS_SONG")
        .env("XIRIUS_PLUGIN_CACHE", root.join("plugins.json"))
        .env("XIRIUS_CONFIG_DIR", root.join("config"))
        .current_dir(root);
    c
}

/// `job.list` or `job.cancel` from a `xirius` of its own, without a song.
fn job(root: &Path, args: &[&str]) -> Value {
    let out = xirius(root).args(args).arg("--json").output().unwrap();
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&out.stderr)))
}

/// Waits for the bounce on the song under `root` to show in `job.list`; returns its id.
fn running_on(root: &Path) -> String {
    let tag = root.file_name().unwrap().to_string_lossy().to_string();
    let start = Instant::now();
    loop {
        let v = job(root, &["job.list"]);
        let found = v["data"]["jobs"].as_array().and_then(|jobs| {
            jobs.iter()
                .find(|j| j["song"].as_str().is_some_and(|s| s.contains(&tag)))
                .cloned()
        });
        if let Some(j) = found {
            assert_eq!(j["command"], "range.bounce", "{j}");
            return j["id"].as_str().unwrap().to_string();
        }
        assert!(start.elapsed() < Duration::from_secs(60), "{v}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn left_alone(dir: &Path, tracks: usize) {
    let s = Session::open(dir).unwrap();
    assert_eq!(s.project().tracks.len(), tracks);
    let bounces = dir.join("Media/Bounces");
    let n = std::fs::read_dir(&bounces).map_or(0, |r| r.count());
    assert_eq!(n, 0, "nothing written stays");
}

#[test]
fn job_cancel_stops_a_bounce_the_cli_runs() {
    let root = tempfile::tempdir().unwrap();
    let (dir, t, n) = song(root.path());
    let to = LONG.to_string();
    let child = xirius(root.path())
        .arg("--song")
        .arg(&dir)
        .args(["range.bounce", "--from", "0", "--to", &to, "--tail", "0"])
        .args(["--tracks", &t, "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let id = running_on(root.path());

    let v = job(root.path(), &["job.cancel", "--id", &id]);
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["message"], "処理 1 件をキャンセルしました");
    assert_eq!(v["data"]["jobs"][0]["id"], id.as_str());

    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    let r: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["error"]["message"], "バウンスをキャンセルしました", "{r}");
    left_alone(&dir, n);
    // Gone from the list, so there is nothing more to stop.
    let v = job(root.path(), &["job.cancel", "--id", &id]);
    assert_eq!(v["ok"], false, "{v}");
}

struct Mcp {
    child: Child,
    input: Option<ChildStdin>,
    replies: Receiver<Value>,
    next: u64,
    events: Vec<Value>,
}

impl Mcp {
    fn start(root: &Path, dir: &Path) -> Mcp {
        let mut child = xirius(root)
            .args(["mcp", "--no-device", "--autosave-sec", "0", "--song"])
            .arg(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
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
        let mut m = Mcp {
            child,
            input,
            replies,
            next: 1,
            events: Vec::new(),
        };
        m.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "job-cancel-test", "version": "0"}
            }),
        );
        m.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        m
    }

    fn send(&mut self, v: Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{v}").unwrap();
        input.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let r = self
                .replies
                .recv_timeout(Duration::from_secs(30))
                .expect("MCP reply");
            if r.get("id").is_none() {
                self.events.push(r);
                continue;
            }
            assert_eq!(r["id"], id, "{r}");
            assert!(r.get("error").is_none(), "{r}");
            return r;
        }
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
        let r = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        r["result"]["structuredContent"].clone()
    }

    /// The result the receipt `id` ends with.
    fn finished(&mut self, id: &str) -> Value {
        let start = Instant::now();
        loop {
            if let Some(e) = self.events.iter().find(|e| {
                e["params"]["receipt"]["id"] == id && e["params"]["receipt"]["state"] == "completed"
            }) {
                return e["params"]["result"].clone();
            }
            assert!(
                start.elapsed() < Duration::from_secs(60),
                "{:?}",
                self.events
            );
            if let Ok(e) = self.replies.recv_timeout(Duration::from_secs(1)) {
                self.events.push(e);
            }
        }
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        self.input.take();
        let start = Instant::now();
        while self.child.try_wait().ok().flatten().is_none() {
            if start.elapsed() > Duration::from_secs(5) {
                let _ = self.child.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.wait();
    }
}

#[test]
fn job_cancel_stops_a_bounce_the_mcp_server_runs_while_it_holds_the_song() {
    let root = tempfile::tempdir().unwrap();
    let (dir, t, n) = song(root.path());
    let mut m = Mcp::start(root.path(), &dir);
    let r = m.call(
        "range_bounce",
        json!({"from": 0, "to": LONG, "tracks": [t], "tail": 0, "wait_ms": 0, "request_key": "long"}),
    );
    assert_eq!(r["receipt"]["id"], "long", "{r}");

    // The same server answers while the bounce holds the song.
    let tag = root
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let start = Instant::now();
    let id = loop {
        let v = m.call("job_list", json!({}));
        let found = v["data"]["jobs"].as_array().and_then(|jobs| {
            jobs.iter()
                .find(|j| j["song"].as_str().is_some_and(|s| s.contains(&tag)))
                .map(|j| j["id"].as_str().unwrap().to_string())
        });
        if let Some(id) = found {
            break id;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "{v}");
        std::thread::sleep(Duration::from_millis(20));
    };
    let v = m.call("job_cancel", json!({"id": id}));
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["message"], "処理 1 件をキャンセルしました");

    let done = m.finished("long");
    assert_eq!(done["ok"], false, "{done}");
    assert_eq!(done["error"]["message"], "バウンスをキャンセルしました");
    drop(m);
    left_alone(&dir, n);
}
