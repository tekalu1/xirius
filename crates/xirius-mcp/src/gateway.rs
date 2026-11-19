//! The one place that turns a tool call into a command run. The commands come
//! from the command table (single definition): a command added to the core is
//! reachable here with no change. Both transports and the admin endpoint go
//! through it.
//!
//! MCP lists a few tools, not one per command (MCP-008): four entrances that run a
//! command by name, split by what an MCP client may ask a person before ([`entry`]), and
//! the commands every session uses ([`DIRECT`]). Which commands an AI may run through them
//! is decided in one place ([`Gateway::offered`]), as it was when every command was a tool.

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

/// The entrance for the commands that only look (`Risk::Read`): `readOnlyHint`.
pub const READ_TOOL: &str = "run_read";
/// The entrance for edits to the song (`Risk::Write`).
pub const EDIT_TOOL: &str = "run_edit";
/// The entrance for saving and writing files (`Risk::Guarded`): apart from the edits, so that
/// a client told to always allow `run_edit` does not save or write a file anywhere with it.
pub const GUARDED_TOOL: &str = "run_guarded";
/// The entrance for edits that remove something (`Risk::Destructive`): `destructiveHint`.
pub const DESTRUCTIVE_TOOL: &str = "run_destructive";
/// The four entrances, in the order `tools/list` gives them.
pub const ENTRANCES: [&str; 4] = [READ_TOOL, EDIT_TOOL, GUARDED_TOOL, DESTRUCTIVE_TOOL];

/// Commands that stay tools of their own: the two that find a command and its arguments,
/// and the one that runs several edits as one step. Everything else goes through [`entry`].
pub const DIRECT: &[&str] = &["ops.list", "ops.schema", "ai.batch"];

/// The entrance that runs a command of `risk`. A command has exactly one, and each entrance
/// runs one risk, so allowing an entrance in a client never allows more than that kind.
/// Human-only commands have none.
pub fn entry(risk: Risk) -> Option<&'static str> {
    match risk {
        Risk::Read => Some(READ_TOOL),
        Risk::Write => Some(EDIT_TOOL),
        Risk::Guarded => Some(GUARDED_TOOL),
        Risk::Destructive => Some(DESTRUCTIVE_TOOL),
        Risk::HumanOnly => None,
    }
}

/// The `tools/call` params that run `command` (a command name, or its tool name) with `args`:
/// the command's own tool when it is one of [`DIRECT`], else its entrance with `command` and
/// `args`. A name the table does not know, or a human-only command, is sent as a tool name of
/// its own, which the server refuses as before.
pub fn call_params(registry: &Registry, command: &str, args: Value) -> Value {
    let def = registry
        .all()
        .iter()
        .find(|d| d.name == command || tool_name(d.name) == command);
    match def {
        Some(d) if DIRECT.contains(&d.name) => {
            json!({"name": tool_name(d.name), "arguments": args})
        }
        Some(d) if entry(d.risk).is_some() => json!({
            "name": entry(d.risk),
            "arguments": {"command": d.name, "args": args},
        }),
        _ => json!({"name": tool_name(command), "arguments": args}),
    }
}

/// What an entrance says about itself in `tools/list`.
fn entry_tool(name: &'static str) -> Value {
    let (description, read_only, destructive) = match name {
        READ_TOOL => (
            "見るだけのコマンド（risk が read）を名前で実行します。一覧・状態・選択・表示の切り替え・job.list などです。\
             コマンドは ops_list で、引数は ops_schema で調べます",
            true,
            false,
        ),
        EDIT_TOOL => (
            "曲を編集するコマンド（risk が write）を名前で実行します。保存と書き出しは run_guarded、削除は run_destructive です。\
             args には dry_run・request_key・wait_ms も渡せます。コマンドは ops_list で、引数は ops_schema で調べます",
            false,
            false,
        ),
        GUARDED_TOOL => (
            "曲の保存とファイルの書き出し（risk が guarded）を名前で実行します。「編集と保存・書き出し」の権限か、ユーザーの承認が必要です（結果に confirmation_required と decision が付きます）。\
             args には dry_run・request_key・wait_ms も渡せます。コマンドは ops_list で、引数は ops_schema で調べます",
            false,
            false,
        ),
        _ => (
            "削除を伴うコマンド（risk が destructive）を名前で実行します。ユーザーの承認を待つことがあります（結果に confirmation_required と decision が付きます）。\
             args には dry_run・request_key・wait_ms も渡せます。コマンドは ops_list で、引数は ops_schema で調べます",
            false,
            true,
        ),
    };
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "コマンド名（例: note.transpose。note_transpose とも書けます）"},
                "args": {"type": "object", "description": "コマンドの引数（ops_schema の inputSchema のとおり）。省略時は引数なし"},
            },
            "required": ["command"],
            "additionalProperties": false,
        },
        "annotations": {"readOnlyHint": read_only, "destructiveHint": destructive},
    })
}

/// `job.list` and `job.cancel` (EXP-047), which the gateway answers without reaching the song:
/// a running bounce holds it, and these are what stop one.
fn job_command(name: &str) -> Option<&'static str> {
    jobs::NAMES
        .into_iter()
        .find(|n| *n == name || tool_name(n) == name)
}

/// A failure to even start a command: a tool or command that is not there.
#[derive(Debug)]
pub struct CallError(pub String);

/// Why an entrance did not run a call ([`Gateway::resolve`]).
#[derive(Debug)]
pub enum Refusal {
    /// No such tool, or no such command from here (unknown, human-only, out of the scope, the
    /// screen's without one): the protocol error it always was, which tells nothing of what
    /// exists.
    NotFound(CallError),
    /// A call the model can put right: a command sent to the wrong entrance, or arguments of
    /// the wrong shape. A tool result with `isError`, saying what to send instead.
    Mistake(Value),
}

/// A [`Refusal::Mistake`]: `invalid_args` with the reason, and the entrance to use when the
/// command was sent to another.
fn mistake(why: String, tool: Option<&str>) -> Refusal {
    let mut v = invalid_args(&why);
    if let Some(tool) = tool {
        v["tool"] = json!(tool);
    }
    Refusal::Mistake(v)
}

/// What a command's group needs to say whether the AI may reach it, kept from the table when
/// the gateway is made: the server's instructions then need no lock on the song.
#[derive(Clone, Debug)]
struct Kind {
    group: String,
    risk: Risk,
    screen_only: bool,
}

fn kinds(host: &dyn Host) -> Arc<Vec<Kind>> {
    let mut out = Vec::new();
    host.with_registry(&mut |r| {
        let mut seen = std::collections::BTreeSet::new();
        out = r
            .all()
            .iter()
            .filter(|d| seen.insert((d.target().to_string(), d.risk, d.screen_only)))
            .map(|d| Kind {
                group: d.target().to_string(),
                risk: d.risk,
                screen_only: d.screen_only,
            })
            .collect();
    });
    Arc::new(out)
}

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
    /// The table's groups, by risk and screen ([`Kind`]).
    kinds: Arc<Vec<Kind>>,
}

impl Gateway {
    /// Wraps a session for an AI called `author`. The session now asks before
    /// deleting and before saving on the AI's behalf (OPEN-04).
    pub fn new(session: Arc<Mutex<Session>>, author: impl Into<String>, scope: Scope) -> Gateway {
        lock(&session).set_confirm(Confirm::OPEN_04);
        Gateway {
            kinds: kinds(&*session),
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
            kinds: kinds(&*host),
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

    /// Every command the AI may run from here, with the schema its arguments follow: the
    /// command's JSON Schema with the tool name, the command name and the entrance that runs
    /// it added. `ops.schema` answers with these over MCP.
    pub fn commands(&self) -> Vec<Value> {
        self.registry(|r| {
            r.all()
                .iter()
                .filter(|d| self.offered(d))
                .map(command_schema)
                .collect()
        })
    }

    /// What `tools/list` returns: the entrances that run at least one command from here, then
    /// [`DIRECT`]. Never the screen's own commands (MCP-001): with a window they are run
    /// through an entrance by name.
    pub fn tools(&self) -> Vec<Value> {
        self.registry(|r| {
            let offered: Vec<_> = r.all().iter().filter(|d| self.offered(d)).collect();
            let mut tools: Vec<Value> = ENTRANCES
                .into_iter()
                .filter(|name| offered.iter().any(|d| entry(d.risk) == Some(*name)))
                .map(entry_tool)
                .collect();
            for name in DIRECT {
                let Some(d) = offered.iter().find(|d| d.name == *name && !d.screen_only) else {
                    continue;
                };
                let mut v = command_schema(d);
                if d.name == "ops.list" {
                    v["description"] = json!(format!(
                        "{}。group（{}）か grep で絞ってください。各行の tool が実行に使うツールです",
                        d.summary,
                        self.groups().join("・")
                    ));
                }
                tools.push(v);
            }
            tools
        })
    }

    /// The groups of the commands the AI may run from here (`note` for `note.quantize`). Kept
    /// from when the gateway was made, so it takes no lock: `initialize` answers while a bounce
    /// holds the song.
    pub fn groups(&self) -> Vec<String> {
        let set: std::collections::BTreeSet<&str> = self
            .kinds
            .iter()
            .filter(|k| self.reachable(k.risk) && (!k.screen_only || self.screen))
            .map(|k| k.group.as_str())
            .collect();
        set.into_iter().map(str::to_string).collect()
    }

    /// The command `name` (a command name, or its tool name) stands for, when the AI may run it
    /// from here.
    fn command_of(&self, name: &str) -> Option<&'static str> {
        self.registry(|r| {
            r.all()
                .iter()
                .find(|d| (d.name == name || tool_name(d.name) == name) && self.offered(d))
                .map(|d| d.name)
        })
    }

    /// Turns a call of a listed tool into the command it runs and that command's arguments.
    /// An entrance runs only the commands of its own kind ([`entry`]). A command the AI may not
    /// reach from here, or that does not exist, is the same "not found" protocol error as
    /// before; the wrong entrance or arguments of the wrong shape come back as a result the
    /// model can act on ([`Refusal::Mistake`]).
    pub fn resolve(
        &self,
        tool: &str,
        raw: Map<String, Value>,
    ) -> Result<(&'static str, Map<String, Value>), Refusal> {
        if !ENTRANCES.contains(&tool) {
            let direct = DIRECT
                .iter()
                .find(|c| **c == tool || tool_name(c) == tool)
                .and_then(|c| self.command_of(c));
            return match direct {
                Some(command) => Ok((command, raw)),
                None => Err(Refusal::NotFound(CallError(format!(
                    "ツール `{tool}` が見つかりません"
                )))),
            };
        }
        if let Some(key) = raw.keys().find(|k| *k != "command" && *k != "args") {
            return Err(mistake(
                format!(
                    "{tool} の引数は command と args です（`{key}` はコマンドの引数として args に入れてください）"
                ),
                None,
            ));
        }
        let Some(name) = raw.get("command").and_then(Value::as_str) else {
            return Err(mistake(
                format!("{tool} には command にコマンド名（例: note.transpose）を渡してください"),
                None,
            ));
        };
        let args = match raw.get("args") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(args)) => args.clone(),
            Some(_) => {
                return Err(mistake(
                    format!(
                        "{tool} の args はオブジェクト（例: {{\"ids\": [\"clp_1\"]}}）で渡してください"
                    ),
                    None,
                ));
            }
        };
        let wrong = |command: &str, right: &'static str| {
            mistake(
                format!(
                    "{command} は {tool} では実行できません。{right} に同じ command と args を渡してください"
                ),
                Some(right),
            )
        };
        if let Some(job) = job_command(name) {
            // Answered without the registry, which waits for the song a running bounce holds
            // (EXP-047). Both are read commands, reachable in every scope and without a screen.
            return match tool {
                READ_TOOL => Ok((job, args)),
                _ => Err(wrong(job, READ_TOOL)),
            };
        }
        let Some(command) = self.command_of(name) else {
            return Err(Refusal::NotFound(CallError(format!(
                "コマンド `{name}` が見つかりません"
            ))));
        };
        // Reached here, the command is offered, so it is not human-only and has an entrance.
        let right = self
            .registry(|r| r.get(command).and_then(|d| entry(d.risk)))
            .expect("an offered command has an entrance");
        if right != tool {
            return Err(wrong(command, right));
        }
        Ok((command, args))
    }

    /// `ops.list` and `ops.schema` over MCP say only what this connection may run, and which
    /// tool runs it.
    fn narrow(&self, command: &str, asked: Option<&str>, mut value: Value) -> Value {
        if value["ok"] != true {
            return value;
        }
        let data = value["data"].take();
        let narrowed = self.registry(|r| {
            let offered = |name: &str| r.get(name).filter(|d| self.offered(d));
            match (command, data) {
                ("ops.list", Value::Array(rows)) => {
                    let rows: Vec<Value> = rows
                        .into_iter()
                        .filter_map(|mut v| {
                            let d = offered(v["name"].as_str()?)?;
                            v["tool"] = json!(entry(d.risk));
                            Some(v)
                        })
                        .collect();
                    Ok((format!("コマンド {} 件", rows.len()), Value::Array(rows)))
                }
                ("ops.schema", Value::Array(rows)) => {
                    let rows: Vec<Value> = rows
                        .into_iter()
                        .filter_map(|v| offered(v["name"].as_str()?).map(command_schema))
                        .collect();
                    Ok((format!("Schema {} 件", rows.len()), Value::Array(rows)))
                }
                ("ops.schema", one) => {
                    let name = one["name"].as_str().unwrap_or_default();
                    // Said with the name asked for, as the table says a name it does not know.
                    let asked = asked.unwrap_or(name);
                    match offered(name) {
                        Some(d) => Ok((format!("{} の Schema", d.name), command_schema(d))),
                        None => Err(CmdError::not_found(format!(
                            "コマンド {asked} が見つかりません"
                        ))),
                    }
                }
                (_, data) => Ok((value["message"].as_str().unwrap_or("").to_string(), data)),
            }
        });
        match narrowed {
            Ok((message, data)) => {
                value["message"] = json!(message);
                value["data"] = data;
                value
            }
            Err(e) => error_json(&e),
        }
    }

    /// Whether `command` (a command name, or its tool name) takes an argument called `arg`.
    pub fn takes(&self, command: &str, arg: &str) -> bool {
        let Some(command) = self.command_of(command) else {
            return false;
        };
        self.registry(|r| {
            r.get(command)
                .is_some_and(|d| d.args.iter().any(|a| a.name == arg))
        })
    }

    /// Runs `command` (a command name, or its tool name) as the AI, with its own arguments
    /// `raw` (what an entrance got as `args`). `Ok((json, is_error))`: a command that fails is
    /// a result with a typed error code, not a protocol error. A command the AI may not reach
    /// from here is `Err`, as an unknown one is.
    pub fn call(
        &self,
        command: &str,
        raw: &Map<String, Value>,
    ) -> Result<(Value, bool), CallError> {
        if let Some(command) = job_command(command) {
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
        let name = command;
        let Some(command) = self.command_of(name) else {
            return Err(CallError(format!("コマンド `{name}` が見つかりません")));
        };
        let opts = RunOptions::ai(self.author.clone());
        let mut raw = raw.clone();
        if command == "ops.schema" {
            // Over MCP, every schema at once is half a megabyte: ask for one command or a group.
            let given = |key: &str| {
                raw.get(key)
                    .and_then(Value::as_str)
                    .is_some_and(|v| !v.trim().is_empty())
            };
            if !given("name") && !given("group") {
                return Ok((
                    invalid_args(&format!(
                        "ops_schema には name（コマンド名。例: note.transpose）か group（{}）を渡してください",
                        self.groups().join("・")
                    )),
                    true,
                ));
            }
        }
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
        let takes_background = self.takes(command, "background");
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
            let value = match command {
                "ops.list" | "ops.schema" => {
                    self.narrow(command, raw.get("name").and_then(Value::as_str), value)
                }
                _ => value,
            };
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

    /// Whether a run of `command` gets a receipt (it changes something), so the server sends
    /// its progress.
    pub fn receipted(&self, command: &str) -> bool {
        if job_command(command).is_some() {
            return false;
        }
        self.command_of(command)
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

/// A command's schema as the AI reads it over MCP: the tool name, the command name and the
/// entrance that runs it under `_meta.xirius`, and a background that starts by default.
fn command_schema(d: &xirius_core::CommandDef) -> Value {
    let mut v = d.json_schema();
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
        x.insert("tool".into(), json!(entry(d.risk)));
        // The part of the schema clients read lives in `annotations`; ours goes under `_meta`.
        m.insert("_meta".into(), json!({"xirius": x}));
        if let Some(a) = m.get_mut("annotations").and_then(Value::as_object_mut) {
            a.remove("xirius");
        }
    }
    v
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
