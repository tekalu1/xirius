use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::id::{SendId, TrackId};
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human()).unwrap().data
}
fn id(v: Value) -> String {
    v["id"]
        .as_str()
        .or_else(|| v["ids"][0].as_str())
        .unwrap()
        .to_string()
}

#[test]
fn a_cut_track_pastes_after_its_source_is_gone_with_new_ids_and_envelopes() {
    let mut s = Session::new("track clipboard");
    let bus = id(run(&mut s, "track.add.bus", json!({})));
    let t = id(run(&mut s, "track.add.audio", json!({})));
    let c = id(run(
        &mut s,
        "clip.add.audio",
        json!({"track": t,"at": 0,"length": 960,"file": "take.wav"}),
    ));
    run(&mut s, "clip.fade.set", json!({"ids":[c],"in": 120}));
    run(&mut s, "send.add", json!({"ids":[t],"to": bus,"level": -6}));
    run(&mut s, "edit.cut", json!({"tracks":[t,bus]}));
    let before = s.project().clone();
    let made = run(&mut s, "edit.paste", json!({}))["ids"]
        .as_array()
        .unwrap()
        .clone();
    let tid: TrackId = made[0].as_str().unwrap().parse().unwrap();
    let bid: TrackId = made[1].as_str().unwrap().parse().unwrap();
    let tr = &s.project().tracks[&tid];
    assert_eq!(tr.sends[0].to, bid);
    assert_ne!(tr.clips[0].id.to_string(), c);
    assert!(s.project().clip_fx.contains_key(&tr.clips[0].id));
    s.project().validate_deep().unwrap();
    run(&mut s, "history.undo", json!({}));
    let mut now = s.project().clone();
    now.next_id = before.next_id;
    assert_eq!(now, before);
}

#[test]
fn muted_send_keeps_its_level_and_editable_curve_and_unmutes_in_one_step() {
    let mut s = Session::new("muted send");
    let bus = id(run(&mut s, "track.add.bus", json!({})));
    let t = id(run(&mut s, "track.add.audio", json!({})));
    let send = id(run(
        &mut s,
        "send.add",
        json!({"ids":[t],"to":bus,"level":-9}),
    ));
    let target = format!("send:{send}");
    run(&mut s, "auto.lane.add", json!({"track":t,"target":target}));
    run(
        &mut s,
        "auto.point.add",
        json!({"track":t,"target":target,"at":0,"value":-3}),
    );
    let before = s.project().clone();
    run(&mut s, "send.mute", json!({"id":send}));
    let tid: TrackId = t.parse().unwrap();
    let sid: SendId = send.parse().unwrap();
    let tr = &s.project().tracks[&tid];
    assert_eq!(tr.sends[0].muted_level, Some(-9.0));
    assert_eq!(tr.sends[0].level_db, xirius_core::model::MIN_DB);
    let lane = tr
        .automation
        .iter()
        .find(|l| l.target == AutoTarget::Send(sid))
        .unwrap();
    assert_eq!(lane.value_at(0.0), None);
    assert_eq!(lane.curve_at(0.0), Some(-3.0));
    run(&mut s, "history.undo", json!({}));
    assert_eq!(&before, s.project());
    run(&mut s, "send.mute", json!({"id":send}));
    run(&mut s, "send.set", json!({"id":send,"level":-12}));
    assert_eq!(
        s.project().tracks[&tid].sends[0].level_db,
        xirius_core::model::MIN_DB
    );
    run(&mut s, "send.mute", json!({"id":send,"off":true}));
    assert_eq!(s.project().tracks[&tid].sends[0].level_db, -12.0);
    assert_eq!(
        s.project().tracks[&tid].automation[0].value_at(0.0),
        Some(-3.0)
    );
}
