//! The MCP server for Xirius. The tools are the command table of
//! `xirius-core`, so a command added there is a tool here with no change.
//!
//! * [`Gateway`] turns a tool call into a command run (and decides which
//!   commands an AI may reach).
//! * [`XiriusServer`] is the rmcp handler over it.
//! * [`serve_stdio`] and [`http::start`] are the two transports. HTTP listens on
//!   a loopback address only and wants a bearer token.
//! * [`run_stdio`] / [`run_http`] are the blocking entry points the command
//!   line uses; they also run the autosave clock, and on closing put the
//!   recordings nothing uses any more in the recycle bin (`project.cleanup`).
//! * [`Host`] is where a gateway runs its commands: a session behind a mutex
//!   (the command line), or a [`bridge`] to the app, which owns its song on
//!   the screen's thread and answers each run there.
//! * [`background::serve_http`] runs the HTTP transport on its own thread, for
//!   the app, which is not async.

pub mod background;
pub mod gateway;
pub mod host;
pub mod http;
mod http_events;
pub mod ipc;
pub mod persist;
mod protocol_check;
pub mod receipts;
pub mod results;
pub mod server;

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::ServiceExt;
use serde_json::Map;
use xirius_core::autosave::{Autosave, DEFAULT_INTERVAL};
use xirius_core::session::RunOptions;
use xirius_core::{ErrorCode, Session};

pub use gateway::{ADMIN_COMMANDS, CallError, Gateway, Scope, tool_name};
pub use host::{Bridge, Host, Job, Jobs, bridge};
pub use http::{HttpConfig, HttpServer};
pub use server::XiriusServer;

/// What the command line passes to the blocking entry points.
#[derive(Clone, Debug)]
pub struct ServeOptions {
    /// The AI's name in the history.
    pub author: String,
    pub scope: Scope,
    /// Write finished edits to the song folder.
    pub save: bool,
    /// How often to keep an automatic version; `None` turns the clock off.
    pub autosave: Option<Duration>,
}

impl Default for ServeOptions {
    fn default() -> Self {
        ServeOptions {
            author: "mcp".to_string(),
            scope: Scope::Edit,
            save: true,
            autosave: Some(DEFAULT_INTERVAL),
        }
    }
}

fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
}

fn start_autosave(session: &Arc<Mutex<Session>>, opts: &ServeOptions) -> Option<Autosave> {
    opts.autosave
        .map(|every| Autosave::start(session.clone(), every, None))
}

/// Closing a song the server saved into: `project.cleanup`, when the session answers it.
fn close(session: &Arc<Mutex<Session>>, opts: &ServeOptions) {
    if !opts.save || opts.scope == Scope::Read {
        return;
    }
    let Ok(mut s) = session.lock() else {
        return;
    };
    if s.dir().is_none() || s.registry().get("project.cleanup").is_none() {
        return;
    }
    match s.run("project.cleanup", &Map::new(), &RunOptions::human()) {
        Ok(r) if r.data["moved"].as_array().is_some_and(|m| !m.is_empty()) => {
            eprintln!("{}", r.message);
        }
        Ok(_) => {}
        Err(e) if e.code == ErrorCode::UnknownCommand => {}
        Err(e) => eprintln!("注意: {}", e.message),
    }
}

/// Serves MCP over any byte transport (a pair of async reader and writer)
/// until the client closes it.
pub async fn serve_on<T, E, A>(gateway: Gateway, transport: T) -> io::Result<()>
where
    T: rmcp::transport::IntoTransport<rmcp::RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let running = XiriusServer::new(gateway.clone())
        .serve(transport)
        .await
        .map_err(io::Error::other)?;
    let result = running.waiting().await.map_err(io::Error::other);
    gateway.shutdown();
    result?;
    Ok(())
}

/// Serves MCP on the process's standard input and output until stdin closes.
pub async fn serve_stdio(gateway: Gateway) -> io::Result<()> {
    serve_on(gateway, rmcp::transport::io::stdio()).await
}

/// Blocking: stdio server for one session.
pub fn run_stdio(session: Session, opts: &ServeOptions) -> io::Result<()> {
    let session = Arc::new(Mutex::new(session));
    let autosave = start_autosave(&session, opts);
    let gateway =
        Gateway::new(session.clone(), opts.author.clone(), opts.scope).with_save(opts.save);
    let out = runtime()?.block_on(serve_stdio(gateway));
    if let Some(a) = autosave {
        a.stop();
    }
    close(&session, opts);
    out
}

/// Blocking: HTTP server for one session until Ctrl-C. `ready` is called once
/// with the server (address and tokens) before it starts to wait.
pub fn run_http(
    session: Session,
    opts: &ServeOptions,
    cfg: HttpConfig,
    ready: impl FnOnce(&HttpServer),
) -> io::Result<()> {
    let session = Arc::new(Mutex::new(session));
    let autosave = start_autosave(&session, opts);
    let gateway =
        Gateway::new(session.clone(), opts.author.clone(), opts.scope).with_save(opts.save);
    let out = runtime()?.block_on(async move {
        let server = http::start(gateway, cfg).await?;
        ready(&server);
        tokio::signal::ctrl_c().await?;
        server.stop().await;
        Ok(())
    });
    if let Some(a) = autosave {
        a.stop();
    }
    close(&session, opts);
    out
}
