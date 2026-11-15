//! A Drum Kit's pad face (UI-357): the voice row's headings (Env, Filter, Pitch, Amp) and
//! several pads chosen with Ctrl and Shift, which a knob then turns together with their
//! differences kept (the track multi-select's rules, `docs/ui-approved.md`).

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;
use xirius_params::pad_key;

use super::layout::{Body, Mark};
use crate::instrument::window::Part;
use crate::instrument::{Pick, current};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

const BOTH: Mods = Mods {
    ctrl: true,
    shift: true,
    alt: false,
};

pub(super) fn window(name: &str) -> (Studio, Geometry, TrackId, PluginId) {
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let t = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    assert!(s.set_instrument(t, &Pick::built_in(name)), "{:?}", s.error);
    let id = plugin(&s, t).id;
    s.show_instrument(t, id);
    s.instruments.detached_target = Some(id);
    (s, g, t, id)
}

pub(super) fn plugin(s: &Studio, t: TrackId) -> Plugin {
    current(&s.project().tracks[&t]).cloned().unwrap()
}

pub(super) fn body(s: &Studio, g: &Geometry, t: TrackId) -> Body {
    let x = plugin(s, t);
    let def = s.instruments.def_of(&x).unwrap();
    let r = s.instruments.frame(0, g);
    s.instruments.body(r, &x, def)
}

/// A sound on pad `n`.
pub(super) fn fill(s: &mut Studio, id: PluginId, n: usize) {
    let file = json!({ "file": "Kick.wav" });
    let args = json!({ "id": id.to_string(), "files": [file], "pad": xirius_params::PAD_NAMES[n] });
    assert!(s.run_command("plugin.samples.add", args), "{:?}", s.error);
}

/// Press pad `n` with `mods` and let go.
pub(super) fn press(s: &mut Studio, g: &Geometry, id: PluginId, n: usize, mods: Mods) {
    let (x, y) = s
        .instrument_center(g, id, Part::Pad(36 + n as u8))
        .expect("the pad");
    assert!(s.instrument_press(g, x, y, 1, mods));
    s.instrument_up();
}

/// The pads the face shows chosen, by their place.
fn lit(s: &Studio, g: &Geometry, t: TrackId) -> Vec<usize> {
    body(s, g, t)
        .spots
        .iter()
        .filter_map(|x| match x.mark {
            Mark::Pad(p) if x.on => Some(p),
            _ => None,
        })
        .collect()
}

fn shown(s: &Studio) -> usize {
    s.instruments.open[0].pick
}

/// The 0..1 value of a pad's `id` in the song (its default when it keeps none).
fn value(s: &Studio, t: TrackId, pad: usize, id: &str) -> Option<f64> {
    plugin(s, t).params.get(&pad_key(pad, id)).copied()
}

pub(super) fn put(s: &mut Studio, id: PluginId, pad: usize, name: &str, v: f64) {
    let args = json!({ "id": id.to_string(), "param": pad_key(pad, name), "value": v });
    assert!(s.run_command("plugin.param.set", args), "{:?}", s.error);
}

/// The centre of the control of the shown pad's `id`.
pub(super) fn centre(s: &Studio, g: &Geometry, t: TrackId, id: &str) -> (f32, f32) {
    let x = plugin(s, t);
    let def = s.instruments.def_of(&x).unwrap();
    let b = body(s, g, t);
    let slot = b
        .slots
        .iter()
        .find(|c| def.params[c.k].id.ends_with(&format!(".{id}")))
        .unwrap_or_else(|| panic!("a control for {id}"));
    (
        slot.cell.x + slot.cell.w / 2.0,
        slot.cell.y + slot.cell.h / 2.0,
    )
}

/// A kit with sounds on pads 1..=3 (A2, A3, A4) and none on pad 4 (A5): all four chosen, A4 shown last.
pub(super) fn kit() -> (Studio, Geometry, TrackId, PluginId) {
    let (mut s, g, t, id) = window("Drum Kit");
    for n in 1..=3 {
        fill(&mut s, id, n);
    }
    press(&mut s, &g, id, 4, Mods::NONE);
    for n in 1..=3 {
        press(&mut s, &g, id, n, Mods::ctrl());
    }
    (s, g, t, id)
}

#[test]
fn the_voice_row_is_headed_env_filter_pitch_and_amp() {
    let (mut s, g, t, id) = window("Drum Kit");
    fill(&mut s, id, 1);
    press(&mut s, &g, id, 1, Mods::NONE);
    let x = plugin(&s, t);
    let def = s.instruments.def_of(&x).unwrap();
    let b = body(&s, &g, t);
    let at = |text: &str| {
        b.names
            .iter()
            .find(|n| n.text == text)
            .unwrap_or_else(|| panic!("the heading {text}"))
            .r
    };
    let (env, filter, pitch, amp) = (at("Env"), at("Filter"), at("Pitch"), at("Amp"));
    assert!(
        env.x < filter.x && filter.x < pitch.x,
        "Env, Filter, Pitch in a row"
    );
    assert!(pitch.y < amp.y, "Amp under Pitch");
    let cell = |id: &str| {
        b.slots
            .iter()
            .find(|c| def.params[c.k].id.ends_with(&format!(".{id}")))
            .unwrap_or_else(|| panic!("{id}"))
            .cell
    };
    for id in ["transpose", "fine"] {
        let c = cell(id);
        assert!(
            c.y > pitch.y && c.bottom() <= amp.y + 0.5,
            "{id} under Pitch"
        );
    }
    for id in ["volume", "pan", "velocity"] {
        assert!(cell(id).y > amp.y, "{id} under Amp");
    }
    // The empty pad's dim row says the same.
    press(&mut s, &g, id, 5, Mods::NONE);
    let b = body(&s, &g, t);
    for text in ["Env", "Filter", "Pitch", "Amp"] {
        assert!(
            b.names.iter().any(|n| n.text == text && n.dim),
            "{text} dim on an empty pad"
        );
    }
}

#[test]
fn the_sampler_keeps_its_headings() {
    let (s, g, t, _) = window("Sampler");
    let b = body(&s, &g, t);
    // Whatever the Sampler shows, its Amp stays one block and it has no Pitch heading.
    assert!(!b.names.iter().any(|n| n.text == "Pitch"));
}

#[test]
fn ctrl_toggles_shift_ranges_and_the_last_pressed_pad_shows() {
    let (mut s, g, t, id) = window("Drum Kit");
    s.instruments.hit = None;
    press(&mut s, &g, id, 2, Mods::NONE);
    assert_eq!((lit(&s, &g, t), shown(&s)), (vec![2], 2));
    press(&mut s, &g, id, 5, Mods::ctrl());
    assert_eq!((lit(&s, &g, t), shown(&s)), (vec![2, 5], 5));
    press(&mut s, &g, id, 2, Mods::ctrl());
    assert_eq!(lit(&s, &g, t), vec![5], "Ctrl on a chosen pad lets it go");
    press(&mut s, &g, id, 5, Mods::ctrl());
    assert_eq!(lit(&s, &g, t), vec![5], "the last one stays");
    // A range from where the last plain or Ctrl press was.
    press(&mut s, &g, id, 8, Mods::shift());
    assert_eq!((lit(&s, &g, t), shown(&s)), (vec![5, 6, 7, 8], 8));
    press(&mut s, &g, id, 3, Mods::shift());
    assert_eq!(lit(&s, &g, t), vec![3, 4, 5], "a range replaces the choice");
    press(&mut s, &g, id, 10, BOTH);
    assert_eq!(lit(&s, &g, t), vec![3, 4, 5, 6, 7, 8, 9, 10]);
    press(&mut s, &g, id, 12, Mods::NONE);
    assert_eq!((lit(&s, &g, t), shown(&s)), (vec![12], 12));
}

#[test]
fn a_press_with_ctrl_or_shift_chooses_and_does_not_sound() {
    let (mut s, g, _, id) = window("Drum Kit");
    press(&mut s, &g, id, 2, Mods::NONE);
    s.instruments.hit = None;
    s.instruments.followed = None;
    press(&mut s, &g, id, 3, Mods::ctrl());
    press(&mut s, &g, id, 6, Mods::shift());
    assert!(s.instruments.hit.is_none() && s.instruments.followed.is_none());
    press(&mut s, &g, id, 7, Mods::NONE);
    assert!(s.instruments.hit.is_some(), "a plain press still sounds");
}

#[test]
fn a_knob_turns_the_chosen_pads_together_keeping_their_differences() {
    let (mut s, g, t, id) = kit();
    assert_eq!(shown(&s), 3);
    put(&mut s, id, 1, "volume", 0.8);
    put(&mut s, id, 2, "volume", 0.3);
    put(&mut s, id, 3, "volume", 0.5);
    let steps = s.song().steps();
    let (x, y) = centre(&s, &g, t, "volume");
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    s.instrument_moved(&g, x, y - 10.0, false);
    s.instrument_up();
    let now = |n| value(&s, t, n, "volume").unwrap();
    let by = now(3) - 0.5;
    assert!(by > 0.0, "the shown pad moved up");
    assert!((now(1) - (0.8 + by).min(1.0)).abs() < 1e-6, "{}", now(1));
    assert!((now(2) - (0.3 + by)).abs() < 1e-6, "{}", now(2));
    assert_eq!(
        value(&s, t, 4, "volume"),
        None,
        "a pad with no sound gets no value"
    );
    assert_eq!(s.song().steps(), steps + 1, "one undo step");
    assert!(s.run_command("history.undo", json!({})));
    assert_eq!(value(&s, t, 1, "volume"), Some(0.8));
    assert_eq!(value(&s, t, 2, "volume"), Some(0.3));
    assert_eq!(value(&s, t, 3, "volume"), Some(0.5));
}

#[test]
fn a_pad_at_an_end_stops_there_and_its_difference_comes_back() {
    let (mut s, g, t, id) = kit();
    put(&mut s, id, 1, "volume", 0.9);
    put(&mut s, id, 2, "volume", 0.3);
    put(&mut s, id, 3, "volume", 0.5);
    let (x, y) = centre(&s, &g, t, "volume");
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    s.instrument_moved(&g, x, y - 300.0, false);
    s.instrument_moved(&g, x, y - 10.0, false);
    s.instrument_up();
    let now = |s: &Studio, n| value(s, t, n, "volume").unwrap();
    let by = now(&s, 3) - 0.5;
    assert!(by > 0.0 && by < 0.1, "{by}");
    assert!(
        (now(&s, 1) - (0.9 + by)).abs() < 1e-6,
        "the end it touched on the way is forgotten: {}",
        now(&s, 1)
    );
    assert!((now(&s, 2) - (0.3 + by)).abs() < 1e-6);
    // Held at the end, it stops there.
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    s.instrument_moved(&g, x, y - 300.0, false);
    s.instrument_up();
    assert_eq!(now(&s, 1), 1.0);
    assert!(now(&s, 3) <= 1.0 && now(&s, 2) <= 1.0 && now(&s, 2) < now(&s, 1));
}

#[test]
fn a_switch_a_segment_and_a_reset_set_the_same_value_on_the_chosen_pads() {
    let (mut s, g, t, id) = kit();
    let x = plugin(&s, t);
    let def = s.instruments.def_of(&x).unwrap();
    // Trigger: the last segment.
    let cell = body(&s, &g, t)
        .slots
        .iter()
        .find(|c| def.params[c.k].id.ends_with(".trigger"))
        .expect("the Trigger control")
        .cell;
    assert!(s.instrument_press(&g, cell.right() - 2.0, cell.y + cell.h / 2.0, 1, Mods::NONE));
    s.instrument_up();
    let last = value(&s, t, 3, "trigger").expect("written");
    for n in [1, 2] {
        assert_eq!(value(&s, t, n, "trigger"), Some(last), "pad {n}");
    }
    assert_eq!(value(&s, t, 4, "trigger"), None);
    // Alt puts the knob back to its default on all of them.
    put(&mut s, id, 1, "pan", 0.9);
    put(&mut s, id, 3, "pan", 0.1);
    let (px, py) = centre(&s, &g, t, "pan");
    let steps = s.song().steps();
    let alt = Mods {
        alt: true,
        ..Mods::NONE
    };
    assert!(s.instrument_press(&g, px, py, 1, alt));
    s.instrument_up();
    let spec = def.resolve(&pad_key(3, "pan")).unwrap().spec;
    for n in [1, 2, 3] {
        let v = value(&s, t, n, "pan").unwrap_or_else(|| spec.default_norm());
        assert!((v - spec.default_norm()).abs() < 1e-6, "pad {n}: {v}");
    }
    assert_eq!(s.song().steps(), steps + 1);
}

#[test]
fn the_wheel_on_choke_moves_the_chosen_pads_to_the_same_group() {
    let (mut s, g, t, _) = kit();
    let (x, y) = centre(&s, &g, t, "choke");
    s.window_wheel(&g, (x, y), 0.0, 2.0, Mods::NONE, false);
    s.device_sweep_end();
    let v = value(&s, t, 3, "choke").expect("written");
    for n in [1, 2] {
        assert_eq!(value(&s, t, n, "choke"), Some(v), "pad {n}");
    }
    assert_eq!(value(&s, t, 4, "choke"), None);
}

/// Press the Choke box, then pick `label` from the list it opens.
fn choke_pick(s: &mut Studio, g: &Geometry, t: TrackId, label: &str) {
    let (x, y) = centre(s, g, t, "choke");
    assert!(s.instrument_press(g, x, y, 1, Mods::NONE));
    s.instrument_up();
    assert!(s.ctx.is_some(), "the Choke box opens its list");
    assert!(crate::context::tests::choose(s, g, label));
}

#[test]
fn the_choke_box_opens_its_list_and_a_pick_writes_the_shown_pad() {
    let (mut s, g, t, id) = window("Drum Kit");
    for n in [1, 2] {
        fill(&mut s, id, n);
    }
    press(&mut s, &g, id, 2, Mods::NONE);
    choke_pick(&mut s, &g, t, "7"); // the ninth choice: None, Self, 1, 2 ...
    let x = plugin(&s, t);
    let spec = s.instruments.def_of(&x).unwrap();
    let want = spec
        .resolve(&pad_key(2, "choke"))
        .unwrap()
        .spec
        .to_norm(8.0);
    assert_eq!(value(&s, t, 2, "choke"), Some(want));
    assert_eq!(value(&s, t, 1, "choke"), None, "the other pad is left");
}

#[test]
fn a_choke_picked_with_three_pads_chosen_goes_to_all_three_in_one_step() {
    let (mut s, g, t, _) = kit();
    let steps = s.song().steps();
    choke_pick(&mut s, &g, t, "7"); // the ninth choice: None, Self, 1, 2 ...
    let x = plugin(&s, t);
    let spec = s.instruments.def_of(&x).unwrap();
    let want = spec
        .resolve(&pad_key(3, "choke"))
        .unwrap()
        .spec
        .to_norm(8.0);
    for n in [1, 2, 3] {
        assert_eq!(value(&s, t, n, "choke"), Some(want), "pad {n}");
    }
    assert_eq!(
        value(&s, t, 4, "choke"),
        None,
        "a pad with no sound gets none"
    );
    assert_eq!(s.song().steps(), steps + 1, "one undo step");
    assert!(s.run_command("history.undo", json!({})));
    for n in [1, 2, 3] {
        assert_ne!(value(&s, t, n, "choke"), Some(want), "pad {n} came back");
    }
}

#[test]
fn plain_presses_and_single_pads_write_the_shown_pad_only() {
    let (mut s, g, t, id) = kit();
    press(&mut s, &g, id, 2, Mods::NONE);
    let (x, y) = centre(&s, &g, t, "volume");
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    s.instrument_moved(&g, x, y - 30.0, false);
    s.instrument_up();
    assert!(value(&s, t, 2, "volume").is_some());
    assert_eq!(value(&s, t, 1, "volume"), None);
    assert_eq!(value(&s, t, 3, "volume"), None);
}

#[test]
fn a_screen_reader_hears_the_pads_chosen_and_how_many() {
    let (s, g, t, id) = kit();
    let marks = crate::a11y::device_marks(&s, &g);
    let _ = (t, id);
    let title = marks
        .iter()
        .find(|m| m.key.ends_with("-title"))
        .expect("the title");
    assert!(
        title
            .node
            .label()
            .is_some_and(|l| l.contains("4 pads selected")),
        "{:?}",
        title.node.label()
    );
    let selected = marks
        .iter()
        .filter(|m| m.key.contains("-pad-") && m.node.is_selected() == Some(true))
        .count();
    assert_eq!(selected, 4, "the empty pad 4 is chosen too");
}

fn title(s: &Studio, g: &Geometry, t: TrackId) -> crate::device::layout::Spot {
    body(s, g, t)
        .spots
        .into_iter()
        .find(|x| x.mark == crate::device::layout::Mark::Title)
        .expect("the title")
}

#[test]
fn the_title_of_pads_chosen_together_lists_them_in_the_order_chosen() {
    use xirius_params::PAD_NAMES as N;
    let (mut s, g, t, id) = kit();
    let x = title(&s, &g, t);
    assert_eq!(x.text, format!("{} {} {} {}", N[4], N[1], N[2], N[3]));
    assert!(x.sub.is_empty() && x.note.is_none(), "no one sound or note");
    // The face still shows the last pressed pad; one pad is its name, sound and note.
    assert_eq!(shown(&s), 3);
    press(&mut s, &g, id, 2, Mods::NONE);
    let x = title(&s, &g, t);
    assert_eq!(x.text, N[2]);
    assert!(!x.sub.is_empty() && x.note.is_some());
}

#[test]
fn a_run_and_a_pad_added_with_ctrl_list_in_order_and_a_screen_reader_hears_every_name() {
    use xirius_params::PAD_NAMES as N;
    let (mut s, g, t, id) = window("Drum Kit");
    for n in 1..=3 {
        fill(&mut s, id, n);
    }
    press(&mut s, &g, id, 1, Mods::NONE);
    press(&mut s, &g, id, 3, Mods::shift());
    press(&mut s, &g, id, 0, Mods::ctrl());
    assert_eq!(
        title(&s, &g, t).text,
        format!("{} {} {} {}", N[1], N[2], N[3], N[0])
    );
    let marks = crate::a11y::device_marks(&s, &g);
    let m = marks.iter().find(|m| m.key.ends_with("-title")).unwrap();
    let label = m.node.label().unwrap().to_string();
    assert!(
        label.contains(N[3]) && label.contains("4 pads selected"),
        "{label}"
    );
}
