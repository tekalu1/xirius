//! The `xirius` program, run as a user would: a real process, a real song
//! folder, real stdin / stdout.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Value, json};

/// `tools/call` params for a command (or its old tool name): through its MCP entrance.
fn call(id: u64, command: &str, args: Value) -> Value {
    let params = xirius_mcp::call_params(&xirius_session::commands::registry(), command, args);
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": params})
}

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A song folder in the system temp dir, removed afterwards.
struct Song {
    dir: PathBuf,
}

impl Song {
    /// An initialised song.
    fn new() -> Song {
        let dir = std::env::temp_dir().join(format!(
            "xirius-cli-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let out = Command::new(env!("CARGO_BIN_EXE_xirius"))
            .args(["init"])
            .arg(&dir)
            .args(["--name", "Test"])
            .output()
            .expect("run xirius init");
        assert!(out.status.success(), "{}", text(&out.stderr));
        Song { dir }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_xirius"));
        c.env_remove("XIRIUS_SONG")
            .current_dir(std::env::temp_dir())
            .arg("--song")
            .arg(&self.dir)
            .args(args);
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().expect("run xirius")
    }

    /// Runs with `--json` and expects success.
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

    fn project_json(&self) -> Vec<u8> {
        std::fs::read(self.dir.join("project.json")).expect("project.json")
    }

    fn id(&self, args: &[&str]) -> String {
        self.ok(args)["data"]["id"]
            .as_str()
            .expect("data.id")
            .to_string()
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

fn xirius(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xirius"))
        .env_remove("XIRIUS_SONG")
        .current_dir(std::env::temp_dir())
        .args(args)
        .output()
        .expect("run xirius")
}

/// A song with a MIDI clip holding two notes; returns (song, note ids).
fn song_with_notes() -> (Song, Vec<String>) {
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
    let a = s.id(&[
        "note.add", "--clip", &clp, "--at", "0", "--pitch", "60", "--length", "480",
    ]);
    let b = s.id(&[
        "note.add", "--clip", &clp, "--at", "480", "--pitch", "64", "--length", "480",
    ]);
    (s, vec![a, b])
}

fn pitches(s: &Song) -> Vec<i64> {
    let v = s.ok(&["note.list"]);
    v["data"]
        .as_array()
        .expect("notes")
        .iter()
        .map(|n| n["pitch"].as_i64().expect("pitch"))
        .collect()
}

#[test]
fn edit_dry_run_apply_and_undo_through_the_command_line() {
    let (s, ids) = song_with_notes();
    assert_eq!(pitches(&s), vec![60, 64]);
    s.ok(&["project.save"]);

    // --dry-run: reports the change, touches nothing (not even the history).
    let before = s.project_json();
    let steps_before = s.ok(&["history.list"])["data"]["cursor"].clone();
    let dry = s.ok(&[
        "note.transpose",
        "--by",
        "12",
        "--dry-run",
        &ids[0],
        &ids[1],
    ]);
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["changes"]["counts"]["note"]["changed"], 2);
    assert!(dry["step"].is_null());
    assert_eq!(s.project_json(), before);
    assert_eq!(pitches(&s), vec![60, 64]);
    assert_eq!(s.ok(&["history.list"])["data"]["cursor"], steps_before);

    // The same without --dry-run changes the song; only project.save writes its file
    // (AI-038), and the next run goes on from the step.
    let done = s.ok(&["note.transpose", "--by", "12", &ids[0], &ids[1]]);
    assert_eq!(done["dry_run"], false);
    assert!(done["step"].is_u64());
    assert_eq!(pitches(&s), vec![72, 76]);
    assert_eq!(s.project_json(), before);
    s.ok(&["project.save"]);
    assert_ne!(s.project_json(), before);

    // One undo is one step.
    s.ok(&["history.undo"]);
    assert_eq!(pitches(&s), vec![60, 64]);
    s.ok(&["project.save"]);
    assert_eq!(s.project_json(), before);
    s.ok(&["history.redo"]);
    assert_eq!(pitches(&s), vec![72, 76]);
}

#[test]
fn text_output_is_readable() {
    let (s, ids) = song_with_notes();
    let out = s.run(&["note.transpose", "--by", "-12", "--dry-run", &ids[0]]);
    assert!(out.status.success());
    let t = text(&out.stdout);
    assert!(t.contains("[ドライラン]"), "{t}");
    assert!(t.contains(&format!("~ note {}", ids[0])), "{t}");
    // Command words may be spaced instead of dotted.
    let out = s.run(&["history", "list"]);
    assert!(out.status.success());
    assert!(text(&out.stdout).contains("note.add"));
    // The command list narrows by a word, and a command's --help describes its arguments.
    let t = text(&xirius(&["ops", "list", "--grep", "note"]).stdout);
    assert!(
        t.contains("note.transpose") && !t.contains("track.add.audio"),
        "{t}"
    );
    assert!(text(&xirius(&["note.transpose", "--help"]).stdout).contains("--by"));
}

#[test]
fn the_selection_carries_over_between_runs() {
    let (s, ids) = song_with_notes();
    s.ok(&["select.set", &ids[0]]);
    let r = s.ok(&["note.transpose", "--by", "1"]);
    assert_eq!(r["changes"]["counts"]["note"]["changed"], 1);
    assert_eq!(pitches(&s), vec![61, 64]);
    // The selection is not part of the song, so a history step is not made.
    let h = s.ok(&["history.list"]);
    let commands: Vec<&str> = h["data"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["command"].as_str().unwrap())
        .collect();
    assert!(
        !commands.iter().any(|c| c.starts_with("select")),
        "{commands:?}"
    );
}

#[test]
fn failures_have_exit_codes_and_typed_errors() {
    let s = Song::new();

    // 1: the command failed. The error carries a code, in text and in JSON.
    let out = s.run(&["track.delete", "trk_999"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("error[not_found]"),
        "{}",
        text(&out.stderr)
    );
    let out = s.run(&["track.delete", "trk_999", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "not_found");

    let out = s.run(&["no.such.command"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("unknown_command"));

    let out = s.run(&["note.transpose"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("invalid_args"));

    // 2: the line itself is wrong.
    let out = s.run(&["note.transpose", "--bogus", "1"]);
    assert_eq!(out.status.code(), Some(2));
    let out = s.run(&["note.transpose", "--by"]);
    assert_eq!(out.status.code(), Some(2));

    // A command that edits needs a song.
    let out = xirius(&["track.add.audio"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("XIRIUS_SONG"));

    // None of this changed the song.
    assert_eq!(s.ok(&["history.list"])["data"]["cursor"], 0);
}

#[test]
fn the_song_is_found_from_the_environment_or_the_current_folder() {
    let s = Song::new();
    let out = Command::new(env!("CARGO_BIN_EXE_xirius"))
        .env("XIRIUS_SONG", &s.dir)
        .args(["track.add.audio", "--name", "A"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = Command::new(env!("CARGO_BIN_EXE_xirius"))
        .env_remove("XIRIUS_SONG")
        .current_dir(&s.dir)
        .args(["track.list", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["data"].to_string().contains("\"A\""), "{v}");
}

#[test]
fn an_ai_is_marked_in_the_history_and_held_to_its_permission() {
    let s = Song::new();

    // Only a person may change what the AI is allowed to do.
    let out = s.run(&["--as", "claude", "ai.permission.set", "--level", "save"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("denied"));

    // "read": the AI's edit becomes a dry run.
    s.ok(&["ai.permission.set", "--level", "read"]);
    let before = s.project_json();
    let r = s.ok(&["--as", "claude", "track.add.audio", "--name", "Ghost"]);
    assert_eq!(r["forced_dry_run"], true);
    assert!(r["step"].is_null());
    assert_eq!(s.project_json(), before);
    assert_eq!(s.ok(&["history.list"])["data"]["cursor"], 0);

    // "edit": the edit is made and one step marks the AI, in the one history
    // the person's edits are in. The song file is not written (no `save`).
    s.ok(&["ai.permission.set", "--level", "edit"]);
    s.ok(&["--as", "claude", "track.add.audio", "--name", "Pad"]);
    s.ok(&["track.add.audio", "--name", "Mine"]);
    let rows = s.ok(&["history.list"])["data"]["rows"].clone();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows[1]["command"], "track.add.audio");
    assert_eq!(rows[1]["author"], json!({"ai": "claude"}));
    assert_eq!(rows[2]["author"], "human");
    assert!(rows[1]["label"].as_str().unwrap().starts_with("AI"));

    // "off": the AI may do nothing, not even look.
    s.ok(&["ai.permission.set", "--level", "off"]);
    let out = s.run(&["--as", "claude", "track.list"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("denied"));
}

#[test]
fn an_ai_save_needs_the_save_permission() {
    let s = Song::new();
    s.ok(&["ai.permission.set", "--level", "edit"]);
    s.ok(&["--as", "claude", "track.add.audio", "--name", "Pad"]);
    // Edit-level AI: the step is in the history, the saved song is unchanged.
    assert!(!text(&s.project_json()).contains("Pad"));
    assert_eq!(s.ok(&["history.list"])["data"]["cursor"], 1);
    // The work is not lost: the next run sees it.
    assert!(s.ok(&["track.list"])["data"].to_string().contains("Pad"));

    // An edit-level AI may not write the song's file.
    let _ = s.run(&["--as", "claude", "project.save"]);
    assert!(!text(&s.project_json()).contains("Pad"));

    // Save-level AI: an edit is still only a step; its project.save writes the file (AI-038).
    s.ok(&["ai.permission.set", "--level", "save"]);
    s.ok(&["--as", "claude", "track.add.audio", "--name", "Strings"]);
    assert!(!text(&s.project_json()).contains("Strings"));
    s.ok(&["--as", "claude", "project.save"]);
    let saved = text(&s.project_json());
    assert!(saved.contains("Pad") && saved.contains("Strings"));
}

#[test]
fn batch_runs_lines_in_one_session_and_stops_at_the_first_error() {
    let s = Song::new();
    let script = "\
# a comment

track.add.instrument --name \"Lead Synth\"
select.set trk_3
track.mute
track.add.audio --name Never --bogus 1
track.add.audio --name AlsoNever
";
    let mut child = s
        .cmd(&["batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("6 行目"),
        "{}",
        text(&out.stderr)
    );

    // The lines before the bad one were applied (the selection made `track.mute` work).
    let v = s.ok(&["track.list"]);
    let t = v["data"].to_string();
    assert!(t.contains("Lead Synth"), "{t}");
    assert!(!t.contains("Never") && !t.contains("AlsoNever"), "{t}");
    assert!(v["data"][0]["mute"] == true, "{t}");
}

#[test]
fn the_song_survives_between_runs_and_versions_work() {
    let (s, ids) = song_with_notes();
    s.ok(&["version.save", "--name", "before"]);
    s.ok(&["note.transpose", "--by", "7", &ids[0]]);
    assert_eq!(pitches(&s), vec![67, 64]);
    let list = s.ok(&["version.list"]);
    let id = list["data"][0]["id"].as_str().unwrap().to_string();
    s.ok(&["version.restore", &id]);
    assert_eq!(pitches(&s), vec![60, 64]);
    // The restore is itself one step: undo brings the later work back.
    s.ok(&["history.undo"]);
    assert_eq!(pitches(&s), vec![67, 64]);

    // A version opens as a separate song; this one is untouched.
    let other_tmp = tempfile::Builder::new()
        .prefix("xirius-cli-version-")
        .tempdir()
        .unwrap();
    let other = other_tmp.path().join("copy");
    s.ok(&["version.open", &id, other.to_str().unwrap()]);
    assert!(other.join("manifest.toml").is_file());
    assert_eq!(pitches(&s), vec![67, 64]);
}

// ---- MCP ---------------------------------------------------------------------------

fn mcp_exchange(song: &Path, extra: &[&str], lines: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xirius"))
        .env_remove("XIRIUS_SONG")
        .env("XIRIUS_CONFIG_DIR", song.join("config"))
        .env("XIRIUS_PLUGIN_CACHE", song.join("plugins.json"))
        .arg("--song")
        .arg(song)
        .args(extra)
        // Protocol tests never need a speaker or a real MIDI endpoint.
        .args(["mcp", "--no-device", "--midi-in", "none"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for l in lines {
        writeln!(stdin, "{l}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    text(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).expect("a JSON line"))
        .collect()
}

#[test]
fn mcp_lists_the_registry_and_runs_a_tool() {
    let s = Song::new();
    let replies = mcp_exchange(
        &s.dir,
        &[],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize",
                   "params":{"protocolVersion":"2025-06-18","capabilities":{},
                             "clientInfo":{"name":"t","version":"0"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            call(3, "track.add.audio", json!({"name":"Via MCP"})),
            call(4, "track.add.audio", json!({"name":"Dry","dry_run":true})),
            call(5, "track.delete", json!({"ids":"trk_999"})),
            call(6, "project.save", json!({})),
            call(7, "ops.list", json!({"group":"folder"})),
            call(8, "ops.schema", json!({"name":"note.quantize"})),
            call(9, "ops.schema", json!({"name":"ai.request.accept"})),
            call(10, "ops.schema", json!({})),
            call(11, "ops.schema", json!({"name":"note_quantize"})),
            json!({"jsonrpc":"2.0","id":12,"method":"tools/call",
                   "params":{"name":"run_edit","arguments":{"command":"project.save"}}}),
        ],
    );
    // The notification gets no reply: ids 1..12 only. Calls run side by side,
    // so the replies are matched by id, not by position.
    assert_eq!(replies.len(), 12);
    let by_id = |id: u64| replies.iter().find(|r| r["id"] == id).unwrap();
    assert_eq!(by_id(1)["result"]["serverInfo"]["name"], "xirius");
    assert!(by_id(1)["result"]["capabilities"]["tools"].is_object());

    // MCP-008: the list is the entrances and a few direct tools, not one tool per command.
    let tools: Vec<&str> = by_id(2)["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tools,
        [
            "run_read",
            "run_edit",
            "run_guarded",
            "run_destructive",
            "ops_list",
            "ops_schema",
            "ai_batch"
        ]
    );
    let registry: Value = {
        let out = xirius(&["ops", "list", "--group", "folder", "--json"]);
        serde_json::from_slice(&out.stdout).unwrap()
    };
    // CLI and MCP list the same table, except for human-only and screen commands.
    let reachable: Vec<&Value> = registry["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["risk"] != "human_only" && c["screen_only"] != true)
        .map(|c| &c["name"])
        .collect();
    let rows = by_id(7)["result"]["structuredContent"]["data"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        rows.iter().map(|r| &r["name"]).collect::<Vec<_>>(),
        reachable
    );
    let bus = rows.iter().find(|r| r["name"] == "folder.bus.add").unwrap();
    assert_eq!(bus["tool"], "run_edit");
    let t = &by_id(8)["result"]["structuredContent"]["data"];
    assert_eq!(t["inputSchema"]["type"], "object");
    assert!(t["description"].as_str().unwrap().len() > 3);
    assert_eq!(
        by_id(9)["result"]["structuredContent"]["error"]["code"],
        "not_found"
    );
    // Over MCP, ops_schema wants a name or a group, not every schema at once; the command
    // line still gives them all.
    assert_eq!(by_id(10)["result"]["isError"], true);
    assert_eq!(
        by_id(10)["result"]["structuredContent"]["error"]["code"],
        "invalid_args"
    );
    let all: Value = serde_json::from_slice(&xirius(&["ops", "schema", "--json"]).stdout).unwrap();
    assert!(
        all["data"].as_array().unwrap().len() > 100,
        "{}",
        all["message"]
    );
    // A command's tool name finds its schema too.
    assert_eq!(
        by_id(11)["result"]["structuredContent"]["data"]["_meta"]["xirius"]["command"],
        "note.quantize"
    );
    // Saving through the edit entrance is a mistake the result puts right: it names the entrance.
    assert_eq!(by_id(12)["result"]["isError"], true);
    assert_eq!(
        by_id(12)["result"]["structuredContent"]["tool"],
        "run_guarded"
    );

    assert_eq!(by_id(3)["result"]["isError"], false);
    assert_eq!(by_id(3)["result"]["structuredContent"]["step"], 1);
    // dry_run goes through as an argument.
    assert_eq!(by_id(4)["result"]["structuredContent"]["dry_run"], true);
    assert!(by_id(4)["result"]["structuredContent"]["step"].is_null());
    // A failure is a tool result with a typed code, not a protocol error.
    assert_eq!(by_id(5)["result"]["isError"], true);
    assert_eq!(
        by_id(5)["result"]["structuredContent"]["error"]["code"],
        "not_found"
    );
    // Saving waits for a person's answer (OPEN-04): a decision, not a save.
    assert_eq!(by_id(6)["result"]["isError"], false);
    let ask = &by_id(6)["result"]["structuredContent"];
    assert_eq!(ask["confirmation_required"], true);
    assert_eq!(ask["decision"]["default_answer"], "no");

    // The edit is in the song's history, marked as an AI's.
    let rows = s.ok(&["history.list"])["data"]["rows"].clone();
    assert_eq!(rows[1]["author"], json!({"ai": "mcp"}));
    assert!(
        s.ok(&["track.list"])["data"]
            .to_string()
            .contains("Via MCP")
    );
}

#[test]
fn cli_and_mcp_create_silent_empty_instruments_and_drum_pads() {
    for mcp in [false, true] {
        for instrument in [None, Some("Sampler"), Some("Drum Kit")] {
            let s = Song::new();
            let t = if mcp {
                let (name, arguments) = match instrument {
                    None => ("track.add.instrument", json!({})),
                    Some(name) => ("track.add.with-instrument", json!({"instrument": name})),
                };
                let replies = mcp_exchange(
                    &s.dir,
                    &[],
                    &[
                        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                        "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}),
                        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                        call(2, name, arguments),
                    ],
                );
                let reply = replies.iter().find(|r| r["id"] == 2).unwrap();
                assert_eq!(reply["result"]["isError"], false, "{reply}");
                reply["result"]["structuredContent"]["data"]["id"]
                    .as_str()
                    .unwrap()
                    .to_string()
            } else {
                match instrument {
                    None => s.id(&["track.add.instrument"]),
                    Some(name) => s.id(&["track.add.with-instrument", "--instrument", name]),
                }
            };
            let c = s.id(&["clip.add.midi", "--track", &t, "--at", "0", "--bars", "1"]);
            s.ok(&[
                "note.add",
                "--clip",
                &c,
                "--at",
                "0",
                "--pitch",
                "36",
                "--length",
                "960",
                "--velocity",
                "127",
            ]);
            let loaded = xirius_core::session::Session::open(&s.dir).unwrap();
            let t = t.parse().unwrap();
            assert!(
                loaded.project().tracks[&t]
                    .inserts
                    .iter()
                    .all(|x| x.samples.is_empty())
            );
            let (mut host, mut audio, warnings) = xirius_session::host::SessionHost::start(
                loaded.project(),
                xirius_session::host::HostConfig::default(),
            )
            .unwrap();
            assert!(warnings.is_empty(), "{warnings:?}");
            host.note(
                t,
                xirius_engine::NoteKind::On {
                    pitch: 36,
                    velocity: 1.0,
                },
            );
            host.play();
            for _ in 0..100 {
                let mut out = [123.0; 960];
                audio.render(&mut out, 2);
                assert!(
                    out.iter().all(|x| *x == 0.0),
                    "mcp={mcp} instrument={instrument:?}"
                );
            }
        }
    }
}

#[test]
fn mcp_reports_protocol_mistakes() {
    let s = Song::new();
    let replies = mcp_exchange(
        &s.dir,
        &["--as", "agent"],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize",
                   "params":{"protocolVersion":"2025-06-18","capabilities":{},
                             "clientInfo":{"name":"t","version":"0"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"nope"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"x_y"}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
                   "params":{"name":"ai_request_accept"}}),
            json!({"jsonrpc":"2.0","id":5,"method":"ping"}),
            json!({"jsonrpc":"2.0","id":6,"method":"tools/call",
                   "params":{"name":"run_edit","arguments":{"command":"ai.request.accept"}}}),
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
                   "params":{"name":"run_read","arguments":{"command":"x.y"}}}),
        ],
    );
    let by_id = |id: u64| replies.iter().find(|r| r["id"] == id).unwrap();
    assert_eq!(by_id(2)["error"]["code"], -32601);
    // A tool that does not exist, and one a person alone may run, look the same,
    // by a tool name or through an entrance.
    assert_eq!(by_id(3)["error"]["code"], -32602);
    assert_eq!(by_id(4)["error"]["code"], -32602);
    assert_eq!(by_id(5)["result"], json!({}));
    assert_eq!(by_id(6)["error"]["code"], -32602);
    assert_eq!(by_id(7)["error"]["code"], -32602);
}

#[test]
fn mcp_http_listens_on_loopback_and_prints_a_token_that_opens_it() {
    use std::io::{BufRead, BufReader};
    use std::net::TcpStream;

    let s = Song::new();
    let mut child = Command::new(env!("CARGO_BIN_EXE_xirius"))
        .env_remove("XIRIUS_SONG")
        .env_remove("XIRIUS_MCP_TOKEN")
        .env_remove("XIRIUS_MCP_ADMIN_TOKEN")
        .arg("--song")
        .arg(&s.dir)
        .arg("--json")
        .args(["mcp", "--http", "--autosave-sec", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut first)
        .unwrap();
    let info: Value = serde_json::from_str(&first).expect("one JSON line with the address");
    let url = info["mcp_url"].as_str().unwrap().to_string();
    let token = info["token"].as_str().unwrap().to_string();
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    assert_ne!(token, info["admin_token"].as_str().unwrap());

    let addr = url.trim_start_matches("http://").trim_end_matches("/mcp");
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string();
    // The printed token opens the door; refusing a missing or wrong one is in the mcp tests
    // (`http_without_the_token_or_with_a_wrong_one_is_refused`).
    let mut c = TcpStream::connect(addr).unwrap();
    write!(
        c,
        "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n\
         Connection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    // Only the status is under test.
    let mut right = String::new();
    BufReader::new(c).read_line(&mut right).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();

    assert!(right.starts_with("HTTP/1.1 200"), "{right}");
}

/// The plugin scan starts the `xirius` program itself for each bundle, so a copy of it alone,
/// with no other file next to it, can scan (the child answers in a file it is told).
#[test]
fn the_command_answers_as_the_plugin_scan_child() {
    let dir = tempfile::Builder::new()
        .prefix("xirius-cli-scan-child-")
        .tempdir()
        .unwrap();
    let bundle = dir.path().join("Fake.vst3");
    std::fs::create_dir_all(&bundle).unwrap();
    let out = dir.path().join("answer.json");
    let status = Command::new(env!("CARGO_BIN_EXE_xirius"))
        .args(["__scan-plugin", "--bundle"])
        .arg(&bundle)
        .arg("--out")
        .arg(&out)
        .status()
        .unwrap();
    assert!(out.is_file(), "no answer file: {status:?}");
    let answer: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert!(answer.get("error").is_some() || answer.get("entries").is_some());
}
