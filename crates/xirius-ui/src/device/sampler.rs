//! The Sampler's window (UI-333) and the Drum Kit's (UI-334), as the approved mocks draw
//! them. The Sampler: the zone's sample (its name, ‹ › to the files beside it, Loop,
//! Reverse, Normalize), its wave with Start and End and the loop on it, the voice (the
//! envelope's and the filter's planes with their knobs, the Amp), then the zones over the
//! keys. The Drum Kit: the bank tabs over the 16 pads on the left; on the right the chosen
//! pad (its Choke, Layer Mode, Trigger, its layers, the chosen layer's sample and wave), then
//! the pad's voice across. With no samples each is the plane a file is dropped on.
//!
//! The controls are the zone's or the pad's (`BuiltinDef::view`: the device's values, then
//! the file's or the pad's under the keys the song keeps them by), so a knob here sets
//! `file2.start` or `A1.cutoff` the way any other device's knob sets its parameter.

use std::path::Path;

use xirius_core::model::Plugin;
use xirius_design::{FontSize, Radius, Weight};
use xirius_engine::stock::files::root_of;
use xirius_params::{BuiltinDef, PADS, Scope, bare_id, pad_key};

use super::control::{self, DIAL_SM, Look, NAME_H, SEG_H, TOGGLE_H, VALUE_H};
use super::graph::{self, Axis, Drive, Point, View};
use super::layout::{Body, MS_FOOT, Mark, Name, PAD, Shape, Slot, Spot};
use super::play::{self, HIGH, KEYS_H, LOW};
use crate::draw::{Align, Pen};
use crate::studio::Rect;

/// What the empty plane and its button say (the mock's words).
pub const DROP_LABEL: &str = "Drop samples here";
pub const BROWSE_LABEL: &str = "Browse";

/// The width the Sampler's window opens at, and the narrowest it goes.
pub const NATURAL_W: f32 = 780.0;
/// The Drum Kit's.
pub const KIT_W: f32 = 905.0;
pub const KIT_MIN_W: f32 = KIT_W;
const GAP: f32 = 8.0;
/// A row of the sample: its name and its buttons.
const ROW_H: f32 = 26.0;
/// The ‹ › beside a sample's name.
const ARROW_W: f32 = 24.0;
/// The longest a sample's name gets in its row.
const NAME_MAX: f32 = 260.0;
const WAVE_H: f32 = 96.0;
/// The voice: the names over its planes, the planes, the Amp's width.
const HEAD_H: f32 = 24.0;
const PLANE_H: f32 = 112.0;
const AMP_W: f32 = 186.0;
const AMP_CELL: f32 = 62.0;
/// The share of the envelope's plane of what the planes have.
const ENV_SHARE: f32 = 0.585;
/// The zones' strip over the keys.
const ZONE_H: f32 = 20.0;
/// The empty plane a sample is dropped on, and its button.
const EMPTY_H: f32 = 196.0;
const BROWSE: (f32, f32) = (96.0, 26.0);
/// The Drum Kit's left column: the bank tabs, the pads.
const LEFT_W: f32 = 305.0;
const BANK_H: f32 = 22.0;
const PAD_H: f32 = 63.0;
/// A pad's M and S.
const MS: (f32, f32) = (16.0, 14.0);
const CHOKE_W: f32 = 96.0;
const LAYER_H: f32 = 22.0;
/// How far in from its plane a graph draws.
const INSET: f32 = 12.0;
/// The voice's knobs.
const ENV: [(&str, &str); 4] = [
    ("attack", "A"),
    ("decay", "D"),
    ("sustain", "S"),
    ("release", "R"),
];
const FILTER: [&str; 3] = ["cutoff", "resonance", "drive"];
const AMP: [&str; 5] = ["transpose", "fine", "volume", "pan", "velocity"];
/// The sample's buttons, from the left.
const SAMPLE_BUTTONS: [&str; 3] = ["loop", "reverse", "normalize"];
const BANKS: [&str; 8] = ["A", "B", "C", "D", "E", "F", "G", "H"];
/// What a pad says of its layers, by its Layer Mode.
const LAYER_TAGS: [&str; 4] = ["Vel", "RR", "Rand", "Stack"];

/// The definition the window of `plugin` shows: the zone's (`pick`, a file) of a Sampler,
/// the pad's of a Drum Kit, the device's own while it has no samples.
pub fn scoped(plugin: &Plugin, pick: usize) -> Option<&'static BuiltinDef> {
    let def = plugin.builtin()?;
    if plugin.samples.is_empty() {
        return Some(def);
    }
    Some(match def.uid {
        "sampler" => def.view(Scope::File(pick.min(plugin.samples.len() - 1))),
        "drum-kit" => def.view(Scope::Pad(pick.min(PADS - 1))),
        _ => def,
    })
}

/// Is this a device whose window follows its files?
pub fn has_files(def: &BuiltinDef) -> bool {
    matches!(def.uid, "sampler" | "drum-kit")
}

/// The index of the parameter `id` (bare: `start`, `cutoff`): the file's or the pad's
/// before the device's (the pad's volume, not the kit's).
pub fn idx(def: &BuiltinDef, id: &str) -> Option<usize> {
    def.params.iter().rposition(|p| bare_id(p.id) == id)
}

/// The plain value of the parameter `id`.
fn plain(def: &BuiltinDef, values: &[f64], id: &str) -> f64 {
    idx(def, id).map_or(0.0, |k| {
        let spec = &def.params[k];
        spec.to_plain(values.get(k).copied().unwrap_or(spec.default_norm()))
    })
}

fn on(def: &BuiltinDef, values: &[f64], id: &str) -> bool {
    plain(def, values, id) >= 0.5
}

/// A file's name without its folder and its extension.
pub fn stem(file: &str) -> String {
    let p = Path::new(file);
    p.file_stem()
        .or(p.file_name())
        .map_or_else(|| file.to_string(), |s| s.to_string_lossy().into_owned())
}

fn leaf(file: &str) -> String {
    Path::new(file)
        .file_name()
        .map_or_else(|| file.to_string(), |s| s.to_string_lossy().into_owned())
}

/// The files (by index) a Drum Kit's pad plays, in their order: its layers.
pub fn pad_files(def: &BuiltinDef, plugin: &Plugin, pad: usize) -> Vec<usize> {
    (0..plugin.samples.len())
        .filter(|&i| def.file_pad(&plugin.params, i) == pad)
        .collect()
}

/// A Sampler's zone: its file, its root and the notes it plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zone {
    pub file: usize,
    pub root: u8,
    pub lo: u8,
    pub hi: u8,
}

/// The Sampler's zones as the engine plays them: by root, each up to halfway to the next.
pub fn zones(files: &[String]) -> Vec<Zone> {
    let mut order: Vec<(u8, usize)> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (root_of(&leaf(f)), i))
        .collect();
    order.sort();
    (0..order.len())
        .map(|k| {
            let (root, file) = order[k];
            let lo = match k {
                0 => 0,
                _ => ((u16::from(order[k - 1].0) + u16::from(root)) / 2 + 1).min(127) as u8,
            };
            let hi = order.get(k + 1).map_or(127, |&(next, _)| {
                ((u16::from(root) + u16::from(next)) / 2) as u8
            });
            Zone {
                file,
                root,
                lo: lo.min(hi),
                hi,
            }
        })
        .collect()
}

/// The velocities each layer of a pad plays in Velocity mode, as the engine splits them
/// (`xirius_engine::stock::files`): from 1 past the layer below's top up to its own
/// `file<n>.top` while the tops rise, else in even shares; the last one goes to 127.
pub fn velocities(def: &BuiltinDef, plugin: &Plugin, files: &[usize]) -> Vec<(u8, u8)> {
    let top_spec = def.per_file.iter().find(|p| p.id == "top");
    let tops: Vec<f64> = files
        .iter()
        .map(|&i| {
            top_spec.map_or(127.0, |s| {
                plugin
                    .params
                    .get(&xirius_params::file_key(i, "top"))
                    .map_or_else(|| s.to_plain(s.default_norm()), |&v| s.to_plain(v))
            })
        })
        .collect();
    let rising = tops.windows(2).all(|w| w[0] < w[1]);
    let count = files.len();
    let mut lo = 0u8;
    (0..count)
        .map(|j| {
            let hi = if j + 1 == count {
                127
            } else if rising {
                tops[j].round().clamp(f64::from(lo), 126.0) as u8
            } else {
                (((j + 1) * 128 + count / 2) / count)
                    .saturating_sub(1)
                    .min(126) as u8
            };
            let out = (lo, hi);
            lo = hi.saturating_add(1).min(127);
            out
        })
        .collect()
}

fn slot(def: &BuiltinDef, id: &str, look: Look, dial: f32, cell: Rect) -> Option<Slot> {
    idx(def, id).map(|k| Slot {
        k,
        look,
        dial,
        cell,
        label: None,
        dim: false,
    })
}

fn name(r: Rect, text: &'static str) -> Name {
    Name {
        r,
        text,
        size: FontSize::Caption,
        strong: false,
    }
}

fn cell_h(dial: f32) -> f32 {
    dial + 2.0 + VALUE_H + NAME_H
}

/// The plane a sample is dropped on, `r`, with its Browse button, and the wave's place
/// (no width) on it, so the plane takes what a taller window adds.
fn drop_plane(b: &mut Body, r: Rect) {
    b.spots.push(Spot::new(r, Mark::Drop, DROP_LABEL));
    let (bw, bh) = BROWSE;
    let at = Rect::new(r.x + (r.w - bw) / 2.0, r.y + r.h / 2.0 + 14.0, bw, bh);
    b.spots.push(Spot::new(at, Mark::Browse, BROWSE_LABEL));
    b.graphs.insert(0, Rect::new(r.x, r.y, 0.0, r.h));
    b.main = 0;
}

/// The sample's row at `(x, y)`, `w` wide: ‹ its name › at the left, its buttons at the right.
fn sample_row(def: &BuiltinDef, b: &mut Body, x: f32, y: f32, w: f32, file: &str) {
    let text = stem(file);
    let name_w = (control::label_w(&text) + 32.0).clamp(80.0, NAME_MAX);
    let ay = y + (ROW_H - ARROW_W) / 2.0;
    b.spots.push(Spot::new(
        Rect::new(x, ay, ARROW_W, ARROW_W),
        Mark::Prev,
        "",
    ));
    b.spots.push(Spot::new(
        Rect::new(x + ARROW_W + 2.0, y, name_w, ROW_H),
        Mark::Name,
        text,
    ));
    b.spots.push(Spot::new(
        Rect::new(x + ARROW_W + 4.0 + name_w, ay, ARROW_W, ARROW_W),
        Mark::Next,
        "",
    ));
    let mut right = x + w;
    for id in SAMPLE_BUTTONS.iter().rev() {
        let Some(k) = idx(def, id) else { continue };
        let tw = control::toggle_w(control::name(def, &def.params[k]));
        right -= tw;
        b.slots.push(Slot {
            k,
            look: Look::Toggle,
            dial: TOGGLE_H,
            cell: Rect::new(right, y, tw, TOGGLE_H),
            label: None,
            dim: false,
        });
        right -= 6.0;
    }
}

/// The voice at `(x, y)`, `w` wide: the envelope's and the filter's names and planes, their
/// knobs under them, the Amp's six at the right. Its foot.
fn voice(def: &BuiltinDef, b: &mut Body, x: f32, y: f32, w: f32) -> f32 {
    let avail = w - AMP_W - 2.0 * GAP;
    let env_w = (avail * ENV_SHARE).floor();
    let filter_w = avail - env_w;
    let env = Rect::new(x, y + HEAD_H, env_w, PLANE_H);
    let filter = Rect::new(env.right() + GAP, y + HEAD_H, filter_w, PLANE_H);
    let ax = x + w - AMP_W;
    b.names
        .push(name(Rect::new(env.x, y + 4.0, 60.0, NAME_H), "Env"));
    b.names
        .push(name(Rect::new(filter.x, y + 4.0, 60.0, NAME_H), "Filter"));
    b.names
        .push(name(Rect::new(ax, y + 4.0, 60.0, NAME_H), "Amp"));
    if let Some(k) = idx(def, "ftype") {
        let sw = control::segments_w(&def.params[k]);
        b.slots.push(Slot {
            k,
            look: Look::Segments,
            dial: SEG_H,
            cell: Rect::new(filter.right() - sw, y, sw, SEG_H),
            label: Some(""),
            dim: false,
        });
    }
    b.graphs.push(env);
    b.graphs.push(filter);
    let ky = env.bottom() + 4.0;
    let kh = cell_h(DIAL_SM);
    let row = |b: &mut Body, ids: &[(&str, Option<&'static str>)], r: Rect| {
        let cw = r.w / ids.len() as f32;
        for (i, (id, label)) in ids.iter().enumerate() {
            let cell = Rect::new(r.x + i as f32 * cw, ky, cw, kh);
            if let Some(mut s) = slot(def, id, Look::Knob, DIAL_SM, cell) {
                s.label = *label;
                b.slots.push(s);
            }
        }
    };
    let env_ids: Vec<(&str, Option<&'static str>)> =
        ENV.iter().map(|(id, l)| (*id, Some(*l))).collect();
    row(b, &env_ids, env);
    let filter_ids: Vec<(&str, Option<&'static str>)> =
        FILTER.iter().map(|id| (*id, None)).collect();
    row(b, &filter_ids, filter);
    for (i, id) in AMP.iter().enumerate() {
        let (col, line) = ((i % 3) as f32, (i / 3) as f32);
        let cell = Rect::new(ax + col * AMP_CELL, y + HEAD_H + line * kh, AMP_CELL, kh);
        b.slots.extend(slot(def, id, Look::Knob, DIAL_SM, cell));
    }
    ky + kh
}

/// The Sampler's body for its files (`s.plugin`) and the zone chosen (`s.pick`).
pub fn body(def: &BuiltinDef, s: &Shape) -> Body {
    let w = s.w.max(NATURAL_W);
    let inner = w - 2.0 * PAD;
    let mut b = Body {
        w,
        ..Body::default()
    };
    let files: &[String] = s.plugin.map_or(&[], |p| p.samples.as_slice());
    if files.is_empty() {
        drop_plane(&mut b, Rect::new(PAD, PAD, inner, EMPTY_H));
        b.h = PAD + EMPTY_H + PAD;
        return b;
    }
    let pick = s.pick.min(files.len() - 1);
    let mut y = PAD;
    sample_row(def, &mut b, PAD, y, inner, &files[pick]);
    y += ROW_H + GAP;
    b.graphs.push(Rect::new(PAD, y, inner, WAVE_H));
    b.main = 0;
    y += WAVE_H + GAP;
    y = voice(def, &mut b, PAD, y, inner) + GAP;
    let keys = play::keys(Rect::new(PAD, y + ZONE_H + 4.0, inner, KEYS_H));
    let span = |p: u8| {
        keys.iter()
            .find(|k| k.pitch == p.clamp(LOW, HIGH))
            .map(|k| (k.r.x, k.r.right(), k.r.x + k.r.w / 2.0))
    };
    for z in zones(files) {
        if z.hi < LOW || z.lo > HIGH {
            continue;
        }
        let (Some((x0, _, _)), Some((_, x1, _))) = (span(z.lo), span(z.hi)) else {
            continue;
        };
        let mut spot = Spot::new(
            Rect::new(x0, y, (x1 - x0 - 1.0).max(4.0), ZONE_H),
            Mark::Zone(z.file),
            stem(&files[z.file]),
        );
        spot.on = z.file == pick;
        spot.note = Some(z.root);
        spot.dot = (LOW..=HIGH).contains(&z.root);
        b.spots.push(spot);
    }
    b.keys = keys;
    b.h = y + ZONE_H + 4.0 + KEYS_H + PAD;
    b
}

/// The Drum Kit's body for its files and the pad chosen (`s.pick`) and its layer (`s.layer`).
pub fn kit_body(def: &BuiltinDef, s: &Shape) -> Body {
    let w = s.w.max(KIT_W);
    let mut b = Body {
        w,
        ..Body::default()
    };
    let pick = s.pick.min(PADS - 1);
    let filled = s.plugin.filter(|p| !p.samples.is_empty());
    let pad_of = |p: usize| filled.map_or_else(Vec::new, |x| pad_files(def, x, p));
    let flag = |p: usize, id: &str| {
        filled.is_some_and(|x| x.params.get(&pad_key(p, id)).is_some_and(|v| *v >= 0.5))
    };
    // The bank tabs and the pads.
    let mut y = PAD;
    let tab_w = LEFT_W / BANKS.len() as f32;
    for (n, letter) in BANKS.iter().enumerate() {
        let mut spot = Spot::new(
            Rect::new(PAD + n as f32 * tab_w, y, tab_w - 2.0, BANK_H),
            Mark::Bank(n),
            *letter,
        );
        spot.on = pick / play::PADS == n;
        spot.dot = (0..play::PADS).any(|p| !pad_of(n * play::PADS + p).is_empty());
        b.spots.push(spot);
    }
    y += BANK_H + 6.0;
    let grid = Rect::new(PAD, y, LEFT_W, 4.0 * PAD_H + 3.0 * play::GAP);
    b.pads = play::pads(grid, pick / play::PADS);
    for k in &b.pads {
        let p = play::pad_of(k.pitch);
        let files = pad_of(p);
        let mut spot = Spot::new(k.r, Mark::Pad(p), "");
        spot.on = p == pick;
        if let (Some(&first), Some(x)) = (files.first(), filled) {
            spot.text = stem(&x.samples[first]);
            if files.len() > 1 {
                let mode = x
                    .params
                    .get(&pad_key(p, "layer_mode"))
                    .and_then(|v| {
                        def.per_pad
                            .iter()
                            .find(|s| s.id == "layer_mode")
                            .map(|s| s.to_plain(*v).round() as usize)
                    })
                    .unwrap_or(0);
                spot.sub = format!("{} {}", LAYER_TAGS[mode.min(3)], files.len());
            }
        }
        b.spots.push(spot);
        if !files.is_empty() {
            let (mw, mh) = MS;
            let my = k.r.bottom() - mh - MS_FOOT;
            let mut m = Spot::new(
                Rect::new(k.r.right() - 2.0 * mw - 6.0, my, mw, mh),
                Mark::Mute(p),
                "M",
            );
            m.on = flag(p, "mute");
            let mut solo = Spot::new(
                Rect::new(k.r.right() - mw - 4.0, my, mw, mh),
                Mark::Solo(p),
                "S",
            );
            solo.on = flag(p, "solo");
            b.spots.push(m);
            b.spots.push(solo);
        }
    }
    let left_foot = grid.bottom();
    // The chosen pad.
    let rx = PAD + LEFT_W + 16.0;
    let rw = w - rx - PAD;
    let files = pad_of(pick);
    let mut title = Spot::new(
        Rect::new(rx, PAD, rw - CHOKE_W - 56.0, ROW_H),
        Mark::Title,
        xirius_params::PAD_NAMES[pick],
    );
    title.note = Some(((usize::from(play::FIRST_PAD) + pick) % 128) as u8);
    if let (Some(&first), Some(x)) = (files.first(), filled) {
        title.sub = stem(&x.samples[first]);
    }
    b.spots.push(title);
    let Some(x) = filled else {
        let y = PAD + ROW_H + GAP;
        drop_plane(&mut b, Rect::new(rx, y, rw, left_foot - y));
        b.h = left_foot + PAD;
        return b;
    };
    b.names.push(name(
        Rect::new(rx + rw - CHOKE_W - 48.0, PAD + 6.0, 44.0, NAME_H),
        "Choke",
    ));
    b.slots.extend(slot(
        def,
        "choke",
        Look::Menu,
        ROW_H,
        Rect::new(rx + rw - CHOKE_W, PAD, CHOKE_W, ROW_H),
    ));
    // Layer Mode and Trigger, the second under the first when they do not fit.
    let mut y = PAD + ROW_H + GAP;
    let mut cx = rx;
    for (id, label, lw) in [
        ("layer_mode", "Layer Mode", 74.0),
        ("trigger", "Trigger", 50.0),
    ] {
        let Some(k) = idx(def, id) else { continue };
        let sw = control::segments_w(&def.params[k]);
        if cx > rx && cx + lw + sw > rx + rw {
            cx = rx;
            y += SEG_H + 6.0;
        }
        b.names.push(name(
            Rect::new(cx, y + (SEG_H - NAME_H) / 2.0, lw, NAME_H),
            label,
        ));
        b.slots.push(Slot {
            k,
            look: Look::Segments,
            dial: SEG_H,
            cell: Rect::new(cx + lw, y, sw, SEG_H),
            label: Some(""),
            dim: false,
        });
        cx += lw + sw + 16.0;
    }
    y += SEG_H + GAP;
    if files.is_empty() {
        drop_plane(&mut b, Rect::new(rx, y, rw, left_foot - y));
    } else {
        let layer = s.layer.min(files.len() - 1);
        b.names.push(name(
            Rect::new(rx, y + (LAYER_H - NAME_H) / 2.0, 48.0, NAME_H),
            "Layers",
        ));
        let (lx, lw) = (rx + 52.0, rw - 52.0);
        let velocity = plain(def, s.values, "layer_mode").round() == 0.0;
        let ranges = velocities(def, x, &files);
        let mut at = lx;
        for (j, &file) in files.iter().enumerate() {
            let (lo, hi) = ranges[j];
            let share = if velocity {
                f32::from(hi - lo + 1) / 128.0
            } else {
                1.0 / files.len() as f32
            };
            let cw = if j + 1 == files.len() {
                lx + lw - at
            } else {
                (lw * share).round()
            };
            let text = if velocity {
                format!("{lo}–{hi}")
            } else {
                stem(&x.samples[file])
            };
            let mut spot = Spot::new(Rect::new(at, y, cw - 2.0, LAYER_H), Mark::Layer(j), text);
            spot.on = j == layer;
            b.spots.push(spot);
            at += cw;
        }
        y += LAYER_H + GAP;
        sample_row(def, &mut b, rx, y, rw, &x.samples[files[layer]]);
        y += ROW_H + GAP;
        b.graphs
            .insert(0, Rect::new(rx, y, rw, (left_foot - y).max(60.0)));
        b.main = 0;
    }
    let top = b.graphs[0].bottom().max(left_foot) + GAP + 4.0;
    let foot = voice(def, &mut b, PAD, top, w - 2.0 * PAD);
    b.h = foot + PAD;
    b
}

/// The file whose wave the window of `plugin` shows: the Sampler's zone's, the Drum Kit's
/// chosen layer's.
pub fn wave_file(plugin: &Plugin, pick: usize, layer: usize) -> Option<&str> {
    let def = plugin.builtin()?;
    match def.uid {
        "sampler" => plugin
            .samples
            .get(pick.min(plugin.samples.len().saturating_sub(1)))
            .map(String::as_str),
        "drum-kit" => {
            let files = pad_files(def, plugin, pick.min(PADS - 1));
            let i = *files.get(layer.min(files.len().saturating_sub(1)))?;
            plugin.samples.get(i).map(String::as_str)
        }
        _ => None,
    }
}

// ------------------------------------------------------------------ the graphs

fn inset(r: Rect) -> Rect {
    Rect::new(
        r.x + INSET,
        r.y + INSET,
        (r.w - 2.0 * INSET).max(1.0),
        (r.h - 2.0 * INSET).max(1.0),
    )
}

fn point(def: &BuiltinDef, values: &[f64], k: usize, at: (f32, f32), label: &str) -> Point {
    let spec = &def.params[k];
    let norm = values.get(k).copied().unwrap_or(spec.default_norm());
    Point {
        name: control::name(def, spec).to_string(),
        label: label.to_string(),
        value: control::text(spec, norm),
        at,
        x: None,
        y: None,
        wheel: Some(k),
        on: true,
        menu: Vec::new(),
        rest: Vec::new(),
    }
}

/// A time's axis from `a`, `w` long, over the parameter's range (ms on a log scale).
fn time_axis(def: &BuiltinDef, k: usize, a: f32, w: f32) -> Axis {
    let (lo, hi) = def.params[k].range();
    Axis::log_from(a, a + w, lo, hi, 1.0)
}

/// The envelope's shape on its plane: the attack's, the decay's and the release's widths
/// and where its corners are.
struct Env {
    a: (f32, f32),
    d: (f32, f32),
    hold: f32,
    r: (f32, f32),
    sustain: f64,
    axes: [Axis; 3],
}

fn env_shape(def: &BuiltinDef, r: Rect, values: &[f64]) -> Option<(Env, [usize; 4])> {
    let i = inset(r);
    let ks = [
        idx(def, "attack")?,
        idx(def, "decay")?,
        idx(def, "sustain")?,
        idx(def, "release")?,
    ];
    let (wa, wd, wh, wr) = (0.22 * i.w, 0.30 * i.w, 0.14 * i.w, 0.30 * i.w);
    let sustain = plain(def, values, "sustain") / 100.0;
    let sy = i.bottom() - i.h * sustain as f32;
    let ta = time_axis(def, ks[0], i.x, wa);
    let ax = ta.px(plain(def, values, "attack"));
    let td = time_axis(def, ks[1], ax, wd);
    let dx = td.px(plain(def, values, "decay"));
    let hold = dx + wh;
    let tr = time_axis(def, ks[3], hold, wr);
    let rx = tr.px(plain(def, values, "release"));
    Some((
        Env {
            a: (ax, i.y),
            d: (dx, sy),
            hold,
            r: (rx, i.bottom()),
            sustain,
            axes: [ta, td, tr],
        },
        ks,
    ))
}

/// The envelope's points: A at its peak, D where it reaches the sustain (its height the
/// sustain), R where the release ends.
pub fn env_points(def: &BuiltinDef, r: Rect, values: &[f64]) -> Vec<Point> {
    let Some((e, [ka, kd, ks, kr])) = env_shape(def, r, values) else {
        return Vec::new();
    };
    let i = inset(r);
    let mut a = point(def, values, ka, e.a, "");
    a.x = Some(Drive {
        k: ka,
        axis: e.axes[0],
    });
    let mut d = point(def, values, kd, e.d, "");
    d.value = format!(
        "{}, {}",
        d.value,
        control::text(&def.params[ks], values.get(ks).copied().unwrap_or(1.0))
    );
    d.x = Some(Drive {
        k: kd,
        axis: e.axes[1],
    });
    d.y = Some(Drive {
        k: ks,
        axis: Axis::lin(i.bottom(), i.y, 0.0, 100.0),
    });
    d.wheel = Some(ks);
    let mut rel = point(def, values, kr, e.r, "");
    rel.x = Some(Drive {
        k: kr,
        axis: e.axes[2],
    });
    vec![a, d, rel]
}

/// The envelope's line on its plane `r`, in the plane's own place.
fn env_curve(e: &Env, r: Rect) -> Vec<(f32, f32)> {
    let i = inset(r);
    let (top, bottom) = (i.y, i.bottom());
    let level = |v: f64| bottom - (bottom - top) * v as f32;
    let mut pts = vec![(i.x, bottom), e.a];
    // 60 dB down over the decay's and the release's time, as the engine falls.
    for n in 1..=24 {
        let t = n as f64 / 24.0;
        let v = e.sustain + (1.0 - e.sustain) * 10f64.powf(-3.0 * t);
        pts.push((e.a.0 + (e.d.0 - e.a.0) * t as f32, level(v)));
    }
    pts.push((e.hold, level(e.sustain)));
    for n in 1..=24 {
        let t = n as f64 / 24.0;
        let v = e.sustain * 10f64.powf(-3.0 * t) * (1.0 - t);
        pts.push((e.hold + (e.r.0 - e.hold) * t as f32, level(v)));
    }
    pts.into_iter().map(|(x, y)| (x - r.x, y - r.y)).collect()
}

pub fn paint_env(pen: &mut Pen, def: &BuiltinDef, r: Rect, values: &[f64], view: &View) {
    let c = pen.c;
    pen.round(r, c.surface_2, Radius::S);
    let Some((e, _)) = env_shape(def, r, values) else {
        return;
    };
    let pts = env_curve(&e, r);
    let base = inset(r).bottom() - r.y;
    let strong = if view.bypassed {
        c.scroll_thumb[0]
    } else {
        c.ink_strong
    };
    pen.mask(r, |pen| {
        graph::area(pen, r, &pts, base, c.surface_3);
        graph::stroke(pen, r, &pts, 2.0, strong);
    });
}

/// The filter's level (dB) at `f` for its type, cutoff and resonance, as the engine's
/// state-variable filter has it.
pub fn filter_db(ftype: usize, cutoff: f64, resonance: f64, f: f64) -> f64 {
    let q = 0.5 + resonance.clamp(0.0, 100.0) * 0.195;
    let w = f / cutoff.max(1.0);
    let den = ((1.0 - w * w).powi(2) + (w / q).powi(2)).sqrt();
    let a = match ftype {
        1 => (w / q) / den,
        2 => w * w / den,
        _ => 1.0 / den,
    };
    20.0 * a.max(1e-6).log10()
}

const FREQ: (f64, f64) = (20.0, 20_000.0);
const LEVEL: (f64, f64) = (-42.0, 30.0);

fn filter_axes(r: Rect) -> (Axis, Axis) {
    let i = inset(r);
    (
        Axis::log(i.x, i.right(), FREQ.0, FREQ.1),
        Axis::lin(i.bottom(), i.y, LEVEL.0, LEVEL.1),
    )
}

/// The filter's point: across the cutoff, up the resonance's peak.
pub fn filter_points(def: &BuiltinDef, r: Rect, values: &[f64]) -> Vec<Point> {
    let (Some(kc), Some(kr)) = (idx(def, "cutoff"), idx(def, "resonance")) else {
        return Vec::new();
    };
    let (fx, ly) = filter_axes(r);
    let res = plain(def, values, "resonance");
    let q = 0.5 + res * 0.195;
    let mut p = point(
        def,
        values,
        kc,
        (fx.px(plain(def, values, "cutoff")), ly.px(20.0 * q.log10())),
        "",
    );
    p.value = format!(
        "{}, Res {}",
        p.value,
        control::text(&def.params[kr], values.get(kr).copied().unwrap_or(0.0))
    );
    p.x = Some(Drive { k: kc, axis: fx });
    p.y = Some(Drive {
        k: kr,
        axis: Axis::log_from(
            ly.px(20.0 * 0.5f64.log10()),
            ly.px(20.0 * 20f64.log10()),
            0.0,
            100.0,
            0.5 / 0.195,
        ),
    });
    p.wheel = Some(kr);
    vec![p]
}

pub fn paint_filter(pen: &mut Pen, def: &BuiltinDef, r: Rect, values: &[f64], view: &View) {
    let c = pen.c;
    pen.round(r, c.surface_2, Radius::S);
    let (fx, ly) = filter_axes(r);
    let ftype = plain(def, values, "ftype").round() as usize;
    let (cutoff, res) = (
        plain(def, values, "cutoff"),
        plain(def, values, "resonance"),
    );
    let i = inset(r);
    let pts: Vec<(f32, f32)> = (0..=96)
        .map(|n| {
            let x = i.x + i.w * n as f32 / 96.0;
            let f = fx.value(x);
            let d = filter_db(ftype, cutoff, res, f).clamp(LEVEL.0, LEVEL.1);
            (x - r.x, ly.px(d) - r.y)
        })
        .collect();
    let strong = if view.bypassed {
        c.scroll_thumb[0]
    } else {
        c.ink_strong
    };
    pen.mask(r, |pen| {
        graph::faces(
            pen,
            r,
            fx,
            &[20.0, 100.0, 1000.0, 10_000.0, 20_000.0],
            c.surface_1.opacity(0.5),
        );
        graph::area(pen, r, &pts, i.bottom() - r.y, c.surface_3);
        graph::stroke(pen, r, &pts, 2.0, strong);
    });
}

fn share_axis(r: Rect) -> Axis {
    let i = inset(r);
    Axis::lin(i.x, i.right(), 0.0, 100.0)
}

/// The wave's points: Start and End at its top, the loop's ends at its foot while it loops.
pub fn wave_points(def: &BuiltinDef, r: Rect, values: &[f64]) -> Vec<Point> {
    if r.w <= 0.0 {
        return Vec::new();
    }
    let i = inset(r);
    let ax = share_axis(r);
    let mut out = Vec::new();
    let mut at = |id: &str, y: f32| {
        if let Some(k) = idx(def, id) {
            let mut p = point(def, values, k, (ax.px(plain(def, values, id)), y), "");
            p.x = Some(Drive { k, axis: ax });
            out.push(p);
        }
    };
    at("start", i.y);
    at("end", i.y);
    if on(def, values, "loop") {
        at("loop_start", i.bottom());
        at("loop_end", i.bottom());
    }
    out
}

pub fn paint_wave(pen: &mut Pen, def: &BuiltinDef, r: Rect, values: &[f64], view: &View) {
    if r.w <= 0.0 {
        return;
    }
    let c = pen.c;
    pen.round(r, c.surface_2, Radius::S);
    let i = inset(r);
    let ax = share_axis(r);
    let mid = i.y + i.h / 2.0 - r.y;
    let ink = if view.bypassed {
        c.scroll_thumb[0]
    } else {
        c.ink
    };
    let (start, end) = (
        ax.px(plain(def, values, "start")),
        ax.px(plain(def, values, "end")),
    );
    pen.mask(r, |pen| {
        if on(def, values, "loop") {
            let (a, b) = (
                ax.px(plain(def, values, "loop_start")),
                ax.px(plain(def, values, "loop_end")),
            );
            if b > a {
                pen.fill(Rect::new(a, r.y, b - a, r.h), c.clip);
            }
        }
        match view.wave {
            Some(p) if p.frames > 0 => {
                let cols = i.w.max(1.0) as usize;
                let per = p.frames as f64 / cols as f64;
                let level = p.level_for(per);
                let reverse = on(def, values, "reverse");
                let half = i.h / 2.0;
                let mut top = Vec::with_capacity(cols);
                let mut bottom = Vec::with_capacity(cols);
                for n in 0..cols {
                    let m = if reverse { cols - 1 - n } else { n };
                    let (a, b) = (m as f64 * per, (m + 1) as f64 * per);
                    let pk = p.reduce(level, a, b).unwrap_or_default();
                    let x = i.x + n as f32 - r.x;
                    top.push((x, mid - half * f32::from(pk.max) / 127.0));
                    bottom.push((x, mid - half * f32::from(pk.min) / 127.0));
                }
                graph::area(pen, r, &top, mid, ink.opacity(0.5));
                graph::area(pen, r, &bottom, mid, ink.opacity(0.5));
            }
            _ => pen.fill(Rect::new(i.x, r.y + mid, i.w, 1.0), ink.opacity(0.5)),
        }
        // What does not play is dim.
        if start > i.x {
            pen.fill(
                Rect::new(r.x, r.y, start - r.x, r.h),
                c.surface_2.opacity(0.6),
            );
        }
        if end < i.right() {
            pen.fill(
                Rect::new(end, r.y, r.right() - end, r.h),
                c.surface_2.opacity(0.6),
            );
        }
        for x in [start, end] {
            pen.fill(Rect::new(x - 1.0, r.y, 2.0, r.h), c.ink_strong);
        }
    });
}

// ------------------------------------------------------------------ the spots

/// The spots of a Sampler's or a Drum Kit's body: `hot` the one under the pointer.
pub fn paint_spots(pen: &mut Pen, body: &Body, hot: Option<usize>) {
    let c = pen.c;
    for (n, s) in body.spots.iter().enumerate() {
        let over = hot == Some(n);
        let r = s.r;
        match s.mark {
            Mark::Prev | Mark::Next => {
                if over {
                    pen.round(r, c.surface_3, Radius::S);
                }
                let icon = if s.mark == Mark::Prev {
                    "chevron-left"
                } else {
                    "chevron-right"
                };
                let d = 14.0;
                let at = Rect::new(r.x + (r.w - d) / 2.0, r.y + (r.h - d) / 2.0, d, d);
                pen.icon(icon, at, c.ink);
            }
            Mark::Name => {
                pen.round(r, c.surface_2, Radius::S);
                pen.icon(
                    "wave",
                    Rect::new(r.x + 8.0, r.y + (r.h - 14.0) / 2.0, 14.0, 14.0),
                    c.ink,
                );
                let t = Rect::new(r.x + 26.0, r.y, r.w - 30.0, r.h);
                let text = pen.fit(&s.text, FontSize::Small, Weight::Semibold, t.w);
                pen.text(
                    &text,
                    FontSize::Small,
                    Weight::Semibold,
                    c.ink_strong,
                    t,
                    Align::Left,
                );
            }
            Mark::Zone(_) | Mark::Layer(_) => {
                let face = if s.on {
                    c.fill
                } else if over {
                    c.surface_3
                } else {
                    c.surface_2
                };
                pen.round(r, face, Radius::Xs);
                let ink = if s.on { c.on_fill } else { c.ink };
                let t = Rect::new(r.x + 6.0, r.y, (r.w - 8.0).max(0.0), r.h);
                let text = pen.fit(&s.text, FontSize::Micro, Weight::Semibold, t.w);
                pen.text(
                    &text,
                    FontSize::Micro,
                    Weight::Semibold,
                    ink,
                    t,
                    Align::Left,
                );
            }
            Mark::Bank(_) => {
                let face = if s.on {
                    c.surface_1
                } else if over {
                    c.surface_3
                } else {
                    c.surface_2
                };
                pen.round(r, face, Radius::S);
                let ink = if s.on { c.ink_strong } else { c.ink };
                pen.text(
                    &s.text,
                    FontSize::Small,
                    Weight::Semibold,
                    ink,
                    r,
                    Align::Center,
                );
                if s.dot {
                    let d = 4.0;
                    let dot = Rect::new(r.x + (r.w - d) / 2.0, r.bottom() - d - 2.0, d, d);
                    pen.round(dot, c.ink_blue, Radius::Pill);
                }
            }
            Mark::Pad(_) => {
                if s.on {
                    pen.outline(r, c.ring, Radius::S, 2.0);
                }
                if !s.text.is_empty() {
                    let t = Rect::new(r.x + 6.0, r.y + r.h / 2.0 - 8.0, r.w - 12.0, 14.0);
                    let text = pen.fit(&s.text, FontSize::Micro, Weight::Semibold, t.w);
                    pen.text(
                        &text,
                        FontSize::Micro,
                        Weight::Semibold,
                        c.ink_strong,
                        t,
                        Align::Left,
                    );
                }
                if !s.sub.is_empty() {
                    let t = Rect::new(r.x + 6.0, r.bottom() - 18.0, r.w / 2.0, 14.0);
                    pen.text(
                        &s.sub,
                        FontSize::Micro,
                        Weight::Regular,
                        c.ink,
                        t,
                        Align::Left,
                    );
                }
            }
            Mark::Mute(_) | Mark::Solo(_) => {
                let face = if s.on {
                    c.fill
                } else if over {
                    c.surface_1
                } else {
                    c.surface_2
                };
                pen.round(r, face, Radius::Xs);
                let ink = if s.on { c.on_fill } else { c.ink };
                pen.text(
                    &s.text,
                    FontSize::Micro,
                    Weight::Semibold,
                    ink,
                    r,
                    Align::Center,
                );
            }
            Mark::Drop => {
                pen.round(r, c.surface_2, Radius::S);
                let d = 24.0;
                let cy = r.y + r.h / 2.0;
                pen.icon(
                    "download",
                    Rect::new(r.x + (r.w - d) / 2.0, cy - 50.0, d, d),
                    c.ink,
                );
                pen.text(
                    &s.text,
                    FontSize::Body,
                    Weight::Semibold,
                    c.ink_strong,
                    Rect::new(r.x, cy - 20.0, r.w, 20.0),
                    Align::Center,
                );
            }
            Mark::Browse => {
                pen.round(r, if over { c.surface_1 } else { c.surface_3 }, Radius::S);
                let d = 14.0;
                pen.icon(
                    "search",
                    Rect::new(r.x + 12.0, r.y + (r.h - d) / 2.0, d, d),
                    c.ink,
                );
                pen.text(
                    &s.text,
                    FontSize::Small,
                    Weight::Semibold,
                    c.ink_strong,
                    Rect::new(r.x + 30.0, r.y, r.w - 36.0, r.h),
                    Align::Left,
                );
            }
            Mark::Title => {
                let w = pen.measure(&s.text, FontSize::Title, Weight::Semibold);
                pen.text(
                    &s.text,
                    FontSize::Title,
                    Weight::Semibold,
                    c.ink_strong,
                    Rect::new(r.x, r.y, w + 4.0, r.h),
                    Align::Left,
                );
                let mut tail = s.sub.clone();
                if let Some(n) = s.note {
                    let note = crate::roll::pitch_name(n);
                    tail = if tail.is_empty() {
                        note
                    } else {
                        format!("{tail} · {note}")
                    };
                }
                let t = Rect::new(r.x + w + 12.0, r.y, (r.w - w - 12.0).max(0.0), r.h);
                let tail = pen.fit(&tail, FontSize::Small, Weight::Regular, t.w);
                pen.text(
                    &tail,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink,
                    t,
                    Align::Left,
                );
            }
        }
        if let (Mark::Zone(_), Some(note)) = (s.mark, s.note.filter(|_| s.dot)) {
            // The root's dot over its key.
            if let Some(k) = body.keys.iter().find(|k| k.pitch == note) {
                let d = 4.0;
                let dot = Rect::new(k.r.x + k.r.w / 2.0 - d / 2.0, r.bottom() - d - 2.0, d, d);
                pen.round(
                    dot,
                    if s.on { c.on_fill } else { c.ink_strong },
                    Radius::Pill,
                );
            }
        }
    }
}

/// The spot under `(x, y)` that takes a press.
pub fn spot_at(body: &Body, x: f32, y: f32) -> Option<usize> {
    body.spots
        .iter()
        .rposition(|s| s.mark.takes() && s.r.contains(x, y))
}
