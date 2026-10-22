use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::id::PluginId;
use xirius_core::model::PluginKind;
use xirius_core::{Session, id::TrackId, session::RunOptions};
fn run(s: &mut Session, n: &str, a: Value) -> Value {
    s.run_value(n, a, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{n}: {e:?}"))
        .data
}
fn track(s: &mut Session) -> TrackId {
    run(s, "track.add.instrument", json!({}))["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}
fn plugin(s: &mut Session, t: TrackId, name: &str) -> PluginId {
    run(s, "plugin.add", json!({"track":t.to_string(),"name":name}))["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn moves_copies_and_duplicates_keep_state_and_automation_without_changing_routes() {
    let mut s = Session::new("mix");
    let a = track(&mut s);
    let b = track(&mut s);
    let id = plugin(&mut s, a, "EQ");
    run(
        &mut s,
        "plugin.state.set",
        json!({"id":id.to_string(),"state":"b3BhcXVl"}),
    );
    run(
        &mut s,
        "auto.lane.add",
        json!({"track":a.to_string(),"target":format!("param:{id}:b1_gain")}),
    );
    let original = s.project().tracks[&a].inserts[0].clone();
    run(
        &mut s,
        "plugin.move",
        json!({"id":id.to_string(),"track":b.to_string(),"index":0}),
    );
    assert!(s.project().tracks[&a].inserts.is_empty());
    assert!(s.project().tracks[&a].automation.is_empty());
    assert_eq!(s.project().tracks[&b].inserts[0], original);
    assert_eq!(
        s.project().tracks[&b].automation[0].target,
        AutoTarget::Param(id, "b1_gain".into())
    );
    run(&mut s, "history.undo", json!({}));
    assert_eq!(s.project().tracks[&a].inserts[0], original);
    let c: PluginId = run(
        &mut s,
        "plugin.copy_to",
        json!({"id":id.to_string(),"track":b.to_string()}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(c, id);
    assert_eq!(s.project().tracks[&b].inserts[0].state, original.state);
    assert_eq!(
        s.project().tracks[&b].automation[0].target,
        AutoTarget::Param(c, "b1_gain".into())
    );
    run(&mut s, "plugin.duplicate", json!({"id":c.to_string()}));
    assert_eq!(s.project().tracks[&b].inserts.len(), 2);
    assert_eq!(s.project().tracks[&b].output, None);
}
#[test]
fn instrument_stays_first_and_invalid_destination_is_atomic() {
    let mut s = Session::new("order");
    let t = track(&mut s);
    let other = track(&mut s);
    let i: PluginId = run(
        &mut s,
        "plugin.add",
        json!({"track":t.to_string(),"name":"Sampler","kind":"instrument"}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let e = plugin(&mut s, t, "EQ");
    run(&mut s, "plugin.move", json!({"id":e.to_string(),"index":0}));
    assert_eq!(
        s.project().tracks[&t].inserts[0].kind,
        PluginKind::Instrument
    );
    let before = s.project().clone();
    assert!(
        s.run_value(
            "plugin.copy_to",
            json!({"id":i.to_string(),"track":t.to_string()}),
            &RunOptions::human()
        )
        .is_err()
    );
    assert_eq!(*s.project(), before);
    run(
        &mut s,
        "plugin.move",
        json!({"id":i.to_string(),"track":other.to_string()}),
    );
    assert_eq!(s.project().tracks[&other].inserts[0].id, i);
}
