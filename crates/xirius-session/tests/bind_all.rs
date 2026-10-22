//! OPS-002: every command of the whole table can be given a key, the key finds it again from the
//! file, a clash is reported, and each layout preset is a complete, conflict-free set of keys.
use std::path::PathBuf;

use serde_json::json;
use xirius_core::session::{RunOptions, Session};
use xirius_prefs::keymap::{Issue, Keymap, preset_bindings};
use xirius_prefs::{Chord, Os, Settings, Shared};
use xirius_session::commands::shared_registry;

/// A folder of the test's own, removed when dropped (also when the test fails).
fn dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-bind-all-{name}-"))
        .tempdir()
        .unwrap()
}

fn session(shared: &Shared) -> Session {
    let mut s = Session::with_registry(
        xirius_core::model::Project::new("bind all"),
        shared_registry(),
    );
    xirius_prefs::attach(&mut s, shared.clone());
    s
}

#[test]
fn every_command_can_be_bound_and_is_found_by_its_key_and_read_back_from_the_file() {
    let tmp = dir("each");
    let d: PathBuf = tmp.path().join("prefs");
    let shared = Settings::open(&d).shared();
    let mut s = session(&shared);
    let human = RunOptions::human();
    let names: Vec<String> = s
        .registry()
        .all()
        .iter()
        .map(|d| d.name.to_string())
        .collect();
    assert!(names.len() > 280, "the whole table: {}", names.len());
    let chord = Chord::parse("Mod+Alt+Shift+F12", Os::Win).unwrap();
    for name in &names {
        // A key nothing uses, in the context that holds every other one.
        s.run_value(
            "keymap.set",
            json!({"key": "Mod+Alt+Shift+F12", "command": name, "context": "global", "replace": true}),
            &human,
        )
        .unwrap_or_else(|e| panic!("{name}: {}", e.message));
        let which = s
            .run_value(
                "keymap.which",
                json!({"key": "Mod+Alt+Shift+F12", "context": "global"}),
                &human,
            )
            .unwrap();
        assert_eq!(which.data["command"], json!(name), "{name}");
        // The text file holds it: a program that opens the folder gets the same row.
        let again = Settings::open(&d);
        assert_eq!(
            again.keymap.lookup("global", &chord).map(|(_, c)| c),
            Some(name.clone()),
            "{name}"
        );
    }
}

#[test]
fn a_second_command_on_the_same_key_is_a_conflict_that_is_reported() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let human = RunOptions::human();
    for command in ["track.mute", "track.solo"] {
        s.run_value(
            "keymap.set",
            json!({"key": "Mod+Alt+Shift+F11", "command": command, "context": "global"}),
            &human,
        )
        .unwrap();
    }
    let issues = s.run_value("keymap.conflicts", json!({}), &human).unwrap();
    assert!(
        issues
            .data
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "conflict" && i["key"] == "Mod+Alt+Shift+F11"),
        "{}",
        issues.data
    );
}

#[test]
fn each_layout_preset_is_a_complete_set_without_a_conflict_of_its_own() {
    let registry = shared_registry();
    let names: Vec<_> = xirius_prefs::catalog::PRESETS
        .iter()
        .map(|p| p.name)
        .collect();
    assert!(names.len() >= 4, "{names:?}");
    for name in names {
        let keymap = Keymap {
            preset: name.into(),
            ..Keymap::default()
        };
        assert_eq!(keymap.changed_rows(), 0, "{name}");
        let clashes: Vec<_> = keymap
            .conflicts(Os::Win)
            .into_iter()
            .filter(|i| matches!(i, Issue::Conflict { .. }))
            .collect();
        assert!(clashes.is_empty(), "{name}: {clashes:?}");
        // Every key of the preset runs a command that is in the table.
        for ((context, chord), commands) in preset_bindings(name) {
            for command in commands {
                let first = command.split_whitespace().next().unwrap();
                assert!(
                    registry.get(first).is_some(),
                    "{name}: {context} {chord} → {command}"
                );
            }
        }
    }
}
