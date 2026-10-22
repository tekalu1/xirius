use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use xirius_mcp::{Gateway, HttpConfig, Scope, http};

#[tokio::test]
async fn http_completion_stream_uses_the_token_and_closes_with_the_server() {
    let gateway = Gateway::new(
        Arc::new(Mutex::new(xirius_core::Session::new("events"))),
        "http",
        Scope::Edit,
    )
    .with_save(false);
    let server = http::start(gateway.clone(), HttpConfig::default())
        .await
        .unwrap();
    let mut socket = tokio::net::TcpStream::connect(server.addr).await.unwrap();
    socket.write_all(format!("GET /events HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nAccept: text/event-stream\r\n\r\n", server.addr, server.token).as_bytes()).await.unwrap();
    let mut lines = BufReader::new(socket).lines();
    loop {
        let line = tokio::time::timeout(std::time::Duration::from_secs(5), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if line.starts_with("data: ") {
            break;
        }
    }
    let worker = gateway.clone();
    tokio::task::spawn_blocking(move || {
        worker
            .call(
                "track_add_audio",
                json!({"request_key": "http-event", "name": "One"})
                    .as_object()
                    .unwrap(),
            )
            .unwrap()
    })
    .await
    .unwrap();
    let mut states = Vec::new();
    while states.last() != Some(&json!("completed")) {
        let line = tokio::time::timeout(std::time::Duration::from_secs(5), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Some(json) = line.strip_prefix("data: ") {
            let event: Value = serde_json::from_str(json).unwrap();
            if event["receipt"]["id"] == "http-event" {
                states.push(event["receipt"]["state"].clone());
            }
        }
    }
    assert_eq!(
        states,
        [json!("accepted"), json!("running"), json!("completed")]
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), server.stop())
        .await
        .unwrap();
}
