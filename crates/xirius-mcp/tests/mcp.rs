//! The MCP server end to end, over both transports. The network is only
//! 127.0.0.1, nothing is played, and no window is touched.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use xirius_core::command::Risk;
use xirius_core::session::{AiPermission, Confirm, RunOptions};
use xirius_core::{Registry, Session};
use xirius_mcp::background::{Background, serve_http};
use xirius_mcp::gateway::{lock, tool_name};
use xirius_mcp::http::{self, HttpConfig, HttpServer};
use xirius_mcp::persist::run_and_persist;
use xirius_mcp::{Gateway, Scope, bridge, serve_on};

const PROTOCOL: &str = "2025-06-18";

fn shared() -> Arc<Mutex<Session>> {
    Arc::new(Mutex::new(Session::new("mcp")))
}

/// Tracks other than the master.
fn tracks(s: &Arc<Mutex<Session>>) -> usize {
    lock(s).project().tracks.len() - 1
}

/// Where the history stands (an undone step stays in the list until a new one
/// replaces it, so the cursor is what counts).
fn steps(s: &Arc<Mutex<Session>>) -> u64 {
    lock(s).cursor()
}

// ---- stdio-style transport (any async reader and writer) ----------------------------------

struct Stdio {
    out: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    lines: tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    next: u64,
    events: Vec<Value>,
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
            events: Vec::new(),
        };
        let init = c
            .request(
                "initialize",
                json!({"protocolVersion": PROTOCOL, "capabilities": {},
                       "clientInfo": {"name": "test", "version": "0"}}),
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
                self.events.push(v);
                continue;
            }
            assert_eq!(v["id"], json!(id), "{v}");
            return v;
        }
    }

    async fn call(&mut self, tool: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": args}))
            .await
    }
}

fn payload(reply: &Value) -> &Value {
    &reply["result"]["structuredContent"]
}

fn http_reply(text: String) -> Value {
    if let Ok(value) = serde_json::from_str(&text) {
        return value;
    }
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|json| serde_json::from_str::<Value>(json).ok())
        .find(|value| value.get("id").is_some())
        .unwrap_or(Value::String(text))
}

#[tokio::test]
async fn completion_is_pushed_over_the_mcp_transport() {
    for wait_ms in [0, 30000] {
        let mut client =
            Stdio::start(Gateway::new(shared(), "events", Scope::Edit).with_save(false)).await;
        let reply = client
            .call(
                "track_add_audio",
                json!({"name": "Event", "request_key": "event-one", "wait_ms": wait_ms}),
            )
            .await;
        assert_eq!(payload(&reply)["receipt"]["id"], "event-one");
        while !client
            .events
            .iter()
            .any(|event| event["params"]["receipt"]["state"] == "completed")
        {
            let line =
                tokio::time::timeout(std::time::Duration::from_secs(10), client.lines.next_line())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
            client.events.push(serde_json::from_str(&line).unwrap());
        }
        let states: Vec<_> = client
            .events
            .iter()
            .filter(|event| event["method"] == "notifications/xirius/operation")
            .map(|event| event["params"]["receipt"]["state"].clone())
            .collect();
        assert_eq!(
            states,
            [json!("accepted"), json!("running"), json!("completed")]
        );
        assert!(
            client.events.last().unwrap()["params"]["result"]["data"]["id"]
                .as_str()
                .is_some()
        );
    }
}

#[tokio::test]
async fn command_protocol_mismatch_is_reported_at_initialize() {
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(serve_on(
        Gateway::new(shared(), "protocol", Scope::Edit),
        tokio::io::split(server),
    ));
    let (reader, writer) = tokio::io::split(client);
    let mut client = Stdio {
        out: writer,
        lines: BufReader::new(reader).lines(),
        next: 1,
        events: Vec::new(),
    };
    let reply = client.request("initialize", json!({"protocolVersion": PROTOCOL, "capabilities": {},
        "clientInfo": {"name": "mismatch", "version": "0"}, "_meta": {"xirius": {"command_protocol": 999}}})).await;
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("一致しません")
    );
}

// ---- the tool list is the command table ---------------------------------------------------

fn assert_tools_match(tools: &[Value], scope: Scope) {
    let registry = Registry::standard();
    let want: Vec<_> = registry
        .all()
        .iter()
        .filter(|d| !d.screen_only)
        .filter(|d| match scope {
            Scope::Edit => d.risk != Risk::HumanOnly,
            Scope::Read => d.risk == Risk::Read,
        })
        .collect();
    assert_eq!(tools.len(), want.len());
    for d in want {
        let t = tools
            .iter()
            .find(|t| t["name"] == tool_name(d.name))
            .unwrap_or_else(|| panic!("{} is not a tool", d.name));
        let schema = d.json_schema();
        assert_eq!(t["description"], schema["description"], "{}", d.name);
        assert_eq!(t["inputSchema"], schema["inputSchema"], "{}", d.name);
        assert_eq!(
            t["annotations"]["readOnlyHint"],
            json!(d.risk == Risk::Read),
            "{}",
            d.name
        );
        assert_eq!(
            t["annotations"]["destructiveHint"],
            json!(d.risk == Risk::Destructive),
            "{}",
            d.name
        );
        assert_eq!(t["_meta"]["xirius"]["command"], d.name);
        assert_eq!(t["_meta"]["xirius"]["history"], json!(d.history));
    }
    // A person's answers are never a tool.
    for hidden in ["ai_decide", "ai_request_accept", "ai_request_revert"] {
        assert!(tools.iter().all(|t| t["name"] != hidden), "{hidden}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stdio_tools_and_schemas_are_the_command_table() {
    let mut c = Stdio::start(Gateway::new(shared(), "claude", Scope::Edit)).await;
    let list = c.request("tools/list", json!({})).await;
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_tools_match(tools, Scope::Edit);
    assert!(tools.len() > 80, "{}", tools.len());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_server_lists_and_runs_only_what_looks() {
    let song = shared();
    let mut c = Stdio::start(Gateway::new(song.clone(), "claude", Scope::Read)).await;
    let list = c.request("tools/list", json!({})).await;
    assert_tools_match(list["result"]["tools"].as_array().unwrap(), Scope::Read);

    // An edit is not a tool at all: a protocol error, and nothing changed.
    let r = c.call("track_add_audio", json!({"name": "Vox"})).await;
    assert_eq!(r["error"]["code"], -32602, "{r}");
    assert_eq!(tracks(&song), 0);
    let r = c.call("track_list", json!({})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
}

#[tokio::test(flavor = "multi_thread")]
async fn stdio_calls_change_the_song_and_can_be_undone() {
    let song = shared();
    let mut c = Stdio::start(Gateway::new(song.clone(), "claude", Scope::Edit)).await;

    let r = c.call("track_add_audio", json!({"name": "Vox"})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(payload(&r)["ok"], true);
    assert_eq!(tracks(&song), 1);
    {
        let s = lock(&song);
        let last = s.steps().last().unwrap();
        assert_eq!(last.author, xirius_core::Author::Ai("claude".into()));
    }

    // The same command by its dotted name works too.
    let r = c.call("track.add.audio", json!({"name": "Gtr"})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(tracks(&song), 2);

    // A dry run changes nothing.
    let r = c
        .call("track_add_audio", json!({"name": "Dry", "dry_run": true}))
        .await;
    assert_eq!(payload(&r)["dry_run"], true);
    assert_eq!(tracks(&song), 2);

    // A mistake in the arguments is a typed result, not a dropped connection.
    let r = c
        .call("track_rename", json!({"id": "trk_999", "name": "x"}))
        .await;
    assert_eq!(r["result"]["isError"], true);
    assert_eq!(payload(&r)["error"]["code"], "not_found");

    let r = c.call("history_undo", json!({})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(tracks(&song), 1);
    c.call("history_undo", json!({})).await;
    assert_eq!(tracks(&song), 0);
    c.call("history_redo", json!({"count": 2})).await;
    assert_eq!(tracks(&song), 2);
}

// ---- 1 request = 1 step, through the wire ---------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn one_ai_request_is_one_step() {
    let song = shared();
    let mut c = Stdio::start(Gateway::new(song.clone(), "claude", Scope::Edit)).await;
    let before = steps(&song);

    // A batch.
    let r = c
        .call(
            "ai_batch",
            json!({"title": "ドラム", "commands": [
                {"command": "track.add.audio", "args": {"name": "Kick"}},
                {"command": "track.add.audio", "args": {"name": "Snare"}},
            ]}),
        )
        .await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(tracks(&song), 2);
    assert_eq!(steps(&song), before + 1);
    c.call("history_undo", json!({})).await;
    assert_eq!(tracks(&song), 0, "one undo takes back the whole batch");

    // Begin … end around separate calls.
    let before = steps(&song);
    c.call("ai_request_begin", json!({"title": "弾く"})).await;
    c.call("track_add_audio", json!({"name": "A"})).await;
    c.call("track_add_audio", json!({"name": "B"})).await;
    c.call("track_add_audio", json!({"name": "C"})).await;
    let r = c.call("ai_request_end", json!({})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(tracks(&song), 3);
    assert_eq!(steps(&song), before + 1, "three calls, one step");
    let last_author_is_ai = lock(&song).steps().last().unwrap().author.is_ai();
    assert!(last_author_is_ai);
    c.call("history_undo", json!({})).await;
    assert_eq!(tracks(&song), 0);
}

// ---- deleting waits for a person -------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn deleting_waits_for_an_answer_that_only_a_person_can_give() {
    let song = shared();
    let t = {
        let mut s = lock(&song);
        let r = s
            .run_value(
                "track.add.audio",
                json!({"name": "Vox"}),
                &RunOptions::human(),
            )
            .unwrap();
        r.data["id"].as_str().unwrap().to_string()
    };
    let gw = Gateway::new(song.clone(), "claude", Scope::Edit);
    let mut c = Stdio::start(gw.clone()).await;

    let r = c.call("track_delete", json!({"ids": [t]})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(payload(&r)["confirmation_required"], true);
    let token = payload(&r)["decision"]["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(payload(&r)["decision"]["default_answer"], "no");
    assert_eq!(tracks(&song), 1, "nothing was deleted");

    // The AI has no tool to answer with.
    let r = c
        .call("ai_decide", json!({"token": token, "answer": "yes"}))
        .await;
    assert_eq!(r["error"]["code"], -32602, "{r}");
    assert_eq!(tracks(&song), 1);

    // A person answers on the admin side.
    let (v, is_error) = gw
        .admin(
            "ai.decide",
            json!({"token": token, "answer": "yes"})
                .as_object()
                .unwrap(),
        )
        .unwrap();
    assert!(!is_error, "{v}");
    assert_eq!(tracks(&song), 0);
    // It is one AI step and it undoes.
    c.call("history_undo", json!({})).await;
    assert_eq!(tracks(&song), 1);

    // And "no" means no.
    let r = c.call("track_delete", json!({"ids": [t]})).await;
    let token = payload(&r)["decision"]["token"]
        .as_str()
        .unwrap()
        .to_string();
    gw.admin(
        "ai.decide",
        json!({"token": token, "answer": "no"}).as_object().unwrap(),
    )
    .unwrap();
    assert_eq!(tracks(&song), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn saving_over_the_song_waits_for_an_answer_too() {
    let song = shared();
    let mut c = Stdio::start(Gateway::new(song.clone(), "claude", Scope::Edit)).await;
    let r = c.call("project_save", json!({})).await;
    assert_eq!(payload(&r)["confirmation_required"], true, "{r}");
}

// ---- a work copy is taken in or thrown away by a person ------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_work_copy_is_accepted_or_reverted_by_a_person() {
    let song = shared();
    let gw = Gateway::new(song.clone(), "claude", Scope::Edit);
    let mut c = Stdio::start(gw.clone()).await;
    let admin = |name: &str, args: Value| {
        let (v, is_error) = gw.admin(name, args.as_object().unwrap()).unwrap();
        assert!(!is_error, "{name}: {v}");
        v
    };

    // Revert: the song never changed.
    c.call("ai_request_begin", json!({"title": "試す", "draft": true}))
        .await;
    c.call("track_add_audio", json!({"name": "Try"})).await;
    assert_eq!(tracks(&song), 0, "the song is untouched while the AI works");
    c.call("ai_request_end", json!({})).await;
    let status = admin("ai.request.status", json!({}));
    assert_eq!(status["data"]["request"]["state"], "review");
    admin("ai.request.revert", json!({}));
    assert_eq!(tracks(&song), 0);

    // Accept: one step, undoable.
    let before = steps(&song);
    c.call("ai_request_begin", json!({"title": "足す", "draft": true}))
        .await;
    c.call("track_add_audio", json!({"name": "Keep1"})).await;
    c.call("track_add_audio", json!({"name": "Keep2"})).await;
    c.call("ai_request_end", json!({})).await;
    admin("ai.request.accept", json!({}));
    assert_eq!(tracks(&song), 2);
    assert_eq!(steps(&song), before + 1);
    admin("history.undo", json!({}));
    assert_eq!(tracks(&song), 0);

    // The AI cannot accept for itself.
    let r = c.call("ai_request_accept", json!({})).await;
    assert_eq!(r["error"]["code"], -32602, "{r}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_admin_door_runs_only_the_answers() {
    let gw = Gateway::new(shared(), "claude", Scope::Edit);
    for name in [
        "track.add.audio",
        "project.save",
        "ai.permission.set",
        "project.open",
    ] {
        let e = gw.admin(name, &serde_json::Map::new()).unwrap_err();
        assert!(e.0.contains("管理用"), "{name}: {}", e.0);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_ui_can_read_what_the_ai_is_editing() {
    let song = shared();
    let mut c = Stdio::start(Gateway::new(song.clone(), "claude", Scope::Edit)).await;
    c.call("ai_request_begin", json!({"title": "触る"})).await;
    c.call("track_add_audio", json!({"name": "Glow"})).await;
    let r = c.call("ai_activity", json!({})).await;
    let act = &payload(&r)["data"]["activity"];
    assert_eq!(act["active"], true, "{r}");
    assert!(
        act["editing"].as_array().is_some_and(|e| !e.is_empty()),
        "{act}"
    );
}

// ---- HTTP ----------------------------------------------------------------------------------------------

/// A small HTTP/1.1 client over a raw socket (one request per connection).
async fn raw_http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    host: &str,
    headers: &[(&str, String)],
    body: Option<&Value>,
) -> (u16, String) {
    let body = body.map(Value::to_string).unwrap_or_default();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() {
        req.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    req.push_str("\r\n");
    req.push_str(&body);
    let mut sock = TcpStream::connect(addr).await.unwrap();
    sock.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        sock.read_to_end(&mut raw),
    )
    .await
    .expect("a reply in time")
    .unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let chunked = head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    (
        status,
        if chunked {
            unchunk(rest)
        } else {
            rest.to_string()
        },
    )
}

fn unchunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = s.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push_str(&rest[..n]);
        s = rest[n..].trim_start_matches("\r\n");
    }
    out
}

struct Http {
    server: HttpServer,
    host: String,
}

impl Http {
    async fn start(gateway: Gateway) -> Http {
        let server = http::start(gateway, HttpConfig::default()).await.unwrap();
        assert!(server.addr.ip().is_loopback());
        let host = server.addr.to_string();
        Http { server, host }
    }

    fn mcp_headers(&self, token: Option<&str>) -> Vec<(&'static str, String)> {
        let mut h = vec![("Accept", "application/json, text/event-stream".to_string())];
        if let Some(t) = token {
            h.push(("Authorization", format!("Bearer {t}")));
        }
        h
    }

    async fn rpc(&self, token: Option<&str>, method: &str, params: Value) -> (u16, Value) {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let (status, text) = raw_http(
            self.server.addr,
            "POST",
            http::MCP_PATH,
            &self.host,
            &self.mcp_headers(token),
            Some(&body),
        )
        .await;
        let v = http_reply(text);
        (status, v)
    }

    async fn call(&self, tool: &str, args: Value) -> Value {
        let (status, v) = self
            .rpc(
                Some(&self.server.token),
                "tools/call",
                json!({"name": tool, "arguments": args}),
            )
            .await;
        assert_eq!(status, 200, "{v}");
        v
    }

    async fn admin(&self, token: Option<&str>, command: &str, args: Value) -> (u16, Value) {
        let mut h = vec![];
        if let Some(t) = token {
            h.push(("Authorization", format!("Bearer {t}")));
        }
        let (status, text) = raw_http(
            self.server.addr,
            "POST",
            http::ADMIN_PATH,
            &self.host,
            &h,
            Some(&json!({"command": command, "args": args})),
        )
        .await;
        (status, http_reply(text))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn http_lists_the_same_tools_and_changes_the_song_undoably() {
    let song = shared();
    let h = Http::start(Gateway::new(song.clone(), "claude", Scope::Edit)).await;

    let (status, list) = h.rpc(Some(&h.server.token), "tools/list", json!({})).await;
    assert_eq!(status, 200, "{list}");
    assert_tools_match(list["result"]["tools"].as_array().unwrap(), Scope::Edit);

    let r = h
        .call("track_add_audio", json!({"name": "Over HTTP"}))
        .await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(tracks(&song), 1);
    assert!(lock(&song).steps().last().unwrap().author.is_ai());

    let r = h.call("history_undo", json!({})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(tracks(&song), 0);
    h.server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn http_without_the_token_or_with_a_wrong_one_is_refused() {
    let song = shared();
    let h = Http::start(Gateway::new(song.clone(), "claude", Scope::Edit)).await;
    let call = json!({"name": "track_add_audio", "arguments": {"name": "No"}});

    let (status, _) = h.rpc(None, "tools/call", call.clone()).await;
    assert_eq!(status, 401);
    let (status, _) = h
        .rpc(Some("not-the-token-at-all"), "tools/call", call.clone())
        .await;
    assert_eq!(status, 401);
    // The right length with one character off is still wrong.
    let mut near = h.server.token.clone();
    near.pop();
    near.push(if h.server.token.ends_with('0') {
        '1'
    } else {
        '0'
    });
    let (status, _) = h.rpc(Some(&near), "tools/call", call.clone()).await;
    assert_eq!(status, 401);
    // The person's token does not open the AI's door, nor the AI's the person's.
    let (status, _) = h.rpc(Some(&h.server.admin_token), "tools/call", call).await;
    assert_eq!(status, 401);
    let (status, _) = h
        .admin(Some(&h.server.token), "ai.pending", json!({}))
        .await;
    assert_eq!(status, 401);
    let (status, _) = h.admin(None, "ai.pending", json!({})).await;
    assert_eq!(status, 401);

    assert_eq!(tracks(&song), 0, "nothing got through");
    assert_eq!(steps(&song), 0);
    h.server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn http_accepts_only_this_pc() {
    // It will not even bind to an address other machines can reach.
    for bind in [
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)),
        IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
    ] {
        let cfg = HttpConfig {
            bind,
            ..HttpConfig::default()
        };
        let e = http::start(Gateway::new(shared(), "claude", Scope::Edit), cfg)
            .await
            .err()
            .unwrap_or_else(|| panic!("{bind} was accepted"));
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput, "{bind}");
    }
    // A peer that is not loopback is refused whatever it sends.
    for peer in [
        "192.168.1.5:5000",
        "10.0.0.2:1",
        "8.8.8.8:80",
        "[2001:db8::1]:9",
    ] {
        assert!(!http::peer_allowed(peer.parse().unwrap()), "{peer}");
    }
    for peer in ["127.0.0.1:5000", "127.9.9.9:1", "[::1]:80"] {
        assert!(http::peer_allowed(peer.parse().unwrap()), "{peer}");
    }

    // A page that rebinds a name to this PC sends another Host: refused, even
    // with the right token.
    let song = shared();
    let h = Http::start(Gateway::new(song.clone(), "claude", Scope::Edit)).await;
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let (status, _) = raw_http(
        h.server.addr,
        "POST",
        http::MCP_PATH,
        "evil.example.com",
        &h.mcp_headers(Some(&h.server.token)),
        Some(&body),
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = raw_http(
        h.server.addr,
        "POST",
        http::ADMIN_PATH,
        "evil.example.com:80",
        &[("Authorization", format!("Bearer {}", h.server.admin_token))],
        Some(&json!({"command": "ai.pending"})),
    )
    .await;
    assert_eq!(status, 403);
    h.server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn tokens_are_given_or_made_and_never_short_or_shared() {
    let gw = Gateway::new(shared(), "claude", Scope::Edit);
    let short = HttpConfig {
        token: Some("abc".into()),
        ..HttpConfig::default()
    };
    assert!(http::start(gw.clone(), short).await.is_err());
    let same = HttpConfig {
        token: Some("0123456789abcdef".into()),
        admin_token: Some("0123456789abcdef".into()),
        ..HttpConfig::default()
    };
    assert!(http::start(gw.clone(), same).await.is_err());

    let a = http::start(gw.clone(), HttpConfig::default())
        .await
        .unwrap();
    let b = http::start(gw, HttpConfig::default()).await.unwrap();
    assert_eq!(a.token.len(), 32);
    assert_ne!(a.token, a.admin_token);
    assert_ne!(a.token, b.token, "a new token every start");
    a.stop().await;
    b.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_answers_over_http_with_the_admin_token() {
    let song = shared();
    let t = {
        let mut s = lock(&song);
        let r = s
            .run_value(
                "track.add.audio",
                json!({"name": "Vox"}),
                &RunOptions::human(),
            )
            .unwrap();
        r.data["id"].as_str().unwrap().to_string()
    };
    let h = Http::start(Gateway::new(song.clone(), "claude", Scope::Edit)).await;

    let r = h.call("track_delete", json!({"ids": [t]})).await;
    let token = payload(&r)["decision"]["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(tracks(&song), 1);

    let (status, pending) = h
        .admin(Some(&h.server.admin_token), "ai.pending", json!({}))
        .await;
    assert_eq!(status, 200, "{pending}");
    assert!(pending.to_string().contains(&token));

    // The admin door runs the answers only.
    let (status, v) = h
        .admin(
            Some(&h.server.admin_token),
            "track.delete",
            json!({"ids": [t]}),
        )
        .await;
    assert_eq!(status, 403, "{v}");

    let (status, v) = h
        .admin(
            Some(&h.server.admin_token),
            "ai.decide",
            json!({"token": token, "answer": "yes"}),
        )
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(tracks(&song), 0);
    // The token was good once.
    let (status, _) = h
        .admin(
            Some(&h.server.admin_token),
            "ai.decide",
            json!({"token": token, "answer": "yes"}),
        )
        .await;
    assert_eq!(status, 422);
    h.server.stop().await;
}

// ---- the four permission levels, as the AI sees them (AI-023, AI-026, AI-027, AI-028) ----------

/// A song with one MIDI clip of three notes, made by a person. Returns the clip.
fn song_with_notes(s: &mut Session) -> String {
    let human = RunOptions::human();
    let id = |v: Value| v["id"].as_str().unwrap().to_string();
    let t = id(s
        .run_value("track.add.instrument", json!({"name": "Keys"}), &human)
        .unwrap()
        .data);
    let clip = id(s
        .run_value("clip.add.midi", json!({"track": t, "at": 0}), &human)
        .unwrap()
        .data);
    for (i, pitch) in [60, 64, 67].into_iter().enumerate() {
        s.run_value(
            "note.add",
            json!({"clip": clip, "at": i * 240, "pitch": pitch}),
            &human,
        )
        .unwrap();
    }
    clip
}

/// The pitches of the notes, and whether the AI changed each last.
fn pitches(s: &Session) -> Vec<(u8, bool)> {
    s.project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .flat_map(|c| c.notes().iter())
        .map(|n| (n.pitch, n.ai))
        .collect()
}

fn notes(s: &Arc<Mutex<Session>>) -> Vec<(u8, bool)> {
    pitches(&lock(s))
}

fn level(song: &Arc<Mutex<Session>>, p: AiPermission) {
    lock(song).set_ai_permission(p).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn each_permission_level_answers_the_ai_with_its_reason() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-mcp-permission-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("mcp-permission");
    let _ = std::fs::remove_dir_all(&dir);
    let song = shared();
    let clip = song_with_notes(&mut lock(&song));
    lock(&song).save_as(&dir).unwrap();
    let mut c = Stdio::start(Gateway::new(song.clone(), "claude", Scope::Edit)).await;
    let transpose = json!({"ids": [clip], "by": 2});
    let before = notes(&song);
    assert_eq!(before, [(60, false), (64, false), (67, false)]);

    // オフ: nothing at all, not even a look, and the result says why.
    level(&song, AiPermission::Off);
    for (tool, args) in [
        ("note_transpose", transpose.clone()),
        ("track_list", json!({})),
    ] {
        let r = c.call(tool, args).await;
        assert_eq!(r["result"]["isError"], true, "{r}");
        assert_eq!(payload(&r)["error"]["code"], "denied", "{r}");
        let why = payload(&r)["error"]["message"].as_str().unwrap();
        assert!(why.contains("AI の権限がオフです"), "{why}");
        assert!(why.contains("AI の権限: off"), "{why}");
    }
    assert_eq!(notes(&song), before);

    // 読み取りのみ: looks run; an edit comes back as the dry-run proposal only.
    level(&song, AiPermission::Read);
    let r = c.call("track_list", json!({})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    let r = c.call("note_transpose", transpose.clone()).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(payload(&r)["dry_run"], true, "{r}");
    let msg = payload(&r)["message"].as_str().unwrap();
    assert!(msg.contains("ドライラン"), "{msg}");
    assert!(
        payload(&r)["changes"].to_string().contains("note"),
        "the proposal names the notes: {r}"
    );
    assert_eq!(notes(&song), before, "a proposal changes nothing");
    let steps_before = steps(&song);
    // Saving is not an edit to propose: it is refused, with the reason.
    let r = c.call("project_save", json!({})).await;
    assert_eq!(payload(&r)["error"]["code"], "denied", "{r}");
    let why = payload(&r)["error"]["message"].as_str().unwrap();
    assert!(why.contains("AI の権限: read"), "{why}");
    assert_eq!(steps(&song), steps_before);

    // 編集: the edit lands, marked as the AI's. Saving waits for an answer when
    // the owner asks (OPEN-04), and is refused with the reason when it does not (the app).
    level(&song, AiPermission::Edit);
    let r = c.call("note_transpose", transpose.clone()).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(notes(&song), [(62, true), (66, true), (69, true)]);
    assert!(lock(&song).is_dirty(), "an AI with 編集 does not save");
    let r = c.call("project_save", json!({})).await;
    assert_eq!(payload(&r)["confirmation_required"], true, "{r}");
    lock(&song).set_confirm(Confirm {
        destructive: true,
        save: false,
    });
    let r = c.call("project_save", json!({})).await;
    assert_eq!(r["result"]["isError"], true, "{r}");
    assert_eq!(payload(&r)["error"]["code"], "denied", "{r}");
    let why = payload(&r)["error"]["message"].as_str().unwrap();
    assert!(
        why.contains("保存には「編集と保存・書き出し」の権限が必要です"),
        "{why}"
    );
    assert!(lock(&song).is_dirty());

    // One undo takes the AI's edit back, marks and all.
    let r = c.call("history_undo", json!({})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(notes(&song), before);

    // 編集と保存・書き出し: the edit is saved with it, and saving runs.
    level(&song, AiPermission::Save);
    let r = c.call("note_transpose", transpose).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert!(!lock(&song).is_dirty(), "an AI with 保存 saves its edit");
    let r = c.call("project_save", json!({})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(payload(&r)["ok"], true, "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- a song the app owns, reached through a bridge --------------------------------------------------

async fn call_over_http(server: &Background, tool: &str, args: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                      "params": {"name": tool, "arguments": args}});
    let headers = vec![
        ("Accept", "application/json, text/event-stream".to_string()),
        ("Authorization", format!("Bearer {}", server.token)),
    ];
    let host = server.addr.to_string();
    let (status, text) = raw_http(
        server.addr,
        "POST",
        http::MCP_PATH,
        &host,
        &headers,
        Some(&body),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    http_reply(text)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_app_answers_each_call_on_its_own_thread() {
    let registry = Arc::new(Registry::standard());
    let (host, mut jobs) = bridge(registry.clone());
    let (ready, clip) = std::sync::mpsc::channel();
    // The "screen": it owns the song and answers the runs one by one.
    let owner = std::thread::spawn(move || {
        let mut s = Session::with_registry(Session::new("bridge").project().clone(), registry);
        let clip = song_with_notes(&mut s);
        s.set_confirm(Confirm {
            destructive: true,
            save: false,
        });
        ready.send(clip).unwrap();
        let mut answered = 0;
        while let Some(job) = jobs.blocking_recv() {
            let r = run_and_persist(&mut s, &job.command, &job.raw, &job.opts, job.save);
            job.answer(r);
            answered += 1;
        }
        (answered, pitches(&s))
    });
    let clip = clip.recv().unwrap();
    let server = serve_http(
        Gateway::hosted(host, "claude", Scope::Edit),
        HttpConfig::default(),
    )
    .unwrap();
    assert!(server.addr.ip().is_loopback());
    assert!(server.mcp_url().ends_with(http::MCP_PATH));

    let r = call_over_http(&server, "note_transpose", json!({"ids": [clip], "by": -1})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    let r = call_over_http(&server, "project_save", json!({})).await;
    assert_eq!(payload(&r)["error"]["code"], "denied", "{r}");
    // Stopping the server drops the gateway, which closes the channel.
    server.stop();
    let (answered, notes) = owner.join().unwrap();
    assert_eq!(answered, 2);
    assert_eq!(notes, [(59, true), (63, true), (66, true)]);
}

#[test]
fn a_bridge_whose_app_has_closed_says_so() {
    let (host, jobs) = bridge(Arc::new(Registry::standard()));
    drop(jobs);
    let gw = Gateway::hosted(host, "claude", Scope::Edit);
    let (v, is_error) = gw.call("track_list", &serde_json::Map::new()).unwrap();
    assert!(is_error);
    let why = v["error"]["message"].as_str().unwrap();
    assert!(why.contains("閉じられています"), "{v}");
}
