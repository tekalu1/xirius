//! The AI's work on the song: one request is one step, a work copy is taken
//! in or thrown away by a person, deleting and saving wait for an answer, and
//! the UI can see what the AI is editing.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use xirius_core::CmdError;
use xirius_core::autosave::{self, Autosave};
use xirius_core::error::ErrorCode;
use xirius_core::session::{
    AiPermission, Confirm, OnConflict, RequestMode, RequestState, RunOptions, RunResult, Session,
};

/// A song folder named `tag`, not made yet, in a folder of the test's own that goes when the
/// guard is dropped (also when the test fails).
fn temp_dir(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("xirius-aiwork-test-")
        .tempdir()
        .unwrap();
    let dir = base.path().join(tag);
    (base, dir)
}

fn human(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn ai(s: &mut Session, name: &str, args: Value) -> Result<RunResult, CmdError> {
    s.run_value(name, args, &RunOptions::ai("claude"))
}

fn ai_ok(s: &mut Session, name: &str, args: Value) -> RunResult {
    ai(s, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

/// Tracks other than the master.
fn tracks(s: &Session) -> usize {
    s.project().tracks.len() - 1
}

fn session() -> Session {
    let mut s = Session::new("ai");
    s.set_ai_permission(AiPermission::Edit).unwrap();
    s.set_confirm(Confirm::OPEN_04);
    s
}

// ---- one request = one step --------------------------------------------------------------

#[test]
fn a_batch_is_one_step_by_the_ai_and_undoes_whole() {
    let mut s = session();
    let before = s.steps().len();
    let r = ai_ok(
        &mut s,
        "ai.batch",
        json!({"title": "ドラムを足す", "commands": [
            {"command": "track.add.audio", "args": {"name": "Kick"}},
            {"command": "track.add.audio", "args": {"name": "Snare"}},
            {"command": "track.add.audio", "args": {"name": "Hat"}},
        ]}),
    );
    assert_eq!(tracks(&s), 3);
    assert_eq!(s.steps().len(), before + 1, "three commands, one step");
    assert_eq!(r.step, Some(s.cursor()));
    let last = s.steps().last().unwrap();
    assert!(last.author.is_ai(), "authored as the AI");
    assert!(last.summary.contains("ドラムを足す"), "{}", last.summary);
    s.undo(1).unwrap();
    assert_eq!(tracks(&s), 0, "one undo brings back all three");
    s.redo(1).unwrap();
    assert_eq!(tracks(&s), 3);
}

#[test]
fn a_batch_that_fails_in_the_middle_changes_nothing() {
    let mut s = session();
    human(&mut s, "track.add.audio", json!({"name": "Keep"}));
    let steps = s.steps().len();
    let hash = s.project().content_hash();
    let e = ai(
        &mut s,
        "ai.batch",
        json!({"commands": [
            {"command": "track.add.audio", "args": {"name": "A"}},
            {"command": "track.rename", "args": {"id": "trk_999", "name": "x"}},
        ]}),
    )
    .expect_err("the second command fails");
    assert!(e.message.contains("2 番目"), "{}", e.message);
    assert_eq!(s.project().content_hash(), hash);
    assert_eq!(s.steps().len(), steps, "no step is left behind");
    assert!(s.request_info().is_none(), "the request is closed");
}

#[test]
fn a_batch_dry_run_shows_the_change_and_edits_nothing() {
    let mut s = session();
    let r = s
        .run_value(
            "ai.batch",
            json!({"commands": [{"command": "track.add.audio", "args": {}}]}),
            &RunOptions::ai("claude").dry(),
        )
        .unwrap();
    assert!(r.dry_run);
    assert!(!r.changes.is_empty());
    assert_eq!(tracks(&s), 0);
}

#[test]
fn an_open_request_folds_its_commands_into_one_step() {
    let mut s = session();
    human(&mut s, "track.add.audio", json!({"name": "Mine"}));
    let before = s.steps().len();
    ai_ok(&mut s, "ai.request.begin", json!({"title": "整える"}));
    ai_ok(&mut s, "track.add.audio", json!({"name": "A"}));
    ai_ok(&mut s, "track.add.audio", json!({"name": "B"}));
    assert_eq!(
        s.steps().len(),
        before + 2,
        "steps pile up while it is open"
    );
    let end = ai_ok(&mut s, "ai.request.end", json!({}));
    assert_eq!(s.steps().len(), before + 1, "and become one at the end");
    assert_eq!(end.data["commands"], 2);
    assert_eq!(tracks(&s), 3);
    s.undo(1).unwrap();
    assert_eq!(tracks(&s), 1, "the human's track stays");
}

#[test]
fn a_request_with_one_command_keeps_that_step_as_it_is() {
    let mut s = session();
    ai_ok(&mut s, "ai.request.begin", json!({"title": "1 つ足す"}));
    ai_ok(&mut s, "track.add.audio", json!({"name": "A"}));
    let steps = s.steps().len();
    ai_ok(&mut s, "ai.request.end", json!({}));
    assert_eq!(s.steps().len(), steps);
}

#[test]
fn a_person_can_throw_a_direct_request_away() {
    let mut s = session();
    human(&mut s, "track.add.audio", json!({"name": "Mine"}));
    let hash = s.project().content_hash();
    let steps = s.steps().len();
    ai_ok(&mut s, "ai.request.begin", json!({"title": "やりすぎ"}));
    ai_ok(&mut s, "track.add.audio", json!({"name": "A"}));
    ai_ok(&mut s, "track.add.audio", json!({"name": "B"}));
    let e = ai(&mut s, "ai.request.revert", json!({})).expect_err("an AI cannot judge itself");
    assert_eq!(e.code, ErrorCode::Denied);
    human(&mut s, "ai.request.revert", json!({}));
    assert_eq!(s.project().content_hash(), hash);
    assert_eq!(s.steps().len(), steps);
    assert!(s.request_info().is_none());
}

#[test]
fn only_one_ai_works_at_a_time() {
    let mut s = session();
    ai_ok(&mut s, "ai.request.begin", json!({"title": "最初"}));
    let e = s
        .run_value("track.add.audio", json!({}), &RunOptions::ai("other"))
        .expect_err("a second AI waits");
    assert_eq!(e.code, ErrorCode::Conflict);
    let e = s
        .run_value(
            "ai.request.begin",
            json!({"title": "二つ目"}),
            &RunOptions::ai("other"),
        )
        .expect_err("and cannot start another request");
    assert_eq!(e.code, ErrorCode::Conflict);
    // a person is never held up
    human(&mut s, "track.add.audio", json!({"name": "Human"}));
}

// ---- the work copy -----------------------------------------------------------------------

#[test]
fn a_work_copy_does_not_touch_the_song_until_a_person_accepts() {
    let mut s = session();
    human(&mut s, "track.add.audio", json!({"name": "Mine"}));
    let hash = s.project().content_hash();
    let steps = s.steps().len();

    ai_ok(
        &mut s,
        "ai.request.begin",
        json!({"title": "コピーで試す", "draft": true}),
    );
    ai_ok(&mut s, "track.add.audio", json!({"name": "A"}));
    ai_ok(&mut s, "track.add.audio", json!({"name": "B"}));
    assert_eq!(s.project().content_hash(), hash, "the song did not move");
    assert_eq!(s.steps().len(), steps, "no step yet");
    assert_eq!(s.draft_project().unwrap().tracks.len(), 4);

    let status = ai_ok(&mut s, "ai.request.status", json!({}));
    assert_eq!(status.data["request"]["mode"], "draft");
    assert!(
        status.data["request"]["changes"]["items"].is_array(),
        "what accepting would change: {}",
        status.data
    );

    ai_ok(&mut s, "ai.request.end", json!({}));
    let info = s.request_info().unwrap();
    assert_eq!(info.state, RequestState::Review);
    let e = ai(&mut s, "track.add.audio", json!({"name": "late"}))
        .expect_err("the AI waits for the person");
    assert_eq!(e.code, ErrorCode::Conflict);

    let e = ai(&mut s, "ai.request.accept", json!({})).expect_err("an AI cannot accept");
    assert_eq!(e.code, ErrorCode::Denied);
    assert_eq!(s.project().content_hash(), hash);

    let r = human(&mut s, "ai.request.accept", json!({}));
    assert_eq!(tracks(&s), 3);
    assert_eq!(s.steps().len(), steps + 1, "accepting is one step");
    assert_eq!(r.step, Some(s.cursor()));
    assert!(s.steps().last().unwrap().author.is_ai());
    assert!(s.request_info().is_none());
    s.undo(1).unwrap();
    assert_eq!(s.project().content_hash(), hash, "and it can be undone");
}

#[test]
fn a_work_copy_can_be_thrown_away() {
    let mut s = session();
    let hash = s.project().content_hash();
    let steps = s.steps().len();
    ai_ok(
        &mut s,
        "ai.request.begin",
        json!({"title": "捨てる", "draft": true}),
    );
    ai_ok(&mut s, "track.add.audio", json!({"name": "A"}));
    ai_ok(&mut s, "ai.request.end", json!({}));
    let r = human(&mut s, "ai.request.revert", json!({}));
    assert_eq!(r.data["result"], "reverted");
    assert_eq!(s.project().content_hash(), hash);
    assert_eq!(s.steps().len(), steps);
    assert!(s.draft_project().is_none());
}

#[test]
fn a_work_copy_refuses_when_the_song_moved_unless_told_to_replace() {
    let mut s = session();
    ai_ok(
        &mut s,
        "ai.request.begin",
        json!({"title": "古くなる", "draft": true}),
    );
    ai_ok(&mut s, "track.add.audio", json!({"name": "AI"}));
    ai_ok(&mut s, "ai.request.end", json!({}));
    human(&mut s, "track.add.audio", json!({"name": "Human"}));

    let e = s
        .run_value("ai.request.accept", json!({}), &RunOptions::human())
        .expect_err("the song moved");
    assert_eq!(e.code, ErrorCode::Conflict);
    assert!(
        s.request_info().is_some(),
        "the copy is kept for the person"
    );
    assert_eq!(tracks(&s), 1);

    let hash = s.project().content_hash();
    human(
        &mut s,
        "ai.request.accept",
        json!({"on_conflict": "replace"}),
    );
    assert_eq!(tracks(&s), 1, "replaced by the copy: only the AI's track");
    s.undo(1).unwrap();
    assert_eq!(
        s.project().content_hash(),
        hash,
        "undo brings the human's back"
    );
}

#[test]
fn a_work_copy_keeps_ids_from_clashing_with_the_song() {
    let mut s = session();
    ai_ok(
        &mut s,
        "ai.request.begin",
        json!({"title": "id", "draft": true}),
    );
    let a = id_of(&ai_ok(&mut s, "track.add.audio", json!({"name": "AI"})));
    ai_ok(&mut s, "ai.request.end", json!({}));
    human(&mut s, "ai.request.accept", json!({}));
    let h = id_of(&human(&mut s, "track.add.audio", json!({"name": "After"})));
    assert_ne!(a, h, "ids are never reused");
}

#[test]
fn the_api_can_open_a_draft_without_the_command() {
    let mut s = session();
    let info = s
        .begin_request(&"ui".into_ai(), "api", RequestMode::Draft)
        .unwrap();
    assert_eq!(info.mode, RequestMode::Draft);
    let out = s.revert_request().unwrap();
    assert_eq!(out.result, "reverted");
    let _ = OnConflict::Reject;
}

trait IntoAi {
    fn into_ai(self) -> xirius_core::Author;
}
impl IntoAi for &str {
    fn into_ai(self) -> xirius_core::Author {
        xirius_core::Author::Ai(self.to_string())
    }
}

// ---- asking before deleting and saving ----------------------------------------------------

#[test]
fn the_ais_delete_waits_for_a_person() {
    let mut s = session();
    let t = id_of(&human(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let hash = s.project().content_hash();
    let steps = s.steps().len();

    let r = ai_ok(&mut s, "track.delete", json!({"ids": [t]}));
    let d = r.decision.expect("a question, not a deletion");
    assert_eq!(d.default_answer, "no");
    assert!(d.token.starts_with("dec_"));
    assert_eq!(d.command, "track.delete");
    assert_eq!(s.project().content_hash(), hash, "nothing was deleted");
    assert_eq!(s.steps().len(), steps);
    assert_eq!(s.pending_decisions().len(), 1);

    // an AI cannot answer for the person
    let e = ai(
        &mut s,
        "ai.decide",
        json!({"token": d.token, "answer": "yes"}),
    )
    .expect_err("not for an AI");
    assert_eq!(e.code, ErrorCode::Denied);
    assert_eq!(tracks(&s), 1);

    let done = human(
        &mut s,
        "ai.decide",
        json!({"token": d.token, "answer": "yes"}),
    );
    assert_eq!(tracks(&s), 0, "now it is deleted");
    assert!(
        s.steps().last().unwrap().author.is_ai(),
        "still the AI's step"
    );
    assert_eq!(done.step, Some(s.cursor()));
    s.undo(1).unwrap();
    assert_eq!(tracks(&s), 1, "and undoable");
}

#[test]
fn the_answer_is_good_once_and_no_means_no() {
    let mut s = session();
    let t = id_of(&human(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let d = ai_ok(&mut s, "track.delete", json!({"ids": [t]}))
        .decision
        .unwrap();
    let no = human(
        &mut s,
        "ai.decide",
        json!({"token": d.token, "answer": "no"}),
    );
    assert!(no.message.contains("実行しませんでした"));
    assert_eq!(tracks(&s), 1);
    let e = s
        .run_value(
            "ai.decide",
            json!({"token": d.token, "answer": "yes"}),
            &RunOptions::human(),
        )
        .expect_err("used up");
    assert_eq!(e.code, ErrorCode::NotFound);
    assert_eq!(tracks(&s), 1);
}

#[test]
fn a_late_answer_is_refused() {
    let mut s = session();
    let t = id_of(&human(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let d = ai_ok(&mut s, "track.delete", json!({"ids": [t]}))
        .decision
        .unwrap();
    let late = d.expires_ms + 1;
    let e = s
        .decide(&d.token, true, late)
        .expect_err("120 seconds passed");
    assert_eq!(e.code, ErrorCode::Expired);
    assert_eq!(tracks(&s), 1);
    // and the token was consumed before it was checked
    assert!(s.pending_decisions().is_empty());
}

#[test]
fn a_person_deleting_is_never_asked() {
    let mut s = session();
    let t = id_of(&human(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let r = human(&mut s, "track.delete", json!({"ids": [t]}));
    assert!(r.decision.is_none());
    assert_eq!(tracks(&s), 0);
}

#[test]
fn a_dry_run_delete_shows_what_would_go_without_asking() {
    let mut s = session();
    let t = id_of(&human(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let r = s
        .run_value(
            "track.delete",
            json!({"ids": [t], "dry_run": true}),
            &RunOptions::ai("claude"),
        )
        .unwrap();
    assert!(r.dry_run && r.decision.is_none());
    assert!(!r.changes.is_empty(), "it says what would go");
    assert_eq!(tracks(&s), 1);
    assert!(s.pending_decisions().is_empty());
}

#[test]
fn a_failing_delete_is_not_worth_a_question() {
    let mut s = session();
    let e = ai(&mut s, "track.delete", json!({"ids": ["trk_999"]})).expect_err("no such track");
    assert_eq!(e.code, ErrorCode::NotFound);
    assert!(s.pending_decisions().is_empty());
}

#[test]
fn the_confirmation_can_be_turned_off_in_the_core() {
    let mut s = Session::new("plain");
    s.set_ai_permission(AiPermission::Edit).unwrap();
    let t = id_of(&ai_ok(&mut s, "track.add.audio", json!({})));
    let r = ai_ok(&mut s, "track.delete", json!({"ids": [t]}));
    assert!(r.decision.is_none());
    assert_eq!(tracks(&s), 0);
}

#[test]
fn a_batch_cannot_hide_a_delete() {
    let mut s = session();
    let t = id_of(&human(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let e = ai(
        &mut s,
        "ai.batch",
        json!({"commands": [
            {"command": "track.add.audio", "args": {}},
            {"command": "track.delete", "args": {"ids": [t]}},
        ]}),
    )
    .expect_err("a delete in a batch needs a person");
    assert_eq!(e.code, ErrorCode::Denied);
    assert_eq!(tracks(&s), 1, "all undone, the track stays");
    assert!(s.pending_decisions().is_empty(), "and no stray question");
}

#[test]
fn saving_over_the_song_waits_for_a_person_unless_the_ai_may_save() {
    let (_save_tmp, save) = temp_dir("save");
    let mut s = session();
    s.save_as(save).unwrap();
    ai_ok(&mut s, "track.add.audio", json!({"name": "A"}));
    let r = ai_ok(&mut s, "project.save", json!({}));
    let d = r.decision.expect("asks first");
    assert!(s.is_dirty(), "not saved yet");
    human(
        &mut s,
        "ai.decide",
        json!({"token": d.token, "answer": "yes"}),
    );
    assert!(!s.is_dirty(), "saved after the answer");

    // with the save permission, no question
    s.set_ai_permission(AiPermission::Save).unwrap();
    ai_ok(&mut s, "track.add.audio", json!({"name": "B"}));
    let r = ai_ok(&mut s, "project.save", json!({}));
    assert!(r.decision.is_none());
    assert!(!s.is_dirty());
}

// ---- what the UI sees ---------------------------------------------------------------------

#[test]
fn the_ui_can_see_what_the_ai_is_editing() {
    let mut s = session();
    assert!(!s.ai_activity().active);
    ai_ok(&mut s, "ai.request.begin", json!({"title": "見せる"}));
    let t = id_of(&ai_ok(&mut s, "track.add.audio", json!({"name": "A"})));
    let a = s.ai_activity();
    assert!(a.active);
    assert!(a.editing.iter().any(|x| x.id == t && x.kind == "track"));
    assert!(a.marks.iter().any(|x| x.id == t));

    let r = ai_ok(&mut s, "ai.activity", json!({}));
    assert_eq!(r.data["live"], true);
    assert_eq!(r.data["request"]["title"], "見せる");

    ai_ok(&mut s, "ai.request.end", json!({}));
    let a = s.ai_activity();
    assert!(!a.active);
    assert!(a.editing.is_empty(), "the glow stops");
    assert!(a.marks.iter().any(|x| x.id == t), "the purple edge stays");
}

#[test]
fn a_person_touching_it_clears_the_mark() {
    let mut s = session();
    let t = id_of(&ai_ok(&mut s, "track.add.audio", json!({"name": "A"})));
    assert!(s.ai_activity().marks.iter().any(|x| x.id == t));
    human(&mut s, "track.rename", json!({"id": t, "name": "Mine"}));
    assert!(!s.ai_activity().marks.iter().any(|x| x.id == t));
}

#[test]
fn a_work_copy_shows_the_same_glow_and_a_throw_away_clears_the_marks() {
    let mut s = session();
    ai_ok(
        &mut s,
        "ai.request.begin",
        json!({"title": "コピー", "draft": true}),
    );
    let t = id_of(&ai_ok(&mut s, "track.add.audio", json!({"name": "A"})));
    assert!(s.ai_activity().editing.iter().any(|x| x.id == t));
    ai_ok(&mut s, "ai.request.end", json!({}));
    human(&mut s, "ai.request.revert", json!({}));
    assert!(s.ai_activity().marks.is_empty());
}

// ---- the autosave clock --------------------------------------------------------------------

#[test]
fn a_tick_makes_a_save_point_only_when_the_song_changed() {
    let (_tick_tmp, tick) = temp_dir("tick");
    let mut s = Session::new("auto");
    s.save_as(tick).unwrap();
    s.auto_interval_ms = 0;
    assert!(autosave::tick(&mut s).is_none(), "nothing changed");
    human(&mut s, "track.add.audio", json!({"name": "A"}));
    assert!(autosave::tick(&mut s).is_some(), "changed: a save point");
    assert!(autosave::tick(&mut s).is_none(), "and not again");
}

#[test]
fn the_default_beat_is_thirty_seconds() {
    assert_eq!(autosave::DEFAULT_INTERVAL, Duration::from_secs(30));
}

#[test]
fn the_clock_makes_save_points_on_its_own_and_stops() {
    let (_clock_tmp, clock) = temp_dir("clock");
    let mut s = Session::new("clock");
    s.save_as(clock).unwrap();
    human(&mut s, "track.add.audio", json!({"name": "A"}));
    let shared = Arc::new(Mutex::new(s));
    let made = Arc::new(Mutex::new(0usize));
    let seen = made.clone();
    let clock = Autosave::start(
        shared.clone(),
        Duration::from_millis(20),
        Some(Box::new(move |_| *seen.lock().unwrap() += 1)),
    );
    let started = std::time::Instant::now();
    while *made.lock().unwrap() == 0 && started.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(10));
    }
    clock.stop();
    assert_eq!(*made.lock().unwrap(), 1, "one save point for one change");
    let n = shared.lock().unwrap().versions().unwrap().len();
    assert_eq!(n, 1);
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(*made.lock().unwrap(), 1, "stopped for good");
}
