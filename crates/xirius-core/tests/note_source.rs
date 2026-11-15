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
        .as_str()
        .unwrap()
        .to_owned();
    let vocal = run(&mut s, "track.add.audio", json!({"name":"Vocal"}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let plugin = run(
        &mut s,
        "plugin.add",
        json!({"track":vocal,"name":"Pitch Correction"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    run(
        &mut s,
        "plugin.note_source.set",
        json!({"id":plugin,"from":source}),
    );
    let set = run(&mut s, "plugin.note_source.list", json!({"id":plugin}));
    assert_eq!(set["from"], source);
    assert_eq!(set["on"], true);
    s.undo(1).unwrap();
    assert!(run(&mut s, "plugin.note_source.list", json!({"id":plugin}))["from"].is_null());
    s.redo(1).unwrap();
    run(
        &mut s,
        "plugin.note_source.enable",
        json!({"id":plugin,"state":"off"}),
    );
    assert_eq!(
        run(&mut s, "plugin.note_source.list", json!({"id":plugin}))["on"],
        false
    );
    s.undo(1).unwrap();
    assert_eq!(
        run(&mut s, "plugin.note_source.list", json!({"id":plugin}))["on"],
        true
    );
    run(&mut s, "plugin.note_source.remove", json!({"id":plugin}));
    assert!(run(&mut s, "plugin.note_source.list", json!({"id":plugin}))["from"].is_null());
    s.undo(1).unwrap();
    assert_eq!(
        run(&mut s, "plugin.note_source.list", json!({"id":plugin}))["from"],
        source
    );
}

#[test]
fn rejects_bad_sources_and_receivers_and_survives_old_songs() {
    let mut s = Session::from_project(Project::new("notes"));
    let source = run(&mut s, "track.add.instrument", json!({"name":"Melody"}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let vocal = run(&mut s, "track.add.audio", json!({"name":"Vocal"}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let pitch = run(
        &mut s,
        "plugin.add",
        json!({"track":vocal,"name":"Pitch Correction"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let eq = run(&mut s, "plugin.add", json!({"track":vocal,"name":"EQ"}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for (id, from) in [
        (&eq, &source),
        (&pitch, &vocal),
        (&pitch, &"t999".to_string()),
    ] {
        assert!(
            s.run_value(
                "plugin.note_source.set",
                json!({"id":id,"from":from}),
                &RunOptions::human()
            )
            .is_err()
        );
    }
    run(
        &mut s,
        "plugin.note_source.set",
        json!({"id":pitch,"from":source}),
    );
    let same_track = run(
        &mut s,
        "plugin.add",
        json!({"track":source,"name":"Pitch Correction"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    run(
        &mut s,
        "plugin.note_source.set",
        json!({"id":same_track,"from":source}),
    );
    let saved = serde_json::to_value(s.project()).unwrap();
    let loaded: Project = serde_json::from_value(saved.clone()).unwrap();
    assert!(
        loaded
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|x| x.id.to_string() == pitch)
            .unwrap()
            .note_source
            .is_some()
    );
    let mut old = saved;
    for track in old["tracks"].as_object_mut().unwrap().values_mut() {
        for plugin in track["inserts"].as_array_mut().unwrap() {
            plugin.as_object_mut().unwrap().remove("note_source");
        }
    }
    let old: Project = serde_json::from_value(old).unwrap();
    assert!(
        old.tracks
            .values()
            .flat_map(|t| &t.inserts)
            .all(|x| x.note_source.is_none())
    );
    run(&mut s, "track.delete", json!({"ids":[source]}));
    assert!(run(&mut s, "plugin.note_source.list", json!({"id":pitch}))["from"].is_null());
    s.undo(1).unwrap();
    assert_eq!(
        run(&mut s, "plugin.note_source.list", json!({"id":pitch}))["from"],
        source
    );
}
