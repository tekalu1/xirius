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
use xirius_engine::stock::files::{root_keys, root_of};
use xirius_params::{BuiltinDef, PADS, Scope, bare_id, pad_key};

use super::control::{self, DIAL_SM, Look, NAME_H, SEG_H, TOGGLE_H, VALUE_H};
use super::graph::{self, Axis, Choice, Drive, Point, View};
use super::layout::{Body, MS_FOOT, Mark, Name, PAD, Shape, Slot, Spot};
use super::play::{self, HIGH, KEYS_H, LOW};
use crate::draw::{Align, Pen};
use crate::studio::Rect;

/// What the empty plane and its button say (the mock's words).
pub const DROP_LABEL: &str = "Drop samples here";
pub const BROWSE_LABEL: &str = "Browse";

/// The width the Sampler's window opens at.
pub const NATURAL_W: f32 = 780.0;
/// The Drum Kit's.
pub const KIT_W: f32 = 905.0;
/// The narrowest both go: the mock's narrow form.
pub const MIN_W: f32 = 520.0;
/// The Drum Kit at this content width or narrower stacks its pads over the pad's editor.
const KIT_STACK: f32 = 700.0;
/// The pads' column then, at its widest; and a pad's height for its width.
const STACK_W: f32 = 340.0;
const PAD_ASPECT: f32 = 0.86;
const GAP: f32 = 8.0;
/// A row of the sample: its name and its buttons.
const ROW_H: f32 = 26.0;
/// The ‹ › beside a sample's name.
const ARROW_W: f32 = 24.0;
/// The loop's X-Fade: a small knob in the sample's row, its name before it, its value after.
const XFADE_DIAL: f32 = 20.0;
/// The longest a sample's name gets in its row.
const NAME_MAX: f32 = 260.0;
const WAVE_H: f32 = 96.0;
/// The tab of a point of the wave.
const TAB: (f32, f32) = (12.0, 10.0);
/// The voice: the names over its planes, the planes, the Amp's width.
const HEAD_H: f32 = 24.0;
const PLANE_H: f32 = 120.0;
const AMP_W: f32 = 174.0;
const AMP_CELL: f32 = 58.0;
/// The share of the envelope's plane of what the planes have.
const ENV_SHARE: f32 = 0.585;
/// The narrowest the envelope's and the filter's planes go before the Amp goes under them,
/// and the content width at which the Amp takes the whole width, its knobs in one row.
const ENV_MIN: f32 = 260.0;
const FILTER_MIN: f32 = 200.0;
const AMP_ROW: f32 = 620.0;
/// The narrowest a sample's name gets before its buttons go under it.
const NAME_MIN: f32 = 80.0;
/// The zones' strip over the keys.
const ZONE_H: f32 = 22.0;
/// The narrowest a zone is drawn, so one of a single key off the keys can be pressed.
const ZONE_MIN: f32 = 6.0;
/// The empty plane a sample is dropped on, and its button.
const EMPTY_H: f32 = 196.0;
const BROWSE: (f32, f32) = (96.0, 26.0);
/// The Drum Kit's left column: the bank tabs, the pads.
const LEFT_W: f32 = 305.0;
const BANK_H: f32 = 24.0;
const PAD_H: f32 = 63.0;
/// A pad's M and S.
const MS: (f32, f32) = (16.0, 14.0);
const CHOKE_W: f32 = 96.0;
const LAYER_H: f32 = 24.0;
/// The weight of a zone's and a layer's words: the mock's regular `fs-micro`.
pub const SPAN_WEIGHT: Weight = Weight::Regular;

/// The narrowest a layer is drawn, so one of a velocity or two can still be pressed
/// (L2-smp-7); and the width of an edge between two that takes the pointer.
const LAYER_MIN: f32 = 14.0;
const EDGE_HIT: f32 = 10.0;
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
const LAYER_TAGS: [&str; 4] = ["Vel", "RR", "Rnd", "Stack"];

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

/// How a Drum Kit's pad picks among its layers: 0 Velocity, 1 Round Robin, 2 Random,
/// 3 Stack.
pub fn pad_mode(def: &BuiltinDef, plugin: &Plugin, pad: usize) -> usize {
    plugin
        .params
        .get(&pad_key(pad, "layer_mode"))
        .and_then(|v| {
            def.per_pad
                .iter()
                .find(|s| s.id == "layer_mode")
                .map(|s| s.to_plain(*v).round() as usize)
        })
        .unwrap_or(0)
        .min(3)
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

/// The Sampler's zones as the engine plays them: by root, each up to halfway to the next;
/// files of one root share its keys.
pub fn zones(files: &[String]) -> Vec<Zone> {
    let mut order: Vec<(u8, usize)> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (root_of(&leaf(f)), i))
        .collect();
    order.sort();
    order
        .iter()
        .map(|&(root, file)| {
            let (lo, hi) = root_keys(&order, root);
            Zone { file, root, lo, hi }
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

/// Where the `j`th of a pad's velocity layers may end: from its lowest velocity to one
/// under the next one's top (UI-334).
pub fn edge_range(def: &BuiltinDef, plugin: &Plugin, files: &[usize], j: usize) -> (u8, u8) {
    let ranges = velocities(def, plugin, files);
    // The first layer's top is no lower than `file<n>.top` goes (low 19).
    let least = def
        .per_file
        .iter()
        .find(|p| p.id == "top")
        .map_or(0, |s| s.to_plain(0.0).round() as u8);
    let lo = ranges.get(j).map_or(0, |r| r.0).max(least);
    let max = ranges.get(j + 1).map_or(127, |r| r.1).saturating_sub(1);
    (lo, max.max(lo))
}

/// The tops (`file<n>.top`, 0..1) that make the `j`th of a pad's velocity layers end at
/// `hi` and keep the others where they play: all of them, so they rise as the engine reads
/// them (UI-334).
pub fn tops_with(
    def: &BuiltinDef,
    plugin: &Plugin,
    files: &[usize],
    j: usize,
    hi: u8,
) -> Vec<(String, f64)> {
    let Some(spec) = def.per_file.iter().find(|p| p.id == "top") else {
        return Vec::new();
    };
    velocities(def, plugin, files)
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let top = if i == j { hi } else { r.1 };
            (
                xirius_params::file_key(files[i], "top"),
                spec.to_norm(f64::from(top)),
            )
        })
        .collect()
}

/// The widths of `shares` (each from 0 to 1) across `w`: each at least [`LAYER_MIN`], what
/// that takes coming off the wider ones (L2-smp-7).
fn widths(shares: &[f32], w: f32) -> Vec<f32> {
    let raw: Vec<f32> = shares.iter().map(|s| s * w).collect();
    let short: f32 = raw.iter().map(|r| (LAYER_MIN - r).max(0.0)).sum();
    let spare: f32 = raw.iter().map(|r| (r - LAYER_MIN).max(0.0)).sum();
    raw.iter()
        .map(|&r| {
            if r <= LAYER_MIN {
                LAYER_MIN
            } else if spare > 0.0 {
                r - (r - LAYER_MIN) / spare * short.min(spare)
            } else {
                r
            }
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

/// The sample's row at `(x, y)`, `w` wide: ‹ its name › at the left, its buttons at the
/// right, the loop's X-Fade after Loop while it is on (UI-333). Where the name does not
/// fit beside them, the buttons go on a second line. Its height.
fn sample_row(
    def: &BuiltinDef,
    values: &[f64],
    b: &mut Body,
    at: (f32, f32),
    w: f32,
    file: &str,
) -> f32 {
    let (x, y) = at;
    let text = stem(file);
    let xfade = idx(def, "xfade").filter(|_| on(def, values, "loop"));
    let buttons: f32 = SAMPLE_BUTTONS
        .iter()
        .filter_map(|id| idx(def, id))
        .map(|k| control::toggle_w(control::name(def, &def.params[k])) + 6.0)
        .sum::<f32>()
        + xfade.map_or(0.0, |k| {
            control::row_knob_w(control::name(def, &def.params[k])) + 6.0
        });
    let arrows = 2.0 * ARROW_W + 6.0;
    let room = w - buttons - arrows;
    let wrap = room < NAME_MIN;
    let fit = if wrap { w - arrows } else { room };
    let name_w = (control::label_w(&text) + 32.0)
        .clamp(NAME_MIN, NAME_MAX)
        .min(fit);
    let by = if wrap { y + ROW_H + 4.0 } else { y };
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
        if *id == "loop"
            && let Some(xf) = xfade
        {
            let cw = control::row_knob_w(control::name(def, &def.params[xf]));
            right -= cw;
            b.slots.push(Slot {
                k: xf,
                look: Look::Knob,
                dial: XFADE_DIAL,
                cell: Rect::new(right, by + (ROW_H - XFADE_DIAL) / 2.0, cw, XFADE_DIAL),
                label: None,
                dim: false,
            });
            right -= 6.0;
        }
        let tw = control::toggle_w(control::name(def, &def.params[k]));
        right -= tw;
        b.slots.push(Slot {
            k,
            look: Look::Toggle,
            dial: TOGGLE_H,
            cell: Rect::new(right, by, tw, TOGGLE_H),
            label: None,
            dim: false,
        });
        right -= 6.0;
    }
    by + ROW_H - y
}

/// The voice at `(x, y)`, `w` wide: the envelope's and the filter's names and planes, their
/// knobs under them, the Amp's knobs at the right. Narrower than the three, the Amp goes
/// under the planes (they share the width 3 : 2 from their narrowest), and at [`AMP_ROW`]
/// or narrower it takes the whole width, its knobs in one row (the mock's). Its foot.
fn voice(def: &BuiltinDef, b: &mut Body, x: f32, y: f32, w: f32) -> f32 {
    let under = w < ENV_MIN + FILTER_MIN + AMP_W + 2.0 * GAP;
    let planes = if under {
        w - GAP
    } else {
        w - AMP_W - 2.0 * GAP
    };
    let env_w = if under {
        (ENV_MIN + (planes - ENV_MIN - FILTER_MIN).max(0.0) * 0.6).floor()
    } else {
        (planes * ENV_SHARE).floor()
    };
    let env = Rect::new(x, y + HEAD_H, env_w, PLANE_H);
    let filter = Rect::new(env.right() + GAP, y + HEAD_H, planes - env_w, PLANE_H);
    let ky = env.bottom() + 4.0;
    let kh = cell_h(DIAL_SM);
    let (ax, ay) = if under {
        (x, ky + kh + GAP)
    } else {
        (x + w - AMP_W, y)
    };
    b.names
        .push(name(Rect::new(env.x, y + 4.0, 60.0, NAME_H), "Env"));
    b.names
        .push(name(Rect::new(filter.x, y + 4.0, 60.0, NAME_H), "Filter"));
    b.names
        .push(name(Rect::new(ax, ay + 4.0, 60.0, NAME_H), "Amp"));
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
    let in_row = under && w <= AMP_ROW;
    for (i, id) in AMP.iter().enumerate() {
        let cell = if in_row {
            let cw = w / AMP.len() as f32;
            Rect::new(x + i as f32 * cw, ay + HEAD_H, cw, kh)
        } else {
            let (col, line) = ((i % 3) as f32, (i / 3) as f32);
            Rect::new(ax + col * AMP_CELL, ay + HEAD_H + line * kh, AMP_CELL, kh)
        };
        b.slots.extend(slot(def, id, Look::Knob, DIAL_SM, cell));
    }
    let lines = if in_row { 1.0 } else { 2.0 };
    (ky + kh).max(ay + HEAD_H + lines * kh)
}

/// The Sampler's body for its files (`s.plugin`) and the zone chosen (`s.pick`).
pub fn body(def: &BuiltinDef, s: &Shape) -> Body {
    let w = s.w.max(MIN_W);
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
    y += sample_row(def, s.values, &mut b, (PAD, y), inner, &files[pick]) + GAP;
    b.graphs.push(Rect::new(PAD, y, inner, WAVE_H));
    b.main = 0;
    y += WAVE_H + GAP;
    y = voice(def, &mut b, PAD, y, inner) + GAP;
    let keys = play::keys(Rect::new(PAD, y + ZONE_H + 4.0, inner, KEYS_H));
    // Every zone can be chosen (N-smp-6): its keys' place on the columns of the white keys,
    // one off the keys a sliver at their edge, and zones of one root side by side in theirs.
    let mut groups: Vec<(Vec<Zone>, f32, f32)> = Vec::new();
    for z in zones(files) {
        match groups.last_mut() {
            Some((g, _, _)) if (g[0].lo, g[0].hi) == (z.lo, z.hi) => g.push(z),
            _ => {
                let (x0, x1) = zone_x(&keys, z.lo, z.hi);
                groups.push((vec![z], x0, x1));
            }
        }
    }
    for k in 1..groups.len() {
        let (prev_x0, prev_x1) = (groups[k - 1].1, groups[k - 1].2);
        if groups[k].1 < prev_x1 {
            groups[k - 1].2 = groups[k].1.max(prev_x0 + ZONE_MIN);
            groups[k].1 = groups[k].1.max(groups[k - 1].2);
            groups[k].2 = groups[k].2.max(groups[k].1 + ZONE_MIN);
        }
    }
    let placed = groups.into_iter().flat_map(|(g, x0, x1)| {
        let share = (x1 - x0) / g.len() as f32;
        g.into_iter().enumerate().map(move |(j, z)| {
            let at = x0 + share * j as f32;
            (z, at, at + share)
        })
    });
    for (z, x0, x1) in placed {
        let mut spot = Spot::new(
            Rect::new(x0, y, (x1 - x0 - 1.0).max(4.0), ZONE_H),
            Mark::Zone(z.file),
            stem(&files[z.file]),
        );
        spot.on = z.file == pick;
        spot.note = Some(z.root);
        spot.dot = true;
        b.spots.push(spot);
    }
    b.keys = keys;
    b.h = y + ZONE_H + 4.0 + KEYS_H + PAD;
    b
}

/// Where a zone of the keys `lo..=hi` sits over `keys`: from the left of its lowest key to
/// the right of its highest, a black key standing for the line it sits on, so zones that meet
/// at a black key meet there; keys off the keyboard a [`ZONE_MIN`] sliver at its edge.
pub fn zone_x(keys: &[play::Key], lo: u8, hi: u8) -> (f32, f32) {
    let (Some(first), Some(last)) = (keys.first(), keys.iter().rfind(|k| !k.black)) else {
        return (0.0, ZONE_MIN);
    };
    let (left, right) = (first.r.x, last.r.right());
    if hi < LOW {
        return (left, left + ZONE_MIN);
    }
    if lo > HIGH {
        return (right - ZONE_MIN, right);
    }
    let place = |p: u8| keys.iter().find(|k| k.pitch == p);
    let line = |r: Rect| r.x + r.w / 2.0;
    let x0 = match place(lo.max(LOW)) {
        Some(k) if k.black => line(k.r),
        Some(k) => k.r.x,
        None => left,
    };
    let x1 = match place(hi.min(HIGH)) {
        Some(k) if k.black => line(k.r),
        Some(k) => k.r.right(),
        None => right,
    };
    (x0, x1.max(x0 + ZONE_MIN))
}

/// The Drum Kit's body for its files and the pad chosen (`s.pick`) and its layer (`s.layer`):
/// the banks and the pads at the left, the chosen pad's editor beside them, the voice under
/// both; at [`KIT_STACK`] or narrower, the pads centred over the editor (the mock's).
pub fn kit_body(def: &BuiltinDef, s: &Shape) -> Body {
    let w = s.w.max(MIN_W);
    let inner = w - 2.0 * PAD;
    let stack = inner <= KIT_STACK;
    let (lx, lw) = if stack {
        let lw = inner.min(STACK_W);
        (PAD + ((inner - lw) / 2.0).floor(), lw)
    } else {
        (PAD, LEFT_W)
    };
    let pad_h = if stack {
        ((lw - 3.0 * play::GAP) / 4.0 * PAD_ASPECT).round()
    } else {
        PAD_H
    };
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
    let soloed = (0..PADS).any(|p| flag(p, "solo"));
    let bank = s.bank.unwrap_or(pick / play::PADS).min(BANKS.len() - 1);
    // The bank tabs and the pads.
    let mut y = PAD;
    let tab_w = lw / BANKS.len() as f32;
    for (n, letter) in BANKS.iter().enumerate() {
        let mut spot = Spot::new(
            Rect::new(lx + n as f32 * tab_w, y, tab_w - 2.0, BANK_H),
            Mark::Bank(n),
            *letter,
        );
        spot.on = bank == n;
        spot.dot = (0..play::PADS).any(|p| !pad_of(n * play::PADS + p).is_empty());
        b.spots.push(spot);
    }
    y += BANK_H + 6.0;
    let grid = Rect::new(lx, y, lw, 4.0 * pad_h + 3.0 * play::GAP);
    b.pads = play::pads(grid, bank);
    for k in &b.pads {
        let p = play::pad_of(k.pitch);
        let files = pad_of(p);
        let mut spot = Spot::new(k.r, Mark::Pad(p), "");
        spot.on = p == pick;
        spot.dot = !files.is_empty();
        spot.dim = !files.is_empty() && (flag(p, "mute") || (soloed && !flag(p, "solo")));
        if let (Some(&first), Some(x)) = (files.first(), filled) {
            spot.text = stem(&x.samples[first]);
            if files.len() > 1 {
                let mode = pad_mode(def, x, p);
                spot.sub = format!("{} {}", LAYER_TAGS[mode], files.len());
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
    // The chosen pad: beside the pads, its planes down to their foot; or under them, its
    // planes at their own heights.
    let (rx, top) = if stack {
        (PAD, left_foot + 12.0)
    } else {
        (PAD + LEFT_W + 16.0, PAD)
    };
    let rw = w - rx - PAD;
    let foot = |y: f32, h: f32| {
        if stack {
            y + h
        } else {
            left_foot.max(y + 60.0)
        }
    };
    let files = pad_of(pick);
    let mut title = Spot::new(
        Rect::new(rx, top, rw - CHOKE_W - 56.0, ROW_H),
        Mark::Title,
        xirius_params::PAD_NAMES[pick],
    );
    title.note = Some(((usize::from(play::FIRST_PAD) + pick) % 128) as u8);
    if let (Some(&first), Some(x)) = (files.first(), filled) {
        title.sub = stem(&x.samples[first]);
    }
    b.spots.push(title);
    // An empty pad, in an empty kit or a filled one, is its name and the plane a sample is
    // dropped on: no Choke, modes or voice, so choosing one writes none of its values.
    let Some(x) = filled.filter(|_| !files.is_empty()) else {
        let y = top + ROW_H + GAP;
        let r = Rect::new(rx, y, rw, foot(y, EMPTY_H) - y);
        drop_plane(&mut b, r);
        b.h = r.bottom() + PAD;
        return b;
    };
    b.names.push(name(
        Rect::new(rx + rw - CHOKE_W - 48.0, top + 6.0, 44.0, NAME_H),
        "Choke",
    ));
    b.slots.extend(slot(
        def,
        "choke",
        Look::Menu,
        ROW_H,
        Rect::new(rx + rw - CHOKE_W, top, CHOKE_W, ROW_H),
    ));
    // Layer Mode and Trigger, the second under the first when they do not fit.
    let mut y = top + ROW_H + GAP;
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
    let layer = s.layer.min(files.len() - 1);
    b.names.push(name(
        Rect::new(rx, y + (LAYER_H - NAME_H) / 2.0, 48.0, NAME_H),
        "Layers",
    ));
    let (lx, lw) = (rx + 52.0, rw - 52.0);
    let velocity = plain(def, s.values, "layer_mode").round() == 0.0;
    let ranges = velocities(def, x, &files);
    let shares: Vec<f32> = ranges
        .iter()
        .map(|&(lo, hi)| {
            if velocity {
                // More than 128 layers leave the last ones no velocity: no width, not an
                // overflow (low 18).
                (f32::from(hi) - f32::from(lo) + 1.0).max(0.0) / 128.0
            } else {
                1.0 / files.len() as f32
            }
        })
        .collect();
    let cells = widths(&shares, lw);
    let mut at = lx;
    let mut edges = Vec::new();
    for j in 0..files.len() {
        let (lo, hi) = ranges[j];
        let cw = if j + 1 == files.len() {
            lx + lw - at
        } else {
            cells[j].round()
        };
        // The mock's words: a velocity layer's range while there are two or more, else
        // its place.
        let text = if velocity && files.len() > 1 {
            format!("{lo}–{hi}")
        } else {
            (j + 1).to_string()
        };
        let mut spot = Spot::new(Rect::new(at, y, cw - 2.0, LAYER_H), Mark::Layer(j), text);
        spot.on = j == layer;
        b.spots.push(spot);
        at += cw;
        if velocity && j + 1 < files.len() {
            // Over the gap between this layer and the next, on top of both.
            let r = Rect::new(at - 1.0 - EDGE_HIT / 2.0, y, EDGE_HIT, LAYER_H);
            edges.push(Spot::new(r, Mark::Edge(j), hi.to_string()));
        }
    }
    b.spots.extend(edges);
    y += LAYER_H + GAP;
    y += sample_row(def, s.values, &mut b, (rx, y), rw, &x.samples[files[layer]]) + GAP;
    b.graphs
        .insert(0, Rect::new(rx, y, rw, foot(y, WAVE_H) - y));
    b.main = 0;
    let top = b.graphs[0].bottom().max(left_foot) + GAP + 4.0;
    let foot = voice(def, &mut b, PAD, top, w - 2.0 * PAD);
    b.h = foot + PAD;
    b
}

/// The file whose wave the window of `plugin` shows: the Sampler's zone's, the Drum Kit's
/// chosen layer's.
pub fn wave_file(plugin: &Plugin, pick: usize, layer: usize) -> Option<&str> {
    let i = wave_slot(plugin, pick, layer)?;
    plugin.samples.get(i).map(String::as_str)
}

/// Which of `plugin`'s samples its wave shows: see [`wave_file`].
pub fn wave_slot(plugin: &Plugin, pick: usize, layer: usize) -> Option<usize> {
    let def = plugin.builtin()?;
    match def.uid {
        "sampler" => {
            let n = plugin.samples.len();
            (n > 0).then(|| pick.min(n - 1))
        }
        "drum-kit" => {
            let files = pad_files(def, plugin, pick.min(PADS - 1));
            files.get(layer.min(files.len().saturating_sub(1))).copied()
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
    // The voice that sounds: a dot where it is on the envelope.
    if let Some(at) = view
        .live
        .and_then(|l| l.env)
        .filter(|_| !view.bypassed)
        .and_then(|v| env_dot(def, r, values, v))
    {
        let d = Rect::new(at.0 - 4.0, at.1 - 4.0, 8.0, 8.0);
        pen.round(d, c.ink_strong, Radius::Pill);
    }
}

/// Where a voice at `v` is on the envelope drawn on `r`, as the mock moves it: up the
/// attack by its level, along the decay and the release by their time, then slowly on along
/// the sustain while it is held. None once its release has ended.
pub fn env_dot(def: &BuiltinDef, r: Rect, values: &[f64], v: graph::Env) -> Option<(f32, f32)> {
    use xirius_engine::stock::VoiceStage;
    let (e, _) = env_shape(def, r, values)?;
    let i = inset(r);
    let y = i.bottom() - i.h * v.level.clamp(0.0, 1.0);
    let secs = |id: &str| (plain(def, values, id) / 1000.0).max(1e-3) as f32;
    let x = match v.stage {
        VoiceStage::Attack => i.x + v.level.clamp(0.0, 1.0) * (e.a.0 - i.x),
        VoiceStage::Decay => {
            let d = secs("decay");
            if v.secs < d {
                e.a.0 + v.secs / d * (e.d.0 - e.a.0)
            } else {
                e.d.0 + (1.0 - (-(v.secs - d) / 0.8).exp()) * (e.hold - e.d.0)
            }
        }
        VoiceStage::Release => {
            let t = v.secs / secs("release");
            if t >= 1.0 {
                return None;
            }
            e.hold + t * (e.r.0 - e.hold)
        }
        VoiceStage::Fade => return None,
    };
    Some((x, y))
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
    // Its type, then open: no filter at all (UI-333).
    if let Some(kt) = idx(def, "ftype") {
        let n = xirius_params::FILTER_TYPES.len();
        let now = plain(def, values, "ftype").round() as usize;
        p.menu = xirius_params::FILTER_TYPES
            .iter()
            .enumerate()
            .map(|(t, label)| {
                Some(Choice {
                    label: label.to_string(),
                    set: vec![(kt, super::eq::choice(n, t))],
                    on: t == now,
                })
            })
            .collect();
        p.menu.push(None);
        let open = |k: usize| {
            (
                k,
                control::snap(&def.params[k], def.params[k].default_norm()),
            )
        };
        p.menu.push(Some(Choice {
            label: "フィルターを開く（かけない）".to_string(),
            set: vec![(kt, 0.0), open(kc), open(kr)],
            on: false,
        }));
        p.rest = vec![kt];
    }
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

/// The wave's points: Start and End at its top, the loop's ends at its foot while it loops,
/// each where its tab is drawn ([`paint_handles`]).
pub fn wave_points(def: &BuiltinDef, r: Rect, values: &[f64]) -> Vec<Point> {
    if r.w <= 0.0 {
        return Vec::new();
    }
    let ax = share_axis(r);
    let mut out = Vec::new();
    let mut at = |id: &str, y: f32| {
        if let Some(k) = idx(def, id) {
            let mut p = point(def, values, k, (ax.px(plain(def, values, id)), y), "");
            p.x = Some(Drive { k, axis: ax });
            out.push(p);
        }
    };
    at("start", r.y + TAB.1 / 2.0);
    at("end", r.y + TAB.1 / 2.0);
    if on(def, values, "loop") {
        at("loop_start", r.bottom() - TAB.1 / 2.0);
        at("loop_end", r.bottom() - TAB.1 / 2.0);
    }
    out
}

/// The wave's points as the mock draws them: a 3 px bar down the plane and a tab at its top
/// (Start, End, in ink) or its foot (the loop's ends, in ink-blue); the one under the
/// pointer or held in ink-strong, the chosen one's bar in the focus ring's ink while the
/// keys move it.
pub fn paint_handles(pen: &mut Pen, plane: Rect, points: &[Point], view: &View) {
    let c = pen.c;
    for (i, p) in points.iter().enumerate() {
        let foot = p.at.1 > plane.y + plane.h / 2.0;
        let hot = view.hot == Some(i) || (view.held && view.chosen == Some(i));
        let tone = if view.bypassed {
            c.scroll_thumb[0]
        } else if hot {
            c.ink_strong
        } else if foot {
            c.ink_blue
        } else {
            c.ink
        };
        let bar = if view.focused && view.chosen == Some(i) {
            c.ring
        } else {
            tone
        };
        pen.round(
            Rect::new(p.at.0 - 1.5, plane.y, 3.0, plane.h),
            bar,
            Radius::Xs,
        );
        // Its tab, TAB (12 × 10).
        let ty = if foot {
            plane.bottom() - TAB.1
        } else {
            plane.y
        };
        pen.round(Rect::new(p.at.0 - 6.0, ty, 12.0, 10.0), tone, Radius::Clip);
    }
    let tip = view
        .hot
        .filter(|_| view.held)
        .or(view.hot)
        .and_then(|i| points.get(i));
    if let Some(p) = tip {
        graph::paint_tip(pen, plane, p);
    }
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
                let cut = cut_of(def, values);
                let half = i.h / 2.0;
                let mut top = Vec::with_capacity(cols);
                let mut bottom = Vec::with_capacity(cols);
                for n in 0..cols {
                    let m = wave_column(n, cols, cut);
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
        // Where the voice that sounds reads, while it is between Start and End.
        if let Some(read) = view.live.and_then(|l| l.read).filter(|_| !view.bypassed) {
            let at = f64::from(read) * 100.0;
            let (s, e) = (plain(def, values, "start"), plain(def, values, "end"));
            if (s.min(e) - 0.05..=s.max(e) + 0.05).contains(&at) {
                // A reversed file reads its cut from End back to Start: the cut shows
                // backwards in its place, and the line goes from Start to End over it, as on
                // the mock.
                let x = ax.px(wave_at(at / 100.0, cut_of(def, values)) * 100.0);
                pen.fill(Rect::new(x - 1.0, r.y, 2.0, r.h), c.ink_strong);
            }
        }
    });
}

/// Start and End as shares of the file, and whether it plays backwards.
fn cut_of(def: &BuiltinDef, values: &[f64]) -> (f64, f64, bool) {
    let (s, e) = (plain(def, values, "start"), plain(def, values, "end"));
    (
        s.min(e) / 100.0,
        s.max(e) / 100.0,
        on(def, values, "reverse"),
    )
}

/// Where along the wave the file's share `p` shows, and the other way: reversed, the cut
/// between Start and End shows backwards in its place, the rest as it is (low 1).
pub fn wave_at(p: f64, (s, e, reverse): (f64, f64, bool)) -> f64 {
    if reverse && (s..=e).contains(&p) {
        s + e - p
    } else {
        p
    }
}

/// The column of the file the `n`th of `cols` columns of the wave draws.
pub fn wave_column(n: usize, cols: usize, cut: (f64, f64, bool)) -> usize {
    let at = wave_at((n as f64 + 0.5) / cols as f64, cut);
    ((at * cols as f64) as usize).min(cols.saturating_sub(1))
}

// ------------------------------------------------------------------ the spots

/// The zones' strip over a Sampler's keys, as wide as the keys.
pub fn zone_strip(body: &Body) -> Option<Rect> {
    let zone = body
        .spots
        .iter()
        .find(|s| matches!(s.mark, Mark::Zone(_)))?;
    let (k0, k1) = body.keys.iter().fold((f32::MAX, f32::MIN), |(a, b), k| {
        (a.min(k.r.x), b.max(k.r.right()))
    });
    (k0 < k1).then(|| Rect::new(k0, zone.r.y, k1 - k0, zone.r.h))
}

/// The bar of a Drum Kit's chosen pad's layers.
pub fn layer_bar(body: &Body) -> Option<Rect> {
    let mut layers = body
        .spots
        .iter()
        .filter(|s| matches!(s.mark, Mark::Layer(_)));
    let first = layers.next()?;
    let right = layers.next_back().unwrap_or(first).r.right();
    let r = first.r;
    Some(Rect::new(r.x, r.y, right - r.x, r.h))
}

/// Where files dropped on a Sampler's or a Drum Kit's body go (the mock's targets).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropOn {
    /// The plane of a window with no samples.
    Plane,
    /// The zones' strip: the files are added to the Sampler's.
    Zones,
    /// The chosen pad's layers' bar: they are added to its layers.
    Layers,
    /// The rest of a Sampler with samples: they take the place of its samples.
    Samples,
}

/// What a drop at `(x, y)` on `body`, `inside` the window's content, does there, and the
/// place that lights while files are held over it. None where a Drum Kit's own plan says
/// it (its pads, its bank tabs).
pub fn drop_on(body: &Body, kit: bool, inside: Rect, x: f32, y: f32) -> Option<(DropOn, Rect)> {
    if let Some(s) = body.spots.iter().find(|s| s.mark == Mark::Drop) {
        return Some((DropOn::Plane, s.r));
    }
    if kit {
        return layer_bar(body)
            .filter(|r| r.contains(x, y))
            .map(|r| (DropOn::Layers, r));
    }
    match zone_strip(body) {
        Some(r) if r.contains(x, y) => Some((DropOn::Zones, r)),
        _ => Some((DropOn::Samples, inside)),
    }
}

/// The spots of a Sampler's or a Drum Kit's body: `hot` the one under the pointer.
pub fn paint_spots(pen: &mut Pen, body: &Body, hot: Option<usize>) {
    let c = pen.c;
    // The zones' strip over the keys and the layers' bar: the ground under their faces.
    for r in [zone_strip(body), layer_bar(body)].into_iter().flatten() {
        pen.round(r, c.surface_2, Radius::Clip);
    }
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
                    c.clip
                } else {
                    c.surface_3
                };
                pen.round(r, face, Radius::Clip);
                let ink = if s.on { c.on_fill } else { c.ink };
                let t = Rect::new(r.x + 6.0, r.y, (r.w - 8.0).max(0.0), r.h);
                let text = pen.fit(&s.text, FontSize::Micro, SPAN_WEIGHT, t.w);
                pen.text(&text, FontSize::Micro, SPAN_WEIGHT, ink, t, Align::Left);
            }
            Mark::Edge(_) => {
                let line = Rect::new(r.x + r.w / 2.0 - 1.0, r.y + (r.h - 18.0) / 2.0, 2.0, 18.0);
                let ink = if over || s.on { c.ring } else { c.ink_strong };
                pen.round(line, ink, Radius::Xs);
            }
            Mark::Bank(_) => {
                if s.on || over {
                    pen.round(r, c.surface_3, Radius::S);
                }
                let ink = if s.ghost {
                    c.ink_blue
                } else if s.on {
                    c.ink_strong
                } else {
                    c.ink
                };
                pen.text(
                    &s.text,
                    FontSize::Caption,
                    Weight::Semibold,
                    ink,
                    r,
                    Align::Center,
                );
                if s.dot {
                    let d = 4.0;
                    let dot = Rect::new(r.x + (r.w - d) / 2.0, r.bottom() - d - 3.0, d, d);
                    pen.round(dot, c.ink_blue, Radius::Pill);
                }
            }
            // A pad's face and words are the pads' own (`play::paint_pads`).
            Mark::Pad(_) => {}
            Mark::Mute(_) | Mark::Solo(_) => {
                let on = if matches!(s.mark, Mark::Solo(_)) {
                    c.solo_on
                } else {
                    c.mute_on
                };
                if s.on {
                    pen.round(r, on.0, Radius::Clip);
                } else if over {
                    pen.round(r, c.surface_1, Radius::Clip);
                }
                let ink = if s.on { on.1 } else { c.ink };
                pen.text(
                    &s.text,
                    FontSize::Micro,
                    Weight::Semibold,
                    ink,
                    r,
                    Align::Center,
                );
            }
            // The mock's `.dz`: s3, its icon ink-blue, while files are held over it.
            Mark::Drop => {
                let face = if s.ghost { c.surface_3 } else { c.surface_2 };
                pen.round(r, face, Radius::S);
                let d = 28.0;
                let cy = r.y + r.h / 2.0;
                pen.icon(
                    "download",
                    Rect::new(r.x + (r.w - d) / 2.0, cy - 56.0, d, d),
                    if s.ghost { c.ink_blue } else { c.ink },
                );
                pen.text(
                    &s.text,
                    FontSize::Small,
                    Weight::Regular,
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
            // The pad's name, its sample's, its note: the mock's `.dk-pn`.
            Mark::Title => {
                let w = pen.measure(&s.text, FontSize::Small, Weight::Semibold);
                pen.text(
                    &s.text,
                    FontSize::Small,
                    Weight::Semibold,
                    c.ink_strong,
                    Rect::new(r.x, r.y, w + 4.0, r.h),
                    Align::Left,
                );
                let mut x = r.x + w + 8.0;
                let note = s.note.map(crate::roll::pitch_name).unwrap_or_default();
                let nw = pen.measure(&note, FontSize::Caption, Weight::Regular);
                if !s.sub.is_empty() {
                    let room = (r.right() - x - nw - 8.0).max(0.0);
                    let sub = pen.fit(&s.sub, FontSize::Small, Weight::Regular, room);
                    let sw = pen.measure(&sub, FontSize::Small, Weight::Regular);
                    pen.text(
                        &sub,
                        FontSize::Small,
                        Weight::Regular,
                        c.ink_strong,
                        Rect::new(x, r.y, sw + 4.0, r.h),
                        Align::Left,
                    );
                    x += sw + 8.0;
                }
                pen.text(
                    &note,
                    FontSize::Caption,
                    Weight::Regular,
                    c.ink,
                    Rect::new(x, r.y, (r.right() - x).max(0.0), r.h),
                    Align::Left,
                );
            }
        }
        if let (Mark::Zone(_), Some(note)) = (s.mark, s.note.filter(|_| s.dot)) {
            // The root's dot over its key, kept in the zone (a root off the keys at their edge).
            if let Some(k) = body.keys.iter().find(|k| k.pitch == note.clamp(LOW, HIGH)) {
                let d = 5.0;
                let mid = (k.r.x + k.r.w / 2.0)
                    .clamp(r.x + d / 2.0, (r.right() - d / 2.0).max(r.x + d / 2.0));
                let dot = Rect::new(mid - d / 2.0, r.bottom() - d - 2.0, d, d);
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
