//! One script of calls, run through the real CLI program and the MCP tool dispatcher: each
//! call answers the same through both. How each argument is read is checked for every command
//! by `args::tests::every_argument_of_every_command_reads_as_the_mcp_call_does`; this checks
//! the way between (one process per call, the song saved and read again between calls, the
//! JSON written out) on a call of every argument type.
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};
use xirius_core::Session;
use xirius_core::command::ArgType;
use xirius_mcp::{Gateway, Scope};

trait Endpoint {
    fn run(&self, name: &str, args: &Value) -> Value;
}
struct Cli(PathBuf);
impl Cli {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("xirius-interface-{}", std::process::id()));
        let output = Command::new(env!("CARGO_BIN_EXE_xirius"))
            .arg("init")
            .arg(&dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self(dir)
    }
}
impl Drop for Cli {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
impl Endpoint for Cli {
    fn run(&self, name: &str, args: &Value) -> Value {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xirius"));
        command
            .env_remove("XIRIUS_SONG")
            .arg("--song")
            .arg(&self.0)
            .arg(name)
            .arg("--json");
        for (key, value) in args.as_object().unwrap() {
            command.arg(format!("--{key}"));
            let encoded = match value {
                Value::String(text) => text.clone(),
                Value::Array(values) if values.iter().all(Value::is_string) => values
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(","),
                other => other.to_string(),
            };
            command.arg(encoded);
        }
        let output = command.output().unwrap();
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "{name}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }
}
struct Mcp(Gateway);
impl Mcp {
    fn new() -> Self {
        let session = Arc::new(Mutex::new(Session::new("interfaces")));
        Self(Gateway::new(session, "test", Scope::Edit).with_save(false))
    }
}
impl Endpoint for Mcp {
    fn run(&self, name: &str, args: &Value) -> Value {
        let tool = name.replace('.', "_");
        self.0.call(&tool, args.as_object().unwrap()).unwrap().0
    }
}

/// The calls: `$track` and `$clip` stand for the IDs the first two made.
fn script() -> Vec<(&'static str, Value)> {
    vec![
        ("track.add.instrument", json!({"name": "Piano"})),
        (
            "clip.add.midi",
            json!({"track": "$track", "at": "1.1.0", "length": 3840}),
        ),
        (
            "note.add",
            json!({"clip": "$clip", "at": "1.2.0", "pitch": "C3"}),
        ),
        (
            "note.put",
            json!({"clip": "$clip", "notes": [
                {"at": 970, "pitch": 60, "length": 240, "velocity": 70},
                {"at": 1930, "pitch": 62, "length": 240, "velocity": 90}
            ]}),
        ),
        ("note.quantize", json!({"ids": ["$clip"], "grid": "1/8"})),
        (
            "note.velocity.shape",
            json!({"ids": ["$clip"], "shape": "stronger", "amount": 10}),
        ),
        ("note.transpose", json!({"ids": ["$clip"], "by": 2})),
        ("track.volume.set", json!({"ids": ["$track"], "db": -6.5})),
        ("track.mute", json!({"ids": ["$track"], "on": true})),
        ("history.undo", json!({})),
        ("history.redo", json!({})),
        ("note.list", json!({"clip": "$clip"})),
        ("track.list", json!({})),
        ("track.add.audio", json!({"name": "Vocal", "index": 99})),
        // A refusal answers alike too.
        ("note.transpose", json!({"ids": ["clp_99"], "by": 1})),
    ]
}

fn replace(value: &Value, ids: &[(&str, Value)]) -> Value {
    match value {
        Value::String(s) => ids
            .iter()
            .find(|(k, _)| k == s)
            .map_or_else(|| value.clone(), |(_, v)| v.clone()),
        Value::Array(a) => Value::Array(a.iter().map(|v| replace(v, ids)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| (k.clone(), replace(v, ids)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// What only one entrance says: the MCP call's receipt, the notes an AI wrote (`ai`), and the
/// song's hash (the songs' names and authors differ).
fn strip(value: &mut Value) {
    match value {
        Value::Object(o) => {
            for key in ["receipt", "state_hash", "ai"] {
                o.remove(key);
            }
            o.values_mut().for_each(strip);
        }
        Value::Array(a) => a.iter_mut().for_each(strip),
        _ => {}
    }
}

fn answers(endpoint: &dyn Endpoint) -> Vec<Value> {
    let mut ids = Vec::new();
    let mut out = Vec::new();
    for (name, args) in script() {
        let mut answer = endpoint.run(name, &replace(&args, &ids));
        strip(&mut answer);
        for (key, made_by) in [
            ("$track", "track.add.instrument"),
            ("$clip", "clip.add.midi"),
        ] {
            if name == made_by {
                ids.push((key, answer["data"]["id"].clone()));
            }
        }
        out.push(answer);
    }
    out
}

/// Every argument type is in the script: a new `ArgType` does not build until it has its
/// index here, and then fails until a call of it is added.
#[test]
fn the_script_has_a_call_of_every_argument_type() {
    let registry = xirius_session::commands::registry();
    let mut seen = [false; 11];
    for (name, args) in script() {
        let def = registry.get(name).unwrap_or_else(|| panic!("{name}"));
        for key in args.as_object().unwrap().keys() {
            let arg = def.args.iter().find(|a| a.name == key).unwrap();
            seen[match arg.ty {
                ArgType::Bool => 0,
                ArgType::Int => 1,
                ArgType::Float => 2,
                ArgType::Text => 3,
                ArgType::Id(_) => 4,
                ArgType::Ids(_) => 5,
                ArgType::Choice(_) => 6,
                ArgType::Position => 7,
                ArgType::Grid => 8,
                ArgType::Pitch => 9,
                ArgType::Objects => 10,
            }] = true;
        }
    }
    const NAMES: [&str; 11] = [
        "Bool", "Int", "Float", "Text", "Id", "Ids", "Choice", "Position", "Grid", "Pitch",
        "Objects",
    ];
    let missing: Vec<&str> = (0..seen.len())
        .filter(|&i| !seen[i])
        .map(|i| NAMES[i])
        .collect();
    assert!(
        missing.is_empty(),
        "no call of the argument types {missing:?}"
    );
}

#[test]
fn each_call_answers_the_same_through_the_cli_and_mcp() {
    let cli = answers(&Cli::new());
    let mcp = answers(&Mcp::new());
    let wrong: Vec<String> = script()
        .iter()
        .zip(cli.iter().zip(&mcp))
        .enumerate()
        .filter(|(_, (_, (c, m)))| c != m)
        .map(|(i, ((name, args), (c, m)))| {
            format!("call {i}, {name} {args}:\n  cli {c}\n  mcp {m}")
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
