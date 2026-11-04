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
