//! Files dragged out of the browser and files from the OS's file window (UI-017, UI-046,
//! BRW-005), on folders made for each test.

use xirius_core::model::{ClipBody, TrackKind};

use super::*;
use crate::browser::RailTab;
use crate::import_shots::{file_row, make_place, over_audio};

/// A folder of the test's own, removed when dropped (also when the test fails).
fn folder(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-ui-pick-{name}-"))
        .tempdir()
        .unwrap()
}

/// The sample song with the browser's files open on a place of WAV and MIDI files, its
/// Loops folder open. The place goes when the guard is dropped.
fn studio(name: &str) -> (Studio, PathBuf, tempfile::TempDir) {
    let top_tmp = folder(name);
    let top = top_tmp.path().join("素材");
    make_place(&top);
    let mut s = Studio::sample();
    s.browser.press_rail(RailTab::Files);
    s.browser.add_place(&top).unwrap();
    s.browser.toggle_dir(&top.join("Loops").to_string_lossy());
    (s, top, top_tmp)
}

fn geometry(s: &Studio) -> Geometry {
    s.geometry(1440.0, 860.0)
}

fn clips_of(s: &Studio, file: &Path) -> Vec<(xirius_core::id::TrackId, i64)> {
    let p = s.project();
    let mut out = Vec::new();
    for t in p.tracks.values() {
        for c in &t.clips {
            if let ClipBody::Audio { file: f, .. } = &c.body
                && Path::new(f) == file
            {
                out.push((t.id, c.start));
            }
        }
    }
    out
}

/// Presses the row of `name`, as the screen does.
fn grab(s: &mut Studio, g: &Geometry, name: &str) -> (f32, f32) {
    let i = file_row(s, g, name).unwrap_or_else(|| panic!("no row {name}"));
    let l = s.browser.layout(g.panel);
    let (x, y) = s.browser.center_of(&l, BrowserHit::File(i)).unwrap();
    let hit = s.hit(g, x, y);
    assert_eq!(hit, Hit::Browser(BrowserHit::File(i)));
    assert!(s.lib_grab(g, hit, x, y, 1));
    (x, y)
}

#[test]
fn a_file_dragged_onto_an_audio_track_is_a_clip_there_and_one_undo_takes_it_away() {
    let (mut s, top, _tmp) = studio("track");
    let g = geometry(&s);
    let loop_ = top.join("Loops/loop_120bpm.wav");
    let steps = s.song.steps();
    let tracks = s.project().tracks.len();
    let (x, y) = over_audio(&s, &g).expect("an audio track on the lanes");
    let track = s.audio_track_at(&g, x, y).unwrap();

    let from = grab(&mut s, &g, "loop_120bpm.wav");
    assert!(s.lib_move(from.0 + 2.0, from.1));
    assert!(
        !s.browser.file_drag.as_ref().unwrap().moved,
        "inside the slop"
    );
    assert!(s.lib_move(x, y));
    let d = s.browser.file_drag.clone().unwrap();
    assert!(d.moved);
    let name = s.project().tracks[&track].name.clone();
    assert_eq!(drop_label(&s, &g, &d), Some(format!("{name} に置く")));

    assert!(s.lib_up(&g, x, y));
    assert_eq!(s.error, None);
    assert!(s.browser.file_drag.is_none());
    assert_eq!(s.project().tracks.len(), tracks);
    let on = clips_of(&s, &loop_);
    assert_eq!(on.len(), 1);
    assert_eq!(on[0].0, track);
    assert_eq!(s.song.steps(), steps + 1);
    assert_eq!(s.notice_text(), Some("「loop_120bpm」を置きました"));

    assert!(s.undo(&g));
    assert!(clips_of(&s, &loop_).is_empty());
    assert_eq!(s.project().tracks.len(), tracks);
}

#[test]
fn a_midi_file_dragged_onto_the_lanes_is_a_track_of_its_own() {
    let (mut s, _, _tmp) = studio("midi");
    let g = geometry(&s);
    let tracks = s.project().tracks.len();
    let (x, y) = over_audio(&s, &g).unwrap();
    grab(&mut s, &g, "chords.mid");
    s.lib_move(x, y);
    let d = s.browser.file_drag.clone().unwrap();
    assert_eq!(
        drop_label(&s, &g, &d).as_deref(),
        Some("新しいトラックに置く")
    );
    s.lib_up(&g, x, y);
    assert_eq!(s.error, None);
    let p = s.project();
    assert_eq!(p.tracks.len(), tracks + 1);
    assert!(
        p.tracks
            .values()
            .any(|t| t.kind == TrackKind::Instrument && t.clips.iter().any(|c| c.name == "chords"))
    );
}

#[test]
fn a_press_that_does_not_move_or_lets_go_off_the_arrangement_puts_nothing() {
    let (mut s, _, _tmp) = studio("still");
    let g = geometry(&s);
    let steps = s.song.steps();
    let at = grab(&mut s, &g, "loop_96bpm.wav");
    assert!(s.lib_up(&g, at.0, at.1));
    // Dragged, then let go over the browser itself.
    let at = grab(&mut s, &g, "loop_96bpm.wav");
    s.lib_move(at.0 + 40.0, at.1 + 40.0);
    let d = s.browser.file_drag.clone().unwrap();
    assert_eq!(drop_label(&s, &g, &d), None);
    assert!(s.lib_up(&g, at.0 + 40.0, at.1 + 40.0));
    assert_eq!(s.song.steps(), steps);
    assert_eq!(s.error, None);
}

#[test]
fn a_double_press_on_a_folder_goes_into_it() {
    let (mut s, top, _tmp) = studio("enter");
    let g = geometry(&s);
    let l = s.browser.layout(g.panel);
    let i = l
        .rows
        .iter()
        .position(|p| matches!(&p.row, crate::browser::BRow::Dir { name, .. } if name == "Drums"))
        .unwrap();
    assert!(s.file_grab(&g, Hit::Browser(BrowserHit::Dir(i)), 0.0, 0.0, 2));
    assert_eq!(s.browser.path, ["Drums"]);
    assert_eq!(s.browser.here_dir(), Some(top.join("Drums")));
    assert!(
        !s.browser
            .open_dirs
            .contains(&*top.join("Drums").to_string_lossy())
    );
}

#[test]
fn from_file_puts_each_file_at_the_playhead_and_says_what_it_could_not() {
    let (mut s, top, _tmp) = studio("picked");
    let tracks = s.project().tracks.len();
    let steps = s.song.steps();
    let bar = s.project().time_map().bar_start(5);
    s.playhead = bar;
    let pad = top.join("pad_44k.wav");
    s.finish_picked(FileRequest::Audio, &[pad.clone(), top.join("idea.mid")]);
    assert_eq!(s.error, None);
    assert_eq!(s.project().tracks.len(), tracks + 2);
    assert_eq!(s.song.steps(), steps + 2);
    assert_eq!(s.notice_text(), Some("ファイル 2 個を置きました"));
    let on = clips_of(&s, &pad);
    assert_eq!(on.len(), 1);
    assert_eq!(on[0].1, bar);
    let p = s.project();
    let idea = p
        .tracks
        .values()
        .find(|t| t.clips.iter().any(|c| c.name == "idea"))
        .unwrap();
    assert_eq!(idea.clips[0].start, bar);

    s.finish_picked(FileRequest::Audio, &[top.join("notes.txt")]);
    assert!(s.error.as_deref().unwrap().contains("notes.txt"));
    assert_eq!(s.project().tracks.len(), tracks + 2);
}

#[test]
fn the_folder_window_gives_the_browser_a_tab() {
    let (mut s, top, _tmp) = studio("place");
    s.finish_picked(FileRequest::Place, &[top.join("Drums")]);
    assert_eq!(s.error, None);
    assert_eq!(s.browser.location().name, "Drums");
    assert_eq!(s.notice_text(), Some("場所のタブ「Drums」を足しました"));
    s.finish_picked(FileRequest::Place, &[top.join("nowhere")]);
    assert!(s.error.is_some());
}

#[test]
fn the_plus_of_the_tabs_and_from_file_ask_the_screen_for_a_file_window() {
    let (mut s, _, _tmp) = studio("ask");
    let g = geometry(&s);
    let l = s.browser.layout(g.panel);
    let (x, y) = s.browser.center_of(&l, BrowserHit::LocAdd).unwrap();
    let hit = s.hit(&g, x, y);
    s.press(&g, hit);
    assert_eq!(s.file_request, Some(FileRequest::Place));
}

#[test]
fn an_empty_song_can_start_from_a_file() {
    let top_tmp = folder("empty");
    let top = top_tmp.path().join("素材");
    make_place(&top);
    let mut s = Studio::new(crate::song::Song::empty());
    let tracks = s.project().tracks.len();
    assert!(
        s.project().tracks.values().all(|t| t.clips.is_empty()),
        "nothing on it"
    );
    let pad = top.join("pad_44k.wav");
    s.finish_picked(FileRequest::Audio, std::slice::from_ref(&pad));
    assert_eq!(s.error, None);
    assert_eq!(s.project().tracks.len(), tracks + 1);
    assert_eq!(clips_of(&s, &pad).len(), 1);
}
