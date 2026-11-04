//! UI-333, UI-334: the Sampler's and the Drum Kit's windows, without a window.

use std::path::PathBuf;

use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;

use super::graph;
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

/// N-smp-6 (UI-333): files of one root (no note in their names) and a zone off the keys can
/// each be chosen.
#[test]
fn ui_333_zones_of_one_root_and_off_the_keys_can_each_be_chosen() {
    let (mut s, g, t, id) = window("Sampler");
    add(
        &mut s,
        id,
        &[
            "Kick.wav",
            "Snare.wav",
            "Hat.wav",
            "Clap.wav",
            "Sub C0.wav",
            "Bell C8.wav",
        ],
    );
    for f in 0..6 {
        let n = spot(&s, &g, t, Mark::Zone(f));
        let r = body(&s, &g, t).spots[n].r;
        assert!(r.w >= 4.0, "{f}: {r:?}");
        press_spot(&mut s, &g, t, id, Mark::Zone(f));
        assert_eq!(s.instruments.open[0].pick, f, "zone {f}");
    }
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

/// UI-333: the loop's X-Fade is a small knob after Loop, there only while Loop is on, in
/// the Sampler and in the Drum Kit.
#[test]
fn ui_333_x_fade_follows_loop_while_it_is_on() {
    for (name, of) in [("Sampler", "file1"), ("Drum Kit", "A1")] {
        let (mut s, g, t, id) = window(name);
        add(&mut s, id, &["Pad C3.wav"]);
        let slot_of = |s: &Studio, id: &str| {
            let x = plugin(s, t);
            let def = s.instruments.def_of(&x).unwrap();
            let k = def.param(id).map(|(k, _)| k);
            body(s, &g, t).slots.into_iter().find(|x| Some(x.k) == k)
        };
        assert!(
            slot_of(&s, &format!("{of}.xfade")).is_none(),
            "{name}: not while off"
        );
        let lp = slot_of(&s, &format!("{of}.loop")).expect("Loop");
        press(&mut s, &g, id, Part::Knob(lp.k));
        assert!(plain(&s, t, &format!("{of}.loop")) >= 0.5, "{name}");
        let xf = slot_of(&s, &format!("{of}.xfade")).expect("X-Fade while on");
        let lp = slot_of(&s, &format!("{of}.loop")).unwrap();
        let rv = slot_of(&s, &format!("{of}.reverse")).unwrap();
        assert_eq!(xf.dial, 20.0);
        assert!(
            xf.cell.x >= lp.cell.right() && xf.cell.right() <= rv.cell.x,
            "{name}"
        );
        // It turns as a knob does: one step.
        let steps = s.song().steps();
        assert!(s.device_set(id, xf.k, 0.5));
        assert_eq!(s.song().steps(), steps + 1);
        assert!(
            (plain(&s, t, &format!("{of}.xfade")) - 50.0).abs() < 1e-6,
            "{name}"
        );
    }
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

/// Empty files named `names` in a folder of their own.
fn wavs(names: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let files = names
        .iter()
        .map(|n| {
            let f = dir.path().join(n);
            std::fs::write(&f, b"").unwrap();
            f
        })
        .collect();
    (dir, files)
}

/// N-smp-3, N-smp-4 (UI-334): with Shift the files dropped are the layers of one pad, and a
/// drop off the pads goes to the pad chosen (an empty one), which stays chosen.
#[test]
fn ui_334_shift_makes_layers_and_a_drop_off_the_pads_goes_to_the_pad_chosen() {
    let (_dir, files) = wavs(&["Snare_a.wav", "Snare_b.wav"]);
    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Kick.wav"]);
    let (x, y) = s.instrument_center(&g, id, Part::Pad(41)).expect("A6");
    s.instruments.drop_layers = true;
    assert!(s.window_drop(&g, x, y, &files), "{:?}", s.error);
    let k = plugin(&s, t);
    let def = k.builtin().unwrap();
    assert_eq!(sampler::pad_files(def, &k, 5), [1, 2]);
    assert!(sampler::pad_files(def, &k, 6).is_empty());
    // Taken once: the next drop without Shift spreads.
    assert!(!s.instruments.drop_layers);
    // B7 chosen and empty: a drop on its surface fills it.
    let (_dir2, more) = wavs(&["Clap.wav"]);
    s.instruments.pick(id, 30, 0);
    let r = body(&s, &g, t).spots[spot(&s, &g, t, Mark::Drop)].r;
    let (x, y) = (r.x + r.w / 2.0, r.y + 12.0);
    assert!(s.window_drop(&g, x, y, &more), "{:?}", s.error);
    let k = plugin(&s, t);
    assert_eq!(sampler::pad_files(k.builtin().unwrap(), &k, 30), [3]);
    assert_eq!(s.instruments.open[0].pick, 30);
    // A file that is not a sound among them is passed over and named (N-smp-5).
    let (_dir4, mixed) = wavs(&["Tom.wav", "notes.txt"]);
    s.error = None;
    assert!(s.window_drop(&g, x, y, &mixed), "{:?}", s.error);
    let k = plugin(&s, t);
    assert_eq!(sampler::pad_files(k.builtin().unwrap(), &k, 31), [4]);
    assert!(
        s.error.as_deref().is_some_and(|e| e.contains("notes.txt")),
        "{:?}",
        s.error
    );
    // A1 is full: a drop there goes to the next empty pad, which is chosen.
    s.instruments.pick(id, 0, 0);
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    let (_dir3, last) = wavs(&["Rim.wav"]);
    assert!(s.window_drop(&g, x, y, &last), "{:?}", s.error);
    let k = plugin(&s, t);
    assert_eq!(sampler::pad_files(k.builtin().unwrap(), &k, 1), [5]);
    assert_eq!(s.instruments.open[0].pick, 1);
}

/// The velocities the layers of the Drum Kit's pad `pad` play.
fn ranges(s: &Studio, t: TrackId, pad: usize) -> Vec<(u8, u8)> {
    let k = plugin(s, t);
    let def = k.builtin().unwrap();
    sampler::velocities(def, &k, &sampler::pad_files(def, &k, pad))
}

/// UI-334, L2-smp-7: the edge between two velocity layers drags where the first ends (a
/// quarter as fast with Shift), in one step and no further than the layers around it; once
/// pressed, ← and → move it by 4 (1 with Shift); Esc while it is held takes nothing in. A
/// layer of a velocity or two is still wide enough to press.
#[test]
fn ui_334_the_edge_between_velocity_layers_drags_and_steps() {
    let (_dir, files) = wavs(&["Snare_a.wav", "Snare_b.wav", "Snare_c.wav"]);
    let (mut s, g, t, id) = window("Drum Kit");
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    s.instruments.drop_layers = true;
    assert!(s.window_drop(&g, x, y, &files), "{:?}", s.error);
    assert_eq!(ranges(&s, t, 0), [(0, 42), (43, 84), (85, 127)]);
    let b = body(&s, &g, t);
    assert!(b.spots.iter().any(|x| x.mark == Mark::Edge(1)));
    assert!(
        !b.spots.iter().any(|x| x.mark == Mark::Edge(2)),
        "none after the last"
    );
    let strip: f32 = b
        .spots
        .iter()
        .filter(|x| matches!(x.mark, Mark::Layer(_)))
        .map(|x| x.r.w + 2.0)
        .sum();
    // A drag of ten velocities: one step, and the screen follows it before.
    let n = spot(&s, &g, t, Mark::Edge(0));
    let (x, y) = s.instrument_center(&g, id, Part::Spot(n)).unwrap();
    let steps = s.song().steps();
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    let dx = strip / 128.0 * 10.0;
    assert!(s.instrument_moved(&g, x + dx, y, false));
    assert_eq!(s.song().steps(), steps);
    let shown = body(&s, &g, t);
    let at = |b: &Body, m: Mark| b.spots.iter().find(|x| x.mark == m).unwrap().r.x;
    assert!(at(&shown, Mark::Edge(0)) > at(&b, Mark::Edge(0)) + dx / 2.0);
    assert!(s.instrument_up());
    assert_eq!(s.song().steps(), steps + 1);
    assert_eq!(ranges(&s, t, 0), [(0, 52), (53, 84), (85, 127)]);
    // With Shift, a quarter as far.
    let n = spot(&s, &g, t, Mark::Edge(0));
    let (x, y) = s.instrument_center(&g, id, Part::Spot(n)).unwrap();
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, x + dx * 4.0, y, true));
    assert!(s.instrument_up());
    assert_eq!(ranges(&s, t, 0)[0], (0, 62));
    // No further than one under the next layer's top.
    let n = spot(&s, &g, t, Mark::Edge(0));
    let (x, y) = s.instrument_center(&g, id, Part::Spot(n)).unwrap();
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, x + strip, y, false));
    assert!(s.instrument_up());
    assert_eq!(ranges(&s, t, 0), [(0, 83), (84, 84), (85, 127)]);
    // The layer of one velocity can still be pressed.
    let one = body(&s, &g, t);
    let r = one
        .spots
        .iter()
        .find(|x| x.mark == Mark::Layer(1))
        .unwrap()
        .r;
    assert!(r.w >= 10.0, "{}", r.w);
    press_spot(&mut s, &g, t, id, Mark::Layer(1));
    assert_eq!(s.instruments.open[0].layer, 1);
    // Pressed, ← and → move it.
    press_spot(&mut s, &g, t, id, Mark::Edge(0));
    assert!(s.instrument_up());
    let steps = s.song().steps();
    assert!(s.field_keys(&g, "left", None, Mods::NONE));
    assert_eq!(ranges(&s, t, 0)[0], (0, 79));
    let shift = Mods {
        shift: true,
        ..Mods::NONE
    };
    assert!(s.field_keys(&g, "left", None, shift));
    assert_eq!(ranges(&s, t, 0)[0], (0, 78));
    assert_eq!(s.song().steps(), steps + 2);
    // Esc while it is held: nothing goes in.
    let n = spot(&s, &g, t, Mark::Edge(1));
    let (x, y) = s.instrument_center(&g, id, Part::Spot(n)).unwrap();
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, x - dx, y, false));
    assert!(s.field_keys(&g, "escape", None, Mods::NONE));
    assert!(!s.instrument_up());
    assert_eq!(s.song().steps(), steps + 2);
    assert_eq!(ranges(&s, t, 0)[1], (79, 84));
    // Another press lets the keys go.
    press_spot(&mut s, &g, t, id, Mark::Layer(0));
    assert_eq!(s.instruments.edge, None);
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
    assert!(span > 520.0, "the C names show: {span}");
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

/// The labels of the menu open, gaps left out.
fn menu(s: &Studio) -> Vec<String> {
    s.context_rows()
        .into_iter()
        .filter(|r| !r.gap)
        .map(|r| r.label)
        .collect()
}

/// Whether the menu's row `label` can be chosen.
fn live_row(s: &Studio, label: &str) -> bool {
    s.context_rows()
        .iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}: {:?}", menu(s)))
        .enabled
}

/// Chooses the menu's row `label`.
fn choose(s: &mut Studio, g: &Geometry, label: &str) {
    let at = s
        .context_rows()
        .iter()
        .position(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}: {:?}", menu(s)));
    s.context_pick(g, at);
}

/// A right press on the spot `mark`.
fn right_spot(s: &mut Studio, g: &Geometry, t: TrackId, id: PluginId, mark: Mark) {
    let n = spot(s, g, t, mark);
    let (x, y) = s.instrument_center(g, id, Part::Spot(n)).expect("in sight");
    assert!(s.instrument_right(g, x, y));
}

/// UI-333: the sample's name opens its menu: another file in its place (the zone's values
/// as they were), shown in the browser when a tab holds it, the zone or all of them taken
/// away; a zone's right press chooses it and takes it away.
#[test]
fn ui_333_the_sample_name_and_a_zone_have_their_menus() {
    let (dir, files) = wavs(&["Kick.wav", "Snare.wav", "Clap.wav"]);
    let (mut s, g, t, id) = window("Sampler");
    let paths: Vec<_> = files[..2]
        .iter()
        .map(|f| json!({ "path": f.to_string_lossy() }))
        .collect();
    assert!(
        s.run_command(
            "plugin.samples.load",
            json!({ "id": id.to_string(), "paths": paths }),
        ),
        "{:?}",
        s.error
    );
    press_spot(&mut s, &g, t, id, Mark::Name);
    assert_eq!(
        menu(&s),
        [
            "サンプルを置き換え…",
            "ブラウザーで表示",
            "このゾーンを外す",
            "すべてのサンプルを外す"
        ]
    );
    assert!(!live_row(&s, "ブラウザーで表示"), "no tab holds it");
    // Another file in the zone's place: one step, the zone's start as it was.
    let w = s.instruments.open[0];
    let key = format!("file{}.start", w.pick + 1);
    let set = json!({ "id": id.to_string(), "param": key, "value": 0.25 });
    assert!(s.run_command("plugin.param.set", set), "{:?}", s.error);
    let start = |s: &Studio| plugin(s, t).params.get(&key).copied();
    let before = (s.song().steps(), start(&s));
    assert_eq!(before.1, Some(0.25));
    choose(&mut s, &g, "サンプルを置き換え…");
    let request = s.file_request.take().expect("the OS's window");
    assert_eq!(request, crate::studio::FileRequest::Sample(id, w.pick));
    s.finish_file_request(request, &files[2]);
    let x = plugin(&s, t);
    assert_eq!(x.samples.len(), 2);
    assert!(x.samples[w.pick].ends_with("Clap.wav"), "{:?}", x.samples);
    assert_eq!((s.song().steps(), start(&s)), (before.0 + 1, before.1));
    // A tab that holds it: the browser's Files show it chosen.
    s.browser.add_place_fixture(dir.path()).unwrap();
    s.browser.set_location(0);
    press_spot(&mut s, &g, t, id, Mark::Name);
    assert!(live_row(&s, "ブラウザーで表示"));
    choose(&mut s, &g, "ブラウザーで表示");
    assert!(s.browser.open && s.browser.tab == RailTab::Files);
    assert_eq!(
        s.browser.selected_file.as_deref(),
        Some(files[2].to_string_lossy().as_ref())
    );
    // A zone's right press chooses it; its one row takes it away.
    let other = 1 - w.pick;
    right_spot(&mut s, &g, t, id, Mark::Zone(other));
    assert_eq!(s.instruments.open[0].pick, other);
    assert_eq!(menu(&s), ["このゾーンを外す"]);
    choose(&mut s, &g, "このゾーンを外す");
    assert_eq!(plugin(&s, t).samples.len(), 1);
    // All of them.
    press_spot(&mut s, &g, t, id, Mark::Name);
    choose(&mut s, &g, "すべてのサンプルを外す");
    assert!(plugin(&s, t).samples.is_empty());
}

/// UI-334: a pad's right press chooses it; its menu puts another file in place of its
/// sample (not on an empty pad), adds files as its layers (on a pad that has some too),
/// empties it or its bank. A layer's takes it away while the pad has another.
#[test]
fn ui_334_a_pad_and_a_layer_have_their_menus() {
    let (_dir, files) = wavs(&["Kick.wav", "Snare_a.wav", "Snare_b.wav", "Hat.wav"]);
    let (mut s, g, t, id) = window("Drum Kit");
    // A2, empty: chosen, nothing to put a file in place of.
    let (x, y) = s.instrument_center(&g, id, Part::Pad(37)).expect("A2");
    assert!(s.instrument_right(&g, x, y));
    assert_eq!(s.instruments.open[0].pick, 1);
    assert_eq!(
        menu(&s),
        [
            "サンプルを置き換え…",
            "サンプルを足す…",
            "パッドを空にする",
            "バンクを空にする"
        ]
    );
    assert!(!live_row(&s, "サンプルを置き換え…"));
    assert!(!live_row(&s, "パッドを空にする"));
    // Two files added: the layers of A2, in one step.
    choose(&mut s, &g, "サンプルを足す…");
    let request = s.file_request.take().expect("the OS's window");
    assert_eq!(request, crate::studio::FileRequest::PadSamples(id, 1));
    let steps = s.song().steps();
    s.finish_picked(request, &files[1..3]);
    let k = plugin(&s, t);
    let def = k.builtin().unwrap();
    assert_eq!(sampler::pad_files(def, &k, 1), [0, 1]);
    assert_eq!(s.song().steps(), steps + 1);
    // A2 has files: one more goes on it too.
    assert!(s.instrument_right(&g, x, y));
    assert!(live_row(&s, "サンプルを置き換え…"));
    choose(&mut s, &g, "サンプルを足す…");
    let request = s.file_request.take().unwrap();
    s.finish_picked(request, &files[3..]);
    let k = plugin(&s, t);
    assert_eq!(sampler::pad_files(def, &k, 1), [0, 1, 2]);
    // Its sample put in another's place: the first layer, the pad's files as many.
    assert!(s.instrument_right(&g, x, y));
    choose(&mut s, &g, "サンプルを置き換え…");
    let request = s.file_request.take().unwrap();
    assert_eq!(request, crate::studio::FileRequest::Sample(id, 0));
    s.finish_file_request(request, &files[0]);
    let k = plugin(&s, t);
    assert!(k.samples[0].ends_with("Kick.wav"), "{:?}", k.samples);
    assert_eq!(sampler::pad_files(def, &k, 1), [0, 1, 2]);
    // A layer's right press does not choose it; it takes it away.
    right_spot(&mut s, &g, t, id, Mark::Layer(2));
    assert_eq!(s.instruments.open[0].layer, 0);
    assert_eq!(menu(&s), ["このレイヤーを外す"]);
    assert!(live_row(&s, "このレイヤーを外す"));
    choose(&mut s, &g, "このレイヤーを外す");
    let k = plugin(&s, t);
    assert_eq!(sampler::pad_files(def, &k, 1), [0, 1]);
    // The name's menu, a Drum Kit's: its layer, not the zone or all.
    press_spot(&mut s, &g, t, id, Mark::Name);
    assert_eq!(
        menu(&s),
        [
            "サンプルを置き換え…",
            "ブラウザーで表示",
            "このレイヤーを外す"
        ]
    );
    // B1 has a file too; A's bank emptied leaves it.
    let b1 = json!({
        "id": id.to_string(),
        "paths": [{ "path": files[3].to_string_lossy() }],
        "pad": "B1",
    });
    assert!(s.run_command("plugin.samples.load", b1), "{:?}", s.error);
    assert!(s.instrument_right(&g, x, y));
    choose(&mut s, &g, "バンクを空にする");
    let k = plugin(&s, t);
    assert_eq!(k.samples.len(), 1);
    assert_eq!(def.file_pad(&k.params, 0), 16);
    // The one layer left on a pad cannot be taken away alone.
    s.instruments.pick(id, 16, 0);
    right_spot(&mut s, &g, t, id, Mark::Layer(0));
    assert!(!live_row(&s, "このレイヤーを外す"));
}

/// UI-333: the filter's point's menu: its type, and open (no filter), which stands for
/// 「既定に戻す」.
#[test]
fn ui_333_the_filter_point_chooses_its_type_and_opens() {
    let (mut s, g, t, id) = window("Sampler");
    add(&mut s, id, &["Kick.wav"]);
    let (x, y) = s
        .instrument_center(&g, id, Part::Point(2, 0))
        .expect("the filter");
    assert!(s.instrument_right(&g, x, y));
    let rows = menu(&s);
    assert_eq!(
        &rows[..4],
        ["LP", "BP", "HP", "フィルターを開く（かけない）"]
    );
    assert!(!rows.iter().any(|r| r == "既定に戻す"), "{rows:?}");
    choose(&mut s, &g, "HP");
    assert_eq!(plain(&s, t, "ftype"), 2.0);
    assert!(s.instrument_right(&g, x, y));
    choose(&mut s, &g, "フィルターを開く（かけない）");
    assert_eq!(plain(&s, t, "ftype"), 0.0);
    assert_eq!(plain(&s, t, "cutoff"), 20_000.0);
}

/// UI-334: a pad's state as the mock draws it: one with files apart from an empty one, a
/// muted one and the others while one is soloed fainter; a press higher up the pad is
/// harder, and the pad stays lit a moment after a short one.
#[test]
fn ui_334_pads_show_files_mute_and_solo_and_a_press_lights_them() {
    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Kick.wav", "Snare.wav", "Hat.wav"]);
    let pad = |s: &Studio, p: usize| {
        body(s, &g, t)
            .spots
            .into_iter()
            .find(|x| x.mark == Mark::Pad(p))
            .unwrap()
    };
    assert!(pad(&s, 0).dot && !pad(&s, 3).dot, "files, and none");
    assert!(!pad(&s, 0).dim);
    // Muted: fainter. Another soloed: all but it fainter, empty pads as they were.
    press_spot(&mut s, &g, t, id, Mark::Mute(1));
    assert!(pad(&s, 1).dim && !pad(&s, 0).dim);
    press_spot(&mut s, &g, t, id, Mark::Solo(2));
    assert!(pad(&s, 0).dim && pad(&s, 1).dim && !pad(&s, 2).dim && !pad(&s, 3).dim);
    // Higher up is harder: 127 at the top, 1 at the foot.
    assert_eq!(crate::instrument::window::pad_velocity(0.0, 60.0), 127);
    assert_eq!(crate::instrument::window::pad_velocity(60.0, 60.0), 1);
    assert_eq!(crate::instrument::window::pad_velocity(30.0, 60.0), 64);
    let r = body_rect(&s, &g, t, 37);
    let velocity = |s: &Studio| match s.live_midi.sent.last() {
        Some((_, xirius_engine::NoteKind::On { velocity, .. })) => *velocity,
        other => panic!("{other:?}"),
    };
    assert!(s.instrument_press(&g, r.x + r.w / 2.0, r.y + 4.0, 1, Mods::NONE));
    let high = velocity(&s);
    s.instrument_up();
    assert!(s.instrument_press(&g, r.x + r.w / 2.0, r.bottom() - 4.0, 1, Mods::NONE));
    let low = velocity(&s);
    assert!(high > 4.0 * low, "{high} {low}");
    // Let go at once, it is still lit; a moment on, not.
    s.instrument_up();
    let now = std::time::Instant::now();
    assert_eq!(s.instruments.lit_pads(id, now), [37]);
    let later = now + crate::instrument::window::HIT;
    assert!(s.instruments.lit_pads(id, later).is_empty());
}

/// Where the pad playing `pitch` is in the window.
fn body_rect(s: &Studio, g: &Geometry, t: TrackId, pitch: u8) -> crate::studio::Rect {
    body(s, g, t)
        .pads
        .iter()
        .find(|k| k.pitch == pitch)
        .unwrap()
        .r
}

/// UI-333: a Sampler's key chooses the zone that plays it.
#[test]
fn ui_333_a_key_chooses_the_zone_that_plays_it() {
    let (mut s, g, _t, id) = window("Sampler");
    add(&mut s, id, &["Piano_C3.wav", "Piano_C4.wav"]);
    let low = s.instruments.open[0].pick;
    press(&mut s, &g, id, Part::Key(72));
    s.instrument_up();
    let high = s.instruments.open[0].pick;
    assert_ne!(low, high);
    press(&mut s, &g, id, Part::Key(60));
    s.instrument_up();
    assert_eq!(s.instruments.open[0].pick, low);
}

/// UI-334: files dragged over a Drum Kit show, before they fall, the pads they fill (their
/// names and layers, no M and S) and their banks; the chip says it in a word. Shift makes
/// one pad; off the window, nothing.
#[test]
fn ui_334_a_drag_shows_the_pads_it_fills() {
    let (dir, files) = wavs(&["Kick.wav", "Snare_v1.wav", "Snare_v2.wav", "notes.txt"]);
    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Hat.wav"]);
    // Over A1, which has a file: the free pads from it.
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    assert!(s.window_drag(&g, x, y, &files, false));
    let v = s.instruments.preview.clone().expect("a plan");
    assert_eq!(v.text, "2 pads, A2–A3");
    assert_eq!(
        v.pads,
        [
            (1, "Kick".to_string(), String::new()),
            (2, "Snare_v1".to_string(), "Vel 2".to_string())
        ]
    );
    // The same again changes nothing.
    assert!(!s.window_drag(&g, x, y, &files, false));
    let mut b = body(&s, &g, t);
    crate::instrument::kit_drag::ghost(&mut b, &v);
    let a2 = b.spots.iter().find(|x| x.mark == Mark::Pad(1)).unwrap();
    assert!(a2.ghost && a2.text == "Kick");
    assert!(b.spots.iter().any(|x| x.mark == Mark::Bank(0) && x.ghost));
    assert!(b.spots.iter().any(|x| x.mark == Mark::Bank(1) && !x.ghost));
    assert!(
        b.spots.iter().any(|x| x.mark == Mark::Mute(0)),
        "A1 keeps its M"
    );
    // Shift: all of them on A1, with its file.
    assert!(s.window_drag(&g, x, y, &files, true));
    let v = s.instruments.preview.clone().unwrap();
    assert_eq!(v.text, "A1: 3 layers");
    assert_eq!(v.pads, [(0, "Hat".to_string(), "Vel 4".to_string())]);
    let mut b = body(&s, &g, t);
    crate::instrument::kit_drag::ghost(&mut b, &v);
    assert!(
        !b.spots.iter().any(|x| x.mark == Mark::Mute(0)),
        "no M on it"
    );
    // One file: its name.
    assert!(s.window_drag(&g, x, y, &files[..1], false));
    assert_eq!(s.instruments.preview.as_ref().unwrap().text, "A2: Kick");
    // A folder: from the pad, its name.
    assert!(s.window_drag(&g, x, y, &[dir.path().to_path_buf()], false));
    let v = s.instruments.preview.clone().unwrap();
    let folder = dir.path().file_name().unwrap().to_string_lossy();
    assert_eq!(v.text, format!("A1–: {folder}"));
    // Off the window, and once the drag leaves: nothing.
    assert!(s.window_drag(&g, 1.0, 1.0, &files, false));
    assert!(s.instruments.preview.is_none());
    s.window_drag(&g, x, y, &files, false);
    s.drop_hover(false);
    assert!(s.instruments.preview.is_none());
}

/// UI-333, UI-334: while a voice sounds, a line on the wave shows where it reads the file
/// shown (between Start and End) and a dot rides the envelope of the pad it plays; another
/// layer's or pad's voice shows neither, and once it ends nothing does.
#[test]
fn ui_333_the_voice_shows_where_it_reads_and_rides_the_envelope() {
    use xirius_engine::stock::{VoiceStage, VoiceView};
    let voice = |source: u16, stage: VoiceStage, env: f32| VoiceView {
        pitch: 60,
        stage,
        file: true,
        source,
        position: 0.5,
        env,
    };
    let (mut s, _g, t, id) = window("Sampler");
    add(&mut s, id, &["Piano_C3.wav", "Piano_C4.wav"]);
    let now = std::time::Instant::now();
    assert!(
        s.instruments
            .hear(id, Some(voice(0, VoiceStage::Attack, 0.5)), now)
    );
    let x = plugin(&s, t);
    let def = s.instruments.def_of(&x).unwrap();
    let live = s.instruments.live_of(&x, def, 0, Some(0), now).unwrap();
    assert_eq!(live.read, Some(0.5));
    let env = live.env.expect("the Sampler's one envelope");
    // Another zone's wave: no line, the dot all the same.
    let other = s.instruments.live_of(&x, def, 1, Some(1), now).unwrap();
    assert!(other.read.is_none() && other.env.is_some());
    // Up the attack by its level, then along the decay and the release by their time.
    let values = values_of(&x, def);
    let r = crate::studio::Rect::new(0.0, 0.0, 300.0, 120.0);
    let at = |v: graph::Env| sampler::env_dot(def, r, &values, v);
    let half = at(env).unwrap();
    let top = at(graph::Env { level: 1.0, ..env }).unwrap();
    assert!(top.0 > half.0 && top.1 < half.1, "{half:?} {top:?}");
    let decay = |secs| graph::Env {
        stage: VoiceStage::Decay,
        level: 0.8,
        secs,
    };
    let (d0, d1, d2) = (
        at(decay(0.0)).unwrap(),
        at(decay(0.05)).unwrap(),
        at(decay(30.0)).unwrap(),
    );
    assert!(d0.0 < d1.0 && d1.0 < d2.0, "{d0:?} {d1:?} {d2:?}");
    let release = |secs| graph::Env {
        stage: VoiceStage::Release,
        level: 0.3,
        secs,
    };
    assert!(at(release(0.0)).unwrap().0 >= d2.0);
    assert!(at(release(60.0)).is_none(), "the release has ended");
    // The stage's time runs from when it began, not from each read.
    let later = now + std::time::Duration::from_millis(200);
    s.instruments
        .hear(id, Some(voice(0, VoiceStage::Attack, 0.9)), later);
    let env = s
        .instruments
        .live_of(&x, def, 0, Some(0), later)
        .unwrap()
        .env
        .unwrap();
    assert!((env.secs - 0.2).abs() < 1e-3);
    // Ended: nothing.
    assert!(!s.instruments.hear(id, None, later));
    assert!(s.instruments.live_of(&x, def, 0, Some(0), later).is_none());

    // A Drum Kit: only the pad it plays shows it, and its layer's wave the line.
    let (mut s, _g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Kick.wav", "Snare.wav"]);
    s.instruments
        .hear(id, Some(voice(1, VoiceStage::Decay, 0.7)), now);
    let x = plugin(&s, t);
    let def = s.instruments.def_of(&x).unwrap();
    let pad = |pick: usize| {
        let shown = sampler::wave_slot(&x, pick, 0);
        s.instruments.live_of(&x, def, pick, shown, now).unwrap()
    };
    assert!(pad(1).read.is_some() && pad(1).env.is_some());
    assert!(pad(0).read.is_none() && pad(0).env.is_none());
}

/// The narrow form (the mock's 520): a Drum Kit's pads centred over the pad's editor and
/// the voice under it; the Amp under the Env and the Filter, its knobs in one row; nothing
/// past the window's edges, the controls over no pad; and taller, the wave grows, not the
/// pads.
#[test]
fn ui_334_a_narrow_window_stacks_the_pads_and_puts_the_amp_under() {
    for (name, files) in [
        ("Drum Kit", &["Kick.wav", "Snare.wav"][..]),
        ("Sampler", &["Piano C3.wav"][..]),
    ] {
        let (mut s, _g, t, id) = window(name);
        add(&mut s, id, files);
        let x = plugin(&s, t);
        let def = s.instruments.def_of(&x).unwrap();
        assert_eq!(layout::min_w(def), sampler::MIN_W);
        let values = values_of(&x, def);
        let shape = layout::Shape {
            w: 300.0,
            values: &values,
            plugin: Some(&x),
            ..layout::Shape::default()
        };
        let b = layout::body(def, &shape);
        assert_eq!(b.w, sampler::MIN_W, "{name}");
        let inside = |r: crate::studio::Rect| r.x >= 0.0 && r.right() <= b.w + 0.01;
        for sl in &b.slots {
            assert!(inside(sl.cell), "{name}: {:?}", sl.cell);
            assert!(
                !b.pads.iter().any(|k| k.r.y < sl.cell.bottom()
                    && sl.cell.y < k.r.bottom()
                    && k.r.x < sl.cell.right()
                    && sl.cell.x < k.r.right()),
                "{name}: a knob on a pad"
            );
        }
        for sp in &b.spots {
            assert!(inside(sp.r), "{name}: {:?} {:?}", sp.mark, sp.r);
        }
        let cell = |id: &str| {
            let k = sampler::idx(def, id).unwrap();
            b.slots.iter().find(|x| x.k == k).unwrap().cell
        };
        let attack = cell("attack");
        let amp: Vec<_> = ["transpose", "fine", "volume", "pan", "velocity"]
            .iter()
            .map(|id| cell(id))
            .collect();
        assert!(
            amp.iter().all(|r| r.y == amp[0].y && r.y > attack.bottom()),
            "{name}"
        );
        assert!(b.h >= amp[0].bottom(), "{name}");
        if name == "Drum Kit" {
            let foot = b.pads.iter().map(|k| k.r.bottom()).fold(0.0, f32::max);
            let left = b.pads.iter().map(|k| k.r.x).fold(f32::MAX, f32::min);
            let right = b.pads.iter().map(|k| k.r.right()).fold(0.0, f32::max);
            assert!(((left + right) / 2.0 - b.w / 2.0).abs() < 1.0);
            assert!(right - left <= 340.0);
            let title = b.spots.iter().find(|x| x.mark == Mark::Title).unwrap();
            assert!(title.r.y > foot && b.graphs[0].y > foot);
            let grown = b.clone().grown(b.h + 100.0);
            assert_eq!(grown.pads, b.pads, "the pads keep their size");
            assert_eq!(grown.graphs[0].h, b.graphs[0].h + 100.0);
        }
    }
}
