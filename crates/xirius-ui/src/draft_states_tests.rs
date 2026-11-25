//! The draft of every 30 s holds what the plug-ins hold now (PRJ-034), with the test VST3 in a
//! host process, its patch changed without a word.

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

use super::*;
use crate::keep::Keep;

/// The fixture saves its "Latency" as a delay of 256 × the value, in 4 little-endian bytes.
fn delay(v: f64) -> Vec<u8> {
    ((v * 256.0) as u32).to_le_bytes().to_vec()
}

/// The fixture on Vocal in a host of its own, playing silently, the song kept in a draft
/// folder of the test's own.
fn song(root: &std::path::Path) -> (Studio, TrackId, PluginId) {
    let mut s = Studio::sample();
    crate::plugin_reload_shots::prepare(&mut s, 0);
    let id = crate::plugin_reload_shots::id(&s);
    let vocal = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .unwrap()
        .id;
    assert!(s.run_command(
        "plugin.isolation.set",
        json!({ "ids": [id.to_string()], "mode": "dedicated" })
    ));
    s.keep = Keep::at(root.to_path_buf());
    s.keep.sync(&s.song.project());
    let g = s.geometry(1600.0, 1000.0);
    s.resized(&g);
    assert!(s.toggle_play());
    let loaded = |s: &Studio| {
        s.transport.host().is_some_and(|host| {
            host.plugins().reports().iter().any(|r| {
                r.id == id
                    && r.status.latency > xirius_engine::server::DEFAULT_LATENCY
                    && !r.status.crashed
            })
        })
    };
    crate::shot::run_for(&mut s, 15.0, loaded);
    assert!(loaded(&s), "{:?}", s.error);
    // The first beat gives the song its draft folder.
    s.keep_beat(xirius_core::store::now_ms());
    assert!(s.song.record_dir().is_some(), "{:?}", s.keep.failed);
    (s, vocal, id)
}

/// The plug-in's "Latency" turned in its own window: the song hears nothing of it. The mix is
/// built again with a new instance; this waits until it has taken the old one's place.
fn turn_silently(s: &mut Studio, id: PluginId, v: f64) {
    let host = s.transport.host().unwrap();
    let link = host.plugins().link(id).unwrap();
    let before = link.current_gate().stamp().instance;
    host.plugins()
        .server_of(id)
        .unwrap()
        .set_param(&link, "Latency", v);
    let rebuilt = |s: &Studio| {
        s.transport.host().is_some_and(|host| {
            !host.swap_pending() && link.current_gate().stamp().instance != before
        })
    };
    crate::shot::run_for(s, 30.0, rebuilt);
    assert!(rebuilt(s), "{:?}", s.error);
}

/// The plug-ins' answers held back on this test's thread while it lives, as from a host
/// slower than any beat.
struct Hold {
    _observe: xirius_core::io_test::Observation,
}

impl Hold {
    fn new() -> Hold {
        let test = std::thread::current().id();
        let _observe = xirius_core::io_test::observe(std::sync::Arc::new(move |op, _| {
            if op == "capture_answer" && std::thread::current().id() == test {
                return Err(std::io::Error::other("held by the test"));
            }
            Ok(())
        }));
        Hold { _observe }
    }
}

/// The delay the insert's state says, in `p`.
fn state(p: &xirius_core::model::Project, t: TrackId) -> Option<Vec<u8>> {
    p.tracks[&t].inserts[0]
        .state
        .as_ref()
        .map(|b| xirius_plugins::decode_state(b).unwrap().component)
}

fn drafts(s: &Studio) -> Vec<String> {
    s.song
        .session()
        .versions()
        .unwrap()
        .into_iter()
        .filter(|v| v.name == xirius_core::session::DRAFT_NAME)
        .map(|v| v.id)
        .collect()
}

/// Beats at `now` until the draft is made or 10 s have passed, as the idle poll would.
fn beat_until_draft(s: &mut Studio, now: u64, before: usize) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while drafts(s).len() == before && std::time::Instant::now() < end {
        s.keep_beat(now);
        s.song.flush_writes();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

#[test]
fn the_draft_holds_what_the_plugin_holds_now_and_the_file_does_not_change() {
    let _records = crate::shot::CrashRecords::new();
    let tmp = crate::shot::TestFolder::new("xirius-ui-draft-states-");
    let (mut s, vocal, id) = song(tmp.path());
    turn_silently(&mut s, id, 0.5);
    assert_eq!(
        state(&s.project(), vocal),
        None,
        "nothing said, nothing kept"
    );
    let steps = s.song.steps();

    let due = xirius_core::store::now_ms() + 31_000;
    s.keep_beat(due);
    // The first beat asks; the window goes on and the input is not held.
    assert!(s.draft_pull.is_some() || !drafts(&s).is_empty());
    assert!(!s.input_blocked());
    beat_until_draft(&mut s, due, 0);
    assert!(s.draft_pull.is_none());

    let all = drafts(&s);
    let [draft] = all.as_slice() else {
        panic!("one draft: {all:?}");
    };
    let kept = s
        .song
        .session()
        .store()
        .unwrap()
        .read_version(draft)
        .unwrap();
    assert_eq!(state(&kept, vocal), Some(delay(0.5)), "the draft holds it");
    assert_eq!(state(&s.project(), vocal), Some(delay(0.5)));
    // In place: no step of its own, nothing to undo.
    assert_eq!(s.song.steps(), steps);
    s.transport.pause();
}

#[test]
fn nothing_is_asked_while_a_take_is_recorded_nor_collected_while_a_knob_is_held() {
    let _records = crate::shot::CrashRecords::new();
    let tmp = crate::shot::TestFolder::new("xirius-ui-draft-states-busy-");
    let (mut s, vocal, id) = song(tmp.path());
    turn_silently(&mut s, id, 0.5);
    let due = xirius_core::store::now_ms() + 31_000;

    s.transport.recording = true;
    s.keep_beat(due);
    s.keep_beat(due + 1);
    assert!(s.draft_pull.is_none(), "not asked during the take");
    assert!(drafts(&s).is_empty());
    s.transport.recording = false;

    // A fader held: nothing asked yet, and a question asked before waits for the release.
    s.pointer_held = true;
    s.keep_beat(due + 2);
    assert!(s.draft_pull.is_none(), "not asked while held");
    s.pointer_held = false;
    s.keep_beat(due + 3);
    assert!(s.draft_pull.is_some() || !drafts(&s).is_empty());
    // A host that answers within the same beat leaves nothing waiting to hold back.
    let waiting = s.draft_pull.is_some();
    s.pointer_held = true;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while waiting && std::time::Instant::now() < end {
        s.keep_beat(due + 4);
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    if waiting {
        assert_eq!(state(&s.project(), vocal), None, "held: not collected");
    }
    s.pointer_held = false;
    beat_until_draft(&mut s, due + 5, 0);
    assert_eq!(state(&s.project(), vocal), Some(delay(0.5)));

    // A take that starts while a question waits drops it: a late answer is not put in.
    turn_silently(&mut s, id, 0.25);
    let later = due + 31_000 + 6;
    let hold = Hold::new();
    s.keep_beat(later);
    assert!(s.draft_pull.is_some(), "the answer is held");
    s.transport.recording = true;
    s.keep_beat(later + 1);
    assert!(s.draft_pull.is_none());
    assert_eq!(state(&s.project(), vocal), Some(delay(0.5)));
    drop(hold);
    s.transport.recording = false;
    s.transport.pause();
}

#[test]
fn a_closed_plugin_window_keeps_its_question_until_a_beat_collects_it() {
    let _records = crate::shot::CrashRecords::new();
    let tmp = crate::shot::TestFolder::new("xirius-ui-draft-states-close-");
    let (mut s, vocal, id) = song(tmp.path());
    turn_silently(&mut s, id, 0.5);
    s.pull_plugins();
    assert!(s.draft_pull.is_some(), "the question is kept, not dropped");
    let now = xirius_core::store::now_ms();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while s.draft_pull.is_some() && std::time::Instant::now() < end {
        s.keep_beat(now);
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    assert_eq!(state(&s.project(), vocal), Some(delay(0.5)));
    // Not a draft of its own: the next draft is made when it is due.
    assert!(drafts(&s).is_empty());
    s.transport.pause();
}
