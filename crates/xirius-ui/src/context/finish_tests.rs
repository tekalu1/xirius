use super::roll_tests::{note_at, notes, roll};
use super::tests::{BAR, choose, labels, lane_at, studio};
use crate::arrange::probe::track;
use crate::tools::{Mods, Tool};
use serde_json::json;

#[test]
fn middle_click_opens_tools_and_middle_drag_only_scrolls() {
    let (mut s, g) = studio();
    let (x, y) = lane_at(&s, &g, "Keys", 13);
    let n = s.song.steps();
    assert!(s.middle_down(&g, x, y));
    assert!(s.middle_up(&g));
    assert!(labels(&s).contains(&"消しゴムツール".to_string()));
    assert!(choose(&mut s, &g, "消しゴムツール"));
    assert_eq!(s.tool, Tool::Eraser);
    assert_eq!(s.song.steps(), n);
    assert!(s.middle_down(&g, x, y));
    assert!(s.pan_move(&g, x - 20.0, y));
    assert!(s.middle_up(&g));
    assert!(!s.context_open());
}

#[test]
fn all_four_note_values_are_entered_from_the_menu_and_undo_once() {
    let (mut s, g) = roll();
    for (label, value) in [
        ("位置…", 1000),
        ("長さ…", 720),
        ("ピッチ…", 65),
        ("ベロシティ…", 77),
    ] {
        let before = notes(&s, &g);
        let (x, y) = note_at(&s, &g, 0);
        assert!(s.scene_context(&g, x, y));
        assert!(choose(&mut s, &g, "値を入力"));
        assert!(choose(&mut s, &g, label));
        for _ in 0..20 {
            s.ask_key("backspace");
        }
        s.ask_text(None, &value.to_string(), false);
        s.ask_key("enter");
        assert!(!s.context_open());
        assert_ne!(notes(&s, &g), before);
        if label == "ベロシティ…" {
            assert_eq!(s.roll.next_vel, 77);
        }
        s.undo(&g);
        assert_eq!(notes(&s, &g), before);
    }
    let (x, y) = note_at(&s, &g, 0);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "消しゴムツール"));
    assert_eq!(s.tool, Tool::Eraser);
}

#[test]
fn delete_uses_the_tempo_selection_and_protects_the_first_point() {
    let (mut s, g) = studio();
    s.song
        .run("tempo.set", json!({"at":2*BAR,"bpm":90}))
        .unwrap();
    let (x, y) = s.tempo_xy(&g, 2 * BAR, 90.0);
    s.browser.typing = true;
    s.input_map.browser_focus = true;
    assert!(s.scene_context(&g, x, y));
    s.popup = None;
    assert!(!s.browser.typing);
    assert!(!s.input_map.browser_focus);
    assert!(s.arrange_down(&g, x, y, Mods::NONE));
    s.arrange_up(&g, x, y, Mods::NONE);
    let clips = s
        .project()
        .tracks
        .values()
        .map(|t| t.clips.len())
        .sum::<usize>();
    assert!(s.mapped_key(&g, "delete", None, Mods::NONE));
    assert_eq!(s.project().tempo.len(), 1);
    assert_eq!(
        s.project()
            .tracks
            .values()
            .map(|t| t.clips.len())
            .sum::<usize>(),
        clips
    );
    s.undo(&g);
    assert_eq!(s.project().tempo.len(), 2);
    let (x, y) = s.tempo_xy(&g, 0, 120.0);
    s.arrange_down(&g, x, y, Mods::NONE);
    s.arrange_up(&g, x, y, Mods::NONE);
    assert!(s.mapped_key(&g, "delete", None, Mods::NONE));
    assert_eq!(s.project().tempo.len(), 2);
}

#[test]
fn take_rows_use_whole_takes_and_cleanup_is_a_single_undo() {
    let (mut s, g) = studio();
    // Undo here must land exactly on the state the test kept; the
    // instrument-input arm that would follow the selection is not
    // what this test checks (REC-024).
    s.rec_set(
        "prefs.set",
        serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
    );
    let t = track(&s, "Vocal");
    s.song
        .run(
            "record.take.add",
            json!({"takes":[
                {"track":t.to_string(),"start":2*BAR,"length":2*BAR,"file":"a.wav","name":"Take 1"},
                {"track":t.to_string(),"start":2*BAR,"length":2*BAR,"file":"b.wav","name":"Take 2"}
            ]}),
        )
        .unwrap();
    s.rebuild(&g);
    let p = s.project();
    let id = p.tracks[&t]
        .clips
        .iter()
        .find(|c| c.name == "Take 1")
        .unwrap()
        .id;
    for label in [
        "このテイクを使用",
        "未使用のテイクをすべて削除",
        "テイクを削除",
    ] {
        let before = s.project();
        assert!(s.take_context_over(id, None, 400.0, 400.0));
        assert!(choose(&mut s, &g, label));
        assert_ne!(s.project(), before);
        s.undo(&g);
        assert_eq!(s.project(), before);
    }
}

#[test]
fn the_menu_of_a_take_row_uses_the_take_over_a_stretch_and_unpacks_it() {
    let (mut s, g) = studio();
    // Undo here must land exactly on the state the test kept; the
    // instrument-input arm that would follow the selection is not
    // what this test checks (REC-024).
    s.rec_set(
        "prefs.set",
        serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
    );
    let t = track(&s, "Vocal");
    s.song
        .run(
            "record.take.add",
            json!({"takes":[
                {"track":t.to_string(),"start":2*BAR,"length":2*BAR,"file":"a.wav","name":"Take 1"},
                {"track":t.to_string(),"start":2*BAR,"length":2*BAR,"file":"b.wav","name":"Take 2"}
            ]}),
        )
        .unwrap();
    s.rebuild(&g);
    let id = s.project().tracks[&t]
        .clips
        .iter()
        .find(|c| c.name == "Take 1")
        .unwrap()
        .id;
    let bar = Some((2 * BAR, 3 * BAR));
    assert!(s.take_context_over(id, bar, 400.0, 400.0));
    assert_eq!(
        labels(&s),
        [
            "このテイクを使用",
            "選択範囲にこのテイクを使用",
            "テイクを新しいトラックに展開",
            "最後の周を取り消す",
            "未使用のテイクをすべて削除",
            "テイクを削除",
        ]
    );
    // Without a stretch under the pointer or chosen, the row says why it cannot run.
    assert!(s.take_context_over(id, None, 400.0, 400.0));
    assert!(!super::tests::enabled(&s, "選択範囲にこのテイクを使用"));
    for label in [
        "選択範囲にこのテイクを使用",
        "テイクを新しいトラックに展開",
        "最後の周を取り消す",
    ] {
        let before = s.project();
        assert!(s.take_context_over(id, bar, 400.0, 400.0));
        assert!(choose(&mut s, &g, label), "{label}: {:?}", s.error);
        assert_ne!(s.project(), before, "{label}");
        s.undo(&g);
        assert_eq!(s.project(), before, "{label}");
    }
    // The stretch chosen with the keys is the selection, not the bar under the pointer.
    s.comp_sel = Some(crate::rec_area::CompSel {
        track: t,
        from: 2 * BAR,
        to: 2 * BAR + BAR / 2,
    });
    assert!(s.take_context_over(id, bar, 400.0, 400.0));
    assert!(choose(&mut s, &g, "選択範囲にこのテイクを使用"));
    let at = |tick| {
        s.project().tracks[&t]
            .clips
            .iter()
            .find(|c| !c.muted && c.start <= tick && tick < c.start + c.length)
            .map(|c| c.name.clone())
    };
    assert_eq!(at(2 * BAR + 10).as_deref(), Some("Take 1"));
    assert_eq!(at(2 * BAR + BAR / 2 + 10).as_deref(), Some("Take 2"));
}

#[test]
fn second_level_contains_no_more_submenus() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let y = crate::arrange::probe::row_at(&s, &g, crate::tree::RowKind::Track(t), 0.5);
    assert!(s.scene_context(&g, g.column.x + 50.0, y));
    assert!(choose(&mut s, &g, "オートメーションを表示"));
    assert!(s.context_rows().iter().all(|r| !r.sub));
}

#[test]
fn long_menus_filter_without_changing_the_actions() {
    let (mut s, g) = studio();
    let rows = (0..20)
        .map(|n| s.ctx_as(&format!("Marker {n}"), "marker.add", json!({"at":n*BAR})))
        .collect();
    s.open_context(rows, 400.0, 300.0);
    assert!(s.context_query().is_some());
    assert_eq!(
        s.popup.as_ref().unwrap().layout(g.size).cells[0].icon,
        Some("search")
    );
    s.ask_text(None, "Marker 17", false);
    assert_eq!(labels(&s), ["Marker 17"]);
    assert!(super::tests::enabled(&s, "Marker 17"));
    assert!(choose(&mut s, &g, "Marker 17"), "{:?}", s.error);
    assert_eq!(s.project().markers[0].tick, 17 * BAR);
    s.undo(&g);
    assert!(s.project().markers.is_empty());
}

#[test]
fn ruler_units_are_view_state_and_selected_export_does_not_change_the_loop() {
    let (mut s, g) = studio();
    let x = s.tick_x(&g, 2 * BAR) + 2.0;
    assert!(s.scene_context(&g, x, g.ruler.bottom() - 3.0));
    assert!(choose(&mut s, &g, "目盛り"));
    assert!(choose(&mut s, &g, "分と秒"));
    assert!(s.song.session().state.view.ruler_seconds);
    let span = s.project().loop_range;
    let (x, y) = crate::arrange::probe::clip_mid(&s, &g, "Vocal", 0);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "選択範囲を書き出す…"));
    assert_eq!(s.export.range, crate::export::Range::Selection);
    assert_eq!(s.project().loop_range, span);
}
