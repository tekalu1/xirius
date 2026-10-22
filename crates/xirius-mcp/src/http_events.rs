//! A persistent event stream alongside stateless MCP HTTP calls.
use crate::Gateway;
use axum::response::{IntoResponse, Response, Sse, sse::Event};
use std::convert::Infallible;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

pub fn response(gateway: &Gateway) -> Response {
    let events = BroadcastStream::new(gateway.events())
        .map(|event| event.unwrap_or_else(|_| serde_json::json!({"resync": true})))
        .take_while(|event| event["shutdown"] != true);
    let stream =
        tokio_stream::once(serde_json::json!({"hello": xirius_core::protocol::Hello::default()}))
            .chain(events)
            .map(|event| {
                // What a run had to tell goes by its own name (`export.stale`, EXP-023).
                let name = event["event"].as_str().unwrap_or("operation").to_string();
                Ok::<_, Infallible>(Event::default().event(name).data(event.to_string()))
            });
    Sse::new(stream).into_response()
}
