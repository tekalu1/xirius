//! What a run has to tell (`export.stale`, EXP-023) goes down the event stream by its own name.
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use xirius_core::command::{Args, CommandDef, Output};
use xirius_core::{CmdResult, ExtContext, Extension};
use xirius_mcp::{Gateway, HttpConfig, Scope, http};

/// Tells how many tracks the song has each time it changes.
#[derive(Default)]
struct Teller(Vec<Value>);

impl Extension for Teller {
    fn answers(&self, _name: &str) -> bool {
        false
    }

    fn run(&mut self, _: &ExtContext<'_>, _: &CommandDef, _: &Args, _: bool) -> CmdResult<Output> {
        unreachable!()
    }

    fn song_changed(&mut self, cx: &ExtContext<'_>) {
        self.0
            .push(json!({ "event": "export.stale", "tracks": cx.project.tracks.len() }));
    }

    fn take_events(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.0)
    }
}

type Lines = tokio::io::Lines<BufReader<tokio::net::TcpStream>>;

async fn next(lines: &mut Lines) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn a_runs_events_go_down_the_stream_by_their_name() {
    let mut session = xirius_core::Session::new("events");
    session.add_extension(Box::<Teller>::default());
    let had = session.project().tracks.len();
    let gateway = Gateway::new(Arc::new(Mutex::new(session)), "http", Scope::Edit).with_save(false);
    let server = http::start(gateway.clone(), HttpConfig::default())
        .await
        .unwrap();
    let mut socket = tokio::net::TcpStream::connect(server.addr).await.unwrap();
    socket.write_all(format!("GET /events HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nAccept: text/event-stream\r\n\r\n", server.addr, server.token).as_bytes()).await.unwrap();
    let mut lines = BufReader::new(socket).lines();
    while !next(&mut lines).await.starts_with("data: ") {}
    let worker = gateway.clone();
    tokio::task::spawn_blocking(move || {
        worker
            .call(
                "track_add_audio",
                json!({"request_key": "run-event", "name": "One"})
                    .as_object()
                    .unwrap(),
            )
            .unwrap()
    })
    .await
    .unwrap();
    let mut name = String::new();
    let told = loop {
        let line = next(&mut lines).await;
        if let Some(n) = line.strip_prefix("event: ") {
            name = n.to_string();
        } else if let Some(data) = line.strip_prefix("data: ") {
            let event: Value = serde_json::from_str(data).unwrap();
            if event["event"] == "export.stale" {
                break event;
            }
            assert_eq!(name, "operation", "{event}");
        }
    };
    assert_eq!(name, "export.stale");
    assert_eq!(told, json!({ "event": "export.stale", "tracks": had + 1 }));
    tokio::time::timeout(std::time::Duration::from_secs(5), server.stop())
        .await
        .unwrap();
}
