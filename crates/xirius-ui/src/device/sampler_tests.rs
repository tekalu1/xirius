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
    // N4-smp-1: the four of C3 split its keys, each its own (all of them sound), and their
    // places on the keys follow in that order.
    let files = plugin(&s, t).samples;
    let c3: Vec<_> = sampler::zones(&files)
        .into_iter()
        .filter(|z| z.root == 60)
        .collect();
    assert_eq!(c3.len(), 4);
    for w in c3.windows(2) {
        assert_eq!(w[0].hi + 1, w[1].lo, "{c3:?}");
    }
    let x = |f| {
        let n = spot(&s, &g, t, Mark::Zone(f));
        body(&s, &g, t).spots[n].r
    };
    for w in c3.windows(2) {
        assert!(x(w[0].file).right() <= x(w[1].file).x, "{c3:?}");
    }
}

/// N3-smp-1 (UI-333): zones that meet at a black key meet on the line that key sits on, each
/// as wide as its keys: none is taken into its neighbour's place and shares it.
#[test]
fn n3_smp_1_zones_that_meet_at_a_black_key_meet_on_its_line() {
    let (mut s, g, t, id) = window("Sampler");
    // B1 plays ..C2, D2 plays C#2..D#2, F2 plays E2..: the middle one starts and ends on a
    // black key.
    add(&mut s, id, &["Pad B1.wav", "Pad D2.wav", "Pad F2.wav"]);
    let b = body(&s, &g, t);
    let key = |p: u8| b.keys.iter().find(|k| k.pitch == p).unwrap().r;
    let line = |p: u8| key(p).x + key(p).w / 2.0;
    let zone = |f: usize| b.spots[spot(&s, &g, t, Mark::Zone(f))].r;
    let (a, m, c) = (zone(0), zone(1), zone(2));
    // B1's zone ends at C2's right; D2's runs from C#2's line to D#2's; F2's starts at E2.
    let gap = sampler::ZONE_GAP;
    assert!((a.right() + gap - key(48).right()).abs() < 0.01, "{a:?}");
    assert!((m.x - line(49)).abs() < 0.01, "{m:?}");
    assert!((m.right() + gap - line(51)).abs() < 0.01, "{m:?}");
    assert!((c.x - key(52).x).abs() < 0.01, "{c:?}");
    // Side by side, not over each other, and the middle one as wide as one white key.
    assert!(a.right() <= m.x && m.right() <= c.x, "{a:?} {m:?} {c:?}");
    assert!((m.w + gap - (line(51) - line(49))).abs() < 0.01);
    assert!(c.w > 2.0 * m.w, "F2 plays up to C5: {c:?}");
    // Each is chosen where it is seen.
    for (f, r) in [(0, a), (1, m), (2, c)] {
        let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        assert_eq!(
            sampler::spot_at(&b, x, y).map(|n| b.spots[n].mark),
            Some(Mark::Zone(f))
        );
    }
    // Zones off the keys sit at their edges, a sliver wide, and leave the next their keys:
    // C0 plays ..F#0 and C8 G#7.., both off C1..C5.
    let (mut s, g, t, id) = window("Sampler");
    add(
        &mut s,
        id,
        &["Sub C0.wav", "Low C1.wav", "High C7.wav", "Bell C8.wav"],
    );
    let b = body(&s, &g, t);
    let zone = |f: usize| b.spots[spot(&s, &g, t, Mark::Zone(f))].r;
    let (sub, low, high, bell) = (zone(0), zone(1), zone(2), zone(3));
    let (first, last) = (b.keys[0].r, b.keys.iter().rfind(|k| !k.black).unwrap().r);
    assert!((sub.x - first.x).abs() < 0.01 && sub.w < 8.0, "{sub:?}");
    assert!(
        (bell.right() + sampler::ZONE_GAP - last.right()).abs() < 0.01 && bell.w < 8.0,
        "{bell:?}"
    );
    assert!(sub.right() <= low.x && low.right() <= high.x && high.right() <= bell.x);
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
    // A bank shows its pads and leaves the pad chosen (low 20, the mock's).
    press_spot(&mut s, &g, t, id, Mark::Bank(1));
    assert_eq!(s.instruments.open[0].pick, 1);
    let b = body(&s, &g, t);
    assert!(b.spots[spot(&s, &g, t, Mark::Bank(1))].on);
    assert!(!b.spots[spot(&s, &g, t, Mark::Bank(0))].on);
    assert!(
        b.pads
            .iter()
            .all(|k| play::pad_of(k.pitch) / play::PADS == 1)
    );
    assert!(title_of(&b).contains("A2"), "{}", title_of(&b));
    // A pad of that bank is chosen; then the tabs follow it.
    press(&mut s, &g, id, Part::Pad(36 + 18));
    assert_eq!(s.instruments.open[0].pick, 18);
    assert_eq!(s.instruments.open[0].bank, None);
    press_spot(&mut s, &g, t, id, Mark::Bank(1));
    assert_eq!(s.instruments.open[0].bank, None, "its own bank");
}

fn title_of(b: &Body) -> String {
    b.spots
        .iter()
        .find(|x| x.mark == Mark::Title)
        .map(|x| x.text.clone())
        .unwrap_or_default()
}

#[test]
fn ui_334_an_empty_pad_or_bank_has_nothing_to_take_and_an_empty_kit_chooses_pads() {
    let (mut s, g, t, id) = window("Drum Kit");
    // No samples yet: a pad pressed is chosen, and stays when its files come.
    press(&mut s, &g, id, Part::Pad(38));
    assert_eq!(s.instruments.open[0].pick, 2);
    add(&mut s, id, &["Kick.wav"]);
    assert!(s.instruments.open[0].filled);
    assert_eq!(s.instruments.open[0].pick, 2);
    let k = plugin(&s, t);
    assert_eq!(sampler::pad_files(k.builtin().unwrap(), &k, 0), [0]);
    // A2 is empty though A1 has a file: it cannot be emptied, its bank can.
    let (x, y) = s.instrument_center(&g, id, Part::Pad(37)).expect("A2");
    assert!(s.instrument_right(&g, x, y));
    assert!(!live_row(&s, "パッドを空にする"));
    assert!(live_row(&s, "バンクを空にする"));
    s.popup = None;
    // B1 of an empty bank: neither.
    press_spot(&mut s, &g, t, id, Mark::Bank(1));
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36 + 16)).expect("B1");
    assert!(s.instrument_right(&g, x, y));
    assert!(!live_row(&s, "パッドを空にする"));
    assert!(!live_row(&s, "バンクを空にする"));
    // The layers' and the zones' words are the mock's regular.
    assert_eq!(sampler::SPAN_WEIGHT, xirius_design::Weight::Regular);
}

#[test]
fn ui_334_an_edge_dragged_parts_the_layers_of_the_pad_chosen_at_the_press() {
    let (_dir, files) = wavs(&["Snare_a.wav", "Snare_b.wav"]);
    let (mut s, g, t, id) = window("Drum Kit");
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    s.instruments.drop_layers = true;
    assert!(s.window_drop(&g, x, y, &files), "{:?}", s.error);
    assert_eq!(ranges(&s, t, 0), [(0, 63), (64, 127)]);
    let n = spot(&s, &g, t, Mark::Edge(0));
    let (x, y) = s.instrument_center(&g, id, Part::Spot(n)).unwrap();
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, x + 40.0, y, false));
    // Another pad chosen while it is held (a note played on it): A1's edge goes in.
    s.instruments.pick(id, 1, 0);
    let steps = s.song().steps();
    assert!(s.instrument_up());
    assert_eq!(s.song().steps(), steps + 1);
    assert_ne!(ranges(&s, t, 0), [(0, 63), (64, 127)]);
}

/// Low 3: a layer's edge is a Slider of the velocity its layer ends at, with its range; a
/// screen reader's increment moves it one velocity and a value set puts it there, each one
/// step.
#[test]
fn ui_334_a_screen_reader_moves_a_layers_edge_as_a_slider() {
    use crate::a11y::Turn;
    let (_dir, files) = wavs(&["Snare_a.wav", "Snare_b.wav"]);
    let (mut s, g, t, id) = window("Drum Kit");
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    s.instruments.drop_layers = true;
    assert!(s.window_drop(&g, x, y, &files), "{:?}", s.error);
    assert_eq!(ranges(&s, t, 0), [(0, 63), (64, 127)]);
    let n = spot(&s, &g, t, Mark::Edge(0));
    let marks = crate::a11y::marks(&s, &g);
    let edge = marks
        .iter()
        .find(|m| m.key == format!("device-{id}-spot-{n}"))
        .expect("the edge's mark");
    assert_eq!(edge.node.role(), accesskit::Role::Slider);
    assert_eq!(edge.node.label(), Some("Layer 1 top"));
    assert_eq!(edge.node.value(), Some("63"));
    assert_eq!(edge.node.numeric_value(), Some(63.0));
    assert_eq!(edge.node.min_numeric_value(), Some(1.0));
    assert_eq!(edge.node.max_numeric_value(), Some(126.0));
    assert_eq!(edge.node.numeric_value_step(), Some(1.0));
    assert_eq!(edge.edge, Some((id, 0)));
    assert_eq!(edge.turn, None);
    let steps = s.song().steps();
    assert!(crate::a11y::edge(&mut s, id, 0, Turn::Up));
    assert_eq!(ranges(&s, t, 0), [(0, 64), (65, 127)]);
    assert!(crate::a11y::edge(&mut s, id, 0, Turn::To(100.0)));
    assert_eq!(ranges(&s, t, 0), [(0, 100), (101, 127)]);
    assert_eq!(s.song().steps(), steps + 2);
    // Where it is already, or past its range: no step, or held at the end.
    assert!(!crate::a11y::edge(&mut s, id, 0, Turn::To(100.0)));
    assert!(crate::a11y::edge(&mut s, id, 0, Turn::To(500.0)));
    assert_eq!(ranges(&s, t, 0), [(0, 126), (127, 127)]);
    assert!(!crate::a11y::edge(&mut s, id, 0, Turn::To(f64::NAN)));
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
    // A file that is not a sound among them is passed over and named (N-smp-5); the pad
    // chosen has a file now, which gives way to it (N3-kit-2).
    let (_dir4, mixed) = wavs(&["Tom.wav", "notes.txt"]);
    s.error = None;
    assert!(s.window_drop(&g, x, y, &mixed), "{:?}", s.error);
    let k = plugin(&s, t);
    assert_eq!(sampler::pad_files(k.builtin().unwrap(), &k, 30), [3]);
    assert!(k.samples[3].ends_with("Tom.wav"), "{:?}", k.samples);
    assert!(
        s.error.as_deref().is_some_and(|e| e.contains("notes.txt")),
        "{:?}",
        s.error
    );
    // A1 has a file: a drop there takes its place (the mock's), and A1 is chosen.
    s.instruments.pick(id, 5, 0);
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    let (_dir3, last) = wavs(&["Rim.wav"]);
    assert!(s.window_drop(&g, x, y, &last), "{:?}", s.error);
    let k = plugin(&s, t);
    let a1 = sampler::pad_files(k.builtin().unwrap(), &k, 0);
    assert_eq!(a1.len(), 1);
    assert!(k.samples[a1[0]].ends_with("Rim.wav"), "{:?}", k.samples);
    assert!(!k.samples.iter().any(|f| f == "Kick.wav"));
    assert_eq!(s.instruments.open[0].pick, 0);
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

/// Low 18, 19 (UI-334): a pad of more than 128 layers draws (the last ones without a width),
/// and the first edge pulled under the lowest `top` goes nowhere and leaves no step to undo.
#[test]
fn ui_334_a_pad_of_130_layers_draws_and_the_first_edge_stops_at_the_least_top() {
    let (mut s, g, t, id) = window("Drum Kit");
    let files: Vec<_> = (0..130)
        .map(|n| json!({ "file": format!("Snare_{n}.wav") }))
        .collect();
    assert!(
        s.run_command(
            "plugin.samples.add",
            json!({ "id": id.to_string(), "files": files, "pad": "A1", "layers": true }),
        ),
        "{:?}",
        s.error
    );
    assert_eq!(ranges(&s, t, 0).len(), 130);
    let b = body(&s, &g, t);
    assert!(b.spots.iter().any(|x| x.mark == Mark::Layer(0)));
    // Two layers: the first edge as low as it goes, then lower.
    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Snare_a.wav"]);
    let (_dir, more) = wavs(&["Snare_b.wav"]);
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    s.instruments.drop_layers = true;
    assert!(s.window_drop(&g, x, y, &more), "{:?}", s.error);
    let strip: f32 = body(&s, &g, t)
        .spots
        .iter()
        .filter(|x| matches!(x.mark, Mark::Layer(_)))
        .map(|x| x.r.w + 2.0)
        .sum();
    let n = spot(&s, &g, t, Mark::Edge(0));
    let (x, y) = s.instrument_center(&g, id, Part::Spot(n)).unwrap();
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, x - strip, y, false));
    assert!(s.instrument_up());
    assert_eq!(ranges(&s, t, 0)[0], (0, 1));
    let steps = s.song().steps();
    let n = spot(&s, &g, t, Mark::Edge(0));
    let (x, y) = s.instrument_center(&g, id, Part::Spot(n)).unwrap();
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    assert!(s.instrument_moved(&g, x - strip, y, false));
    s.instrument_up();
    assert!(s.field_keys(&g, "left", None, Mods::NONE));
    assert!(!s.layer_edge_set(id, 0, 0, 0));
    assert_eq!(ranges(&s, t, 0)[0], (0, 1));
    assert_eq!(s.song().steps(), steps);
}

/// Low 1 (UI-333): reversed, the line stands where the wave draws what the voice reads,
/// wherever Start and End are, and goes from Start to End; out of the cut the file draws as
/// it is.
#[test]
fn ui_333_reversed_the_play_line_stands_over_what_it_reads() {
    let cols = 400;
    for cut in [
        (0.1, 0.4, true),
        (0.25, 0.9, true),
        (0.0, 1.0, true),
        (0.3, 0.6, false),
    ] {
        let mut last = None;
        for k in [0.9, 0.5, 0.1] {
            let p = cut.0 + (cut.1 - cut.0) * k;
            let x = sampler::wave_at(p, cut);
            let m = sampler::wave_column((x * cols as f64) as usize, cols, cut);
            assert!(
                (m as f64 / cols as f64 - p).abs() < 2.0 / cols as f64,
                "{cut:?} {p}"
            );
            if cut.2 {
                // End back to Start in the file is Start to End on the wave.
                assert!(last.is_none_or(|l| x > l), "{cut:?} {p}");
            }
            last = Some(x);
        }
    }
    assert_eq!(sampler::wave_column(20, cols, (0.1, 0.4, true)), 20);
    assert_eq!(sampler::wave_column(300, cols, (0.1, 0.4, true)), 300);
    assert_eq!(sampler::wave_column(0, cols, (0.0, 1.0, true)), cols - 1);
}

/// Low 23 (UI-333): the keys' names go once the window's content is 520 or narrower, as on
/// the mock, not once the keys' frame within it (16 narrower) is.
#[test]
fn ui_333_the_wave_and_the_zones_wear_the_mocks_colours() {
    for theme in [xirius_design::Theme::Dark, xirius_design::Theme::Light] {
        let c = crate::paint::Colors::new(theme);
        let on = sampler::wave_inks(&c, false);
        assert_eq!(on.ground, c.surface_2);
        assert_eq!(on.wave, c.scroll_thumb[0], "thumb-hover, not ink at .5");
        assert_eq!(on.wave.a, 1.0);
        assert_eq!(on.span, c.surface_3);
        assert_eq!(on.outside, c.surface_0.opacity(0.72));
        assert_eq!(sampler::wave_inks(&c, true).wave, c.scroll_thumb[1]);
    }
    // A zone is 2 px short of its keys (n3_smp_1 measures it), its words 4 px in from each
    // side.
    assert_eq!(sampler::ZONE_GAP, 2.0);
    let (mut s, g, t, id) = window("Sampler");
    add(&mut s, id, &["C3.wav"]);
    let a = body(&s, &g, t).spots[spot(&s, &g, t, Mark::Zone(0))].clone();
    let words = sampler::span_text(&a);
    assert_eq!((words.x - a.r.x, a.r.right() - words.right()), (4.0, 4.0));
}

#[test]
fn ui_333_the_keys_names_go_by_the_windows_width() {
    let (mut s, g, t, id) = window("Sampler");
    add(&mut s, id, &["Piano_C3.wav"]);
    for (w, named) in [(530.0, true), (521.0, true), (520.0, false)] {
        // As tall as the body at that width, so the frame does not narrow it to fit.
        let x = plugin(&s, t);
        let def = s.instruments.def_of(&x).unwrap();
        s.instruments.open[0].body = layout::size_of(def, w, Some(&x));
        let b = body(&s, &g, t);
        let keys =
            b.keys.first().unwrap().r.x..b.keys.iter().rfind(|k| !k.black).unwrap().r.right();
        assert!(keys.end - keys.start < 520.0, "{w}");
        assert_eq!(b.w, w);
        assert_eq!(play::key_names(b.w), named, "{w}");
    }
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
    // L2-smp-6: the pad says its sample and is selected; the chosen pad's title reads.
    assert_eq!(find("A1 C1, Kick").node.is_selected(), Some(true));
    assert_eq!(find("A2 C#1").node.is_selected(), None);
    assert_eq!(find("A1, Kick, C1").node.role(), accesskit::Role::Label);
    let _ = t;
}

/// L2-smp-6: a zone says its root, and a layer says it is one.
#[test]
fn l2_smp_6_zones_say_their_root_and_layers_say_layer() {
    let (mut s, g, _t, id) = window("Sampler");
    add(&mut s, id, &["Piano_C3.wav", "Piano_C4.wav"]);
    let marks = crate::a11y::marks(&s, &g);
    let labels: Vec<_> = marks.iter().filter_map(|m| m.node.label()).collect();
    assert!(labels.contains(&"Piano_C3, root C3"), "{labels:?}");
    assert!(labels.contains(&"Piano_C4, root C4"), "{labels:?}");
    let (_dir, files) = wavs(&["Snare_a.wav", "Snare_b.wav"]);
    let (mut s, g, _t, id) = window("Drum Kit");
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    s.instruments.drop_layers = true;
    assert!(s.window_drop(&g, x, y, &files), "{:?}", s.error);
    let marks = crate::a11y::marks(&s, &g);
    let labels: Vec<_> = marks.iter().filter_map(|m| m.node.label()).collect();
    assert!(labels.contains(&"Layer 0–63"), "{labels:?}");
    assert!(labels.contains(&"Layer 64–127"), "{labels:?}");
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
    // A2 has files: one more goes on it too. The window has grown to show them.
    let (x, y) = s.instrument_center(&g, id, Part::Pad(37)).expect("A2");
    assert!(s.instrument_right(&g, x, y));
    assert!(live_row(&s, "サンプルを置き換え…"));
    choose(&mut s, &g, "サンプルを足す…");
    let request = s.file_request.take().unwrap();
    s.finish_picked(request, &files[3..]);
    let k = plugin(&s, t);
    assert_eq!(sampler::pad_files(def, &k, 1), [0, 1, 2]);
    // Its sample put in another's place: the first layer, the pad's files as many.
    let (x, y) = s.instrument_center(&g, id, Part::Pad(37)).expect("A2");
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
    // Over A1, which has a file: the pads from it, A1 in place of its file (N3-kit-2).
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    assert!(s.window_drag(&g, x, y, &files, false));
    let v = s.instruments.preview.clone().expect("a plan");
    assert_eq!(v.text, "2 pads, A1–A2");
    assert_eq!(
        v.pads,
        [
            (0, "Kick".to_string(), String::new()),
            (1, "Snare_v1".to_string(), "Vel 2".to_string())
        ]
    );
    // The same again changes nothing.
    assert!(!s.window_drag(&g, x, y, &files, false));
    let mut b = body(&s, &g, t);
    crate::instrument::kit_drag::ghost(&mut b, &v);
    let a1 = b.spots.iter().find(|x| x.mark == Mark::Pad(0)).unwrap();
    assert!(a1.ghost && a1.text == "Kick");
    assert!(b.spots.iter().any(|x| x.mark == Mark::Bank(0) && x.ghost));
    assert!(b.spots.iter().any(|x| x.mark == Mark::Bank(1) && !x.ghost));
    assert!(
        !b.spots.iter().any(|x| x.mark == Mark::Mute(0)),
        "A1 shows what it gets, without its M"
    );
    // Shift: all of them on A1, with its file; the chip says the layers it has then.
    assert!(s.window_drag(&g, x, y, &files, true));
    let v = s.instruments.preview.clone().unwrap();
    assert_eq!(v.text, "A1: 4 layers");
    assert_eq!(v.pads, [(0, "Hat".to_string(), "Vel 4".to_string())]);
    let mut b = body(&s, &g, t);
    crate::instrument::kit_drag::ghost(&mut b, &v);
    assert!(
        !b.spots.iter().any(|x| x.mark == Mark::Mute(0)),
        "no M on it"
    );
    // One file: its name.
    assert!(s.window_drag(&g, x, y, &files[..1], false));
    assert_eq!(s.instruments.preview.as_ref().unwrap().text, "A1: Kick");
    // A folder: its sounds, as the drop reads them (N3-kit-1).
    assert!(s.window_drag(&g, x, y, &[dir.path().to_path_buf()], false));
    let v = s.instruments.preview.clone().unwrap();
    assert_eq!(v.text, "2 pads, A1–A2");
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
        born: 1,
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
    // The same key again, read in the same stage: another voice, its time from now (low 22).
    let again = VoiceView {
        born: 2,
        ..voice(0, VoiceStage::Attack, 0.1)
    };
    s.instruments.hear(id, Some(again), later);
    let env = s
        .instruments
        .live_of(&x, def, 0, Some(0), later)
        .unwrap()
        .env
        .unwrap();
    assert_eq!(env.secs, 0.0);
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

/// L2-smp-5 (UI-333, UI-334): the mock's drop targets. Files dropped on a Sampler with
/// samples take their place, on its zones' strip they join them; on a Drum Kit's layers'
/// bar they are the chosen pad's layers. Each place lights while files are held over it.
#[test]
fn l2_smp_5_each_drop_target_does_what_the_mock_does() {
    let (_dir, two) = wavs(&["Pad_C3.wav", "Pad_C4.wav"]);
    let (mut s, g, t, id) = window("Sampler");
    add(&mut s, id, &["Old.wav"]);
    let strip = sampler::zone_strip(&body(&s, &g, t)).expect("the zones' strip");
    let (x, y) = (strip.x + 4.0, strip.y + strip.h / 2.0);
    assert!(s.window_drag(&g, x, y, &two, false));
    assert_eq!(
        s.instruments.drop_on.map(|d| (d.1, d.2)),
        Some((sampler::DropOn::Zones, strip))
    );
    assert!(s.window_drop(&g, x, y, &two), "{:?}", s.error);
    assert_eq!(plugin(&s, t).samples.len(), 3, "they join the one there");
    // Anywhere else on the window, they take the place of all of them.
    let b = body(&s, &g, t);
    let wave = b.graphs[b.main];
    let (x, y) = (wave.x + wave.w / 2.0, wave.y + wave.h / 2.0);
    let (_dir2, one) = wavs(&["New.wav"]);
    assert!(s.window_drag(&g, x, y, &one, false));
    assert_eq!(
        s.instruments.drop_on.map(|d| d.1),
        Some(sampler::DropOn::Samples)
    );
    // The drop alone takes the light away: the screen calls nothing else after it.
    assert!(s.drop_files(&g, x, y, &one), "{:?}", s.error);
    assert_eq!(s.instruments.drop_on, None);
    assert!(s.instruments.preview.is_none());
    let p = plugin(&s, t);
    assert_eq!(p.samples.len(), 1);
    assert!(p.samples[0].ends_with("New.wav"), "{:?}", p.samples);
    // The light goes too when the drag leaves.
    assert!(s.window_drag(&g, x, y, &one, false));
    assert!(s.drop_hover(false));
    assert_eq!(s.instruments.drop_on, None);

    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Kick.wav"]);
    let bar = sampler::layer_bar(&body(&s, &g, t)).expect("A1's layers");
    let (x, y) = (bar.right() - 4.0, bar.y + bar.h / 2.0);
    let (_dir3, snare) = wavs(&["Snare.wav"]);
    assert!(s.window_drag(&g, x, y, &snare, false));
    assert_eq!(
        s.instruments.drop_on.map(|d| (d.1, d.2)),
        Some((sampler::DropOn::Layers, bar))
    );
    assert!(s.drop_files(&g, x, y, &snare), "{:?}", s.error);
    assert_eq!(s.instruments.drop_on, None, "no band is left lit");
    let k = plugin(&s, t);
    assert_eq!(sampler::pad_files(k.builtin().unwrap(), &k, 0), [0, 1]);
    assert_eq!(s.instruments.open[0].pick, 0);
}

/// N3-kit-1 (UI-334): the preview is what the drop does. A folder shows its pads, their
/// range and their banks' dots; the layers' bar adds to the pad chosen, said in its layers;
/// Shift pressed or let go without a move plans again.
#[test]
fn n3_kit_1_the_preview_of_a_folder_the_layers_bar_and_shift_is_the_drop() {
    let names: Vec<String> = (1..=21).map(|i| format!("Hit{i:02}.wav")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let (dir, _) = wavs(&refs);
    let folder = [dir.path().to_path_buf()];
    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Kick.wav"]);
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    assert!(s.window_drag(&g, x, y, &folder, false));
    let v = s.instruments.preview.clone().expect("a plan");
    assert_eq!(v.text, "21 pads, A1–B5");
    assert_eq!(v.pads.len(), 21);
    assert_eq!(v.pads[20].1, "Hit21");
    let mut b = body(&s, &g, t);
    crate::instrument::kit_drag::ghost(&mut b, &v);
    for bank in [0, 1] {
        assert!(
            b.spots
                .iter()
                .any(|x| x.mark == Mark::Bank(bank) && x.ghost)
        );
    }
    assert!(b.spots.iter().any(|x| x.mark == Mark::Bank(2) && !x.ghost));
    // Dropped, it does what it showed.
    assert!(s.drop_files(&g, x, y, &folder), "{:?}", s.error);
    let k = plugin(&s, t);
    let def = k.builtin().unwrap();
    for (pad, name, _) in &v.pads {
        let f = sampler::pad_files(def, &k, *pad);
        assert_eq!(f.len(), 1);
        assert_eq!(&sampler::stem(&k.samples[f[0]]), name);
    }
    // Over A1's layers' bar: on A1, as its layers.
    let (_dir2, two) = wavs(&["Snare.wav", "Clap.wav"]);
    s.instruments.pick(id, 0, 0);
    let bar = sampler::layer_bar(&body(&s, &g, t)).expect("A1's layers");
    let (bx, by) = (bar.right() - 4.0, bar.y + bar.h / 2.0);
    assert!(s.window_drag(&g, bx, by, &two, false));
    let v = s.instruments.preview.clone().unwrap();
    assert_eq!(v.text, "A1: 3 layers");
    assert_eq!(v.pads.len(), 1);
    assert_eq!(v.pads[0].0, 0);
    // Shift held and let go over A2 without a move: one pad, then the pads from it again.
    let (x2, y2) = s.instrument_center(&g, id, Part::Pad(37)).expect("A2");
    s.window_drag(&g, x2, y2, &two, false);
    assert_eq!(
        s.instruments.preview.as_ref().unwrap().text,
        "2 pads, A2–A3"
    );
    assert!(s.drag_shift(&g, true));
    assert_eq!(s.instruments.preview.as_ref().unwrap().text, "A2: 3 layers");
    assert!(!s.drag_shift(&g, true), "no change, no plan");
    assert!(s.drag_shift(&g, false));
    assert_eq!(
        s.instruments.preview.as_ref().unwrap().text,
        "2 pads, A2–A3"
    );
    // Once the drag leaves, Shift plans nothing.
    s.drop_hover(false);
    assert!(!s.drag_shift(&g, true));
    assert!(s.instruments.preview.is_none());
}

/// N3-kit-2 (UI-334): files dropped on a Drum Kit's pad that has a file fill the pads from it,
/// as the preview showed: the pad's file gives way to the first of them, and the pad stays
/// chosen.
#[test]
fn n3_kit_2_a_drop_on_a_filled_pad_replaces_it_as_the_preview_said() {
    let (_dir, files) = wavs(&["Kick.wav", "Snare.wav"]);
    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Hat.wav", "Ride.wav"]);
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    assert!(s.window_drag(&g, x, y, &files, false));
    let shown = s.instruments.preview.clone().expect("a plan").pads;
    assert!(s.drop_files(&g, x, y, &files), "{:?}", s.error);
    let k = plugin(&s, t);
    let def = k.builtin().unwrap();
    let got: Vec<(usize, String)> = shown
        .iter()
        .map(|(pad, _, _)| {
            let f = sampler::pad_files(def, &k, *pad);
            assert_eq!(f.len(), 1, "{pad}: {:?}", k.samples);
            (*pad, sampler::stem(&k.samples[f[0]]))
        })
        .collect();
    assert_eq!(got, [(0, "Kick".to_string()), (1, "Snare".to_string())]);
    assert_eq!(k.samples.len(), 2, "Hat and Ride gave way: {:?}", k.samples);
    assert_eq!(s.instruments.open[0].pick, 0);
}

/// N4-kit-1 (UI-334): files dropped on H14 fill H14 to H16; the chip says how many do not
/// fit before the drop, and the line at the foot says it after (not as a fault).
#[test]
fn n4_kit_1_the_files_past_h16_are_said_on_the_chip_and_after_the_drop() {
    let (dir, _) = wavs(&["Hit1.wav", "Hit2.wav", "Hit3.wav", "Hit4.wav", "Hit5.wav"]);
    let folder = [dir.path().to_path_buf()];
    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Kick.wav"]);
    s.instruments.pick(id, 125, 0);
    let pitch = ((125 + 36) % 128) as u8;
    let (x, y) = s.instrument_center(&g, id, Part::Pad(pitch)).expect("H14");
    assert!(s.window_drag(&g, x, y, &folder, false));
    let v = s.instruments.preview.clone().expect("a plan");
    assert_eq!(v.text, "3 pads, H14–H16（2 個は入らない）");
    assert!(s.drop_files(&g, x, y, &folder), "{:?}", s.error);
    assert_eq!(s.error, None);
    assert_eq!(
        s.notice_text(),
        Some("パッドに入りきらない 2 個は入れていません")
    );
    let k = plugin(&s, t);
    assert_eq!(k.samples.len(), 4, "{:?}", k.samples);
}

/// N4-kit-2 (UI-334): an OS drag's moves say no keys, and the window's own Shift is stale while
/// the other app has the keyboard: Shift is the OS's, for the plan and for the drop.
#[test]
fn n4_kit_2_shift_of_an_os_drag_is_read_from_the_os() {
    let (_dir, two) = wavs(&["Snare.wav", "Clap.wav"]);
    let (mut s, g, t, id) = window("Drum Kit");
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    crate::os_shift::hold(false);
    assert!(s.files_moved(&g, x, y, &two, false));
    assert_eq!(
        s.instruments.preview.as_ref().unwrap().text,
        "2 pads, A1–A2"
    );
    // Held in the OS, though the move says nothing: the layers of A1.
    crate::os_shift::hold(true);
    assert!(s.files_moved(&g, x, y, &two, false));
    assert_eq!(s.instruments.preview.as_ref().unwrap().text, "A1: 2 layers");
    // A stale Shift of the window does not count where the OS says.
    if cfg!(windows) {
        crate::os_shift::hold(false);
        assert!(s.files_moved(&g, x, y, &two, true));
        assert_eq!(
            s.instruments.preview.as_ref().unwrap().text,
            "2 pads, A1–A2"
        );
        crate::os_shift::hold(true);
    }
    // Dropped while held: both are A1's layers.
    assert!(s.files_landed(&g, x, y, &two, false), "{:?}", s.error);
    crate::os_shift::hold(false);
    let k = plugin(&s, t);
    assert_eq!(sampler::pad_files(k.builtin().unwrap(), &k, 0), [0, 1]);
    assert!(!s.instruments.drop_layers, "the drop takes the Shift");
}

/// Low 14 of the fourth review: what takes a drop before the window takes the plan too. A
/// `.dawproject` among the files opens as a song, and over the quantize panel the groove
/// takes them: no pads are planned there.
#[test]
fn the_plan_gives_way_where_a_dawproject_or_the_groove_takes_the_drop() {
    let (dir, two) = wavs(&["Snare.wav", "Clap.wav"]);
    let (mut s, g, _, id) = window("Drum Kit");
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    let mut with_song = two.clone();
    with_song.push(dir.path().join("Song.dawproject"));
    s.files_moved(&g, x, y, &with_song, false);
    assert!(s.instruments.preview.is_none());
    assert!(s.instruments.drop_on.is_none());
    s.files_moved(&g, x, y, &two, false);
    assert!(s.instruments.preview.is_some());
    s.open_popup(
        crate::popup::PopupKind::Quantize,
        crate::studio::Rect::new(x, y, 1.0, 1.0),
    );
    let r = s.popup.as_ref().unwrap().layout(g.size).rect;
    let frame = s.instruments.frame(0, &g);
    let (px, py) = (
        r.x.max(frame.x) + 2.0,
        r.y.max(frame.y + crate::title_bar::BAR_H) + 2.0,
    );
    assert!(
        r.contains(px, py) && frame.contains(px, py),
        "{r:?} {frame:?}"
    );
    s.files_moved(&g, px, py, &two, false);
    assert!(s.instruments.preview.is_none());
    assert!(s.instruments.drop_on.is_none());
}

/// Low 20 of the fourth review: input held in the middle of a drag (a question on the song)
/// puts out the plan and the light at the next move, not only when the drag leaves.
#[test]
fn a_drag_held_by_a_question_puts_its_plan_out() {
    let (_dir, two) = wavs(&["Snare.wav", "Clap.wav"]);
    let (mut s, g, _, id) = window("Drum Kit");
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    assert!(s.files_moved(&g, x, y, &two, false));
    assert!(s.instruments.preview.is_some());
    s.ask_unsaved(crate::unsaved::Pending::Close);
    assert!(s.input_blocked());
    assert!(s.files_moved(&g, x, y, &two, false));
    assert!(s.instruments.preview.is_none());
    assert!(s.instruments.drop_on.is_none());
    assert!(
        !s.files_moved(&g, x, y, &two, false),
        "nothing more to put out"
    );
}

/// L2-smp-5 (UI-334): an empty pad of a filled kit is its name and the drop plane, as on
/// the mock: no Choke, modes, layers or voice to write its values.
#[test]
fn l2_smp_5_an_empty_pad_of_a_filled_kit_is_its_name_and_the_plane() {
    let (mut s, g, t, id) = window("Drum Kit");
    add(&mut s, id, &["Kick.wav"]);
    assert!(!body(&s, &g, t).slots.is_empty());
    s.instruments.pick(id, 5, 0);
    let b = body(&s, &g, t);
    assert!(b.slots.is_empty(), "{:?}", b.slots.len());
    assert!(b.spots.iter().any(|x| x.mark == Mark::Drop));
    assert!(b.spots.iter().any(|x| x.mark == Mark::Title));
    assert!(!b.spots.iter().any(|x| matches!(x.mark, Mark::Layer(_))));
}

/// L2-smp-5 (UI-334): a pad pressed in the window chooses the layer that sounded, as on
/// the mock; a voice heard after the wait does not move it.
#[test]
fn l2_smp_5_a_pressed_pad_chooses_the_layer_that_sounded() {
    use xirius_engine::stock::{VoiceStage, VoiceView};
    let voice = |source: u16| VoiceView {
        pitch: 36,
        stage: VoiceStage::Attack,
        file: true,
        source,
        position: 0.0,
        env: 0.5,
        born: 1,
    };
    let (_dir, files) = wavs(&["Snare_a.wav", "Snare_b.wav"]);
    let (mut s, g, t, id) = window("Drum Kit");
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).expect("A1");
    s.instruments.drop_layers = true;
    assert!(s.window_drop(&g, x, y, &files), "{:?}", s.error);
    assert_eq!(s.instruments.open[0].layer, 0);
    press(&mut s, &g, id, Part::Pad(36));
    let now = std::time::Instant::now();
    s.instruments.hear(id, Some(voice(1)), now);
    let k = plugin(&s, t);
    let def = s.instruments.def_of(&k).unwrap();
    assert!(s.instruments.follow(&k, def, now));
    assert_eq!(s.instruments.open[0].layer, 1);
    // Once is all: the next voice of the press does not move it again.
    s.instruments.hear(id, Some(voice(0)), now);
    assert!(!s.instruments.follow(&k, def, now));
    assert_eq!(s.instruments.open[0].layer, 1);
    // Heard after the wait, it does not.
    press(&mut s, &g, id, Part::Pad(36));
    let pressed = s.instruments.open[0].layer;
    s.instruments.hear(id, Some(voice(1)), now);
    let late = std::time::Instant::now() + std::time::Duration::from_secs(1);
    assert!(!s.instruments.follow(&k, def, late));
    assert_eq!(s.instruments.open[0].layer, pressed);
}

/// L2-smp-8: undoing the files' coming or going leaves the window on a zone the song has,
/// and the wave's Start dragged is one step.
#[test]
fn l2_smp_8_undo_keeps_the_zone_chosen_and_a_wave_drag_is_one_step() {
    let (mut s, g, t, id) = window("Sampler");
    add(&mut s, id, &["Piano_C3.wav"]);
    add(&mut s, id, &["Piano_C4.wav", "Piano_C5.wav"]);
    // The wave's Start: a drag of many moves is one step.
    let (x, y) = s
        .instrument_center(&g, id, Part::Point(0, 0))
        .expect("the wave's Start");
    let steps = s.song().steps();
    assert!(s.instrument_press(&g, x, y, 1, Mods::NONE));
    for d in 1..=20 {
        s.instrument_moved(&g, x + d as f32 * 2.0, y, false);
    }
    assert!(s.instrument_up());
    assert_eq!(s.song().steps(), steps + 1);
    let key = format!("file{}.start", s.instruments.open[0].pick + 1);
    assert!(plain(&s, t, &key) > 0.0, "{key}");
    s.undo(&g);
    assert_eq!(plain(&s, t, &key), 0.0, "{key}");
    press_spot(&mut s, &g, t, id, Mark::Zone(2));
    assert_eq!(s.instruments.open[0].pick, 2);
    s.undo(&g);
    assert_eq!(plugin(&s, t).samples.len(), 1);
    assert_eq!(s.instruments.open[0].pick, 0);
    assert!(!body(&s, &g, t).slots.is_empty());
    s.redo(&g);
    press_spot(&mut s, &g, t, id, Mark::Zone(2));
    let remove = json!({ "id": id.to_string(), "index": 2 });
    assert!(
        s.run_command("plugin.samples.remove", remove),
        "{:?}",
        s.error
    );
    assert_eq!(s.instruments.open[0].pick, 1);
    s.undo(&g);
    assert_eq!(plugin(&s, t).samples.len(), 3);
    assert!(s.instruments.open[0].pick < 3);
}

/// Low 12: a window another device takes in ends its sweep first, on the zone it turned
/// (not on the first zone, as the window gone would leave it).
#[test]
fn ui_333_a_window_another_device_takes_ends_its_sweep_on_the_zone_turned() {
    let (mut s, g, t, id) = window("Sampler");
    add(&mut s, id, &["Pad_C3.wav", "Pad_C4.wav"]);
    s.instruments.pick(id, 1, 0);
    let def = s.instruments.def_of(&plugin(&s, t)).unwrap();
    let k = (0..def.params.len())
        .find(|&k| {
            def.params[k].id.starts_with("file2.")
                && s.instrument_center(&g, id, Part::Knob(k)).is_some()
        })
        .expect("a knob of the second zone");
    let zone = |s: &Studio, n: usize| {
        let x = plugin(s, t);
        values_of(&x, sampler::scoped(&x, n).unwrap())
    };
    let (first, second) = (zone(&s, 0), zone(&s, 1));
    let (x, y) = s.instrument_center(&g, id, Part::Knob(k)).unwrap();
    assert!(s.device_wheel(&g, x, y, 4.0, false));
    assert!(s.instruments.sweep.is_some());
    let vocal = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .unwrap()
        .id;
    assert!(s.run_command("plugin.add", json!({"track": vocal, "name": "EQ"})));
    let eq = s.project().tracks[&vocal].inserts.last().unwrap().id;
    s.show_instrument(vocal, eq);
    assert!(!s.instruments.is_open(id), "the EQ took the window in");
    assert!(s.instruments.sweep.is_none());
    assert_eq!(zone(&s, 0), first, "the first zone stays");
    assert_ne!(zone(&s, 1)[k], second[k], "the turn went to the second");
}
