use serde_json::json;
use xirius_core::session::{RunOptions, Session};

// That each marker edit is one step that undoes and that markers save and read back:
// `every_history_command_undoes_and_redoes`.
#[test]
fn a_marker_before_the_start_is_refused() {
    let mut s = Session::new("markers");
    let before = s.project().clone();
    assert!(
        s.run_value("marker.add", json!({"at": -1}), &RunOptions::human())
            .is_err()
    );
    assert_eq!(&before, s.project());
}
