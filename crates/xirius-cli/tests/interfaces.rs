//! One editing contract, exercised through the real CLI and MCP tool dispatcher.
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use xirius_core::{Session, session::AiPermission};
use xirius_mcp::{Gateway, Scope};

trait Endpoint {
    fn run(&self, name: &str, args: Value) -> Value;
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
    fn run(&self, name: &str, args: Value) -> Value {
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
        let result: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "{name}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert!(output.status.success(), "{result}");
        result
    }
}
struct Mcp {
    gateway: Gateway,
    session: Arc<Mutex<Session>>,
}
impl Mcp {
    fn new() -> Self {
        let session = Arc::new(Mutex::new(Session::new("interfaces")));
        let gateway = Gateway::new(session.clone(), "test", Scope::Edit).with_save(false);
        Self { gateway, session }
    }
    fn expand(&self, value: Value) -> Value {
        let Some(id) = value["result_id"].as_str() else {
            return value;
        };
        let mut text = String::new();
        for page in 0..value["pages"].as_u64().unwrap() {
            let (page, error) = self
                .gateway
                .call(
                    "ops_get-result",
                    json!({"id": id, "page": page}).as_object().unwrap(),
                )
                .unwrap();
            assert!(!error, "{page}");
            text.push_str(page["text"].as_str().unwrap());
        }
        serde_json::from_str(&text).unwrap()
    }
}
impl Endpoint for Mcp {
    fn run(&self, name: &str, args: Value) -> Value {
        let tool = name.replace('.', "_");
        let (mut value, error) = self.gateway.call(&tool, args.as_object().unwrap()).unwrap();
        assert!(!error, "{value}");
        if value["confirmation_required"] == true {
            assert_eq!(name, "ai.permission.set");
            assert_eq!(value["receipt"]["state"], "accepted");
            assert_eq!(
                self.session.lock().unwrap().ai_permission(),
                AiPermission::Edit,
                "AI cannot raise its own permission"
            );
            let id = value["receipt"]["id"].clone();
            let token = value["decision"]["token"].clone();
            let (decision, error) = self
                .gateway
                .admin(
                    "ai.decide",
                    json!({"token": token, "answer": "yes"})
                        .as_object()
                        .unwrap(),
                )
                .unwrap();
            assert!(!error, "{decision}");
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                value = self
                    .gateway
                    .call("ops_receipt", json!({"id": id}).as_object().unwrap())
                    .unwrap()
                    .0;
                if value["receipt"]["state"] == "completed" {
                    break;
                }
                assert!(Instant::now() < deadline, "{value}");
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(
                self.session.lock().unwrap().ai_permission(),
                AiPermission::Save
            );
        }
        self.expand(value)
    }
}

fn exercise(endpoint: &dyn Endpoint) {
    const REQUIRED: [&str; 7] = [
        "note.quantize",
        "note.velocity.shape",
        "note.transpose",
        "history.undo",
        "history.redo",
        "ops.list",
        "ai.permission.set",
    ];
    let operations = endpoint.run("ops.list", json!({}));
    let listed = operations["data"].as_array().unwrap();
    for name in REQUIRED {
        assert!(listed.iter().any(|row| row["name"] == name), "{name}");
    }
    let track =
        endpoint.run("track.add.instrument", json!({"name": "Piano"}))["data"]["id"].clone();
    let clip = endpoint.run(
        "clip.add.midi",
        json!({"track": track, "at": 0, "length": 3840}),
    )["data"]["id"]
        .clone();
    endpoint.run(
        "note.put",
        json!({"clip": clip, "notes": [
            {"at": 970, "pitch": 60, "length": 240, "velocity": 70},
            {"at": 1930, "pitch": 62, "length": 240, "velocity": 90}
        ]}),
    );
    let result = endpoint.run("note.quantize", json!({"ids": [clip], "grid": "1/8"}));
    assert_eq!(result["changes"]["counts"]["note"]["changed"], 2);
    endpoint.run(
        "note.velocity.shape",
        json!({"ids": [clip], "shape": "stronger", "amount": 10}),
    );
    endpoint.run("note.transpose", json!({"ids": [clip], "by": 2}));
    let notes = endpoint.run("note.list", json!({"clip": clip}))["data"].clone();
    assert_eq!(notes[0]["start"], 960);
    assert_eq!(notes[1]["start"], 1920);
    assert_eq!(notes[0]["velocity"], 80);
    assert_eq!(notes[1]["velocity"], 100);
    assert_eq!(notes[0]["pitch"], 62);
    endpoint.run("history.undo", json!({}));
    assert_eq!(
        endpoint.run("note.list", json!({"clip": clip}))["data"][0]["pitch"],
        60
    );
    endpoint.run("history.redo", json!({}));
    assert_eq!(
        endpoint.run("note.list", json!({"clip": clip}))["data"][0]["pitch"],
        62
    );
    endpoint.run("ai.permission.set", json!({"level": "save"}));
    assert_eq!(
        endpoint.run("ai.permission.get", json!({}))["data"]["level"],
        "save"
    );
}

#[test]
fn seven_mvp_commands_have_the_same_cli_and_mcp_editing_contract() {
    exercise(&Cli::new());
    exercise(&Mcp::new());
}
