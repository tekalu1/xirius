use serde_json::{Value, json};
use xirius_core::model::Output;
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
#[test]
fn new_bus_routes_all_sources_in_one_undo() {
    let mut s = Session::new("bus");
    let a = track(&mut s);
    let b = track(&mut s);
    let to: TrackId = run(
        &mut s,
        "track.add.bus",
        json!({"sources":[a.to_string(),b.to_string()]}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    for t in [a, b] {
        assert_eq!(s.project().tracks[&t].output, Some(Output::Bus(to)));
    }
    run(&mut s, "history.undo", json!({}));
    assert!(!s.project().tracks.contains_key(&to));
    assert_eq!(s.project().tracks[&a].output, None);
    run(&mut s, "history.redo", json!({}));
    assert_eq!(s.project().tracks[&b].output, Some(Output::Bus(to)));
}
