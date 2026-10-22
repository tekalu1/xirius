//! B-1 / B-3: the table marks what only a window answers, the command list says so, and a
//! session with no screen refuses those commands instead of answering `ok: true`.

use serde_json::json;
use xirius_core::Registry;
use xirius_core::session::{RunOptions, Session};

#[test]
fn the_table_marks_the_commands_only_a_window_answers() {
    let registry = Registry::standard();
    for name in [
        "project.new",
        "view.mixer",
        "view.zoom.time",
        "edit.nudge.right",
        "loop.selection",
        "transport.start",
        "browser.put",
        "tool.select",
    ] {
        let def = registry.get(name).unwrap_or_else(|| panic!("{name}"));
        assert!(def.screen_only, "{name} is not marked screen_only");
    }
    // A command the CLI answers is not marked.
    assert!(!registry.get("track.add.audio").unwrap().screen_only);
    assert!(!registry.get("view.grid").unwrap().screen_only);
}

#[test]
fn ops_list_says_which_commands_need_a_screen() {
    let mut s = Session::new("list");
    let out = s
        .run_value("ops.list", json!({}), &RunOptions::human())
        .unwrap();
    let rows = out.data.as_array().unwrap();
    let row = |name: &str| {
        rows.iter()
            .find(|r| r["name"] == name)
            .unwrap_or_else(|| panic!("{name}"))
    };
    assert_eq!(row("view.mixer")["screen_only"], true);
    assert_eq!(row("tool.select")["screen_only"], true);
    assert_eq!(row("track.add.audio")["screen_only"], false);
}

#[test]
fn a_command_that_only_changes_the_screen_is_not_a_quiet_success_without_one() {
    let mut s = Session::new("cli");
    let h = RunOptions::human();
    for name in ["tool.select", "snap.invert", "project.new", "view.mixer"] {
        let error = s.run_value(name, json!({}), &h).expect_err(name);
        assert!(
            error.message.contains("画面でだけ使えます"),
            "{name}: {error}"
        );
    }
}
