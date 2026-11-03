//! PLG-036: the window's save, close and open wait over the frames for the ARA plugins'
//! archives, with the stand-in plugin of `xirius_ara::fake` registered as `fake:ara` and a
//! silent player. The clock of the waits is the test's (`wait::script_time`), so running out of
//! time takes no real seconds. One test: the stand-in's edits are global to the process.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use xirius_ara::fake::{self, Config};
use xirius_ara::service::modification_id;
use xirius_core::id::ClipId;
use xirius_session::ara::STORE_WAIT;
use xirius_session::output::OutputMode;

use super::{ARA_LATE, ARA_LATE_LEAVING, Work};
use crate::clip_ara::Badge;
use crate::song::Song;
use crate::studio::{FileRequest, Studio};

const PLUGIN: &str = "fake:ara";
const RATE: u32 = 48_000;
/// Longer than the frames below take to run out the wait, shorter than a test should last.
const SLOW: Duration = Duration::from_millis(1500);

/// The window's frames until `done`.
fn frames(s: &mut Studio, what: &str, done: impl Fn(&Studio) -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !done(s) {
        assert!(
            Instant::now() < end,
            "timed out waiting for {what}: {:?}",
            s.error
        );
        s.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The save or the open has its plugins' states, and waits only for the ARA archive.
fn states_ready(s: &Studio) -> bool {
    let save = s
        .saving
        .as_ref()
        .is_some_and(|save| matches!(&save.work, Work::States(p) if p.ready()));
    let open = s.opening.as_ref().is_some_and(|o| o.states_ready());
    save || open
}

/// An archive asked for is on its way.
fn arriving(s: &Studio) -> bool {
    s.transport.host().is_some_and(|h| h.take_ara_arrived())
}

/// The window's plain save, as Ctrl+S.
fn save(s: &mut Studio) {
    assert!(s.start_save(), "{:?}", s.error);
    assert!(s.saving.is_some(), "the save did not start: {:?}", s.error);
}

/// Runs the wait out: the test's clock jumps to the end of [`STORE_WAIT`] after `since`.
fn run_out(s: &mut Studio, since: Instant) {
    frames(s, "the plugins' states", states_ready);
    assert!(arriving(s), "the archive was not asked for");
    crate::wait::script_time(Some(since + STORE_WAIT));
}

#[test]
fn saving_and_leaving_wait_for_the_ara_archive_over_the_frames() {
    xirius_session::ara::register(
        PLUGIN,
        "Fake ARA",
        Arc::new(|| fake::loader(Config::default())),
    );
    let root = tempfile::tempdir().unwrap();
    let song = root.path().join("song");
    let mut s = Studio::new(Song::empty());
    s.transport.mode = OutputMode::Silent { paced: false };
    s.finish_file_request_sync(FileRequest::SaveAs, &song);
    assert_eq!(s.error, None);
    let v = vec![0.5; RATE as usize];
    xirius_session::export::write_wav(&song.join("a.wav"), &v, &v, RATE, 32).unwrap();
    let t = s.song.run("track.add.audio", json!({})).unwrap()["id"].clone();
    let c = s
        .song
        .run(
            "clip.add.audio",
            json!({"track": t, "at": 0, "length": 1920, "file": "a.wav"}),
        )
        .unwrap()["id"]
        .clone();
    s.song
        .run("clip.ara.attach", json!({"ids": [c], "plugin": PLUGIN}))
        .unwrap();
    let c: ClipId = c.as_str().unwrap().parse().unwrap();
    let modification = modification_id(c.0);
    let p = s.project();
    s.transport.start_host(&p, s.song.dir()).unwrap();
    frames(&mut s, "the analysis", |s| {
        s.ara_badge(c) == Some(Badge::Ready)
    });
    save(&mut s);
    frames(&mut s, "the first save", |s| s.saving.is_none());
    assert!(s.song.is_saved(), "{:?}", s.error);

    // The archive comes while the save waits: it is written with the edit, and nothing is said.
    let before = s.project().ara.archives.get(PLUGIN).cloned();
    fake::slow_store(Duration::from_millis(200));
    fake::edit(&modification, 0.3);
    save(&mut s);
    frames(&mut s, "the save", |s| s.saving.is_none());
    assert!(s.song.is_saved(), "{:?}", s.error);
    assert!(s.toast.is_none(), "{:?}", s.toast.as_ref().map(|t| &t.text));
    assert_ne!(
        s.project().ara.archives.get(PLUGIN).cloned(),
        before,
        "the save left the plugin's edit out"
    );

    // Late for a plain save: written with what came, said so, and unsaved again once it comes.
    fake::slow_store(SLOW);
    fake::edit(&modification, 0.4);
    let since = Instant::now();
    crate::wait::script_time(Some(since));
    save(&mut s);
    run_out(&mut s, since);
    frames(&mut s, "the save", |s| s.saving.is_none());
    assert!(s.song.is_saved(), "{:?}", s.error);
    assert_eq!(s.toast.as_ref().map(|t| t.text.as_str()), Some(ARA_LATE));
    crate::wait::script_time(None);
    frames(&mut s, "the late archive in the song", |s| {
        !s.song.is_saved()
    });
    save(&mut s);
    frames(&mut s, "the save", |s| s.saving.is_none());
    assert!(s.song.is_saved(), "{:?}", s.error);

    // Late as the song is closed: it stays open with the question, and nothing is lost.
    fake::edit(&modification, 0.5);
    let since = Instant::now();
    crate::wait::script_time(Some(since));
    assert!(!s.request_close());
    run_out(&mut s, since);
    frames(&mut s, "the close", |s| s.saving.is_none());
    assert_eq!(s.error.as_deref(), Some(ARA_LATE_LEAVING));
    assert!(
        s.unsaved.is_some() && s.window_ask.is_none(),
        "the close went on without the plugin's edit"
    );
    s.answer_unsaved(false);
    crate::wait::script_time(None);
    frames(&mut s, "the late archive in the song", |s| {
        !arriving(s) && !s.song.is_saved()
    });
    save(&mut s);
    frames(&mut s, "the save", |s| s.saving.is_none());
    assert!(s.song.is_saved(), "{:?}", s.error);

    // Late as another song is opened: the song stays, with the question.
    fake::edit(&modification, 0.6);
    let since = Instant::now();
    crate::wait::script_time(Some(since));
    assert!(s.request_open_file(FileRequest::Open, &song));
    run_out(&mut s, since);
    frames(&mut s, "the open", |s| s.unsaved.is_some());
    assert_eq!(s.error.as_deref(), Some(ARA_LATE_LEAVING));
    s.answer_unsaved(false);
    frames(&mut s, "the open given up", |s| s.opening.is_none());
    assert_eq!(s.song.dir().as_deref(), Some(song.as_path()));
    crate::wait::script_time(None);
    frames(&mut s, "the late archive in the song", |s| {
        !arriving(s) && !s.song.is_saved()
    });
    fake::slow_store(Duration::ZERO);
}
