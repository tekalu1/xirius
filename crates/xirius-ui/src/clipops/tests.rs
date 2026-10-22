//! The selecting and clipboard keys, pressed as the window sends them, on the sample song.

use serde_json::json;
use xirius_core::clipboard::Clipboard;
use xirius_core::time::Tick;

use crate::arrange::probe::{clip_mid, point_xy, track, two_lanes};
use crate::studio::{Geometry, Studio};
use crate::tools::{Mods, SnapMode};

const BAR: Tick = 3840;

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.snap_mode = SnapMode::Bar;
    s.drawer_open = false;
    let g = s.geometry(1440.0, 1000.0);
    s.resized(&g);
    (s, g)
}

fn key(s: &mut Studio, g: &Geometry, k: &str, m: Mods) {
    assert!(s.edit_key(g, k, m), "{k}");
    assert!(s.error.is_none(), "{k}: {:?}", s.error);
}

fn click(s: &mut Studio, g: &Geometry, at: (f32, f32), m: Mods) {
    s.arrange_down(g, at.0, at.1, m);
    s.arrange_up(g, at.0, at.1, m);
}

fn count(s: &Studio) -> usize {
    s.project().tracks.values().map(|t| t.clips.len()).sum()
}

fn on(s: &Studio, name: &str) -> Vec<(Tick, Tick)> {
    let t = track(s, name);
    s.project().tracks[&t]
        .clips
        .iter()
        .map(|c| (c.start, c.end()))
        .collect()
}

fn last(s: &Studio) -> String {
    s.song
        .history()
        .into_iter()
        .rfind(|h| h.current)
        .map(|h| h.command)
        .unwrap_or_default()
}

/// The steps in force (an undone one is not counted, a new step drops the undone ones).
fn done(s: &Studio) -> usize {
    s.song.history().iter().filter(|h| !h.undone).count() - 1
}

fn pick(s: &mut Studio, g: &Geometry, name: &str, i: usize) {
    let at = clip_mid(s, g, name, i);
    click(s, g, at, Mods::NONE);
    assert!(s.arrange.focus);
    assert_eq!(s.arrange.clips.len(), 1);
}

fn ctrl() -> Mods {
    Mods::ctrl()
}

fn ctrl_shift() -> Mods {
    Mods::ctrl_shift()
}

#[test]
fn a_copied_clip_is_pasted_at_the_playhead_and_one_undo_takes_it_back() {
    let (mut s, g) = studio();
    pick(&mut s, &g, "Keys", 0);
    let (n, steps) = (count(&s), done(&s));
    key(&mut s, &g, "c", ctrl());
    // Copying changes nothing in the song, and is no step of the history.
    assert_eq!((count(&s), done(&s)), (n, steps));
    assert!(matches!(
        s.song.session().state.clipboard,
        Some(Clipboard::Clips(_))
    ));
    let before = on(&s, "Keys");
    s.seek(40 * BAR);
    key(&mut s, &g, "v", ctrl());
    assert_eq!(count(&s), n + 1);
    assert_eq!(done(&s), steps + 1);
    assert_eq!(last(&s), "edit.paste");
    assert!(on(&s, "Keys").iter().any(|c| c.0 == 40 * BAR));
    // The pasted clip is the selection.
    assert_eq!(s.arrange.clips.len(), 1);
    key(&mut s, &g, "z", ctrl());
    assert_eq!(on(&s, "Keys"), before);
    // The clipboard stays, so a second paste works.
    key(&mut s, &g, "v", ctrl());
    assert_eq!(count(&s), n + 1);
}

#[test]
fn ctrl_shift_v_puts_the_clip_back_where_it_was() {
    let (mut s, g) = studio();
    pick(&mut s, &g, "Keys", 0);
    key(&mut s, &g, "c", ctrl());
    s.seek(30 * BAR);
    let n = count(&s);
    key(&mut s, &g, "v", ctrl_shift());
    assert_eq!(count(&s), n + 1);
    let spots = on(&s, "Keys");
    let first = spots[0];
    assert_eq!(spots.iter().filter(|c| **c == first).count(), 2);
}

#[test]
fn a_cut_clip_goes_and_comes_back_by_one_undo() {
    let (mut s, g) = studio();
    pick(&mut s, &g, "Keys", 0);
    let (n, steps, before) = (count(&s), done(&s), on(&s, "Keys"));
    key(&mut s, &g, "x", ctrl());
    assert_eq!(count(&s), n - 1);
    assert_eq!(done(&s), steps + 1);
    assert_eq!(last(&s), "edit.cut");
    assert!(s.arrange.clips.is_empty());
    s.seek(20 * BAR);
    key(&mut s, &g, "v", ctrl());
    assert_eq!(count(&s), n);
    key(&mut s, &g, "z", ctrl());
    key(&mut s, &g, "z", ctrl());
    assert_eq!(on(&s, "Keys"), before);
}

#[test]
fn nothing_to_paste_says_so_and_changes_nothing() {
    let (mut s, g) = studio();
    s.arrange.focus = true;
    let steps = done(&s);
    key(&mut s, &g, "v", ctrl());
    assert_eq!(done(&s), steps);
    assert!(s.notice.is_some());
}

#[test]
fn select_all_invert_none_on_tracks_and_in_loop_follow_the_core_rules() {
    let (mut s, g) = studio();
    pick(&mut s, &g, "Keys", 0);
    let n = count(&s);
    let steps = done(&s);
    key(&mut s, &g, "a", ctrl());
    assert_eq!(s.arrange.clips.len(), n);
    key(&mut s, &g, "i", ctrl());
    assert!(s.arrange.clips.is_empty());
    key(&mut s, &g, "i", ctrl());
    assert_eq!(s.arrange.clips.len(), n);
    key(&mut s, &g, "d", ctrl());
    assert!(s.arrange.clips.is_empty());
    // Selecting is no step of the history.
    assert_eq!(done(&s), steps);

    // One clip selected: invert leaves every other clip.
    pick(&mut s, &g, "Keys", 0);
    key(&mut s, &g, "i", ctrl());
    assert_eq!(s.arrange.clips.len(), n - 1);

    // The clips on the track of the selected one.
    pick(&mut s, &g, "Keys", 0);
    key(&mut s, &g, "a", ctrl_shift());
    let t = track(&s, "Keys");
    assert_eq!(s.arrange.clips.len(), s.project().tracks[&t].clips.len());
    assert!(s.arrange.clips.iter().all(|(k, _)| *k == t));

    // In the loop: said when there is no loop; the overlapping clips once there is.
    key(&mut s, &g, "d", ctrl());
    s.edit_key(&g, "l", Mods::alt());
    assert!(s.error.is_some());
    s.error = None;
    s.song
        .run("loop.set", json!({ "start": 0, "end": BAR }))
        .unwrap();
    key(&mut s, &g, "l", Mods::alt());
    assert!(!s.arrange.clips.is_empty());
    let p = s.project();
    for (t, c) in &s.arrange.clips {
        let c = p.tracks[t].clips.iter().find(|k| k.id == *c).unwrap();
        assert!(c.start < BAR);
    }
}

#[test]
fn automation_points_are_picked_copied_pasted_duplicated_and_deleted() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Keys");
    let lane = |s: &Studio| -> Vec<Tick> {
        s.project().tracks[&t]
            .automation
            .iter()
            .find(|l| l.target.to_string() == "volume")
            .unwrap()
            .points
            .iter()
            .map(|p| p.tick)
            .collect()
    };
    let start = lane(&s);
    assert_eq!(start.len(), 3);

    // A press on a point selects it; Shift adds the next.
    let a = point_xy(&s, &g, t, 0, 0);
    click(&mut s, &g, a, Mods::NONE);
    assert_eq!(s.arrange.points.ticks().len(), 1);
    let b = point_xy(&s, &g, t, 0, 1);
    click(&mut s, &g, b, Mods::shift());
    assert_eq!(s.arrange.points.ticks().len(), 2);

    // Copy, then paste at the playhead: the first lands there.
    let steps = done(&s);
    key(&mut s, &g, "c", ctrl());
    assert_eq!(done(&s), steps);
    s.seek(20 * BAR);
    key(&mut s, &g, "v", ctrl());
    assert_eq!(done(&s), steps + 1);
    assert_eq!(last(&s), "edit.paste");
    let after = lane(&s);
    assert_eq!(after.len(), 5);
    assert!(after.contains(&(20 * BAR)));
    // The pasted points are the selection.
    assert!(s.arrange.points.ticks().contains(&(20 * BAR)));
    key(&mut s, &g, "z", ctrl());
    assert_eq!(lane(&s), start);

    // Delete takes the selected points, one step.
    let a = point_xy(&s, &g, t, 0, 0);
    click(&mut s, &g, a, Mods::NONE);
    let steps = done(&s);
    key(&mut s, &g, "delete", Mods::NONE);
    assert_eq!(done(&s), steps + 1);
    assert_eq!(lane(&s).len(), 2);
    key(&mut s, &g, "z", ctrl());
    assert_eq!(lane(&s), start);

    // D copies them to the right.
    let a = point_xy(&s, &g, t, 0, 0);
    click(&mut s, &g, a, Mods::NONE);
    key(&mut s, &g, "d", Mods::NONE);
    assert_eq!(last(&s), "edit.duplicate");
    assert_eq!(lane(&s).len(), 4);
    key(&mut s, &g, "z", ctrl());
    assert_eq!(lane(&s), start);

    // Ctrl+A picks every point of the lane; cut takes them all.
    let a = point_xy(&s, &g, t, 0, 0);
    click(&mut s, &g, a, Mods::NONE);
    key(&mut s, &g, "a", ctrl());
    assert_eq!(s.arrange.points.ticks().len(), 3);
    key(&mut s, &g, "x", ctrl());
    assert_eq!(lane(&s).len(), 0);
    key(&mut s, &g, "z", ctrl());
    assert_eq!(lane(&s), start);
}

#[test]
fn a_press_elsewhere_lets_the_points_go() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Keys");
    let a = point_xy(&s, &g, t, 0, 0);
    click(&mut s, &g, a, Mods::NONE);
    assert!(!s.arrange.points.is_empty());
    pick(&mut s, &g, "Bass", 0);
    assert!(s.arrange.points.is_empty());
}

#[test]
fn clips_cannot_go_into_the_roll_and_say_so() {
    let (mut s, g) = studio();
    pick(&mut s, &g, "Keys", 0);
    key(&mut s, &g, "c", ctrl());
    // The roll is open and has the focus (a press in it).
    s.drawer_open = true;
    let g = s.geometry(1440.0, 820.0);
    s.resized(&g);
    assert!(s.roll_input(&g).is_some());
    s.arrange.focus = false;
    let steps = done(&s);
    key(&mut s, &g, "v", ctrl());
    assert_eq!(done(&s), steps);
    assert!(s.notice.is_some());
}

#[test]
fn every_new_key_is_a_row_with_its_chip_and_runs_the_same_command() {
    let (mut s, g) = studio();
    pick(&mut s, &g, "Keys", 0);
    key(&mut s, &g, "c", ctrl());
    let rows = s.palette_rows(&g);
    for (name, chip) in [
        ("edit.copy", "Ctrl+C"),
        ("edit.cut", "Ctrl+X"),
        ("edit.paste", "Ctrl+V"),
        ("select.all", "Ctrl+A"),
        ("select.clear", "Ctrl+D"),
        ("select.invert", "Ctrl+I"),
        ("select.on-tracks", "Ctrl+Shift+A"),
        ("select.in-loop", "Alt+L"),
    ] {
        let r = rows.iter().find(|r| r.name == name).expect(name);
        assert!(
            r.key.as_deref().is_some_and(|text| text.contains(chip)),
            "{name}"
        );
    }
    let by = |n: &str| rows.iter().find(|r| r.name == n).unwrap().enabled;
    assert!(by("edit.copy") && by("edit.paste") && by("select.all"));
    // No loop yet: that row waits.
    assert!(!by("select.in-loop"));
    let paste = rows
        .iter()
        .find(|r| r.name == "edit.paste")
        .unwrap()
        .clone();
    let n = count(&s);
    assert!(s.run_row(&g, &paste));
    assert_eq!(count(&s), n + 1);
    assert_eq!(last(&s), "edit.paste");
}
