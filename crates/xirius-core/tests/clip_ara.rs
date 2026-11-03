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
    let r = run(
        s,
        "clip.ara.attach.apply",
        json!({"ids": [id], "plugin": "vst3:m", "name": "M"}),
    );
    // The window plugs clips in through this step, so it says what it did.
    assert_eq!(r.message, "クリップ 1 件を ARA のプラグイン M に付けました");
}

fn refused(s: &mut Session, name: &str, args: Value) -> String {
    try_run(s, name, args).map(|_| ()).unwrap_err().message
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
fn only_a_plain_audio_clip_is_attached_and_each_reason_is_its_own() {
    let mut s = Session::new("ara");
    let apply = |id: &str| json!({"ids": [id], "plugin": "vst3:m"});
    let cases = [
        (
            "clip.tempo.set",
            json!({"bpm": 90.0}),
            "テンポが変更されています",
        ),
        (
            "clip.speed.set",
            json!({"percent": 150.0}),
            "速さが変更されています",
        ),
        (
            "clip.transpose",
            json!({"semis": 2}),
            "ピッチが変更されています",
        ),
        (
            "clip.tune",
            json!({"cents": -10}),
            "ピッチが変更されています",
        ),
    ];
    for (name, mut args, said) in cases {
        let a = audio(&mut s);
        args["ids"] = json!([a]);
        run(&mut s, name, args);
        let e = refused(&mut s, "clip.ara.attach.apply", apply(&a));
        assert_eq!(e, format!("クリップ {a}: {said}"), "{name}");
    }
    let a = audio(&mut s);
    run(
        &mut s,
        "clip.reverse.apply",
        json!({"clips": [{"id": a, "reverse": true, "offset": 0}]}),
    );
    let e = refused(&mut s, "clip.ara.attach.apply", apply(&a));
    assert_eq!(e, format!("クリップ {a}: 逆再生が有効です"));

    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let m = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "length": 1920}),
    ));
    let e = refused(&mut s, "clip.ara.attach.apply", apply(&m));
    assert_eq!(e, format!("クリップ {m}: 音声クリップを選択してください"));
    assert!(s.project().ara.is_empty());
}

#[test]
fn a_frozen_tracks_clips_are_neither_attached_nor_detached() {
    let mut s = Session::new("ara");
    let a = audio(&mut s);
    let t = s.project().clip(cid(&a)).unwrap().0.id.to_string();
    let b = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 7680, "length": 1920, "file": "b.wav"}),
    ));
    attach(&mut s, &a);
    run(
        &mut s,
        "track.freeze.apply",
        json!({"tracks": [{"id": t, "file": "frozen.wav", "start": 0}]}),
    );
    let frozen = "フリーズ中のトラックは編集できません";
    let e = refused(
        &mut s,
        "clip.ara.attach.apply",
        json!({"ids": [b], "plugin": "vst3:m"}),
    );
    assert_eq!(e, format!("クリップ {b}: {frozen}"));
    let e = refused(&mut s, "clip.ara.detach", json!({"ids": [a]}));
    assert_eq!(e, format!("クリップ {a}: {frozen}"));
    assert_eq!(s.project().ara.clips.len(), 1);
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
