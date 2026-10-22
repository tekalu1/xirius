//! The command definition. One `CommandDef` carries everything the UI, the
//! CLI and MCP need: name, argument types, description, whether it goes into
//! the history and how risky it is. Nothing else lists commands by hand
//! (ARC-002 / ARC-014 / ARC-016).
//!
//! Names follow `<target>.<verb>` (`context-matrix.md` §1): `track.add.audio`,
//! `note.transpose`, `track.mute` (a toggle: `--on` / `--off`, flips when
//! omitted), `track.volume.set` (set a value).

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::clipboard::Clipboard;
use crate::error::{CmdError, CmdResult};
use crate::id::{AnyId, IdKind};
use crate::model::Project;
use crate::session::SessionState;
use crate::store::Store;

/// Who ran a command. Kept on every history step.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Author {
    Human,
    Ai(String),
}

impl Author {
    pub fn is_ai(&self) -> bool {
        matches!(self, Author::Ai(_))
    }

    /// The label of a history row: `note.quantize` for a human,
    /// `AI · note.quantize` for the AI.
    pub fn row_label(&self, command: &str) -> String {
        match self {
            Author::Human => command.to_string(),
            Author::Ai(_) => format!("AI · {command}"),
        }
    }
}

/// How dangerous a command is; the AI's permission decides which it may run.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Looks only, or changes selection / view.
    Read,
    /// Edits the song (undoable).
    Write,
    /// Edits the song and removes something (a track, a clip, notes, ...). The
    /// AI's edits of this kind can be held back for a person's answer.
    Destructive,
    /// Saves or writes files: the AI needs the `save` permission.
    Guarded,
    /// Only a human may run it.
    HumanOnly,
}

#[derive(Clone, PartialEq, Debug)]
pub enum ArgType {
    Bool,
    Int,
    Float,
    Text,
    Id(IdKind),
    Ids(IdKind),
    Choice(Vec<&'static str>),
    /// `bar.beat.tick` or a tick count.
    Position,
    /// `1/16`, `1/8t`, `1/4d` or a tick count.
    Grid,
    /// A list of objects (JSON text is accepted too, for the command line).
    Objects,
}

#[derive(Clone, Debug)]
pub struct ArgDef {
    pub name: &'static str,
    pub ty: ArgType,
    pub required: bool,
    pub default: Option<Value>,
    pub help: &'static str,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// The range is shown, but a value outside it is held at its end instead of refused
    /// (UI-311).
    pub clamp: bool,
    /// Older or alternative spellings of the name (`--tracks` for `--ids`). They are accepted
    /// wherever the name is, and shown nowhere: the schema, the palette and the docs list `name`.
    pub aliases: &'static [&'static str],
    /// Given only by name (`--overlap trim`): the CLI's bare words never fill it, so a word
    /// too many stays a usage error.
    pub named_only: bool,
}

impl ArgDef {
    fn new(name: &'static str, ty: ArgType, help: &'static str) -> ArgDef {
        ArgDef {
            name,
            ty,
            required: false,
            default: None,
            help,
            min: None,
            max: None,
            clamp: false,
            aliases: &[],
            named_only: false,
        }
    }
    pub fn text(name: &'static str, help: &'static str) -> ArgDef {
        Self::new(name, ArgType::Text, help)
    }
    pub fn int(name: &'static str, help: &'static str) -> ArgDef {
        Self::new(name, ArgType::Int, help)
    }
    pub fn float(name: &'static str, help: &'static str) -> ArgDef {
        Self::new(name, ArgType::Float, help)
    }
    pub fn flag(name: &'static str, help: &'static str) -> ArgDef {
        Self::new(name, ArgType::Bool, help)
    }
    pub fn id(name: &'static str, kind: IdKind, help: &'static str) -> ArgDef {
        Self::new(name, ArgType::Id(kind), help)
    }
    pub fn ids(kind: IdKind, help: &'static str) -> ArgDef {
        Self::new("ids", ArgType::Ids(kind), help)
    }
    /// A list of ids under a name of its own (`--sources`), where `--ids` is what the command
    /// acts on.
    pub fn id_list(name: &'static str, kind: IdKind, help: &'static str) -> ArgDef {
        Self::new(name, ArgType::Ids(kind), help)
    }
    pub fn choice(name: &'static str, options: &[&'static str], help: &'static str) -> ArgDef {
        Self::new(name, ArgType::Choice(options.to_vec()), help)
    }
    pub fn objects(name: &'static str, help: &'static str) -> ArgDef {
        Self::new(name, ArgType::Objects, help)
    }
    pub fn pos(name: &'static str, help: &'static str) -> ArgDef {
        Self::new(name, ArgType::Position, help)
    }
    pub fn grid(name: &'static str, help: &'static str) -> ArgDef {
        Self::new(name, ArgType::Grid, help)
    }
    /// Also accepts these names for the argument.
    pub fn alias(mut self, names: &'static [&'static str]) -> ArgDef {
        self.aliases = names;
        self
    }
    /// Only by name: see [`ArgDef::named_only`].
    pub fn named(mut self) -> ArgDef {
        self.named_only = true;
        self
    }
    pub fn req(mut self) -> ArgDef {
        self.required = true;
        self
    }
    pub fn default(mut self, v: Value) -> ArgDef {
        self.default = Some(v);
        self
    }
    pub fn range(mut self, min: f64, max: f64) -> ArgDef {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// A range the schema shows, whose outside is held at its end rather than refused.
    pub fn clamp_range(mut self, min: f64, max: f64) -> ArgDef {
        self.min = Some(min);
        self.max = Some(max);
        self.clamp = true;
        self
    }

    fn json_schema(&self) -> Value {
        let mut s = match &self.ty {
            ArgType::Bool => json!({"type": "boolean"}),
            ArgType::Int => json!({"type": "integer"}),
            ArgType::Float => json!({"type": "number"}),
            ArgType::Text => json!({"type": "string"}),
            ArgType::Id(k) => {
                json!({"type": "string", "description": format!("{} の ID", k.label())})
            }
            ArgType::Ids(k) => json!({
                "type": "array",
                "items": {"type": "string"},
                "description": format!("{} の ID のリスト", k.label()),
            }),
            ArgType::Choice(o) => json!({"type": "string", "enum": o}),
            ArgType::Position => json!({
                "oneOf": [{"type": "integer"}, {"type": "string"}],
                "description": "小節.拍.tick（例: 3.1.0）または tick 数",
            }),
            ArgType::Grid => json!({
                "oneOf": [{"type": "integer"}, {"type": "string"}],
                "description": "1/16・1/8t（3 連符）・1/4d（付点）または tick 数",
            }),
            ArgType::Objects => json!({"type": "array", "items": {"type": "object"}}),
        };
        let obj = s.as_object_mut().expect("object");
        let help = if self.help.is_empty() {
            None
        } else {
            Some(match obj.get("description").and_then(Value::as_str) {
                Some(d) => format!("{}（{d}）", self.help),
                None => self.help.to_string(),
            })
        };
        if let Some(h) = help {
            obj.insert("description".into(), Value::String(h));
        }
        if let Some(m) = self.min {
            obj.insert("minimum".into(), json!(m));
        }
        if let Some(m) = self.max {
            obj.insert("maximum".into(), json!(m));
        }
        if let Some(d) = &self.default {
            obj.insert("default".into(), d.clone());
        }
        s
    }
}

/// What a command returns.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Output {
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
    /// What a cut leaves for the session's clipboard; the session takes it (never serialized).
    #[serde(skip)]
    pub clipboard: Option<Clipboard>,
}

impl Output {
    pub fn msg(message: impl Into<String>) -> Output {
        Output {
            message: message.into(),
            data: Value::Null,
            clipboard: None,
        }
    }
    pub fn with(message: impl Into<String>, data: Value) -> Output {
        Output {
            message: message.into(),
            data,
            clipboard: None,
        }
    }
    /// The same answer, also leaving `c` on the clipboard.
    pub fn keeping(mut self, c: Clipboard) -> Output {
        self.clipboard = Some(c);
        self
    }
}

/// Everything an edit command may look at besides the project.
pub struct EditCtx<'a> {
    pub author: &'a Author,
    pub store: Option<&'a Store>,
    /// What `edit.copy` / `edit.cut` left (the session's clipboard).
    pub clipboard: Option<&'a Clipboard>,
}

pub type EditFn = fn(&mut Project, &EditCtx<'_>, &Args) -> CmdResult<Output>;
pub type ReadFn = fn(&Project, &mut SessionState, &Registry, &Args) -> CmdResult<Output>;

#[derive(Clone, Copy)]
pub enum Handler {
    /// Changes the song; the session wraps it in a history step.
    Edit(EditFn),
    /// Looks at the song or changes only selection / view.
    Read(ReadFn),
    /// History and file commands, run by the session itself.
    Session,
}

#[derive(Clone)]
pub struct CommandDef {
    pub name: &'static str,
    pub summary: &'static str,
    pub args: Vec<ArgDef>,
    /// Whether a run is recorded as a history step.
    pub history: bool,
    pub risk: Risk,
    pub handler: Handler,
    /// Only a window answers it: a session with no screen (the CLI, the CLI's MCP server)
    /// says it cannot run instead of calling it implemented.
    pub screen_only: bool,
}

impl CommandDef {
    /// Marks a command only the screen answers.
    pub fn screen_only(mut self) -> CommandDef {
        self.screen_only = true;
        self
    }

    /// The short name a menu or the palette shows (「ノートをクオンタイズ」);
    /// [`CommandDef::summary`] is the description.
    pub fn label(&self) -> Option<&'static str> {
        crate::commands::labels::label(self.name)
    }

    pub fn icon(&self) -> &'static str {
        crate::commands::visual::icon(self.name)
    }

    pub fn bindings(&self) -> impl Iterator<Item = &'static crate::commands::keys::Entry> + '_ {
        crate::commands::keys::ENTRIES
            .iter()
            .filter(|e| e.command.split_whitespace().next() == Some(self.name))
    }

    pub fn json_schema(&self) -> Value {
        let mut props = Map::new();
        for a in &self.args {
            props.insert(a.name.to_string(), a.json_schema());
        }
        if self.risk != Risk::Read {
            props.insert(
                "request_key".into(),
                json!({"type": "string", "description": "再送時にも同じ操作キーを使います"}),
            );
            props.insert("wait_ms".into(), json!({"type": "integer", "minimum": 0, "maximum": 30000, "description": "結果を待つ時間。0 は受付後すぐに返します"}));
            props.insert(
                "dry_run".into(),
                json!({"type": "boolean", "description": "曲は変更せず、変更内容だけを返します"}),
            );
        }
        let required: Vec<&str> = self
            .args
            .iter()
            .filter(|a| a.required)
            .map(|a| a.name)
            .collect();
        json!({
            "name": self.name,
            "description": self.summary,
            "inputSchema": {
                "type": "object",
                "properties": props,
                "required": required,
                "additionalProperties": false,
            },
            "annotations": {
                "readOnlyHint": self.risk == Risk::Read,
                "destructiveHint": self.risk == Risk::Destructive,
                "xirius": {"history": self.history, "risk": self.risk, "screen_only": self.screen_only, "label": self.label(), "icon": self.icon(), "keys": self.bindings().map(|e| json!({"context": e.context, "command": e.command, "keys": e.keys})).collect::<Vec<_>>()},
            },
        })
    }

    /// The first segment of the name: `note` for `note.quantize`.
    pub fn target(&self) -> &str {
        self.name.split('.').next().unwrap_or("")
    }
}

/// The one table of commands.
#[derive(Clone)]
pub struct Registry {
    defs: Vec<CommandDef>,
    index: BTreeMap<&'static str, usize>,
    /// Older names of commands: they find the command, and `all()` never lists them.
    aliases: BTreeMap<&'static str, usize>,
}

impl Registry {
    pub fn from_defs(defs: Vec<CommandDef>) -> Registry {
        let mut index = BTreeMap::new();
        for (i, d) in defs.iter().enumerate() {
            let dup = index.insert(d.name, i);
            assert!(dup.is_none(), "コマンド {} が重複しています", d.name);
        }
        Registry {
            defs,
            index,
            aliases: BTreeMap::new(),
        }
    }

    /// Makes each older name of `pairs` (`(old, current)`) reach the command it stands for.
    pub fn with_aliases(mut self, pairs: &[(&'static str, &'static str)]) -> Registry {
        for (old, current) in pairs {
            let i = *self
                .index
                .get(current)
                .unwrap_or_else(|| panic!("別名 {old} の先 {current} が表にありません"));
            assert!(
                !self.index.contains_key(old),
                "別名 {old} は表のコマンドと重なっています"
            );
            let dup = self.aliases.insert(old, i);
            assert!(dup.is_none(), "別名 {old} が重複しています");
        }
        self
    }

    /// Every command the app ships.
    pub fn standard() -> Registry {
        Registry::from_defs(crate::commands::all()).with_aliases(crate::commands::ALIASES)
    }

    pub fn get(&self, name: &str) -> Option<&CommandDef> {
        self.index
            .get(name)
            .or_else(|| self.aliases.get(name))
            .map(|i| &self.defs[*i])
    }

    /// The name a command is listed under: `name` itself, or what an older name stands for.
    pub fn canonical<'a>(&'a self, name: &'a str) -> &'a str {
        self.get(name).map_or(name, |d| d.name)
    }

    pub fn all(&self) -> &[CommandDef] {
        &self.defs
    }

    /// The short name of a command of this registry.
    pub fn label(&self, name: &str) -> Option<&'static str> {
        self.get(name).and_then(CommandDef::label)
    }

    pub fn find(&self, grep: Option<&str>) -> Vec<&CommandDef> {
        let g = grep.map(str::to_lowercase);
        self.defs
            .iter()
            .filter(|d| {
                g.as_ref().is_none_or(|g| {
                    d.name.to_lowercase().contains(g) || d.summary.to_lowercase().contains(g)
                })
            })
            .collect()
    }

    /// The MCP `tools/list` payload.
    pub fn tool_schemas(&self) -> Vec<Value> {
        self.defs.iter().map(CommandDef::json_schema).collect()
    }

    /// `raw` with every alias of an argument written under the argument's own name. Saying one
    /// argument twice (`--ids` and `--id`) is refused.
    ///
    /// `--ids` is the one way to name the target: a command whose target is a single `--id` takes
    /// `--ids` with that one id too.
    pub fn spelled(def: &CommandDef, raw: &Map<String, Value>) -> CmdResult<Map<String, Value>> {
        let mut out = raw.clone();
        let single = def
            .args
            .iter()
            .any(|a| a.name == "id" && matches!(a.ty, ArgType::Id(_)))
            && !def.args.iter().any(|a| a.name == "ids");
        if single && let Some(v) = out.get("ids").filter(|v| !v.is_null()).cloned() {
            let one = match &v {
                Value::Array(xs) if xs.len() == 1 => xs[0].clone(),
                Value::Array(_) => {
                    return Err(CmdError::invalid(format!(
                        "{} の対象は 1 つだけです。--ids に 1 つだけ指定してください",
                        def.name
                    )));
                }
                other => other.clone(),
            };
            if out.get("id").is_some_and(|x| !x.is_null()) {
                return Err(CmdError::invalid(format!(
                    "{} の --ids と --id は同じ引数です。片方だけを指定してください",
                    def.name
                )));
            }
            out.remove("ids");
            out.insert("id".to_string(), one);
        }
        for a in &def.args {
            for alias in a.aliases {
                let Some(v) = out.remove(*alias) else {
                    continue;
                };
                if v.is_null() {
                    continue;
                }
                if out.get(a.name).is_some_and(|x| !x.is_null()) {
                    return Err(CmdError::invalid(format!(
                        "{} の --{} と --{alias} は同じ引数です。片方だけを指定してください",
                        def.name, a.name
                    )));
                }
                out.insert(a.name.to_string(), v);
            }
        }
        Ok(out)
    }

    /// Checks `raw` against the definition and fills defaults. Strings are
    /// accepted where a number or boolean is expected, so a command line can
    /// be passed through as is.
    pub fn normalize(&self, def: &CommandDef, raw: &Map<String, Value>) -> CmdResult<Args> {
        let mut out = Map::new();
        let raw = &Self::spelled(def, raw)?;
        for k in raw.keys() {
            if k == "dry_run" {
                continue;
            }
            if !def.args.iter().any(|a| a.name == k) {
                let valid: Vec<&str> = def.args.iter().map(|a| a.name).collect();
                return Err(CmdError::invalid(format!(
                    "{} に `{k}` という引数はありません（使用できる引数: {}）",
                    def.name,
                    if valid.is_empty() {
                        "なし".to_string()
                    } else {
                        valid.join("・")
                    }
                )));
            }
        }
        for a in &def.args {
            match raw.get(a.name) {
                Some(v) if !v.is_null() => {
                    out.insert(a.name.to_string(), coerce(def.name, a, v)?);
                }
                _ => {
                    if let Some(d) = &a.default {
                        out.insert(a.name.to_string(), d.clone());
                    } else if a.required {
                        return Err(CmdError::invalid(format!(
                            "{} には --{} が必要です",
                            def.name, a.name
                        )));
                    }
                }
            }
        }
        Ok(Args(out))
    }
}

fn coerce(cmd: &str, a: &ArgDef, v: &Value) -> CmdResult<Value> {
    let bad = |what: &str| {
        CmdError::invalid(format!(
            "{cmd} の --{} には{what}を指定してください",
            a.name
        ))
    };
    let checked = |n: f64| -> CmdResult<()> {
        if a.clamp {
            return Ok(());
        }
        if a.min.is_some_and(|m| n < m) || a.max.is_some_and(|m| n > m) {
            return Err(CmdError::range(format!(
                "{cmd} の --{} は {}〜{} の範囲で指定してください（{n} は範囲外です）",
                a.name,
                a.min.map_or("-∞".into(), |m| m.to_string()),
                a.max.map_or("∞".into(), |m| m.to_string())
            )));
        }
        Ok(())
    };
    Ok(match &a.ty {
        ArgType::Bool => match v {
            Value::Bool(_) => v.clone(),
            Value::String(s) => match s.as_str() {
                "true" | "on" | "1" | "yes" => Value::Bool(true),
                "false" | "off" | "0" | "no" => Value::Bool(false),
                _ => return Err(bad("真偽値（true / false）")),
            },
            _ => return Err(bad("真偽値（true / false）")),
        },
        ArgType::Int => {
            let n = match v {
                Value::Number(n) => n
                    .as_i64()
                    .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)),
                Value::String(s) => s.trim().parse::<i64>().ok(),
                _ => None,
            }
            .ok_or_else(|| bad("整数"))?;
            checked(n as f64)?;
            let n = if a.clamp {
                n.clamp(
                    a.min.map_or(i64::MIN, |m| m as i64),
                    a.max.map_or(i64::MAX, |m| m as i64),
                )
            } else {
                n
            };
            json!(n)
        }
        ArgType::Float => {
            let n = match v {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().parse::<f64>().ok(),
                _ => None,
            }
            .filter(|f| f.is_finite())
            .ok_or_else(|| bad("数値"))?;
            checked(n)?;
            let n = if a.clamp {
                n.clamp(
                    a.min.unwrap_or(f64::NEG_INFINITY),
                    a.max.unwrap_or(f64::INFINITY),
                )
            } else {
                n
            };
            json!(n)
        }
        ArgType::Text => match v {
            Value::String(_) => v.clone(),
            Value::Number(n) => Value::String(n.to_string()),
            _ => return Err(bad("文字列")),
        },
        ArgType::Id(kind) => {
            let s = v.as_str().ok_or_else(|| bad("ID（文字列）"))?;
            check_id(cmd, a, *kind, s)?;
            Value::String(s.to_string())
        }
        ArgType::Ids(kind) => {
            let items: Vec<String> = match v {
                Value::Array(xs) => xs
                    .iter()
                    .map(|x| {
                        x.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| bad("ID のリスト"))
                    })
                    .collect::<CmdResult<_>>()?,
                Value::String(s) => s
                    .split([',', ' '])
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect(),
                _ => return Err(bad("ID のリスト（配列またはカンマ区切り）")),
            };
            for s in &items {
                check_id(cmd, a, *kind, s)?;
            }
            json!(items)
        }
        ArgType::Choice(options) => {
            let s = v.as_str().ok_or_else(|| bad("文字列"))?;
            if !options.contains(&s) {
                return Err(CmdError::invalid(format!(
                    "{cmd} の --{} には {} のいずれかを指定してください（`{s}` は使用できません）",
                    a.name,
                    options.join("・")
                )));
            }
            v.clone()
        }
        ArgType::Position | ArgType::Grid => match v {
            Value::Number(_) | Value::String(_) => v.clone(),
            _ => return Err(bad("位置（文字列または数値）")),
        },
        ArgType::Objects => {
            let list = match v {
                Value::Array(_) => v.clone(),
                Value::String(s) => serde_json::from_str::<Value>(s).map_err(|e| {
                    CmdError::invalid(format!("{cmd} の --{}: JSON を読み込めません: {e}", a.name))
                })?,
                _ => return Err(bad("オブジェクトの配列")),
            };
            match &list {
                Value::Array(items) if items.iter().all(Value::is_object) => list,
                _ => return Err(bad("オブジェクトの配列")),
            }
        }
    })
}

fn check_id(cmd: &str, a: &ArgDef, kind: IdKind, s: &str) -> CmdResult<()> {
    let id: AnyId = s
        .parse()
        .map_err(|e: String| CmdError::invalid(format!("{cmd} の --{}: {e}", a.name)))?;
    if !kind.accepts(&id) {
        return Err(CmdError::invalid(format!(
            "{cmd} の --{} には {} の ID を指定してください（`{s}` は使用できません）",
            a.name,
            kind.label()
        )));
    }
    Ok(())
}

/// Checked arguments with typed getters.
#[derive(Clone, Debug, Default)]
pub struct Args(pub Map<String, Value>);

impl Args {
    pub fn from_json(v: Value) -> Args {
        match v {
            Value::Object(m) => Args(m),
            _ => Args::default(),
        }
    }
    pub fn has(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }
    pub fn opt_str(&self, name: &str) -> Option<&str> {
        self.0.get(name).and_then(Value::as_str)
    }
    pub fn str(&self, name: &str) -> CmdResult<&str> {
        self.opt_str(name)
            .ok_or_else(|| CmdError::invalid(format!("--{name} が必要です")))
    }
    pub fn opt_i64(&self, name: &str) -> Option<i64> {
        self.0.get(name).and_then(Value::as_i64)
    }
    pub fn i64(&self, name: &str) -> CmdResult<i64> {
        self.opt_i64(name)
            .ok_or_else(|| CmdError::invalid(format!("--{name} が必要です")))
    }
    pub fn opt_f64(&self, name: &str) -> Option<f64> {
        self.0.get(name).and_then(Value::as_f64)
    }
    pub fn f64(&self, name: &str) -> CmdResult<f64> {
        self.opt_f64(name)
            .ok_or_else(|| CmdError::invalid(format!("--{name} が必要です")))
    }
    pub fn flag(&self, name: &str) -> bool {
        self.0.get(name).and_then(Value::as_bool).unwrap_or(false)
    }
    pub fn opt_flag(&self, name: &str) -> Option<bool> {
        self.0.get(name).and_then(Value::as_bool)
    }

    /// `--on` / `--off` of a toggle: `Some(state)` when given, `None` to flip.
    pub fn toggle(&self) -> CmdResult<Option<bool>> {
        match (self.opt_flag("on"), self.opt_flag("off")) {
            (Some(true), Some(true)) => {
                Err(CmdError::invalid("--on と --off は同時に指定できません"))
            }
            (Some(true), _) => Ok(Some(true)),
            (_, Some(true)) => Ok(Some(false)),
            _ => Ok(None),
        }
    }

    pub fn id<T: FromStr<Err = String>>(&self, name: &str) -> CmdResult<T> {
        self.str(name)?.parse::<T>().map_err(CmdError::invalid)
    }
    pub fn opt_id<T: FromStr<Err = String>>(&self, name: &str) -> CmdResult<Option<T>> {
        self.opt_str(name)
            .map(|s| s.parse::<T>().map_err(CmdError::invalid))
            .transpose()
    }

    /// The ids in `--ids` that are of type `T` (others are ignored, so a mixed
    /// selection can be passed to a command that takes only tracks).
    pub fn ids<T: FromStr<Err = String>>(&self, name: &str) -> CmdResult<Vec<T>> {
        let Some(Value::Array(xs)) = self.0.get(name) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for x in xs {
            let s = x.as_str().unwrap_or_default();
            if seen.insert(s.to_string())
                && let Ok(id) = s.parse::<T>()
            {
                out.push(id);
            }
        }
        Ok(out)
    }

    pub fn require_ids<T: FromStr<Err = String>>(&self, what: &str) -> CmdResult<Vec<T>> {
        let v = self.ids::<T>("ids")?;
        if v.is_empty() {
            return Err(CmdError::invalid(format!(
                "{what}を --ids で指定してください（選択中のものもありません）"
            )));
        }
        Ok(v)
    }
}
