//! UI-289: the screen takes what another process saved to `project.json` as one step.
//! Each session holds a lock of its own on the folder, as a process does (PRJ-023), so the
//! second session stands for the `xirius` program.

use serde_json::json;
use xirius_core::Project;
use xirius_core::command::Author;
use xirius_core::session::{RunOptions, Session};

fn add(s: &mut Session, name: &str) {
    s.run_value(
        "track.add.audio",
        json!({ "name": name }),
        &RunOptions::human(),
    )
    .unwrap();
}

fn names(s: &Session) -> Vec<String> {
    let mut n: Vec<String> = s
        .project()
        .tracks
        .values()
        .map(|t| t.name.clone())
        .filter(|n| n != "Master")
        .collect();
    n.sort();
    n
}

fn undo(s: &mut Session) {
    s.run_value("history.undo", json!({}), &RunOptions::human())
        .unwrap();
}

#[test]
fn another_process_s_save_comes_in_as_one_step_that_undo_takes_back() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut screen = Session::create(&dir, Project::new("Reload")).unwrap();
    add(&mut screen, "A");
    screen.save().unwrap();
    // Its own save is not a change.
    assert_eq!(screen.disk_changed().unwrap(), None);
    assert!(screen.reload_from_disk(&Author::Human).unwrap().is_none());

    let mut cli = Session::open(&dir).unwrap();
    add(&mut cli, "B");
    add(&mut cli, "C");
    cli.save().unwrap();
    drop(cli);

    let hash = screen.disk_changed().unwrap().expect("the file changed");
    let steps = screen.history().len();
    let (n, changes) = screen
        .reload_from_disk(&Author::Human)
        .unwrap()
        .expect("a step");
    assert!(!changes.is_empty());
    assert_eq!(names(&screen), ["A", "B", "C"]);
    assert_eq!(screen.history().len(), steps + 1);
    let last = screen.history().pop().unwrap();
    assert_eq!((last.n, last.command.as_str()), (n, "project.reload"));
    assert!(!screen.is_dirty(), "the song is the file now");
    assert_eq!(screen.disk_changed().unwrap(), None);
    assert!(screen.reload_from_disk(&Author::Human).unwrap().is_none());
    assert_eq!(screen.store().unwrap().project_hash().unwrap(), Some(hash));

    undo(&mut screen);
    assert_eq!(names(&screen), ["A"]);
    assert!(screen.is_dirty(), "the song is no longer the file");
    // The undone load is not taken for a new change.
    assert_eq!(screen.disk_changed().unwrap(), None);
}

#[test]
fn unsaved_edits_stay_under_the_loaded_step() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut screen = Session::create(&dir, Project::new("Reload")).unwrap();
    screen.save().unwrap();
    // The other process starts from the screen's song as it is when it opens (PRJ-023).
    let mut cli = Session::open(&dir).unwrap();
    add(&mut screen, "Mine");
    assert!(screen.is_dirty());
    add(&mut cli, "Theirs");
    cli.save().unwrap();
    drop(cli);

    assert!(screen.disk_changed().unwrap().is_some());
    screen.reload_from_disk(&Author::Human).unwrap().unwrap();
    assert_eq!(names(&screen), ["Theirs"]);
    assert!(!screen.is_dirty());
    undo(&mut screen);
    assert_eq!(
        names(&screen),
        ["Mine"],
        "undo brings the screen's edit back"
    );
    // New ids do not collide with the screen's own.
    add(&mut screen, "Next");
    assert_eq!(names(&screen), ["Mine", "Next"]);
}

#[test]
fn a_save_after_the_load_is_undone_is_what_the_next_process_opens() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut screen = Session::create(&dir, Project::new("Reload")).unwrap();
    add(&mut screen, "A");
    screen.save().unwrap();
    let mut cli = Session::open(&dir).unwrap();
    add(&mut cli, "B");
    cli.save().unwrap();
    drop(cli);
    screen.reload_from_disk(&Author::Human).unwrap().unwrap();
    undo(&mut screen);
    screen.save().unwrap();
    assert_eq!(screen.disk_changed().unwrap(), None);

    let mut cli = Session::open(&dir).unwrap();
    assert_eq!(names(&cli), ["A"], "it starts from the screen's save");
    add(&mut cli, "C");
    cli.save().unwrap();
    drop(cli);
    screen.reload_from_disk(&Author::Human).unwrap().unwrap();
    assert_eq!(names(&screen), ["A", "C"]);
}

#[test]
fn the_next_process_goes_on_from_its_own_save_while_the_file_is_it() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut screen = Session::create(&dir, Project::new("Reload")).unwrap();
    screen.save().unwrap();
    // Commands one after another, the screen taking none of them yet.
    for n in ["A", "B"] {
        let mut cli = Session::open(&dir).unwrap();
        add(&mut cli, n);
        cli.save().unwrap();
    }
    screen.reload_from_disk(&Author::Human).unwrap().unwrap();
    assert_eq!(names(&screen), ["A", "B"]);
    // Taken back and not saved: the file is still the other process's, and it goes on.
    undo(&mut screen);
    let mut cli = Session::open(&dir).unwrap();
    assert_eq!(names(&cli), ["A", "B"]);
    add(&mut cli, "C");
    cli.save().unwrap();
    drop(cli);
    // Saved over by the screen: it goes on from that, its own run one step back.
    screen.save().unwrap();
    let mut cli = Session::open(&dir).unwrap();
    assert_eq!(names(&cli), Vec::<String>::new());
    undo(&mut cli);
    assert_eq!(names(&cli), ["A", "B", "C"]);
}

#[test]
fn a_file_with_the_song_as_it_is_adds_no_step_but_counts_as_saved() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut screen = Session::create(&dir, Project::new("Reload")).unwrap();
    screen.save().unwrap();
    add(&mut screen, "A");
    assert!(screen.is_dirty());
    // Another process writes the song exactly as the screen has it.
    xirius_core::store::Store::new(&dir)
        .write_project(screen.project())
        .unwrap();

    let steps = screen.history().len();
    assert!(screen.disk_changed().unwrap().is_some());
    assert!(screen.reload_from_disk(&Author::Human).unwrap().is_none());
    assert_eq!(screen.history().len(), steps);
    assert!(!screen.is_dirty());
    assert_eq!(screen.disk_changed().unwrap(), None);
}
