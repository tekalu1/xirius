use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

use super::*;
use crate::device_window::with_surface;
use crate::instrument::window::Part;
use crate::mixer::MixerHit;
use crate::studio::DrawerTab;
use crate::tools::Mods;

fn studio(w: f32) -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    s.drawer_size.mixer = Some(600.0);
    s.instruments.open.clear();
    let g = s.geometry(w, 1000.0);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .unwrap()
        .id
}

fn add(s: &mut Studio, t: TrackId, name: &str) -> PluginId {
    assert!(s.run_command("plugin.add", json!({"track":t,"name":name})));
    let id = s.project().tracks[&t].inserts.last().unwrap().id;
    s.instruments.open.clear();
    id
}

/// A press and a let go on a row's name, as the pointer does it.
fn click_row(s: &mut Studio, g: &Geometry, t: TrackId, id: PluginId) {
    let (x, y) = s.mixer_center(g, MixerHit::Open(t, id)).unwrap();
    s.chosen_press(g, x, y);
    assert!(s.chain_down(g, x, y));
    assert!(s.fx_up(g, x, y, false));
}

fn press(s: &mut Studio, id: PluginId, part: Part) {
    with_surface(s, id, |s, g| {
        let (x, y) = s.instrument_center(g, id, part).unwrap();
        assert!(s.instrument_press(g, x, y, 1, Mods::NONE));
        s.instrument_up();
    });
}

fn picked(w: f32) -> (Studio, Geometry, TrackId, PluginId) {
    let (mut s, g) = studio(w);
    let t = s.mixer_strips(&g)[0].track;
    let eq = add(&mut s, t, "EQ");
    click_row(&mut s, &g, t, eq);
    (s, g, t, eq)
}

#[test]
fn a_picked_device_leaves_the_whole_drawer_for_the_strips_at_every_width() {
    for width in [760.0, crate::studio::WINDOW_MIN.0, 1600.0] {
        let (mut s, g, t, eq) = picked(width);
        assert!(s.instruments.detached.contains(&eq));
        assert_eq!(s.instruments.open[0].track, t);
        assert_eq!(s.instruments.frame(0, &g), Rect::default());
        with_surface(&mut s, eq, |s, local| {
            let (x, y) = s.instrument_center(local, eq, Part::Knob(0)).unwrap();
            assert!(s.instruments.frame(0, local).contains(x, y));
        });
    }
}

#[test]
fn row_and_arrow_selection_replace_only_an_unpinned_view() {
    let (mut s, g, t, eq) = picked(1600.0);
    let comp = add(&mut s, t, "Compressor");
    click_row(&mut s, &g, t, comp);
    assert_eq!(s.instruments.open[0].plugin, comp);
    assert!(s.field_keys(&g, "up", None, Mods::NONE));
    assert_eq!(s.instruments.open[0].plugin, eq);
    press(&mut s, eq, Part::Pin);
    assert!(s.field_keys(&g, "down", None, Mods::NONE));
    assert!(s.instruments.is_open(eq) && s.instruments.is_open(comp));
    assert_eq!(s.instruments.open.len(), 2);
}

#[test]
fn closing_the_independent_view_keeps_the_device_and_its_values() {
    let (mut s, _, _, eq) = picked(1600.0);
    let before = s.project().content_hash();
    let steps = s.song().steps();
    press(&mut s, eq, Part::Close);
    assert!(!s.instruments.is_open(eq));
    assert_eq!(s.project().content_hash(), before);
    assert_eq!(s.song().steps(), steps);
}

#[test]
fn changing_the_main_tab_does_not_hide_or_recreate_the_device() {
    let (mut s, _, t, eq) = picked(1600.0);
    s.instruments.open[0].pick = 3;
    s.instruments.open[0].layer = 1;
    s.instruments.focus = Some((eq, 2));
    s.tab = DrawerTab::Roll;
    s.show_instrument(t, eq);
    assert_eq!(s.instruments.open.len(), 1);
    let w = s.instruments.open[0];
    assert_eq!((w.pick, w.layer), (3, 1));
    assert_eq!(s.instruments.focus, Some((eq, 2)));
    with_surface(&mut s, eq, |s, g| {
        assert!(s.instrument_center(g, eq, Part::Close).is_some());
    });
}

#[test]
fn a_local_knob_drag_is_one_undoable_edit() {
    let (mut s, _, t, eq) = picked(1600.0);
    let before = s.project().tracks[&t]
        .inserts
        .iter()
        .find(|p| p.id == eq)
        .unwrap()
        .clone();
    let steps = s.song().steps();
    with_surface(&mut s, eq, |s, g| {
        let p = s.project();
        let plugin = p.tracks[&t].inserts.iter().find(|p| p.id == eq).unwrap();
        let k = plugin.builtin().unwrap().param("output").unwrap().0;
        let (x, y) = s.instrument_center(g, eq, Part::Knob(k)).unwrap();
        assert!(s.instrument_down(g, x, y));
        assert!(s.instrument_moved(g, x, y + 30.0, false));
        assert_eq!(s.song().steps(), steps);
        assert!(s.instrument_up());
        assert_eq!(s.song().steps(), steps + 1);
        assert!(s.undo(g));
    });
    let after = s.project().tracks[&t]
        .inserts
        .iter()
        .find(|p| p.id == eq)
        .unwrap()
        .clone();
    assert_eq!(before.params, after.params);
}

#[test]
fn independent_resize_grows_the_window() {
    let (mut s, _, _, eq) = picked(1600.0);
    let before = s.instruments.open[0].body;
    with_surface(&mut s, eq, |s, local| {
        let (x, y) = s.instrument_center(local, eq, Part::Resize).unwrap();
        assert!(s.instrument_down(local, x, y));
        assert!(s.instrument_moved(local, x + 100.0, y + 60.0, false));
        s.instrument_up();
    });
    assert!(s.instruments.open[0].body.0 > before.0);
}

#[test]
fn main_accessibility_and_keys_do_not_enter_another_surface() {
    let (mut s, g, _, eq) = picked(1600.0);
    let key = format!("device-{eq}-");
    assert!(
        !crate::a11y::marks(&s, &g)
            .iter()
            .flat_map(|m| std::iter::once(m).chain(m.children.iter()))
            .any(|m| m.key.starts_with(&key))
    );
    s.instruments.focus = Some((eq, 0));
    let before = s.project().content_hash();
    assert!(!s.device_key(&g, "right", Mods::NONE));
    assert_eq!(s.project().content_hash(), before);
    assert!(!s.device_enter(true));
}

#[test]
fn an_open_independent_view_is_human_work_until_closed() {
    let (mut s, _, _, eq) = picked(1600.0);
    assert!(s.human_interaction_active());
    s.tab = DrawerTab::Roll;
    assert!(s.human_interaction_active());
    press(&mut s, eq, Part::Close);
    assert!(!s.human_interaction_active());
}
#[test]
fn files_held_over_an_independent_drum_kit_reach_its_pads() {
    let (mut s, g) = studio(1600.0);
    let t = track(&s, "Vocal");
    let kit = add(&mut s, t, "Drum Kit");
    click_row(&mut s, &g, t, kit);
    with_surface(&mut s, kit, |s, g| {
        let i = s
            .instruments
            .open
            .iter()
            .position(|w| w.plugin == kit)
            .unwrap();
        let r = s.instruments.frame(i, g);
        let head = crate::title_bar::BAR_H;
        let (x, y) = (r.x + r.w / 2.0, r.y + head + (r.h - head) / 2.0);
        let (id, on, lit) = s.drop_target(g, x, y).expect("the face takes the files");
        assert_eq!((id, on), (kit, crate::device::sampler::DropOn::Plane));
        assert!(r.contains(lit.x, lit.y) && lit.w > 0.0, "the face lights");
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<std::path::PathBuf> = ["Kick.wav", "Snare.wav"]
            .iter()
            .map(|n| {
                let f = dir.path().join(n);
                std::fs::write(&f, b"").unwrap();
                f
            })
            .collect();
        let samples = |s: &Studio| -> Vec<String> {
            s.project().tracks[&t]
                .inserts
                .iter()
                .find(|x| x.id == kit)
                .unwrap()
                .samples
                .clone()
        };
        assert!(samples(s).is_empty());
        // Held over the face first, as the OS drags them: the face lights (the fifth review's
        // low 10).
        assert!(s.files_moved(g, x, y, &files, false));
        assert_eq!(s.instruments.drop_on.map(|d| (d.0, d.1)), Some((kit, on)));
        assert!(s.drop_files(g, x, y, &files), "{:?}", s.error);
        let got = samples(s);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].ends_with("Kick.wav") && got[1].ends_with("Snare.wav"));
        assert_eq!(s.instruments.drop_on, None, "the light goes with the drop");
        // Each on its pad from A1.
        let x = s.project().tracks[&t]
            .inserts
            .iter()
            .find(|x| x.id == kit)
            .unwrap()
            .clone();
        let def = s.instruments.def_of(&x).unwrap();
        let pads: Vec<_> = (0..3)
            .map(|p| crate::device::sampler::pad_files(def, &x, p))
            .collect();
        assert_eq!(pads, vec![vec![0], vec![1], vec![]]);
        s.instruments.open[i].bank = Some(1);
        s.show_instrument(t, kit);
        let w = s.instruments.open.iter().find(|w| w.plugin == kit).unwrap();
        assert_eq!(w.bank, Some(1), "the window shows the bank the face showed");
    });
}
