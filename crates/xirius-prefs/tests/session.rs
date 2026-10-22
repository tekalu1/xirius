//! The settings' commands as the CLI, the MCP server and the screen run them: through a
//! session with the extension attached.

use serde_json::json;
use xirius_core::error::ErrorCode;
use xirius_core::session::{RunOptions, Session};
use xirius_prefs::{Chord, Os, Settings, Shared};

/// A folder of the test's own, removed when dropped (also when the test fails).
fn dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-prefs-t-{name}-"))
        .tempdir()
        .unwrap()
}

fn session(shared: &Shared) -> Session {
    let mut s = Session::new("t");
    xirius_prefs::attach(&mut s, shared.clone());
    s
}

#[test]
fn a_change_is_saved_and_another_session_sees_it() {
    let d_tmp = dir("shared");
    let d = d_tmp.path().to_path_buf();
    let a = Settings::open(&d).shared();
    let mut s = session(&a);
    let r = s
        .run_value(
            "prefs.set",
            json!({"key": "edit.automation.follows-clips", "value": "false"}),
            &RunOptions::human(),
        )
        .unwrap();
    assert_eq!(r.data["edit.automation.follows-clips"], false);
    // Settings changes are not steps of the song's history.
    assert!(!s.can_undo());
    let text = std::fs::read_to_string(d.join("prefs.toml")).unwrap();
    assert!(text.contains("follows-clips = false"), "{text}");

    // Another program (the CLI against the open screen) reads the same folder.
    let b = Settings::open(&d).shared();
    let mut other = session(&b);
    let r = other
        .run_value(
            "prefs.get",
            json!({"key": "edit.automation.follows-clips"}),
            &RunOptions::human(),
        )
        .unwrap();
    assert_eq!(r.data["edit.automation.follows-clips"], false);
    other
        .run_value(
            "prefs.set",
            json!({"key": "edit.automation.locks-clips", "value": "true"}),
            &RunOptions::human(),
        )
        .unwrap();
    // The first session reads the file again before its next command.
    let r = s
        .run_value(
            "prefs.get",
            json!({"key": "edit.automation.locks-clips"}),
            &RunOptions::human(),
        )
        .unwrap();
    assert_eq!(r.data["edit.automation.locks-clips"], true);
    assert!(a.lock().unwrap().prefs.bool("edit.automation.locks-clips"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_bad_value_is_refused_and_a_dry_run_changes_nothing() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let e = s
        .run_value(
            "prefs.set",
            json!({"key": "audio.buffer", "value": "5000"}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs);
    let e = s
        .run_value(
            "prefs.set",
            json!({"key": "no.such", "value": "1"}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs);

    let r = s
        .run_value(
            "prefs.set",
            json!({"key": "ui.theme", "value": "light"}),
            &RunOptions::human().dry(),
        )
        .unwrap();
    assert!(r.message.starts_with("（試行）"), "{}", r.message);
    assert_eq!(shared.lock().unwrap().prefs.text("ui.theme"), "dark");
}

#[test]
fn an_ai_does_not_change_the_mcp_settings() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let ai = RunOptions::ai("claude");
    let e = s
        .run_value(
            "prefs.set",
            json!({"key": "ai.mcp.listen", "value": "network"}),
            &ai,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Denied);
    s.run_value(
        "prefs.set",
        json!({"key": "ai.mcp.listen", "value": "network"}),
        &RunOptions::human(),
    )
    .unwrap();
    s.run_value(
        "prefs.set",
        json!({"key": "ui.theme", "value": "light"}),
        &ai,
    )
    .unwrap();
    // Putting everything back leaves the MCP settings as the person set them.
    s.run_value("prefs.reset", json!({}), &ai).unwrap();
    let g = shared.lock().unwrap();
    assert_eq!(g.prefs.text("ai.mcp.listen"), "network");
    assert_eq!(g.prefs.text("ui.theme"), "dark");
    drop(g);
    let e = s
        .run_value("keymap.export", json!({"to": "keys.toml"}), &ai)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Denied);
}

#[test]
fn a_key_conflict_is_reported_and_a_preset_keeps_the_changed_rows() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    // Q quantizes in the roll; giving it a second command is a conflict.
    let r = s
        .run_value(
            "keymap.set",
            json!({"key": "Q", "command": "note.delete", "context": "roll"}),
            &h,
        )
        .unwrap();
    assert!(r.message.contains("衝突"), "{}", r.message);
    let r = s.run_value("keymap.conflicts", json!({}), &h).unwrap();
    assert!(
        r.data
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "conflict" && i["key"] == "Q"),
        "{}",
        r.data
    );
    // Replacing takes the key from the other command.
    s.run_value(
        "keymap.set",
        json!({"key": "Q", "command": "note.delete", "context": "roll", "replace": true}),
        &h,
    )
    .unwrap();
    let r = s
        .run_value("keymap.which", json!({"key": "Q", "context": "roll"}), &h)
        .unwrap();
    assert_eq!(r.data["command"], "note.delete");

    // A command with arguments can be bound; an unknown one cannot.
    s.run_value(
        "keymap.set",
        json!({"key": "Mod+Alt+V", "command": "note.transpose --by 12", "context": "roll"}),
        &h,
    )
    .unwrap();
    let e = s
        .run_value(
            "keymap.set",
            json!({"key": "Mod+Alt+B", "command": "no.such"}),
            &h,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs);

    let r = s
        .run_value("keymap.preset", json!({"name": "cubase"}), &h)
        .unwrap();
    // note.delete gained Q, note.quantize lost it, and the transpose is new.
    assert_eq!(r.data["kept"], 3);
    let g = shared.lock().unwrap();
    assert_eq!(g.keymap.preset, "cubase");
    let q = Chord::parse("Q", Os::Win).unwrap();
    assert_eq!(g.keymap.lookup("roll", &q).unwrap().1, "note.delete");
    let v = Chord::parse("Mod+Alt+V", Os::Win).unwrap();
    assert_eq!(
        g.keymap.lookup("roll", &v).unwrap().1,
        "note.transpose --by 12"
    );
    drop(g);

    // Back to the preset.
    s.run_value("keymap.reset", json!({}), &h).unwrap();
    assert_eq!(shared.lock().unwrap().keymap.changed_rows(), 0);
    let e = s
        .run_value("keymap.unset", json!({"key": "Mod+Alt+F12"}), &h)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NothingToDo);
}

#[test]
fn a_hold_is_set_and_put_back() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    s.run_value(
        "mousemap.set",
        json!({"context": "roll.select.hold", "modifier": "Mod+Alt", "action": "tool:mute"}),
        &h,
    )
    .unwrap();
    let r = s
        .run_value("mousemap.get", json!({"context": "roll.select.hold"}), &h)
        .unwrap();
    assert!(r.message.contains("tool:mute"), "{}", r.message);
    let e = s
        .run_value(
            "mousemap.set",
            json!({"context": "roll.select.hold", "modifier": "Alt", "action": "note.delete"}),
            &h,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs);
    let r = s.run_value("mousemap.reset", json!({}), &h).unwrap();
    assert!(r.message.starts_with('1'), "{}", r.message);
}

#[test]
fn a_knob_is_given_to_a_part_and_comes_back_after_a_reopen() {
    let d_tmp = dir("midimap");
    let d = d_tmp.path().to_path_buf();
    let shared = Settings::open(&d).shared();
    let mut s = session(&shared);
    s.run_value(
        "track.add.audio",
        json!({"name": "Vocal"}),
        &RunOptions::human(),
    )
    .unwrap();
    let id = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .unwrap()
        .id
        .to_string();

    let r = s
        .run_value(
            "midimap.set",
            json!({"message": "cc:21", "target": format!("track.volume:{id}"), "port": "Keys"}),
            &RunOptions::human(),
        )
        .unwrap();
    assert!(
        r.message.contains("CC 21") && r.message.contains("Vocal"),
        "{}",
        r.message
    );
    assert_eq!(r.data["rows"].as_array().unwrap().len(), 1);

    // One part has one knob for a value: a second knob takes it over.
    s.run_value(
        "midimap.set",
        json!({"message": "cc:22", "target": format!("track.volume:{id}"), "port": "Keys"}),
        &RunOptions::human(),
    )
    .unwrap();
    let listed = s
        .run_value("midimap.list", json!({}), &RunOptions::human())
        .unwrap();
    let rows = listed.data["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["message"], "cc:22");

    // A part that is not in the song is refused.
    let e = s
        .run_value(
            "midimap.set",
            json!({"message": "cc:30", "target": "track.volume:no-such"}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert_ne!(e.code, ErrorCode::NothingToDo);

    // The file holds it, and a program opening the folder again reads it.
    let text = std::fs::read_to_string(d.join("midimap.toml")).unwrap();
    assert!(text.contains("cc:22"), "{text}");
    let again = Settings::open(&d);
    assert_eq!(again.midimap.rows().len(), 1);
    assert!(again.problems.is_empty(), "{:?}", again.problems);

    s.run_value(
        "midimap.unset",
        json!({"message": "cc:22"}),
        &RunOptions::human(),
    )
    .unwrap();
    let e = s
        .run_value("midimap.unset", json!({}), &RunOptions::human())
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NothingToDo);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_alternative_tool_is_chosen_by_name_or_taken_in_turn_and_default_leaves_no_trace() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    let alt = |s: &mut Session, args: serde_json::Value| {
        s.run_value("tool.alt", args, &h).unwrap().message
    };
    // Range is the default; asking for it changes nothing on file.
    alt(&mut s, json!({"tool": "range"}));
    assert_eq!(shared.lock().unwrap().mousemap.changed_count(), 0);
    // Without a tool it is the next one, round the list.
    let mut seen = Vec::new();
    for _ in 0..xirius_core::commands::ALT_TOOLS.len() {
        seen.push(alt(&mut s, json!({})));
    }
    assert!(seen[0].contains("cut"), "{seen:?}");
    assert!(seen.last().unwrap().contains("range"), "{seen:?}");
    assert_eq!(shared.lock().unwrap().mousemap.changed_count(), 0);
    // The roll takes only the tools it has.
    for _ in 0..8 {
        let m = alt(&mut s, json!({"area": "roll"}));
        let t = m.rsplit(' ').next().unwrap();
        assert!(
            ["cut", "erase", "draw", "mute", "listen"].contains(&t),
            "{m}"
        );
    }
    let e = s
        .run_value("tool.alt", json!({"tool": "select"}), &h)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs);
}

#[test]
fn ops_004_the_alternative_tool_lives_only_in_the_mouse_table() {
    let d_tmp = dir("alt-tool");
    let d = d_tmp.path().to_path_buf();
    // A file from before the setting was removed still reads, and the value is dropped quietly.
    std::fs::write(
        d.join("prefs.toml"),
        "[edit.arrow]\nalternative-tool = \"mute\"\nmode = \"extended\"\n",
    )
    .unwrap();
    let shared = Settings::open(&d).shared();
    assert_eq!(shared.lock().unwrap().problems, Vec::<String>::new());
    assert_eq!(
        shared.lock().unwrap().prefs.text("edit.arrow.mode"),
        "extended"
    );
    let mut s = session(&shared);
    let h = RunOptions::human();
    let hold = |shared: &Shared| {
        shared
            .lock()
            .unwrap()
            .mousemap
            .get("arrange.select.hold", "Mod")
    };
    assert_eq!(hold(&shared), "tool:range");
    s.run_value("tool.alt", json!({"tool": "mute"}), &h)
        .unwrap();
    assert_eq!(hold(&shared), "tool:mute");
    // The setting is gone: nothing writes it, and it is not a name any more.
    let e = s
        .run_value(
            "prefs.set",
            json!({"key": "edit.arrow.alternative-tool", "value": "erase"}),
            &h,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs);
    // The next save of the settings leaves it out.
    s.run_value(
        "prefs.set",
        json!({"key": "edit.arrange.link", "value": "true"}),
        &h,
    )
    .unwrap();
    let prefs = std::fs::read_to_string(d.join("prefs.toml")).unwrap();
    assert!(!prefs.contains("alternative-tool"), "{prefs}");
    assert!(prefs.contains("mode = \"extended\""), "{prefs}");
    // It survives a save and a reopen.
    let again = Settings::open(&d).shared();
    assert_eq!(
        again
            .lock()
            .unwrap()
            .mousemap
            .get("arrange.select.hold", "Mod"),
        "tool:mute"
    );
}

#[test]
fn key_019_a_hold_moves_to_a_free_modifier_and_a_taken_one_is_refused() {
    let d_tmp = dir("hold-move");
    let d = d_tmp.path().to_path_buf();
    let shared = Settings::open(&d).shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    let col = |shared: &Shared, m: &str| shared.lock().unwrap().mousemap.get("roll.select.hold", m);
    // The roll's defaults are Ctrl = draw and Ctrl+Shift = cut, so Ctrl+Shift is taken.
    assert_eq!(col(&shared, "Mod+Shift"), "tool:cut");
    let e = s
        .run_value(
            "mousemap.set",
            json!({"context": "roll.select.hold", "modifier": "Ctrl+Shift",
                   "action": "tool:draw", "move-from": "Ctrl"}),
            &h,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs);
    assert!(e.message.contains("使われています"), "{}", e.message);
    // Nothing changed, in memory or on file.
    assert_eq!(col(&shared, "Mod"), "tool:draw");
    assert_eq!(col(&shared, "Mod+Shift"), "tool:cut");
    assert!(!d.join("mousemap.toml").exists());
    // A free modifier takes it, and the old one is let go.
    s.run_value(
        "mousemap.set",
        json!({"context": "roll.select.hold", "modifier": "Alt",
               "action": "tool:draw", "move-from": "Ctrl"}),
        &h,
    )
    .unwrap();
    assert_eq!(col(&shared, "Alt"), "tool:draw");
    assert_eq!(col(&shared, "Mod"), "");
    let again = Settings::open(&d).shared();
    assert_eq!(
        again
            .lock()
            .unwrap()
            .mousemap
            .get("roll.select.hold", "Alt"),
        "tool:draw"
    );
}

#[test]
fn key_019_conflicts_lists_a_hold_that_hides_a_click_on_the_same_modifier() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    let none = s.run_value("mousemap.conflicts", json!({}), &h).unwrap();
    assert_eq!(none.data["conflicts"].as_array().unwrap().len(), 0);
    // Ctrl-click on a clip body toggles the selection, while Ctrl is also the hold to range.
    s.run_value(
        "mousemap.set",
        json!({"context": "arrange.select.clip-body.click", "modifier": "Ctrl",
               "action": "select.toggle"}),
        &h,
    )
    .unwrap();
    let out = s.run_value("mousemap.conflicts", json!({}), &h).unwrap();
    let list = out.data["conflicts"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["kind"], "warning");
    assert_eq!(list[0]["context"], "arrange.select.clip-body.click");
    assert_eq!(list[0]["modifier"], "Mod");
    assert_eq!(list[0]["hold"], "tool:range");
}

#[test]
fn the_record_panel_commands_flip_set_and_share_the_count_in() {
    let d = dir("record-panel");
    let shared = Settings::open(d.path()).shared();
    let mut s = session(&shared);
    let h = RunOptions::human();

    // No argument flips; the data says where it ended.
    let r = s.run_value("record.punch", json!({}), &h).unwrap();
    assert_eq!(r.data["on"], true);
    assert!(shared.lock().unwrap().prefs.bool("record.punch"));
    let r = s.run_value("record.punch", json!({}), &h).unwrap();
    assert_eq!(r.data["on"], false);

    // The post-roll needs the auto-punch.
    let e = s
        .run_value("record.postroll", json!({"on": true}), &h)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    s.run_value("record.punch", json!({"on": true}), &h)
        .unwrap();
    let r = s
        .run_value("record.postroll", json!({"on": true, "bars": 2}), &h)
        .unwrap();
    assert_eq!(r.data["bars"], 2);

    // Typing the bars alone keeps the switch where it was.
    let r = s
        .run_value("record.preroll", json!({"bars": 3}), &h)
        .unwrap();
    assert_eq!(
        (r.data["on"].clone(), r.data["bars"].clone()),
        (json!(false), json!(3))
    );
    s.run_value("record.preroll", json!({"on": true}), &h)
        .unwrap();

    // The pre-count is the same value as the setting `audio.count-in`.
    s.run_value("record.count-in", json!({"bars": 2}), &h)
        .unwrap();
    assert_eq!(shared.lock().unwrap().prefs.number("audio.count-in"), 2.0);
    let r = s.run_value("record.count-in", json!({}), &h).unwrap();
    assert_eq!(r.data["on"], false);
    assert_eq!(shared.lock().unwrap().prefs.number("audio.count-in"), 0.0);
    let r = s.run_value("record.count-in", json!({}), &h).unwrap();
    assert_eq!(r.data["on"], true);
    assert_eq!(shared.lock().unwrap().prefs.number("audio.count-in"), 1.0);

    let r = s
        .run_value(
            "record.input-quantize",
            json!({"on": true, "value": "1/8T"}),
            &h,
        )
        .unwrap();
    assert_eq!(r.data["value"], "1/8T");
    let e = s
        .run_value("record.input-quantize", json!({"value": "1/3"}), &h)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs);
    assert!(!s.can_undo());

    // Another program sees every one of them after a reopen.
    let b = Settings::open(d.path()).shared();
    let p = &b.lock().unwrap().prefs;
    assert!(p.bool("record.punch") && p.bool("record.preroll") && p.bool("record.postroll"));
    assert_eq!(p.number("record.preroll-bars"), 3.0);
    assert_eq!(p.number("record.postroll-bars"), 2.0);
    assert_eq!(p.number("audio.count-in"), 1.0);
    assert!(p.bool("edit.record.input-quantize"));
    assert_eq!(p.text("edit.record.input-quantize-value"), "1/8T");
}

#[test]
fn key_012_keep_none_starts_over_from_the_preset_and_keep_mine_is_the_default() {
    let d_tmp = dir("preset-keep");
    let d = d_tmp.path().to_path_buf();
    let shared = Settings::open(&d).shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    s.run_value(
        "keymap.set",
        json!({"key": "Mod+Alt+V", "command": "note.transpose --by 12", "context": "roll"}),
        &h,
    )
    .unwrap();
    let v = Chord::parse("Mod+Alt+V", Os::Win).unwrap();
    // Without `keep` the row stays.
    let r = s
        .run_value("keymap.preset", json!({"name": "logic"}), &h)
        .unwrap();
    assert_eq!(r.data["keep"], "mine");
    assert_eq!(r.data["kept"], 1);
    assert!(shared.lock().unwrap().keymap.lookup("roll", &v).is_some());
    // `none` drops it, and the preset's own keys are what is in effect.
    let r = s
        .run_value(
            "keymap.preset",
            json!({"name": "ableton", "keep": "none"}),
            &h,
        )
        .unwrap();
    assert_eq!(r.data["keep"], "none");
    assert_eq!(r.data["kept"], 0);
    {
        let g = shared.lock().unwrap();
        assert_eq!(g.keymap.preset, "ableton");
        assert_eq!(g.keymap.changed_rows(), 0);
        assert!(g.keymap.lookup("roll", &v).is_none());
        let f9 = Chord::parse("F9", Os::Win).unwrap();
        assert_eq!(g.keymap.lookup("global", &f9).unwrap().1, "record.start");
    }
    // A word that is neither is refused and nothing changes.
    let e = s
        .run_value(
            "keymap.preset",
            json!({"name": "cubase", "keep": "all"}),
            &h,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs);
    assert_eq!(shared.lock().unwrap().keymap.preset, "ableton");
    // It is kept on file.
    let again = Settings::open(&d).shared();
    assert_eq!(again.lock().unwrap().keymap.preset, "ableton");
}

#[test]
fn key_012_no_preset_takes_the_main_keys_of_the_arrange_and_adds_no_conflict() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    let key = |c: &str| Chord::parse(c, Os::Win).unwrap();
    let base = {
        let g = shared.lock().unwrap();
        g.keymap.conflicts(Os::Win).len()
    };
    for name in ["studio-pro", "cubase", "logic", "ableton", "reaper"] {
        s.run_value("keymap.preset", json!({"name": name, "keep": "none"}), &h)
            .unwrap();
        let g = shared.lock().unwrap();
        // Collapse the track list, insert time and delete time stay where they are.
        assert_eq!(
            g.keymap.lookup("arrange", &key("Mod+Shift+L")).unwrap().1,
            "view.tracklist.collapse",
            "{name}"
        );
        assert_eq!(
            g.keymap.lookup("arrange", &key("Mod+Alt+I")).unwrap().1,
            "range.time.insert",
            "{name}"
        );
        assert_eq!(
            g.keymap.lookup("arrange", &key("Mod+Alt+D")).unwrap().1,
            "range.time.delete",
            "{name}"
        );
        // And the preset does not leave more clashes than Studio Pro has.
        assert!(
            g.keymap.conflicts(Os::Win).len() <= base,
            "{name}: {:?}",
            g.keymap.conflicts(Os::Win)
        );
    }
}

#[test]
fn key_012_next_and_previous_channel_and_preset_have_keys_in_every_preset() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    let key = |c: &str| Chord::parse(c, Os::Win).unwrap();
    for name in ["studio-pro", "cubase", "logic", "ableton", "reaper"] {
        s.run_value("keymap.preset", json!({"name": name, "keep": "none"}), &h)
            .unwrap();
        let g = shared.lock().unwrap();
        let at = |ctx: &str, k: &str| g.keymap.lookup(ctx, &key(k)).map(|b| b.1);
        assert_eq!(
            at("mixer", "]").as_deref(),
            Some("mixer.channel.next"),
            "{name}"
        );
        assert_eq!(
            at("mixer", "[").as_deref(),
            Some("mixer.channel.prev"),
            "{name}"
        );
        assert_eq!(
            at("global", "Alt+PageDown").as_deref(),
            Some("plugin.preset.next"),
            "{name}"
        );
        assert_eq!(
            at("global", "Alt+PageUp").as_deref(),
            Some("plugin.preset.prev"),
            "{name}"
        );
        // The arrows stay the selection's.
        assert_eq!(
            at("arrange", "Right").as_deref(),
            Some("select.step --dir right"),
            "{name}"
        );
    }
    // A Studio Pro key scheme brings its own keys for the four.
    for (category, row, ctx, command) in [
        ("Console", "Next Channel", "mixer", "mixer.channel.next"),
        ("Console", "Previous Channel", "mixer", "mixer.channel.prev"),
        ("Presets", "Next", "global", "plugin.preset.next"),
        ("Presets", "Previous", "global", "plugin.preset.prev"),
    ] {
        assert_eq!(
            xirius_prefs::import::targets(category, row),
            vec![(ctx.to_string(), command.to_string())]
        );
    }
    // Toggle Size stays without a command.
    assert!(xirius_prefs::import::targets("Console", "Toggle Size").is_empty());
}

/// The keys KEY-012 adds for the rest of the Studio Pro table: (context, key, command).
const REST: &[(&str, &str, &str)] = &[
    ("global", "Mod+.", "project.settings"),
    ("arrange", "Mod+Shift+G", "view.snap"),
    ("global", "Mod+Shift+S", "track.solo.clear"),
    ("global", "Mod+Shift+M", "track.mute.clear"),
    ("arrange", "Mod+Shift+D", "select.off-tracks"),
    ("arrange", "Alt+Shift+Home", "select.span --way start"),
    ("arrange", "Alt+Shift+End", "select.span --way end"),
    ("arrange", "Alt+H", "take.next"),
    ("arrange", "Alt+G", "take.prev"),
    ("arrange", "PageUp", "view.page --way up"),
    ("roll", "PageDown", "view.page --way down"),
    ("arrange", "Tab", "view.focus --way next"),
    ("mixer", "Shift+Tab", "view.focus --way prev"),
    ("global", "Shift+F", "view.fullscreen"),
    ("global", "Mod+Q", "app.quit"),
    ("global", "Mod+W", "app.quit"),
    ("global", "Mod+F4", "app.quit"),
    ("arrange", "Shift+Insert", "marker.add.named"),
    ("arrange", "Shift+Y", "marker.add.named"),
    ("arrange", "Alt+R", "range.from-clips"),
    ("arrange", "Shift+R", "range.from-clips"),
    ("global", "Mod+F", "browser.search"),
    ("global", "F6", "browser.tab --tab instruments"),
    ("global", "F7", "browser.tab --tab effects"),
    ("global", "F9", "browser.tab --tab files"),
    ("mixer", "}", "mixer.bank --way next"),
    ("mixer", "{", "mixer.bank --way prev"),
    ("global", "#", "transport.panic"),
    ("global", "CapsLock", "view.pc-keyboard"),
    ("global", "Mod+T", "transport.locate"),
    ("global", "Mod+Alt+T", "track.reveal"),
    ("global", "Mod+Alt+C", "track.reveal --in mixer"),
    ("global", "F11", "plugin.editor.open"),
    ("global", "Mod+NumPad.", "transport.goto"),
    ("arrange", "Mod+P", "clip.add.midi"),
];

#[test]
fn key_012_the_rest_of_the_studio_pro_table_has_its_keys_in_every_preset() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    let key = |c: &str| Chord::parse(c, Os::Win).unwrap();
    for name in ["studio-pro", "cubase", "logic", "ableton", "reaper"] {
        s.run_value("keymap.preset", json!({"name": name, "keep": "none"}), &h)
            .unwrap();
        let g = shared.lock().unwrap();
        for (ctx, k, command) in REST {
            // Where the preset's own list gives the key to something else.
            let want = match (name, *k) {
                ("logic", "Mod+T") => "clip.split",
                ("ableton", "F9") => "record.start",
                _ => command,
            };
            let ctx = if want == "clip.split" { "arrange" } else { ctx };
            assert_eq!(
                g.keymap.lookup(ctx, &key(k)).map(|b| b.1).as_deref(),
                Some(want),
                "{name} {ctx} {k}"
            );
        }
    }
}

#[test]
fn key_012_every_studio_pro_row_has_a_command_of_this_app() {
    let shared = Settings::memory().shared();
    let g = shared.lock().unwrap();
    let rows = g.keymap.rows();
    let mut taken = 0;
    for (name, ctx, cmd) in xirius_prefs::import::STUDIO_PRO {
        assert!(
            rows.iter().any(|r| r.context == *ctx && r.command == *cmd),
            "{name}: {ctx} / {cmd} is not a row of the studio-pro preset"
        );
        taken += 1;
    }
    assert_eq!(taken, 211);
    // Only one name is written by two commands (`SHARED`), so a name alone is one answer.
    assert_eq!(
        xirius_prefs::import::targets("Transport", "Start"),
        [("global".to_string(), "transport.start".to_string())]
    );
}

/// Two clips back to back with a gap of 480 on one track; gives the session, the track and
/// the clips.
fn edt_135_song(s: &mut Session) -> (String, Vec<String>) {
    let h = RunOptions::human();
    let id = |v: serde_json::Value| v["id"].as_str().unwrap().to_string();
    let t = id(s
        .run_value("track.add.instrument", json!({}), &h)
        .unwrap()
        .data);
    let clips = [0, 960]
        .iter()
        .map(|at| {
            id(s.run_value(
                "clip.add.midi",
                json!({"track": t, "at": at, "length": 480}),
                &h,
            )
            .unwrap()
            .data)
        })
        .collect();
    (t, clips)
}

fn edt_135_starts(s: &Session, track: &str) -> Vec<i64> {
    let id: xirius_core::id::TrackId = track.parse().unwrap();
    let mut v: Vec<_> = s.project().tracks[&id]
        .clips
        .iter()
        .map(|c| c.start)
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn edt_135_the_ripple_switch_is_the_setting_and_the_clip_edits_follow_it() {
    let d = dir("ripple");
    let shared = Settings::open(d.path()).shared();
    let mut s = session(&shared);
    let h = RunOptions::human();

    // Off by default: a delete leaves the gap.
    let (t, c) = edt_135_song(&mut s);
    s.run_value("clip.delete", json!({"ids": [c[0]]}), &h)
        .unwrap();
    assert_eq!(edt_135_starts(&s, &t), [960]);

    // No argument flips the setting, which is saved and is not a step of the song.
    let steps = s.steps().len();
    let r = s.run_value("edit.ripple", json!({}), &h).unwrap();
    assert_eq!(r.data["on"], true);
    assert!(shared.lock().unwrap().prefs.bool("edit.ripple"));
    assert_eq!(s.steps().len(), steps);
    assert!(Settings::open(d.path()).prefs.bool("edit.ripple"));

    // On: the same delete closes up, and so does a shorter clip.
    let (t, c) = edt_135_song(&mut s);
    s.run_value("clip.delete", json!({"ids": [c[0]]}), &h)
        .unwrap();
    assert_eq!(edt_135_starts(&s, &t), [480]);
    let (t, c) = edt_135_song(&mut s);
    s.run_value("clip.resize", json!({"ids": [c[0]], "by": -240}), &h)
        .unwrap();
    assert_eq!(edt_135_starts(&s, &t), [0, 720]);

    // What the caller says wins over the setting.
    let (t, c) = edt_135_song(&mut s);
    s.run_value("clip.delete", json!({"ids": [c[0]], "ripple": false}), &h)
        .unwrap();
    assert_eq!(edt_135_starts(&s, &t), [960]);

    // The automation follows the setting `edit.automation.follows-clips` when left out.
    s.run_value(
        "prefs.set",
        json!({"key": "edit.automation.follows-clips", "value": "false"}),
        &h,
    )
    .unwrap();
    let (t, c) = edt_135_song(&mut s);
    s.run_value(
        "auto.point.add",
        json!({"track": t, "target": "pan", "at": 1000, "value": 0.5}),
        &h,
    )
    .unwrap();
    s.run_value("clip.delete", json!({"ids": [c[0]]}), &h)
        .unwrap();
    let id: xirius_core::id::TrackId = t.parse().unwrap();
    let ticks: Vec<i64> = s.project().tracks[&id].automation[0]
        .points
        .iter()
        .map(|x| x.tick)
        .collect();
    assert_eq!(ticks, [1000]);

    // `--off` turns it off again; another program sees it.
    let r = s
        .run_value("edit.ripple", json!({"off": true}), &h)
        .unwrap();
    assert_eq!(r.data["on"], false);
    let (t, c) = edt_135_song(&mut s);
    s.run_value("clip.delete", json!({"ids": [c[0]]}), &h)
        .unwrap();
    assert_eq!(edt_135_starts(&s, &t), [960]);
    assert!(!Settings::open(d.path()).prefs.bool("edit.ripple"));
}

#[test]
fn edt_137_138_the_overlap_settings_fill_what_the_note_and_clip_edits_leave_out() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    let id = |v: &serde_json::Value| v["id"].as_str().unwrap().to_string();
    let t = id(&s
        .run_value("track.add.instrument", json!({}), &h)
        .unwrap()
        .data);
    let clip = |s: &mut Session, at: i64| {
        id(&s
            .run_value(
                "clip.add.midi",
                json!({"track": t, "at": at, "length": 960}),
                &h,
            )
            .unwrap()
            .data)
    };
    let spans = |s: &Session| {
        let tid: xirius_core::id::TrackId = t.parse().unwrap();
        let mut v: Vec<(i64, i64)> = s.project().tracks[&tid]
            .clips
            .iter()
            .map(|c| (c.start, c.length))
            .collect();
        v.sort_unstable();
        v
    };
    let notes = |s: &Session, c: &str| {
        let cid: xirius_core::id::ClipId = c.parse().unwrap();
        let mut v: Vec<(i64, i64)> = s
            .project()
            .clip(cid)
            .unwrap()
            .1
            .notes()
            .iter()
            .map(|n| (n.start, n.length))
            .collect();
        v.sort_unstable();
        v
    };
    let set = |s: &mut Session, key: &str, value: &str| {
        s.run_value("prefs.set", json!({"key": key, "value": value}), &h)
            .unwrap();
    };
    let lay = |s: &mut Session, c: &str| {
        s.run_value(
            "note.put",
            json!({"clip": c, "notes": [
                {"at": 0, "pitch": 60, "length": 480},
                {"at": 240, "pitch": 60, "length": 480},
            ]}),
            &h,
        )
        .unwrap();
    };

    // Off by default: notes and clips overlap.
    let a = clip(&mut s, 0);
    lay(&mut s, &a);
    assert_eq!(notes(&s, &a), [(0, 480), (240, 480)]);
    let b = clip(&mut s, 1920);
    s.run_value("clip.move", json!({"ids": [b], "at": 480}), &h)
        .unwrap();
    assert_eq!(spans(&s), [(0, 960), (480, 960)]);

    // On: the same edits trim, without anything said.
    set(&mut s, "edit.note.overlap", "trim");
    set(&mut s, "edit.clip.no-overlap", "true");
    let c = clip(&mut s, 7680);
    lay(&mut s, &c);
    assert_eq!(notes(&s, &c), [(0, 240), (240, 480)]);
    s.run_value("clip.move", json!({"ids": [b], "at": 7680 - 480}), &h)
        .unwrap();
    assert_eq!(spans(&s), [(0, 960), (7200, 960), (8160, 480)]);

    // What the caller says wins over the setting.
    let d = clip(&mut s, 15360);
    s.run_value(
        "note.put",
        json!({"clip": d, "overlap": "allow", "notes": [
            {"at": 0, "pitch": 60, "length": 480},
            {"at": 240, "pitch": 60, "length": 480},
        ]}),
        &h,
    )
    .unwrap();
    assert_eq!(notes(&s, &d), [(0, 480), (240, 480)]);
    s.run_value(
        "clip.move",
        json!({"ids": [d], "at": 7680 + 240, "no-overlap": false}),
        &h,
    )
    .unwrap();
    assert_eq!(spans(&s), [(0, 960), (7200, 960), (7920, 960), (8160, 480)]);
}

/// EDT-139: with the ripple and both overlap settings on, a recording lays its takes over what
/// is there without cutting anything, and the clip edits left to the settings ripple first
/// and cut neither the takes of one stack nor the clips they pushed along, in one step.
#[test]
fn edt_139_recording_is_outside_the_overlap_settings_and_ripple_goes_with_them() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    for key in ["edit.ripple", "edit.clip.no-overlap"] {
        s.run_value("prefs.set", json!({"key": key, "value": "true"}), &h)
            .unwrap();
    }
    s.run_value(
        "prefs.set",
        json!({"key": "edit.note.overlap", "value": "trim"}),
        &h,
    )
    .unwrap();
    let t = s
        .run_value("track.add.instrument", json!({}), &h)
        .unwrap()
        .data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let spans = |s: &Session| {
        let tid: xirius_core::id::TrackId = t.parse().unwrap();
        let mut v: Vec<(i64, i64)> = s.project().tracks[&tid]
            .clips
            .iter()
            .map(|c| (c.start, c.length))
            .collect();
        v.sort_unstable();
        v
    };
    let clip = |s: &mut Session, at: i64, length: i64| {
        s.run_value(
            "clip.add.midi",
            json!({"track": t, "at": at, "length": length}),
            &h,
        )
        .unwrap()
        .data["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // A clip the takes are recorded over (it becomes a row of their stack) and one after it.
    clip(&mut s, 0, 3840);
    clip(&mut s, 3840, 960);

    // Two takes recorded over the first clip: everything stays, the notes as they came.
    let take = json!({"track": t, "start": 0, "length": 3840, "notes": [
        {"start": 0, "length": 480, "pitch": 60},
        {"start": 240, "length": 480, "pitch": 60},
    ]});
    let r = s
        .run_value("record.take.add", json!({"takes": [take, take]}), &h)
        .unwrap();
    assert_eq!(spans(&s), [(0, 3840), (0, 3840), (0, 3840), (3840, 960)]);
    let takes: Vec<String> = r.data["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    for id in &takes {
        let cid: xirius_core::id::ClipId = id.parse().unwrap();
        let mut notes: Vec<(i64, i64)> = s
            .project()
            .clip(cid)
            .unwrap()
            .1
            .notes()
            .iter()
            .map(|n| (n.start, n.length))
            .collect();
        notes.sort_unstable();
        assert_eq!(notes, [(0, 480), (240, 480)]);
    }

    // A take made longer, nothing said: the clip after it is pushed, no take is cut.
    let steps = s.steps().len();
    s.run_value("clip.resize", json!({"ids": [takes[1]], "by": 960}), &h)
        .unwrap();
    assert_eq!(spans(&s), [(0, 3840), (0, 3840), (0, 4800), (4800, 960)]);
    assert_eq!(s.steps().len(), steps + 1);
    s.undo(1).unwrap();
    assert_eq!(spans(&s), [(0, 3840), (0, 3840), (0, 3840), (3840, 960)]);
}

#[test]
fn edt_135_the_ripple_switch_has_its_key_in_every_preset() {
    let shared = Settings::memory().shared();
    let mut s = session(&shared);
    let h = RunOptions::human();
    let key = |c: &str| Chord::parse(c, Os::Win).unwrap();
    for (name, k) in [
        ("studio-pro", "Ctrl+Alt+R"),
        ("cubase", "Ctrl+Alt+R"),
        ("logic", "Ctrl+Alt+R"),
        ("ableton", "Ctrl+Alt+R"),
        ("reaper", "Alt+P"),
    ] {
        s.run_value("keymap.preset", json!({"name": name, "keep": "none"}), &h)
            .unwrap();
        let g = shared.lock().unwrap();
        assert_eq!(
            g.keymap.lookup("arrange", &key(k)).map(|b| b.1).as_deref(),
            Some("edit.ripple"),
            "{name} {k}"
        );
    }
}
