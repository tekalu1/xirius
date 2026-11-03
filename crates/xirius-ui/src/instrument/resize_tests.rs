//! UI-331: a window's corner takes its size; the main graph grows, the
//! controls keep theirs, and each kind of device opens at its last size.

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

use super::window::{GRIP, Part};
use crate::device::{compressor, eq, layout};
use crate::studio::{Geometry, Studio};
use crate::title_bar;

/// The sample song with an EQ and a Compressor on Keys.
fn two() -> (Studio, Geometry, TrackId, [PluginId; 2]) {
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let t = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    let ids = ["EQ", "Compressor"].map(|name| {
        assert!(s.run_command(
            "plugin.add",
            json!({ "track": t.to_string(), "name": name }),
        ));
        s.project().tracks[&t]
            .inserts
            .iter()
            .find(|x| x.name == name)
            .unwrap()
            .id
    });
    (s, g, t, ids)
}

/// The body of the top window, on the screen.
fn body(s: &Studio, g: &Geometry) -> layout::Body {
    let i = s.instruments.open.len() - 1;
    let w = s.instruments.open[i];
    let p = s.project();
    let plugin = p.tracks[&w.track]
        .inserts
        .iter()
        .find(|x| x.id == w.plugin)
        .unwrap();
    let r = s.instruments.frame(i, g);
    s.instruments.body(r, plugin, plugin.builtin().unwrap())
}

/// Drags the top window's corner by `(dx, dy)` and lets go.
fn drag(s: &mut Studio, g: &Geometry, id: PluginId, dx: f32, dy: f32) {
    let (x, y) = s
        .instrument_center(g, id, Part::Resize)
        .expect("the corner");
    assert!(s.instrument_down(g, x, y));
    for i in 1..=8 {
        let f = i as f32 / 8.0;
        s.instrument_moved(g, x + dx * f, y + dy * f, false);
    }
    assert!(s.instrument_up());
}

#[test]
fn ui_331_the_corner_is_a_part_of_its_own() {
    let (mut s, g, t, [eq, _]) = two();
    s.show_instrument(t, eq);
    let r = s.instruments.frame(0, &g);
    let p = s.project();
    let at = |x, y| s.instruments.at(&p, &g, x, y).map(|h| h.1);
    assert_eq!(at(r.right() - 2.0, r.bottom() - 2.0), Some(Part::Resize));
    assert_ne!(
        at(r.right() - GRIP - 4.0, r.bottom() - 2.0),
        Some(Part::Resize)
    );
    // The pointer shows the corner's arrows over it.
    assert!(s.instrument_hover(&g, r.right() - 2.0, r.bottom() - 2.0));
    assert!(s.instruments.sizing());
    s.instrument_hover(&g, r.x + r.w / 2.0, r.y + 4.0);
    assert!(!s.instruments.sizing());
}

#[test]
fn ui_331_a_bigger_window_grows_the_curve_and_keeps_the_knobs() {
    let (mut s, g, t, [eq, _]) = two();
    s.show_instrument(t, eq);
    let r0 = s.instruments.frame(0, &g);
    let b0 = body(&s, &g);
    let steps = s.song().steps();
    drag(&mut s, &g, eq, 200.0, 150.0);
    let r1 = s.instruments.frame(0, &g);
    assert_eq!((r1.x, r1.y), (r0.x, r0.y), "the corner opposite stays");
    assert_eq!((r1.w, r1.h), (r0.w + 200.0, r0.h + 150.0));
    let b1 = body(&s, &g);
    assert_eq!(b1.graphs[0].w, b0.graphs[0].w + 200.0);
    assert_eq!(b1.graphs[0].h, b0.graphs[0].h + 150.0);
    assert_eq!(b1.slots.len(), b0.slots.len());
    for (a, b) in b0.slots.iter().zip(&b1.slots) {
        // The dials keep their size; the band's columns only spread out.
        assert_eq!((a.cell.h, a.dial), (b.cell.h, b.dial));
        assert_eq!(b.cell.y, a.cell.y + 150.0, "moved down with the foot");
    }
    // The output's meter beside it stretches to its knob.
    assert_eq!(b1.meters[0].h, b0.meters[0].h + 150.0);
    // The body still ends a pad under its controls.
    let last = b1.slots.iter().map(|x| x.cell.bottom()).fold(0.0, f32::max);
    assert!(last <= r1.bottom() && last >= r1.bottom() - layout::PAD - 24.0);
    // A size is the screen's, not the song's.
    assert_eq!(s.song().steps(), steps);
}

#[test]
fn ui_331_it_goes_no_smaller_than_the_device_and_its_controls() {
    let (mut s, g, t, [eq, _]) = two();
    s.show_instrument(t, eq);
    drag(&mut s, &g, eq, -900.0, -900.0);
    let def = xirius_params::find("eq").unwrap();
    let (w, h) = s.instruments.open[0].body;
    assert_eq!(w, eq::MIN_W);
    assert_eq!((w, h), layout::size(def, eq::MIN_W), "the narrow layout");
    // Nor past the screen's edge.
    drag(&mut s, &g, eq, 5000.0, 5000.0);
    let r = s.instruments.frame(0, &g);
    assert!(r.right() <= g.size.0 + 0.5 && r.bottom() <= g.size.1 + 0.5);
}

#[test]
fn ui_331_a_compressor_grows_its_history_and_its_meters() {
    let (mut s, g, t, [_, comp]) = two();
    s.show_instrument(t, comp);
    assert_eq!(s.instruments.open[0].body.0, compressor::NATURAL_W);
    let b0 = body(&s, &g);
    drag(&mut s, &g, comp, 120.0, 100.0);
    let b1 = body(&s, &g);
    let [xfer0, hist0] = b0.graphs[..] else {
        panic!("two planes")
    };
    let [xfer1, hist1] = b1.graphs[..] else {
        panic!("two planes")
    };
    assert_eq!(
        (xfer1.w, xfer1.h),
        (xfer0.w, xfer0.h),
        "the curve stays square"
    );
    assert_eq!(hist1.w, hist0.w + 120.0);
    assert_eq!(hist1.h, hist0.h + 100.0);
    for (a, b) in b0.levels.iter().zip(&b1.levels) {
        assert_eq!(b.bars[0].h, a.bars[0].h + 100.0);
        assert_eq!(b.readout.y, a.readout.y + 100.0);
        assert!(b.readout.y >= b.bars[0].bottom());
        assert!(b.readout.bottom() <= hist1.bottom() + 0.5);
    }
    for (a, b) in b0.slots.iter().zip(&b1.slots) {
        assert_eq!((a.cell.w, a.cell.h), (b.cell.w, b.cell.h));
        assert_eq!(b.cell.y, a.cell.y + 100.0);
    }
}

#[test]
fn ui_331_each_kind_opens_at_the_size_it_was_left_at() {
    let (mut s, g, t, [eq, comp]) = two();
    s.show_instrument(t, eq);
    drag(&mut s, &g, eq, 160.0, 80.0);
    let grown = s.instruments.open[0].body;
    // The Compressor takes the window in at its own size.
    s.show_instrument(t, comp);
    assert_eq!(s.instruments.open[0].body.0, compressor::NATURAL_W);
    // The EQ comes back at the size it was left at.
    s.show_instrument(t, eq);
    assert_eq!(s.instruments.open[0].body, grown);
    // So does a closed one opened again.
    let (x, y) = s.instrument_center(&g, eq, Part::Close).unwrap();
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instruments.open.is_empty());
    s.show_instrument(t, eq);
    assert_eq!(s.instruments.open[0].body, grown);
    let r = s.instruments.frame(0, &g);
    assert_eq!(r.h, title_bar::BAR_H + grown.1);
}
