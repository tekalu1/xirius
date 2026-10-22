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
fn settings_duplicate_keeps_channel_settings_but_has_no_timeline_content() {
    let mut s = Session::new("copy");
    let t = track(&mut s);
    plugin(&mut s, t, "EQ");
    run(
        &mut s,
        "clip.add.midi",
        json!({"track":t.to_string(),"at":0,"length":960}),
    );
    run(
        &mut s,
        "track.volume.set",
        json!({"ids":[t.to_string()],"db":-12}),
    );
    let n: TrackId = run(
        &mut s,
        "track.duplicate",
        json!({"ids":[t.to_string()],"mode":"settings"}),
    )["ids"][0]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let p = s.project();
    assert!(p.tracks[&n].clips.is_empty());
    assert!(p.tracks[&n].automation.is_empty());
    assert_eq!(p.tracks[&n].volume_db, -12.0);
    assert_ne!(p.tracks[&n].inserts[0].id, p.tracks[&t].inserts[0].id);
    let n: TrackId = run(
        &mut s,
        "track.duplicate",
        json!({"ids":[t.to_string()],"mode":"all"}),
    )["ids"][0]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(s.project().tracks[&n].clips.len(), 1);
}
