//! A plugin switched off to free its CPU (ENG-008). It is not bypass: it keeps its state and
//! parameters, it is one undo step, and an old song without the key reads as active.

use serde_json::{Value, json};
use xirius_core::id::TrackId;
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn id(v: &Value) -> String {
    v["id"].as_str().expect("an id").to_string()
}

fn song() -> (Session, String, String) {
    let mut s = Session::new("deactivate");
    let t = id(&run(&mut s, "track.add.audio", json!({ "name": "A" })));
    let p = id(&run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "Reverb", "kind": "effect", "format": "builtin", "uid": "Reverb" }),
    ));
    (s, t, p)
}

fn insert(s: &Session, t: &str) -> xirius_core::model::Plugin {
    let t: TrackId = t.parse().expect("a track id");
    s.project().tracks[&t].inserts[0].clone()
}

#[test]
fn deactivating_is_not_bypass_and_activating_brings_it_back() {
    let (mut s, t, p) = song();
    run(
        &mut s,
        "plugin.state.set",
        json!({ "id": p, "state": "AAEC" }),
    );
    run(&mut s, "plugin.deactivate", json!({ "ids": [p] }));
    let x = insert(&s, &t);
    assert!(x.deactivated);
    assert!(!x.bypass, "bypass is its own switch");
    assert!(!x.runs());
    assert_eq!(x.state.as_deref(), Some("AAEC"), "the state is kept");
    run(&mut s, "plugin.bypass", json!({ "ids": [p] }));
    run(&mut s, "plugin.activate", json!({ "ids": [p] }));
    let x = insert(&s, &t);
    assert!(!x.deactivated);
    assert!(x.bypass, "activating leaves the bypass as it was");
}

#[test]
fn it_is_one_undo_step_each_and_an_unknown_id_fails() {
    let (mut s, t, p) = song();
    let before = s.steps().len();
    run(&mut s, "plugin.deactivate", json!({ "ids": [p] }));
    assert_eq!(s.steps().len(), before + 1);
    assert!(insert(&s, &t).deactivated);
    s.undo(1).expect("undo");
    assert!(!insert(&s, &t).deactivated);
    let err = s
        .run_value(
            "plugin.deactivate",
            json!({ "ids": ["pl-ffffffff"] }),
            &RunOptions::human(),
        )
        .expect_err("no such plugin");
    assert!(!format!("{err:?}").is_empty());
}

#[test]
fn the_song_keeps_it_through_save_and_reopen_and_an_old_song_reads_as_active() {
    let (mut s, t, p) = song();
    run(&mut s, "plugin.deactivate", json!({ "ids": [p] }));
    let text = serde_json::to_string(&s.project()).expect("serialise");
    let back: Project = serde_json::from_str(&text).expect("read");
    let tid: TrackId = t.parse().unwrap();
    assert!(back.tracks[&tid].inserts[0].deactivated);

    // A song written before the key existed has none, and an active plugin writes none.
    run(&mut s, "plugin.activate", json!({ "ids": [p] }));
    let text = serde_json::to_string(&s.project()).expect("serialise");
    assert!(!text.contains("deactivated"));
    let back: Project = serde_json::from_str(&text).expect("read");
    assert!(!back.tracks[&tid].inserts[0].deactivated);
}

#[test]
fn copying_a_channels_settings_carries_the_switch() {
    let (mut s, _t, p) = song();
    let b = id(&run(&mut s, "track.add.audio", json!({ "name": "B" })));
    run(&mut s, "plugin.deactivate", json!({ "ids": [p] }));
    let a = {
        let proj = s.project();
        proj.tracks
            .iter()
            .find(|(_, t)| t.name == "A")
            .map(|(k, _)| k.to_string())
            .unwrap()
    };
    run(
        &mut s,
        "channel.settings.paste",
        json!({ "ids": [b], "from": a }),
    );
    assert!(insert(&s, &b).deactivated);
}
