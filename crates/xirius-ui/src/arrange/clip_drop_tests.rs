//! UI-381: the ghost clip, the seam it opens and where the files go. The times are given, not
//! waited for: each move and frame says when it is.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;
use crate::arrange::clip_drop::{Aim, GHOST_ALPHA, stem};
use crate::drop_audio::tests::tone;

/// A folder of the test's own, removed when dropped (also when the test fails).
fn folder(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-ui-clipdrop-{name}-"))
        .tempdir()
        .unwrap()
}

fn ms(t0: Instant, n: u64) -> Instant {
    t0 + Duration::from_millis(n)
}

/// An audio track whose row has another row right above it: its row and the boundary above it.
fn audio_below_a_row(s: &Studio) -> (Row, (Parent, usize)) {
    let p = s.project();
    let tree = s.tree();
    let row = tree
        .rows
        .iter()
        .skip(1)
        .find(|r| match r.kind {
            RowKind::Track(t) => p.tracks[&t].kind == TrackKind::Audio,
            RowKind::Folder(_) => false,
        })
        .copied()
        .expect("an audio track below another row");
    let key = p.find_node(node_of(row.kind)).unwrap();
    (row, key)
}

/// An instrument track that is not the last row.
fn instrument_not_last(s: &Studio) -> Row {
    let p = s.project();
    let tree = s.tree();
    let n = tree.rows.len();
    tree.rows[..n - 1]
        .iter()
        .find(|r| matches!(r.kind, RowKind::Track(t) if p.tracks[&t].kind == TrackKind::Instrument))
        .copied()
        .expect("an instrument track above another row")
}

fn track_of(row: &Row) -> TrackId {
    match row.kind {
        RowKind::Track(t) => t,
        RowKind::Folder(_) => panic!("a track row"),
    }
}

/// The height on screen of a point `dy` below the top of `row`, before any gap. The sample
/// shows all its rows: nothing is scrolled (`hold` checks).
fn screen_y(g: &Geometry, row: &Row, dy: f32) -> f32 {
    g.column.y + row.y + dy
}

fn mid_x(g: &Geometry) -> f32 {
    g.lanes.x + g.lanes.w / 2.0
}

/// Files held from the system at `(x, y)` at `now`: the window says it is over.
fn hold(s: &mut Studio, g: &Geometry, x: f32, y: f32, now: Instant, paths: &[PathBuf]) {
    assert_eq!(s.scroll.1, 0.0);
    s.drop_over = true;
    assert!(s.clip_drop_moved(g, x, y, false, now, paths, true));
}

fn new_track_at(s: &Studio, parent: Parent, i: usize) -> TrackId {
    match s.project().children(parent)[i] {
        Node::Track(t) => t,
        Node::Folder(_) => panic!("a track at {i}"),
    }
}

#[test]
fn a_slow_pointer_opens_the_seam_at_once_and_a_fast_one_once_it_stops() {
    let dir = folder("timing");
    let kick = tone(dir.path(), "Kick.wav", 0.5);
    let x;
    let (row, key);
    {
        // Slow (just come from rest): at once.
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        (row, key) = audio_below_a_row(&s);
        x = mid_x(&g);
        let t0 = Instant::now();
        hold(
            &mut s,
            &g,
            x,
            screen_y(&g, &row, 1.0),
            t0,
            std::slice::from_ref(&kick),
        );
        let d = &s.arrange.clip_drop;
        assert_eq!(d.live.live().map(|l| l.spot), Some(key));
        assert!(matches!(d.aim, Some(Aim::Seam { seam, .. }) if seam.key == key && !seam.bottom));
    }
    // Fast: it waits, and shows once the pointer has stood still for 120 ms.
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let t0 = Instant::now();
    hold(
        &mut s,
        &g,
        x,
        screen_y(&g, &row, row.h / 2.0),
        t0,
        std::slice::from_ref(&kick),
    );
    hold(
        &mut s,
        &g,
        x,
        screen_y(&g, &row, 1.0),
        ms(t0, 8),
        std::slice::from_ref(&kick),
    );
    assert!(s.arrange.clip_drop.live.wait_pending());
    assert!(s.arrange.clip_drop.live.live().is_none());
    assert_eq!(
        s.arrange.clip_drop.aim,
        Some(Aim::Track {
            track: track_of(&row),
            at: s.clip_drop_at(&g, x, false)
        }),
        "on the track until the seam shows"
    );
    assert!(s.clip_drop_tick(&g, ms(t0, 8 + 119)));
    assert!(s.arrange.clip_drop.live.live().is_none());
    s.clip_drop_tick(&g, ms(t0, 8 + 120));
    assert_eq!(s.arrange.clip_drop.live.live().map(|l| l.spot), Some(key));

    // One going through does not show it at all.
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    hold(
        &mut s,
        &g,
        x,
        screen_y(&g, &row, row.h / 2.0),
        t0,
        std::slice::from_ref(&kick),
    );
    hold(
        &mut s,
        &g,
        x,
        screen_y(&g, &row, 1.0),
        ms(t0, 8),
        std::slice::from_ref(&kick),
    );
    hold(
        &mut s,
        &g,
        x,
        screen_y(&g, &row, row.h / 2.0),
        ms(t0, 16),
        &[kick],
    );
    s.clip_drop_tick(&g, ms(t0, 200));
    assert!(s.arrange.clip_drop.live.live().is_none());
}

#[test]
fn the_gap_opens_over_120_ms_and_the_rows_below_move_down_into_it() {
    let dir = folder("gap");
    let kick = tone(dir.path(), "Kick.wav", 0.5);
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let (row, key) = audio_below_a_row(&s);
    let above = s.tree().rows.iter().find(|r| r.y < row.y).copied().unwrap();
    let full = s.arrange.track_h;
    let x = mid_x(&g);
    let t0 = Instant::now();
    let paths = [kick];
    hold(&mut s, &g, x, screen_y(&g, &row, 1.0), t0, &paths);
    assert_eq!(s.arrange.clip_drop.gap_of(key), 0.0);
    s.clip_drop_tick(&g, ms(t0, 60));
    let half = s.arrange.clip_drop.gap_of(key);
    assert!(
        half > 0.5 * full && half < full,
        "{half} of {full}: ease-out"
    );
    s.clip_drop_tick(&g, ms(t0, 120));
    assert!((s.arrange.clip_drop.gap_of(key) - full).abs() < 0.01);
    // The rows from the seam down move by a track's height; the ones above stay.
    assert!((s.arrange.clip_drop.shift_of(row.y) - full).abs() < 0.01);
    assert_eq!(s.arrange.clip_drop.shift_of(above.y), 0.0);
    assert!((s.row_y(&g, &row) - (g.column.y + row.y + full - s.scroll.1)).abs() < 0.01);
    assert!((s.row_y(&g, &above) - (g.column.y + above.y - s.scroll.1)).abs() < 0.01);

    // Anywhere in the gap is the same seam.
    for dy in [full * 0.25, full * 0.5, full - 2.0] {
        hold(&mut s, &g, x, screen_y(&g, &row, dy), ms(t0, 130), &paths);
        assert!(
            matches!(s.arrange.clip_drop.aim, Some(Aim::Seam { seam, .. }) if seam.key == key),
            "{dy}"
        );
        assert_eq!(s.arrange.clip_drop.live.live().map(|l| l.spot), Some(key));
    }

    // On the track again (where it shows now, moved down): the seam goes after its grace and
    // the gap closes.
    let t1 = ms(t0, 140);
    hold(
        &mut s,
        &g,
        x,
        screen_y(&g, &row, full + row.h / 2.0),
        t1,
        &paths,
    );
    assert_eq!(
        s.arrange.clip_drop.aim,
        Some(Aim::Track {
            track: track_of(&row),
            at: s.clip_drop_at(&g, x, false)
        })
    );
    s.clip_drop_tick(&g, ms(t1, 60));
    assert!(s.arrange.clip_drop.live.live().is_none());
    s.clip_drop_tick(&g, ms(t1, 60 + 120));
    assert!(!s.arrange.clip_drop.has_gaps());
    assert_eq!(s.arrange.clip_drop.shift_of(row.y), 0.0);

    // Let go of elsewhere: everything goes at once.
    hold(&mut s, &g, x, screen_y(&g, &row, 1.0), ms(t1, 1000), &paths);
    s.drop_over = false;
    assert!(s.clip_drop_tick(&g, ms(t1, 1010)));
    let d = &s.arrange.clip_drop;
    assert!(d.paths.is_empty() && !d.has_gaps() && d.aim.is_none() && d.live.live().is_none());
}

#[test]
fn over_an_instrument_track_the_file_goes_onto_a_new_track_right_below_it() {
    let dir = folder("instrument");
    let vox = tone(dir.path(), "Vox.wav", 0.5);
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let row = instrument_not_last(&s);
    let (parent, i) = s.project().find_node(node_of(row.kind)).unwrap();
    let (x, y) = (mid_x(&g), screen_y(&g, &row, row.h / 2.0));
    // Even a fast pointer: the boundary is what the whole row stands for.
    let t0 = Instant::now();
    hold(
        &mut s,
        &g,
        x,
        screen_y(&g, &row, -40.0),
        t0,
        std::slice::from_ref(&vox),
    );
    hold(&mut s, &g, x, y, ms(t0, 5), std::slice::from_ref(&vox));
    let d = &s.arrange.clip_drop;
    assert_eq!(d.live.live().map(|l| l.spot), Some((parent, i + 1)));
    assert!(
        matches!(d.aim, Some(Aim::Seam { seam, .. }) if seam.key == (parent, i + 1) && !seam.bottom)
    );
    assert!(!s.clip_drop_refused());
    let tracks = s.project().tracks.len();
    assert!(s.import_audio_at(&g, x, y, &[vox.as_path()]));
    s.settle_browser_import();
    assert_eq!(s.error, None);
    assert_eq!(s.project().tracks.len(), tracks + 1);
    let new = new_track_at(&s, parent, i + 1);
    assert_eq!(s.project().tracks[&new].kind, TrackKind::Audio);
    assert_eq!(s.selected_clip.map(|c| c.0), Some(new));
    assert!(s.arrange.clip_drop.paths.is_empty(), "the ghost goes");
}

#[test]
fn the_ruler_the_bands_and_off_the_tracks_take_no_file() {
    let dir = folder("refused");
    let vox = tone(dir.path(), "Vox.wav", 0.5);
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let x = mid_x(&g);
    let mut places = vec![(x, g.ruler.y + g.ruler.h / 2.0)];
    for band in [g.section_band, g.tempo] {
        if band.h > 0.0 {
            places.push((x, band.y + band.h / 2.0));
        }
    }
    places.push((g.lanes.right() + 4.0, g.lanes.y + 20.0));
    let before = s.project();
    let t0 = Instant::now();
    for (n, (x, y)) in places.into_iter().enumerate() {
        hold(
            &mut s,
            &g,
            x,
            y,
            ms(t0, 10 * n as u64),
            std::slice::from_ref(&vox),
        );
        assert_eq!(s.arrange.clip_drop.aim, Some(Aim::Refused), "{x},{y}");
        assert!(s.clip_drop_refused());
        assert_eq!(
            crate::pointer::of_studio(&s, &g).style,
            gpui::CursorStyle::OperationNotAllowed
        );
        // Letting go there puts nothing in.
        assert!(!s.import_audio_at(&g, x, y, &[vox.as_path()]));
        s.settle_browser_import();
        assert_eq!(*s.project(), *before);
        assert!(!s.clip_drop_refused(), "the drag is over");
    }
}

#[test]
fn below_the_last_row_nothing_moves_and_the_ghost_row_shows_under_it() {
    let dir = folder("bottom");
    let vox = tone(dir.path(), "Vox.wav", 0.5);
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let last = *s.tree().rows.last().unwrap();
    let y = g.lanes.bottom() - 2.0;
    assert!(
        y > screen_y(&g, &last, last.h + 10.0),
        "room below the rows"
    );
    let t0 = Instant::now();
    hold(&mut s, &g, mid_x(&g), y, t0, std::slice::from_ref(&vox));
    let key = (Parent::Root, s.project().root.len());
    assert!(
        matches!(s.arrange.clip_drop.aim, Some(Aim::Seam { seam, .. }) if seam.key == key && seam.bottom)
    );
    s.clip_drop_tick(&g, ms(t0, 200));
    let full = s.arrange.track_h;
    assert!(
        (s.arrange.clip_drop.gap_of(key) - full).abs() < 0.01,
        "the ghost row"
    );
    assert_eq!(s.arrange.clip_drop.shift_of(last.y), 0.0);
    assert!((s.row_y(&g, &last) - screen_y(&g, &last, 0.0)).abs() < 0.01);
    assert!(s.import_audio_at(&g, mid_x(&g), y, &[vox.as_path()]));
    s.settle_browser_import();
    let new = new_track_at(&s, Parent::Root, key.1);
    assert_eq!(s.selected_clip.map(|c| c.0), Some(new));
}

#[test]
fn a_file_let_go_at_a_seam_is_a_new_track_there_with_its_clip_selected() {
    let dir = folder("seam");
    let kick = tone(dir.path(), "Kick.wav", 0.5);
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let (row, (parent, i)) = audio_below_a_row(&s);
    let (x, y) = (mid_x(&g), screen_y(&g, &row, 1.0));
    let t0 = Instant::now();
    hold(&mut s, &g, x, y, t0, std::slice::from_ref(&kick));
    s.clip_drop_tick(&g, ms(t0, 120));
    let at = s.clip_drop_at(&g, x, false);
    assert!(s.import_audio_at(&g, x, y, &[kick.as_path()]));
    s.settle_browser_import();
    assert_eq!(s.error, None);
    let new = new_track_at(&s, parent, i);
    assert_eq!(
        new_track_at(&s, parent, i + 1),
        track_of(&row),
        "it moved down"
    );
    let (t, c) = s.selected_clip.expect("the clip is selected");
    assert_eq!(t, new);
    let p = s.project();
    assert_eq!(p.tracks[&new].kind, TrackKind::Audio);
    assert_eq!(p.tracks[&new].name, "Kick");
    assert_eq!(p.clip(c).unwrap().1.start, at);
    assert!(!s.arrange.clip_drop.has_gaps());
}

#[test]
fn several_files_stack_on_new_tracks_and_with_alt_go_end_to_end() {
    let dir = folder("several");
    let one = tone(dir.path(), "One.wav", 1.0);
    let two = tone(dir.path(), "Two.wav", 0.5);
    let paths = [one.clone(), two.clone()];
    let files = [one.as_path(), two.as_path()];
    let audio_on = |s: &Studio, t: TrackId| -> Vec<(String, Tick, Tick)> {
        let mut v: Vec<_> = s.project().tracks[&t]
            .clips
            .iter()
            .filter_map(|c| match &c.body {
                ClipBody::Audio { file, .. } if Path::new(file).is_absolute() => {
                    Some((stem(Path::new(file)), c.start, c.length))
                }
                _ => None,
            })
            .collect();
        v.sort_by_key(|c| c.1);
        v
    };

    // On an audio track: the first there, the second on a new track right below it.
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let (row, (parent, i)) = audio_below_a_row(&s);
    let track = track_of(&row);
    let (x, y) = (mid_x(&g), screen_y(&g, &row, row.h / 2.0));
    let at = s.clip_drop_at(&g, x, false);
    hold(&mut s, &g, x, y, Instant::now(), &paths);
    assert!(s.import_audio_at(&g, x, y, &files));
    s.settle_browser_import();
    assert_eq!(s.error, None);
    assert!(
        audio_on(&s, track)
            .iter()
            .any(|c| c.0 == "One" && c.1 == at)
    );
    let below = new_track_at(&s, parent, i + 1);
    assert_ne!(below, track);
    assert_eq!(
        audio_on(&s, below)
            .iter()
            .map(|c| (c.0.as_str(), c.1))
            .collect::<Vec<_>>(),
        [("Two", at)]
    );

    // At a seam with Alt: one new track, the second clip where the first ends.
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let tracks = s.project().tracks.len();
    let y = screen_y(&g, &row, 1.0);
    let t0 = Instant::now();
    hold(&mut s, &g, x, y, t0, &paths);
    s.clip_drop_tick(&g, ms(t0, 120));
    s.arrange.clip_drop.alt = true;
    assert!(s.import_audio_at(&g, x, y, &files));
    s.settle_browser_import();
    assert_eq!(s.error, None);
    assert_eq!(s.project().tracks.len(), tracks + 1);
    let new = new_track_at(&s, parent, i);
    let clips = audio_on(&s, new);
    assert_eq!(clips.len(), 2, "{clips:?}");
    assert_eq!((clips[0].0.as_str(), clips[0].1), ("One", at));
    assert_eq!(clips[1].0, "Two");
    assert_eq!(clips[1].1, clips[0].1 + clips[0].2, "end to end");
}

#[test]
fn the_start_snaps_to_the_grid_and_shift_lets_it_off() {
    let dir = folder("shift");
    let kick = tone(dir.path(), "Kick.wav", 0.5);
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let (row, _) = audio_below_a_row(&s);
    let track = track_of(&row);
    let bar3 = s.project().time_map().bar_start(3);
    let x = s.tick_x(&g, bar3 + 37);
    let free = s.tick_at(&g, x);
    let snapped = s.snap_tick(free, false);
    assert_ne!(free, snapped, "off the grid");
    let y = screen_y(&g, &row, row.h / 2.0);
    assert!(s.clip_drop_moved(
        &g,
        x,
        y,
        false,
        Instant::now(),
        std::slice::from_ref(&kick),
        true
    ));
    assert_eq!(
        s.arrange.clip_drop.aim,
        Some(Aim::Track { track, at: snapped })
    );
    assert!(s.clip_drop_moved(
        &g,
        x,
        y,
        true,
        Instant::now(),
        std::slice::from_ref(&kick),
        true
    ));
    assert_eq!(
        s.arrange.clip_drop.aim,
        Some(Aim::Track { track, at: free })
    );
    assert!(s.import_audio_at(&g, x, y, &[kick.as_path()]));
    s.settle_browser_import();
    let (_, c) = s.selected_clip.unwrap();
    assert_eq!(s.project().clip(c).unwrap().1.start, free);
}

#[test]
fn the_ghost_is_named_after_the_file_and_a_bar_long_until_it_is_read() {
    let s = Studio::sample();
    let tm = s.project().time_map();
    let bar = tm.bar_start(2) - tm.bar_start(1);
    assert_eq!(s.clip_drop_length(Path::new("no-such-file.wav"), 0), bar);
    assert_eq!(stem(Path::new("loops/Kick 120bpm.wav")), "Kick 120bpm");
    assert_eq!(stem(Path::new("")), "Audio");
    assert!((GHOST_ALPHA - 0.62).abs() < f32::EPSILON);
}

/// The sample with one clip selected, as a click on it leaves it.
fn with_a_selected_clip() -> (Studio, Geometry, (TrackId, ClipId)) {
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let p = s.project();
    let (t, c) = p
        .tracks
        .values()
        .find_map(|t| t.clips.first().map(|c| (t.id, c.id)))
        .expect("a clip");
    s.arrange.clips.insert((t, c));
    s.selected_clip = Some((t, c));
    (s, g, (t, c))
}

#[test]
fn esc_lets_go_of_the_selection_once_nothing_else_is_called_off() {
    let (mut s, g, sel) = with_a_selected_clip();
    // A zoom box waiting goes first.
    s.input_map.zoom_rect_armed = true;
    assert!(s.mapped_key(&g, "escape", None, Mods::NONE));
    assert!(!s.input_map.zoom_rect_armed);
    assert!(s.arrange.clips.contains(&sel));
    // Then the selection; the editor keeps its clip, which no longer shows as selected.
    assert!(s.mapped_key(&g, "escape", None, Mods::NONE));
    assert!(s.arrange.clips.is_empty());
    assert_eq!(s.selected_clip, Some(sel));

    // A file dragged from the browser goes first, with its ghost.
    let (mut s, g, sel) = with_a_selected_clip();
    let dir = folder("esc");
    let kick = tone(dir.path(), "Kick.wav", 0.5);
    let (row, _) = audio_below_a_row(&s);
    let (x, y) = (mid_x(&g), screen_y(&g, &row, row.h / 2.0));
    s.browser.file_drag = Some(crate::browser_pick::FileDrag {
        path: kick.clone(),
        name: "Kick".into(),
        from: (x - 200.0, y),
        at: (x, y),
        moved: true,
    });
    assert!(s.clip_drop_moved(&g, x, y, false, Instant::now(), &[kick], false));
    assert!(s.mapped_key(&g, "escape", None, Mods::NONE));
    assert!(s.browser.file_drag.is_none());
    assert!(s.arrange.clip_drop.paths.is_empty(), "the ghost goes");
    assert!(s.arrange.clips.contains(&sel));
}

#[test]
fn a_press_below_the_last_row_lets_go_of_the_selection() {
    let (mut s, g, sel) = with_a_selected_clip();
    let last = *s.tree().rows.last().unwrap();
    let y = g.lanes.bottom() - 2.0;
    assert!(y > screen_y(&g, &last, last.h));
    s.arrange_down(&g, mid_x(&g), y, Mods::NONE);
    assert!(s.arrange.clips.is_empty());
    assert_eq!(s.selected_clip, Some(sel), "the editor keeps its clip");

    // With Shift the clips stay, as on an empty lane.
    let (mut s, g, sel) = with_a_selected_clip();
    let shift = Mods {
        shift: true,
        ..Mods::NONE
    };
    s.arrange_down(&g, mid_x(&g), y, shift);
    assert!(s.arrange.clips.contains(&sel));
}
