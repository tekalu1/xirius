//! The MCP server the window keeps for an outside AI (AI-019, AI-022), and
//! the OS's wish for less motion (AI-037).

use xirius_mcp::background::{Background, serve_http};
use xirius_mcp::{Gateway, HttpConfig, Jobs, Scope, bridge};

use super::Connection;
use crate::studio::Studio;

/// Who the history and the marks name for a run that came over MCP.
const AUTHOR: &str = "AI";

/// What the pop-over shows for where it listens when the setting turned the server off.
pub(crate) const OFF: &str = "MCP サーバーは設定でオフになっています";

pub(crate) type Starting = xirius_session::background::Job<(Background, Jobs)>;

/// Socket setup, runtime construction and its readiness recv stay on a worker.
pub(crate) fn start_window(studio: &mut Studio) -> Option<Starting> {
    if !studio
        .song
        .settings()
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .prefs
        .bool("ai.mcp.enabled")
    {
        studio.ai.connection = Some(Err(OFF.into()));
        return None;
    }
    match xirius_session::background::Job::spawn("window MCP startup", || {
        #[cfg(any(test, feature = "loading-probe"))]
        xirius_core::io_test::before("mcp_start", std::path::Path::new(""))
            .map_err(|e| e.to_string())?;
        let (host, jobs) = bridge(xirius_session::commands::shared_registry());
        let server = serve_http(
            Gateway::hosted(host, AUTHOR, Scope::Edit),
            HttpConfig::default(),
        )
        .map_err(|e| e.to_string())?;
        Ok((server, jobs))
    }) {
        Ok(job) => Some(job),
        Err(e) => {
            studio.ai.connection = Some(Err(e));
            None
        }
    }
}

/// Starts the loopback server for the studio's song and shows where it
/// listens in the AI's pop-over. Its runs arrive at the returned jobs; each
/// goes to [`Studio::ai_job`] on the window's thread. The server stops when
/// the returned [`Background`] is dropped. With the setting `ai.mcp.enabled` off, no server
/// starts (the setting is read when the window opens).
pub fn start(studio: &mut Studio) -> Option<(Background, Jobs)> {
    let enabled = studio
        .song
        .settings()
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .prefs
        .bool("ai.mcp.enabled");
    if !enabled {
        studio.ai.connection = Some(Err(OFF.to_string()));
        return None;
    }
    let (host, jobs) = bridge(xirius_session::commands::shared_registry());
    match serve_http(
        Gateway::hosted(host, AUTHOR, Scope::Edit),
        HttpConfig::default(),
    ) {
        Ok(server) => {
            studio.ai.connection = Some(Ok(Connection {
                url: server.mcp_url(),
                token: server.token.clone(),
            }));
            Some((server, jobs))
        }
        Err(e) => {
            studio.ai.connection = Some(Err(format!("MCP のサーバーを開始できません: {e}")));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, Condvar, Mutex, mpsc},
        time::{Duration, Instant},
    };
    #[test]
    fn server_readiness_is_never_waited_for_by_window_callbacks() {
        let mut s = Studio::new(crate::song::Song::empty());
        s.song
            .set(
                "prefs.set",
                serde_json::json!({"key":"ai.mcp.enabled","value":"true"}),
            )
            .unwrap();
        s.bind_window_io();
        let held = Arc::new((Mutex::new(false), Condvar::new()));
        struct Release(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.0.lock().unwrap() = true;
                self.0.1.notify_all();
            }
        }
        let release = Release(held.clone());
        let (tx, entered) = mpsc::channel();
        let ui = std::thread::current().id();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, _| {
            if op == "mcp_start" {
                assert_ne!(std::thread::current().id(), ui);
                let _ = tx.send(());
                drop(
                    held.1
                        .wait_while(held.0.lock().unwrap(), |go| !*go)
                        .unwrap(),
                );
            }
            Ok(())
        }));
        let job = start_window(&mut s).unwrap();
        entered.recv_timeout(Duration::from_secs(30)).unwrap();
        {
            let _ui = xirius_core::io_test::forbid_ui_io();
            s.tick();
            s.idle();
            s.geometry(880.0, 600.0);
            assert!(job.poll().is_none());
        }
        drop(release);
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(got) = job.poll() {
                let (server, _jobs) = got.unwrap();
                assert!(server.mcp_url().starts_with("http://127.0.0.1:"));
                break;
            }
            assert!(Instant::now() < end, "MCP startup watchdog");
            std::thread::yield_now();
        }
    }
}
