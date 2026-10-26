//! UI-289: another process saves the song the screen has open. The other process is a
//! session of its own on the folder (PRJ-023), as `xirius --song …` is; the screen is told by
//! the OS's notice, as it is in the window.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::session::{RunOptions, Session};

use super::*;
use crate::song::Song;
use crate::studio::Hit;
use crate::toast::ToastPart;
use xirius_session::output::OutputMode;

#[test]
fn preparation_never_registers_or_stats_a_watch_even_after_look_interval() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, _, dir) = studio(root.path());
    let path = dir.clone();
    let _observe = xirius_core::io_test::observe(std::sync::Arc::new(move |op, p| {
        assert!(
            !(p == path && matches!(op, "watch_register" | "watch_metadata")),
            "a blocked frame must not enter watcher I/O"
        );
        Ok(())
    }));
    let mut card = crate::prepare_card::Card::new(Default::default());
    card.background = true;
    s.preparing = Some(card);
    let now = Instant::now() + LOOK * 4;
    assert!(!s.poll_other_change_at(now, |_| panic!("no geometry work")));
    s.other = OtherChange::default();
    assert!(!s.poll_other_change_at(now, |_| panic!("no registration")));
    assert!(s.other.watch.is_none());
    assert!(s.prepare_key("escape"));
}

fn studio(root: &Path) -> (Studio, Geometry, PathBuf) {
    let dir = root.join("song");
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&dir).unwrap();
    let g = s.geometry(1440.0, 820.0);
    // The first look starts the watch.
    assert!(!s.poll_other_change(|s| s.geometry(1440.0, 820.0)));
    assert!(s.other.watch.is_some());
    (s, g, dir)
}

fn add(s: &mut Session, name: &str) {
    s.run_value(
        "track.add.audio",
        json!({ "name": name }),
        &RunOptions::human(),
    )
    .unwrap();
}

/// What `xirius --song <dir> track add …` does: opens, changes, saves, lets go.
fn other_app(dir: &Path, names: &[&str]) {
    let mut cli = Session::open(dir).unwrap();
    for n in names {
        add(&mut cli, n);
    }
    cli.save().unwrap();
}

fn names(s: &Studio) -> Vec<String> {
    let mut n: Vec<String> = s
        .song
        .session()
        .project()
        .tracks
        .values()
        .map(|t| t.name.clone())
        .filter(|n| n != "Master")
        .collect();
    n.sort();
    n
}

fn mine(s: &mut Studio, name: &str) {
    assert!(s.command(|song| {
        song.run("track.add.audio", json!({ "name": name }))
            .map(|_| ())
    }));
    assert_eq!(s.error, None);
}

/// Polls as the window does, until `done`, or gives up after half a minute (the OS's notice,
/// and failing that the look every two seconds, comes well before).
fn until(s: &mut Studio, what: &str, done: impl Fn(&Studio) -> bool) {
    let t = Instant::now();
    while !done(s) {
        assert!(t.elapsed() < Duration::from_secs(30), "{what}");
        s.poll_other_change(|s| s.geometry(1440.0, 820.0));
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Polls for `d`, and nothing is to happen in it.
fn quiet(s: &mut Studio, d: Duration) {
    let t = Instant::now();
    while t.elapsed() < d {
        s.poll_other_change(|s| s.geometry(1440.0, 820.0));
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn text(s: &Studio) -> Option<&str> {
    s.toast_shown().map(|t| t.text.as_str())
}

#[test]
fn ui_289_a_clean_song_takes_the_file_as_one_step_and_undo_is_the_toast_s_button() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, g, dir) = studio(root.path());
    let steps = s.song.history().len();
    other_app(&dir, &["B", "C"]);
    until(&mut s, "the file comes in", |s| text(s) == Some(LOADED));

    assert_eq!(names(&s), ["B", "C"]);
    let history = s.song.history();
    assert_eq!(history.len(), steps + 1, "one step for both commands");
    assert_eq!(history.last().unwrap().label, "ほかのアプリの変更");
    assert!(!s.song.session().is_dirty(), "the song is the file now");
    let t = s.toast_shown().unwrap();
    assert!(t.info && t.brief && !t.alert);
    assert!(!t.stays(), "it goes after a while");
    assert_eq!(t.button, Some((UNDO.to_string(), Answer::UndoReload)));
    assert!(crate::toast::layout(&s, &g).unwrap().close.is_none());

    assert!(s.press(&g, Hit::Toast(ToastPart::Button)));
    assert_eq!(names(&s), Vec::<String>::new(), "back to before the file");
    assert!(s.toast_shown().is_none_or(|t| t.text != LOADED));
    // The undone file is not asked about again.
    quiet(&mut s, SETTLE * 2);
    assert!(s.toast_shown().is_none());
    // Redo brings it back, as for any step.
    assert!(s.key(&g, "z", true, true));
    assert_eq!(names(&s), ["B", "C"]);
}

#[test]
fn ui_289_ctrl_z_takes_the_file_back_as_one_step_and_the_toast_goes_with_it() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, g, dir) = studio(root.path());
    mine(&mut s, "A");
    s.key(&g, "s", true, false);
    s.wait_for_save();
    other_app(&dir, &["B", "C"]);
    until(&mut s, "the file comes in", |s| text(s) == Some(LOADED));
    assert!(s.key(&g, "z", true, false));
    assert_eq!(names(&s), ["A"]);
    s.poll_other_change(|s| s.geometry(1440.0, 820.0));
    assert!(
        s.toast.is_none(),
        "its button would take back something else"
    );
}

#[test]
fn ui_289_unsaved_changes_ask_first_and_x_keeps_the_screen_s_song_until_the_next_change() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, g, dir) = studio(root.path());
    // The other app opens before the screen's edit, so it does not have it (PRJ-023).
    let mut cli = Session::open(&dir).unwrap();
    mine(&mut s, "Mine");
    add(&mut cli, "Theirs");
    cli.save().unwrap();
    until(&mut s, "the question", |s| text(s) == Some(ASK));

    let t = s.toast_shown().unwrap();
    assert!(t.info && t.stays() && !t.alert);
    assert_eq!(t.button, Some((LOAD.to_string(), Answer::Reload)));
    assert!(crate::toast::layout(&s, &g).unwrap().close.is_some());
    assert_eq!(names(&s), ["Mine"], "nothing is taken before the answer");

    // ×: the screen keeps its song, and the same file is not asked about again.
    assert!(s.press(&g, Hit::Toast(ToastPart::Close)));
    quiet(&mut s, LOOK + SETTLE);
    assert!(s.toast.is_none());
    assert_eq!(names(&s), ["Mine"]);

    // It changes again: asked again.
    add(&mut cli, "Again");
    cli.save().unwrap();
    drop(cli);
    until(&mut s, "asked again", |s| text(s) == Some(ASK));

    // 読み込む: the file comes in, as one step over the screen's edit.
    let steps = s.song.history().len();
    assert!(s.press(&g, Hit::Toast(ToastPart::Button)));
    assert_eq!(names(&s), ["Again", "Theirs"]);
    assert_eq!(s.song.history().len(), steps + 1);
    assert_eq!(text(&s), Some(LOADED));
    assert!(!s.song.session().is_dirty());
    // 元に戻す brings the screen's edit back.
    assert!(s.press(&g, Hit::Toast(ToastPart::Button)));
    assert_eq!(names(&s), ["Mine"]);
}

#[test]
fn ui_289_the_question_goes_when_the_screen_saves_over_the_file() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, g, dir) = studio(root.path());
    let mut cli = Session::open(&dir).unwrap();
    mine(&mut s, "Mine");
    add(&mut cli, "Theirs");
    cli.save().unwrap();
    drop(cli);
    until(&mut s, "the question", |s| text(s) == Some(ASK));
    s.key(&g, "s", true, false);
    s.wait_for_save();
    until(&mut s, "the question goes", |s| s.toast.is_none());
    assert_eq!(names(&s), ["Mine"]);
}

#[test]
fn ui_289_the_screen_s_own_save_is_not_another_app_s() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, g, _dir) = studio(root.path());
    let steps = s.song.history().len();
    mine(&mut s, "A");
    s.key(&g, "s", true, false);
    s.wait_for_save();
    // Past the look at the file's time too.
    quiet(&mut s, LOOK + SETTLE * 2);
    assert!(
        s.toast_shown()
            .is_none_or(|t| t.text != LOADED && t.text != ASK)
    );
    assert_eq!(s.song.history().len(), steps + 1);
    assert!(!s.song.session().is_dirty());
}

#[test]
fn ui_289_a_run_of_writes_comes_in_once() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, _g, dir) = studio(root.path());
    let steps = s.song.history().len();
    // Supply notice times and the polling clock: disk speed under the default
    // parallel runner must not turn a simulated burst into separate writes.
    s.other.watch.as_mut().unwrap()._os.take();
    let start = Instant::now();
    let mut now = start;
    for n in ["A", "B", "C", "D", "E"] {
        other_app(&dir, &[n]);
        *s.other.watch.as_ref().unwrap().heard.lock().unwrap() = Some(now);
        s.poll_other_change_at(now, |s| s.geometry(1440.0, 820.0));
        assert_eq!(
            s.song.history().len(),
            steps,
            "nothing taken during the burst"
        );
        now += Duration::from_millis(60);
    }
    s.poll_other_change_at(now + SETTLE, |s| s.geometry(1440.0, 820.0));
    s.poll_other_change_at(now + SETTLE * 2, |s| s.geometry(1440.0, 820.0));
    assert_eq!(s.song.history().len(), steps + 1, "one step for the run");
    assert_eq!(names(&s), ["A", "B", "C", "D", "E"]);
}

#[test]
fn ui_289_a_drag_puts_it_off_until_the_button_comes_up() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, _g, dir) = studio(root.path());
    s.pointer_held = true;
    other_app(&dir, &["B"]);
    quiet(&mut s, LOOK + SETTLE * 2);
    assert!(names(&s).is_empty(), "nothing comes in under the drag");
    assert!(s.toast.is_none());
    s.pointer_held = false;
    until(&mut s, "it comes in after", |s| text(s) == Some(LOADED));
    assert_eq!(names(&s), ["B"]);
}

#[test]
fn ui_289_a_warning_is_not_covered_by_the_news() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, _g, dir) = studio(root.path());
    s.warn("保存できません".to_string(), None);
    other_app(&dir, &["B"]);
    quiet(&mut s, SETTLE * 3);
    assert_eq!(text(&s), Some("保存できません"));
    assert!(names(&s).is_empty());
    s.toast = None;
    until(&mut s, "it comes in after", |s| text(s) == Some(LOADED));
}

#[test]
fn ui_289_playing_goes_on_through_the_load() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, _g, dir) = studio(root.path());
    s.transport.mode = OutputMode::Silent { paced: true };
    assert!(s.toggle_play());
    assert!(s.transport.playing, "{:?}", s.error);
    other_app(&dir, &["B"]);
    until(&mut s, "the file comes in", |s| text(s) == Some(LOADED));
    assert!(s.transport.playing, "the load does not stop it");
    assert_eq!(s.error, None);
    assert!(s.recovery.is_none(), "no card for a song opened again");
    assert!(s.preparing.is_none(), "no card for a song opened again");
    let g = s.geometry(1440.0, 820.0);
    assert!(s.press(&g, Hit::Toast(ToastPart::Button)));
    assert!(names(&s).is_empty());
    assert!(s.transport.playing, "nor does taking it back");
    s.stop_transport();
}

#[test]
fn ui_289_the_os_s_notice_comes_before_the_look_at_the_file_s_time() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, _g, dir) = studio(root.path());
    assert!(
        s.other.watch.as_ref().unwrap()._os.is_some(),
        "the OS watches the folder here"
    );
    let watch = s.other.watch.as_mut().unwrap();
    let frozen = watch.looked;
    let initial_stamp = watch.stamp;
    other_app(&dir, &["B"]);
    // Freeze the fallback clock: even a slow OS callback must be the source
    // of this answer. A wall-clock race against LOOK also measures scheduling
    // of unrelated parallel tests and cannot prove which path answered.
    let deadline = Instant::now() + Duration::from_secs(30);
    while watch.changed(frozen).is_none() {
        assert!(Instant::now() < deadline, "OS notification watchdog");
        std::thread::yield_now();
    }
    assert_eq!(watch.looked, frozen);
    assert_eq!(
        watch.stamp, initial_stamp,
        "the fallback must not stat the file"
    );
}
