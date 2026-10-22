//! The CLI's MCP entry point with session commands, over the real stdio transport.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    replies: Receiver<Value>,
    dir: PathBuf,
    next: u64,
    events: Vec<Value>,
}

impl Client {
    fn start(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("xirius-mcp-session-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = xirius_core::Session::create(&dir, xirius_core::Project::new("MCP")).unwrap();
        s.run(
            "ai.permission.set",
            json!({"level": "save"}).as_object().unwrap(),
            &xirius_core::session::RunOptions::human(),
        )
        .unwrap();
        s.save().unwrap();
        drop(s);

        let mut child = Command::new(env!("CARGO_BIN_EXE_xirius"))
            .args(["mcp", "--no-device", "--autosave-sec", "0", "--song"])
            .arg(&dir)
            .env("XIRIUS_PLUGIN_CACHE", dir.join("plugins.json"))
            .env("XIRIUS_CONFIG_DIR", dir.join("config"))
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
        let mut client = Self {
            child,
            input,
            replies,
            dir,
            next: 1,
            events: Vec::new(),
        };
        let r = client.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "session-test", "version": "0"}
            }),
        );
        assert_eq!(r["result"]["serverInfo"]["name"], "xirius");
        client.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        client
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

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let r = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        assert_eq!(r["result"]["isError"], false, "{r}");
        let mut result = r["result"]["structuredContent"].clone();
        let receipt = result["receipt"]["id"].clone();
        if result["data"].is_null()
            && !receipt.is_null()
            && let Some(event) = self.events.iter().find(|event| {
                event["params"]["receipt"]["id"] == receipt
                    && !event["params"]["result"]["data"].is_null()
            })
        {
            result = event["params"]["result"].clone();
        }
        while result["data"].is_null() && !receipt.is_null() {
            let event = self
                .replies
                .recv_timeout(Duration::from_secs(20))
                .expect("operation result event");
            assert!(event.get("id").is_none(), "{event}");
            if event["params"]["receipt"]["id"] == receipt && event["params"]["result"].is_object()
            {
                result = event["params"]["result"].clone();
                assert_ne!(result["ok"], false, "{result}");
            }
            self.events.push(event);
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
        assert_ne!(result["confirmation_required"], true, "{result}");
        result["data"].clone()
    }

    fn poll(&mut self, tool: &str, args: Value, ready: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let v = self.call(tool, args.clone());
            if ready(&v) {
                return v;
            }
            assert!(start.elapsed() < Duration::from_secs(20), "{v}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Client {
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
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn export_completion_arrives_without_status_polling() {
    let mut client = Client::start("export-event");
    let track = client.call("track_add_instrument", json!({"name": "Keys"}))["id"].clone();
    client.call("clip_add_midi", json!({"track": track, "at": 0, "bars": 1}));
    let output = client.dir.join("event.wav");
    let reply = client.request("tools/call", json!({"name": "export_mixdown", "arguments": {"out": output, "request_key": "export-event"}}));
    let receipt = &reply["result"]["structuredContent"]["receipt"];
    assert_eq!(receipt["id"], "export-event");
    assert!(matches!(
        receipt["state"].as_str(),
        Some("accepted" | "running" | "completed")
    ));
    let completed = |event: &Value| {
        event["params"]["receipt"]["id"] == "export-event"
            && event["params"]["receipt"]["state"] == "completed"
    };
    while !client.events.iter().any(completed) {
        let event = client
            .replies
            .recv_timeout(Duration::from_secs(20))
            .expect("export completion event");
        assert!(event.get("id").is_none(), "{event}");
        client.events.push(event);
    }
    let event = client.events.iter().find(|event| completed(event)).unwrap();
    assert_eq!(event["params"]["result"]["data"]["state"], "done");
    assert_eq!(event["params"]["result"]["command"], "export.mixdown");
    let hash = &event["params"]["result"]["data"]["state_hash"];
    assert!(hash["before"].is_string());
    assert_eq!(hash["before"], hash["after"]);
    assert!(output.is_file());
}

#[test]
fn mcp_exports_polls_cancels_and_controls_silent_playback() {
    let mut c = Client::start("main");
    let list = c.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    for name in xirius_session::commands::NAMES {
        assert!(
            tools.iter().any(|t| t["name"] == name.replace('.', "_")),
            "{name}"
        );
    }
    let mix = tools
        .iter()
        .find(|t| t["name"] == "export_mixdown")
        .unwrap();
    assert_eq!(
        mix["inputSchema"]["properties"]["background"]["default"],
        true
    );
    let t = c.call("track_add_instrument", json!({"name": "Keys"}))["id"].clone();
    let clip = c.call("clip_add_midi", json!({"track": t, "at": 0, "bars": 2}))["id"].clone();
    c.call(
        "note_add",
        json!({"clip": clip, "at": 0, "pitch": 60, "length": 3840}),
    );
    c.call("loop_set", json!({"start": 0, "end": 3840}));

    let out = c.dir.join("mix.wav");
    let dry = c.request(
        "tools/call",
        json!({"name": "export_mixdown", "arguments": {
            "out": out, "dry_run": true
        }}),
    );
    assert_eq!(dry["result"]["structuredContent"]["dry_run"], true);
    assert!(!out.exists());
    let job = c.call("export_mixdown", json!({"out": out}));
    assert_eq!(job["state"], "running");
    let done = c.poll("export_status", json!({"job": job["job"]}), |v| {
        v["state"] != "running"
    });
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["fraction"], 1.0);
    assert_eq!(done["total"], 4 * 48000);
    let mut wav = hound::WavReader::open(&out).unwrap();
    assert!(wav.samples::<i32>().any(|s| s.unwrap().abs() > 100));
    drop(wav);

    let stems = c.dir.join("stems");
    let job = c.call("export_stems", json!({"dir": stems}));
    assert_eq!(
        c.poll("export_status", json!({"job": job["job"]}), |v| v["state"]
            != "running")["state"],
        "done"
    );
    let names: Vec<String> = std::fs::read_dir(&stems)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().any(|n| n.ends_with("_Keys.wav")), "{names:?}");

    let cancelled = c.dir.join("cancelled.wav");
    let job = c.call("export_mixdown", json!({"out": cancelled, "tail": 600}));
    c.call("export_cancel", json!({"job": job["job"]}));
    let done = c.poll("export_status", json!({"job": job["job"]}), |v| {
        v["state"] != "running"
    });
    assert_eq!(done["state"], "cancelled", "{done}");
    assert!(!cancelled.exists());

    c.call("transport_play", json!({}));
    let playing = c.poll("transport_status", json!({}), |v| {
        v["playing"] == true && v["frame"].as_u64().unwrap() > 0
    });
    assert_eq!(playing["output"], "無音");
    c.call("transport_stop", json!({}));
    c.poll("transport_status", json!({}), |v| v["playing"] == false);
    c.call("transport_seek", json!({"to": "2.1.0"}));
    c.poll("transport_status", json!({}), |v| v["frame"] == 96000);
    c.call("transport_loop", json!({"on": true}));
    c.poll("transport_status", json!({}), |v| v["looping"] == true);

    // A new CLI process reads the cache without opening installed plugins.
    let cache = json!({"version": 2, "bundles": [{
        "path": "missing.vst3", "fingerprint": "test", "entries": [],
        "failure": {"path": "missing.vst3", "kind": "failed", "error": "unavailable"}
    }]});
    std::fs::write(c.dir.join("plugins.json"), cache.to_string()).unwrap();
    let cli = Command::new(env!("CARGO_BIN_EXE_xirius"))
        .args(["browser.plugin.list", "--failed", "--json"])
        .env_remove("XIRIUS_SONG")
        .env("XIRIUS_PLUGIN_CACHE", c.dir.join("plugins.json"))
        .env("XIRIUS_CONFIG_DIR", c.dir.join("config"))
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    assert!(cli.status.success());
    let cli: Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(cli["data"]["failures"][0]["error"], "unavailable");
    // The running session keeps its catalog snapshot, which was empty at export time.
    let snapshot = c.call("browser_plugin_list", json!({"failed": true}));
    assert_eq!(snapshot["count"], 0);
    assert_eq!(snapshot["failures"], json!([]));
}

/// `xirius --song <dir> <args> --json`, a one-shot process next to the server.
fn cli(dir: &std::path::Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_xirius"))
        .env_remove("XIRIUS_SONG")
        .env("XIRIUS_PLUGIN_CACHE", dir.join("plugins.json"))
        .env("XIRIUS_CONFIG_DIR", dir.join("config"))
        .current_dir(std::env::temp_dir())
        .arg("--song")
        .arg(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Value>(&out.stdout).unwrap()["data"].clone()
}

#[test]
fn key_022_settings_commands_share_the_cli_and_mcp_and_external_reload() {
    let mut c = Client::start("settings");
    let tools = c.request("tools/list", json!({}));
    for command in xirius_prefs::ext::NAMES {
        assert!(
            tools["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == command.replace('.', "_"))
        );
    }
    let changed = c.call(
        "keymap_set",
        json!({"context":"roll","key":"F8","command":"note.mute"}),
    );
    assert!(changed["rows"].is_array() && changed["conflicts"].is_array());
    let from_cli = cli(&c.dir, &["keymap", "which", "F8", "--context", "roll"]);
    assert_eq!(from_cli["command"], "note.mute");
    let changed = cli(
        &c.dir,
        &[
            "keymap",
            "set",
            "F8",
            "note.delete",
            "--context",
            "roll",
            "--replace",
        ],
    );
    assert!(changed["rows"].is_array() && changed["conflicts"].is_array());
    assert_eq!(
        c.call("keymap_which", json!({"key":"F8","context":"roll"}))["command"],
        "note.delete"
    );
    for command in ["mousemap_set", "mousemap_unset", "mousemap_reset"] {
        let args = if command == "mousemap_set" {
            json!({"context":"roll.select.hold","modifier":"Alt","action":"tool:erase"})
        } else {
            json!({"context":"roll.select.hold","modifier":"Alt"})
        };
        let v = c.call(command, args);
        assert!(v["contexts"].is_object() && v["conflicts"].is_array() && v["warnings"].is_array());
    }
    c.call(
        "prefs_set",
        json!({"key":"edit.arrow.mode","value":"extended"}),
    );
    assert_eq!(
        cli(&c.dir, &["prefs", "get", "edit.arrow.mode"])["edit.arrow.mode"],
        "extended"
    );
    let export = c.call("keymap_export", json!({"format":"toml"}));
    assert!(export["text"].as_str().unwrap().contains("F8"));
    c.call("keymap_reset", json!({}));
    assert!(
        c.call("keymap_conflicts", json!({}))
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn key_007_cli_executes_the_registry_name_used_by_keys_and_note_menus() {
    let c = Client::start("shared-command");
    let t = cli(&c.dir, &["track.add.instrument", "--name", "Keys"])["id"]
        .as_str()
        .unwrap()
        .to_string();
    let clip = cli(
        &c.dir,
        &["clip.add.midi", "--track", &t, "--at", "0", "--bars", "1"],
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let note = cli(
        &c.dir,
        &[
            "note.add", "--clip", &clip, "--at", "0", "--pitch", "60", "--length", "240",
        ],
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    cli(&c.dir, &["note.duplicate", "--ids", &note, "--by", "240"]);
    let session = xirius_core::Session::open(&c.dir).unwrap();
    assert_eq!(
        session
            .history()
            .into_iter()
            .find(|h| h.current)
            .unwrap()
            .command,
        "note.duplicate"
    );
}

#[test]
fn export_jobs_live_in_the_song_folder_and_outlast_the_server() {
    let mut c = Client::start("jobs");
    let t = c.call("track_add_instrument", json!({"name": "Keys"}))["id"].clone();
    let clip = c.call("clip_add_midi", json!({"track": t, "at": 0, "bars": 1}))["id"].clone();
    c.call(
        "note_add",
        json!({"clip": clip, "at": 0, "pitch": 60, "length": 3840}),
    );
    let dir = c.dir.clone();

    // A finished FLAC export: another process reads its record.
    let job = c.call("export_mixdown", json!({"format": "flac", "bit_depth": 16}));
    let id = job["job"].as_str().unwrap().to_string();
    let done = c.poll("export_status", json!({"job": id}), |v| {
        v["state"] != "running"
    });
    assert_eq!(done["state"], "done", "{done}");
    let flac = dir.join("Mixdown").join("MCP.flac");
    assert_eq!(done["result"]["path"], flac.display().to_string());
    assert_eq!(&std::fs::read(&flac).unwrap()[..4], b"fLaC");
    assert!(dir.join("exports").join(format!("{id}.json")).exists());
    let seen = cli(&dir, &["export.status", "--job", &id]);
    assert_eq!(seen["state"], "done", "{seen}");
    assert_eq!(seen["args"]["format"], "flac");

    // A running one is cancelled from another process.
    let job = c.call(
        "export_mixdown",
        json!({"tail": 600, "out": dir.join("long.wav")}),
    );
    let long = job["job"].as_str().unwrap().to_string();
    assert_eq!(
        cli(&dir, &["export.status", "--job", &long])["state"],
        "running"
    );
    cli(&dir, &["export.cancel", "--job", &long]);
    let v = c.poll("export_status", json!({"job": long}), |v| {
        v["state"] != "running"
    });
    assert_eq!(v["state"], "cancelled", "{v}");
    assert!(!dir.join("long.wav").exists());

    // The server is killed mid-export: the next process finds it interrupted.
    let job = c.call(
        "export_mixdown",
        json!({"tail": 600, "out": dir.join("cut.wav")}),
    );
    let cut = job["job"].as_str().unwrap().to_string();
    c.poll("export_status", json!({"job": cut}), |v| {
        v["done"].as_u64().unwrap_or(0) > 0
    });
    c.child.kill().unwrap();
    c.child.wait().unwrap();
    let v = cli(&dir, &["export.status", "--job", &cut]);
    assert_eq!(v["state"], "interrupted", "{v}");
    assert!(!dir.join("cut.wav").exists());
    let all = cli(&dir, &["export.status"]);
    let states: Vec<&str> = all["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["state"].as_str().unwrap())
        .collect();
    assert_eq!(states, vec!["done", "cancelled", "interrupted"]);

    // Opened again, the interrupted export can be run again as it was asked.
    let open = cli(&dir, &["export.open", "--job", &cut]);
    assert_eq!(open["args"]["tail"], 600.0, "{open}");
}

#[test]
fn ai_013_listen_is_one_name_in_the_cli_and_mcp_and_gives_the_same_numbers() {
    let mut c = Client::start("listen");
    let tools = c.request("tools/list", json!({}));
    assert!(
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "mix_listen")
    );
    let t = c.call("track_add_instrument", json!({"name": "Keys"}))["id"].clone();
    let clip = c.call("clip_add_midi", json!({"track": t, "at": 0, "bars": 2}))["id"].clone();
    c.call(
        "note_add",
        json!({"clip": clip, "at": 0, "pitch": 60, "length": 7680}),
    );
    c.call("project_save", json!({}));
    let range = "1.1.0..3.1.0";
    let heard = c.call("mix_listen", json!({"range": range}));
    assert_eq!(c.call("mix_listen", json!({"range": range})), heard);
    assert!(heard["levels"]["lufs"].is_number(), "{heard}");
    assert_eq!(heard["bars"].as_array().unwrap().len(), 2);
    assert_eq!(heard["spectrum"]["items"].as_array().unwrap().len(), 10);
    // Another process reading the saved song hears the same.
    assert_eq!(cli(&c.dir, &["mix.listen", "--range", range]), heard);
}

#[test]
fn brw_004_the_pools_commands_are_one_name_in_the_cli_and_mcp() {
    let mut c = Client::start("pool");
    let tools = c.request("tools/list", json!({}));
    for name in [
        "file_collect",
        "file_locate",
        "select_using_file",
        "file_list",
    ] {
        assert!(
            tools["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == name),
            "{name}"
        );
    }
    let take = c.dir.join("Media").join("Recordings").join("take.wav");
    std::fs::create_dir_all(take.parent().unwrap()).unwrap();
    std::fs::write(&take, b"RIFF").unwrap();
    let listed = c.call("file_list", json!({}));
    assert_eq!(listed["files"][0]["file"], "Media/Recordings/take.wav");
    assert_eq!(listed["files"][0]["clips"], 0);
    assert_eq!(listed["files"][0]["kind"], "record");
    // Another process reading the song lists the same.
    assert_eq!(cli(&c.dir, &["file.list"]), listed);
    let r = c.request(
        "tools/call",
        json!({"name": "file_collect", "arguments": {}}),
    );
    assert_eq!(r["result"]["isError"], true, "{r}");
}
