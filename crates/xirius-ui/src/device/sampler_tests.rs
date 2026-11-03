//! UI-333, UI-334: the Sampler's and the Drum Kit's windows, without a window.

use std::path::PathBuf;

use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;

use super::layout::{self, Body, Mark};
use super::play;
use super::sampler::{self, BROWSE_LABEL, DROP_LABEL};
use crate::browser::RailTab;
use crate::instrument::window::{Part, values_of};
use crate::instrument::{Pick, current};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// The sample song with Keys playing the built-in `name`, its window open.
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
    assert!(s.instruments.is_open(id));
    (s, g, t, id)
}

fn plugin(s: &Studio, t: TrackId) -> Plugin {
    current(&s.project().tracks[&t]).cloned().unwrap()
}

fn body(s: &Studio, g: &Geometry, t: TrackId) -> Body {
    let x = plugin(s, t);
    let def = s.instruments.def_of(&x).unwrap();
    let r = s.instruments.rect(0, g.size);
    s.instruments.body(r, &x, def)
}

/// The first spot that is `mark`.
fn spot(s: &Studio, g: &Geometry, t: TrackId, mark: Mark) -> usize {
    body(s, g, t)
        .spots
        .iter()
        .position(|x| x.mark == mark)
        .unwrap_or_else(|| panic!("{mark:?}"))
}

fn press(s: &mut Studio, g: &Geometry, id: PluginId, part: Part) {
    let (x, y) = s.instrument_center(g, id, part).expect("in sight");
    assert!(s.instrument_press(g, x, y, 1, Mods::NONE));
}

fn press_spot(s: &mut Studio, g: &Geometry, t: TrackId, id: PluginId, mark: Mark) {
    let n = spot(s, g, t, mark);
    press(s, g, id, Part::Spot(n));
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

/// The value of `key` in the song, plain.
fn plain(s: &Studio, t: TrackId, key: &str) -> f64 {
    let x = plugin(s, t);
    let def = s.instruments.def_of(&x).unwrap();
    let k = def.params.iter().position(|p| p.id == key).expect(key);
    def.params[k].to_plain(values_of(&x, def)[k])
}

#[test]
fn ui_333_an_empty_sampler_is_the_drop_plane_and_browse_opens_the_files() {
    let (mut s, g, t, id) = window("Sampler");
    let b = body(&s, &g, t);
    // Nothing to set until a sample is in: the plane and its button.
    assert!(b.slots.is_empty());
    let drop = &b.spots[spot(&s, &g, t, Mark::Drop)];
    let browse = &b.spots[spot(&s, &g, t, Mark::Browse)];
    assert_eq!(drop.text, DROP_LABEL);
    assert_eq!(browse.text, BROWSE_LABEL);
    assert!(drop.r.contains(browse.r.x, browse.r.y));
    // Browse opens the files; a second press keeps them open.
    s.browser.open = false;
    press_spot(&mut s, &g, t, id, Mark::Browse);
    assert!(s.browser.open);
    assert_eq!(s.browser.tab, RailTab::Files);
    press_spot(&mut s, &g, t, id, Mark::Browse);
    assert!(s.browser.open);
    assert_eq!(s.browser.tab, RailTab::Files);
}

#[test]
fn ui_333_the_zones_choose_whose_sample_the_knobs_set_in_one_step() {
    let (mut s, g, t, id) = window("Sampler");
    let empty = s.instruments.open[0].body;
    add(&mut s, id, &["Piano C3.wav", "Piano C5.wav"]);
    // The window grows to show the sample and the voice.
    assert!(s.instruments.open[0].filled);
    assert!(s.instruments.open[0].body.1 > empty.1);
    let b = body(&s, &g, t);
    let zones: Vec<_> = b
        .spots
        .iter()
        .filter_map(|x| match x.mark {
            Mark::Zone(f) => Some(f),
            _ => None,
        })
        .collect();
    assert_eq!(zones, [0, 1]);
    // The first zone is chosen; its sample's knobs are `file1.*`.
    let x = plugin(&s, t);
    let def = s.instruments.def_of(&x).unwrap();
    assert!(def.param("file1.start").is_some());
    assert!(def.param("file2.start").is_none());
    press_spot(&mut s, &g, t, id, Mark::Zone(1));
    assert_eq!(s.instruments.open[0].pick, 1);
    let x = plugin(&s, t);
    let def = s.instruments.def_of(&x).unwrap();
    let (k, _) = def.param("file2.start").expect("the second zone's");
    // A typed value goes in once, on the second file only.
    let steps = s.song().steps();
    assert!(s.device_set(id, k, 0.25));
    assert_eq!(s.song().steps(), steps + 1);
    assert!((plain(&s, t, "file2.start") - 25.0).abs() < 1e-6);
    let first = plugin(&s, t).params.get("file1.start").copied();
    assert!(first.is_none_or(|v| v == 0.0));
    s.undo(&g);
    assert_eq!(plain(&s, t, "file2.start"), 0.0);
}

#[test]
fn ui_334_a_pad_chooses_its_controls_and_m_s_banks_set_and_pick() {
    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Kick.wav", "Snare.wav"]);
    assert!(s.instruments.open[0].filled);
    // The second pad chosen: its title and its `A2.*` controls.
    press(&mut s, &g, id, Part::Pad(37));
    assert_eq!(s.instruments.open[0].pick, 1);
    let b = body(&s, &g, t);
    let title = b.spots.iter().find(|x| x.mark == Mark::Title).unwrap();
    assert!(title.text.contains("A2"), "{}", title.text);
    let x = plugin(&s, t);
    assert!(
        s.instruments
            .def_of(&x)
            .unwrap()
            .param("A2.cutoff")
            .is_some()
    );
    // M is one step and one undo.
    let steps = s.song().steps();
    press_spot(&mut s, &g, t, id, Mark::Mute(0));
    assert_eq!(s.song().steps(), steps + 1);
    assert_eq!(plugin(&s, t).params.get("A1.mute").copied(), Some(1.0));
    assert!(body(&s, &g, t).spots[spot(&s, &g, t, Mark::Mute(0))].on);
    press_spot(&mut s, &g, t, id, Mark::Solo(1));
    assert_eq!(plugin(&s, t).params.get("A2.solo").copied(), Some(1.0));
    s.undo(&g);
    s.undo(&g);
    assert_eq!(
        plugin(&s, t).params.get("A1.mute").copied().unwrap_or(0.0),
        0.0
    );
    // A bank keeps the pad's place: A2 → B2.
    press_spot(&mut s, &g, t, id, Mark::Bank(1));
    assert_eq!(s.instruments.open[0].pick, 17);
    assert!(body(&s, &g, t).spots[spot(&s, &g, t, Mark::Bank(1))].on);
}

#[test]
fn ui_334_layers_choose_the_wave_and_files_dropped_on_a_pad_go_there() {
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<PathBuf> = ["Tom_v1.wav", "Tom_v2.wav"]
        .iter()
        .map(|n| {
            let f = dir.path().join(n);
            std::fs::write(&f, b"").unwrap();
            f
        })
        .collect();
    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Kick.wav"]);
    // Dropped on A3, the two layers of one tom fill it.
    let (x, y) = s.instrument_center(&g, id, Part::Pad(38)).expect("A3");
    assert!(s.window_drop(&g, x, y, &files), "{:?}", s.error);
    let x = plugin(&s, t);
    assert_eq!(x.samples.len(), 3);
    assert_eq!(s.instruments.open[0].pick, 2);
    let def = x.builtin().unwrap();
    assert_eq!(sampler::pad_files(def, &x, 2), [1, 2]);
    // The layers of the pad: the second one chosen shows its file.
    press_spot(&mut s, &g, t, id, Mark::Layer(1));
    let w = s.instruments.open[0];
    assert_eq!(w.layer, 1);
    assert_eq!(
        sampler::wave_file(&plugin(&s, t), w.pick, w.layer),
        Some(x.samples[2].as_str())
    );
    // Off the window, a drop is not the window's.
    assert!(!s.window_drop(&g, 1.0, 1.0, &files));
}

/// The window `dy` taller than it opened.
fn taller(s: &mut Studio, dy: f32) {
    s.instruments.open[0].body.1 += dy;
}

/// M-ui-6 (UI-331): a taller Sampler gives the height to its wave, the keys move down at
/// their size, and they are wide enough for their C names.
#[test]
fn ui_331_a_taller_sampler_grows_its_wave_and_keeps_the_keys() {
    let (mut s, g, t, id) = window("Sampler");
    add(&mut s, id, &["Piano C3.wav"]);
    let a = body(&s, &g, t);
    taller(&mut s, 120.0);
    let b = body(&s, &g, t);
    assert_eq!(b.graphs[0].h, a.graphs[0].h + 120.0);
    for (ka, kb) in a.keys.iter().zip(&b.keys) {
        assert_eq!((kb.r.y, kb.r.w, kb.r.h), (ka.r.y + 120.0, ka.r.w, ka.r.h));
    }
    for (sa, sb) in a.slots.iter().zip(&b.slots) {
        assert_eq!((sb.cell.w, sb.cell.h), (sa.cell.w, sa.cell.h));
    }
    let span = b.keys.last().unwrap().r.right() - b.keys[0].r.x;
    assert!(span + 2.0 * layout::PAD > 520.0, "the C names show: {span}");
}

/// M-ui-6 (UI-331): a taller Drum Kit gives the height to its pads (and its wave beside
/// them), empty or not; a pad's M and S stay at its foot and the knobs keep their size.
#[test]
fn ui_331_a_taller_drum_kit_grows_its_pads() {
    for files in [&[][..], &["Kick.wav", "Snare.wav"][..]] {
        let (mut s, g, t, id) = window("Drum Kit");
        if !files.is_empty() {
            add(&mut s, id, files);
        }
        let a = body(&s, &g, t);
        taller(&mut s, 120.0);
        let b = body(&s, &g, t);
        let grid = |b: &Body| {
            let top = b.pads.iter().map(|k| k.r.y).fold(f32::MAX, f32::min);
            let foot = b.pads.iter().map(|k| k.r.bottom()).fold(0.0, f32::max);
            (top, foot)
        };
        let ((t0, f0), (t1, f1)) = (grid(&a), grid(&b));
        assert_eq!(t1, t0, "{files:?}");
        assert!((f1 - (f0 + 120.0)).abs() < 0.01, "{files:?}: {f0} → {f1}");
        assert!(b.pads[0].r.h > a.pads[0].r.h);
        for k in &b.pads {
            let p = play::pad_of(k.pitch);
            let on = b.spots.iter().find(|x| x.mark == Mark::Pad(p)).unwrap();
            assert_eq!(on.r, k.r);
            if let Some(m) = b.spots.iter().find(|x| x.mark == Mark::Mute(p)) {
                assert!((m.r.bottom() + layout::MS_FOOT - k.r.bottom()).abs() < 0.01);
            }
        }
        for (sa, sb) in a.slots.iter().zip(&b.slots) {
            assert_eq!((sb.cell.w, sb.cell.h), (sa.cell.w, sa.cell.h), "{files:?}");
            let on_pad = b.pads.iter().any(|k| {
                let (r, c) = (k.r, sb.cell);
                c.x < r.right() && r.x < c.right() && c.y < r.bottom() && r.y < c.bottom()
            });
            assert!(!on_pad, "{files:?}: a knob on a pad");
        }
        if !files.is_empty() {
            assert_eq!(b.graphs[0].h, a.graphs[0].h + 120.0);
        }
    }
}

#[test]
fn ui_333_the_spots_read_as_buttons() {
    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Kick.wav"]);
    let marks = crate::a11y::marks(&s, &g);
    let find = |label: &str| {
        marks
            .iter()
            .find(|m| m.node.label() == Some(label))
            .unwrap_or_else(|| panic!("{label}"))
    };
    let mute = find("Mute A1");
    assert_eq!(mute.node.role(), accesskit::Role::Button);
    assert_eq!(mute.node.toggled(), Some(accesskit::Toggled::False));
    assert_eq!(find("Bank A").node.is_selected(), Some(true));
    find("Previous sample");
    find("Next sample");
    let _ = t;
}
