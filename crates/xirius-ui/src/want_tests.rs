//! The shapes over the windows of their own (UI-384).

use super::Want;
use crate::instrument::window::Part;
use crate::instrument::{DEFAULT, Pick, current};
use crate::studio::{Rect, Studio};
use gpui::CursorStyle;
use xirius_core::id::TrackId;

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .unwrap()
}

fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

#[test]
fn a_devices_window_shows_the_arrows_on_its_knobs_and_the_hand_on_its_buttons() {
    let mut s = Studio::sample();
    let t = track(&s, "Keys");
    assert!(s.set_instrument(t, &Pick::built_in(DEFAULT)));
    let id = current(&s.project().tracks[&t]).unwrap().id;
    // A sample in the Sampler, so that its window shows its knobs.
    assert!(s.run_command(
        "plugin.samples.add",
        serde_json::json!({ "id": id.to_string(), "file": "Piano C3.wav" }),
    ));
    s.show_instrument(t, id);
    let cutoff = current(&s.project().tracks[&t])
        .unwrap()
        .builtin()
        .unwrap()
        .param("cutoff")
        .unwrap()
        .0;
    crate::device_window::with_surface(&mut s, id, |s, g| {
        let want_at = |s: &Studio, part: Part| {
            let (x, y) = s.instrument_center(g, id, part).expect("in sight");
            s.float_want(g, x, y)
        };
        assert_eq!(want_at(s, Part::Head), (Want::Plain, false), "the bar");
        assert_eq!(want_at(s, Part::Close), (Want::Press, false));
        assert_eq!(want_at(s, Part::Pin), (Want::Press, false));
        assert_eq!(want_at(s, Part::Preset), (Want::Press, false));
        assert_eq!(want_at(s, Part::Knob(cutoff)), (Want::DragV, false));
        // Turned, it keeps its arrows wherever the pointer goes (it is not hidden).
        let (x, y) = s.instrument_center(g, id, Part::Knob(cutoff)).unwrap();
        assert!(s.instrument_down(g, x, y));
        assert!(s.instrument_moved(g, x + 300.0, y + 40.0, false));
        assert_eq!(s.float_want(g, x + 300.0, y + 40.0), (Want::DragV, true));
        assert_eq!(
            crate::pointer::show(Want::DragV, false, true).style,
            CursorStyle::ResizeUpDown
        );
        assert!(s.instrument_up());
        assert_eq!(s.float_want(g, x, y), (Want::DragV, false));
    });
}

#[test]
fn the_channel_editors_window_shows_the_hand_on_its_buttons() {
    let mut s = Studio::sample();
    s.instruments.open.clear();
    let g = s.geometry(1600.0, 1000.0);
    s.open_channel(track(&s, "Vocal"));
    s.instruments.detached_channel = true;
    let v = s.channel_view(&g).unwrap();
    assert_eq!(
        s.float_want(&g, mid(v.bar.close).0, mid(v.bar.close).1).0,
        Want::Press
    );
    for (_, r) in &v.tools {
        let (x, y) = mid(*r);
        assert_eq!(s.float_want(&g, x, y), (Want::Press, false), "a tool");
    }
    let head = crate::channel::drag_rects(&v)[0];
    let (x, y) = mid(head);
    assert_eq!(s.float_want(&g, x, y), (Want::Plain, false), "the bar");
    // Every place answers as its part does.
    let mut y = v.r.y;
    while y < v.r.bottom() {
        let mut x = v.r.x;
        while x < v.r.right() {
            if let Some(part) = crate::channel::part_at(&v, x, y) {
                assert_eq!(s.float_want(&g, x, y).0, part.want(), "{part:?}");
            }
            x += 6.0;
        }
        y += 6.0;
    }
}

#[test]
fn the_macro_controls_window_shows_the_arrows_on_its_knobs_and_the_crosshair_on_its_pad() {
    let mut s = Studio::sample();
    s.instruments.open.clear();
    let g = s.geometry(1600.0, 1000.0);
    s.open_macros(track(&s, "Vocal"));
    s.instruments.detached_macros = true;
    let v = s.macro_view().unwrap();
    for knob in v.knobs {
        let (x, y) = mid(knob);
        assert_eq!(s.float_want(&g, x, y), (Want::DragV, false), "a knob");
    }
    let (x, y) = mid(v.bar.close);
    assert_eq!(s.float_want(&g, x, y), (Want::Press, false));
    let (x, y) = mid(v.pad);
    assert_eq!(s.float_want(&g, x, y), (Want::Locate, false));
    assert!(s.macro_press(x, y));
    assert_eq!(
        s.float_want(&g, v.pad.right() + 50.0, y),
        (Want::Locate, true),
        "held off the pad"
    );
    s.macro_up();
}
