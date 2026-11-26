use serde_json::json;
use xirius_core::id::TrackId;

use super::*;
use crate::studio::{DrawerTab, Geometry, Hit};
use crate::tools::Mods;
use crate::tree::RowKind;

const W: f32 = 1600.0;
const H: f32 = 1000.0;

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .unwrap_or_else(|| panic!("no track {name}"))
}

/// The sample song with the mixer open, Keys at -12 dB, Vocal at -6 dB and Guitar at +6 dB,
/// the three of them selected.
fn three() -> (Studio, Geometry, [TrackId; 3]) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    // tall enough that the output's menu opens clear of the next strip's fader
    s.drawer_size.mixer = Some(600.0);
    let g = s.geometry(W, H);
    let ids = [track(&s, "Keys"), track(&s, "Vocal"), track(&s, "Guitar")];
    for (t, db) in ids.iter().zip([-12.0, -6.0, 6.0]) {
        assert!(s.run_command(
            "track.volume.set",
            json!({ "ids": [t.to_string()], "db": db })
        ));
    }
    for (t, pan) in ids.iter().zip([-0.5, 0.0, 0.8]) {
        assert!(s.run_command(
            "track.pan.set",
            json!({ "ids": [t.to_string()], "pan": pan })
        ));
    }
    s.pick_row(RowKind::Track(ids[0]), Mods::NONE);
    s.pick_row(
        RowKind::Track(ids[2]),
        Mods {
            shift: true,
            ..Mods::NONE
        },
    );
    assert_eq!(s.picked_tracks().len(), 3);
    (s, g, ids)
}

/// Presses `hit` as the window does: the arrangement takes the press's modifiers first, then
/// the press itself and the start of its drag. True when a drag began.
fn window_press(s: &mut Studio, g: &Geometry, hit: MixerHit, mods: Mods) -> bool {
    let (x, y) = s.mixer_center(g, hit).expect("on screen");
    assert!(!s.arrange_down(g, x, y, mods));
    let h = s.hit(g, x, y);
    assert_eq!(h, Hit::Mixer(hit));
    s.press(g, h);
    s.drag_begin(g, h, y)
}

/// Presses `hit` as the window does and starts its drag.
fn grab(s: &mut Studio, g: &Geometry, hit: MixerHit, mods: Mods) {
    assert!(window_press(s, g, hit, mods));
}

fn set_db(s: &mut Studio, t: TrackId, db: f64) {
    assert!(s.run_command(
        "track.volume.set",
        json!({ "ids": [t.to_string()], "db": db })
    ));
}

fn shown(s: &Studio, ids: &[TrackId; 3], hit: fn(TrackId) -> MixerHit) -> Vec<f64> {
    ids.iter()
        .map(|t| s.mixer_value(hit(*t)).unwrap())
        .collect()
}

fn volumes(s: &Studio, ids: &[TrackId; 3]) -> Vec<f64> {
    ids.iter()
        .map(|t| s.project().tracks[t].volume_db)
        .collect()
}

fn pans(s: &Studio, ids: &[TrackId; 3]) -> Vec<f64> {
    ids.iter().map(|t| s.project().tracks[t].pan).collect()
}

fn near(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

/// In a row of the table below, the value the screen showed before the grab.
const HELD: f64 = f64::NAN;

/// `want` with each [`HELD`] read as what `before` held there.
fn resolve(want: [f64; 3], before: &[f64]) -> Vec<f64> {
    want.iter()
        .zip(before)
        .map(|(w, b)| if w.is_nan() { *b } else { *w })
        .collect()
}

#[test]
fn a_selected_fader_moves_every_selected_track_and_keeps_their_differences() {
    struct Row {
        name: &'static str,
        setup: fn(&mut Studio, &[TrackId; 3]),
        /// Which of Keys, Vocal and Guitar is grabbed.
        grab: usize,
        relative: bool,
        /// Where the grabbed fader is taken, and what the three faders show then.
        path: &'static [(f64, [f64; 3])],
        /// What the faders show, and the song keeps, once let go.
        after: [f64; 3],
        /// Undoable steps the release adds; one undo takes it back.
        steps: usize,
    }
    let rows = [
        Row {
            // Guitar reaches +12 dB and stops there while the others go on; back again, its
            // difference comes back.
            name: "there and back past the top",
            setup: |_, _| {},
            grab: 1,
            relative: true,
            path: &[
                (-2.0, [-8.0, -2.0, 10.0]),
                (2.0, [-4.0, 2.0, 12.0]),
                (-2.0, [-8.0, -2.0, 10.0]),
            ],
            after: [-8.0, -2.0, 10.0],
            steps: 1,
        },
        Row {
            name: "a track stopped at the end when it lets go stays there",
            setup: |_, _| {},
            grab: 0,
            relative: true,
            path: &[(-2.0, [-2.0, 4.0, 12.0])],
            after: [-2.0, 4.0, 12.0],
            steps: 1,
        },
        Row {
            name: "back to where it began adds no step",
            setup: |_, _| {},
            grab: 1,
            relative: true,
            path: &[(0.0, [-6.0, 0.0, 12.0]), (-6.0, [-12.0, -6.0, 6.0])],
            after: [-12.0, -6.0, 6.0],
            steps: 0,
        },
        Row {
            name: "pulled to the bottom silences every one",
            setup: |_, _| {},
            grab: 1,
            relative: true,
            path: &[(MIN_DB, [MIN_DB; 3])],
            after: [MIN_DB; 3],
            steps: 1,
        },
        Row {
            // Keys at -61 dB would show -∞ and still sound: it is silent.
            name: "one taken below the floor is silent and its difference comes back",
            setup: |_, _| {},
            grab: 1,
            relative: true,
            path: &[
                (-55.0, [MIN_DB, -55.0, -43.0]),
                (-50.0, [-56.0, -50.0, -38.0]),
                (-55.0, [MIN_DB, -55.0, -43.0]),
            ],
            after: [MIN_DB, -55.0, -43.0],
            steps: 1,
        },
        Row {
            name: "one at -∞ stays there while the others move",
            setup: |s, ids| set_db(s, ids[0], MIN_DB),
            grab: 1,
            relative: true,
            path: &[(0.0, [MIN_DB, 0.0, 12.0])],
            after: [MIN_DB, 0.0, 12.0],
            steps: 1,
        },
        Row {
            // Below the fader's floor is silence already: the track keeps its own value.
            name: "one below the floor at -70 dB stays there while the others move",
            setup: |s, ids| set_db(s, ids[0], -70.0),
            grab: 1,
            relative: true,
            path: &[(0.0, [-70.0, 0.0, 12.0])],
            after: [-70.0, 0.0, 12.0],
            steps: 1,
        },
        Row {
            // There is no difference to keep from silence.
            name: "raised from -∞ it moves its track alone",
            setup: |s, ids| set_db(s, ids[1], MIN_DB),
            grab: 1,
            relative: false,
            path: &[(-10.0, [-12.0, -10.0, 6.0])],
            after: [-12.0, -10.0, 6.0],
            steps: 1,
        },
        Row {
            // What the song keeps under the lane is not read: the lane plays.
            name: "one its lane holds stays still on the screen",
            setup: |s, ids| {
                let bar = i64::from(s.project().ppq) * 4;
                for at in [0, 4 * bar] {
                    assert!(s.run_command(
                        "auto.point.add",
                        json!({ "track": ids[0].to_string(), "target": "volume", "at": at, "value": -30.0 })
                    ));
                }
            },
            grab: 1,
            relative: true,
            path: &[(0.0, [HELD, 0.0, 12.0])],
            after: [HELD, 0.0, 12.0],
            steps: 1,
        },
    ];
    let mut failures = Vec::new();
    for row in &rows {
        let (mut s, g, ids) = three();
        (row.setup)(&mut s, &ids);
        let start = volumes(&s, &ids);
        let before = shown(&s, &ids, MixerHit::Fader);
        let steps = s.song().steps();
        let mut bad = Vec::new();
        grab(&mut s, &g, MixerHit::Fader(ids[row.grab]), Mods::NONE);
        if s.drag_relative() != row.relative {
            bad.push(format!("relative is {}", !row.relative));
        }
        for &(to, want) in row.path {
            s.drag_to(to);
            let got = shown(&s, &ids, MixerHit::Fader);
            if !near(&got, &resolve(want, &before)) {
                bad.push(format!("taken to {to}, shows {got:?}"));
            }
        }
        // Nothing reached the song before the release.
        if volumes(&s, &ids) != start {
            bad.push(format!("the song held {:?}", volumes(&s, &ids)));
        }
        if !s.drag_end() {
            bad.push("the release did nothing".into());
        }
        let got = shown(&s, &ids, MixerHit::Fader);
        if !near(&got, &resolve(row.after, &before)) {
            bad.push(format!("let go, shows {got:?}"));
        }
        let kept = volumes(&s, &ids);
        if kept
            .iter()
            .zip(row.after)
            .any(|(k, w)| !w.is_nan() && (k - w).abs() >= 1e-9)
        {
            bad.push(format!("let go, the song keeps {kept:?}"));
        }
        if s.song().steps() != steps + row.steps {
            bad.push(format!("steps {} -> {}", steps, s.song().steps()));
        }
        if s.picked_tracks().len() != 3 {
            bad.push(format!("{} picked", s.picked_tracks().len()));
        }
        if row.steps > 0 {
            s.undo(&g);
            if volumes(&s, &ids) != start {
                bad.push(format!("undone, the song keeps {:?}", volumes(&s, &ids)));
            }
        }
        if !bad.is_empty() {
            failures.push(format!("{}: {bad:?}", row.name));
        }
    }
    assert!(failures.is_empty(), "group fader drag: {failures:#?}");
}

#[test]
fn a_pointer_drag_moves_the_others_by_as_much_as_the_dragged_one() {
    let (mut s, g, ids) = three();
    let hit = MixerHit::Fader(ids[1]);
    let (_, y) = s.mixer_center(&g, hit).unwrap();
    grab(&mut s, &g, hit, Mods::NONE);
    assert!(s.drag_move(y + 20.0, false));
    let now = shown(&s, &ids, MixerHit::Fader);
    let by = now[1] + 6.0;
    assert!(by < 0.0);
    assert!(near(&now, &[-12.0 + by, -6.0 + by, 6.0 + by]), "{now:?}");
    assert!(s.drag_end());
    assert!(near(&volumes(&s, &ids), &now));
}

#[test]
fn alt_moves_the_dragged_track_alone_and_keeps_the_selection() {
    let (mut s, g, ids) = three();
    let alt = Mods {
        alt: true,
        ..Mods::NONE
    };
    grab(&mut s, &g, MixerHit::Fader(ids[1]), alt);
    assert!(!s.drag_relative());
    s.drag_to(-2.0);
    assert_eq!(shown(&s, &ids, MixerHit::Fader), [-12.0, -2.0, 6.0]);
    assert!(s.drag_end());
    assert_eq!(volumes(&s, &ids), [-12.0, -2.0, 6.0]);
    assert_eq!(s.picked_tracks().len(), 3);
}

#[test]
fn a_fader_outside_the_selection_moves_its_track_alone_and_keeps_the_selection() {
    let (mut s, g, ids) = three();
    let bass = track(&s, "Bass");
    let was = s.project().tracks[&bass].volume_db;
    let picked = s.picked_rows();
    grab(&mut s, &g, MixerHit::Fader(bass), Mods::NONE);
    assert!(!s.drag_relative());
    s.drag_to(was - 3.0);
    assert!(s.drag_end());
    assert_eq!(s.project().tracks[&bass].volume_db, was - 3.0);
    assert_eq!(volumes(&s, &ids), [-12.0, -6.0, 6.0]);
    assert_eq!(s.picked_rows(), picked);
}

#[test]
fn a_selected_pan_knob_moves_every_selected_track_and_stops_at_the_side() {
    let (mut s, g, ids) = three();
    let before = s.song().steps();
    grab(&mut s, &g, MixerHit::Pan(ids[1]), Mods::NONE);
    s.drag_to(0.3);
    assert!(near(&shown(&s, &ids, MixerHit::Pan), &[-0.2, 0.3, 1.0]));
    s.drag_to(-0.1);
    assert!(near(&shown(&s, &ids, MixerHit::Pan), &[-0.6, -0.1, 0.7]));
    assert!(s.drag_end());
    assert!(near(&pans(&s, &ids), &[-0.6, -0.1, 0.7]));
    assert_eq!(s.song().steps(), before + 1);
    assert!(s.undo(&g));
    assert!(near(&pans(&s, &ids), &[-0.5, 0.0, 0.8]));
}

#[test]
fn a_pan_moved_with_others_keeps_to_the_hundredths() {
    let pan = MixerHit::Pan(TrackId::MASTER);
    assert_eq!(moved(pan, 0.1, 0.2), 0.3);
    assert_eq!(moved(pan, -0.7, 0.1), -0.6);
}

#[test]
fn an_undo_while_a_fader_is_held_lets_the_drag_go() {
    let (mut s, g, ids) = three();
    set_db(&mut s, ids[1], -3.0);
    grab(&mut s, &g, MixerHit::Fader(ids[1]), Mods::NONE);
    s.drag_to(0.0);
    assert_eq!(shown(&s, &ids, MixerHit::Fader), [-9.0, 0.0, 9.0]);
    // Ctrl+Z takes back the -3 dB; the drag that began from it goes too.
    assert!(s.undo(&g));
    assert!(s.mixer.drag.is_none());
    assert_eq!(volumes(&s, &ids), [-12.0, -6.0, 6.0]);
    assert_eq!(shown(&s, &ids, MixerHit::Fader), [-12.0, -6.0, 6.0]);
    let steps = s.song().steps();
    let (_, y) = s.mixer_center(&g, MixerHit::Fader(ids[1])).unwrap();
    assert!(!s.drag_move(y - 30.0, false));
    s.drag_end();
    assert_eq!(volumes(&s, &ids), [-12.0, -6.0, 6.0]);
    assert_eq!(s.song().steps(), steps);
}

#[test]
fn a_press_while_a_menu_is_open_reads_its_own_modifiers() {
    let (mut s, g, ids) = three();
    let alt = Mods {
        alt: true,
        ..Mods::NONE
    };
    // A menu opens under a press with Alt; Alt is let go before the press that closes it.
    assert!(!window_press(&mut s, &g, MixerHit::Output(ids[0]), alt));
    assert!(s.popup.is_some());
    grab(&mut s, &g, MixerHit::Fader(ids[1]), Mods::NONE);
    assert!(s.drag_relative());
}

#[test]
fn a_press_on_a_strip_outside_the_selection_chooses_only_by_its_name() {
    let (mut s, g, ids) = three();
    let bass = track(&s, "Bass");
    let picked = s.picked_rows();
    for hit in [
        MixerHit::AddInsert(bass),
        MixerHit::AddSend(bass),
        MixerHit::Output(bass),
    ] {
        window_press(&mut s, &g, hit, Mods::NONE);
        assert_eq!(s.picked_rows(), picked, "{hit:?}");
        s.popup = None;
    }
    // The right press on them keeps it too.
    for hit in [
        MixerHit::AddInsert(bass),
        MixerHit::AddSend(bass),
        MixerHit::Mute(bass),
        MixerHit::Arm(bass),
        MixerHit::Output(bass),
    ] {
        let (x, y) = s.mixer_center(&g, hit).unwrap();
        s.scene_context(&g, x, y);
        assert_eq!(s.picked_rows(), picked, "{hit:?}");
        s.popup = None;
    }
    // Inside the selection, a press moves only the focus.
    window_press(&mut s, &g, MixerHit::Output(ids[2]), Mods::NONE);
    s.popup = None;
    assert_eq!(s.picked_tracks().len(), 3);
    assert_eq!(s.selected_row, Some(RowKind::Track(ids[2])));
    // The name chooses as a head does.
    window_press(&mut s, &g, MixerHit::Strip(bass), Mods::NONE);
    assert_eq!(s.picked_rows(), [RowKind::Track(bass)]);
}
