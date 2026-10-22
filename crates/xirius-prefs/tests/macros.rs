//! KEY-015: macros through the session, as the CLI, the MCP server and the screen run them.

use std::path::PathBuf;

use serde_json::{Value, json};
use xirius_core::error::ErrorCode;
use xirius_core::session::{RunOptions, Session};
use xirius_prefs::{Chord, File, Os, Settings, Shared};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("xirius-macros-t-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn session(shared: &Shared) -> Session {
    let mut s = Session::new("t");
    xirius_prefs::attach(&mut s, shared.clone());
    s
}

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

/// A clip with one note at pitch 60; the note's id.
fn voice(s: &mut Session) -> String {
    let t = run(s, "track.add.instrument", json!({"name": "lead"}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let c = run(
        s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "bars": 2.0}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(
        s,
        "note.add",
        json!({"clip": c, "at": 0, "pitch": 60, "length": 3840, "velocity": 100}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn pitch(s: &Session) -> u8 {
    s.project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .flat_map(|c| c.notes().iter())
        .next()
        .map(|n| n.pitch)
        .unwrap()
}

fn steps(lines: &[String]) -> Value {
    Value::Array(lines.iter().map(|l| json!({"command": l})).collect())
}

#[test]
fn a_macro_is_made_listed_run_with_one_undo_and_kept_after_reopening() {
    let d = dir("run");
    let shared = Settings::open(&d).shared();
    let mut s = session(&shared);
    let note = voice(&mut s);
    let lines = vec![
        format!("note.transpose --ids {note} --by 1"),
        format!("note.transpose --ids {note} --by 2"),
        format!("note.transpose --ids {note} --by 4"),
    ];
    let made = run(
        &mut s,
        "macro.set",
        json!({"name": "打ち込みを整える", "steps": steps(&lines)}),
    );
    assert_eq!(made["name"], "打ち込みを整える");
    // Settings changes are not steps of the song; macros are not either.
    let before = s.steps().len();

    let listed = run(&mut s, "macro.list", json!({}));
    assert_eq!(listed["macros"][0]["steps"].as_array().unwrap().len(), 3);
    assert_eq!(listed["macros"][0]["context"], "roll");

    assert_eq!(pitch(&s), 60);
    run(&mut s, "macro.run", json!({"name": "打ち込みを整える"}));
    assert_eq!(pitch(&s), 67);
    assert_eq!(s.steps().len(), before + 3);

    // The three steps are taken back and done again with one undo and one redo.
    s.undo(1).unwrap();
    assert_eq!(pitch(&s), 60);
    s.redo(1).unwrap();
    assert_eq!(pitch(&s), 67);
    s.undo(1).unwrap();
    assert_eq!(pitch(&s), 60);
    assert!(s.can_redo());

    // Another session on the same folder has it (the file was written at once).
    let reopened = Settings::open(&d);
    assert!(reopened.problems.is_empty(), "{:?}", reopened.problems);
    assert_eq!(reopened.macros.list().len(), 1);
    assert_eq!(reopened.macros.list()[0].steps, lines);
    assert!(d.join("macros.toml").is_file());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn two_runs_are_two_undos() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let note = voice(&mut s);
    run(
        &mut s,
        "macro.set",
        json!({"name": "up", "steps": steps(&[
            format!("note.transpose --ids {note} --by 1"),
            format!("note.transpose --ids {note} --by 1"),
        ])}),
    );
    run(&mut s, "macro.run", json!({"name": "up"}));
    run(&mut s, "macro.run", json!({"name": "up"}));
    assert_eq!(pitch(&s), 64);
    s.undo(1).unwrap();
    assert_eq!(pitch(&s), 62);
    s.undo(1).unwrap();
    assert_eq!(pitch(&s), 60);
    // Undoing two at once takes both runs.
    s.redo(2).unwrap();
    assert_eq!(pitch(&s), 64);
}

#[test]
fn a_key_of_the_macro_is_in_the_context_of_its_first_command_and_follows_rename_and_remove() {
    let d = dir("keys");
    let shared = Settings::open(&d).shared();
    let mut s = session(&shared);
    run(
        &mut s,
        "macro.set",
        json!({"name": "整える", "steps": steps(&["note.quantize --grid 1/16".into()])}),
    );
    run(
        &mut s,
        "keymap.set",
        json!({"context": "roll", "key": "Alt+M", "command": "macro.run --name 整える"}),
    );
    let key = Chord::parse("Alt+M", Os::Win).unwrap();
    let k = |shared: &Shared| shared.lock().unwrap().keymap.clone();
    assert_eq!(
        k(&shared).lookup("roll", &key).unwrap().1,
        "macro.run --name 整える"
    );

    // Renaming carries the key.
    run(
        &mut s,
        "macro.rename",
        json!({"name": "整える", "new-name": "ならす"}),
    );
    assert_eq!(
        k(&shared).lookup("roll", &key).unwrap().1,
        "macro.run --name ならす"
    );
    let listed = run(&mut s, "macro.list", json!({}));
    assert_eq!(listed["macros"][0]["name"], "ならす");
    assert_eq!(listed["macros"][0]["keys"][0]["key"], "Alt+M");

    // A first command in another place moves the key there.
    run(
        &mut s,
        "macro.set",
        json!({"name": "ならす", "steps": steps(&["clip.split".into()])}),
    );
    let moved = k(&shared);
    assert!(moved.lookup("roll", &key).is_none());
    assert_eq!(
        moved.lookup("arrange", &key).unwrap().1,
        "macro.run --name ならす"
    );

    // The key is on disk too.
    let on_disk = Settings::open(&d);
    assert_eq!(
        on_disk.keymap.lookup("arrange", &key).unwrap().1,
        "macro.run --name ならす"
    );

    // Removing the macro takes its keys with it.
    run(&mut s, "macro.remove", json!({"name": "ならす"}));
    assert!(k(&shared).lookup("arrange", &key).is_none());
    assert!(Settings::open(&d).macros.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_step_must_be_a_command_that_edits_the_song_and_a_name_must_be_free() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    for bad in [
        "prefs.set --key ui.theme --value light",
        "macro.run --name a",
        "no.such.command",
        "note.transpose --by banana",
    ] {
        let e = s
            .run_value(
                "macro.set",
                json!({"name": "a", "steps": steps(&[bad.into()])}),
                &RunOptions::human(),
            )
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidArgs, "{bad}: {e:?}");
    }
    assert!(shared.lock().unwrap().macros.is_empty());

    run(&mut s, "macro.set", json!({"name": "a"}));
    run(&mut s, "macro.set", json!({"name": "b"}));
    let e = s
        .run_value(
            "macro.rename",
            json!({"name": "a", "new-name": "b"}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs);
    let e = s
        .run_value("macro.remove", json!({"name": "x"}), &RunOptions::human())
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    // A macro without steps has nothing to run.
    let e = s
        .run_value("macro.run", json!({"name": "a"}), &RunOptions::human())
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NothingToDo);
    // A dry run changes nothing.
    let r = s
        .run_value(
            "macro.set",
            json!({"name": "c"}),
            &RunOptions {
                dry_run: true,
                ..RunOptions::human()
            },
        )
        .unwrap();
    assert!(r.message.starts_with("（試行）"));
    assert_eq!(shared.lock().unwrap().macros.list().len(), 2);
}

#[test]
fn macros_toml_that_cannot_be_read_is_kept_and_the_rest_still_works() {
    let d = dir("old");
    std::fs::create_dir_all(&d).unwrap();
    // A settings folder from before macros has no macros.toml; one with a broken line is
    // reported and the good macros stay.
    let shared = Settings::open(&d).shared();
    assert!(shared.lock().unwrap().macros.is_empty());
    std::fs::write(
        d.join("macros.toml"),
        "[[macro]]\nname = \"ok\"\nsteps = [\"note.mute\"]\n[[macro]]\nsteps = []\n",
    )
    .unwrap();
    let s = Settings::open(&d);
    assert_eq!(s.macros.list().len(), 1);
    assert_eq!(s.problems.len(), 1, "{:?}", s.problems);
    let _ = File::Macros;
    let _ = std::fs::remove_dir_all(&d);
}
