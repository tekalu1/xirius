//! The one place that turns a tool call into a command run. The tool list is
//! generated from the command table (single definition): a command added to
//! the core is a tool here with no change. Both transports and the admin
//! endpoint go through it.

use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Map, Value, json};
use xirius_core::commands::jobs;
use xirius_core::session::{Confirm, RunOptions, RunResult};
use xirius_core::{Args, CmdError, ErrorCode, Registry, Risk, Session};

use crate::host::Host;
use crate::persist::result_json;

/// What the AI on the other end of the wire may reach.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// Only commands that look (`Risk::Read`).
    Read,
    /// Everything but the human-only commands. Deleting and saving still wait
    /// for a person's answer (OPEN-04); the session decides.
    Edit,
}

/// Commands a person may run through the admin endpoint: the answers to what
/// the AI is waiting on, and the looks that go with them. Nothing else.
pub const ADMIN_COMMANDS: &[&str] = &[
    "ai.decide",
    "ai.pending",
    "ai.request.accept",
    "ai.request.revert",
    "ai.request.status",
    "ai.activity",
    "history.undo",
    "history.redo",
    "history.list",
];

/// `note.transpose` → `note_transpose` (some clients reject dots).
pub fn tool_name(command: &str) -> String {
    command.replace('.', "_")
}

/// `job.list` and `job.cancel` (EXP-047), which the gateway answers without reaching the song:
/// a running bounce holds it, and these are what stop one.
fn job_command(tool: &str) -> Option<&'static str> {
    jobs::NAMES
        .into_iter()
        .find(|n| *n == tool || tool_name(n) == tool)
}

/// A failure to even start a command: wrong tool name or arguments shape.
#[derive(Debug)]
pub struct CallError(pub String);

#[derive(Clone)]
pub struct Gateway {
    host: Arc<dyn Host>,
    /// The session, when the gateway owns it behind a mutex ([`Gateway::new`]).
    session: Option<Arc<Mutex<Session>>>,
    author: String,
    scope: Scope,
    /// A window is there: the commands only the screen answers are tools too (the app's
    /// server). The CLI's server has none and leaves them out.
    screen: bool,
    save: bool,
    receipts: crate::receipts::Receipts,
    results: crate::results::Results,
}

impl Gateway {
    /// Wraps a session for an AI called `author`. The session now asks before
    /// deleting and before saving on the AI's behalf (OPEN-04).
    pub fn new(session: Arc<Mutex<Session>>, author: impl Into<String>, scope: Scope) -> Gateway {
        lock(&session).set_confirm(Confirm::OPEN_04);
        Gateway {
            host: session.clone(),
            session: Some(session),
            author: author.into(),
            scope,
            screen: false,
            save: true,
            receipts: Default::default(),
            results: Default::default(),
        }
    }

    /// Runs on a song someone else owns (the app's, through a
    /// [`bridge`](crate::host::bridge)). What the AI must ask first is the
    /// owner's to set on its session.
    pub fn hosted(host: Arc<dyn Host>, author: impl Into<String>, scope: Scope) -> Gateway {
        Gateway {
            host,
            session: None,
            author: author.into(),
            scope,
            screen: true,
            save: true,
            receipts: Default::default(),
            results: Default::default(),
        }
    }

    /// Says whether a window answers the screen-only commands (the app's server).
    pub fn with_screen(mut self, screen: bool) -> Gateway {
        self.screen = screen;
        self
    }

    pub fn has_screen(&self) -> bool {
        self.screen
    }

    /// Whether finished edits are written to the song folder (`--no-save` off).
    pub fn with_save(mut self, save: bool) -> Gateway {
        self.save = save;
        self
    }

    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// The session, when the gateway owns it ([`Gateway::new`]).
    pub fn session(&self) -> Option<&Arc<Mutex<Session>>> {
        self.session.as_ref()
    }

    fn registry<T>(&self, f: impl FnOnce(&Registry) -> T) -> T {
        let mut f = Some(f);
        let mut out = None;
        self.host.with_registry(&mut |r| {
            if let Some(f) = f.take() {
                out = Some(f(r));
            }
        });
        out.expect("the host calls back once")
    }

    fn reachable(&self, risk: Risk) -> bool {
        match (self.scope, risk) {
            (_, Risk::HumanOnly) => false,
            (Scope::Read, r) => r == Risk::Read,
            (Scope::Edit, _) => true,
        }
    }

    /// A command the AI may reach from here: reachable by its risk, and answered by a window
    /// when only the screen can do it.
    fn offered(&self, def: &xirius_core::CommandDef) -> bool {
        self.reachable(def.risk) && (!def.screen_only || self.screen)
    }

    /// The tools, one per reachable command: the command's JSON Schema with
    /// the tool name and the command name added.
    pub fn tools(&self) -> Vec<Value> {
        self.registry(|r| {
            r.all()
                .iter()
                .filter(|d| self.offered(d))
                .map(|d| {
                    let mut v = d.json_schema();
                    if d.risk != Risk::Read {
                        v["inputSchema"]["properties"]["request_key"] = json!({"type": "string", "description": "再送時にも同じ操作キーを使います"});
                        v["inputSchema"]["properties"]["wait_ms"] = json!({"type": "integer", "minimum": 0, "maximum": 30000, "description": "結果を待つ時間。0 は受付後すぐに返します"});
                    }
                    if d.args.iter().any(|a| a.name == "background") {
                        let background = &mut v["inputSchema"]["properties"]["background"];
                        background["default"] = json!(true);
                        background["description"] = json!(
                            "バックグラウンドで始め、すぐに job の id を返します（進行状況は export.status）。省略時は背景で始めます。false を渡すと終わるまで待ちます"
                        );
                    }
                    if let Value::Object(m) = &mut v {
                        m.insert("name".into(), json!(tool_name(d.name)));
                        let x = m
                            .get("annotations")
                            .and_then(|a| a.get("xirius"))
                            .cloned()
                            .unwrap_or(Value::Null);
                        let mut x = x.as_object().cloned().unwrap_or_default();
                        x.insert("command".into(), json!(d.name));
                        // The part of the schema clients read lives in `annotations`;
                        // ours goes under `_meta`.
                        m.insert("_meta".into(), json!({"xirius": x}));
                        if let Some(a) = m.get_mut("annotations").and_then(Value::as_object_mut) {
                            a.remove("xirius");
                        }
                    }
                    v
                })
                .collect()
        })
    }

    fn command_of(&self, tool: &str) -> Option<&'static str> {
        self.registry(|r| {
            r.all()
                .iter()
                .find(|d| (d.name == tool || tool_name(d.name) == tool) && self.offered(d))
                .map(|d| d.name)
        })
    }

    /// Whether the tool's command takes an argument called `arg`.
    pub fn takes(&self, tool: &str, arg: &str) -> bool {
        let Some(command) = self.command_of(tool) else {
            return false;
        };
        self.registry(|r| {
            r.get(command)
                .is_some_and(|d| d.args.iter().any(|a| a.name == arg))
        })
    }

    /// Runs a tool as the AI. `Ok((json, is_error))`: a command that fails is
    /// a result with a typed error code, not a protocol error.
    pub fn call(&self, tool: &str, raw: &Map<String, Value>) -> Result<(Value, bool), CallError> {
        if let Some(command) = job_command(tool) {
            // Answered without the song, which a running bounce holds (EXP-047).
            return Ok(match jobs::answer(command, &Args(raw.clone())) {
                Ok(out) => {
                    let r = RunResult {
                        command: command.to_string(),
                        message: out.message,
                        data: out.data,
                        changes: Default::default(),
                        dry_run: false,
                        forced_dry_run: false,
                        step: None,
                        decision: None,
                        events: Vec::new(),
                    };
                    (result_json(&r), false)
                }
                Err(e) => (error_json(&e), true),
            });
        }
        let Some(command) = self.command_of(tool) else {
            return Err(CallError(format!("ツール `{tool}` が見つかりません")));
        };
        let opts = RunOptions::ai(self.author.clone());
        let mut raw = raw.clone();
        if command == "ops.receipt" || command == "ops.get-result" {
            if let Err(error) = self
                .host
                .run("ai.permission.get", &Map::new(), &opts, false)
            {
                return Ok((error_json(&error), true));
            }
            let args = self.registry(|r| r.normalize(r.get(command).unwrap(), &raw));
            let result = args.and_then(|args| {
                let id = args.str("id")?;
                if command == "ops.receipt" {
                    self.receipts.get(id)
                } else {
                    self.results
                        .get_result(id, args.opt_i64("page").unwrap_or(0) as usize)
                }
            });
            return Ok(match result {
                Ok(value) => (value, false),
                Err(e) => (error_json(&e), true),
            });
        }
        // The command table declares which operations can run in the background. The MCP
        // server keeps starting them in the background when the caller says nothing (a render
        // can take minutes), but a caller that says `false` waits for the end.
        let takes_background = self.takes(tool, "background");
        let asked = raw.get("background").and_then(|value| match value {
            Value::Bool(b) => Some(*b),
            Value::String(s) if s == "true" => Some(true),
            Value::String(s) if s == "false" => Some(false),
            _ => None,
        });
        let foreground = takes_background && asked == Some(false);
        let preview = raw
            .get("dry_run")
            .is_some_and(|value| value == &Value::Bool(true) || value == "true");
        let asynchronous = takes_background && asked != Some(false) && !preview;
        if takes_background {
            raw.insert("background".into(), json!(!foreground));
        }
        let changes = self.registry(|r| r.get(command).unwrap().risk != Risk::Read);
        if !changes {
            let (value, error) = self.run(command, &raw, &opts);
            let id = crate::http::new_token().map_err(|e| CallError(e.to_string()))?;
            let value = self.results.store(&id, value);
            let failed = error || value["ok"] == false;
            return Ok((value, failed));
        }
        let id = match raw.remove("request_key") {
            Some(Value::String(id)) if !id.is_empty() && id.len() <= 256 => id,
            None => crate::http::new_token().map_err(|e| CallError(e.to_string()))?,
            _ => {
                return Ok((
                    invalid_args("操作キーは 1〜256 バイトの文字列で指定してください"),
                    true,
                ));
            }
        };
        let wait_given = raw.contains_key("wait_ms");
        let wait = match raw.remove("wait_ms") {
            None if asynchronous => 0,
            None => 30_000,
            Some(Value::Number(n)) if n.as_u64().is_some_and(|n| n <= 30_000) => {
                n.as_u64().unwrap()
            }
            _ => {
                return Ok((
                    invalid_args("wait_ms は 0〜30000 の整数で指定してください"),
                    true,
                ));
            }
        };
        let fingerprint = canonical(json!({"command": command, "args": raw})).to_string();
        let accepted = match self.receipts.accept(&id, command, fingerprint) {
            Ok(start) => start,
            Err(e) => return Ok((error_json(&e), true)),
        };
        if accepted {
            let gateway = self.clone();
            let id = id.clone();
            std::thread::spawn(move || {
                use xirius_core::protocol::ReceiptState;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // The run is made first: a command that only asks a person answers at
                    // once, and its receipt carries the decision rather than a bare "running".
                    let (value, _) = gateway.run(command, &raw, &opts);
                    if value["confirmation_required"] == true {
                        gateway.receipts.update(
                            &id,
                            ReceiptState::Accepted,
                            Some(gateway.results.store(&id, value.clone())),
                        );
                        let token = value["decision"]["token"].as_str().unwrap_or("");
                        loop {
                            if gateway.receipts.stopped() {
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            let (status, error) = gateway.run(
                                "ai.decision.get",
                                json!({"token": token}).as_object().unwrap(),
                                &RunOptions::human(),
                            );
                            if error || status["data"]["state"] == "completed" {
                                let mut value = if error {
                                    status
                                } else {
                                    status["data"]["result"].clone()
                                };
                                if value["data"]["accepted"] == false {
                                    value = error_json(&CmdError::new(
                                        ErrorCode::Denied,
                                        "操作は承認されませんでした",
                                    ));
                                }
                                gateway.receipts.update(
                                    &id,
                                    ReceiptState::Completed,
                                    Some(gateway.results.store(&id, value)),
                                );
                                break;
                            }
                        }
                    } else if value["data"]["state"] == "running"
                        && let Some(job) = value["data"]["job"].as_str()
                    {
                        gateway
                            .receipts
                            .update(&id, ReceiptState::Running, Some(value.clone()));
                        loop {
                            if gateway.receipts.stopped() {
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            let (mut status, error) = gateway.run(
                                "export.status",
                                json!({"job": job}).as_object().unwrap(),
                                &opts,
                            );
                            if error || status["data"]["state"] != "running" {
                                status["command"] = json!(command);
                                if let Some(hash) = value["data"].get("state_hash") {
                                    status["data"]["state_hash"] = hash.clone();
                                }
                                if !error && status["data"]["state"] != "done" {
                                    status["ok"] = json!(false);
                                    status["error"] = json!(CmdError::io(
                                        status["data"]["error"]
                                            .as_str()
                                            .unwrap_or("処理は完了しませんでした")
                                    ));
                                }
                                let value = gateway.results.store(&id, status);
                                gateway
                                    .receipts
                                    .update(&id, ReceiptState::Completed, Some(value));
                                break;
                            }
                        }
                    } else {
                        gateway.receipts.update(&id, ReceiptState::Running, None);
                        let value = gateway.results.store(&id, value);
                        gateway
                            .receipts
                            .update(&id, ReceiptState::Completed, Some(value));
                    }
                }));
                if result.is_err() || gateway.receipts.stopped() {
                    gateway.receipts.update(
                        &id,
                        ReceiptState::Unknown,
                        Some(error_json(&CmdError::io("処理の結果を確認できません"))),
                    );
                }
            });
        }
        let value = if foreground && !wait_given {
            // Said `background: false`: stay until it is done (or a person's answer is what
            // it is waiting on).
            self.receipts.wait_done(&id)
        } else if asynchronous {
            // Say at once whether the operation started, and hand over a person's answer
            // when that is what it waits on: never just `pending` for a confirmation.
            self.receipts
                .wait_started(&id, std::time::Duration::from_secs(30))
        } else {
            self.receipts
                .wait(&id, std::time::Duration::from_millis(wait))
        }
        .map_err(|e| CallError(e.to_string()))?;
        let error = value["ok"] == false;
        Ok((value, error))
    }

    pub fn events(&self) -> tokio::sync::broadcast::Receiver<Value> {
        self.receipts.subscribe()
    }

    pub fn shutdown(&self) {
        self.receipts.shutdown();
    }

    pub fn receipt(&self, id: &str) -> Result<Value, CmdError> {
        self.receipts.get(id)
    }

    pub fn receipted(&self, tool: &str) -> bool {
        if job_command(tool).is_some() {
            return false;
        }
        self.command_of(tool)
            .is_some_and(|name| self.registry(|r| r.get(name).unwrap().risk != Risk::Read))
    }

    /// Runs one of [`ADMIN_COMMANDS`] as a person.
    pub fn admin(
        &self,
        command: &str,
        raw: &Map<String, Value>,
    ) -> Result<(Value, bool), CallError> {
        let command = command.replace('_', ".");
        let Some(name) = ADMIN_COMMANDS.iter().find(|c| **c == command) else {
            return Err(CallError(format!(
                "`{command}` は管理用のエンドポイントからは実行できません（使用できるのは {}）",
                ADMIN_COMMANDS.join("・")
            )));
        };
        Ok(self.run(name, raw, &RunOptions::human()))
    }

    fn run(&self, command: &str, raw: &Map<String, Value>, opts: &RunOptions) -> (Value, bool) {
        match self.host.run(command, raw, opts, self.save) {
            Ok(r) => {
                for event in &r.events {
                    self.receipts.publish(event.clone());
                }
                (result_json(&r), false)
            }
            Err(e) => (error_json(&e), true),
        }
    }
}

pub fn error_json(e: &CmdError) -> Value {
    json!({"ok": false, "error": e})
}

pub fn invalid_args(why: &str) -> Value {
    error_json(&CmdError::new(ErrorCode::InvalidArgs, why))
}

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        value => value,
    }
}

/// A poisoned lock still holds a usable song: the session never half-applies
/// a step, so the reader goes on.
pub fn lock(s: &Mutex<Session>) -> MutexGuard<'_, Session> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}
