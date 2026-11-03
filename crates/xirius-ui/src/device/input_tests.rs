//! UI-324: every way a device's control moves, without a window.

use std::time::{Duration, Instant};

use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;

use crate::instrument::window::{Part, values};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// The sample song with a Delay on Keys, its window open.
fn delay() -> (Studio, Geometry, TrackId, PluginId) {
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let t = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "Delay" }),
    ));
    let id = fx(&s, t).id;
    s.show_instrument(t, id);
    assert!(s.instruments.is_open(id));
    (s, g, t, id)
}

fn fx(s: &Studio, t: TrackId) -> Plugin {
    s.project().tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == "Delay")
        .cloned()
        .unwrap()
}

/// The parameter `id`'s index and its value now.
fn value(s: &Studio, t: TrackId, id: &str) -> (usize, f64) {
    let x = fx(s, t);
    let (k, _) = x.builtin().unwrap().param(id).unwrap();
    (k, values(&x)[k])
}

fn plain(s: &Studio, t: TrackId, id: &str) -> f64 {
    let x = fx(s, t);
    let (k, spec) = x.builtin().unwrap().param(id).unwrap();
    spec.to_plain(values(&x)[k])
}

/// Sync off: the Time box shows in place of the Beats box (UI-329).
fn unsynced(s: &mut Studio, id: PluginId) {
    assert!(s.run_command(
        "plugin.param.set",
        json!({ "id": id.to_string(), "param": "sync", "value": 0.0 }),
    ));
}

fn center(s: &Studio, g: &Geometry, id: PluginId, k: usize) -> (f32, f32) {
    s.instrument_center(g, id, Part::Knob(k)).expect("in sight")
}

fn click(s: &mut Studio, g: &Geometry, id: PluginId, k: usize, clicks: u32, mods: Mods) {
    let (x, y) = center(s, g, id, k);
    assert!(s.instrument_press(g, x, y, clicks, mods));
    s.instrument_up();
}

fn type_in(s: &mut Studio, g: &Geometry, id: PluginId, k: usize, text: &str) {
    click(s, g, id, k, 1, Mods::NONE);
    click(s, g, id, k, 2, Mods::NONE);
    assert!(s.instruments.entry.is_some(), "a double click types");
    assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Device));
    s.ime_replace(None, text, false);
}

#[test]
fn ui_324_a_typed_value_goes_in_on_enter_and_escape_keeps_the_old_one() {
    let (mut s, g, t, id) = delay();
    unsynced(&mut s, id);
    let (time, _) = value(&s, t, "time");
    let before = s.song().steps();
    type_in(&mut s, &g, id, time, "250");
    // The old text goes at the first letter typed.
    assert_eq!(s.instruments.entry.as_ref().unwrap().field.text(), "250");
    assert!(s.field_keys(&g, "enter", None, Mods::NONE));
    assert!(s.instruments.entry.is_none());
    assert_eq!(s.song().steps(), before + 1, "one step");
    assert!((plain(&s, t, "time") - 250.0).abs() < 0.5);

    type_in(&mut s, &g, id, time, "900");
    assert!(s.field_keys(&g, "escape", None, Mods::NONE));
    assert_eq!(s.song().steps(), before + 1, "Esc puts nothing in");
    assert!((plain(&s, t, "time") - 250.0).abs() < 0.5);

    // What does not read as a value leaves it.
    type_in(&mut s, &g, id, time, "abc");
    assert!(s.field_keys(&g, "enter", None, Mods::NONE));
    assert_eq!(s.song().steps(), before + 1);
}

#[test]
fn ui_324_typed_values_read_kilo_percent_fractions_and_choices() {
    let (mut s, g, t, id) = delay();
    unsynced(&mut s, id);
    let cases = [
        ("feedback", "50%", 50.0),
        ("mix", "４０", 40.0),
        ("time", "0.5s", 500.0),
    ];
    for (param, text, want) in cases {
        let (k, _) = value(&s, t, param);
        type_in(&mut s, &g, id, k, text);
        assert!(s.field_keys(&g, "tab", None, Mods::NONE));
        let got = plain(&s, t, param);
        assert!((got - want).abs() < 0.5, "{param} {text}: {got}");
    }
    // The cuts and the beats are points and a menu now: what they would
    // read, typed.
    let def = fx(&s, t).builtin().unwrap();
    let typed = |p: &str, text: &str| def.param(p).unwrap().1.typed(text);
    assert_eq!(typed("high_cut", "2k"), Some(2000.0));
    assert_eq!(typed("division", "1/8"), Some(2.0));
}

#[test]
fn ui_324_one_drag_is_one_param_set_and_one_undo() {
    let (mut s, g, t, id) = delay();
    let (k, start) = value(&s, t, "feedback");
    let before = s.song().steps();
    let (x, y) = center(&s, &g, id, k);
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    for dy in 1..=30 {
        assert!(s.instrument_moved(&g, x, y - dy as f32, false));
    }
    // It reads the value it is dragged to, the song has not taken it.
    assert!(s.instruments.turning(id, k).unwrap() > start);
    assert_eq!(value(&s, t, "feedback").1, start);
    assert!(s.instrument_up());
    assert_eq!(s.song().steps(), before + 1);
    assert!((value(&s, t, "feedback").1 - (start + 0.2)).abs() < 5e-4);
    assert!(s.undo(&g));
    assert_eq!(value(&s, t, "feedback").1, start);
}

/// L-par-3: Esc during a drag lets go of the knob: the song keeps its value, no step is
/// made, and letting the button go after does nothing more.
#[test]
fn ui_324_escape_during_a_drag_puts_nothing_in() {
    let (mut s, g, t, id) = delay();
    let (k, start) = value(&s, t, "feedback");
    let before = s.song().steps();
    let (x, y) = center(&s, &g, id, k);
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, x, y - 30.0, false));
    assert!(s.instruments.turning(id, k).unwrap() > start);
    assert!(s.field_keys(&g, "escape", None, Mods::NONE));
    assert!(s.instruments.grab.is_none());
    assert_eq!(s.instruments.turning(id, k), None);
    assert!(!s.instrument_up());
    assert_eq!(s.song().steps(), before);
    assert_eq!(value(&s, t, "feedback").1, start);
}

#[test]
fn ui_324_shift_drags_a_quarter_as_far() {
    let (mut s, g, t, id) = delay();
    let (k, start) = value(&s, t, "feedback");
    let (x, y) = center(&s, &g, id, k);
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, x, y - 60.0, true));
    assert!(s.instrument_up());
    assert!((value(&s, t, "feedback").1 - (start + 0.1)).abs() < 5e-4);
}

#[test]
fn ui_324_alt_press_and_delete_put_the_default_back() {
    let (mut s, g, t, id) = delay();
    let (k, start) = value(&s, t, "mix");
    let (x, y) = center(&s, &g, id, k);
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, x, y - 45.0, false));
    s.instrument_up();
    assert_ne!(value(&s, t, "mix").1, start);
    let alt = Mods {
        alt: true,
        ..Mods::NONE
    };
    click(&mut s, &g, id, k, 1, alt);
    assert_eq!(value(&s, t, "mix").1, start);

    assert!(s.field_keys(&g, "end", None, Mods::NONE));
    assert_eq!(value(&s, t, "mix").1, 1.0);
    assert!(s.field_keys(&g, "delete", None, Mods::NONE));
    assert_eq!(value(&s, t, "mix").1, start);
    assert!(s.field_keys(&g, "home", None, Mods::NONE));
    assert_eq!(value(&s, t, "mix").1, 0.0);
}

#[test]
fn ui_324_on_off_is_a_button_and_the_beats_a_box_with_its_menu() {
    use crate::device::control::Look;
    let (mut s, g, t, id) = delay();
    let def = fx(&s, t).builtin().unwrap();
    let look = |s: &Studio, p: &str| {
        let r = s.instruments.rect(0, g.size);
        let k = def.param(p).unwrap().0;
        s.instruments
            .slots(r, &fx(s, t), def)
            .iter()
            .find(|x| x.k == k)
            .map(|x| x.look)
    };
    assert_eq!(look(&s, "sync"), Some(Look::Toggle));
    assert_eq!(look(&s, "ping_pong"), Some(Look::Toggle));
    assert_eq!(look(&s, "division"), Some(Look::Menu));
    assert_eq!(look(&s, "time"), None, "the Beats box stands for it");

    // A press on the Beats box opens its menu; a row goes in, in one step.
    let (division, _) = value(&s, t, "division");
    click(&mut s, &g, id, division, 1, Mods::NONE);
    let at = s
        .context_rows()
        .iter()
        .position(|r| r.label == "1/4 T")
        .expect("the row");
    let before = s.song().steps();
    assert!(s.context_pick(&g, at));
    assert_eq!(s.song().steps(), before + 1);
    assert_eq!(plain(&s, t, "division"), 3.0);
    assert_eq!(plain(&s, t, "feel"), 2.0);

    // The button flips with one press, and back; off, the Time box shows.
    let (sync, on) = value(&s, t, "sync");
    assert_eq!(on, 1.0);
    click(&mut s, &g, id, sync, 1, Mods::NONE);
    assert_eq!(value(&s, t, "sync").1, 0.0);
    assert_eq!(look(&s, "time"), Some(Look::Field));
    assert_eq!(look(&s, "division"), None);
    assert!(s.field_keys(&g, "enter", None, Mods::NONE));
    assert_eq!(value(&s, t, "sync").1, 1.0);
}

/// M-ui-4: Space is the transport's after a press on a knob or a button: the device does
/// not take it, and the rest of the keys stay the control's.
#[test]
fn ui_324_space_plays_after_a_press_on_a_control() {
    let (mut s, g, t, id) = delay();
    for param in ["feedback", "sync"] {
        let (k, was) = value(&s, t, param);
        click(&mut s, &g, id, k, 1, Mods::NONE);
        let now = value(&s, t, param).1;
        assert_eq!(s.instruments.focus, Some((id, k)));
        assert!(!s.field_keys(&g, "space", None, Mods::NONE), "{param}");
        assert_eq!(value(&s, t, param).1, now, "{param}: {was}");
    }
}

/// M-ui-4: Tab in a typed value puts it in and goes on to the next control.
#[test]
fn ui_324_tab_in_a_typed_value_puts_it_in_and_goes_on() {
    let (mut s, g, t, id) = delay();
    let (k, _) = value(&s, t, "feedback");
    let before = s.song().steps();
    type_in(&mut s, &g, id, k, "50%");
    assert!(s.field_keys(&g, "tab", None, Mods::NONE));
    assert!(s.instruments.entry.is_none());
    assert_eq!(s.song().steps(), before + 1);
    assert!((plain(&s, t, "feedback") - 50.0).abs() < 0.5);
    let to = s.instruments.focus.expect("the keys stay in the window");
    assert_ne!(to, (id, k), "on to the next");
}

/// M-ui-4: Tab past the screen's last place goes into the window on top, and Shift+Tab
/// before its first, to the window's last control; Esc gives the keys back.
#[test]
fn ui_324_tab_from_the_screen_goes_into_the_window_on_top() {
    let (mut s, g, _, id) = delay();
    s.device_blur();
    let mut n = 0;
    while s.instruments.focus.is_none() {
        assert!(s.run_binding(&g, "view.focus --way next"));
        n += 1;
        assert!(n <= 4, "the window is a stop");
    }
    let first = s.instruments.focus.unwrap();
    assert_eq!(first.0, id);
    assert!(s.field_keys(&g, "escape", None, Mods::NONE));
    assert_eq!(s.instruments.focus, None);
    let mut n = 0;
    while s.instruments.focus.is_none() {
        assert!(s.run_binding(&g, "view.focus --way prev"));
        n += 1;
        assert!(n <= 4);
    }
    let last = s.instruments.focus.unwrap();
    assert_ne!(last, first);
    // From the last, Tab comes round to the first.
    assert!(s.field_keys(&g, "tab", None, Mods::NONE));
    assert_eq!(s.instruments.focus, Some(first));
}

#[test]
fn ui_324_the_keys_step_tab_and_go_in_once_still() {
    let (mut s, g, t, id) = delay();
    let (k, start) = value(&s, t, "feedback");
    click(&mut s, &g, id, k, 1, Mods::NONE);
    assert_eq!(s.instruments.focus, Some((id, k)));
    let before = s.song().steps();
    for _ in 0..5 {
        assert!(s.field_keys(&g, "up", None, Mods::NONE));
    }
    assert!(s.field_keys(&g, "pageup", None, Mods::NONE));
    // Heard and shown, not yet in the song.
    let shown = s.device_value(id, k).unwrap();
    assert!((shown - (start + 0.15)).abs() < 5e-4, "{shown}");
    assert_eq!(s.song().steps(), before);
    // Still for a moment: one step.
    s.instruments.sweep.as_mut().unwrap().at = Instant::now() - Duration::from_secs(1);
    assert!(s.device_quiet());
    assert_eq!(s.song().steps(), before + 1);
    assert!((value(&s, t, "feedback").1 - (start + 0.15)).abs() < 5e-4);

    // Shift steps finer; Tab goes to the next control, Shift+Tab back.
    let shift = Mods {
        shift: true,
        ..Mods::NONE
    };
    assert!(s.field_keys(&g, "down", None, shift));
    assert!(s.device_sweep_end());
    assert!((value(&s, t, "feedback").1 - (start + 0.148)).abs() < 5e-4);
    assert!(s.field_keys(&g, "tab", None, Mods::NONE));
    assert_ne!(s.instruments.focus, Some((id, k)));
    assert!(s.field_keys(&g, "tab", None, shift));
    assert_eq!(s.instruments.focus, Some((id, k)));
    // Escape lets the keys go back to the rest of the screen.
    assert!(s.field_keys(&g, "escape", None, Mods::NONE));
    assert_eq!(s.instruments.focus, None);
    assert!(!s.device_key("up", Mods::NONE));
}

#[test]
fn ui_324_the_wheel_turns_a_control_and_goes_in_once() {
    let (mut s, g, t, id) = delay();
    let (k, start) = value(&s, t, "mix");
    let (x, y) = center(&s, &g, id, k);
    let before = s.song().steps();
    for _ in 0..3 {
        assert!(s.device_wheel(&g, x, y, 1.0, false));
    }
    assert_eq!(s.song().steps(), before);
    assert!(!s.device_quiet(), "not still yet");
    assert!(s.device_sweep_end());
    assert_eq!(s.song().steps(), before + 1);
    assert!((value(&s, t, "mix").1 - (start + 0.06)).abs() < 5e-4);
}

#[test]
fn ui_324_a_right_press_offers_default_automation_and_midi_learn() {
    let (mut s, g, t, id) = delay();
    let (k, _) = value(&s, t, "mix");
    let (x, y) = center(&s, &g, id, k);
    assert!(s.instrument_right(&g, x, y));
    let rows: Vec<_> = s.context_rows().into_iter().filter(|r| !r.gap).collect();
    let labels: Vec<_> = rows.iter().map(|r| r.label.as_str()).collect();
    for want in ["既定に戻す", "オートメーションを表示", "MIDI 学習"] {
        assert!(labels.contains(&want), "{labels:?}");
    }
    // At its default there is nothing to put back.
    assert!(
        !rows
            .iter()
            .find(|r| r.label == "既定に戻す")
            .unwrap()
            .enabled
    );
}

#[test]
fn ui_324_a_learnt_knob_moves_the_parameter_once_still() {
    let (mut s, _, t, id) = delay();
    let (k, _) = value(&s, t, "mix");
    let before = s.song().steps();
    s.device_midi(t, id, "mix", 127);
    assert_eq!(s.device_value(id, k), Some(1.0));
    assert_eq!(s.song().steps(), before);
    assert!(s.device_sweep_end());
    assert_eq!(value(&s, t, "mix").1, 1.0);
    let target = xirius_prefs::midimap::Target::Param {
        track: t.to_string(),
        plugin: id.to_string(),
        param: "mix".into(),
    };
    assert!(s.midi_target_name(&target).ends_with("Delay · Mix"));
}

#[test]
fn ui_324_every_control_reads_its_name_and_value() {
    let (mut s, g, t, id) = delay();
    unsynced(&mut s, id);
    let marks = crate::a11y::marks(&s, &g);
    let of = |p: &str| {
        marks
            .iter()
            .find(|m| m.key == format!("device-{id}-{p}"))
            .unwrap_or_else(|| panic!("no mark for {p}"))
    };
    let time = of("time");
    assert_eq!(time.node.role(), accesskit::Role::Slider);
    assert_eq!(time.node.label(), Some("Time"));
    assert_eq!(time.node.value(), Some("375 ms"));
    let sync = of("sync");
    assert_eq!(sync.node.role(), accesskit::Role::Button);
    assert_eq!(sync.node.label(), Some("Sync"));
    assert_eq!(sync.node.toggled(), Some(accesskit::Toggled::False));
    assert_eq!(of("feedback").node.role(), accesskit::Role::Slider);
    // Each knob, box and button reads, and each point on the planes.
    let def = fx(&s, t).builtin().unwrap();
    let r = s.instruments.rect(0, g.size);
    for slot in s.instruments.slots(r, &fx(&s, t), def) {
        of(def.params[slot.k].id);
    }
}
