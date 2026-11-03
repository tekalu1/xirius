//! The controls of a built-in device's window (UI-324): a knob, a row of
//! segments for a choice of five or fewer, a button for on and off. Each one
//! stands for one parameter (0..1 as `xirius-params` keeps it) and moves the
//! same ways: a knob by a vertical drag (Shift turns it slower), the wheel, a
//! typed value, Alt+press or Delete back to the default, and the keys while it
//! has the focus. The names inside the window are the ones Studio Pro's own
//! devices use (English), as the approved mock writes them.

use xirius_design::{FontSize, Radius, Weight};
use xirius_params::{BuiltinDef, Curve, ParamSpec};

use crate::draw::{Align, Pen};
use crate::settings::field::Field;
use crate::studio::Rect;

/// How far the pointer goes for a knob's whole sweep.
pub const TRAVEL: f32 = 150.0;
/// How much slower a knob turns with Shift.
pub const FINE: f32 = 0.25;

/// The dial of a knob: a main one, the output's, a small one.
pub const DIAL: f32 = 40.0;
pub const DIAL_OUT: f32 = 30.0;
pub const DIAL_SM: f32 = 28.0;
/// The value's line and the name's line under a dial.
pub const VALUE_H: f32 = 16.0;
pub const NAME_H: f32 = 14.0;
/// A row of segments, and one segment's least width.
pub const SEG_H: f32 = 22.0;
const SEG_MIN: f32 = 26.0;
const SEG_PAD: f32 = 8.0;
/// The on and off button.
pub const TOGGLE_H: f32 = 26.0;
const TOGGLE_PAD: f32 = 10.0;
/// The field a value is typed in.
pub const ENTRY_W: f32 = 64.0;
/// The dot of an on button.
pub const DOT: f32 = 10.0;

/// What a parameter looks like in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Knob,
    /// A choice of five or fewer.
    Segments,
    /// On and off.
    Toggle,
    /// On and off as a dot (a band's head: blue while on).
    Dot,
    /// A choice that says its value and opens a menu of the others (a
    /// band's type).
    Menu,
    /// A value in a box, typed or dragged like a knob (the Delay's time).
    Field,
}

pub fn look(spec: &ParamSpec) -> Look {
    match spec.curve {
        Curve::Toggle => Look::Toggle,
        Curve::Choice(labels) if labels.len() <= 5 => Look::Segments,
        _ => Look::Knob,
    }
}

/// The name a parameter has inside the window and for the screen reader:
/// Studio Pro's (Pro EQ's band names, the Compressor's, ...). One a device
/// does not name here keeps its own label.
pub fn name(def: &BuiltinDef, spec: &ParamSpec) -> &'static str {
    let own = match (def.uid, spec.id) {
        ("eq", "low_cut_on") => "Low Cut",
        ("eq", "low_cut_freq") => "LC Freq",
        ("eq", "low_cut_slope") => "LC Slope",
        ("eq", "low_freq") => "LF Freq",
        ("eq", "low_gain") => "LF Gain",
        ("eq", "mid1_freq") => "LMF Freq",
        ("eq", "mid1_gain") => "LMF Gain",
        ("eq", "mid1_q") => "LMF Q",
        ("eq", "mid2_freq") => "HMF Freq",
        ("eq", "mid2_gain") => "HMF Gain",
        ("eq", "mid2_q") => "HMF Q",
        ("eq", "high_freq") => "HF Freq",
        ("eq", "high_gain") => "HF Gain",
        ("eq", "high_cut_on") => "High Cut",
        ("eq", "high_cut_freq") => "HC Freq",
        ("eq", "high_cut_slope") => "HC Slope",
        ("eq", "b1_type") => "LC Type",
        ("eq", "b1_gain") => "LC Gain",
        ("eq", "b1_q") => "LC Q",
        ("eq", "b2_on") => "LF On",
        ("eq", "b2_type") => "LF Type",
        ("eq", "b2_q") => "LF Q",
        ("eq", "b2_slope") => "LF Slope",
        ("eq", "b3_on") => "LMF On",
        ("eq", "b3_type") => "LMF Type",
        ("eq", "b3_slope") => "LMF Slope",
        ("eq", "b4_on") => "HMF On",
        ("eq", "b4_type") => "HMF Type",
        ("eq", "b4_slope") => "HMF Slope",
        ("eq", "b5_on") => "HF On",
        ("eq", "b5_type") => "HF Type",
        ("eq", "b5_q") => "HF Q",
        ("eq", "b5_slope") => "HF Slope",
        ("eq", "b6_type") => "HC Type",
        ("eq", "b6_gain") => "HC Gain",
        ("eq", "b6_q") => "HC Q",
        ("compressor", "makeup") => "Makeup",
        ("limiter", "input") => "Gain",
        ("delay", "division") => "Beats",
        ("delay", "ping_pong") => "Ping-Pong",
        ("reverb", "predelay") => "Pre-delay",
        ("sampler", "resonance") => "Res",
        ("sampler", "tune") => "Transpose",
        ("sampler", "fine") => "Tune",
        ("drum-kit", "hats") => "Hi-Hat",
        ("drum-kit", "percussion") => "Perc",
        _ => "",
    };
    if !own.is_empty() {
        return own;
    }
    match spec.id {
        "output" => "Output",
        "threshold" => "Threshold",
        "ratio" => "Ratio",
        "attack" => "Attack",
        "decay" => "Decay",
        "sustain" => "Sustain",
        "release" => "Release",
        "knee" => "Knee",
        "mix" => "Mix",
        "sync" => "Sync",
        "time" => "Time",
        "feel" => "Feel",
        "feedback" => "Feedback",
        "low_cut" => "Low Cut",
        "high_cut" => "High Cut",
        "size" => "Size",
        "damping" => "Damping",
        "early" => "Early",
        "width" => "Width",
        "ceiling" => "Ceiling",
        "sound" => "Sound",
        "cutoff" => "Cutoff",
        "velocity" => "Velocity",
        "volume" => "Volume",
        "kit" => "Kit",
        "tune" => "Tune",
        "kick" => "Kick",
        "snare" => "Snare",
        "toms" => "Toms",
        "cymbals" => "Cymbals",
        _ => spec.label,
    }
}

/// The labels of a choice, as its segments say them: the first word
/// (`12 dB/oct` reads `12`).
pub fn segment_labels(spec: &ParamSpec) -> Vec<&'static str> {
    match spec.curve {
        Curve::Choice(labels) => labels
            .iter()
            .map(|l| l.split(' ').next().unwrap_or(l))
            .collect(),
        _ => Vec::new(),
    }
}

/// A value on a parameter's own steps (a choice, a whole number, on/off).
pub fn snap(spec: &ParamSpec, norm: f64) -> f64 {
    let n = norm.clamp(0.0, 1.0);
    match spec.steps() {
        Some(_) => spec.to_norm(spec.to_plain(n)),
        None => (n * 10_000.0).round() / 10_000.0,
    }
}

/// A value as it reads (`-12.0 dB`, `1.20 kHz`, `付点`).
pub fn text(spec: &ParamSpec, norm: f64) -> String {
    spec.text(spec.to_plain(norm))
}

/// Where a knob's arc lights from: the middle of a range across zero (a gain,
/// a tune), else its start.
pub fn from(spec: &ParamSpec) -> f32 {
    if spec.min < 0.0 && spec.max > 0.0 && !matches!(spec.curve, Curve::Log) {
        spec.to_norm(0.0) as f32
    } else {
        0.0
    }
}

/// One step of a parameter with steps, as 0..1.
fn one_step(spec: &ParamSpec) -> Option<f64> {
    spec.steps()
        .filter(|n| *n > 1)
        .map(|n| 1.0 / f64::from(n - 1))
}

/// How far a key moves a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// An arrow with Shift.
    Fine,
    /// An arrow.
    Small,
    /// PgUp, PgDn.
    Large,
}

/// The value `dir` steps (±1) of `by` away, on the parameter's own steps.
pub fn step(spec: &ParamSpec, v: f64, dir: f64, by: Step) -> f64 {
    let d = match (one_step(spec), by) {
        (Some(s), _) => s,
        (None, Step::Fine) => 0.002,
        (None, Step::Small) => 0.01,
        (None, Step::Large) => 0.1,
    };
    snap(spec, v + dir * d)
}

/// The value after `notches` of the wheel (up is positive): one step of a
/// parameter with steps, else 0.02 (0.002 with Shift) a notch.
pub fn wheel(spec: &ParamSpec, v: f64, notches: f64, fine: bool) -> f64 {
    let d = match one_step(spec) {
        Some(s) => s * notches.signum(),
        None if fine => 0.002 * notches,
        None => 0.02 * notches,
    };
    snap(spec, v + d)
}

/// The value a drag of `dy` (down is positive) from `start` turns a knob to.
pub fn dragged(spec: &ParamSpec, start: f64, dy: f32, fine: bool) -> f64 {
    let scale = if fine { FINE } else { 1.0 };
    snap(spec, start - f64::from(dy / TRAVEL * scale))
}

/// About how wide a label is at the size the controls use, for laying them
/// out before there is a pen (the painter measures for real).
pub fn label_w(s: &str) -> f32 {
    s.chars()
        .map(|c| if c.is_ascii() { 6.2 } else { 11.0 })
        .sum()
}

fn segment_want(labels: &[&str]) -> Vec<f32> {
    labels
        .iter()
        .map(|l| (label_w(l) + SEG_PAD * 2.0).max(SEG_MIN))
        .collect()
}

/// The width a row of segments wants.
pub fn segments_w(spec: &ParamSpec) -> f32 {
    let labels = segment_labels(spec);
    let gaps = labels.len().saturating_sub(1) as f32 * 2.0;
    segment_want(&labels).iter().sum::<f32>() + 4.0 + gaps
}

/// The width the on and off button wants.
pub fn toggle_w(label: &str) -> f32 {
    label_w(label) + TOGGLE_PAD * 2.0
}

/// The segments of a row in `r`: each as wide as its label wants, the row's
/// spare width shared out.
pub fn segment_rects(r: Rect, spec: &ParamSpec) -> Vec<Rect> {
    let labels = segment_labels(spec);
    if labels.is_empty() {
        return Vec::new();
    }
    let want = segment_want(&labels);
    let inner = r.w - 4.0 - (labels.len() - 1) as f32 * 2.0;
    let total: f32 = want.iter().sum();
    let k = if total > 0.0 { inner / total } else { 1.0 };
    let mut x = r.x + 2.0;
    want.iter()
        .map(|w| {
            let s = Rect::new(x, r.y + 2.0, w * k, r.h - 4.0);
            x += w * k + 2.0;
            s
        })
        .collect()
}

/// The segment under `x`, as a value (0..1).
pub fn segment_value(r: Rect, spec: &ParamSpec, x: f32) -> Option<f64> {
    let rects = segment_rects(r, spec);
    let last = rects.len().checked_sub(1)?;
    let i = rects
        .iter()
        .position(|s| x < s.x + s.w + 1.0)
        .unwrap_or(last);
    Some(if last == 0 {
        0.0
    } else {
        i as f64 / last as f64
    })
}

/// A knob: its dial, its value (or the field it is typed in) and its name.
pub struct KnobView<'a> {
    pub spec: &'a ParamSpec,
    pub name: &'a str,
    pub value: f64,
    pub dial: f32,
    pub entry: Option<&'a Field>,
    pub focused: bool,
    /// What it shapes is off (an EQ's band): its value is the plain ink.
    pub dim: bool,
}

/// The dial of a knob in its cell: centred at the top.
pub fn dial_rect(cell: Rect, dial: f32) -> Rect {
    Rect::new(cell.x + (cell.w - dial) / 2.0, cell.y, dial, dial)
}

/// The line a knob's value reads on, under its dial.
pub fn value_rect(cell: Rect, dial: f32) -> Rect {
    Rect::new(cell.x, cell.y + dial + 2.0, cell.w, VALUE_H)
}

pub fn paint_knob(pen: &mut Pen, cell: Rect, k: &KnobView) {
    let c = pen.c;
    let dial = dial_rect(cell, k.dial);
    crate::mixer::paint::knob(pen, dial, k.value as f32, from(k.spec), 3.0, true);
    if k.focused {
        let ring = Rect::new(dial.x - 2.0, dial.y - 2.0, dial.w + 4.0, dial.h + 4.0);
        pen.outline(ring, c.ring, Radius::Pill, 2.0);
    }
    let value = value_rect(cell, k.dial);
    match k.entry {
        Some(field) => paint_entry(pen, value, field),
        None => {
            pen.text(
                &text(k.spec, k.value),
                FontSize::Micro,
                Weight::Regular,
                if k.dim { c.ink } else { c.ink_strong },
                value,
                Align::Center,
            );
        }
    }
    paint_name(pen, cell, k.dial, k.name);
}

/// A control's name under its dial's value line.
pub fn paint_name(pen: &mut Pen, cell: Rect, dial: f32, name: &str) {
    let ink = pen.c.ink;
    let label = Rect::new(cell.x, cell.y + dial + 2.0 + VALUE_H, cell.w, NAME_H);
    pen.mask(label, |pen| {
        pen.text(
            name,
            FontSize::Micro,
            Weight::Regular,
            ink,
            label,
            Align::Center,
        );
    });
}

/// The field a value is typed in, centred on the line its value reads on.
pub fn entry_rect(value_line: Rect) -> Rect {
    let w = ENTRY_W.min(value_line.w);
    Rect::new(
        value_line.x + (value_line.w - w) / 2.0,
        value_line.y,
        w,
        VALUE_H,
    )
}

pub fn paint_entry(pen: &mut Pen, value_line: Rect, field: &Field) {
    let c = pen.c;
    let r = entry_rect(value_line);
    pen.round(r, c.surface_3, Radius::Clip);
    pen.outline(r, c.ring, Radius::Clip, 1.0);
    pen.mask(r, |pen| {
        pen.text(
            &field.text(),
            FontSize::Micro,
            Weight::Regular,
            c.ink_strong,
            r,
            Align::Center,
        );
    });
}

/// A row of segments: the chosen one filled.
pub fn paint_segments(pen: &mut Pen, r: Rect, spec: &ParamSpec, value: f64, focused: bool) {
    let c = pen.c;
    pen.round(r, c.surface_3, Radius::S);
    let labels = segment_labels(spec);
    let last = labels.len().saturating_sub(1);
    let chosen = (value * last as f64).round() as usize;
    for (i, (s, l)) in segment_rects(r, spec).into_iter().zip(labels).enumerate() {
        let ink = if i == chosen {
            pen.round(s, c.fill, Radius::S);
            c.on_fill
        } else {
            c.ink_strong
        };
        pen.mask(s, |pen| {
            pen.text(l, FontSize::Caption, Weight::Regular, ink, s, Align::Center);
        });
    }
    if focused {
        pen.outline(r, c.ring, Radius::S, 2.0);
    }
}

/// The on and off button: filled while on, its name inside.
pub fn paint_toggle(pen: &mut Pen, r: Rect, label: &str, on: bool, focused: bool) {
    let c = pen.c;
    let (face, ink) = if on {
        (c.fill, c.on_fill)
    } else {
        (c.surface_3, c.ink_strong)
    };
    pen.round(r, face, Radius::S);
    pen.mask(r, |pen| {
        pen.text(
            label,
            FontSize::Caption,
            Weight::Regular,
            ink,
            r,
            Align::Center,
        );
    });
    if focused {
        pen.outline(r, c.ring, Radius::S, 2.0);
    }
}

/// The dot of a band's on (UI-326) in its round button `r`: blue while on.
pub fn paint_dot(pen: &mut Pen, r: Rect, on: bool, hot: bool, focused: bool) {
    let c = pen.c;
    if hot {
        pen.round(r, c.surface_3, Radius::Pill);
    }
    let dot = Rect::new(r.x + (r.w - DOT) / 2.0, r.y + (r.h - DOT) / 2.0, DOT, DOT);
    let dot_face = if on { c.ink_blue } else { c.scroll_thumb[0] };
    pen.round(dot, dot_face, Radius::Pill);
    if focused {
        pen.outline(r, c.ring, Radius::Pill, 2.0);
    }
}

/// A choice's button that opens its menu (a band's type): its label to the
/// right, cut short when it does not fit.
pub fn paint_choice(pen: &mut Pen, r: Rect, label: &str, hot: bool, focused: bool) {
    let c = pen.c;
    if hot {
        pen.round(r, c.surface_3, Radius::S);
    }
    let ink = if hot { c.ink_strong } else { c.ink };
    let t = Rect::new(r.x + 4.0, r.y, (r.w - 8.0).max(0.0), r.h);
    pen.mask(t, |pen| {
        pen.text(
            label,
            FontSize::Caption,
            Weight::Regular,
            ink,
            t,
            Align::Right,
        );
    });
    if focused {
        pen.outline(r, c.ring, Radius::S, 2.0);
    }
}

/// What a choice's menu button says: a band's type, the Delay's beats
/// (`1/8 D`). `values` are the device's (0..1).
pub fn menu_text(def: &BuiltinDef, k: usize, values: &[f64]) -> String {
    if def.uid == "delay" {
        return super::delay::beats_text(def, values);
    }
    let spec = &def.params[k];
    let v = values.get(k).copied().unwrap_or(spec.default_norm());
    super::eq::band_of(def, k).map_or(String::new(), |b| {
        xirius_params::EQ_BANDS[b]
            .type_of(spec.to_plain(v))
            .label()
            .to_string()
    })
}

/// A choice's box that opens its menu (the Delay's beats): its label in the
/// middle, a chevron at the right.
pub fn paint_box(pen: &mut Pen, r: Rect, label: &str, hot: bool, focused: bool) {
    let c = pen.c;
    pen.round(r, if hot { c.surface_1 } else { c.surface_3 }, Radius::S);
    let chevron = Rect::new(r.right() - 20.0, r.y + (r.h - 12.0) / 2.0, 12.0, 12.0);
    let t = Rect::new(r.x + 6.0, r.y, (r.w - 28.0).max(0.0), r.h);
    pen.mask(t, |pen| {
        pen.text(
            label,
            FontSize::Caption,
            Weight::Regular,
            c.ink_strong,
            t,
            Align::Center,
        );
    });
    pen.icon("chevron-down", chevron, c.ink);
    if focused {
        pen.outline(r, c.ring, Radius::S, 2.0);
    }
}

/// A value's box (the Delay's time): the value in the middle, or what is
/// being typed in it with its ring.
pub fn paint_field(
    pen: &mut Pen,
    r: Rect,
    text: &str,
    entry: Option<&Field>,
    hot: bool,
    focused: bool,
) {
    let c = pen.c;
    pen.round(r, if hot { c.surface_1 } else { c.surface_3 }, Radius::S);
    let shown = entry.map_or_else(|| text.to_string(), |f| f.text());
    pen.mask(r, |pen| {
        pen.text(
            &shown,
            FontSize::Caption,
            Weight::Regular,
            c.ink_strong,
            r,
            Align::Center,
        );
    });
    if entry.is_some() {
        pen.outline(r, c.ring, Radius::S, 1.0);
    } else if focused {
        pen.outline(r, c.ring, Radius::S, 2.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(uid: &str, id: &str) -> (&'static BuiltinDef, &'static ParamSpec) {
        let def = xirius_params::find(uid).unwrap();
        (def, def.param(id).unwrap().1)
    }

    #[test]
    fn ui_324_a_short_choice_is_segments_and_on_off_is_a_button() {
        assert_eq!(look(spec("eq", "low_cut_slope").1), Look::Segments);
        assert_eq!(look(spec("eq", "low_cut_on").1), Look::Toggle);
        assert_eq!(look(spec("eq", "low_freq").1), Look::Knob);
        // Six notes are too many for a row: a knob with steps.
        assert_eq!(look(spec("delay", "division").1), Look::Knob);
        assert_eq!(
            segment_labels(spec("eq", "low_cut_slope").1),
            ["12", "24", "48"]
        );
    }

    #[test]
    fn ui_324_the_wheel_and_the_keys_move_a_stepped_value_a_step() {
        let (_, division) = spec("delay", "division");
        let one = 1.0 / 5.0;
        assert!((wheel(division, 0.0, 1.0, false) - one).abs() < 1e-9);
        assert!(
            (wheel(division, 0.0, 3.0, false) - one).abs() < 1e-9,
            "a step a notch"
        );
        assert!((step(division, one, -1.0, Step::Large) - 0.0).abs() < 1e-9);
        let (_, gain) = spec("eq", "low_gain");
        assert!((wheel(gain, 0.5, 1.0, false) - 0.52).abs() < 1e-9);
        assert!((wheel(gain, 0.5, 1.0, true) - 0.502).abs() < 1e-9);
        assert!((step(gain, 0.5, 1.0, Step::Small) - 0.51).abs() < 1e-9);
        assert!((step(gain, 0.5, -1.0, Step::Large) - 0.4).abs() < 1e-9);
        assert_eq!(step(gain, 0.99, 1.0, Step::Large), 1.0);
    }

    #[test]
    fn ui_324_a_drag_of_150_is_the_whole_sweep_and_shift_a_quarter() {
        let (_, gain) = spec("eq", "low_gain");
        assert_eq!(dragged(gain, 0.0, -150.0, false), 1.0);
        assert!((dragged(gain, 0.5, -60.0, true) - 0.6).abs() < 1e-9);
    }

    #[test]
    fn ui_324_a_press_on_a_segment_chooses_it() {
        let (_, slope) = spec("eq", "low_cut_slope");
        let r = Rect::new(0.0, 0.0, segments_w(slope), SEG_H);
        let rects = segment_rects(r, slope);
        assert_eq!(rects.len(), 3);
        for (i, s) in rects.iter().enumerate() {
            let v = segment_value(r, slope, s.x + s.w / 2.0).unwrap();
            assert!((v - i as f64 / 2.0).abs() < 1e-9);
        }
        assert!(rects[2].x + rects[2].w <= r.w - 1.9);
    }

    #[test]
    fn ui_324_every_parameter_has_an_english_name_in_the_window() {
        for def in xirius_params::BUILTINS {
            for spec in def.params {
                let n = name(def, spec);
                assert!(!n.is_empty());
                assert!(n.is_ascii(), "{} {}: {n}", def.uid, spec.id);
            }
        }
    }
}
