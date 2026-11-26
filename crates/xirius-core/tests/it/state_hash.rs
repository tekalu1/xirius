use serde_json::json;
use xirius_core::{Session, session::RunOptions};

#[test]
fn edit_preview_and_undo_return_actual_state_hashes() {
    let mut session = Session::new("hashes");
    let before = session.project().content_hash();
    let edit = session
        .run_value(
            "track.add.audio",
            json!({"name": "Audio"}),
            &RunOptions::human(),
        )
        .unwrap();
    let after = session.project().content_hash();
    assert_ne!(before, after);
    assert_eq!(
        edit.data["state_hash"],
        json!({"before": before, "after": after})
    );
    assert!(!edit.changes.is_empty());
    let preview = session
        .run_value(
            "track.add.audio",
            json!({"name": "Preview"}),
            &RunOptions::human().dry(),
        )
        .unwrap();
    assert_eq!(
        preview.data["state_hash"],
        json!({"before": after, "after": after})
    );
    assert_eq!(session.project().content_hash(), after);
    let undo = session
        .run_value("history.undo", json!({}), &RunOptions::human())
        .unwrap();
    assert_eq!(undo.data["state_hash"]["before"], after);
    assert_eq!(
        undo.data["state_hash"]["after"],
        session.project().content_hash()
    );
}
