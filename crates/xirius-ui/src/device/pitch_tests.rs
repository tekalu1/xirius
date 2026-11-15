//! UI-359: the Pitch Correction window without opening a native window.

use super::control::{self, Look, Step};
use super::layout::{self, Shape};
use xirius_params::{PITCH_CORRECTION, pitch_correction};

#[test]
fn pitch_ui_has_the_approved_controls_and_graph_at_both_widths() {
    let def = &PITCH_CORRECTION;
    for width in [360.0, 640.0] {
        let values: Vec<_> = def.params.iter().map(|p| p.default_norm()).collect();
        let body = layout::body(def, &Shape { w: width, values: &values, ..Shape::default() });
        assert!(body.w <= width);
        assert_eq!(body.graphs.len(), 1);
        assert_eq!(body.keys.len(), 12);
        assert_eq!(body.slots.len(), 7);
        assert_eq!(body.slot(0).unwrap().look, Look::Menu);
        assert_eq!(body.slot(1).unwrap().look, Look::Menu);
        assert!(body.slots.iter().all(|s| s.cell.bottom() <= body.h));
    }
}

#[test]
fn pitch_ui_keyboard_follows_key_and_restores_the_named_scale() {
    let major = 1.0 / 3.0;
    let key = 2_u8; // D
    let lit = |norm: f64, root: u8, pitch: u8| {
        pitch_correction::scale_mask(norm) & (1 << ((pitch + 12 - root) % 12)) != 0
    };
    assert_eq!((0..12).filter(|&n| lit(major, key, n)).collect::<Vec<_>>(), vec![1, 2, 4, 6, 7, 9, 11]);
    let custom = pitch_correction::toggle_relative(major, 4);
    assert!(!lit(custom, key, 6));
    let restored = pitch_correction::toggle_relative(custom, 4);
    assert_eq!(pitch_correction::scale_mask(restored), pitch_correction::MAJOR);
    assert!(lit(restored, 1, 5)); // C# Major moves F to F# relative to D Major.
}

#[test]
fn pitch_ui_knobs_use_the_existing_drag_and_arrow_sensitivity() {
    let speed = &PITCH_CORRECTION.params[2];
    let initial = speed.default_norm();
    let dragged = control::dragged(speed, initial, -40.0, false);
    assert!(speed.to_plain(dragged) > 20.0);
    let down = control::step(speed, dragged, -1.0, Step::Small);
    assert!(speed.to_plain(down) < speed.to_plain(dragged));
}
