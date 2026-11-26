//! A Drum Kit's window is as tall as its voice's row (Env, Filter, Amp), whichever pad is
//! chosen and whether or not it holds a sound; an empty pad shows that row dim and takes
//! nothing from it.

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;

use super::control::Look;
use super::layout::{self, Body};
use super::sampler;
use crate::instrument::window::Part;
use crate::instrument::{Pick, current};
use crate::popup::PopupKind;
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// The body height a Drum Kit's window opens at, and its width.
const KIT: (f32, f32) = (905.0, 538.0);

fn window(name: &str) -> (Studio, Geometry, TrackId, PluginId) {
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

fn plugin(s: &Studio, t: TrackId) -> Plugin {
    current(&s.project().tracks[&t]).cloned().unwrap()
}

fn body(s: &Studio, g: &Geometry, t: TrackId) -> Body {
    let x = plugin(s, t);
    let def = s.instruments.def_of(&x).unwrap();
    let r = s.instruments.frame(0, g);
    s.instruments.body(r, &x, def)
}

fn size(s: &Studio) -> (f32, f32) {
    s.instruments.open[0].body
}

fn add(s: &mut Studio, id: PluginId, files: &[&str]) {
    let files: Vec<_> = files.iter().map(|f| json!({ "file": f })).collect();
    assert!(
        s.run_command(
            "plugin.samples.add",
            json!({ "id": id.to_string(), "files": files }),
        ),
        "{:?}",
        s.error
    );
}

/// Choose pad `n`.
fn pick(s: &mut Studio, g: &Geometry, id: PluginId, n: usize) {
    let (x, y) = s
        .instrument_center(g, id, Part::Pad(36 + n as u8))
        .expect("the pad");
    assert!(s.instrument_press(g, x, y, 1, Mods::NONE));
    s.instrument_up();
    assert_eq!(s.instruments.open[0].pick, n);
}

/// A sound on pad `n`, which stays chosen.
fn fill(s: &mut Studio, g: &Geometry, id: PluginId, n: usize) {
    pick(s, g, id, n);
    let file = json!({ "file": "Kick.wav" });
    let args = json!({ "id": id.to_string(), "files": [file], "pad": xirius_params::PAD_NAMES[n] });
    assert!(s.run_command("plugin.samples.add", args), "{:?}", s.error);
}

/// The size the window opens at for a body of width `w`.
fn need(s: &Studio, t: TrackId, w: f32) -> (f32, f32) {
    let x = plugin(s, t);
    layout::size_of(s.instruments.def_of(&x).unwrap(), w, Some(&x))
}

/// The window's foot: its body, and the bar over it.
fn foot(s: &Studio, g: &Geometry) -> f32 {
    s.instruments.frame(0, g).bottom()
}

/// Every control of the voice row (live or dim) lies inside the window.
fn row_inside(s: &Studio, g: &Geometry, t: TrackId) {
    let b = body(s, g, t);
    let foot = foot(s, g);
    let cells = b.slots.iter().chain(&b.idle_slots).map(|x| x.cell);
    assert!(cells.clone().count() > 0);
    for c in cells {
        assert!(c.bottom() <= foot + 0.01, "{c:?} past {foot}");
    }
    for r in b
        .graphs
        .iter()
        .copied()
        .chain(b.idle_graphs.iter().map(|(_, r)| *r))
    {
        assert!(r.bottom() <= foot + 0.01, "{r:?} past {foot}");
    }
}

/// The widths the tests run at: the approved wide one, and the narrow ones that stack.
const WIDTHS: [f32; 3] = [905.0, 640.0, 520.0];

fn open_at(s: &mut Studio, t: TrackId, w: f32) {
    let n = need(s, t, w);
    s.instruments.open[0].body = n;
}

#[test]
fn choosing_a_pad_or_dropping_a_sound_does_not_change_the_window() {
    let (mut s, g, t, id) = window("Drum Kit");
    let at = size(&s);
    for n in [3, 0, 9, 15] {
        pick(&mut s, &g, id, n);
        assert_eq!(size(&s), at, "pad {n}");
    }
    fill(&mut s, &g, id, 9);
    assert_eq!(size(&s), at);
    pick(&mut s, &g, id, 2);
    assert_eq!(size(&s), at);
    row_inside(&s, &g, t);
}

#[test]
fn a_taller_window_keeps_its_height_over_a_sound_added_or_all_removed() {
    let (mut s, g, t, id) = window("Drum Kit");
    s.instruments.open[0].body.1 += 120.0;
    let tall = size(&s);
    add(&mut s, id, &["Kick.wav"]);
    assert_eq!(size(&s), tall);
    assert!(s.run_command(
        "plugin.samples.remove",
        json!({ "id": id.to_string(), "all": true }),
    ));
    assert_eq!(size(&s), tall);
    row_inside(&s, &g, t);
}

#[test]
fn the_sampler_window_is_sized_as_before() {
    let (mut s, _g, t, id) = window("Sampler");
    let empty = size(&s);
    assert_eq!(empty, need(&s, t, empty.0));
    add(&mut s, id, &["Piano_C3.wav"]);
    let x = plugin(&s, t);
    let def = s.instruments.def_of(&x).unwrap();
    assert_eq!(size(&s), layout::size_of(def, size(&s).0, Some(&x)));
    assert_ne!(size(&s), empty, "a Sampler's size still follows its files");
}

#[test]
fn an_empty_pad_shows_its_row_dim_and_takes_no_value_from_it() {
    let (mut s, g, t, id) = window("Drum Kit");
    fill(&mut s, &g, id, 4);
    pick(&mut s, &g, id, 0);
    let b = body(&s, &g, t);
    assert!(!b.idle_slots.is_empty() && !b.idle_graphs.is_empty());
    assert!(b.idle_slots.iter().all(|x| x.dim), "dim");
    assert!(b.names.iter().any(|n| n.dim), "the headings dim");
    // What is drawn for them is the value-less form (`paint_knob_idle`, `paint_segments_idle`
    // and a plane with no point): a look the idle painter has no value for would be dropped,
    // so only those two looks may be here.
    assert!(
        b.idle_slots
            .iter()
            .all(|x| matches!(x.look, Look::Knob | Look::Segments)),
        "a look the idle form does not draw"
    );
    let (params, steps) = (plugin(&s, t).params.clone(), s.song().steps());
    let still = |s: &Studio, what: &str| {
        assert_eq!(plugin(s, t).params, params, "{what}: a value written");
        assert_eq!(s.song().steps(), steps, "{what}: an undo step");
        assert!(s.popup.is_none(), "{what}: a menu");
    };
    let mut points: Vec<(f32, f32)> = b
        .idle_slots
        .iter()
        .map(|x| (x.cell.x + x.cell.w / 2.0, x.cell.y + x.cell.h / 2.0))
        .collect();
    points.extend(
        b.idle_graphs
            .iter()
            .map(|(_, r)| (r.x + r.w / 2.0, r.y + r.h / 2.0)),
    );
    for (x, y) in points {
        s.instrument_press(&g, x, y, 1, Mods::NONE);
        s.instrument_moved(&g, x, y - 50.0, false);
        s.instrument_up();
        still(&s, "drag");
        s.window_wheel(&g, (x, y), 0.0, -3.0, Mods::NONE, false);
        still(&s, "wheel");
        s.instrument_press(&g, x, y, 1, Mods::NONE);
        s.instrument_up();
        for key in ["up", "down", "pageup", "home"] {
            s.device_key(&g, key, Mods::NONE);
        }
        still(&s, "key");
        s.instrument_right(&g, x, y);
        still(&s, "right-click");
    }
}

#[test]
fn the_narrow_stacked_form_follows_the_same_rule() {
    for w in WIDTHS {
        let (mut s, g, t, id) = window("Drum Kit");
        assert_eq!(size(&s), KIT, "an empty kit opens at its natural size");
        open_at(&mut s, t, w);
        let empty = size(&s);
        row_inside(&s, &g, t);
        fill(&mut s, &g, id, 0);
        assert_eq!(size(&s), empty, "{w}: A1 filled");
        row_inside(&s, &g, t);
        let mut s2 = window("Drum Kit");
        open_at(&mut s2.0, s2.2, w);
        fill(&mut s2.0, &s2.1, s2.3, 4);
        assert_eq!(size(&s2.0), empty, "{w}: A5 filled");
        // A sound on another pad than A1 puts its knobs in the window, live.
        let b = body(&s2.0, &s2.1, s2.2);
        assert!(b.idle_slots.is_empty() && b.idle_graphs.is_empty(), "{w}");
        let x = plugin(&s2.0, s2.2);
        let k = sampler::idx(s2.0.instruments.def_of(&x).unwrap(), "cutoff");
        let k = k.expect("the pad's cutoff");
        assert!(b.slots.iter().any(|x| x.k == k && !x.dim), "{w}");
        row_inside(&s2.0, &s2.1, s2.2);
        pick(&mut s2.0, &s2.1, s2.3, 1);
        assert_eq!(size(&s2.0), empty, "{w}: empty pad chosen");
        let b = body(&s2.0, &s2.1, s2.2);
        assert!(
            !b.idle_slots.is_empty() && b.idle_slots.iter().all(|x| x.dim),
            "{w}"
        );
        row_inside(&s2.0, &s2.1, s2.2);
    }
}

#[test]
fn output_sits_above_layers_and_inside_each_kit_window() {
    for w in WIDTHS {
        let (mut s, g, t, id) = window("Drum Kit");
        open_at(&mut s, t, w);
        fill(&mut s, &g, id, 0);
        let b = body(&s, &g, t);
        let output = b
            .spots
            .iter()
            .find(|x| x.mark == layout::Mark::Output)
            .unwrap();
        let label = b.names.iter().find(|x| x.text == "Output").unwrap();
        let layers = b.names.iter().find(|x| x.text == "Layers").unwrap();
        assert!(label.r.y < layers.r.y, "{w}");
        assert!(output.r.bottom() <= foot(&s, &g) + 0.01, "{w}");
        assert_eq!(output.text, "Main");
    }
}

#[test]
fn pad_output_uses_the_searchable_pick_list() {
    let (mut s, _, _, id) = window("Drum Kit");
    let mut list = s.pick_view(PopupKind::PadOutput(id)).unwrap();
    let rows = list.visible();
    assert_eq!(rows.first().unwrap().label, "Main");
    assert_eq!(rows.first().unwrap().icon, Some("output"));
    assert_eq!(rows.last().unwrap().label, "New Bus");
    list.field.value = "new".into();
    let found = list.visible();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].to(), "new");
}
