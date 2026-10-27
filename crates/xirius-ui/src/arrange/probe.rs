//! Arrangement gestures for the screenshot run: each stage leaves a drag in
//! the middle, so the picture shows its preview and the value next to the
//! pointer.

use serde_json::json;
use xirius_core::auto::AutoTarget;
use xirius_core::id::{FolderId, TrackId};

use super::{Arrange, LaneButton};
use crate::studio::{Geometry, Hit, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

pub(crate) fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no track {name}"))
        .id
}

pub(crate) fn folder(s: &Studio, name: &str) -> FolderId {
    s.project()
        .folders
        .values()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no folder {name}"))
        .id
}

/// The middle of a track's `i`-th clip.
pub(crate) fn clip_mid(s: &Studio, g: &Geometry, name: &str, i: usize) -> (f32, f32) {
    let t = track(s, name);
    let c = s.project().tracks[&t].clips[i].id;
    s.center_of(g, Hit::Clip(t, c)).expect("clip on screen")
}

/// Where a row's name is in the track column.
pub(crate) fn row_mid(s: &Studio, g: &Geometry, kind: RowKind) -> (f32, f32) {
    s.center_of(g, Hit::Row(kind)).expect("row on screen")
}

/// The fraction `f` of the way down a row (its own height, not its lanes).
pub(crate) fn row_at(s: &Studio, g: &Geometry, kind: RowKind, f: f32) -> f32 {
    let row = *s.tree().row_of(kind).expect("row");
    s.row_y(g, &row) + row.h * f
}

/// Presses at `at` as the window does: the arrangement first, then a head's own press, which
/// chooses the row (UI-340).
pub(crate) fn down(s: &mut Studio, g: &Geometry, at: (f32, f32), mods: Mods) {
    if !s.arrange_down(g, at.0, at.1, mods) {
        let hit = s.hit(g, at.0, at.1);
        if matches!(hit, Hit::Row(_) | Hit::TrackMark(_)) {
            s.press(g, hit);
        }
    }
}

/// Presses at `from` and moves to `to` without letting go.
pub(crate) fn hold(s: &mut Studio, g: &Geometry, from: (f32, f32), to: (f32, f32), mods: Mods) {
    s.arrange.drag = None;
    down(s, g, from, mods);
    // Two steps, so the drag starts before it reaches the end.
    let mid = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    s.arrange_move(g, mid.0, mid.1, mods);
    s.arrange_move(g, to.0, to.1, mods);
}

fn bars(s: &Studio, n: f32) -> f32 {
    s.ticks_to_px(i64::from(s.project().ppq) * 4) * n
}

/// A track with volume and pan lanes open, and a few points on each.
pub(crate) fn two_lanes(s: &mut Studio, g: &Geometry, name: &str) -> TrackId {
    let t = track(s, name);
    let tid = t.to_string();
    if !s.arrange.open.contains(&t) {
        s.toggle_lanes(g, t);
    }
    let ppq = i64::from(s.project().ppq);
    let bar = ppq * 4;
    for (at, v) in [(4 * bar, -24.0), (6 * bar, 0.0), (8 * bar, -18.0)] {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        s.arrange_run(
            "auto.point.add",
            json!({ "track": tid, "target": "volume", "at": at, "value": v }),
        );
    }
    s.lane_button(g, t, &AutoTarget::Volume, LaneButton::Add);
    s.popup_action(
        g,
        crate::popup::Action::AutomationTarget(t, None, "pan".into()),
    );
    for (at, v) in [(4 * bar, -0.6), (8 * bar, 0.5)] {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        s.arrange_run(
            "auto.point.add",
            json!({ "track": tid, "target": "pan", "at": at, "value": v }),
        );
    }
    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
    s.arrange_run("auto.mode", json!({ "ids": [tid], "mode": "read" }));
    s.rebuild(g);
    t
}

/// One stage of the run. Every stage starts with nothing held.
pub fn shot(s: &mut Studio, g: &Geometry, stage: u8) {
    s.arrange.drag = None;
    s.arrange.hover = super::Spot::None;
    // The arrangement alone, so every row fits.
    s.drawer_open = false;
    let g = &s.geometry(g.size.0, g.size.1);
    s.rebuild(g);
    let none = Mods::default();
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    match stage {
        // A clip moved two bars later, the balloon showing where it starts.
        0 => {
            let from = clip_mid(s, g, "Keys", 0);
            hold(s, g, from, (from.0 + bars(s, 2.0), from.1), none);
        }
        // Alt + drag: a copy of the drum clip, later.
        1 => {
            let from = clip_mid(s, g, "Drums", 0);
            hold(s, g, from, (from.0 + bars(s, 4.0), from.1), alt);
        }
        // The loop's end dragged four bars later.
        2 => {
            let bar = i64::from(s.project().ppq) * 4;
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            s.arrange_run("loop.set", json!({ "start": 4 * bar, "end": 8 * bar }));
            let x = s.tick_x(g, 8 * bar);
            let y = g.ruler.y + Arrange::LOOP_H / 2.0;
            hold(s, g, (x, y), (x + bars(s, 4.0), y), none);
        }
        // Keys dragged up between Drums and Bass: the line between rows.
        3 => {
            let keys = RowKind::Track(track(s, "Keys"));
            let from = row_mid(s, g, keys);
            let y = row_at(s, g, RowKind::Track(track(s, "Bass")), 0.2);
            hold(s, g, from, (from.0, y), none);
        }
        // Keys dragged onto the Percussion folder: the folder lights up.
        4 => {
            let keys = RowKind::Track(track(s, "Keys"));
            let from = row_mid(s, g, keys);
            let y = row_at(s, g, RowKind::Folder(folder(s, "Percussion")), 0.5);
            hold(s, g, from, (from.0, y), none);
        }
        // Two automation lanes on Bass, a volume point being dragged.
        5 => {
            let t = two_lanes(s, g, "Bass");
            let (x, y) = point_xy(s, g, t, 0, 1);
            hold(s, g, (x, y), (x + bars(s, 0.5), y - 12.0), none);
        }
        // The line between two volume points bent.
        6 => {
            let t = track(s, "Bass");
            let (x0, _) = point_xy(s, g, t, 0, 1);
            let (x1, _) = point_xy(s, g, t, 0, 2);
            let x = (x0 + x1) / 2.0;
            let row = *s.tree().row_of(RowKind::Track(t)).expect("row");
            let top = s.lane_top(g, &row, 0);
            let lane = s.project().tracks[&t].automation[0].clone();
            let v = lane.curve_at(s.tick_at(g, x) as f64).unwrap_or(0.0);
            let y = top + super::lane_value_y(&lane.target, v);
            hold(s, g, (x, y), (x, y + 18.0), none);
        }
        // The Intro section dragged past Verse.
        7 => {
            let p = s.project();
            let intro = &p.sections[0];
            let x = s.tick_x(g, intro.start + intro.length / 2);
            let y = g.section_band.y + g.section_band.h / 2.0;
            hold(s, g, (x, y), (x + bars(s, 7.0), y), none);
        }
        // A tempo change at bar 9 dragged up.
        8 => {
            let bar = i64::from(s.project().ppq) * 4;
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            s.arrange_run("tempo.set", json!({ "bpm": 128.0, "at": 8 * bar }));
            let p = s.project();
            let i = p
                .tempo
                .iter()
                .position(|t| t.tick == 8 * bar)
                .expect("tempo");
            let (x, y) = s.tempo_xy(g, p.tempo[i].tick, p.tempo[i].bpm);
            hold(s, g, (x, y), (x, y - 16.0), none);
        }
        // Taller tracks: Ctrl + wheel over the rows.
        9 => {
            let ctrl = Mods {
                ctrl: true,
                ..Mods::default()
            };
            let at = row_mid(s, g, RowKind::Track(track(s, "Keys")));
            s.arrange_wheel(g, g.lanes.x + 200.0, at.1, 4.0, ctrl);
        }
        // Volume and pan lanes on Bass, played in Read; nothing held.
        10 => {
            two_lanes(s, g, "Bass");
        }
        _ => {}
    }
}

/// Where a lane's point is on screen.
pub(crate) fn point_xy(s: &Studio, g: &Geometry, t: TrackId, lane: usize, i: usize) -> (f32, f32) {
    let row = *s.tree().row_of(RowKind::Track(t)).expect("row");
    let track = &s.project().tracks[&t];
    let l = &s.arrange.lanes_of(track)[lane];
    let p = l.points[i];
    (
        s.tick_x(g, p.tick),
        s.lane_top(g, &row, lane) + super::lane_value_y(&l.target, p.value),
    )
}
