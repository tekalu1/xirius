//! B-1 / B-2 / MCP-008: the commands only a window answers run from the app's server, not
//! from the CLI's, and neither lists them in `tools/list` (they are reached through an
//! entrance by name). Their read-only hint tells the truth about the ones that change the song.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use xirius_core::Session;
use xirius_mcp::{EDIT_TOOL, Gateway, READ_TOOL, Scope, serve_on};

const PROTOCOL: &str = "2025-06-18";

struct Stdio {
    out: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    lines: tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    next: u64,
}

impl Stdio {
    async fn start(gateway: Gateway) -> Stdio {
        let (client, server) = tokio::io::duplex(1 << 20);
        tokio::spawn(serve_on(gateway, tokio::io::split(server)));
        let (r, w) = tokio::io::split(client);
        let mut c = Stdio {
            out: w,
            lines: BufReader::new(r).lines(),
            next: 1,
        };
        let init = c
            .request(
                "initialize",
                json!({"protocolVersion": PROTOCOL, "capabilities": {},
                       "clientInfo": {"name": "screen-test", "version": "0"}}),
            )
            .await;
        assert_eq!(init["result"]["serverInfo"]["name"], "xirius");
        c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        c
    }

    async fn send(&mut self, msg: Value) {
        self.out
            .write_all(format!("{msg}\n").as_bytes())
            .await
            .unwrap();
        self.out.flush().await.unwrap();
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        loop {
            let line =
                tokio::time::timeout(std::time::Duration::from_secs(20), self.lines.next_line())
                    .await
                    .expect("a reply in time")
                    .unwrap()
                    .expect("the server closed");
            let v: Value = serde_json::from_str(&line).unwrap();
            if v.get("id").is_none() {
                continue;
            }
            assert_eq!(v["id"], json!(id), "{v}");
            return v;
        }
    }

    async fn tools(&mut self) -> Vec<Value> {
        let reply = self.request("tools/list", json!({})).await;
        reply["result"]["tools"].as_array().unwrap().clone()
    }
}

fn song() -> Arc<Mutex<Session>> {
    Arc::new(Mutex::new(Session::new("screen")))
}

const SCREEN_COMMANDS: &[&str] = &[
    "project.new",
    "view.mixer",
    "view.zoom.full",
    "view.zoom.time",
    "edit.nudge.right",
    "loop.selection",
    "transport.start",
    "browser.put",
    "tool.select",
];

const LISTED: &[&str] = &[
    "run_read",
    "run_edit",
    "run_guarded",
    "run_destructive",
    "ops_list",
    "ops_schema",
    "ai_batch",
];

fn names(tools: &[Value]) -> Vec<&str> {
    tools.iter().map(|t| t["name"].as_str().unwrap()).collect()
}

async fn schema(c: &mut Stdio, command: &str) -> Value {
    let r = c
        .request(
            "tools/call",
            json!({"name": "ops_schema", "arguments": {"name": command}}),
        )
        .await;
    r["result"]["structuredContent"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_cli_server_leaves_out_the_screens_own_commands() {
    let mut c = Stdio::start(Gateway::new(song(), "cli", Scope::Edit)).await;
    assert_eq!(names(&c.tools().await), LISTED);
    for name in SCREEN_COMMANDS {
        let v = schema(&mut c, name).await;
        assert_eq!(v["error"]["code"], "not_found", "{name}: {v}");
    }
    assert_eq!(schema(&mut c, "track.add.audio").await["ok"], true);
    // Calling one is not a quiet success either: the command is simply not there.
    let reply = c
        .request(
            "tools/call",
            json!({"name": READ_TOOL, "arguments": {"command": "view.mixer"}}),
        )
        .await;
    assert!(reply.get("result").is_none(), "{reply}");
    assert_eq!(reply["error"]["code"], -32602, "{reply}");
    // Nor by its old tool name.
    let reply = c
        .request("tools/call", json!({"name": "view_mixer", "arguments": {}}))
        .await;
    assert_eq!(reply["error"]["code"], -32602, "{reply}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_app_server_runs_them_with_a_read_only_hint_that_matches() {
    let mut c = Stdio::start(Gateway::new(song(), "app", Scope::Edit).with_screen(true)).await;
    // The list stays the same few tools: the screen adds commands, not tools.
    assert_eq!(names(&c.tools().await), LISTED);
    for name in SCREEN_COMMANDS {
        let v = schema(&mut c, name).await;
        assert_eq!(v["ok"], true, "{name}: {v}");
        assert_eq!(v["data"]["_meta"]["xirius"]["screen_only"], true, "{name}");
    }
    // Placing a file and making a song change the song: never read-only, and they run
    // through the edit entrance.
    for name in ["browser.put", "project.new"] {
        let v = schema(&mut c, name).await;
        assert_eq!(v["data"]["annotations"]["readOnlyHint"], false, "{name}");
        assert_eq!(v["data"]["_meta"]["xirius"]["tool"], EDIT_TOOL, "{name}");
    }
    for name in ["view.mixer", "tool.select"] {
        let v = schema(&mut c, name).await;
        assert_eq!(v["data"]["annotations"]["readOnlyHint"], true, "{name}");
        assert_eq!(v["data"]["_meta"]["xirius"]["tool"], READ_TOOL, "{name}");
    }
    let v = schema(&mut c, "track.add.audio").await;
    assert_eq!(v["data"]["_meta"]["xirius"]["screen_only"], false);
    // With a screen, the entrance reaches the command (the answer is the command's own,
    // not a protocol error).
    let reply = c
        .request(
            "tools/call",
            json!({"name": READ_TOOL, "arguments": {"command": "view.mixer"}}),
        )
        .await;
    assert!(reply.get("error").is_none(), "{reply}");
    assert!(reply["result"].is_object(), "{reply}");
    // The read entrance still refuses a screen command that changes the song, and says
    // which entrance runs it.
    let reply = c
        .request(
            "tools/call",
            json!({"name": READ_TOOL, "arguments": {"command": "project.new"}}),
        )
        .await;
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let v = &reply["result"]["structuredContent"];
    assert_eq!(v["error"]["code"], "invalid_args", "{reply}");
    assert_eq!(v["tool"], EDIT_TOOL, "{reply}");
}
