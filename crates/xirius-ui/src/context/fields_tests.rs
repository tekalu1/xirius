//! The fields at the head of an audio clip's menu (mock 01 and 03 of the audio processing),
//! pressed and typed as the window does it.

use serde_json::json;
use xirius_core::model::{AudioProcess, ClipBody};

use super::fields::Kind;
use super::tests::{labels, steps, studio};
use crate::arrange::probe::{clip_mid, track};
use crate::popup::{Action, Look};
use crate::studio::{Geometry, Hit, Studio};

fn open(s: &mut Studio, g: &Geometry, i: usize) {
    let (x, y) = clip_mid(s, g, "Vocal", i);
    assert!(s.scene_context(g, x, y));
}

/// Opens the menu of the `i`-th clip of `name`.
fn press_on(s: &mut Studio, g: &Geometry, name: &str, i: usize) -> bool {
    s.popup = None;
    let (x, y) = clip_mid(s, g, name, i);
    s.scene_context(g, x, y)
}

fn process(s: &Studio, i: usize) -> AudioProcess {
    let t = track(s, "Vocal");
    match &s.project().tracks[&t].clips[i].body {
        ClipBody::Audio { process, .. } => *process,
        _ => unreachable!(),
    }
}

/// What the field of `kind` says.
fn said(s: &Studio, kind: Kind) -> String {
    let row = s
        .context_rows()
        .into_iter()
        .find_map(|r| r.field.filter(|f| f.kind == kind))
        .expect("the menu has the field");
    row.text
}

/// Presses the cell of the open menu whose action is `action`, through the window's hit test.
fn press(s: &mut Studio, g: &Geometry, action: &Action) -> bool {
    let layout = s.popup.as_ref().expect("a menu is open").layout(g.size);
    let cell = layout
        .cells
        .iter()
        .find(|c| c.action.as_ref() == Some(action))
        .unwrap_or_else(|| panic!("no cell for {action:?}"));
    let (x, y) = (
        cell.rect.x + cell.rect.w / 2.0,
        cell.rect.y + cell.rect.h / 2.0,
    );
    let hit = s.hit(g, x, y);
    assert!(matches!(hit, Hit::Item(_)), "{hit:?}");
    s.press(g, hit)
}

/// Presses the field of `kind`.
fn focus(s: &mut Studio, g: &Geometry, kind: Kind) {
    let i = s
        .context_rows()
        .iter()
        .position(|r| r.field.as_ref().is_some_and(|f| f.kind == kind))
        .expect("the field");
    assert!(press(
        s,
        g,
        &Action::Field(i, super::fields::FieldAct::Focus)
    ));
}

fn type_in(s: &mut Studio, text: &str) {
    s.ask_text(None, text, false);
}

#[test]
fn an_audio_clip_starts_with_its_four_fields_and_then_following_the_tempo() {
    let (mut s, g) = studio();
    open(&mut s, &g, 0);
    let rows = labels(&s);
    assert_eq!(
        rows[..6],
        [
            "テンポ",
            "速さ",
            "トランスポーズ",
            "微調整",
            "曲のテンポに合わせる",
            "クリップを分割"
        ]
    );
    assert_eq!(rows[6], "クリップ名を変更", "{rows:?}");
    assert_eq!(said(&s, Kind::Speed), "100%");
    assert_eq!(said(&s, Kind::Semis), "0");
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    let fields: Vec<_> = layout
        .cells
        .iter()
        .filter(|c| c.look == Look::Field)
        .collect();
    assert_eq!(fields.len(), 4);
    // The fields stand in one column, inside the menu.
    assert!(fields.windows(2).all(|w| w[0].rect.x == w[1].rect.x));
    assert!(
        fields
            .iter()
            .all(|c| layout.rect.contains(c.rect.x, c.rect.y))
    );
    // No unit is drawn beside a field (UI-311): the value carries 「%」, and the other units
    // are only said.
    assert!(
        !layout
            .cells
            .iter()
            .any(|c| ["BPM", "%", "半音", "セント"].contains(&c.text.as_str()))
    );
    let speed = layout
        .cells
        .iter()
        .find(|c| c.look == Look::Field && c.text == "100%")
        .expect("the speed shows its %");
    assert_eq!(speed.spoken, "速さ 100%");
    let semis = layout
        .cells
        .iter()
        .find(|c| c.look == Look::Field && c.spoken.starts_with("トランスポーズ"))
        .expect("the transpose field");
    assert_eq!(semis.spoken, "トランスポーズ 0 半音");
}

#[test]
fn a_typed_value_is_one_step_and_the_menu_stays_with_the_field_showing_it() {
    let (mut s, g) = studio();
    open(&mut s, &g, 0);
    focus(&mut s, &g, Kind::Tempo);
    let n = steps(&s);
    type_in(&mut s, "9");
    type_in(&mut s, "3.5");
    assert_eq!(said(&s, Kind::Tempo), "93.5");
    // The search box did not get the digits.
    assert_eq!(s.context_query().unwrap_or_default(), "");
    assert_eq!(s.field_key(&g, "enter", None, false), Some(true));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(process(&s, 0).tempo, Some(93.5));
    assert!(s.context_open(), "the menu stays");
    assert_eq!(said(&s, Kind::Tempo), "93.50");
    assert!(s.field_typing().is_some_and(|t| t.fresh));
    // What the window types after a key (a simulated Enter's 「\n」, a space) is not the field's,
    // and leaves the value shown.
    s.ask_text(None, "\n", false);
    s.ask_text(None, " ", false);
    assert_eq!(said(&s, Kind::Tempo), "93.50");
    assert!(s.field_typing().is_some_and(|t| t.fresh));
    assert_eq!(s.context_query().unwrap_or_default(), "");

    // Tab goes on to the speed; past the range is the end of it.
    assert_eq!(s.field_key(&g, "tab", None, false), Some(true));
    type_in(&mut s, "300");
    s.field_key(&g, "enter", None, false);
    assert_eq!(process(&s, 0).speed, AudioProcess::MAX_SPEED);
    assert_eq!(said(&s, Kind::Speed), "200%");
    assert_eq!(steps(&s), n + 2);

    // A minus types a negative fine tune.
    focus(&mut s, &g, Kind::Cents);
    type_in(&mut s, "−");
    type_in(&mut s, "5");
    assert_eq!(said(&s, Kind::Cents), "−5");
    s.field_key(&g, "enter", None, false);
    assert_eq!(process(&s, 0).cents, -5);

    // Each change is a step of its own.
    s.undo(&g);
    assert_eq!(process(&s, 0).cents, 0);
    assert_eq!(process(&s, 0).speed, AudioProcess::MAX_SPEED);
}

#[test]
fn the_arrows_change_the_transpose_at_once_and_shift_by_an_octave() {
    let (mut s, g) = studio();
    open(&mut s, &g, 0);
    focus(&mut s, &g, Kind::Semis);
    let n = steps(&s);
    s.field_key(&g, "up", None, true);
    assert_eq!(process(&s, 0).semis, 12);
    s.field_key(&g, "down", None, false);
    assert_eq!(process(&s, 0).semis, 11);
    assert_eq!(said(&s, Kind::Semis), "+11");
    assert_eq!(steps(&s), n + 2);
    // Enter with nothing typed changes nothing.
    s.field_key(&g, "enter", None, false);
    assert_eq!(steps(&s), n + 2);
}

#[test]
fn clips_that_differ_show_a_dash_and_a_value_goes_to_all_of_them() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let first = s.project().tracks[&vocal].clips[0].clone();
    let made = s
        .arrange_run(
            "clip.add.audio",
            json!({ "track": vocal.to_string(), "at": first.end(), "length": first.length,
                    "file": "vocal_take2.wav" }),
        )
        .expect("a second clip");
    let second = made["id"]
        .as_str()
        .or_else(|| made["ids"][0].as_str())
        .expect("its id")
        .to_string();
    s.arrange_run("clip.transpose", json!({ "ids": [second], "semis": 2 }));
    s.arrange_run(
        "clip.transpose",
        json!({ "ids": [first.id.to_string()], "semis": 0 }),
    );
    s.rebuild(&g);
    s.arrange.clips = s.project().tracks[&vocal]
        .clips
        .iter()
        .map(|c| (vocal, c.id))
        .collect();
    open(&mut s, &g, 0);
    assert_eq!(said(&s, Kind::Semis), "—");
    assert_eq!(said(&s, Kind::Speed), "100%");
    // ↑ with Shift adds an octave to each.
    focus(&mut s, &g, Kind::Semis);
    s.field_key(&g, "up", None, true);
    assert_eq!((process(&s, 0).semis, process(&s, 1).semis), (12, 14));
    assert_eq!(said(&s, Kind::Semis), "—");
    // A typed value goes to both.
    type_in(&mut s, "5");
    s.field_key(&g, "enter", None, false);
    assert_eq!((process(&s, 0).semis, process(&s, 1).semis), (5, 5));
    assert_eq!(said(&s, Kind::Semis), "+5");
}

#[test]
fn doubling_and_halving_the_tempo_are_buttons_that_wait_for_a_known_tempo() {
    let (mut s, g) = studio();
    open(&mut s, &g, 0);
    let double = |s: &Studio| {
        let layout = s.popup.as_ref().unwrap().layout(g.size);
        layout
            .cells
            .iter()
            .find(|c| c.text == "×2")
            .map(|c| c.live())
            .unwrap()
    };
    if process(&s, 0).tempo.is_none() {
        assert_eq!(said(&s, Kind::Tempo), "—");
        assert!(!double(&s));
        s.popup = None;
        let t = track(&s, "Vocal");
        let id = s.project().tracks[&t].clips[0].id.to_string();
        s.arrange_run("clip.tempo.set", json!({ "ids": [id], "bpm": 120 }));
        open(&mut s, &g, 0);
    }
    assert!(double(&s));
    let i = s
        .context_rows()
        .iter()
        .position(|r| r.label == "テンポ")
        .unwrap();
    assert!(press(
        &mut s,
        &g,
        &Action::Field(i, super::fields::FieldAct::Double)
    ));
    assert_eq!(process(&s, 0).tempo, Some(240.0));
    assert_eq!(said(&s, Kind::Tempo), "240.00");
    assert!(press(
        &mut s,
        &g,
        &Action::Field(i, super::fields::FieldAct::Halve)
    ));
    assert_eq!(process(&s, 0).tempo, Some(120.0));
}

#[test]
fn a_frozen_track_keeps_the_fields_dim_with_the_reason() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    s.arrange_run(
        "track.freeze.apply",
        json!({ "tracks": [{ "id": t.to_string(), "file": "frozen.wav", "start": 0 }] }),
    );
    open(&mut s, &g, 0);
    let row = s
        .context_rows()
        .into_iter()
        .find(|r| r.label == "速さ")
        .unwrap();
    assert!(!row.enabled);
    assert_eq!(row.reason, crate::toast::FROZEN);
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    assert!(
        layout
            .cells
            .iter()
            .filter(|c| c.look == Look::Field)
            .all(|c| !c.live())
    );
}

#[test]
fn the_search_box_takes_the_keys_and_the_label_gives_them_back() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    let click = |s: &mut Studio, cell: &crate::popup::Cell| {
        let hit = s.hit(&g, cell.rect.x + 5.0, cell.rect.y + cell.rect.h / 2.0);
        assert!(matches!(hit, Hit::Item(_)), "{hit:?}");
        s.press(&g, hit)
    };
    let search = layout
        .cells
        .iter()
        .find(|c| c.icon == Some("search"))
        .expect("the search box")
        .clone();
    assert!(!search.on, "the field has the keys, not the search box");
    assert!(click(&mut s, &search));
    assert!(s.field_typing().is_none(), "the search box has the keys");
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    assert!(
        layout
            .cells
            .iter()
            .find(|c| c.icon == Some("search"))
            .is_some_and(|c| c.on),
        "the caret is the search box's now"
    );
    let label = layout
        .cells
        .iter()
        .find(|c| c.look == Look::Note && c.text == "トランスポーズ")
        .expect("the field's label")
        .clone();
    assert!(click(&mut s, &label));
    assert!(
        s.field_typing().is_some_and(|t| t.row == 0),
        "the label gives the field the keys"
    );
}

fn semis(s: &Studio) -> i8 {
    s.project().tracks[&track(s, "Keys")].clips[0].transpose()
}

#[test]
fn the_menu_opens_with_the_first_field_taking_the_keys_and_the_ime_writes_there() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    // The field takes the keys as the menu opens, so the caret is already there (UI-313).
    assert!(s.field_typing().is_some_and(|t| t.row == 0 && t.fresh));
    // The IME composes 「５」: the field keeps it, and the search box keeps none of it.
    s.ask_text(None, "５", true);
    let t = s.field_typing().expect("the field");
    assert_eq!(t.marked, "5");
    assert!(
        s.ctx
            .as_ref()
            .unwrap()
            .search
            .as_ref()
            .unwrap()
            .field
            .marked
            .is_empty()
    );
    // The commit is the field's, and Enter sets the clip's value.
    s.ask_text(None, "５", false);
    assert_eq!(s.field_key(&g, "enter", None, false), Some(true));
    assert_eq!(semis(&s), 5);
    assert!(
        s.ctx
            .as_ref()
            .unwrap()
            .search
            .as_ref()
            .unwrap()
            .field
            .marked
            .is_empty()
    );
}

#[test]
fn a_digit_and_enter_change_the_field_as_soon_as_the_menu_opens() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    s.ask_text(None, "3", false);
    assert_eq!(s.field_key(&g, "enter", None, false), Some(true));
    assert_eq!(semis(&s), 3);
}

#[test]
fn escape_shuts_the_menu_when_nothing_is_typed_and_only_drops_the_typing_otherwise() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert!(
        s.field_typing()
            .is_some_and(|t| t.fresh && t.marked.is_empty())
    );
    assert!(s.field_keys(&g, "escape", None, crate::tools::Mods::NONE));
    assert!(!s.context_open(), "nothing typed: Esc shuts the menu");

    assert!(press_on(&mut s, &g, "Keys", 0));
    s.ask_text(None, "5", false);
    assert!(s.field_keys(&g, "escape", None, crate::tools::Mods::NONE));
    assert!(s.context_open(), "half typed: the menu stays");
    assert!(s.field_typing().is_none());
    assert_eq!(semis(&s), 0, "nothing was written");

    assert!(press_on(&mut s, &g, "Keys", 0));
    s.ask_text(None, "2", true);
    assert!(s.field_keys(&g, "escape", None, crate::tools::Mods::NONE));
    assert!(s.context_open(), "composing: the IME has the key");
    assert!(s.field_typing().is_some_and(|t| t.marked == "2"));
}

#[test]
fn the_ime_keeps_enter_backspace_and_the_arrows_while_it_composes() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    s.ask_text(None, "2", true);
    assert_eq!(s.field_key(&g, "enter", None, false), Some(true));
    assert_eq!(semis(&s), 0, "Enter did not take the composition");
    assert!(s.field_typing().is_some_and(|t| t.marked == "2"));
    // The arrow and Backspace are the composition's too.
    assert_eq!(s.field_key(&g, "up", None, false), Some(true));
    assert_eq!(s.field_key(&g, "backspace", None, false), Some(true));
    assert!(s.field_typing().is_some_and(|t| t.marked == "2"));
    // An empty composition takes the field's marked text away.
    s.ask_text(None, "", true);
    assert!(s.field_typing().is_some_and(|t| t.marked.is_empty()));
    // The commit is what sets the value, and full-width digits count as their half-width.
    s.ask_text(None, "１２", true);
    assert!(s.field_typing().is_some_and(|t| t.marked == "12"));
    s.ask_text(None, "１２", false);
    assert_eq!(s.field_key(&g, "enter", None, false), Some(true));
    assert_eq!(semis(&s), 12);
}

#[test]
fn a_letter_leaves_the_field_and_filters_the_search_box() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert!(s.field_typing().is_some());
    s.ask_text(None, "く", false);
    assert_eq!(s.context_query().as_deref(), Some("く"));
    assert!(s.field_typing().is_none(), "the search box has the keys");
    assert_eq!(semis(&s), 0);
}

fn field_cell(s: &Studio, g: &Geometry, kind: Kind) -> (f32, f32) {
    let layout = s.popup.as_ref().expect("a menu is open").layout(g.size);
    let cell = layout
        .cells
        .iter()
        .find(|c| c.look == Look::Field && c.spoken.starts_with(kind.label()))
        .unwrap_or_else(|| panic!("the field of {kind:?}"));
    (
        cell.rect.x + cell.rect.w / 2.0,
        cell.rect.y + cell.rect.h / 2.0,
    )
}

#[test]
fn a_wheel_over_a_menu_field_changes_it_and_does_not_move_the_list() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    let offset = s.popup.as_ref().unwrap().offset;
    // Fixed times: the whole turn is one undo step whatever the machine's load (P3-4 of review
    // 15).
    let mut now = std::time::Instant::now();
    let mut turn = |s: &mut Studio, px: f32| {
        now += std::time::Duration::from_millis(10);
        s.context_field_wheel_at(&g, 0, px, false, false, now)
    };
    assert!(turn(&mut s, 60.0));
    assert_eq!(semis(&s), 1);
    assert_eq!(s.popup.as_ref().unwrap().offset, offset, "the list stayed");
    assert!(s.field_typing().is_some(), "the field keeps the keys");
    // A touchpad's small turns add up.
    for _ in 0..3 {
        assert!(turn(&mut s, 20.0));
    }
    assert_eq!(semis(&s), 2);
    // Turning back throws away what was gathered the other way.
    assert!(turn(&mut s, 20.0));
    assert!(turn(&mut s, -20.0));
    assert_eq!(semis(&s), 2);
    assert!(turn(&mut s, -20.0));
    assert!(turn(&mut s, -20.0));
    assert_eq!(semis(&s), 1);
    // One undo takes the whole turn back (UI-313).
    s.undo(&g);
    assert_eq!(semis(&s), 0);
}

#[test]
fn an_audio_menu_opens_on_the_transpose_not_the_tempo() {
    let (mut s, g) = studio();
    open(&mut s, &g, 0);
    let t = s.field_typing().expect("a field takes the keys");
    assert_eq!(t.row, 2, "the transpose, after the tempo and the speed");
    assert_eq!(s.context_rows()[2].label, "トランスポーズ");
}

#[test]
fn a_turn_left_in_one_field_does_not_move_the_next_field() {
    let (mut s, g) = studio();
    open(&mut s, &g, 0);
    let mut now = std::time::Instant::now();
    // 40 px in the transpose leaves a fraction there. Moving to the fine tune starts its own
    // turn: its 20 px is not the rest of the transpose's notch (P3-1 of review 15).
    now += std::time::Duration::from_millis(10);
    assert!(s.context_field_wheel_at(&g, 2, 40.0, false, false, now));
    now += std::time::Duration::from_millis(10);
    assert!(s.context_field_wheel_at(&g, 3, 20.0, false, false, now));
    let p = process(&s, 0);
    assert_eq!((p.semis, p.cents), (0, 0));
}

#[test]
fn shift_and_ctrl_change_how_far_a_menu_wheel_notch_goes() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    let at = field_cell(&s, &g, Kind::Semis);
    // Windows sends Shift + wheel sideways, 20 px for one character: still one notch.
    assert!(s.window_wheel(&g, at, 20.0, 0.0, crate::tools::Mods::shift(), false));
    assert_eq!(semis(&s), 12);

    let (mut s, g) = studio();
    open(&mut s, &g, 0);
    let at = field_cell(&s, &g, Kind::Speed);
    assert!(s.window_wheel(
        &g,
        at,
        0.0,
        60.0,
        crate::tools::Mods {
            ctrl: true,
            ..crate::tools::Mods::NONE
        },
        true
    ));
    assert_eq!(process(&s, 0).speed, 100.1);
}

#[test]
fn the_label_scrolls_the_list_and_a_list_already_moving_keeps_moving_over_the_field() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    let at = field_cell(&s, &g, Kind::Semis);
    // The label is not the value: the wheel leaves the value alone.
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    let label = layout
        .cells
        .iter()
        .find(|c| c.look == Look::Note && c.text == "トランスポーズ")
        .unwrap()
        .rect;
    assert!(s.window_wheel(
        &g,
        (label.x + 5.0, label.y + label.h / 2.0),
        0.0,
        60.0,
        crate::tools::Mods::NONE,
        false
    ));
    assert_eq!(semis(&s), 0);
    assert!(
        s.ctx.as_ref().unwrap().scrolled_at.is_some(),
        "the list was sent, even at its end"
    );
    // The field itself takes the wheel: one notch, and the list stays where it is.
    s.ctx.as_mut().unwrap().scrolled_at = None;
    let offset = s.popup.as_ref().unwrap().offset;
    assert!(s.window_wheel(&g, at, 0.0, 60.0, crate::tools::Mods::NONE, false));
    assert_eq!(semis(&s), 1);
    assert_eq!(s.popup.as_ref().unwrap().offset, offset);
    // A list that scrolled a moment ago keeps scrolling: the field does not take the turn.
    s.ctx.as_mut().unwrap().scrolled_at = Some(std::time::Instant::now());
    assert!(s.window_wheel(&g, at, 0.0, 60.0, crate::tools::Mods::NONE, false));
    assert_eq!(semis(&s), 1);
}
