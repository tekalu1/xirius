//! B-1 / B-2: the commands only a window answers are tools of the app's server, not of the
//! CLI's, and the read-only hint tells the truth about the ones that change the song.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use xirius_core::Session;
use xirius_mcp::{Gateway, Scope, serve_on};

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

const SCREEN_TOOLS: &[&str] = &[
    "project_new",
    "view_mixer",
    "view_zoom_full",
    "view_zoom_time",
    "edit_nudge_right",
    "loop_selection",
    "transport_start",
    "browser_put",
    "tool_select",
];

fn has(tools: &[Value], name: &str) -> bool {
    tools.iter().any(|t| t["name"] == name)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_cli_server_leaves_out_the_screens_own_commands() {
    let mut c = Stdio::start(Gateway::new(song(), "cli", Scope::Edit)).await;
    let tools = c.tools().await;
    for name in SCREEN_TOOLS {
        assert!(!has(&tools, name), "{name} is a tool of the CLI server");
    }
    assert!(has(&tools, "track_add_audio"), "the rest stays");
    // Calling one is not a quiet success either: the tool is simply not there.
    let reply = c
        .request("tools/call", json!({"name": "view_mixer", "arguments": {}}))
        .await;
    assert!(reply.get("result").is_none(), "{reply}");
    assert_eq!(reply["error"]["code"], -32602, "{reply}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_app_server_offers_them_with_a_read_only_hint_that_matches() {
    let mut c = Stdio::start(Gateway::new(song(), "app", Scope::Edit).with_screen(true)).await;
    let tools = c.tools().await;
    for name in SCREEN_TOOLS {
        assert!(has(&tools, name), "{name} is missing from the app server");
    }
    let by = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap();
    // Placing a file and making a song change the song: never read-only.
    for name in ["browser_put", "project_new"] {
        assert_eq!(by(name)["annotations"]["readOnlyHint"], false, "{name}");
        assert_eq!(by(name)["_meta"]["xirius"]["screen_only"], true, "{name}");
    }
    for name in ["view_mixer", "tool_select"] {
        assert_eq!(by(name)["annotations"]["readOnlyHint"], true, "{name}");
        assert_eq!(by(name)["_meta"]["xirius"]["screen_only"], true, "{name}");
    }
    assert_eq!(
        by("track_add_audio")["_meta"]["xirius"]["screen_only"],
        false
    );
}
