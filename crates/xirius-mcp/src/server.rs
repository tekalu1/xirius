//! The MCP side: an rmcp `ServerHandler` over the [`Gateway`]. It holds no
//! logic of its own; the tool list and every call come from the command table.

use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};

use crate::gateway::Gateway;

type McpError = rmcp::ErrorData;

const INSTRUCTIONS: &str = "Xirius の曲を操作します。ツールは登録されたコマンドと 1 対 1 で対応します。\
dry_run を true にすると、曲は変更せず変更内容だけを返します。\
削除と保存はユーザーの承認を待ちます（結果に confirmation_required と decision が付きます）。\
複数の編集を 1 つの操作にまとめるには ai_batch を、作業コピーで試すには ai_request_begin から始めてください。";

#[derive(Clone)]
pub struct XiriusServer {
    gateway: Gateway,
}

impl XiriusServer {
    pub fn new(gateway: Gateway) -> XiriusServer {
        XiriusServer { gateway }
    }

    /// The tools as rmcp values (what `tools/list` returns).
    pub fn tool_list(&self) -> Result<Vec<Tool>, McpError> {
        self.gateway
            .tools()
            .into_iter()
            .map(|v| {
                serde_json::from_value::<Tool>(v).map_err(|e| {
                    McpError::internal_error(format!("ツールの定義を読み込めません: {e}"), None)
                })
            })
            .collect()
    }
}

impl XiriusServer {
    /// The server's instructions, with what only this kind of server can say: the app's
    /// server has a window (and a new address every launch), the CLI's has one song.
    fn instructions(&self) -> String {
        let mut text = INSTRUCTIONS.to_string();
        text.push_str("曲は画面で開きます。");
        if self.gateway.has_screen() {
            text.push_str(
                "接続先は起動のたびに変わります。AI の画面の「接続情報をコピー」で取ります。",
            );
        }
        text
    }
}

impl ServerHandler for XiriusServer {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("xirius", env!("CARGO_PKG_VERSION")))
            .with_instructions(self.instructions());
        info.meta = Some(rmcp::model::MetaObject(
            serde_json::json!({"xirius": xirius_core::protocol::Hello::default()})
                .as_object()
                .unwrap()
                .clone(),
        ));
        info
    }

    async fn initialize(
        &self,
        request: rmcp::model::InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::InitializeResult, McpError> {
        let meta = serde_json::to_value(&context.meta).unwrap_or_default();
        crate::protocol_check::check(&meta)
            .map_err(|e| McpError::invalid_params(e.message, None))?;
        context.peer.set_peer_info(request.clone());
        self.negotiate_initialize(&request)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.tool_list()?))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let mut raw = request.arguments.unwrap_or_default();
        let mut notifications = None;
        if self.gateway.receipted(&request.name) {
            let id = raw
                .entry("request_key")
                .or_insert(serde_json::json!(crate::http::new_token().map_err(
                    |error| McpError::internal_error(error.to_string(), None)
                )?))
                .clone();
            notifications = id.as_str().map(|id| (id.to_owned(), self.gateway.events()));
        }
        let gateway = self.gateway.clone();
        let (value, is_error) =
            tokio::task::spawn_blocking(move || gateway.call(&request.name, &raw))
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("コマンドの実行に失敗しました: {e}"), None)
                })?
                .map_err(|e| McpError::invalid_params(e.0, None))?;
        if let Some((id, mut events)) = notifications
            && value.get("receipt").is_some()
        {
            let gateway = self.gateway.clone();
            tokio::spawn(async move {
                let mut clock = tokio::time::interval(std::time::Duration::from_millis(500));
                loop {
                    let event = tokio::select! {
                        biased;
                        event = events.recv() => match event {
                            Ok(event) => event,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                let Ok(mut value) = gateway.receipt(&id) else { break; };
                                let receipt = value.as_object_mut().unwrap().remove("receipt").unwrap();
                                serde_json::json!({"receipt": receipt, "result": value})
                            },
                            Err(_) => break,
                        },
                        _ = clock.tick() => {
                            if context.peer.is_transport_closed() { break; }
                            let Ok(mut value) = gateway.receipt(&id) else { break; };
                            if value["receipt"]["state"] != "completed" && value["receipt"]["state"] != "unknown" { continue; }
                            let receipt = value.as_object_mut().unwrap().remove("receipt").unwrap();
                            serde_json::json!({"receipt": receipt, "result": value})
                        }
                    };
                    if event["receipt"]["id"] != id {
                        continue;
                    }
                    let done = event["receipt"]["state"] == "completed"
                        || event["receipt"]["state"] == "unknown";
                    let notification = rmcp::model::CustomNotification::new(
                        "notifications/xirius/operation",
                        Some(event),
                    );
                    if context
                        .peer
                        .send_notification(rmcp::model::ServerNotification::CustomNotification(
                            notification,
                        ))
                        .await
                        .is_err()
                        || done
                    {
                        break;
                    }
                }
            });
        }
        let result = if is_error {
            CallToolResult::structured_error(value)
        } else {
            CallToolResult::structured(value)
        };
        Ok(result.into())
    }
}
