//! B-10: naming a song into a folder that already holds something says what to do instead.

use serde_json::json;
use xirius_core::error::ErrorCode;
use xirius_core::session::{RunOptions, Session};

#[test]
fn a_folder_with_something_in_it_says_which_folder_to_choose() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("stems1");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("Kick.wav"), b"not really a wav").unwrap();

    let mut s = Session::new("conflict");
    let e = s
        .run_value(
            "project.save-as",
            json!({"path": dir.to_string_lossy()}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    assert!(e.message.contains("空のフォルダーではありません"), "{e:?}");
    assert!(
        e.message
            .contains("空のフォルダーか、まだ無いフォルダーを指定してください"),
        "{e:?}"
    );
}
