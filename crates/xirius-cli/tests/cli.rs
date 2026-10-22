//! The `xirius` program, run as a user would: a real process, a real song
//! folder, real stdin / stdout.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Value, json};

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

    // The same without --dry-run changes the song and saves it.
    let done = s.ok(&["note.transpose", "--by", "12", &ids[0], &ids[1]]);
    assert_eq!(done["dry_run"], false);
    assert!(done["step"].is_u64());
    assert_eq!(pitches(&s), vec![72, 76]);
    assert_ne!(s.project_json(), before);

    // One undo is one step, and it is saved too.
    s.ok(&["history.undo"]);
    assert_eq!(pitches(&s), vec![60, 64]);
    assert_eq!(s.project_json(), before);
    s.ok(&["history.redo"]);
    assert_eq!(pitches(&s), vec![72, 76]);
}

#[test]
fn a_folder_gets_a_new_bus_in_one_step_through_the_command_line() {
    let s = Song::new();
    let f = s.id(&["folder.add", "--name", "Drums"]);
    let before = s.project_json();
    let done = s.ok(&["folder.bus.add", "--id", &f, "--name", "Drum bus"]);
    let bus = done["data"]["id"].as_str().unwrap().to_string();
    let song: Value = serde_json::from_slice(&s.project_json()).unwrap();
    assert!(song.to_string().contains("Drum bus"), "{song}");
    assert!(s.ok(&["track.list"])["data"].to_string().contains(&bus));
    // One undo takes both the bus and the connection away.
    s.ok(&["history.undo"]);
    assert_eq!(s.project_json(), before);
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
fn ops_list_and_schema_come_from_the_registry() {
    let out = xirius(&["ops", "list", "--grep", "note"]);
    assert!(out.status.success());
    let t = text(&out.stdout);
    assert!(
        t.contains("note.transpose") && t.contains("note.quantize"),
        "{t}"
    );
    assert!(!t.contains("track.add.audio"), "{t}");

    let out = xirius(&["ops.list", "--json"]);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let names: Vec<&str> = v["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    for want in [
        "track.add.audio",
        "track.mute",
        "track.move",
        "clip.split",
        "note.add",
        "note.move",
        "note.quantize",
        "note.transpose",
        "note.velocity.set",
        "select.set",
        "view.zoom",
        "history.undo",
        "version.restore",
    ] {
        assert!(names.contains(&want), "{want} is not registered");
    }

    let out = xirius(&["ops", "schema", "note.transpose", "--json"]);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let schema = &v["data"];
    assert_eq!(schema["inputSchema"]["required"], json!(["by"]));
    assert_eq!(schema["inputSchema"]["properties"]["by"]["type"], "integer");
    assert_eq!(schema["annotations"]["xirius"]["history"], true);

    // A command with --help describes its arguments.
    let out = xirius(&["note.transpose", "--help"]);
    assert!(text(&out.stdout).contains("--by"));
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

    s.ok(&["ai.permission.set", "--level", "save"]);
    s.ok(&["--as", "claude", "track.add.audio", "--name", "Strings"]);
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
        .arg("--song")
        .arg(song)
        .args(extra)
        .arg("mcp")
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
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
                   "params":{"name":"track_add_audio","arguments":{"name":"Via MCP"}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
                   "params":{"name":"track_add_audio","arguments":{"name":"Dry","dry_run":true}}}),
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call",
                   "params":{"name":"track_delete","arguments":{"ids":"trk_999"}}}),
            json!({"jsonrpc":"2.0","id":6,"method":"tools/call",
                   "params":{"name":"project_save","arguments":{}}}),
        ],
    );
    // The notification gets no reply: ids 1..6 only. Calls run side by side,
    // so the replies are matched by id, not by position.
    assert_eq!(replies.len(), 6);
    let by_id = |id: u64| replies.iter().find(|r| r["id"] == id).unwrap();
    assert_eq!(by_id(1)["result"]["serverInfo"]["name"], "xirius");
    assert!(by_id(1)["result"]["capabilities"]["tools"].is_object());

    let tools = by_id(2)["result"]["tools"].as_array().unwrap();
    let registry: Value = {
        let out = xirius(&["ops", "list", "--json"]);
        serde_json::from_slice(&out.stdout).unwrap()
    };
    // CLI and MCP expose the same table, except for human-only commands.
    let reachable = registry["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["risk"] != "human_only" && c["screen_only"] != true)
        .count();
    assert_eq!(tools.len(), reachable);
    assert!(tools.iter().all(|t| t["name"] != "ai_request_accept"));
    assert!(tools.iter().any(|t| t["name"] == "folder_bus_add"));
    let t = tools.iter().find(|t| t["name"] == "note_quantize").unwrap();
    assert_eq!(t["inputSchema"]["type"], "object");
    assert!(t["description"].as_str().unwrap().len() > 3);

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
        ],
    );
    let by_id = |id: u64| replies.iter().find(|r| r["id"] == id).unwrap();
    assert_eq!(by_id(2)["error"]["code"], -32601);
    // A tool that does not exist, and one a person alone may run, look the same.
    assert_eq!(by_id(3)["error"]["code"], -32602);
    assert_eq!(by_id(4)["error"]["code"], -32602);
    assert_eq!(by_id(5)["result"], json!({}));
}

#[test]
fn mcp_http_listens_on_loopback_and_wants_its_token() {
    use std::io::{BufRead, BufReader, Read};
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
    let post = |auth: &str| -> String {
        let mut c = TcpStream::connect(addr).unwrap();
        write!(
            c,
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\nConnection: close\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        out
    };
    let none = post("");
    let wrong = post("Authorization: Bearer not-the-token-not-the-token\r\n");
    let right = post(&format!("Authorization: Bearer {token}\r\n"));
    child.kill().unwrap();
    child.wait().unwrap();

    assert!(none.starts_with("HTTP/1.1 401"), "{none}");
    assert!(wrong.starts_with("HTTP/1.1 401"), "{wrong}");
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
