//! Input settings use isolated folders and never open a device or the user's settings.
use serde_json::json;
use xirius_core::command::Registry;
use xirius_core::session::{RunOptions, Session};
use xirius_prefs::keymap::{Issue, Others};
use xirius_prefs::{Chord, File, Keymap, Os, Prefs, Settings};

#[test]
fn ops_012_supported_defaults_match_the_source_and_unsupported_keys_stay_unbound() {
    let map = Keymap::default();
    for (context, key, command) in [
        ("global", "Enter", "transport.start"),
        ("global", "Space", "transport.play"),
        ("global", "F2", "clip.open"),
        ("global", "Mod+E", "export.open"),
        ("edit", "1", "tool.select"),
        ("edit", "5", "tool.draw"),
        ("arrange", "G", "clip.join"),
        ("roll", "G", "note.join"),
        ("roll", "Shift+Q", "note.quantize --restore"),
    ] {
        assert_eq!(
            map.lookup(context, &Chord::parse(key, Os::Win).unwrap())
                .unwrap()
                .1,
            command
        );
    }
}

#[test]
fn key_009_duplicate_rows_remain_conflicts_and_invalid_arguments_are_skipped() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-duplicate-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("duplicate");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("keymap.toml"), "preset='studio-pro'\npreset='bad'\n[roll]\nF8='note.mute'\nF8='note.delete'\nF9='note.transpose --unknown 1'\n").unwrap();
    let s = Settings::open(&dir);
    assert!(
        s.keymap
            .conflicts(Os::Win)
            .iter()
            .any(|i| matches!(i, Issue::Conflict{chord,..} if chord.key=="F8"))
    );
    assert!(
        s.keymap
            .lookup("roll", &Chord::parse("F9", Os::Win).unwrap())
            .is_none()
    );
    assert!(s.problems.len() >= 3);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn key_011_shadow_on_an_unchanged_preset_binding_survives_saving_and_reset() {
    let mut map = Keymap::default();
    let q = Chord::parse("Q", Os::Win).unwrap();
    map.set("global", &q, "tool.draw", None, Others::Keep);
    map.shadow.insert(("roll".into(), q.clone()));
    let restored = Keymap::from_toml(&map.to_toml().parse().unwrap(), &mut Vec::new());
    assert!(restored.shadow.contains(&("roll".into(), q.clone())));
    assert_eq!(map.bindings(), restored.bindings());
    map.reset(Some("roll"), None, Some("note.quantize"));
    assert!(!map.shadow.contains(&("roll".into(), q.clone())));
    map.shadow.insert(("global".into(), q));
    map.reset(None, None, None);
    assert_eq!(map, Keymap::default());
}

#[test]
fn key_025_display_by_command_name_and_arguments_tracks_changes_and_conflicts() {
    let mut map = Keymap::default();
    assert!(
        map.display_for("roll", "note.transpose", Os::Win)
            .contains("Alt+↑")
    );
    let key = Chord::parse("F8", Os::Win).unwrap();
    map.set("roll", &key, "note.transpose --by 7", None, Others::Keep);
    assert_eq!(
        map.display_for("roll", "note.transpose --by 7", Os::Win),
        "F8"
    );
    map.set("roll", &key, "note.mute", None, Others::Keep);
    assert_eq!(
        map.display_for("roll", "note.transpose --by 7", Os::Win),
        ""
    );
}

#[test]
fn key_009_recovers_good_lines_reloads_and_saves_only_differences_with_backup() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-keymap-recovery-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("keymap-recovery");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("keymap.toml");
    let text =
        "preset = 'studio-pro'\n[roll]\n'Alt+F8' = 'note.delete'\nBAD LINE\n'F9' = 'tool.erase'\n";
    std::fs::write(&path, text).unwrap();
    let mut settings = Settings::open(&dir);
    assert!(settings.problems.iter().any(|p| p.contains(":4:")));
    assert_eq!(
        settings
            .keymap
            .lookup("roll", &Chord::parse("Alt+F8", Os::Win).unwrap())
            .unwrap()
            .1,
        "note.delete"
    );
    settings.save(File::Keymap).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("keymap.toml.bak")).unwrap(),
        text
    );
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(!saved.contains("history.undo") && !saved.contains("note.quantize"));
    std::fs::write(&path, "[roll]\n'Alt+F10'='note.mute'\n").unwrap();
    assert!(settings.reload_if_changed());
    assert_eq!(
        settings
            .keymap
            .lookup("roll", &Chord::parse("Alt+F10", Os::Win).unwrap())
            .unwrap()
            .1,
        "note.mute"
    );
    assert!(!settings.reload_if_changed());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn key_010_normalizes_jis_characters_keypad_and_physical_names() {
    assert_eq!(
        Chord::parse("Shift+Ctrl+Alt+q", Os::Win)
            .unwrap()
            .to_string(),
        "Mod+Alt+Shift+Q"
    );
    assert_eq!(Chord::parse("Ctrl++", Os::Win).unwrap().key, "+");
    assert_eq!(
        Chord::parse("Ctrl+NumPad+", Os::Win).unwrap().key,
        "NumPad+"
    );
    assert_ne!(
        Chord::parse("Ctrl+NumPad+", Os::Win).unwrap(),
        Chord::parse("Ctrl++", Os::Win).unwrap()
    );
    assert_eq!(
        Chord::parse("Mod+code:Slash", Os::Win).unwrap().key,
        "code:Slash"
    );
    for character in ["+", "@", "^", ":", "_"] {
        assert!(Chord::parse(character, Os::Win).is_ok());
    }
}

#[test]
fn key_008_011_conflicts_stop_dispatch_and_shadow_is_explicit() {
    let mut map = Keymap::default();
    let key = Chord::parse("F8", Os::Win).unwrap();
    map.set("global", &key, "history.undo", None, Others::Keep);
    map.set("edit", &key, "tool.draw", None, Others::Keep);
    assert_eq!(map.lookup("roll", &key).unwrap().1, "tool.draw");
    map.set("roll", &key, "note.delete", None, Others::Keep);
    assert_eq!(map.lookup("roll", &key).unwrap().1, "note.delete");
    assert!(
        map.conflicts(Os::Win)
            .iter()
            .any(|i| matches!(i,Issue::Shadow{context,..} if context=="roll"))
    );
    map.shadow.insert(("roll".into(), key.clone()));
    assert!(
        !map.conflicts(Os::Win)
            .iter()
            .any(|i| matches!(i,Issue::Shadow{context,..} if context=="roll"))
    );
    let restored = Keymap::from_toml(&map.to_toml().parse().unwrap(), &mut Vec::new());
    assert!(restored.shadow.contains(&("roll".into(), key.clone())));
    map.set("roll", &key, "note.mute", None, Others::Keep);
    assert!(map.lookup("roll", &key).is_none());
    assert!(
        map.conflicts(Os::Win)
            .iter()
            .any(|i| matches!(i, Issue::Conflict { .. }))
    );
    map.set(
        "global",
        &Chord::parse("Alt+F4", Os::Win).unwrap(),
        "history.undo",
        None,
        Others::Keep,
    );
    map.set(
        "text",
        &Chord::parse("Q", Os::Win).unwrap(),
        "note.delete",
        None,
        Others::Keep,
    );
    assert!(
        map.conflicts(Os::Win)
            .iter()
            .any(|i| matches!(i, Issue::Reserved { .. }))
    );
    assert!(
        map.conflicts(Os::Win)
            .iter()
            .any(|i| matches!(i, Issue::Text { .. }))
    );
    assert!(
        map.lookup("text", &Chord::parse("Q", Os::Win).unwrap())
            .is_none()
    );
}

#[test]
fn key_001_013_the_import_table_names_only_commands_this_app_has() {
    // Every line of the import table points at a row of this app's own key table. The manual's
    // keys and sections are not kept there (LIC-034), so the keys are this app's.
    for (name, ctx, cmd) in xirius_prefs::import::STUDIO_PRO {
        assert!(
            xirius_prefs::catalog::entry(ctx, cmd).is_some(),
            "{name}: {ctx} / {cmd} is not in the built-in table"
        );
    }
    // Studio Pro's Start and Toggle Start take the keys of this app's transport.
    let map = Keymap::default();
    for (key, command) in [
        ("Enter", "transport.start"),
        ("Space", "transport.play"),
        ("MediaPlayPause", "transport.play"),
    ] {
        assert_eq!(
            map.lookup("arrange", &Chord::parse(key, Os::Win).unwrap())
                .unwrap()
                .1,
            command
        );
    }
}

#[test]
fn key_020_edit_preferences_defaults_and_changed_values_round_trip() {
    let defaults = Prefs::default();
    for (key, value, changed) in [
        ("arrow.mode", "basic", "extended"),
        ("arrange.link", "false", "true"),
        ("click-empty.locate", "true", "false"),
        ("cursor.crosshair", "false", "true"),
        ("draw.length", "inspector", "last"),
        ("draw.velocity", "inspector", "last"),
        ("audition", "true", "false"),
        ("note.overlap", "allow", "trim"),
        ("quantize.auto", "false", "true"),
        ("record.input-quantize", "false", "true"),
        ("autoscroll", "turn-over", "left"),
        ("cursor.follows-edit", "false", "true"),
        ("automation.follows-clips", "true", "false"),
        ("automation.locks-clips", "true", "false"),
        ("clip.no-overlap", "false", "true"),
        ("ripple", "false", "true"),
        ("drag.value-tip", "true", "false"),
        ("wheel.ruler", "zoom", "scroll"),
        ("snap.arrange.mode", "adaptive", "frames"),
        ("snap.roll.mode", "adaptive", "bar"),
    ] {
        let key = format!("edit.{key}");
        assert_eq!(
            defaults.get(&key).unwrap(),
            xirius_prefs::prefs::schema(&key)
                .unwrap()
                .read(&json!(value))
                .unwrap()
        );
        let mut preferences = defaults.clone();
        preferences.set(&key, &json!(changed)).unwrap();
        let restored = Prefs::from_toml(&preferences.to_toml().parse().unwrap(), &mut Vec::new());
        assert_eq!(restored.get(&key), preferences.get(&key));
    }
    for area in ["arrange", "roll"] {
        for (name, value) in [("grid", true), ("events", true), ("relative", false)] {
            assert_eq!(
                defaults.get(&format!("edit.snap.{area}.{name}")),
                Some(json!(value))
            );
        }
    }
}

#[test]
fn key_022_023_changes_return_effective_bindings_without_entering_song_history() {
    let mut session = Session::new("settings");
    xirius_prefs::attach(&mut session, Settings::memory().shared());
    for (command, args) in [
        (
            "keymap.set",
            json!({"key":"Alt+F8","command":"note.transpose --by 12","context":"roll"}),
        ),
        ("keymap.unset", json!({"key":"Alt+F8","context":"roll"})),
        ("keymap.preset", json!({"name":"studio-pro"})),
        ("keymap.reset", json!({})),
        (
            "mousemap.set",
            json!({"context":"roll.select.note-body.drag","modifier":"Alt","action":"note.move"}),
        ),
        ("mousemap.reset", json!({})),
    ] {
        let result = session
            .run_value(command, args, &RunOptions::human())
            .unwrap();
        assert!(
            result.data.get("conflicts").is_some(),
            "{command}: {:?}",
            result.data
        );
        assert!(
            result
                .data
                .get(if command.starts_with("keymap") {
                    "rows"
                } else {
                    "contexts"
                })
                .is_some()
        );
        assert!(!session.can_undo());
    }
    let registry = Registry::standard();
    for name in xirius_prefs::ext::NAMES {
        assert!(registry.get(name).is_some());
    }
}

#[test]
fn ui_184_the_crosshair_is_off_unless_a_saved_file_says_so() {
    // A file from before the setting changed its default, with nothing about it: off.
    let mut problems = Vec::new();
    let old = "[edit]\narrow.mode = \"extended\"\n".parse().unwrap();
    let p = Prefs::from_toml(&old, &mut problems);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(p.get("edit.cursor.crosshair"), Some(json!(false)));
    assert_eq!(
        p.get("edit.arrow.mode"),
        Some(json!("extended")),
        "the rest is kept"
    );
    // A file that saved it on keeps it on, through saving and opening again.
    let saved = "[edit]\ncursor.crosshair = true\n".parse().unwrap();
    let p = Prefs::from_toml(&saved, &mut problems);
    assert_eq!(p.get("edit.cursor.crosshair"), Some(json!(true)));
    let again = Prefs::from_toml(&p.to_toml().parse().unwrap(), &mut problems);
    assert_eq!(again.get("edit.cursor.crosshair"), Some(json!(true)));
    assert!(problems.is_empty(), "{problems:?}");
}
