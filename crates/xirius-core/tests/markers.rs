use serde_json::json;
use xirius_core::session::{RunOptions, Session};

#[test]
fn markers_roundtrip_and_each_edit_undoes_once() {
    let mut s = Session::new("markers");
    let opts = RunOptions::human();
    let a = s
        .run_value("marker.add", json!({"at": 3840, "name": "Verse"}), &opts)
        .unwrap()
        .data["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.run_value("marker.add", json!({"at": 7680}), &opts)
        .unwrap();
    for (name, args) in [
        ("marker.rename", json!({"id": a, "name": "Chorus"})),
        ("marker.color.set", json!({"ids": [a], "color": "#ff8800"})),
        ("marker.loop", json!({"id": a})),
        ("marker.delete", json!({"ids": [a]})),
    ] {
        let before = s.project().clone();
        s.run_value(name, args, &opts).unwrap();
        assert_ne!(s.project(), &before);
        s.run_value("history.undo", json!({}), &opts).unwrap();
        assert_eq!(s.project(), &before);
    }
    let bytes = serde_json::to_vec(s.project()).unwrap();
    let p: xirius_core::model::Project = serde_json::from_slice(&bytes).unwrap();
    p.validate_deep().unwrap();
    assert_eq!(&p, s.project());
    let before = s.project().clone();
    assert!(s.run_value("marker.add", json!({"at": -1}), &opts).is_err());
    assert_eq!(&before, s.project());
}
