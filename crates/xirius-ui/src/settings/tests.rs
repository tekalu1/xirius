//! The settings screen, without a window.

use std::path::PathBuf;

use serde_json::json;
use xirius_prefs::{Chord, Os, Settings};

use super::*;
use crate::studio::Hit;

/// A folder of its own for each test, empty; removed when dropped (also when the test fails).
fn temp(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-ui-settings-{name}-"))
        .tempdir()
        .unwrap()
}

/// A window's studio that keeps its settings in `dir`, as a launch does.
fn launch(dir: &std::path::Path) -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.song.load_settings(dir);
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    (s, g)
}

fn none() -> Mods {
    Mods::default()
}

fn ctrl() -> Mods {
    Mods {
        ctrl: true,
        ..Mods::default()
    }
}

/// Presses the middle of the part that does `act`, as the pointer would,
/// the body scrolled first to show it.
fn press(s: &mut Studio, g: &Geometry, act: &Do) {
    for _ in 0..100 {
        let scene = s.settings_scene(g);
        let mid = |i: &scene::Item| (i.r.x + i.r.w / 2.0, i.r.y + i.r.h / 2.0);
        // The veil closes too, but its middle is under the modal: the button is aimed at.
        let open = scene
            .items
            .iter()
            .filter(|i| i.act == *act)
            .map(mid)
            .find(|&(x, y)| scene.item_at(x, y).is_some_and(|i| i.act == *act));
        if let Some((x, y)) = open {
            assert!(
                s.settings_press(g, x, y),
                "the press at {x},{y} did nothing"
            );
            return;
        }
        let item = scene
            .find(act)
            .unwrap_or_else(|| panic!("nothing on the screen does {act:?}"));
        let (_, y) = mid(item);
        let body = scene.body;
        let dy = if y > body.bottom() { -60.0 } else { 60.0 };
        s.settings_wheel(g, body.x + body.w / 2.0, body.y + body.h / 2.0, dy);
    }
    panic!("{act:?} is not to be pressed");
}

fn pref_bool(s: &Studio, key: &str) -> bool {
    s.with_settings(|p| p.prefs.bool(key))
}

/// The switch of `key` on 編集, which turns it to `!on`.
fn switch(key: &str, on: bool) -> Do {
    Do::Pref(key.into(), (!on).to_string())
}

fn q() -> Chord {
    Chord::parse("Q", Os::Win).unwrap()
}

#[test]
fn the_gear_ctrl_comma_and_the_palette_open_general() {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);

    // The gear at the bottom of the rail.
    let (x, y) = s.center_of(&g, Hit::RailSettings).expect("the gear");
    let hit = s.hit(&g, x, y);
    assert_eq!(hit, Hit::RailSettings);
    assert!(s.press(&g, hit));
    assert_eq!(
        s.settings.as_ref().map(|u| u.section),
        Some(Section::General)
    );

    // Ctrl+, closes them, and opens them again on 一般.
    s.settings.as_mut().unwrap().go(Section::Midi);
    assert!(s.settings_key(",", ctrl()));
    assert!(s.settings.is_none());
    assert!(s.settings_key(",", ctrl()));
    assert_eq!(
        s.settings.as_ref().map(|u| u.section),
        Some(Section::General)
    );
    assert!(s.settings_key("escape", none()));
    assert!(s.settings.is_none());

    // The palette's row 設定を表示.
    let row = s
        .palette_rows(&g)
        .into_iter()
        .find(|r| r.name == crate::palette::SETTINGS.0)
        .expect("the settings' row");
    assert_eq!(row.key.as_deref(), Some("Ctrl+,"));
    assert!(s.run_row(&g, &row));
    assert_eq!(
        s.settings.as_ref().map(|u| u.section),
        Some(Section::General)
    );
    assert!(s.palette.is_none());
}

#[test]
fn every_section_draws_and_the_nav_goes_there() {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.open_settings(Section::General);
    for section in Section::ALL {
        press(&mut s, &g, &Do::Nav(section));
        assert_eq!(s.settings.as_ref().unwrap().section, section);
        let scene = s.settings_scene(&g);
        assert!(scene.has_text(section.label()), "{section:?}");
        assert!(!scene.items.is_empty(), "{section:?}");
    }
    press(&mut s, &g, &Do::Sub(Sub::Mouse));
    assert!(s.settings_scene(&g).has_text("一時的なツールの切り替え"));
    press(&mut s, &g, &Do::Close);
    assert!(s.settings.is_none());
}

#[test]
fn empty_space_inside_settings_stays_open_and_only_the_outside_closes_it() {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    for section in Section::ALL {
        s.open_settings(section);
        let scene = s.settings_scene(&g);
        let f = scene.frame;
        // Padding in the header, body and navigation, including the unused bottom of each.
        for (x, y) in [
            (f.x + NAV_W + 2.0, f.y + 2.0),
            (scene.body.x + 2.0, scene.body.y + 2.0),
            (scene.body.right() - 2.0, scene.body.bottom() - 2.0),
            (f.x + 2.0, f.bottom() - 2.0),
        ] {
            assert_eq!(scene.item_at(x, y).map(|i| &i.act), Some(&Do::Nothing));
            assert!(s.settings_press(&g, x, y));
            assert!(s.settings.is_some(), "{section:?}: {x},{y}");
        }
        assert!(s.settings_press(&g, f.x - 2.0, f.y + f.h / 2.0));
        assert!(s.settings.is_none());
    }
    s.open_settings(Section::General);
    assert!(s.settings_key("escape", none()));
    assert!(s.settings.is_none());
    s.open_settings(Section::General);
    press(&mut s, &g, &Do::Close);
    assert!(s.settings.is_none());
}

#[test]
fn a_switch_is_there_the_next_launch() {
    let dir_tmp = temp("switch");
    let dir = dir_tmp.path().to_path_buf();
    let follows = "edit.automation.follows-clips";
    let locks = "edit.automation.locks-clips";
    let (mut s, g) = launch(&dir);
    let (f0, l0) = (pref_bool(&s, follows), pref_bool(&s, locks));
    s.open_settings(Section::Edit);
    press(&mut s, &g, &switch(follows, f0));
    press(&mut s, &g, &switch(locks, l0));
    assert_eq!(pref_bool(&s, follows), !f0);
    assert_eq!(pref_bool(&s, locks), !l0);
    // Not a step of the song: the history has nothing to undo.
    assert!(!s.song.can_undo());
    assert!(dir.join("prefs.toml").exists());
    drop(s);

    let (mut s, g) = launch(&dir);
    assert_eq!(pref_bool(&s, follows), !f0);
    assert_eq!(pref_bool(&s, locks), !l0);
    s.open_settings(Section::Edit);
    // The switches show it: each now turns back.
    let scene = s.settings_scene(&g);
    assert!(scene.find(&switch(follows, !f0)).is_some());
    assert!(scene.find(&switch(locks, !l0)).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_theme_is_switched_on_the_screen_and_is_there_the_next_launch() {
    let dir_tmp = temp("theme");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.open_settings(Section::General);
    let dark = xirius_design::Theme::Dark;
    assert_eq!(s.wanted_theme(dark), dark);
    press(&mut s, &g, &Do::Pref("ui.theme".into(), "light".into()));
    assert_eq!(s.wanted_theme(dark), xirius_design::Theme::Light);
    // The screen itself is drawn in the colours the preference asks for.
    assert!(
        s.settings_scene(&g)
            .find(&Do::Pref("ui.theme".into(), "dark".into()))
            .is_some()
    );
    assert!(!s.song.can_undo());
    drop(s);

    let (s, _) = launch(&dir);
    assert_eq!(s.wanted_theme(dark), xirius_design::Theme::Light);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_key_is_assigned_and_a_conflict_is_said_and_settled() {
    let dir_tmp = temp("keys");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.open_settings(Section::Keys);
    let delete_keys = s.with_settings(|p| p.keymap.keys_of("roll", "note.delete"));
    let from = delete_keys.first().cloned();
    s.settings_do(
        &g,
        Do::Capture {
            ctx: "roll".into(),
            cmd: "note.delete".into(),
            from: from.clone(),
        },
        Rect::default(),
    );
    assert!(s.settings_scene(&g).has_text("キーを入力"));
    // A modifier alone waits on; Q is taken (クオンタイズ has it too).
    assert!(s.settings_key("shift", none()));
    assert!(s.settings.as_ref().unwrap().capture.is_some());
    assert!(s.settings_key("q", none()));
    assert!(s.settings.as_ref().unwrap().capture.is_none());
    assert!(s.with_settings(|p| p.keymap.keys_of("roll", "note.delete").contains(&q())));
    assert!(s.with_settings(|p| p.keymap.keys_of("roll", "note.quantize").contains(&q())));

    // The search by the key shows both, each saying the other.
    assert!(s.settings_key("q", none()));
    let scene = s.settings_scene(&g);
    let quantize = xirius_prefs::ext::command_label("note.quantize", s.song.registry());
    assert!(
        scene
            .texts
            .iter()
            .any(|t| t == &format!("「{quantize}」と重複")),
        "{:?}",
        scene.texts
    );
    // Swapping: クオンタイズ takes the key 削除 had.
    let swap = Do::Swap {
        ctx: "roll".into(),
        cmd: "note.delete".into(),
        key: q(),
    };
    press(&mut s, &g, &swap);
    let (quantize_keys, issues) = s.with_settings(|p| {
        (
            p.keymap.keys_of("roll", "note.quantize"),
            p.keymap.conflicts(Os::Win),
        )
    });
    assert!(!quantize_keys.contains(&q()), "{quantize_keys:?}");
    if let Some(f) = &from {
        assert!(quantize_keys.contains(f), "{quantize_keys:?}");
    }
    assert!(
        !issues
            .iter()
            .any(|i| matches!(i, xirius_prefs::keymap::Issue::Conflict { .. })),
        "{issues:?}"
    );

    // Restored, then on Q again, and 「解除」 lets クオンタイズ go of it.
    let restore = Do::Restore {
        ctx: "roll".into(),
        cmd: "note.delete".into(),
    };
    s.settings_do(&g, restore, Rect::default());
    s.song
        .set(
            "keymap.reset",
            json!({"context": "roll", "command": "note.quantize"}),
        )
        .unwrap();
    s.song
        .set(
            "keymap.set",
            json!({"key": "Q", "command": "note.delete", "context": "roll"}),
        )
        .unwrap();
    let unlink = Do::Unlink {
        ctx: "roll".into(),
        cmd: "note.delete".into(),
        key: q(),
    };
    press(&mut s, &g, &unlink);
    assert!(!s.with_settings(|p| p.keymap.keys_of("roll", "note.quantize").contains(&q())));
    assert!(s.with_settings(|p| p.keymap.keys_of("roll", "note.delete").contains(&q())));
    // Kept for the next launch.
    drop(s);
    let s = Settings::open(&dir);
    assert!(s.keymap.keys_of("roll", "note.delete").contains(&q()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn key_012_reset_all_goes_back_to_the_preset_in_use_and_undo_brings_the_rows_back() {
    let dir_tmp = temp("preset-reset");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.song
        .set("keymap.preset", json!({"name": "ableton"}))
        .unwrap();
    s.song
        .set(
            "keymap.set",
            json!({"key": "Alt+Shift+K", "command": "note.delete", "context": "roll"}),
        )
        .unwrap();
    let mine = Chord::parse("Alt+Shift+K", Os::Win).unwrap();
    s.open_settings(Section::Keys);
    s.settings_pick(&MenuKind::More, "reset-all");
    let (preset, keys, n) = s.with_settings(|p| {
        (
            p.keymap.preset.clone(),
            p.keymap.keys_of("roll", "note.delete"),
            p.keymap.changed_rows(),
        )
    });
    // The preset stays; only the user's rows are gone.
    assert_eq!(preset, "ableton");
    assert!(!keys.contains(&mine), "{keys:?}");
    assert_eq!(n, 0);
    // 取り消す gives the row back.
    press(&mut s, &g, &Do::ToastUndo);
    assert!(s.with_settings(|p| p.keymap.keys_of("roll", "note.delete").contains(&mine)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_preset_keeps_the_users_rows() {
    let dir_tmp = temp("preset");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    let mine = Chord::parse("Alt+Shift+K", Os::Win).unwrap();
    s.song
        .set(
            "keymap.set",
            json!({"key": "Alt+Shift+K", "command": "note.delete", "context": "roll"}),
        )
        .unwrap();
    let changed = s.with_settings(|p| p.keymap.changed_rows());
    assert!(changed > 0);
    s.open_settings(Section::Keys);
    press(&mut s, &g, &Do::Menu(MenuKind::Preset));
    assert!(s.settings.as_ref().unwrap().menu.is_some());
    // The list opens on Studio Pro (in use); the next row is Cubase.
    s.settings_key("down", none());
    s.settings_key("enter", none());
    let (preset, keys) = s.with_settings(|p| {
        (
            p.keymap.preset.clone(),
            p.keymap.keys_of("roll", "note.delete"),
        )
    });
    assert_eq!(preset, "cubase");
    assert!(keys.contains(&mine), "{keys:?}");
    let toast = s
        .settings
        .as_ref()
        .unwrap()
        .toast
        .clone()
        .expect("the toast");
    assert!(toast.text.contains("Cubase"), "{}", toast.text);
    assert!(toast.text.contains("そのまま"), "{}", toast.text);
    assert!(s.settings_scene(&g).has_text(&toast.text));

    // 取り消す puts the old preset back, the user's row still there.
    press(&mut s, &g, &Do::ToastUndo);
    let (preset, keys) = s.with_settings(|p| {
        (
            p.keymap.preset.clone(),
            p.keymap.keys_of("roll", "note.delete"),
        )
    });
    assert_eq!(preset, "studio-pro");
    assert!(keys.contains(&mine));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_the_cli_changes_shows_on_the_screen() {
    let dir_tmp = temp("cli");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    let follows = "edit.automation.follows-clips";
    let on = pref_bool(&s, follows);
    s.open_settings(Section::Edit);
    assert!(s.settings_scene(&g).find(&switch(follows, on)).is_some());

    // Another program (the CLI) writes the file; the next poll reads it.
    let mut cli = Settings::open(&dir);
    cli.prefs.set(follows, &json!(!on)).unwrap();
    cli.save(xirius_prefs::File::Prefs).unwrap();
    assert!(s.idle().changed);
    assert_eq!(pref_bool(&s, follows), !on);
    assert!(s.settings_scene(&g).find(&switch(follows, !on)).is_some());

    // A command in this program (the AI over MCP): the theme follows at once.
    assert_eq!(
        s.wanted_theme(xirius_design::Theme::Dark),
        xirius_design::Theme::Dark
    );
    s.song
        .set("prefs.set", json!({"key": "ui.theme", "value": "light"}))
        .unwrap();
    assert_eq!(
        s.wanted_theme(xirius_design::Theme::Dark),
        xirius_design::Theme::Light
    );
    s.song
        .set("prefs.set", json!({"key": "ui.theme", "value": "auto"}))
        .unwrap();
    assert_eq!(
        s.wanted_theme(xirius_design::Theme::Light),
        xirius_design::Theme::Light
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_search_takes_text_from_the_ime() {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.open_settings(Section::Keys);
    press(&mut s, &g, &Do::KeyMode(false));
    let label = xirius_prefs::ext::command_label("note.quantize", s.song.registry());
    s.settings_text(None, &label, true);
    assert_eq!(s.settings.as_ref().unwrap().query, label);
    s.settings_unmark();
    let scene = s.settings_scene(&g);
    assert!(scene.field.is_some());
    assert!(scene.has_text(&label));
    // A name nothing has.
    s.settings_text(None, "ぞぞぞ", false);
    assert!(s.settings_scene(&g).has_text("見つかりません"));
    assert!(s.settings_key("escape", none()));
    assert!(s.settings.as_ref().unwrap().query.is_empty());
}

#[test]
fn a_hold_changed_in_the_settings_is_what_the_roll_and_the_arrangement_use() {
    use crate::tools::{Area, Tool};
    let dir_tmp = temp("holds");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, _g) = launch(&dir);
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    // Nothing changed: the app's own holds.
    assert_eq!(s.tools().held(Area::Roll, alt), Tool::Select);
    assert_eq!(s.tools().held(Area::Roll, ctrl()), Tool::Draw);
    s.song
        .set(
            "mousemap.set",
            json!({"context": "roll.select.hold", "modifier": "Alt", "action": "tool:erase"}),
        )
        .unwrap();
    s.song
        .set(
            "mousemap.set",
            json!({"context": "arrange.select.hold", "modifier": "Mod", "action": "tool:cut"}),
        )
        .unwrap();
    assert_eq!(s.tools().held(Area::Roll, alt), Tool::Eraser);
    // The other holds of the table stay.
    assert_eq!(s.tools().held(Area::Roll, ctrl()), Tool::Draw);
    assert_eq!(s.tools().held(Area::Roll, none()), Tool::Select);
    assert_eq!(s.tools().held(Area::Arrange, ctrl()), Tool::Cut);
    // A tool whose holds are not changed keeps them.
    assert_eq!(s.holds().tool(Area::Arrange, Tool::Cut, alt), Tool::Select);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_mouse_cell_is_set_from_its_menu() {
    let dir_tmp = temp("mouse");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.open_settings(Section::Keys);
    press(&mut s, &g, &Do::Sub(Sub::Mouse));
    let scene = s.settings_scene(&g);
    let cell = scene
        .items
        .iter()
        .find(|i| matches!(&i.act, Do::Menu(MenuKind::MouseCell { .. })))
        .cloned()
        .expect("a cell of the table");
    let Do::Menu(MenuKind::MouseCell { ctx, modifier }) = cell.act.clone() else {
        unreachable!()
    };
    let before = s.with_settings(|p| p.mousemap.get(&ctx, &modifier));
    press(&mut s, &g, &cell.act);
    // The list opens on the choice in use; the one over it differs.
    s.settings_key("up", none());
    s.settings_key("enter", none());
    let after = s.with_settings(|p| p.mousemap.get(&ctx, &modifier));
    assert_ne!(before, after);
    assert!(s.with_settings(|p| !p.mousemap.changed.is_empty()));
    drop(s);
    let back = Settings::open(&dir);
    assert_eq!(back.mousemap.get(&ctx, &modifier), after);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn key_017_the_preview_names_every_zone_and_cell_of_each_area() {
    let dir_tmp = temp("key017-zones");
    let (mut s, g) = launch(dir_tmp.path());
    s.open_settings(Section::Keys);
    press(&mut s, &g, &Do::Sub(Sub::Mouse));
    for area in xirius_prefs::mousemap::AREAS {
        press(&mut s, &g, &Do::Area(area.name.to_string()));
        let tool = mouse::table_tools(area.name)[0].name;
        let zone = s.settings.as_ref().unwrap().zone.clone();
        assert_eq!(s.settings.as_ref().unwrap().tool, tool, "{}", area.name);
        let scene = s.settings_scene(&g);
        for z in xirius_prefs::mousemap::zones(area.name) {
            assert!(
                scene
                    .items
                    .iter()
                    .any(|i| i.act == Do::Zone(z.name.to_string())),
                "{}.{} is not in the preview",
                area.name,
                z.name
            );
        }
        // The cells of the table are the contexts the map names.
        for gesture in xirius_prefs::mousemap::gestures(area.name) {
            let name = format!("{}.{tool}.{zone}.{}", area.name, gesture.name);
            assert!(
                scene.items.iter().any(|i| matches!(&i.act,
                    Do::Menu(MenuKind::MouseCell { ctx, .. }) if *ctx == name)),
                "{name} has no cell"
            );
        }
    }
}

#[test]
fn key_017_a_cell_offers_only_what_its_area_has() {
    let names =
        |ctx: &str| -> Vec<String> { mouse::choices(ctx).into_iter().map(|(a, _)| a).collect() };
    let lane = names("lane.select.cc-point.drag");
    assert!(lane.iter().any(|a| a == "cc.point.move --lock"));
    assert!(
        lane.iter()
            .all(|a| !a.starts_with("clip.") && !a.starts_with("auto."))
    );
    let auto = names("automation.select.point.click");
    assert!(auto.iter().any(|a| a == "auto.point.delete"));
    assert!(
        auto.iter()
            .all(|a| !a.starts_with("note.") && !a.starts_with("cc."))
    );
    assert_eq!(
        names("ruler.any.empty.wheel"),
        ["", "view.zoom.time", "view.zoom.data", "view.scroll"]
    );
    assert_eq!(
        names("ruler.any.empty.drag"),
        ["", "view.zoom.time", "view.zoom.data", "view.scroll"]
    );
    assert_eq!(names("ruler.any.empty.click"), ["", "transport.locate"]);
}

#[test]
fn edt_070_the_autoscroll_mode_is_chosen_in_the_settings_and_kept() {
    use crate::follow::Mode;
    let dir_tmp = temp("autoscroll");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    assert_eq!(s.follow.mode, Mode::TurnOver);
    s.open_settings(Section::Edit);
    for (value, mode) in [
        ("center", Mode::ContinuousCenter),
        ("left", Mode::ContinuousLeft),
        ("turn-over", Mode::TurnOver),
    ] {
        let act = Do::Pref("edit.autoscroll".into(), value.into());
        assert!(
            s.settings_scene(&g).find(&act).is_some(),
            "{value} is not offered"
        );
        press(&mut s, &g, &act);
        // The idle poll carries a changed preference to the follow mode.
        s.settings_reload();
        assert_eq!(s.follow.mode, mode, "{value}");
    }
    press(
        &mut s,
        &g,
        &Do::Pref("edit.autoscroll".into(), "center".into()),
    );
    assert!(!s.song.can_undo());
    drop(s);
    let (mut s, _) = launch(&dir);
    s.apply_edit_prefs();
    assert_eq!(s.follow.mode, Mode::ContinuousCenter);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_song_keeps_the_theme_and_what_is_chosen_after_it_is_there_the_next_launch() {
    let dir_tmp = temp("theme-new-song");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    let dark = xirius_design::Theme::Dark;
    s.open_settings(Section::General);
    press(&mut s, &g, &Do::Pref("ui.theme".into(), "light".into()));
    s.close_settings();
    // The entrance of a new song is drawn in the theme that was chosen, not in the first one.
    s.new_song_sync();
    assert_eq!(s.wanted_theme(dark), xirius_design::Theme::Light);
    // What is chosen in the second song is written to the same folder: "auto" follows the
    // system, and "light" chosen again after another new song stays.
    s.open_settings(Section::General);
    press(&mut s, &g, &Do::Pref("ui.theme".into(), "auto".into()));
    s.close_settings();
    s.new_song_sync();
    assert_eq!(s.wanted_theme(dark), dark);
    assert_eq!(
        s.wanted_theme(xirius_design::Theme::Light),
        xirius_design::Theme::Light,
        "auto follows the system"
    );
    s.open_settings(Section::General);
    press(&mut s, &g, &Do::Pref("ui.theme".into(), "light".into()));
    s.close_settings();
    drop(s);

    let (s, _) = launch(&dir);
    assert_eq!(s.wanted_theme(dark), xirius_design::Theme::Light);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The audio device `inputs` of the studio's settings, shown on `section`.
fn with_device(
    dir: &std::path::Path,
    inputs: &[&str],
    ports: &[&str],
    section: Section,
) -> (Studio, Geometry) {
    let (mut s, g) = launch(dir);
    s.open_settings(section);
    {
        let mut io = s.song.io.lock().unwrap_or_else(|e| e.into_inner());
        io.view.devices = vec![xirius_engine::devices::Device {
            name: "Speakers".into(),
            inputs: inputs.iter().map(|n| n.to_string()).collect(),
            outputs: vec!["Out 1".into(), "Out 2".into()],
        }];
        io.view.device = "Speakers".into();
        io.view.ports = ports.iter().map(|n| n.to_string()).collect();
    }
    (s, g)
}

#[test]
fn a_device_without_inputs_cannot_detect_one() {
    let dir_tmp = temp("no-input-detect");
    let dir = dir_tmp.path().to_path_buf();
    let (s, g) = with_device(&dir, &[], &[], Section::Audio);
    let scene = s.settings_scene(&g);
    assert!(
        scene.find(&Do::Detect).is_none(),
        "nothing to detect: the button does nothing"
    );
    assert!(scene.has_text("入力を検出"), "it is still there, dim");
    assert!(
        scene.has_text("なし"),
        "the list of inputs is not empty-looking"
    );

    // With inputs it is pressable, and there is no empty row.
    let (s, g) = with_device(&dir, &["In 1", "In 2"], &[], Section::Audio);
    let scene = s.settings_scene(&g);
    assert!(scene.find(&Do::Detect).is_some());
    assert!(!scene.has_text("なし"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn with_no_midi_port_the_list_of_inputs_has_the_pc_keyboard() {
    use xirius_session::audio_settings::PC_KEYBOARD;
    let dir_tmp = temp("no-midi-ports");
    let dir = dir_tmp.path().to_path_buf();
    let (s, g) = with_device(&dir, &[], &[], Section::Midi);
    let scene = s.settings_scene(&g);
    assert!(scene.has_text(PC_KEYBOARD), "the list is never empty");
    assert!(!scene.has_text("なし"));
    let off = Do::Pref(format!("midi.inputs.{PC_KEYBOARD}"), "false".into());
    assert!(scene.find(&off).is_some(), "it turns off as a port does");

    // The ports come after it.
    let (s, g) = with_device(&dir, &[], &["Keystation 49"], Section::Midi);
    let scene = s.settings_scene(&g);
    let at = |t: &str| scene.texts.iter().position(|x| x.contains(t)).unwrap();
    assert!(at(PC_KEYBOARD) < at("Keystation 49"));
    assert!(!scene.has_text("なし"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pc_key_on_the_midi_page_lights_the_pc_keyboard_and_shows_its_note() {
    use xirius_session::audio_settings::PC_KEYBOARD;
    let dir_tmp = temp("pc-key-light");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = with_device(&dir, &[], &[], Section::Midi);
    let poll = |s: &Studio| {
        let mut io = s.song.io.lock().unwrap();
        io.poll(std::time::Instant::now());
        io.view.received.clone()
    };
    assert!(poll(&s).is_empty());
    assert!(!s.settings_scene(&g).has_text("G4"));

    // G is the G of the fourth octave.
    assert!(s.settings_key("g", Mods::default()));
    assert!(s.settings.is_some(), "the key stays on the page");
    assert_eq!(poll(&s), vec![PC_KEYBOARD.to_string()]);
    assert!(s.settings_scene(&g).has_text("G4"));
    s.pc_key_up("g");

    // Off: the keys light nothing and play nothing.
    let off = Do::Pref(format!("midi.inputs.{PC_KEYBOARD}"), "false".into());
    let i = s.settings_scene(&g).find(&off).cloned().unwrap();
    assert!(s.settings_press(&g, i.r.x + i.r.w / 2.0, i.r.y + i.r.h / 2.0));
    assert!(!s.pc_keyboard_on());
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(poll(&s).is_empty());
    assert!(s.settings_key("a", Mods::default()));
    assert!(poll(&s).is_empty());
    assert!(!s.settings_scene(&g).has_text("C4"));
    s.settings = None;
    s.pc_keys.on = true;
    assert!(
        !s.pc_key_down("a", false),
        "A goes back to being a shortcut"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_default_holds_the_mouse_table_and_the_top_bar_agree_on_what_ctrl_does() {
    use crate::tools::{Area, Tool};
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    // The Studio Pro form: Ctrl is the range in the arrange; in the roll it is the eraser, and
    // Ctrl+Shift splits.
    let both = Mods {
        ctrl: true,
        shift: true,
        ..Mods::default()
    };
    assert_eq!(
        s.holds().tool(Area::Arrange, Tool::Select, ctrl()),
        Tool::Range
    );
    assert_eq!(s.holds().tool(Area::Roll, Tool::Select, ctrl()), Tool::Draw);
    assert_eq!(s.holds().tool(Area::Roll, Tool::Select, both), Tool::Cut);
    // The settings' table draws the same rows: one per hold, the tool beside its modifier.
    s.open_settings(Section::Keys);
    press(&mut s, &g, &Do::Sub(Sub::Mouse));
    let rows = s.with_settings(|p| {
        (
            p.mousemap.table("arrange.select.hold"),
            p.mousemap.table("roll.select.hold"),
        )
    });
    let rows = (
        rows.0.into_iter().collect::<Vec<_>>(),
        rows.1.into_iter().collect::<Vec<_>>(),
    );
    assert_eq!(rows.0, vec![("Mod".to_string(), "tool:range".to_string())]);
    assert_eq!(
        rows.1,
        vec![
            ("Mod".to_string(), "tool:draw".to_string()),
            ("Mod+Shift".to_string(), "tool:cut".to_string())
        ]
    );
    assert!(s.settings_scene(&g).has_text("一時的なツールの切り替え"));
    // Choosing another alternative tool moves both together.
    s.song
        .set("tool.alt", json!({"tool": "mute", "area": "arrange"}))
        .unwrap();
    assert_eq!(
        s.holds().tool(Area::Arrange, Tool::Select, ctrl()),
        Tool::Mute
    );
    s.arrange.focus = true;
    s.choose_editor();
    assert_eq!(s.alt_tool(), Tool::Mute);
}

fn macro_steps(s: &Studio, name: &str) -> Vec<String> {
    s.with_settings(|p| {
        p.macros
            .get(name)
            .map(|m| m.steps.clone())
            .unwrap_or_default()
    })
}

fn type_name(s: &mut Studio, text: &str) {
    for _ in 0..20 {
        s.settings_key("backspace", none());
    }
    s.settings_text(None, text, false);
}

#[test]
fn key_015_a_macro_is_made_edited_given_a_key_and_run_from_the_shortcut_list() {
    let dir_tmp = temp("macros");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.open_settings(Section::Keys);
    // The group is the first thing of the list, with 「＋」 in it.
    assert!(s.settings_scene(&g).has_text("マクロ"));
    press(&mut s, &g, &Do::MacroNew);
    assert_eq!(macro_steps(&s, "マクロ 1"), Vec::<String>::new());
    // The name is typed at once, and Enter puts it in.
    type_name(&mut s, "二本足す");
    assert!(s.settings_key("enter", none()));
    assert!(macro_steps(&s, "二本足す").is_empty());
    assert!(s.with_settings(|p| p.macros.get("マクロ 1").is_none()));
    assert_eq!(
        s.settings.as_ref().unwrap().macro_open.as_deref(),
        Some("二本足す")
    );

    // Steps are added from the list of commands, in order, and moved and taken out.
    press(&mut s, &g, &Do::MacroPick);
    assert!(s.settings_scene(&g).has_text("コマンドを検索"));
    s.settings_text(None, "note.mute", false);
    assert!(s.settings_key("enter", none()));
    assert_eq!(macro_steps(&s, "二本足す"), vec!["note.mute".to_string()]);
    press(&mut s, &g, &Do::MacroAdd("note.delete".into()));
    s.settings_text(None, "clip.split", false);
    press(&mut s, &g, &Do::MacroAdd("clip.split".into()));
    assert_eq!(macro_steps(&s, "二本足す").len(), 3);
    press(&mut s, &g, &Do::MacroMove(0, true));
    assert_eq!(
        macro_steps(&s, "二本足す"),
        vec!["note.delete", "note.mute", "clip.split"]
    );
    press(&mut s, &g, &Do::MacroRemove(2));
    assert_eq!(
        macro_steps(&s, "二本足す"),
        vec!["note.delete", "note.mute"]
    );
    // The key lives in the context of the first command (the roll).
    assert_eq!(
        xirius_prefs::macro_ext::context_of(&macro_steps(&s, "二本足す")),
        "roll"
    );

    // A key is given from the row like any other.
    let line = xirius_prefs::macros::run_line("二本足す");
    s.settings_do(
        &g,
        Do::Capture {
            ctx: "roll".into(),
            cmd: line.clone(),
            from: None,
        },
        Rect::default(),
    );
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    assert!(s.settings_key("m", alt));
    let chord = Chord::parse("Alt+M", Os::Win).unwrap();
    assert!(s.with_settings(|p| p.keymap.keys_of("roll", &line).contains(&chord)));
    let scene = s.settings_scene(&g);
    assert!(
        scene.has_text(&chord.display(Os::current())),
        "{:?}",
        scene.texts
    );
    // The macro is one row of the group, and not one more of the roll's list.
    s.settings_do(&g, Do::MacroOpen("二本足す".into()), Rect::default());
    let scene = s.settings_scene(&g);
    assert_eq!(
        scene.texts.iter().filter(|t| *t == "二本足す").count(),
        1,
        "the macro's row is listed once"
    );
    assert!(!scene.texts.iter().any(|t| t.contains("macro.run")));

    // Kept for the next launch, the key with it.
    drop(s);
    let again = Settings::open(&dir);
    assert!(again.problems.is_empty(), "{:?}", again.problems);
    assert_eq!(
        again.macros.get("二本足す").map(|m| m.steps.clone()),
        Some(vec!["note.delete".to_string(), "note.mute".to_string()])
    );
    assert!(again.keymap.keys_of("roll", &line).contains(&chord));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn key_015_the_screen_runs_a_macro_as_one_step_and_removing_it_takes_its_key() {
    let dir_tmp = temp("macro-run");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.open_settings(Section::Keys);
    s.song
        .set(
            "macro.set",
            json!({"name": "二つ足す", "steps": [
                {"command": "track.add.instrument --name 一"},
                {"command": "track.add.instrument --name 二"},
            ]}),
        )
        .unwrap();
    s.song
        .set(
            "keymap.set",
            json!({"key": "Alt+M", "command": "macro.run --name 二つ足す", "context": "global"}),
        )
        .unwrap();
    let before = s.project().tracks.len();
    s.settings_do(&g, Do::MacroOpen("二つ足す".into()), Rect::default());
    press(&mut s, &g, &Do::MacroRun("二つ足す".into()));
    assert_eq!(s.project().tracks.len(), before + 2, "{:?}", s.error);
    // One undo takes both back.
    s.settings = None;
    assert!(s.undo(&g));
    assert_eq!(s.project().tracks.len(), before);

    // A macro with no step has nothing to run; the screen says so.
    s.open_settings(Section::Keys);
    s.song
        .set("macro.set", json!({"name": "空", "steps": []}))
        .unwrap();
    s.settings_do(&g, Do::MacroRun("空".into()), Rect::default());
    assert!(s.settings.as_ref().unwrap().toast.is_some());

    // Removing the macro takes its key.
    s.settings_do(&g, Do::MacroOpen("二つ足す".into()), Rect::default());
    press(&mut s, &g, &Do::MacroDelete("二つ足す".into()));
    assert!(s.with_settings(|p| p.macros.get("二つ足す").is_none()));
    let line = xirius_prefs::macros::run_line("二つ足す");
    assert!(s.with_settings(|p| p.keymap.keys_of("global", &line).is_empty()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn key_015_macros_are_found_by_name_and_a_macro_with_a_bad_key_row_still_shows() {
    let dir_tmp = temp("macro-search");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.open_settings(Section::Keys);
    s.song
        .set(
            "macro.set",
            json!({"name": "仕上げ", "steps": [{"command": "note.mute"}]}),
        )
        .unwrap();
    assert!(s.settings_scene(&g).has_text("仕上げ"));
    s.settings_do(&g, Do::KeyMode(false), Rect::default());
    s.settings_text(None, "なにもない名前", false);
    let none_found = s.settings_scene(&g);
    assert!(!none_found.has_text("仕上げ"));
    assert!(!none_found.has_text("マクロ"), "{:?}", none_found.texts);
    s.settings_do(&g, Do::ClearQuery, Rect::default());
    s.settings_text(None, "仕上", false);
    assert!(s.settings_scene(&g).has_text("仕上げ"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The list of a scan with two usable plugins and one bundle that could not be read.
fn a_scan_found(name_of_bad: &str) -> xirius_plugins::Catalog {
    use xirius_core::model::PluginKind;
    let entry = |name: &str, vendor: &str, format: &str, ara: bool| xirius_plugins::Entry {
        uid: format!("{:0>32}", name.len()),
        name: name.into(),
        vendor: vendor.into(),
        kind: PluginKind::Effect,
        format: format.into(),
        category: "Fx".into(),
        version: "1.0".into(),
        path: PathBuf::from(format!("C:/plugins/{name}.{format}")),
        ara,
        requires_process_main: false,
    };
    xirius_plugins::Catalog {
        entries: vec![
            entry("Zeta Comp", "Test Audio", "vst3", false),
            entry("Alpha Tune", "Pitch Works", "vst3", true),
        ],
        failures: vec![xirius_plugins::Failure {
            path: PathBuf::from(format!("C:/plugins/{name_of_bad}.vst3")),
            kind: xirius_plugins::FailureKind::Timeout,
            error: "no answer".into(),
        }],
    }
}

#[test]
fn the_plugin_page_lists_what_the_scan_found_and_the_unusable_last_with_the_reason() {
    let dir_tmp = temp("plugin-page-list");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.use_plugins(a_scan_found("Broken Verb"));
    s.open_settings(Section::Plugins);
    let scene = s.settings_scene(&g);
    for name in ["Alpha Tune", "Zeta Comp", "Test Audio", "Broken Verb"] {
        assert!(scene.has_text(name), "{name} is on the page");
    }
    assert!(
        scene.has_text("応答がありません"),
        "the unusable one says why"
    );
    for fixture in ["Pro-Q 4", "Serum 2", "Tape Echo 3", "Melodyne 5"] {
        assert!(!scene.has_text(fixture), "{fixture} was only a fixture");
    }
    // The usable ones are by name; the unusable one is after them.
    let at = |t: &str| scene.texts.iter().position(|x| x.contains(t)).unwrap();
    assert!(at("Alpha Tune") < at("Zeta Comp"));
    assert!(at("Zeta Comp") < at("Broken Verb"));
    // The count of the unusable ones is by the page's switch and the nav.
    assert!(scene.has_text("読み込めない（1）"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_plugin_page_with_no_plugin_says_none() {
    let dir_tmp = temp("plugin-page-none");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.use_plugins(xirius_plugins::Catalog::default());
    s.open_settings(Section::Plugins);
    let scene = s.settings_scene(&g);
    assert!(scene.has_text("なし"));
    assert!(!scene.has_text("Pro-Q 4"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_plugin_page_shows_the_scan_that_runs_and_stops_it_from_its_own_row() {
    let dir_tmp = temp("plugin-page-scanning");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.use_plugins(a_scan_found("Broken Verb"));
    s.open_settings(Section::Plugins);
    // Not running: the button scans, and there is no row to stop.
    let scene = s.settings_scene(&g);
    assert!(scene.find(&Do::Rescan).is_some());
    assert!(scene.find(&Do::StopScan).is_none());

    s.pose_scan(12, 48);
    let scene = s.settings_scene(&g);
    assert!(scene.has_text("スキャン中"));
    assert!(scene.has_text("12 / 48"));
    assert!(scene.find(&Do::StopScan).is_some());
    // The head has how far it is, and once the scan says, the bundle it opens (PLG-059).
    let file = r"C:\Program Files\Common Files\VST3\Kilohearts\kHs Gain.vst3";
    s.mixer.scan.file = file.to_string();
    let scene = s.settings_scene(&g);
    assert!(
        scene.texts.iter().any(|t| t.ends_with("kHs Gain.vst3")),
        "{:?}",
        scene.texts
    );
    assert!(
        scene.find(&Do::Rescan).is_none(),
        "a second scan cannot start while one runs"
    );
    // What the last scan found stays under it.
    assert!(scene.has_text("Zeta Comp"));

    // Before the scan knows how many bundles there are, it only says it is looking.
    s.pose_scan(0, 0);
    s.mixer.scan.file.clear();
    let scene = s.settings_scene(&g);
    assert!(scene.has_text("検索中"));
    assert!(!scene.has_text("0 / 0"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_plugin_page_marks_the_bundle_tried_again_and_only_that_one() {
    let dir_tmp = temp("plugin-page-retry");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    let mut catalog = a_scan_found("Broken Verb");
    catalog.failures.push(xirius_plugins::Failure {
        path: PathBuf::from("C:/plugins/Hung Delay.vst3"),
        kind: xirius_plugins::FailureKind::Crashed,
        error: "gone".into(),
    });
    s.use_plugins(catalog);
    s.open_settings(Section::Plugins);
    let retry = |name: &str| Do::Retry(PathBuf::from(format!("C:/plugins/{name}.vst3")));
    let scene = s.settings_scene(&g);
    assert!(scene.find(&retry("Broken Verb")).is_some());
    assert!(scene.find(&retry("Hung Delay")).is_some());

    s.browser.retrying = Some(PathBuf::from("C:/plugins/Hung Delay.vst3"));
    let scene = s.settings_scene(&g);
    assert!(scene.find(&retry("Broken Verb")).is_some());
    assert!(
        scene.find(&retry("Hung Delay")).is_none(),
        "its button is the spinner while it is read again"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_crosshair_switch_reaches_the_pointer_and_the_next_launch() {
    let dir_tmp = temp("crosshair");
    let dir = dir_tmp.path().to_path_buf();
    let key = "edit.cursor.crosshair";
    let (mut s, g) = launch(&dir);
    assert!(!pref_bool(&s, key), "the tool's own picture by default");
    assert!(!s.crosshair);
    s.open_settings(Section::Edit);
    press(&mut s, &g, &switch(key, false));
    assert!(s.crosshair, "the switch reaches the studio");
    drop(s);

    let (mut s, g) = launch(&dir);
    s.apply_edit_prefs();
    assert!(s.crosshair, "and is there the next launch");
    s.open_settings(Section::Edit);
    press(&mut s, &g, &switch(key, true));
    assert!(!s.crosshair);
}

#[test]
fn key_019_the_mouse_table_has_ctrl_shift_and_holds_are_edited_on_screen() {
    let dir_tmp = temp("key019");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.open_settings(Section::Keys);
    press(&mut s, &g, &Do::Sub(Sub::Mouse));
    press(&mut s, &g, &Do::Area("roll".into()));
    let texts = |s: &mut Studio| s.settings_scene(&g).texts.clone();
    let t = texts(&mut s);
    // The Ctrl+Shift column shows the roll's cut; the tools are named as the toolbar names them.
    assert!(t.iter().any(|x| x == "Ctrl+Shift"), "{t:?}");
    assert!(t.iter().any(|x| x == "分割（一時的）"), "{t:?}");
    // A held tool too long for its column ends in 「…」 inside the cell, in blue, and the
    // tooltip has the whole name.
    fn cut(ops: &[super::scene::Op]) -> Option<super::scene::Text> {
        ops.iter().find_map(|o| match o {
            super::scene::Op::Text(t) if t.s.starts_with("鉛筆") => Some(t.clone()),
            super::scene::Op::Mask(_, inner) => cut(inner),
            _ => None,
        })
    }
    let c = crate::paint::Colors::new(xirius_design::Theme::Dark);
    let e = cut(&s.settings_scene_with(&g, &c).ops).expect("the pencil's held cell");
    assert_eq!(e.color, c.ink_blue);
    assert!(
        e.s == "鉛筆（一時的）"
            || (e.s.ends_with('…') && "鉛筆（一時的）".starts_with(e.s.trim_end_matches('…'))),
        "{}",
        e.s
    );
    assert!(
        super::scene::width(&e.s, e.size) <= e.r.w,
        "{} in {}",
        e.s,
        e.r.w
    );
    if e.s != "鉛筆（一時的）" {
        s.settings_hover(e.r.x + 4.0, e.r.y + e.r.h / 2.0);
        let tip = s.settings_scene(&g).tip.map(|(_, t)| t);
        assert_eq!(tip.as_deref(), Some("鉛筆（一時的）"));
    }
    // No internal name is written on the screen (it is the heading's tooltip).
    assert!(
        t.iter()
            .all(|x| !x.contains("roll.select") && !x.contains("note.")),
        "{t:?}"
    );
    // Moving Ctrl's tool onto Ctrl+Shift, which is taken, is refused and says why.
    let ctx = "roll.select.hold".to_string();
    s.settings_pick(&MenuKind::HoldMod(ctx.clone(), "Mod".into()), "Mod+Shift");
    assert_eq!(
        s.with_settings(|p| p.mousemap.get(&ctx, "Mod")),
        "tool:draw"
    );
    assert_eq!(
        s.with_settings(|p| p.mousemap.get(&ctx, "Mod+Shift")),
        "tool:cut"
    );
    assert!(s.with_settings(|p| p.mousemap.changed.is_empty()));
    let e = s.settings.as_ref().unwrap().hold_error.clone().unwrap();
    assert!(e.2.contains("使われています"), "{e:?}");
    assert!(texts(&mut s).iter().any(|x| x.contains("使われています")));
    // A free modifier takes it.
    s.settings_pick(&MenuKind::HoldMod(ctx.clone(), "Mod".into()), "Alt");
    assert_eq!(
        s.with_settings(|p| p.mousemap.get(&ctx, "Alt")),
        "tool:draw"
    );
    assert!(s.settings.as_ref().unwrap().hold_error.is_none());
    // A hold is added for a tool, and taken away again.
    s.settings_pick(&MenuKind::HoldAdd("roll".into()), "mute");
    let added = s.with_settings(|p| p.mousemap.table("roll.mute.hold"));
    assert_eq!(added.len(), 1, "{added:?}");
    let (m, _) = added.iter().next().unwrap();
    press(
        &mut s,
        &g,
        &Do::HoldRemove("roll.mute.hold".into(), m.clone()),
    );
    assert!(s.with_settings(|p| p.mousemap.table("roll.mute.hold").is_empty()));
    // What was set is on file.
    drop(s);
    let back = Settings::open(&dir);
    assert_eq!(back.mousemap.get(&ctx, "Alt"), "tool:draw");
    assert_eq!(back.mousemap.get(&ctx, "Mod"), "");
}

#[test]
fn set_001_the_search_by_q_shows_a_row_per_place_and_keeps_command_names_off_the_rows() {
    let dir = temp("set-001-q");
    let (mut s, g) = launch(dir.path());
    s.open_settings(Section::Keys);
    assert!(s.settings_key("q", none()));
    let texts = s.settings_scene(&g).texts;
    for want in ["ノートをクオンタイズ", "クリップをクオンタイズ"] {
        assert_eq!(
            texts.iter().filter(|t| *t == want).count(),
            1,
            "{want}: {texts:?}"
        );
    }
    let (arrange, roll) = s.with_settings(|p| {
        (
            p.keymap.keys_of("arrange", "clip.quantize"),
            p.keymap.keys_of("roll", "note.quantize"),
        )
    });
    assert_eq!(arrange.len(), 1);
    assert_eq!(arrange, roll);
    // The command's own name is for the tooltip: no row text is one.
    assert!(
        !texts
            .iter()
            .any(|t| t.contains("clip.quantize") || t.contains("note.quantize")),
        "{texts:?}"
    );
    // D: the duplicates, one row for each place.
    assert!(s.settings_key("escape", none()));
    assert!(s.settings_key("d", none()));
    let d = s.settings_scene(&g).texts;
    let at = |t: &str| {
        d.iter()
            .position(|x| x == t)
            .unwrap_or_else(|| panic!("{t}: {d:?}"))
    };
    for (place, row) in [
        ("アレンジ", "クリップを複製"),
        ("ピアノロール", "ノートを複製"),
        ("レーン", "ノートを複製"),
        ("オートメーション", "クリップを複製"),
    ] {
        let g = at(place);
        assert_eq!(d[g + 2], "D", "{d:?}");
        assert_eq!(d[g + 3], row, "{place}: {d:?}");
    }
    // The same key in two places is not a clash.
    let issues = s.with_settings(|p| p.keymap.conflicts(Os::Win));
    assert!(
        !issues
            .iter()
            .any(|i| format!("{i:?}").contains("clip.quantize")),
        "{issues:?}"
    );
}

#[test]
fn a_long_path_loses_its_front_and_keeps_its_file() {
    let path = "C:/Program Files/Common Files/VST3/Some Vendor/Some Very Long Plugin Name.vst3";
    let w = 200.0;
    let short = fit_front(path, FontSize::Caption, w);
    assert!(short.starts_with('…'), "{short}");
    assert!(short.ends_with("Name.vst3"), "{short}");
    assert!(scene::width(&short, FontSize::Caption) <= w);
    assert_eq!(fit_front("VST3", FontSize::Caption, w), "VST3");
}

/// A launch whose plugins are looked for in `dir`'s own folder, kept in `dir`'s own list.
fn launch_scanning(dir: &std::path::Path) -> (Studio, Geometry) {
    let plugins = dir.join("VST3");
    std::fs::create_dir_all(&plugins).unwrap();
    let opts = xirius_plugins::ScanOptions {
        dirs: vec![plugins],
        cache: dir.join("plugins.json"),
        ..xirius_plugins::ScanOptions::default()
    };
    let mut s = Studio::new(crate::song::Song::sample_with(opts));
    s.song.load_settings(dir);
    // As the real window does.
    s.mixer.scan.auto = true;
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    (s, g)
}

fn wait_scan(s: &mut Studio) {
    let start = std::time::Instant::now();
    while s.mixer.scan.running() {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(30),
            "the scan never ended"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
        s.poll_plugin_scan();
    }
}

#[test]
fn a_first_launch_scans_in_the_background_and_the_plugin_page_follows_it() {
    let dir_tmp = temp("first-scan");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch_scanning(&dir);
    assert!(!dir.join("plugins.json").exists(), "no list yet");

    // The first frame starts it and does not wait for it.
    assert!(s.poll_plugin_scan());
    assert!(s.mixer.scan.running());
    s.open_settings(Section::Plugins);
    let scene = s.settings_scene(&g);
    assert!(scene.find(&Do::StopScan).is_some());
    assert!(scene.find(&Do::Rescan).is_none());
    let st = s.song.run("plugin.scan.status", json!({})).unwrap();
    assert!(st["job"].is_string(), "{st:?}");

    // It ends on a later frame, and the page can scan again.
    wait_scan(&mut s);
    assert!(dir.join("plugins.json").exists());
    assert_eq!(s.mixer.scan.error, None);
    let scene = s.settings_scene(&g);
    assert!(scene.find(&Do::StopScan).is_none());
    let i = scene.find(&Do::Rescan).cloned().unwrap();

    // 「再スキャン」 is plugin.rescan: a job of its own.
    let first = st["job"].clone();
    assert!(s.settings_press(&g, i.r.x + i.r.w / 2.0, i.r.y + i.r.h / 2.0));
    assert!(s.mixer.scan.running());
    let st = s.song.run("plugin.scan.status", json!({})).unwrap();
    assert!(st["job"].is_string() && st["job"] != first, "{st:?}");
    wait_scan(&mut s);

    // The next launch has the list: it does not scan by itself.
    let (mut s, _) = launch_scanning(&dir);
    assert!(!s.poll_plugin_scan());
    assert!(!s.mixer.scan.running());
}

/// UI-262: 「更新を確認」 on 一般 turns the asking at launch off, and the dot with it; it is
/// off the next launch too.
#[test]
fn turning_launch_update_check_off_asks_nothing_and_keeps_the_dot() {
    use crate::about::tests::{ask, newer_answer, stub};
    use xirius_session::app::UPDATE_PREF;

    let dir_tmp = temp("update");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    assert!(pref_bool(&s, UPDATE_PREF), "on by default");
    let (fetch, _) = stub(Ok(&newer_answer()));
    ask(&mut s, &dir.join("app"), fetch);
    assert!(s.bar_view().update);

    s.open_settings(Section::General);
    let scene = s.settings_scene(&g);
    assert!(scene.has_text("更新を確認"));
    assert!(scene.has_text("起動時"));
    press(&mut s, &g, &switch(UPDATE_PREF, true));
    assert!(!pref_bool(&s, UPDATE_PREF));
    assert!(s.bar_view().update, "off, the discovery stays");
    drop(s);

    let (mut s, _) = launch(&dir);
    assert!(!pref_bool(&s, UPDATE_PREF), "kept");
    let (fetch, asked) = stub(Ok(&newer_answer()));
    ask(&mut s, &dir.join("app2"), fetch);
    assert_eq!(
        asked.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "nothing asked"
    );
    assert!(!s.bar_view().update);
    assert!(!dir.join("app2").join("update.json").exists());
}

#[test]
fn a_macro_does_not_close_a_wheels_open_undo_group_with_its_own() {
    let dir_tmp = temp("wheel-macro");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    s.wheel_group_start(crate::context::fields::Kind::Tempo, &[]);
    assert!(s.wheel_group.is_some());
    s.song
        .settings()
        .lock()
        .unwrap()
        .macros
        .set("二段", vec!["no.such.command".into()]);
    assert!(s.run_macro(&g, "二段"));
    assert!(
        s.wheel_group.is_none(),
        "the wheel's group is closed before the macro's begins"
    );
}

/// UI-282: 「ベータ版も受け取る」 is off at first, and is kept once turned on.
#[test]
fn the_beta_channel_is_off_by_default_and_kept_when_turned_on() {
    use xirius_session::update::BETA_PREF;

    let dir_tmp = temp("update-beta");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = launch(&dir);
    assert!(!pref_bool(&s, BETA_PREF), "off by default");
    s.open_settings(Section::General);
    let scene = s.settings_scene(&g);
    assert!(scene.has_text("更新を確認"));
    assert!(scene.has_text("ベータ版も受け取る"));
    press(&mut s, &g, &switch(BETA_PREF, false));
    assert!(pref_bool(&s, BETA_PREF));
    drop(s);

    let (mut s, g) = launch(&dir);
    assert!(pref_bool(&s, BETA_PREF), "kept");
    s.open_settings(Section::General);
    press(&mut s, &g, &switch(BETA_PREF, true));
    assert!(!pref_bool(&s, BETA_PREF));
    drop(s);
    let (s, _) = launch(&dir);
    assert!(!pref_bool(&s, BETA_PREF), "turning it off is kept too");
}

#[test]
fn the_beta_switch_is_painted_visible_and_reachable_in_both_themes() {
    use scene::{Op, Pick};
    use xirius_design::Theme;
    use xirius_session::update::BETA_PREF;

    // Read the actual paint list, with its masks: a hit target alone does not prove a switch
    // is visible. Both the track and its knob must be painted inside the visible body.
    fn rounded(ops: &[Op], clip: Rect, out: &mut Vec<(Rect, gpui::Hsla)>) {
        for op in ops {
            match op {
                Op::Round(r, color, _) if scene::intersect(*r, clip) == *r => {
                    out.push((*r, *color));
                }
                Op::Mask(r, inner) => rounded(inner, scene::intersect(*r, clip), out),
                _ => {}
            }
        }
    }
    let mut s = Studio::sample();
    s.open_settings(Section::General);
    for size in [(1440.0, 860.0), (960.0, 640.0)] {
        let g = s.geometry(size.0, size.1);
        for theme in [Theme::Light, Theme::Dark] {
            let c = Colors::new(theme);
            for on in [false, true] {
                s.song
                    .settings()
                    .lock()
                    .unwrap()
                    .prefs
                    .set(BETA_PREF, &json!(on))
                    .unwrap();
                let scene = s.settings_scene_with(&g, &c);
                let item = scene.find(&switch(BETA_PREF, on)).expect("the beta switch");
                assert_eq!(item.pick, Some(Pick::Switch(on)));
                let (x, y) = (item.r.x + item.r.w / 2.0, item.r.y + item.r.h / 2.0);
                assert_eq!(scene.item_at(x, y), Some(item));
                let mut shapes = Vec::new();
                rounded(&scene.ops, Rect::new(0.0, 0.0, size.0, size.1), &mut shapes);
                assert!(
                    shapes.iter().any(|(r, _)| *r == item.r),
                    "the switch track is visible"
                );
                assert!(
                    shapes.iter().any(|(r, color)| {
                        r.w == 14.0
                            && r.h == 14.0
                            && scene::intersect(*r, item.r) == *r
                            && *color == if on { c.on_fill } else { c.switch_off[0].1 }
                    }),
                    "the switch knob is visible: {theme:?}, {on}"
                );
            }
        }
    }
}

#[test]
fn every_settings_switch_uses_the_visible_off_colors_before_and_during_hover() {
    use scene::Op;
    use xirius_design::{SwitchFace, Theme};

    let row = Rect::new(0.0, 0.0, 160.0, 40.0);
    for theme in [Theme::Dark, Theme::Light] {
        let c = Colors::new(theme);
        for hovered in [false, true] {
            let pointer = hovered.then_some((145.0, 20.0));
            let mut b = B::new(&c, row, pointer);
            let r = parts::toggle(&mut b, row.right(), row, false, Do::Nothing, None);
            let scene = b.finish();
            let expected = SwitchFace::off(theme, hovered);
            assert!(scene.ops.iter().any(|op| matches!(op,
                Op::Round(at, color, _) if *at == r && *color == crate::theme::paint(expected.track)
            )));
            assert!(scene.ops.iter().any(|op| matches!(op,
                Op::Round(at, color, _) if at.w == 14.0 && at.h == 14.0 && *color == crate::theme::paint(expected.knob)
            )));
            assert_eq!(
                scene.item_at(145.0, 20.0).unwrap().pick,
                Some(scene::Pick::Switch(false))
            );
        }
    }
}

/// The open settings' list.
fn open_pick(s: &Studio) -> Option<(crate::pick_list::PickList, Vec<crate::popup::Cell>)> {
    let m = s.settings.as_ref()?.menu.as_ref()?;
    Some((m.pick.clone()?, m.menu.layout.cells.clone()))
}

/// How many of the list part's four jobs the settings' dropdown of `kind`
/// shows (UI-338, as `pick_list::tests::counted` for the list menus): the
/// search box, the empty state, Esc's two, and ↑↓ with Enter.
fn settings_counted(s: &mut Studio, g: &Geometry, kind: &MenuKind) -> usize {
    use crate::popup::Look;
    let act = Do::Menu(kind.clone());
    press(s, g, &act);
    let Some((pick, cells)) = open_pick(s) else {
        return 0;
    };
    let mut n = 0;
    // 1: the search box, over the list and lit.
    if cells
        .first()
        .is_some_and(|c| c.look == Look::Input && c.icon == Some("search") && c.on)
    {
        n += 1;
    }
    // 2: words that match nothing hide the rows; the empty state says so.
    s.pick_text(None, "ぞぞぞ", false);
    if let Some((after, cells)) = open_pick(s)
        && after.visible().is_empty()
        && !pick.visible().is_empty()
        && cells
            .iter()
            .any(|c| c.look == Look::Status && c.text == crate::pick_list::NO_MATCH)
    {
        n += 1;
    }
    // 3: Esc clears the words, then shuts the list.
    s.settings_key("escape", none());
    let cleared = open_pick(s).is_some_and(|(p, _)| p.query.is_empty());
    s.settings_key("escape", none());
    if cleared && s.settings.as_ref().is_some_and(|u| u.menu.is_none()) {
        n += 1;
    }
    // 4: ↓ walks to the next row that runs, Enter picks and shuts.
    press(s, g, &act);
    if let Some((pick, _)) = open_pick(s) {
        let from = pick.selected;
        let nav = pick.nav();
        let want = (from + 1..nav.len())
            .find(|&i| nav[i].enabled)
            .unwrap_or(from);
        s.settings_key("down", none());
        let moved = open_pick(s).is_some_and(|(p, cells)| {
            p.selected == want
                && cells
                    .iter()
                    .filter(|c| c.cursor)
                    .all(|c| c.text == nav[want].label)
        });
        s.settings_key("enter", none());
        if moved && s.settings.as_ref().is_some_and(|u| u.menu.is_none()) {
            n += 1;
        }
    }
    if let Some(u) = s.settings.as_mut() {
        u.menu = None;
    }
    n
}

/// Every dropdown of the settings that lists choices is the list part
/// (UI-338): the four jobs are counted on each one on the screen, the
/// keyboard's and the mouse's tables too. Only 「その他」 is a menu of
/// operations (no search box).
#[test]
fn every_settings_list_is_the_pick_list_four_jobs() {
    let dir = temp("lists");
    let (mut s, g) = launch(dir.path());
    // Two devices to pick between.
    crate::audioset_shots::fixture(&mut s);
    let mut seen: Vec<MenuKind> = Vec::new();
    let mut more = false;
    for section in Section::ALL {
        let subs: &[Option<Sub>] = if section == Section::Keys {
            &[Some(Sub::Keyboard), Some(Sub::Mouse)]
        } else {
            &[None]
        };
        for sub in subs {
            s.open_settings(section);
            if let Some(sub) = sub {
                press(&mut s, &g, &Do::Sub(*sub));
            }
            let kinds: Vec<MenuKind> = s
                .settings_scene(&g)
                .items
                .iter()
                .filter_map(|i| match &i.act {
                    Do::Menu(k) => Some(k.clone()),
                    _ => None,
                })
                .collect();
            for kind in kinds {
                if seen.contains(&kind) {
                    continue;
                }
                seen.push(kind.clone());
                if !kind.is_list() {
                    press(&mut s, &g, &Do::Menu(kind.clone()));
                    assert!(open_pick(&s).is_none(), "{kind:?} is a menu of operations");
                    more = true;
                    if let Some(u) = s.settings.as_mut() {
                        u.menu = None;
                    }
                    continue;
                }
                assert_eq!(settings_counted(&mut s, &g, &kind), 4, "{kind:?}");
            }
            s.close_settings();
        }
    }
    let has = |f: &dyn Fn(&MenuKind) -> bool| seen.iter().any(f);
    assert!(more, "「その他」 is on the screen");
    assert!(has(&|k| matches!(k, MenuKind::Pref(_))));
    assert!(has(&|k| matches!(k, MenuKind::Preset)));
    assert!(has(&|k| matches!(k, MenuKind::MouseCell { .. })));
    assert!(seen.len() > 6, "{seen:?}");
}

/// With no audio device, the device list says 「オーディオデバイスなし」
/// with its alert, picks nothing and never puts the cursor on it (UI-338).
#[test]
fn the_device_list_with_no_device_says_so() {
    let dir = temp("no-device");
    let (mut s, g) = launch(dir.path());
    s.open_settings(Section::Audio);
    s.song.io.lock().unwrap().view.devices.clear();
    press(&mut s, &g, &Do::Menu(MenuKind::Pref("audio.device".into())));
    let (pick, cells) = open_pick(&s).expect("the device list");
    assert_eq!(pick.nav().len(), 1);
    let row = cells
        .iter()
        .find(|c| c.text == crate::start::rig::NO_DEVICE)
        .expect("the row");
    assert_eq!(row.icon, Some("alert"));
    assert!(!row.live() && !row.cursor && !row.locked);
    s.settings_key("down", none());
    s.settings_key("enter", none());
    let (_, cells) = open_pick(&s).expect("still open: nothing was picked");
    assert!(cells.iter().all(|c| !c.cursor));
}
