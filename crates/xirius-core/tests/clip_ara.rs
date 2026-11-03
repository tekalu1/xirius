//! Audio clips put through an ARA plugin and taken out (EDT-023), and what copies, cuts and
//! deletions do to them.

use serde_json::{Value, json};
use xirius_core::CmdError;
use xirius_core::id::ClipId;
use xirius_core::session::{RunOptions, RunResult, Session};

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, CmdError> {
    s.run_value(name, args, &RunOptions::human())
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn audio(s: &mut Session) -> String {
    let t = id_of(&run(s, "track.add.audio", json!({})));
    id_of(&run(
        s,
        "clip.add.audio",
        json!({"track": t, "at": 1920, "length": 1920, "file": "a.wav"}),
    ))
}

fn attach(s: &mut Session, id: &str) {
    run(
        s,
        "clip.ara.attach.apply",
        json!({"ids": [id], "plugin": "vst3:m", "name": "M"}),
    );
}

fn cid(id: &str) -> ClipId {
    id.parse().unwrap()
}

#[test]
fn attach_and_detach_are_one_step_each() {
    let mut s = Session::new("ara");
    let a = audio(&mut s);
    attach(&mut s, &a);
    let row = &s.project().ara.clips[&cid(&a)];
    assert_eq!((row.plugin.as_str(), row.name.as_str()), ("vst3:m", "M"));
    assert!(s.is_dirty());

    run(&mut s, "clip.ara.detach", json!({"ids": [a]}));
    assert!(s.project().ara.is_empty());
    assert!(try_run(&mut s, "clip.ara.detach", json!({"ids": [a]})).is_err());

    s.undo(1).unwrap();
    assert!(s.project().ara.clips.contains_key(&cid(&a)));
    s.undo(1).unwrap();
    assert!(s.project().ara.is_empty());
}

#[test]
fn only_a_plain_audio_clip_is_attached() {
    let mut s = Session::new("ara");
    let a = audio(&mut s);
    run(&mut s, "clip.tempo.set", json!({"ids": [a], "bpm": 90.0}));
    assert!(
        try_run(
            &mut s,
            "clip.ara.attach.apply",
            json!({"ids": [a], "plugin": "vst3:m"}),
        )
        .is_err()
    );
    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let m = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "length": 1920}),
    ));
    assert!(
        try_run(
            &mut s,
            "clip.ara.attach.apply",
            json!({"ids": [m], "plugin": "vst3:m"}),
        )
        .is_err()
    );
    assert!(s.project().ara.is_empty());
}

#[test]
fn copies_and_cuts_clone_the_edits_and_a_deleted_clip_is_forgotten() {
    let mut s = Session::new("ara");
    let a = audio(&mut s);
    attach(&mut s, &a);

    let right = id_of(&run(&mut s, "clip.split", json!({"ids": [a], "at": 2880})));
    let row = &s.project().ara.clips[&cid(&right)];
    assert_eq!(row.clone_of, Some(cid(&a)));

    let copy = id_of(&run(&mut s, "clip.duplicate", json!({"ids": [a]})));
    assert_eq!(s.project().ara.clips[&cid(&copy)].clone_of, Some(cid(&a)));

    run(&mut s, "clip.delete", json!({"ids": [a]}));
    let p = s.project();
    assert!(!p.ara.clips.contains_key(&cid(&a)));
    assert_eq!(p.ara.clips[&cid(&copy)].clone_of, None);
    assert_eq!(p.ara.clips.len(), 2);
}
