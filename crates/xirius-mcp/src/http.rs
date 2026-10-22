//! MCP over HTTP on this PC only. Three guards stand in front of every
//! request, in this order:
//!
//! 1. the peer address must be loopback (and the server refuses to bind to
//!    anything but a loopback address in the first place);
//! 2. the `Host` header must name this PC (a web page cannot rebind a name to
//!    reach it);
//! 3. a bearer token. The MCP endpoint (`/mcp`) and the admin endpoint
//!    (`/admin/run`) have *different* tokens: the AI gets the first, a person
//!    keeps the second and uses it to answer what the AI is waiting on.

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{Method, StatusCode, header};
use axum::response::Response;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::{Map, Value, json};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::gateway::{Gateway, invalid_args};
use crate::server::XiriusServer;

pub const MCP_PATH: &str = "/mcp";
pub const EVENTS_PATH: &str = "/events";
pub const ADMIN_PATH: &str = "/admin/run";
const MAX_ADMIN_BODY: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct HttpConfig {
    /// Must be a loopback address.
    pub bind: IpAddr,
    /// 0 lets the OS choose.
    pub port: u16,
    /// Generated when not given.
    pub token: Option<String>,
    /// Generated when not given.
    pub admin_token: Option<String>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 0,
            token: None,
            admin_token: None,
        }
    }
}

/// A running server. Dropping it without [`HttpServer::stop`] leaves the task
/// to end with the runtime.
pub struct HttpServer {
    pub addr: SocketAddr,
    pub token: String,
    pub admin_token: String,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    gateway: Gateway,
}

impl HttpServer {
    pub fn events_url(&self) -> String {
        format!("http://{}{EVENTS_PATH}", self.addr)
    }
    pub fn mcp_url(&self) -> String {
        format!("http://{}{MCP_PATH}", self.addr)
    }

    pub fn admin_url(&self) -> String {
        format!("http://{}{ADMIN_PATH}", self.addr)
    }

    pub async fn stop(mut self) {
        self.gateway.shutdown();
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        let _ = self.task.await;
    }

    /// Waits until the server ends on its own (it does not, until stopped).
    pub async fn wait(&mut self) {
        let _ = (&mut self.task).await;
    }
}

struct Shared {
    token: String,
    admin_token: String,
    mcp: StreamableHttpService<XiriusServer, NeverSessionManager>,
    gateway: Gateway,
}

/// 128 random bits from the OS, as hex.
pub fn new_token() -> io::Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| io::Error::other(format!("乱数を取得できません: {e}")))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

pub async fn start(gateway: Gateway, cfg: HttpConfig) -> io::Result<HttpServer> {
    if !cfg.bind.is_loopback() {
        eprintln!(
            "警告: {} は localhost 以外から接続できるアドレスです。待ち受けを開始できません",
            cfg.bind
        );
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} では待ち受けできません。外部から接続できるアドレスは使用できません（127.0.0.1 か ::1 のみ）",
                cfg.bind
            ),
        ));
    }
    let token = match cfg.token {
        Some(t) => non_empty(t, "token")?,
        None => new_token()?,
    };
    let admin_token = match cfg.admin_token {
        Some(t) => non_empty(t, "admin token")?,
        None => new_token()?,
    };
    if token == admin_token {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "AI 用の token と管理用の token には別の値を指定してください",
        ));
    }
    let listener = TcpListener::bind(SocketAddr::new(cfg.bind, cfg.port)).await?;
    let addr = listener.local_addr()?;

    let server = XiriusServer::new(gateway.clone());
    let mcp = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true),
    );
    let shared = Arc::new(Shared {
        token: token.clone(),
        admin_token: admin_token.clone(),
        mcp,
        gateway,
    });
    let app = Router::new().fallback(route).with_state(shared.clone());
    let (tx, rx) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = rx.await;
        })
        .await;
    });
    Ok(HttpServer {
        addr,
        token,
        admin_token,
        stop: Some(tx),
        task,
        gateway: shared.gateway.clone(),
    })
}

fn non_empty(t: String, what: &str) -> io::Result<String> {
    if t.len() < 16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{what} は 16 文字以上にしてください（推測されないようにするため）"),
        ));
    }
    Ok(t)
}

fn reply(status: StatusCode, body: Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_default()
}

fn refuse(status: StatusCode, why: &str) -> Response {
    let mut r = reply(status, json!({"ok": false, "error": {"message": why}}));
    if status == StatusCode::UNAUTHORIZED {
        r.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            axum::http::HeaderValue::from_static("Bearer"),
        );
    }
    r
}

/// Compares without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut d = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        d |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    d == 0
}

fn bearer(req: &Request) -> Option<&str> {
    let v = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = v.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
}

/// Only a peer on this PC gets any answer.
pub fn peer_allowed(peer: SocketAddr) -> bool {
    peer.ip().is_loopback()
}

/// `localhost`, `127.0.0.1` or `[::1]`, with or without a port.
fn host_is_local(req: &Request) -> bool {
    let Some(h) = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let name = if let Some(rest) = h.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        h.split(':').next().unwrap_or("")
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

async fn route(
    State(sh): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
) -> Response {
    if !peer_allowed(peer) {
        return refuse(
            StatusCode::FORBIDDEN,
            "この PC 内からの接続のみ受け付けます",
        );
    }
    if !host_is_local(&req) {
        return refuse(StatusCode::FORBIDDEN, "Host がこの PC を指していません");
    }
    let path = req.uri().path().to_string();
    let expected = match path.as_str() {
        MCP_PATH | EVENTS_PATH => &sh.token,
        ADMIN_PATH => &sh.admin_token,
        _ => return refuse(StatusCode::NOT_FOUND, "見つかりません"),
    };
    match bearer(&req) {
        Some(t) if same(t, expected) => {}
        _ => return refuse(StatusCode::UNAUTHORIZED, "token が無いか、正しくありません"),
    }
    if path == EVENTS_PATH {
        return if req.method() == Method::GET {
            crate::http_events::response(&sh.gateway)
        } else {
            refuse(StatusCode::METHOD_NOT_ALLOWED, "GET で接続してください")
        };
    }
    if path == MCP_PATH {
        return sh.mcp.handle(req).await.map(Body::new);
    }
    admin(&sh.gateway, req).await
}

async fn admin(gateway: &Gateway, req: Request) -> Response {
    if req.method() != Method::POST {
        return refuse(StatusCode::METHOD_NOT_ALLOWED, "POST で送信してください");
    }
    let bytes = match to_bytes(req.into_body(), MAX_ADMIN_BODY).await {
        Ok(b) => b,
        Err(_) => {
            return refuse(
                StatusCode::PAYLOAD_TOO_LARGE,
                "リクエストの本文が大きすぎます",
            );
        }
    };
    let body: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            return reply(
                StatusCode::BAD_REQUEST,
                invalid_args(&format!("JSON を読み込めません: {e}")),
            );
        }
    };
    let Some(command) = body.get("command").and_then(Value::as_str) else {
        return reply(
            StatusCode::BAD_REQUEST,
            invalid_args("{\"command\": 名前, \"args\": {…}} の形式で送信してください"),
        );
    };
    let args: Map<String, Value> = match body.get("args") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return reply(
                StatusCode::BAD_REQUEST,
                invalid_args("args は {名前: 値} の形式で指定してください"),
            );
        }
    };
    let gateway = gateway.clone();
    let command = command.to_string();
    // The song is behind a plain lock; keep the wait off the async threads.
    let out = tokio::task::spawn_blocking(move || gateway.admin(&command, &args)).await;
    match out {
        Ok(Ok((v, false))) => reply(StatusCode::OK, v),
        Ok(Ok((v, true))) => reply(StatusCode::UNPROCESSABLE_ENTITY, v),
        Ok(Err(e)) => reply(StatusCode::FORBIDDEN, invalid_args(&e.0)),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "内部エラー"),
    }
}
