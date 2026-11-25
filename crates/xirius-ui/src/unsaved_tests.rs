//! 「変更を保存しますか」 (PRJ-033): asked only when the song is unsaved, its buttons
//! やめる・保存しない・保存 left to right, the keys starting on 保存.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::json;

use super::*;
use crate::keep::{Keep, LastOpen, RecentList, SaveMark};
use crate::song::Song;
use crate::studio::FileRequest;

/// A studio that keeps its songs in a folder of the test's own; the guard comes first, so it
/// is dropped after the studio.
fn kept(tag: &str) -> (crate::shot::TestFolder, Studio, PathBuf) {
    let root_tmp = crate::shot::TestFolder::new(&format!("xirius-ui-unsaved-{tag}-"));
    let root = root_tmp.path().to_path_buf();
    let mut s = Studio::new(Song::empty());
    s.keep = Keep::at(root.clone());
    s.keep.sync(&s.song.project());
    (root_tmp, s, root)
}

fn add(s: &mut Studio, name: &str) {
    assert!(s.run_command("track.add.audio", json!({ "name": name })));
}

/// A song with one track, saved in `song` under the test's folder.
fn saved(tag: &str) -> (crate::shot::TestFolder, Studio, PathBuf) {
    let (tmp, mut s, root) = kept(tag);
    add(&mut s, "saved");
    let dir = root.join("song");
    s.finish_file_request_sync(FileRequest::SaveAs, &dir);
    assert_eq!(s.song.dir(), Some(dir.clone()));
    s.keep.settle_recent();
    s.keep.poll_recent();
    (tmp, s, dir)
}

fn file(dir: &Path) -> Vec<u8> {
    std::fs::read(dir.join("project.json")).unwrap()
}

fn mark(s: &Studio) -> SaveMark {
    s.keep.mark(&s.song, u64::MAX)
}

/// Closes the window to the end, asking nothing more on the way.
fn close_all_the_way(s: &mut Studio) {
    let end = Instant::now() + Duration::from_secs(15);
    while !s.request_close() {
        assert!(Instant::now() < end, "the close did not get ready");
        assert!(s.unsaved.is_none(), "asked again");
        s.wait_for_save();
        s.keep.settle_recent();
        s.poll_recent_close();
    }
    s.release_song();
    s.keep.settle_recent();
}

fn focus(s: &Studio) -> usize {
    s.unsaved.as_ref().unwrap().focus
}

fn center(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

#[test]
fn the_mark_follows_the_saved_step_and_an_untouched_new_song_shows_none() {
    {
        let (_tmp, mut s, _root) = kept("mark");
        assert_eq!(mark(&s), SaveMark::Hidden);
        add(&mut s, "new");
        assert_eq!(mark(&s), SaveMark::Unsaved);
        assert!(s.save_ask().is_some());
        assert!(s.song.undo());
        assert_eq!(mark(&s), SaveMark::Hidden, "undone back to empty");
        assert!(s.save_ask().is_none(), "and leaving it asks nothing");
    }
    let (_tmp, mut s, dir) = saved("mark-saved");
    let at_save = file(&dir);
    assert_eq!(mark(&s), SaveMark::Saved);
    add(&mut s, "later");
    assert_eq!(mark(&s), SaveMark::Unsaved);
    // An edit goes to the draft, not the song's file.
    s.song.flush_writes();
    assert_eq!(file(&dir), at_save);
    assert!(s.song.undo());
    assert_eq!(mark(&s), SaveMark::Saved, "undone back to the save");
    assert!(s.song.redo());
    assert_eq!(mark(&s), SaveMark::Unsaved);
}

#[test]
fn a_song_at_its_save_closes_without_asking() {
    let (_tmp, mut s, _dir) = saved("clean");
    assert!(s.save_question().is_none());
    close_all_the_way(&mut s);
}

#[test]
fn the_question_reads_cancel_discard_save_left_to_right_and_the_keys_start_on_save() {
    let (_tmp, mut s, dir) = saved("keys");
    let at_save = file(&dir);
    add(&mut s, "later");
    assert!(!s.request_close());
    let ask = s.save_question().expect("an unsaved song asks");
    assert!(!ask.new_song);
    let g = s.geometry(1600.0, 1000.0);
    let said = said(&s, &g).unwrap();
    assert_eq!(
        said.title,
        format!("「{}」{SAVE_TITLE_TAIL}", s.project().name)
    );
    let labels: Vec<&str> = said.buttons.iter().map(|b| b.0).collect();
    assert_eq!(labels, [CANCEL, DISCARD, SAVE]);
    assert!(
        said.buttons.windows(2).all(|w| w[0].1.right() <= w[1].1.x),
        "left to right"
    );
    assert_eq!(focus(&s), 2, "the keys start on 保存");
    // Tab and the arrows go round the three.
    assert!(s.unsaved_key("tab"));
    assert_eq!(focus(&s), 0);
    assert!(s.unsaved_key("left"));
    assert_eq!(focus(&s), 2);
    assert!(s.unsaved_key("left"));
    assert_eq!(focus(&s), 1);
    assert!(s.unsaved_key("right"));
    assert_eq!(focus(&s), 2);

    // Esc: nothing closes and nothing is saved.
    assert!(s.unsaved_key("escape"));
    assert!(s.unsaved.is_none());
    assert!(s.window_ask.is_none());
    assert!(s.saving.is_none());
    assert_eq!(file(&dir), at_save);
    assert_eq!(mark(&s), SaveMark::Unsaved);

    // Enter on 保存: the song is saved, then the window goes.
    assert!(!s.request_close());
    assert_eq!(focus(&s), 2);
    assert!(s.unsaved_key("enter"));
    s.wait_for_save();
    assert_ne!(file(&dir), at_save);
    assert_eq!(mark(&s), SaveMark::Saved);
    close_all_the_way(&mut s);
}

#[test]
fn discard_leaves_the_file_as_saved_and_the_next_open_starts_there_with_a_draft() {
    let (_tmp, mut s, dir) = saved("discard");
    let at_save = file(&dir);
    add(&mut s, "later");
    assert!(!s.request_close());
    let g = s.geometry(1600.0, 1000.0);
    let [_, discard, _] = save_buttons(&g);
    let (x, y) = center(discard);
    assert!(s.unsaved_press(&g, x, y));
    assert!(s.unsaved.is_none());
    close_all_the_way(&mut s);
    assert_eq!(file(&dir), at_save, "保存しない: the file is as saved");
    drop(s);

    let mut session = xirius_core::session::Session::open(&dir).unwrap();
    assert!(session.recovery().is_none(), "closed, not crashed");
    session.start_from_saved().unwrap();
    let names = |p: &xirius_core::model::Project| {
        let mut n: Vec<String> = p.tracks.values().map(|t| t.name.clone()).collect();
        n.sort();
        n
    };
    assert_eq!(names(session.project()), ["Master", "saved"]);
    let drafts: Vec<_> = session
        .versions()
        .unwrap()
        .into_iter()
        .filter(|v| v.name == xirius_core::session::DRAFT_NAME)
        .collect();
    assert_eq!(drafts.len(), 1, "the changes are in a draft");
    let kept = session
        .store()
        .unwrap()
        .read_version(&drafts[0].id)
        .unwrap();
    assert_eq!(names(&kept), ["Master", "later", "saved"]);
}

#[test]
fn a_new_song_not_saved_goes_to_the_trash_and_off_the_list() {
    let (_tmp, mut s, root) = kept("trash");
    let bin = root.join("bin");
    s.keep.test_bin = Some(bin.clone());
    add(&mut s, "x");
    let draft = s.song.record_dir().expect("the edit made a draft");
    assert_eq!(mark(&s), SaveMark::Unsaved);
    assert!(!s.request_close());
    let ask = s.save_question().expect("asked");
    assert!(ask.new_song);
    let g = s.geometry(1600.0, 1000.0);
    let said = said(&s, &g).unwrap();
    assert_eq!(said.title, NEW_SONG_TITLE);
    let labels: Vec<&str> = said.buttons.iter().map(|b| b.0).collect();
    assert_eq!(labels, [CANCEL, DISCARD, SAVE_AS]);
    assert!(s.unsaved_key("left") && s.unsaved_key("enter"));
    close_all_the_way(&mut s);
    assert!(!draft.exists());
    assert!(
        bin.join(draft.file_name().unwrap()).exists(),
        "in the trash"
    );
    let list = RecentList::in_dir(&root);
    assert!(list.songs().iter().all(|r| r.dir != draft));
    assert_eq!(list.last_open(), LastOpen::Empty);
}

#[test]
fn save_as_on_a_new_song_asks_where_and_closes_once_saved() {
    let (_tmp, mut s, root) = kept("save-as");
    add(&mut s, "x");
    let draft = s.song.record_dir().unwrap();
    assert!(!s.request_close());
    assert!(s.unsaved_key("enter"));
    assert!(matches!(s.file_request.take(), Some(FileRequest::SaveAs)));
    let dest = root.join("named");
    s.start_file_request(FileRequest::SaveAs, &dest);
    s.wait_for_save();
    assert_eq!(s.song.dir(), Some(dest.clone()));
    close_all_the_way(&mut s);
    assert!(dest.join("project.json").exists());
    assert!(!draft.exists());
}

#[test]
fn a_new_song_left_empty_again_goes_without_asking() {
    let (_tmp, mut s, _root) = kept("empty");
    add(&mut s, "x");
    let draft = s.song.record_dir().unwrap();
    assert!(s.song.undo());
    assert!(s.save_ask().is_none());
    close_all_the_way(&mut s);
    assert!(!draft.exists(), "an empty draft is deleted");
}
