//! UI-351: deleting a draft from the start screen's list. Every case here ran red before the
//! deletion was built (there was no key, no menu, no question).

use super::tests::{NONE, centre, home, key, labels, row_spot, song_state, with_device};
use super::*;
use crate::song::Song;
use crate::tools::Mods;

/// A song to list: its name, whether it is a draft, and when it was last edited.
type Seed<'a> = (&'a str, bool, u64);

/// The start screen over these songs, oldest first (the list shows them newest first). A
/// draft's folder is one of the keeper's own; a saved song's lies beside them.
fn listed(songs: &[Seed]) -> (Studio, Geometry, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let keep = crate::keep::Keep::at(tmp.path().join("keep"));
    for (name, draft, at) in songs {
        let path = if *draft {
            tmp.path().join("keep").join("drafts").join(name)
        } else {
            tmp.path().join(name)
        };
        let mut seed = Song::empty();
        seed.save_as(&path).unwrap();
        drop(seed);
        keep.remember(&path, name, *draft, *at);
    }
    let mut s = Studio::new(Song::empty());
    with_device(&mut s, true);
    s.keep = keep;
    s.keep.sync(&s.song.project());
    s.keep.settle_recent();
    s.keep.poll_recent();
    s.home = Some(Home::default());
    s.home_tick();
    let g = s.geometry(1280.0, 720.0);
    (s, g, tmp)
}

fn dir_of(tmp: &tempfile::TempDir, name: &str) -> PathBuf {
    tmp.path().join("keep").join("drafts").join(name)
}

/// The worker answers, the keeper's list follows, and the screen is built again.
fn settle(s: &mut Studio) {
    for _ in 0..1000 {
        s.home_tick();
        if s.draft_removal.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    s.keep.settle_recent();
    s.keep.poll_recent();
    s.home_tick();
}

fn select(s: &mut Studio, row: usize, focus: Focus) {
    let h = s.home.as_mut().unwrap();
    h.pick.selected = row;
    h.focus = focus;
    h.touched = true;
}

fn chosen_label(s: &Studio) -> String {
    labels(s)[home(s).pick.selected].clone()
}

/// Asks about the cursor's draft with the Delete key and says 「削除」.
fn delete_chosen(s: &mut Studio, g: &Geometry) {
    assert!(key(s, g, "delete", NONE, None));
    assert!(s.unsaved.is_some(), "it asks first");
    assert!(s.unsaved_key("tab"));
    assert!(s.unsaved_key("enter"));
    settle(s);
}

#[test]
fn delete_on_a_draft_in_the_list_asks_and_removes_nothing_yet() {
    let (mut s, g, tmp) = listed(&[("old", false, 1), ("d1", true, 2)]);
    assert_eq!(labels(&s)[0], "d1（保存前）");
    select(&mut s, 0, Focus::List);
    assert!(key(&mut s, &g, "delete", NONE, None));
    assert!(s.unsaved.is_some());
    assert!(dir_of(&tmp, "d1").exists());
    assert!(s.draft_removal.is_none());
}

#[test]
fn delete_does_nothing_on_a_saved_song_the_sample_or_a_button() {
    let (mut s, g, tmp) = listed(&[("old", false, 1), ("d1", true, 2)]);
    let before = song_state(&s);
    // A saved song and the sample.
    for row in [1, 2] {
        select(&mut s, row, Focus::List);
        assert!(key(&mut s, &g, "delete", NONE, None), "the list's key");
        assert!(s.unsaved.is_none(), "row {row} asks nothing");
        assert!(s.popup.is_none());
    }
    // A button has the keys: Delete stays what it was.
    s.home.as_mut().unwrap().focus = Focus::Button(Button::New);
    key(&mut s, &g, "delete", NONE, None);
    assert!(s.unsaved.is_none());
    assert_eq!(song_state(&s), before);
    assert!(tmp.path().join("old").exists() && dir_of(&tmp, "d1").exists());
    settle(&mut s);
    assert_eq!(labels(&s).len(), 3);
}

#[test]
fn delete_in_the_search_stays_text_editing() {
    let (mut s, g, _tmp) = listed(&[("d1", true, 2)]);
    s.home.as_mut().unwrap().focus = Focus::Search;
    s.pick_text(None, "d", false);
    select(&mut s, 0, Focus::Search);
    assert!(
        !s.home_key(&g, "delete", NONE),
        "left to the text editing, which edits the search"
    );
    assert!(s.unsaved.is_none());
    // Backspace in the search edits it as well, even with the cursor on a draft.
    assert!(s.home_key(&g, "backspace", NONE));
    assert!(s.unsaved.is_none());
    assert_eq!(home(&s).focus, Focus::Search);
}

#[test]
fn backspace_deletes_only_when_the_search_has_no_text_and_cmd_backspace_on_a_mac() {
    let ctrl = Mods { ctrl: true, ..NONE };
    assert!(delete::is_delete("delete", NONE, false, false));
    assert!(delete::is_delete("backspace", NONE, true, false));
    assert!(!delete::is_delete("backspace", NONE, false, false));
    assert!(!delete::is_delete("backspace", ctrl, true, false));
    assert!(delete::is_delete("backspace", ctrl, false, true));
    assert!(!delete::is_delete("delete", ctrl, true, true));
    assert!(!delete::is_delete("a", NONE, true, true));
}

#[test]
fn a_right_press_on_a_draft_row_opens_the_one_row_menu_and_chooses_the_row() {
    let (mut s, g, tmp) = listed(&[("old", false, 1), ("d1", true, 2)]);
    select(&mut s, 1, Focus::Button(Button::New));
    let at = centre(
        s.home_layout(&g)
            .unwrap()
            .rect(row_spot(&s, &g, 0))
            .unwrap(),
    );
    assert!(s.window_right_press(&g, at.0, at.1));
    assert!(s.context_open());
    let rows = s.context_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label, "削除");
    assert_eq!(rows[0].key, "Del");
    assert_eq!((home(&s).pick.selected, home(&s).focus), (0, Focus::List));
    // Choosing it asks; nothing goes yet.
    assert!(s.context_pick(&g, 0));
    assert!(s.unsaved.is_some());
    assert!(dir_of(&tmp, "d1").exists());
}

#[test]
fn a_right_press_on_a_saved_song_the_sample_or_nothing_opens_no_menu_and_goes_no_further() {
    let (mut s, g, _tmp) = listed(&[("old", false, 1), ("d1", true, 2)]);
    for row in [1, 2] {
        let at = centre(
            s.home_layout(&g)
                .unwrap()
                .rect(row_spot(&s, &g, row))
                .unwrap(),
        );
        assert!(!s.window_right_press(&g, at.0, at.1), "row {row}");
        assert!(s.popup.is_none() && s.ctx.is_none(), "row {row}");
    }
    let empty = (g.size.0 - 10.0, g.size.1 - 10.0);
    assert!(!s.window_right_press(&g, empty.0, empty.1));
    assert!(s.popup.is_none());
}

#[test]
fn the_menu_key_and_shift_f10_open_the_draft_rows_menu_and_no_other() {
    let shift = Mods {
        shift: true,
        ..NONE
    };
    let (mut s, g, _tmp) = listed(&[("old", false, 1), ("d1", true, 2)]);
    select(&mut s, 0, Focus::List);
    assert!(key(&mut s, &g, "menu", NONE, None));
    assert!(s.context_open());
    assert_eq!(s.context_rows()[0].label, "削除");
    s.popup = None;
    s.ctx = None;
    assert!(key(&mut s, &g, "f10", shift, None));
    assert!(s.context_open());
    s.popup = None;
    s.ctx = None;
    // A plain F10, or the menu key with Shift, is not the menu.
    assert!(!key(&mut s, &g, "f10", NONE, None) || !s.context_open());
    assert!(!s.context_open());
    // A saved song's row and the sample have no menu; the key goes no further.
    for row in [1, 2] {
        select(&mut s, row, Focus::List);
        assert!(key(&mut s, &g, "menu", NONE, None));
        assert!(key(&mut s, &g, "f10", shift, None));
        assert!(!s.context_open(), "row {row}");
    }
    // Nor from the search, where the keys are text.
    select(&mut s, 0, Focus::Search);
    assert!(!s.home_key(&g, "menu", NONE));
    assert!(!s.context_open());
}

#[test]
fn the_question_starts_on_back_and_enter_only_goes_back() {
    let (mut s, g, tmp) = listed(&[("d1", true, 2)]);
    select(&mut s, 0, Focus::List);
    assert!(key(&mut s, &g, "delete", NONE, None));
    // Enter again and again: the first goes back, and nothing is deleted.
    assert!(s.unsaved_key("enter"));
    assert!(s.unsaved.is_none());
    settle(&mut s);
    assert!(dir_of(&tmp, "d1").exists());
    assert_eq!(labels(&s).len(), 2);
    // Delete asks again; Esc goes back.
    assert!(key(&mut s, &g, "delete", NONE, None));
    assert!(s.unsaved_key("escape"));
    assert!(s.unsaved.is_none());
    settle(&mut s);
    assert!(dir_of(&tmp, "d1").exists());
    // Tab to 「削除」 and Enter deletes; Tab twice is back on 「戻る」.
    assert!(key(&mut s, &g, "delete", NONE, None));
    assert!(s.unsaved_key("tab") && s.unsaved_key("tab"));
    assert!(s.unsaved_key("enter"));
    settle(&mut s);
    assert!(dir_of(&tmp, "d1").exists());
    assert!(key(&mut s, &g, "delete", NONE, None));
    assert!(s.unsaved_key("right"));
    assert!(s.unsaved_key("enter"));
    settle(&mut s);
    assert!(!dir_of(&tmp, "d1").exists());
    assert_eq!(labels(&s), [crate::song::SAMPLE_NAME]);
}

#[test]
fn the_question_names_the_song_and_says_when_it_was_edited() {
    let now = xirius_core::store::now_ms();
    let (mut s, g, _tmp) = listed(&[
        ("d2", true, now.saturating_sub(5 * 60_000)),
        ("d1", true, now),
    ]);
    select(&mut s, 0, Focus::List);
    assert!(key(&mut s, &g, "delete", NONE, None));
    let Some(ask) = crate::unsaved::draft_ask(&s) else {
        panic!("the deletion's question");
    };
    assert_eq!(ask.name, "d1");
    assert_eq!(
        crate::unsaved::delete_body(ask),
        [
            "たった今編集した保存前の曲です。".to_string(),
            "削除すると元に戻せません。".to_string()
        ]
    );
    s.unsaved_key("escape");
    select(&mut s, 1, Focus::List);
    assert!(key(&mut s, &g, "delete", NONE, None));
    let Some(ask) = crate::unsaved::draft_ask(&s) else {
        panic!("the deletion's question");
    };
    assert_eq!(
        crate::unsaved::delete_body(ask)[0],
        "5 分前に編集した保存前の曲です。"
    );
}

#[test]
fn a_long_name_is_cut_on_its_own_side_and_the_tail_stays() {
    let measure = |t: &str| crate::browser::text_width(t, xirius_design::FontSize::Lead);
    let short = crate::unsaved::delete_title("曲", 298.0, measure);
    assert_eq!(short, "「曲」を削除しますか");
    let long = "とても長い名前の曲".repeat(8);
    let cut = crate::unsaved::delete_title(&long, 298.0, measure);
    assert!(
        cut.starts_with('「') && cut.ends_with("」を削除しますか"),
        "{cut}"
    );
    assert!(cut.contains('…'), "{cut}");
    assert!(measure(&cut) <= 298.0, "{cut}");
}

#[test]
fn the_question_takes_every_key_and_press_from_the_screen_and_the_song() {
    let (mut s, g, tmp) = listed(&[("a", false, 1), ("d1", true, 2)]);
    select(&mut s, 0, Focus::List);
    let before = song_state(&s);
    assert!(key(&mut s, &g, "delete", NONE, None));
    assert!(s.input_blocked());
    // Keys of the list, of a draft's menu and of the song reach nothing.
    for k in ["down", "up", "end", "menu", "delete", "space", "z", "a"] {
        assert!(s.mapped_key(&g, k, Some(k), NONE), "{k}");
        assert!(s.unsaved.is_some(), "{k} is not an answer");
    }
    assert_eq!(home(&s).pick.selected, 0);
    // A right press opens nothing; typing goes nowhere.
    let at = centre(
        s.home_layout(&g)
            .unwrap()
            .rect(row_spot(&s, &g, 0))
            .unwrap(),
    );
    assert!(!s.home_right_press(&g, at.0, at.1));
    assert!(s.home_typing().is_none());
    assert!(!s.home_key(&g, "end", NONE));
    // A press on the list's row is the question's: it goes to neither the row nor the song.
    assert!(s.unsaved_press(&g, at.0, at.1));
    assert!(s.unsaved.is_some());
    assert_eq!(home(&s).pick.selected, 0);
    assert_eq!(song_state(&s), before);
    assert!(dir_of(&tmp, "d1").exists() && tmp.path().join("a").exists());
}

#[test]
fn pressing_the_buttons_of_the_question_answers_it() {
    let (mut s, g, tmp) = listed(&[("d1", true, 2)]);
    select(&mut s, 0, Focus::List);
    assert!(key(&mut s, &g, "delete", NONE, None));
    let (back, delete) = crate::unsaved::delete_buttons(&g);
    let (x, y) = centre(back);
    assert!(s.unsaved_press(&g, x, y));
    assert!(s.unsaved.is_none());
    settle(&mut s);
    assert!(dir_of(&tmp, "d1").exists());
    assert!(key(&mut s, &g, "delete", NONE, None));
    let (x, y) = centre(delete);
    assert!(s.unsaved_press(&g, x, y));
    settle(&mut s);
    assert!(!dir_of(&tmp, "d1").exists());
}

#[test]
fn the_row_that_moves_up_is_chosen_the_focus_stays_and_nothing_is_said() {
    let (mut s, g, tmp) = listed(&[
        ("d1", true, 1),
        ("d2", true, 2),
        ("d3", true, 3),
        ("old", false, 4),
    ]);
    // old, d3, d2, d1, the sample: delete d2 (third row).
    select(&mut s, 2, Focus::List);
    assert_eq!(chosen_label(&s), "d2（保存前）");
    delete_chosen(&mut s, &g);
    assert!(!dir_of(&tmp, "d2").exists());
    assert_eq!(
        labels(&s),
        [
            "old",
            "d3（保存前）",
            "d1（保存前）",
            crate::song::SAMPLE_NAME
        ]
    );
    assert_eq!(chosen_label(&s), "d1（保存前）");
    assert_eq!(home(&s).focus, Focus::List);
    assert!(s.toast.is_none() && s.unsaved.is_none());
}

#[test]
fn deleting_the_row_that_ends_the_list_chooses_the_one_above_and_the_sample_alone_stays() {
    let (mut s, g, tmp) = listed(&[("d1", true, 1), ("d2", true, 2)]);
    // The row that takes the place is the sample's.
    select(&mut s, 1, Focus::List);
    delete_chosen(&mut s, &g);
    assert_eq!(labels(&s), ["d2（保存前）", crate::song::SAMPLE_NAME]);
    assert_eq!(chosen_label(&s), crate::song::SAMPLE_NAME);
    // With a search that hides the sample, the last row goes and the one above is chosen.
    let (mut s, g, tmp2) = listed(&[("d1", true, 1), ("d2", true, 2)]);
    s.home.as_mut().unwrap().pick.field.value = "d".into();
    s.home_tick();
    assert_eq!(labels(&s), ["d2（保存前）", "d1（保存前）"]);
    select(&mut s, 1, Focus::List);
    delete_chosen(&mut s, &g);
    assert!(!dir_of(&tmp2, "d1").exists());
    assert_eq!(labels(&s), ["d2（保存前）"]);
    assert_eq!(chosen_label(&s), "d2（保存前）");
    // Down to the sample alone, as it looks today.
    select(&mut s, 0, Focus::List);
    s.home.as_mut().unwrap().pick.field.value.clear();
    s.home_tick();
    select(&mut s, 0, Focus::List);
    delete_chosen(&mut s, &g);
    assert!(!dir_of(&tmp2, "d2").exists());
    assert_eq!(labels(&s), [crate::song::SAMPLE_NAME]);
    assert_eq!(home(&s).focus, Focus::List);
    drop(tmp);
}
#[test]
fn a_draft_with_a_live_lock_is_kept_and_the_person_is_told() {
    let (mut s, g, tmp) = listed(&[("d1", true, 1)]);
    std::fs::write(
        dir_of(&tmp, "d1").join("session.lock"),
        std::process::id().to_string(),
    )
    .unwrap();
    select(&mut s, 0, Focus::List);
    delete_chosen(&mut s, &g);
    assert!(dir_of(&tmp, "d1").exists());
    assert_eq!(labels(&s).len(), 2);
    let toast = s.toast.as_ref().expect("told");
    assert!(toast.text.contains("曲を削除できません"), "{}", toast.text);
}

#[test]
fn a_draft_with_a_dead_lock_goes() {
    let (mut s, g, tmp) = listed(&[("d1", true, 1)]);
    std::fs::write(dir_of(&tmp, "d1").join("session.lock"), "4000000000").unwrap();
    select(&mut s, 0, Focus::List);
    delete_chosen(&mut s, &g);
    assert!(!dir_of(&tmp, "d1").exists());
    assert_eq!(labels(&s), [crate::song::SAMPLE_NAME]);
}

#[test]
fn a_folder_that_will_not_go_keeps_its_row_and_says_so() {
    let (mut s, g, tmp) = listed(&[("d1", true, 1)]);
    let dir = dir_of(&tmp, "d1");
    let held = dir.clone();
    let _watch = xirius_core::io_test::observe(std::sync::Arc::new(move |op, path| {
        if op == "draft_remove" && path == held {
            Err(std::io::Error::other("held"))
        } else {
            Ok(())
        }
    }));
    select(&mut s, 0, Focus::List);
    delete_chosen(&mut s, &g);
    assert!(dir.exists());
    assert_eq!(labels(&s).len(), 2);
    assert!(
        s.toast
            .as_ref()
            .is_some_and(|t| t.text.contains("曲を削除できません"))
    );
}

#[test]
fn delete_while_the_list_is_filtered_deletes_the_shown_draft() {
    let (mut s, g, tmp) = listed(&[("alpha", true, 1), ("beta", true, 2)]);
    s.home.as_mut().unwrap().pick.field.value = "alp".into();
    s.home_tick();
    select(&mut s, 0, Focus::List);
    assert_eq!(chosen_label(&s), "alpha（保存前）");
    delete_chosen(&mut s, &g);
    assert!(!dir_of(&tmp, "alpha").exists());
    assert!(dir_of(&tmp, "beta").exists());
    assert!(labels(&s).is_empty() || !labels(&s).iter().any(|l| l.starts_with("alpha")));
}

#[test]
fn a_non_draft_dir_is_never_handed_to_the_worker() {
    let (mut s, _g, tmp) = listed(&[("old", false, 1)]);
    s.begin_draft_delete(tmp.path().join("old"));
    assert!(s.draft_removal.is_none());
    assert!(tmp.path().join("old").exists());
}

#[test]
fn a_screen_reader_reads_the_question_as_a_dialog_with_two_buttons_and_the_menu_as_one_row() {
    let (mut s, g, _tmp) = listed(&[("d1", true, 1)]);
    select(&mut s, 0, Focus::List);
    assert!(crate::unsaved::said(&s, &g).is_none());
    // The menu: one row, 「削除」.
    assert!(key(&mut s, &g, "menu", NONE, None));
    let rows = s.context_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].label.as_str(), rows[0].key.as_str()),
        ("削除", "Del")
    );
    s.ctx = None;
    s.popup = None;
    assert!(key(&mut s, &g, "delete", NONE, None));
    let said = crate::unsaved::said(&s, &g).expect("asked");
    assert_eq!(said.title, "「d1」を削除しますか");
    let [(back, back_at, back_goes), (delete, delete_at, delete_goes)] = said.buttons;
    assert_eq!((back, back_goes), ("戻る", false));
    assert_eq!((delete, delete_goes), ("削除", true));
    // Both buttons lie inside the dialog, side by side.
    for at in [back_at, delete_at] {
        assert!(at.x >= said.card.x && at.right() <= said.card.right());
        assert!(at.y >= said.card.y && at.bottom() <= said.card.bottom());
    }
    assert!(back_at.right() <= delete_at.x);
}
