use serde_json::json;
use xirius_core::{Project, Session, session::RunOptions};

#[test]
fn unavailable_plugin_state_and_parameters_survive_open_save_open() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-missing-plugin-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("missing-plugin");
    let mut session = Session::create(&dir, Project::new("unknown plugin")).unwrap();
    let track = session
        .run_value(
            "track.add.audio",
            json!({"name": "Audio"}),
            &RunOptions::human(),
        )
        .unwrap()
        .data["id"]
        .as_str()
        .unwrap()
        .parse::<xirius_core::id::TrackId>()
        .unwrap();
    let plugin = session.run_value("plugin.add", json!({"track": track.to_string(), "name": "Unavailable", "format": "vst3", "uid": "missing-vendor/opaque-id"}), &RunOptions::human()).unwrap().data["id"].clone();
    session
        .run_value(
            "plugin.state.set",
            json!({"id": plugin, "state": "AAECA/8="}),
            &RunOptions::human(),
        )
        .unwrap();
    let expected = session.project().tracks[&track].inserts[0].clone();
    session.save().unwrap();
    drop(session);
    // Core has no plugin loader: the plugin is unavailable throughout this round trip.
    let mut session = Session::open(&dir).unwrap();
    assert_eq!(session.project().tracks[&track].inserts[0], expected);
    session
        .run_value(
            "track.rename",
            json!({"id": track.to_string(), "name": "Renamed"}),
            &RunOptions::human(),
        )
        .unwrap();
    session.save().unwrap();
    drop(session);
    let session = Session::open(&dir).unwrap();
    assert_eq!(session.project().tracks[&track].inserts[0], expected);
    drop(session);
    std::fs::remove_dir_all(dir).unwrap();
}
