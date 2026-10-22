use serde_json::{Value, json};
use xirius_core::id::FolderId;
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
fn unpack_restores_nonconsecutive_tracks_across_a_serialization() {
    let mut s = Session::new("folder");
    let a = track(&mut s);
    let _b = track(&mut s);
    let c = track(&mut s);
    let root = s.project().root.clone();
    let f: FolderId = run(
        &mut s,
        "folder.pack",
        json!({"ids":[c.to_string(),a.to_string()]}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(s.project().folders[&f].children.len(), 2);
    let p = serde_json::from_slice(&serde_json::to_vec(s.project()).unwrap()).unwrap();
    let mut reopened = Session::from_project(p);
    run(&mut reopened, "folder.unpack", json!({"id":f.to_string()}));
    assert_eq!(reopened.project().root, root);
    run(&mut s, "history.undo", json!({}));
    assert_eq!(s.project().root, root);
}
