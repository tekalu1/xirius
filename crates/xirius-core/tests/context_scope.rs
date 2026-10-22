use serde_json::json;
use xirius_core::id::AnyId;
use xirius_core::session::{RunOptions, Session};

#[test]
fn folder_selection_and_open_state_use_the_command_table_without_song_history() {
    let mut s = Session::new("Folders");
    let opts = RunOptions::human();
    let f = s.run_value("folder.add", json!({}), &opts).unwrap().data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let nested = s
        .run_value("folder.add", json!({"into":f}), &opts)
        .unwrap()
        .data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let t = s
        .run_value("track.add.audio", json!({"into":nested}), &opts)
        .unwrap()
        .data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let before = s.project().clone();
    let steps = s.steps().len();
    s.run_value("select.in_folder", json!({"ids":[f,nested]}), &opts)
        .unwrap();
    assert_eq!(s.state.selection, vec![t.parse::<AnyId>().unwrap()]);
    s.run_value("view.folders", json!({"state":"closed"}), &opts)
        .unwrap();
    assert!(s.state.view.folders_closed);
    s.run_value("view.folders", json!({"state":"open"}), &opts)
        .unwrap();
    assert!(!s.state.view.folders_closed);
    assert_eq!(s.project(), &before);
    assert_eq!(s.steps().len(), steps);
}
