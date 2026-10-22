//! The HTTP server on its own thread, for a program that is not async (the
//! app). It runs until stopped or dropped.

use std::io;
use std::net::SocketAddr;
use std::sync::mpsc;
use std::thread::JoinHandle;

use tokio::sync::oneshot;

use crate::gateway::Gateway;
use crate::http::{self, HttpConfig, MCP_PATH};

pub struct Background {
    pub addr: SocketAddr,
    pub token: String,
    pub admin_token: String,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Background {
    pub fn mcp_url(&self) -> String {
        format!("http://{}{MCP_PATH}", self.addr)
    }

    /// Stops the server and waits for its thread. A call still running must
    /// be able to finish (a [`bridge`](crate::host::bridge) whose jobs are no
    /// longer answered ends its calls with an error once its receiver goes).
    pub fn stop(mut self) {
        self.signal();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    fn signal(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for Background {
    /// Tells the server to stop; does not wait (the thread ends on its own).
    fn drop(&mut self) {
        self.signal();
    }
}

/// Starts [`http::start`] on a new thread with its own runtime and returns
/// once it listens (or failed to).
pub fn serve_http(gateway: Gateway, cfg: HttpConfig) -> io::Result<Background> {
    let (ready, started) = mpsc::channel();
    let (stop, stopped) = oneshot::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("xirius-mcp".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = ready.send(Err(e));
                    return;
                }
            };
            runtime.block_on(async move {
                match http::start(gateway, cfg).await {
                    Ok(server) => {
                        let _ = ready.send(Ok((
                            server.addr,
                            server.token.clone(),
                            server.admin_token.clone(),
                        )));
                        let _ = stopped.await;
                        server.stop().await;
                    }
                    Err(e) => {
                        let _ = ready.send(Err(e));
                    }
                }
            });
        })?;
    let (addr, token, admin_token) = started
        .recv()
        .map_err(|_| io::Error::other("MCP のサーバーを開始できません"))??;
    Ok(Background {
        addr,
        token,
        admin_token,
        stop: Some(stop),
        thread: Some(thread),
    })
}
