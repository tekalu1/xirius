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

/// Starts the loopback server for the studio's song and shows where it
/// listens in the AI's pop-over. Its runs arrive at the returned jobs; each
/// goes to [`Studio::ai_job`] on the window's thread. The server stops when
/// the returned [`Background`] is dropped. With the setting `ai.mcp.enabled` off, no server
/// starts (the setting is read when the window opens).
pub fn start(studio: &mut Studio) -> Option<(Background, Jobs)> {
    let enabled = studio
        .song
        .settings()
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
