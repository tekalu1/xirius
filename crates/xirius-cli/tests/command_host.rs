use serde_json::json;
use std::sync::Mutex;
use xirius_core::{ErrorCode, Project, Session, protocol::Hello, session::RunOptions};
use xirius_mcp::{Host, ipc::PipeHost};

fn child(hello: Hello) -> Result<PipeHost, xirius_core::CmdError> {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_xirius"));
    command.arg("--command-host");
    PipeHost::start(command, xirius_session::commands::registry(), hello)
}

fn exercise(host: &dyn Host) {
    let run = |name, args: serde_json::Value| {
        host.run(name, args.as_object().unwrap(), &RunOptions::human(), false)
            .unwrap()
    };
    let track = run("track.add.audio", json!({"name": "Audio"}));
    let clear = run("select.clear", json!({}));
    assert!(clear.data.is_null() || clear.data.is_object());
    let id = track.data["id"].as_str().unwrap();
    assert_eq!(track.changes.count("track"), 1);
    let mute = run("track.mute", json!({"ids": [id], "on": true}));
    assert_eq!(mute.changes.count("track"), 1);
    let undo = run("history.undo", json!({}));
    assert_eq!(
        undo.data["state_hash"]["before"],
        mute.data["state_hash"]["after"]
    );
    assert_eq!(
        undo.data["state_hash"]["after"],
        track.data["state_hash"]["after"]
    );
    let error = host
        .run(
            "track.rename",
            json!({"id": "trk_9999", "name": "Missing"})
                .as_object()
                .unwrap(),
            &RunOptions::human(),
            false,
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    let dir = tempfile::Builder::new()
        .prefix("xirius-pipe-render-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("render.wav");
    let result = run(
        "export.mixdown",
        json!({"out": path, "range": "custom", "from": 0, "to": 480, "sample_rate": 48000, "bit_depth": 32}),
    );
    assert_eq!(result.command, "export.mixdown");
    let mut reader = hound::WavReader::open(&path).unwrap();
    assert_eq!(reader.duration(), 12000);
    assert!(reader.samples::<f32>().all(|sample| sample.unwrap() == 0.0));
    drop(reader);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn same_command_contract_in_process_and_through_a_process_pipe() {
    let mut session = Session::with_registry(
        Project::new("engine"),
        xirius_session::commands::shared_registry(),
    );
    xirius_session::commands::attach(&mut session, Default::default());
    let session = Mutex::new(session);
    exercise(&session);
    exercise(&child(Hello::default()).unwrap());
}

#[test]
fn mismatched_protocol_is_refused_before_the_first_command() {
    let error = child(Hello {
        command_protocol: u32::MAX,
    })
    .err()
    .unwrap();
    assert_eq!(error.code, ErrorCode::Conflict);
}
