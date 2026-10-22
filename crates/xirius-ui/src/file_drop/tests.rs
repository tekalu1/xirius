//! Dropping MIDI files (UI-076). Playing goes to a silent output that keeps
//! time: nothing reaches the speakers.

use std::time::{Duration, Instant};

use xirius_core::model::{ClipBody, TrackKind};
use xirius_session::host::Strip;
use xirius_session::output::OutputMode;

use super::*;
use crate::instrument::current;
use crate::song::Song;

/// A folder of its own for one test's files.
fn folder(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-ui-drop-{name}-"))
        .tempdir()
        .unwrap()
}

/// C, E, G as `three.mid` in `dir`.
fn three(dir: &Path) -> PathBuf {
    let path = dir.join("three.mid");
    std::fs::write(&path, crate::smf::tests::three_notes()).unwrap();
    path
}

fn empty() -> (Studio, Geometry) {
    let s = Studio::new(Song::empty());
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

/// The middle of the empty screen, where the entries are.
fn middle(g: &Geometry) -> (f32, f32) {
    let z = crate::start::zone(g);
    (z.x + z.w / 2.0, z.y + z.h / 2.0)
}

#[test]
fn a_dropped_mid_becomes_an_instrument_track_with_its_notes() {
    let dir_tmp = folder("one");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = empty();
    let (x, y) = middle(&g);
    assert!(s.drop_hover(true));
    assert!(s.drop_files(&g, x, y, &[three(&dir)]));
    assert_eq!(s.error, None);
    assert!(!s.drop_over);
    let p = s.project();
    let t = p
        .tracks
        .values()
        .find(|t| t.kind != TrackKind::Master)
        .unwrap();
    assert_eq!(t.kind, TrackKind::Instrument);
    assert_eq!(current(t).map(|x| x.name.as_str()), Some(DEFAULT));
    let clip = &t.clips[0];
    assert_eq!((clip.name.as_str(), clip.start), ("three", 0));
    assert_eq!(clip.length, p.time_map().bar_start(2), "one whole bar");
    let ClipBody::Midi { notes, .. } = &clip.body else {
        panic!("a MIDI clip")
    };
    let got: Vec<_> = notes
        .iter()
        .map(|n| (n.start, n.length, n.pitch, n.velocity))
        .collect();
    assert_eq!(
        got,
        [(0, 960, 60, 100), (960, 960, 64, 90), (1920, 960, 67, 80)]
    );
    assert_eq!(s.selected_clip, Some((t.id, clip.id)));
    assert_eq!(s.selected_row, Some(RowKind::Track(t.id)));
    assert_eq!(s.notice_text(), Some("three.mid をインポートしました"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn on_the_lanes_the_clip_starts_at_the_bar_under_the_drop() {
    let dir_tmp = folder("bar");
    let dir = dir_tmp.path().to_path_buf();
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let tm = s.project().time_map();
    let bar3 = tm.bar_start(3);
    let x = s.tick_x(&g, bar3 + 1200);
    let y = g.lanes.y + 10.0;
    assert!(g.lanes.contains(x, y), "bar 3 shows");
    assert_eq!(s.drop_tick(&g, x, y), bar3);
    let before = s.project().tracks.len();
    assert!(s.drop_files(&g, x, y, &[three(&dir)]));
    assert_eq!(s.error, None);
    let p = s.project();
    assert_eq!(p.tracks.len(), before + 1);
    let (track, clip) = s.selected_clip.unwrap();
    let clip = p.tracks[&track]
        .clips
        .iter()
        .find(|c| c.id == clip)
        .unwrap();
    assert_eq!(clip.start, bar3);
    // Off the lanes it goes to the head.
    assert_eq!(s.drop_tick(&g, g.column.x + 5.0, y), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn other_files_and_broken_ones_are_refused_with_their_name() {
    let dir_tmp = folder("bad");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = empty();
    let (x, y) = middle(&g);
    let wav = dir.join("take.wav");
    std::fs::write(&wav, b"RIFF").unwrap();
    assert!(s.drop_files(&g, x, y, std::slice::from_ref(&wav)));
    assert!(
        s.error
            .as_deref()
            .is_some_and(|e| e.contains("「take.wav」"))
    );
    let broken = dir.join("broken.mid");
    std::fs::write(&broken, b"MThd\0\0").unwrap();
    assert!(s.drop_files(&g, x, y, &[broken]));
    assert_eq!(
        s.error.as_deref(),
        Some("「broken.mid」: MIDI ファイルが途中で終わっています")
    );
    assert!(s.start_shown(), "no track was made");
    // A good file among them is still read.
    assert!(s.drop_files(&g, x, y, &[wav, three(&dir)]));
    assert_eq!(s.project().root.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn playing_after_the_drop_sounds_the_instrument() {
    let dir_tmp = folder("play");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = empty();
    s.transport.mode = OutputMode::Silent { paced: true };
    let (x, y) = middle(&g);
    assert!(s.drop_files(&g, x, y, &[three(&dir)]));
    let (track, _) = s.selected_clip.unwrap();
    assert_eq!(s.playhead, 0);
    assert!(s.toggle_play());
    assert_eq!(s.error, None);
    let since = Instant::now();
    let mut peak = 0.0f32;
    // Until the notes sound, not for a time: on a busy machine the player starts late. The
    // minute is only a bound.
    while peak <= 0.01 && since.elapsed() < Duration::from_secs(60) {
        s.transport.poll();
        if let Some(r) = s.transport.take_levels().get(&Strip::Track(track)) {
            peak = peak.max(r.peak[0]).max(r.peak[1]);
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    s.stop_transport();
    assert!(peak > 0.01, "the dropped notes sound ({peak})");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The tracks of the song's top level, in column order.
fn columns(s: &Studio) -> Vec<std::sync::Arc<xirius_core::model::Track>> {
    let p = s.project();
    p.root
        .iter()
        .filter_map(|n| match n {
            xirius_core::id::Node::Track(t) => p.tracks.get(t).cloned(),
            _ => None,
        })
        .filter(|t| t.kind != TrackKind::Master)
        .collect()
}

#[test]
fn tracks_from_the_entry_and_drops_take_the_colours_in_turn_without_a_limit() {
    let dir_tmp = folder("colours");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = empty();
    let e = crate::start::Entry::Midi;
    let (x, y) = s
        .center_of(&g, crate::studio::Hit::Start(e))
        .expect("on screen");
    assert_eq!(s.hit(&g, x, y), crate::studio::Hit::Start(e));
    assert!(s.press(&g, crate::studio::Hit::Start(e)));
    let (x, y) = middle(&g);
    for i in 0..5 {
        let path = dir.join(format!("take{i}.mid"));
        std::fs::copy(three(&dir), &path).unwrap();
        assert!(s.drop_files(&g, x, y, &[path]));
        assert_eq!(s.error, None);
    }
    let made = columns(&s);
    assert_eq!(made.len(), 6, "more than four tracks");
    let colours: Vec<usize> = made.iter().map(|t| crate::draw::track_color(t)).collect();
    assert_eq!(colours, [0, 1, 2, 3, 4, 5], "the colours in turn");
    let c = crate::paint::Colors::new(xirius_design::Theme::Dark);
    let drawn: std::collections::HashSet<_> = made
        .iter()
        .map(|t| format!("{:?}", crate::draw::color_of(&c, t)))
        .collect();
    assert_eq!(drawn.len(), 6, "each drawn in a colour of its own");
    // With the first gone, the next track passes over the colour the sixth has.
    s.song
        .run(
            "track.delete",
            serde_json::json!({ "ids": [made[0].id.to_string()] }),
        )
        .unwrap();
    s.after_edit();
    let path = dir.join("again.mid");
    std::fs::copy(three(&dir), &path).unwrap();
    assert!(s.drop_files(&g, x, y, &[path]));
    let last = columns(&s).last().cloned().unwrap();
    assert_eq!(
        crate::draw::track_color(&last),
        6,
        "the used colour is passed"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Melody and Bass at 100 BPM in 3/4, as a format 1 file `band.mid` in `dir`.
fn band(dir: &Path) -> PathBuf {
    use xirius_session::smf::build::{file, mtrk};
    let conductor = mtrk(&[
        (0, &[0xff, 0x51, 0x03, 0x09, 0x27, 0xc0]),
        (0, &[0xff, 0x58, 0x04, 3, 2, 24, 8]),
    ]);
    let melody = mtrk(&[
        (0, &[0xff, 0x03, 0x06, b'M', b'e', b'l', b'o', b'd', b'y']),
        (0, &[0x90, 72, 100]),
        (480, &[0x80, 72, 0]),
    ]);
    let bass = mtrk(&[
        (0, &[0xff, 0x03, 0x04, b'B', b'a', b's', b's']),
        (480, &[0x91, 36, 110]),
        (960, &[0x81, 36, 0]),
    ]);
    let path = dir.join("band.mid");
    std::fs::write(&path, file(1, 480, &[conductor, melody, bass])).unwrap();
    path
}

#[test]
fn each_track_of_a_dropped_file_is_a_track_and_an_empty_song_takes_its_tempo() {
    let dir_tmp = folder("band");
    let dir = dir_tmp.path().to_path_buf();
    let (mut s, g) = empty();
    let (x, y) = middle(&g);
    let steps = s.song.steps();
    assert!(s.drop_files(&g, x, y, &[band(&dir)]));
    assert_eq!(s.error, None);
    let p = s.project();
    let tracks: Vec<_> = p
        .tracks
        .values()
        .filter(|t| t.kind == TrackKind::Instrument)
        .map(|t| {
            let ClipBody::Midi { notes, .. } = &t.clips[0].body else {
                panic!("a MIDI clip")
            };
            let notes: Vec<_> = notes.iter().map(|n| (n.start, n.pitch)).collect();
            (t.name.clone(), t.clips[0].start, notes)
        })
        .collect();
    assert_eq!(
        tracks,
        [
            ("Melody".to_string(), 0, vec![(0, 72)]),
            ("Bass".to_string(), 0, vec![(960, 36)]),
        ]
    );
    let order: Vec<_> = p
        .root
        .iter()
        .filter_map(|n| match n {
            xirius_core::id::Node::Track(id) => Some(p.tracks[id].name.clone()),
            xirius_core::id::Node::Folder(_) => None,
        })
        .collect();
    assert_eq!(order, ["Melody", "Bass"], "in the file's order");
    assert_eq!(p.tempo.iter().map(|t| t.bpm).collect::<Vec<_>>(), [100.0]);
    assert_eq!(
        p.timesig
            .iter()
            .map(|t| (t.bar, t.num, t.den))
            .collect::<Vec<_>>(),
        [(1, 3, 4)]
    );
    assert_eq!(s.song.steps(), steps + 1, "one step");
    // A song that has clips keeps its tempo.
    assert!(s.drop_files(&g, x, y, &[three(&dir)]));
    assert_eq!(
        s.project().tempo.iter().map(|t| t.bpm).collect::<Vec<_>>(),
        [100.0]
    );
    assert!(s.undo(&g));
    assert!(s.undo(&g));
    let p = s.project();
    assert!(p.tracks.values().all(|t| t.kind != TrackKind::Instrument));
    assert_eq!(p.tempo.iter().map(|t| t.bpm).collect::<Vec<_>>(), [120.0]);
    let _ = std::fs::remove_dir_all(&dir);
}
