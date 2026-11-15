//! MIX-051: a receiving effect owns one instrument note source.
use serde_json::{Value, json};
use xirius_core::model::Project;
use xirius_core::{Session, session::RunOptions};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .data
}

#[test]
fn note_source_set_enable_remove_each_undo_once() {
    let mut s = Session::from_project(Project::new("notes"));
    let source = run(&mut s, "track.add.instrument", json!({"name":"Melody"}))["id"]
        .as_str().unwrap().to_owned();
    let vocal = run(&mut s, "track.add.audio", json!({"name":"Vocal"}))["id"]
        .as_str().unwrap().to_owned();
    let plugin = run(&mut s, "plugin.add", json!({"track":vocal,"name":"Pitch Correction"}))["id"]
        .as_str().unwrap().to_owned();
    run(&mut s, "plugin.note_source.set", json!({"id":plugin,"from":source}));
    let set = run(&mut s, "plugin.note_source.list", json!({"id":plugin}));
    assert_eq!(set["from"], source);
    assert_eq!(set["on"], true);
    s.undo(1).unwrap();
    assert!(run(&mut s, "plugin.note_source.list", json!({"id":plugin}))["from"].is_null());
    s.redo(1).unwrap();
    run(&mut s, "plugin.note_source.enable", json!({"id":plugin,"state":"off"}));
    assert_eq!(run(&mut s, "plugin.note_source.list", json!({"id":plugin}))["on"], false);
    s.undo(1).unwrap();
    assert_eq!(run(&mut s, "plugin.note_source.list", json!({"id":plugin}))["on"], true);
    run(&mut s, "plugin.note_source.remove", json!({"id":plugin}));
    assert!(run(&mut s, "plugin.note_source.list", json!({"id":plugin}))["from"].is_null());
    s.undo(1).unwrap();
    assert_eq!(run(&mut s, "plugin.note_source.list", json!({"id":plugin}))["from"], source);
}
