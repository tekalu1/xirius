//! OPS-006 / CTX-002: one spelling for targets (`--ids`), positions (`--at`) and ranges
//! (`--range`). The older spellings keep working as aliases and the palette lists only the new ones.
use serde_json::{Value, json};
use xirius_core::command::Registry;
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human()).unwrap().data
}

fn id(v: &Value) -> String {
    v["id"]
        .as_str()
        .or_else(|| v["ids"][0].as_str())
        .unwrap()
        .to_string()
}

fn song() -> (Session, String, String) {
    let mut s = Session::new("aliases");
    let t = id(&run(&mut s, "track.add.audio", json!({})));
    let c = id(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 960, "file": "take.wav"}),
    ));
    (s, t, c)
}

fn set_loop(s: &mut Session) {
    run(s, "loop.start.set", json!({"at": 240}));
    run(s, "loop.end.set", json!({"at": 480}));
}

fn starts(s: &Session) -> Vec<i64> {
    let mut v: Vec<i64> = s
        .project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter().map(|c| c.start))
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn a_position_is_at_and_the_older_to_still_works() {
    let (mut a, _, ca) = song();
    let (mut b, _, cb) = song();
    run(&mut a, "clip.move", json!({"ids": [ca], "at": 1920}));
    run(&mut b, "clip.move", json!({"ids": [cb], "to": 1920}));
    assert_eq!(starts(&a), [1920]);
    assert_eq!(starts(&a), starts(&b));
    // Both spellings at once are two answers to one question.
    let err = b
        .run_value(
            "clip.move",
            json!({"ids": [cb], "at": 0, "to": 960}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("同じ引数"), "{err}");
}

#[test]
fn targets_are_ids_and_the_older_tracks_still_works() {
    let (mut s, t, _) = song();
    let bus = id(&run(&mut s, "track.add.bus", json!({})));
    run(
        &mut s,
        "edit.copy",
        json!({"ids": [t.clone(), bus.clone()]}),
    );
    let a = run(&mut s, "edit.paste", json!({}))["ids"].clone();
    run(&mut s, "edit.copy", json!({"tracks": [t, bus]}));
    let b = run(&mut s, "edit.paste", json!({}))["ids"].clone();
    assert_eq!(a.as_array().unwrap().len(), 2);
    assert_eq!(b.as_array().unwrap().len(), 2);
}

#[test]
fn a_command_with_one_id_takes_ids_with_one_element() {
    let (mut s, t, _) = song();
    let name = |s: &Session| {
        s.project()
            .tracks
            .iter()
            .find(|(id, _)| id.to_string() == t)
            .map(|(_, tr)| tr.name.clone())
            .unwrap()
    };
    run(
        &mut s,
        "track.rename",
        json!({"ids": [t.clone()], "name": "Vox"}),
    );
    assert_eq!(name(&s), "Vox");
    run(
        &mut s,
        "track.rename",
        json!({"id": t.clone(), "name": "Gtr"}),
    );
    assert_eq!(name(&s), "Gtr");
    let two = s.run_value(
        "track.rename",
        json!({"ids": [t.clone(), t.clone()], "name": "x"}),
        &RunOptions::human(),
    );
    assert!(two.is_err());
    let both = s.run_value(
        "track.rename",
        json!({"ids": [t.clone()], "id": t, "name": "x"}),
        &RunOptions::human(),
    );
    assert!(both.is_err());
}

#[test]
fn a_range_is_named_by_loop_by_two_positions_or_by_a_section() {
    let (mut s, t, c) = song();
    set_loop(&mut s);
    run(
        &mut s,
        "clip.split.range",
        json!({"ids": [c.clone()], "range": "loop"}),
    );
    let mut cuts = starts(&s);
    cuts.sort_unstable();
    assert_eq!(cuts, [0, 240, 480]);
    // The older spellings do the same on another song.
    let (mut o, _, co) = song();
    set_loop(&mut o);
    run(
        &mut o,
        "clip.split.range",
        json!({"ids": [co], "loop": true}),
    );
    assert_eq!(starts(&o), cuts);
    // Two positions: bar.beat.tick and a bare tick count both read.
    let (mut p, _, cp) = song();
    run(
        &mut p,
        "clip.split.range",
        json!({"ids": [cp], "range": "1.1.240..480"}),
    );
    assert_eq!(starts(&p), [0, 240, 480]);
    // A word that is no range is refused, and so is a missing loop.
    let (mut q, _, cq) = song();
    assert!(
        q.run_value(
            "clip.split.range",
            json!({"ids": [cq.clone()], "range": "nowhere"}),
            &RunOptions::human()
        )
        .is_err()
    );
    assert!(
        q.run_value(
            "clip.split.range",
            json!({"ids": [cq], "range": "loop"}),
            &RunOptions::human()
        )
        .is_err()
    );
    let _ = t;
}

#[test]
fn the_send_level_name_is_an_alias_of_send_set() {
    let reg = Registry::standard();
    assert_eq!(reg.canonical("send.level.set"), "send.set");
    assert_eq!(reg.canonical("send.set"), "send.set");
    assert_eq!(reg.canonical("no.such.command"), "no.such.command");
    assert!(reg.get("send.level.set").is_some());
    // The palette lists the new name only.
    assert!(reg.all().iter().all(|d| d.name != "send.level.set"));
    assert_eq!(reg.all().iter().filter(|d| d.name == "send.set").count(), 1);
}

#[test]
fn no_argument_alias_collides_with_an_argument_of_the_same_command() {
    for def in Registry::standard().all() {
        for a in &def.args {
            for alias in a.aliases {
                assert!(
                    def.args.iter().all(|b| b.name != *alias),
                    "{}: --{alias} is both an argument and an alias",
                    def.name
                );
            }
        }
    }
}
