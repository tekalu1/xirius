use serde_json::json;
use xirius_core::{Session, session::RunOptions};

#[test]
fn a_tempo_move_preserves_its_id_and_shape_and_is_one_undo() {
    let mut s = Session::new("tempo");
    let opts = RunOptions::human();
    s.run_value("tempo.set", json!({"at":1920,"bpm":90,"ramp":true}), &opts)
        .unwrap();
    s.run_value("tempo.set", json!({"at":3840,"bpm":150}), &opts)
        .unwrap();
    let point = s.project().tempo[1].clone();
    let before = s.project().clone();
    let steps = s.steps().len();
    s.run_value(
        "tempo.move",
        json!({"id":point.id,"at":4800,"bpm":100}),
        &opts,
    )
    .unwrap();
    assert_eq!(s.steps().len(), steps + 1);
    assert_eq!(s.project().tempo.len(), 3);
    assert_eq!(s.project().tempo[2].id, point.id);
    assert!(s.project().tempo[2].ramp);
    assert_eq!(
        (s.project().tempo[2].tick, s.project().tempo[2].bpm),
        (4800, 100.0)
    );
    s.undo(1).unwrap();
    assert_eq!(s.project().tempo, before.tempo);
    s.redo(1).unwrap();
    let hash = s.project().content_hash();
    assert!(
        s.run_value("tempo.move", json!({"id":point.id,"at":3840}), &opts)
            .is_err()
    );
    let first = s.project().tempo[0].id;
    assert!(
        s.run_value("tempo.move", json!({"id":first,"at":100}), &opts)
            .is_err()
    );
    assert_eq!(s.project().content_hash(), hash);
}
