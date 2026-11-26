//! UI-327: the Compressor's window, without a window.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::accesskit;
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;
use xirius_engine::stock::{Probe, reduction};

use super::compressor::{self, SPAN};
use super::dynamics::{self, Dynamics, STEP};
use super::layout;
use crate::instrument::window::{Part, values};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// The sample song with a Compressor on Keys, its window open.
fn cmp_window() -> (Studio, Geometry, TrackId, PluginId) {
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
        json!({ "track": t.to_string(), "name": "Compressor" }),
    ));
    let id = fx(&s, t).id;
    s.show_instrument(t, id);
    s.instruments.detached_target = Some(id);
    assert!(s.instruments.is_open(id));
    (s, g, t, id)
}

fn fx(s: &Studio, t: TrackId) -> Plugin {
    s.project().tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == "Compressor")
        .cloned()
        .unwrap()
}

fn plain(s: &Studio, t: TrackId, id: &str) -> f64 {
    let x = fx(s, t);
    let (k, spec) = x.builtin().unwrap().param(id).unwrap();
    spec.to_plain(values(&x)[k])
}

/// Makes the window `w` wide.
fn resize(s: &mut Studio, t: TrackId, w: f32) {
    let def = fx(s, t).builtin().unwrap();
    s.instruments.open[0].body = layout::size(def, w);
}

/// Drags the `at`th point of the curve by `(dx, dy)` and lets go.
fn drag(s: &mut Studio, g: &Geometry, id: PluginId, at: usize, (dx, dy): (f32, f32)) {
    let (x, y) = s
        .instrument_center(g, id, Part::Point(0, at))
        .expect("the point");
    assert!(s.instrument_press(g, x, y, 1, Mods::NONE));
    for i in 1..=10 {
        let f = i as f32 / 10.0;
        s.instrument_moved(g, x + dx * f, y + dy * f, false);
    }
    assert!(s.instrument_up());
}

#[test]
fn ui_327_the_curve_is_the_engines_without_the_makeup() {
    let knobs = (-20.0, 4.0, 0.0);
    // Below the threshold it passes; above, a quarter of what is over.
    assert_eq!(compressor::out_db(-30.0, knobs), -30.0);
    assert!((compressor::out_db(-10.0, knobs) - -17.5).abs() < 1e-9);
    let r = crate::studio::Rect::new(0.0, 0.0, 200.0, 200.0);
    let (ax, ay) = (compressor::in_axis(r), compressor::out_axis(r));
    for (x, y) in compressor::curve(r, (-24.0, 3.0, 6.0)) {
        let v = ax.value(x);
        let want = v - reduction(v, -24.0, 3.0, 6.0);
        assert!((y - ay.px(want)).abs() < 0.01, "at {v} dB");
    }
}

#[test]
fn ui_327_it_opens_wide_with_the_history_beside_the_curve_and_narrow_under_it() {
    let (mut s, g, t, _) = cmp_window();
    assert_eq!(s.instruments.open[0].body.0, compressor::NATURAL_W);
    let def = fx(&s, t).builtin().unwrap();
    let r = s.instruments.frame(0, &g);
    let b = s.instruments.body(r, &fx(&s, t), def);
    let [xfer, hist] = b.graphs[..] else {
        panic!("two planes")
    };
    assert_eq!(xfer.w, xfer.h, "the curve's plane is square");
    assert!(hist.x > xfer.right() && hist.y == xfer.y);
    assert_eq!(b.levels.len(), 3);
    assert!(b.levels[0].name.x > hist.right());
    let knobs: Vec<_> = compressor::KNOBS
        .iter()
        .map(|id| b.slot(def.param(id).unwrap().0).unwrap().cell)
        .collect();
    assert!(knobs.iter().all(|c| c.y == knobs[0].y), "one row");
    for id in ["filter", "listen", "low_cut", "high_cut"] {
        assert!(b.slot(def.param(id).unwrap().0).is_some(), "{id}");
    }

    resize(&mut s, t, compressor::MIN_W + 40.0);
    let r = s.instruments.frame(0, &g);
    let b = s.instruments.body(r, &fx(&s, t), def);
    let [xfer, hist] = b.graphs[..] else {
        panic!("two planes")
    };
    assert!(hist.y > xfer.bottom(), "the history goes under");
    assert!(b.levels[0].name.x > xfer.right());
    let rows: std::collections::BTreeSet<i32> = compressor::KNOBS
        .iter()
        .map(|id| b.slot(def.param(id).unwrap().0).unwrap().cell.y as i32)
        .collect();
    assert_eq!(rows.len(), 2, "two rows of four");
}

#[test]
fn ui_327_the_threshold_and_ratio_points_are_each_one_step() {
    let (mut s, g, t, id) = cmp_window();
    let (t0, r0) = (plain(&s, t, "threshold"), plain(&s, t, "ratio"));
    let before = s.song().steps();
    drag(&mut s, &g, id, 1, (-30.0, 0.0));
    assert_eq!(s.song().steps(), before + 1);
    assert!(plain(&s, t, "threshold") < t0);
    assert_eq!(plain(&s, t, "ratio"), r0);
    // A drag down makes the ratio stronger.
    drag(&mut s, &g, id, 0, (0.0, 30.0));
    assert_eq!(s.song().steps(), before + 2);
    assert!(plain(&s, t, "ratio") > r0);
    assert!(s.undo(&g));
    assert_eq!(plain(&s, t, "ratio"), r0);
    assert!(s.undo(&g));
    assert_eq!(plain(&s, t, "threshold"), t0);
}

#[test]
fn ui_327_the_history_keeps_four_seconds_and_the_readouts_their_most() {
    let mut d = Dynamics::default();
    let t0 = Instant::now();
    let loud = ([0.5, 0.25], [0.9, 0.8]);
    for i in 0..400u32 {
        d.feed(loud, 6.0, t0 + STEP * i, SPAN);
    }
    let cap = (SPAN.as_millis() / STEP.as_millis()) as usize;
    assert_eq!(d.history.len(), cap);
    let last = *d.history.back().unwrap();
    assert!((last.input - dynamics::db(0.5)).abs() < 1e-4);
    assert_eq!(last.gr, 6.0);
    assert_eq!(d.readout(0), "-6.0");
    assert_eq!(d.readout(1), "-6.0");
    assert_eq!(d.readout(2), "-0.9");
    assert!(!d.over());
    // A long gap while the meters still moved is a stall: what came in fills it, no
    // further than its span (low 14).
    let t1 = t0 + Duration::from_secs(30);
    d.feed(loud, 6.0, t1, SPAN);
    assert_eq!(d.history.len(), cap);
    assert!(d.history.iter().all(|s| s.gr == 6.0));
    // Quiet until nothing moves, then a long gap: the frames paused, the gap is quiet and
    // only the newest step is loud.
    let mut at = t1;
    for _ in 0..1000 {
        if !d.moving() {
            break;
        }
        at += STEP;
        d.feed(([0.0; 2], [0.0; 2]), 0.0, at, SPAN);
    }
    assert!(!d.moving());
    d.feed(
        ([1.2, 0.0], [1.5, 0.0]),
        0.0,
        t0 + Duration::from_secs(60),
        SPAN,
    );
    assert_eq!(d.history.len(), cap);
    assert!(d.over());
    let loud_ones = d.history.iter().filter(|s| s.output > -59.0).count();
    assert_eq!(loud_ones, 1);
    assert!(d.history.back().unwrap().output > 0.0);
    // A slow frame of a few steps fills them with what it gathered.
    let t1 = t0 + Duration::from_secs(60);
    d.feed(loud, 6.0, t1 + STEP * 3, SPAN);
    assert!(d.history.iter().rev().take(3).all(|s| s.gr == 6.0));
    d.clear(2);
    assert_eq!(d.readout(2), "-\u{221e}");
    assert!(!d.over());
}

/// The sample song with the built-in effect `name` on Keys, its window open.
fn effect_window(name: &str) -> (Studio, Geometry, PluginId) {
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
        json!({ "track": t.to_string(), "name": name }),
    ));
    let id = s.project().tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == name)
        .unwrap()
        .id;
    s.show_instrument(t, id);
    s.instruments.detached_target = Some(id);
    assert!(s.instruments.is_open(id), "{name}");
    (s, g, id)
}

/// The value the screen reader hears from the mark `part` of the device `id`.
fn mark(s: &Studio, g: &Geometry, id: PluginId, part: &str) -> accesskit::Node {
    crate::a11y::device_marks(s, g)
        .into_iter()
        .find(|m| m.key == format!("device-{id}-{part}"))
        .unwrap_or_else(|| panic!("no mark for {part}"))
        .node
}

/// M-ui-5: on the Compressor and the Limiter, the input's and the output's readouts are the
/// loudest sample (1.2 is +1.6 dB), say + over full scale and turn red, as every meter's do;
/// the reduction's never. Under full scale there is no + and no red. Each readout is a button
/// whose press clears its most and leaves the others theirs.
#[test]
fn ui_327_328_the_readouts_read_plus_and_red_over_full_scale_and_a_press_clears_one() {
    let rows = [
        ("Compressor", compressor::SPAN),
        ("Limiter", super::limiter::SPAN),
    ];
    let mut failures = Vec::new();
    for (name, span) in rows {
        let (mut s, g, id) = effect_window(name);
        let mut bad = Vec::new();
        let d = s.instruments.dynamics.entry(id).or_default();
        d.feed(([1.2, 0.3], [1.1, 0.9]), 3.0, Instant::now(), span);
        let said: Vec<_> = (0..3)
            .map(|k| mark(&s, &g, id, &format!("readout-{k}")))
            .map(|n| {
                (
                    n.role(),
                    n.label().map(str::to_owned),
                    n.value().map(str::to_owned),
                )
            })
            .collect();
        let want: Vec<_> = [
            ("Input peak", "+1.6"),
            ("Reduction peak", "-3.0"),
            ("Output peak", "+0.8"),
        ]
        .iter()
        .map(|(l, v)| {
            (
                accesskit::Role::Button,
                Some(l.to_string()),
                Some(v.to_string()),
            )
        })
        .collect();
        if said != want {
            bad.push(format!("readouts {said:?}"));
        }
        let x = s.project().tracks[&s.instruments.open[0].track]
            .inserts
            .iter()
            .find(|x| x.id == id)
            .cloned()
            .unwrap();
        let b = s
            .instruments
            .body(s.instruments.frame(0, &g), &x, x.builtin().unwrap());
        let d = &s.instruments.dynamics[&id];
        let red: Vec<_> = b.levels.iter().map(|l| l.alert(d)).collect();
        if red != [true, false, true] {
            bad.push(format!("red {red:?}"));
        }
        let mut under = Dynamics::default();
        under.feed(([0.9, 0.3], [0.8, 0.5]), 0.0, Instant::now(), span);
        if b.levels
            .iter()
            .any(|l| l.text(&under).starts_with('+') || l.alert(&under))
        {
            bad.push("under full scale, a + or red".into());
        }
        for (k, cleared) in [(0, "-\u{221e}"), (1, "0.0")] {
            let (x, y) = s.instrument_center(&g, id, Part::Readout(k)).unwrap();
            if !s.instrument_press(&g, x, y, 1, Mods::NONE) {
                bad.push(format!("readout {k} took no press"));
            }
            s.instrument_up();
            let v = mark(&s, &g, id, &format!("readout-{k}"))
                .value()
                .map(str::to_owned);
            if v.as_deref() != Some(cleared) {
                bad.push(format!("readout {k} pressed reads {v:?}"));
            }
        }
        let kept = mark(&s, &g, id, "readout-2").value().map(str::to_owned);
        if kept.as_deref() != Some("+0.8") {
            bad.push(format!("the output kept {kept:?}"));
        }
        if !bad.is_empty() {
            failures.push(format!("{name}: {bad:?}"));
        }
    }
    assert!(failures.is_empty(), "readouts: {failures:#?}");
}

/// Each built-in effect window that draws what it hears watches its probe while it is open
/// (the Compressor's and the Limiter's meters, the Delay's and the Reverb's onsets), and lets
/// it go once closed.
#[test]
fn every_effect_window_watches_its_probe_while_it_is_open() {
    type Kept = fn(&Studio, PluginId) -> bool;
    let rows: [(&str, Kept); 4] = [
        ("Compressor", |s, id| {
            s.instruments.dynamics.contains_key(&id)
        }),
        ("Limiter", |s, id| s.instruments.dynamics.contains_key(&id)),
        ("Delay", |s, id| s.instruments.echoes.contains_key(&id)),
        ("Reverb", |s, id| s.instruments.echoes.contains_key(&id)),
    ];
    let mut failures = Vec::new();
    for (name, kept) in rows {
        let (mut s, g, id) = effect_window(name);
        let mut bad = Vec::new();
        let probe = Arc::new(Probe::new());
        let find = |x: PluginId| (x == id).then(|| probe.clone());
        let p = s.project();
        s.instruments
            .poll_with(&p, 48_000.0, Instant::now(), true, find);
        if !probe.watching() {
            bad.push("not watched while open");
        }
        if !kept(&s, id) {
            bad.push("nothing kept of what it heard");
        }
        let (x, y) = s.instrument_center(&g, id, Part::Close).unwrap();
        if !s.instrument_press(&g, x, y, 1, Mods::NONE) {
            bad.push("the close took no press");
        }
        s.instrument_up();
        let p = s.project();
        s.instruments
            .poll_with(&p, 48_000.0, Instant::now(), true, find);
        if probe.watching() {
            bad.push("still watched once closed");
        }
        if !bad.is_empty() {
            failures.push(format!("{name}: {bad:?}"));
        }
    }
    assert!(failures.is_empty(), "probes: {failures:#?}");
}

#[test]
fn ui_327_the_gain_reads_by_what_it_drives_and_the_readouts_are_buttons() {
    let (mut s, g, t, id) = cmp_window();
    let of = |s: &Studio, p: &str| {
        crate::a11y::device_marks(s, &g)
            .into_iter()
            .find(|m| m.key == format!("device-{id}-{p}"))
            .unwrap_or_else(|| panic!("no mark for {p}"))
            .node
    };
    assert_eq!(of(&s, "gain").label(), Some("Input Gain"));
    assert_eq!(of(&s, "threshold").role(), accesskit::Role::Slider);
    let peak = of(&s, "readout-0");
    assert_eq!(peak.role(), accesskit::Role::Button);
    assert_eq!(peak.label(), Some("Input peak"));
    assert_eq!(peak.value(), Some("-\u{221e}"));
    assert_eq!(of(&s, "readout-1").label(), Some("Reduction peak"));
    assert_eq!(of(&s, "readout-2").label(), Some("Output peak"));
    let drums = s
        .project()
        .tracks
        .values()
        .find(|x| x.name != "Keys" && x.kind == xirius_core::model::TrackKind::Audio)
        .map(|x| x.id)
        .unwrap();
    assert!(
        s.run_command(
            "plugin.sidechain.add",
            json!({ "id": id.to_string(), "from": drums.to_string() }),
        ),
        "{:?}",
        s.error
    );
    if !fx(&s, t).sidechain.is_some_and(|x| x.on) {
        assert!(s.run_command(
            "plugin.sidechain.enable",
            json!({ "id": id.to_string(), "state": "on" }),
        ));
    }
    assert_eq!(of(&s, "gain").label(), Some("Side Gain"));
}
