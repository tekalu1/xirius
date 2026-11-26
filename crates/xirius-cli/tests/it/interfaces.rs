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
            assert!(matches!(
                name,
                "ai.permission.set" | "plugin.note_source.remove"
            ));
            assert_eq!(value["receipt"]["state"], "accepted");
            if name == "ai.permission.set" {
                assert_eq!(
                    self.session.lock().unwrap().ai_permission(),
                    AiPermission::Edit,
                    "AI cannot raise its own permission"
                );
            }
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
            if name == "ai.permission.set" {
                assert_eq!(
                    self.session.lock().unwrap().ai_permission(),
                    AiPermission::Save
                );
            }
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

#[test]
fn audio_track_with_an_effect_can_be_positioned_from_cli_and_mcp() {
    for endpoint in [&Cli::new() as &dyn Endpoint, &Mcp::new() as &dyn Endpoint] {
        let folder = endpoint.run("folder.add", json!({"name":"Group"}))["data"]["id"].clone();
        let anchor = endpoint.run("track.add.audio", json!({"into":folder}))["data"]["id"].clone();
        let list = || endpoint.run("track.list", json!({}))["data"].clone();
        let original = list();
        for position in [json!({"before":anchor}), json!({"into":folder,"index":0})] {
            let mut args = position;
            args["effect"] = json!("EQ");
            let added = endpoint.run("track.add.with-effect", args)["data"].clone();
            let current = list();
            let rows = current.as_array().unwrap();
            let index = rows
                .iter()
                .position(|row| row["id"] == added["id"])
                .unwrap();
            assert_eq!(rows[index + 1]["id"], anchor);
            assert_eq!(rows[index]["parent"], folder);
            assert_eq!(rows[index]["kind"], "audio");
            assert_eq!(rows[index]["inserts"], 1);
            assert!(added["plugin"].as_str().is_some());
            endpoint.run("history.undo", json!({}));
            assert_eq!(list(), original);
        }
    }
}

#[test]
fn note_source_commands_have_the_same_cli_and_mcp_contract() {
    for endpoint in [&Cli::new() as &dyn Endpoint, &Mcp::new() as &dyn Endpoint] {
        let source =
            endpoint.run("track.add.instrument", json!({"name":"Melody"}))["data"]["id"].clone();
        let vocal = endpoint.run("track.add.audio", json!({"name":"Vocal"}))["data"]["id"].clone();
        let plugin = endpoint.run(
            "plugin.add",
            json!({"track":vocal,"name":"Pitch Correction"}),
        )["data"]["id"]
            .clone();
        endpoint.run("plugin.note_source.set", json!({"id":plugin,"from":source}));
        let value = endpoint.run("plugin.note_source.list", json!({"id":plugin}));
        assert_eq!(value["data"]["from"], source);
        endpoint.run(
            "plugin.note_source.enable",
            json!({"id":plugin,"state":"off"}),
        );
        assert_eq!(
            endpoint.run("plugin.note_source.list", json!({"id":plugin}))["data"]["on"],
            false
        );
        endpoint.run("plugin.note_source.remove", json!({"id":plugin}));
        assert!(
            endpoint.run("plugin.note_source.list", json!({"id":plugin}))["data"]["from"].is_null()
        );
    }
}

#[test]
fn pitch_key_toggle_is_available_through_cli_and_mcp() {
    for endpoint in [&Cli::new() as &dyn Endpoint, &Mcp::new() as &dyn Endpoint] {
        let vocal = endpoint.run("track.add.audio", json!({"name":"Vocal"}))["data"]["id"].clone();
        let plugin = endpoint.run(
            "plugin.add",
            json!({"track":vocal,"name":"Pitch Correction"}),
        )["data"]["id"]
            .clone();
        let before = endpoint.run("plugin.param.list", json!({"id":plugin}))["data"].clone();
        endpoint.run("plugin.pitch_key.toggle", json!({"id":plugin,"relative":4}));
        let after = endpoint.run("plugin.param.list", json!({"id":plugin}))["data"].clone();
        assert_ne!(before, after);
        endpoint.run("history.undo", json!({}));
        assert_eq!(
            endpoint.run("plugin.param.list", json!({"id":plugin}))["data"],
            before
        );
    }
}

#[test]
fn pitch_range_remains_settable_through_cli_and_mcp() {
    for endpoint in [&Cli::new() as &dyn Endpoint, &Mcp::new() as &dyn Endpoint] {
        let vocal = endpoint.run("track.add.audio", json!({"name":"Vocal"}))["data"]["id"].clone();
        let plugin = endpoint.run(
            "plugin.add",
            json!({"track":vocal,"name":"Pitch Correction"}),
        )["data"]["id"]
            .clone();
        let before = endpoint.run("plugin.param.list", json!({"id":plugin}))["data"].clone();
        endpoint.run(
            "plugin.param.set",
            json!({"id":plugin,"param":"range","plain":"High"}),
        );
        let after = endpoint.run("plugin.param.list", json!({"id":plugin}))["data"].clone();
        assert_ne!(before, after);
        endpoint.run("history.undo", json!({}));
        assert_eq!(
            endpoint.run("plugin.param.list", json!({"id":plugin}))["data"],
            before
        );
    }
}

#[test]
fn pad_output_uses_the_same_cli_and_mcp_command() {
    for endpoint in [&Cli::new() as &dyn Endpoint, &Mcp::new() as &dyn Endpoint] {
        let track =
            endpoint.run("track.add.instrument", json!({"name":"Drums"}))["data"]["id"].clone();
        let kit =
            endpoint.run("plugin.add", json!({"track":track,"name":"Drum Kit"}))["data"]["id"]
                .clone();
        let made = endpoint.run(
            "plugin.pad_output.set",
            json!({"id":kit,"pads":[{"pad":"A1"},{"pad":"A2"}],"active":"A2","to":"new"}),
        );
        let bus = made["data"]["output"].as_str().unwrap();
        assert!(bus.starts_with("trk_"), "{made}");
        endpoint.run("history.undo", json!({}));
        let main = endpoint.run(
            "plugin.pad_output.set",
            json!({"id":kit,"pads":[{"pad":"A1"}],"to":"Main"}),
        );
        assert_eq!(main["data"]["output"], "main");
    }
}

#[test]
fn every_solo_turned_off_is_put_back_through_cli_and_mcp() {
    // UI-376: the column's foot runs these; the CLI and the AI reach the same commands.
    for endpoint in [&Cli::new() as &dyn Endpoint, &Mcp::new() as &dyn Endpoint] {
        let a = endpoint.run("track.add.audio", json!({"name":"A"}))["data"]["id"].clone();
        let b = endpoint.run("track.add.audio", json!({"name":"B"}))["data"]["id"].clone();
        endpoint.run("track.solo", json!({"ids":[a.clone()]}));
        let soloed = |endpoint: &dyn Endpoint| -> Vec<Value> {
            endpoint.run("track.list", json!({}))["data"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|t| t["solo"] == true)
                .map(|t| t["id"].clone())
                .collect()
        };
        assert_eq!(soloed(endpoint), vec![a.clone()]);
        endpoint.run("track.solo.clear", json!({}));
        assert!(soloed(endpoint).is_empty());
        endpoint.run("track.solo.restore", json!({}));
        assert_eq!(soloed(endpoint), vec![a.clone()]);
        // A mute set by hand forgets the earlier clear; the last clear is what comes back.
        endpoint.run("track.mute", json!({"ids":[b.clone()]}));
        endpoint.run("track.mute.clear", json!({}));
        endpoint.run("track.mute", json!({"ids":[a]}));
        endpoint.run("track.mute", json!({"ids":[b]}));
        endpoint.run("track.mute.clear", json!({}));
        endpoint.run("track.mute.restore", json!({}));
        let muted = endpoint.run("track.list", json!({}))["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["mute"] == true)
            .count();
        assert_eq!(muted, 2, "the last clear's two come back");
    }
}
