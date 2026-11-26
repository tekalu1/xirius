use serde_json::{Value, json};
use xirius_core::id::PluginId;
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
fn master_and_track_hold_sixty_four_inserts_and_nine_sends() {
    let mut s = Session::new("many");
    let t = track(&mut s);
    let b = track(&mut s);
    for _ in 0..64 {
        plugin(&mut s, t, "EQ");
        plugin(&mut s, TrackId::MASTER, "EQ");
    }
    assert_eq!(s.project().tracks[&t].inserts.len(), 64);
    assert_eq!(s.project().tracks[&TrackId::MASTER].inserts.len(), 64);
    let to = run(&mut s, "track.add.bus", json!({}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for _ in 0..9 {
        run(&mut s, "send.add", json!({"ids":[b.to_string()],"to":to}));
    }
    assert_eq!(s.project().tracks[&b].sends.len(), 9);
}
