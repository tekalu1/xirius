//! The Pitch Correction body's placement and pitch keyboard (UI-359).

use xirius_design::{FontSize, Radius, Weight};
use xirius_params::{BuiltinDef, pitch_correction};

use super::control::{self, Look};
use super::layout::{Body, Name, PAD, Shape, Slot};
use super::play::Key;
use crate::draw::{Align, Pen};
use crate::studio::Rect;
use gpui::{PathBuilder, point, px};
use std::time::{Duration, Instant};

pub const NATURAL_W: f32 = 640.0;
pub const MIN_W: f32 = 320.0;

#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub at: Instant,
    pub input: f32,
    pub output: f32,
    pub target: i32,
    pub cents: f32,
}

/// The probe holds only its newest value; the window keeps a fixed four-second
/// history. Its timestamps are when the processed audio reaches the listener.
#[derive(Debug)]
pub struct History {
    points: [Option<Point>; 256],
    next: usize,
    axis_low: f32,
}

impl Default for History {
    fn default() -> Self {
        Self {
            points: [None; 256],
            next: 0,
            axis_low: 52.0,
        }
    }
}

impl History {
    pub fn feed(&mut self, view: (f32, f32, i32, f32), now: Instant, latency: Duration) {
        if view.0 > 0.0 {
            let note = midi(view.0);
            if note < self.axis_low + 1.0 || note > self.axis_low + 11.0 {
                // A new register starts a new trace, so old points cannot jump
                // when their screen coordinates change.
                self.points.fill(None);
                self.next = 0;
                self.axis_low = (note.round() - 6.0).clamp(0.0, 115.0);
            }
        }
        self.points[self.next] = Some(Point {
            at: now + latency,
            input: view.0,
            output: view.1,
            target: view.2,
            cents: view.3,
        });
        self.next = (self.next + 1) % self.points.len();
    }

    pub fn audible(&self, now: Instant) -> impl Iterator<Item = Point> + '_ {
        self.points.iter().filter_map(|p| *p).filter(move |p| {
            p.at <= now && now.duration_since(p.at) <= Duration::from_secs(4) && p.input > 0.0
        })
    }

    pub fn clear(&mut self) {
        self.points.fill(None);
        self.next = 0;
    }

    pub fn axis_low(&self) -> f32 {
        self.axis_low
    }
}

pub fn latency(rate: f64, range: f64) -> Duration {
    let low = match range.round() as u8 {
        0 => 60.0,
        2 => 200.0,
        _ => 100.0,
    };
    let max_tau = (rate / low).ceil() as u64;
    let samples = (5 * max_tau).div_ceil(2) + 1;
    Duration::from_secs_f64(samples as f64 / rate)
}

fn midi(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

pub fn note_of(hz: f32) -> u8 {
    midi(hz).round().clamp(0.0, 127.0) as u8
}

fn y_of(r: Rect, note: f32, axis_low: f32) -> f32 {
    (r.bottom() - (note - axis_low) / 12.0 * r.h).clamp(r.y, r.bottom())
}

fn segment(pen: &mut Pen, a: (f32, f32), b: (f32, f32), color: gpui::Hsla, width: f32) {
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(point(px(pen.origin.0 + a.0), px(pen.origin.1 + a.1)));
    path.line_to(point(px(pen.origin.0 + b.0), px(pen.origin.1 + b.1)));
    if let Ok(path) = path.build() {
        pen.window.paint_path(path, color);
    }
}

pub(crate) fn legend_items(graph: Rect) -> [(Rect, Rect); 3] {
    const SAMPLE_W: f32 = 11.0;
    const NAME_GAP: f32 = 4.0;
    const PAIR_GAP: f32 = 10.0;
    const NAME_W: [f32; 3] = [18.0, 24.0, 32.0];
    let mut x = graph.right() - 144.0;
    std::array::from_fn(|i| {
        let sample = Rect::new(x, graph.y - 19.0, SAMPLE_W, 16.0);
        let name = Rect::new(sample.right() + NAME_GAP, sample.y, NAME_W[i], 16.0);
        x = name.right() + PAIR_GAP;
        (sample, name)
    })
}

pub fn paint_graph(pen: &mut Pen, r: Rect, history: Option<&History>, now: Instant, midi_on: bool) {
    let c = pen.c;
    let [
        (in_sample, in_name),
        (out_sample, out_name),
        (midi_sample, midi_name),
    ] = legend_items(r);
    pen.fill(
        Rect::new(in_sample.x, in_sample.y + 8.0, in_sample.w, 1.0),
        c.ink_blue,
    );
    pen.text(
        "In",
        FontSize::Caption,
        Weight::Regular,
        c.ink_blue,
        in_name,
        Align::Left,
    );
    pen.fill(
        Rect::new(out_sample.x, out_sample.y + 7.0, out_sample.w, 3.0),
        c.ink_blue,
    );
    pen.text(
        "Out",
        FontSize::Caption,
        Weight::Regular,
        c.ink_blue,
        out_name,
        Align::Left,
    );
    if midi_on {
        pen.fill(
            Rect::new(midi_sample.x, midi_sample.y + 6.0, midi_sample.w, 5.0),
            c.ink_mark,
        );
        pen.text(
            "MIDI",
            FontSize::Caption,
            Weight::Regular,
            c.ink_mark,
            midi_name,
            Align::Left,
        );
    }
    pen.round(r, c.surface_0, Radius::S);
    let plot = Rect::new(r.x + 32.0, r.y + 7.0, r.w - 40.0, r.h - 14.0);
    let axis_low = history.map_or(52.0, History::axis_low);
    for row in 0..12 {
        let top = plot.y + row as f32 * plot.h / 12.0;
        if row % 2 == 1 {
            pen.fill(Rect::new(plot.x, top, plot.w, plot.h / 12.0), c.surface_1);
        }
    }
    for offset in 0..=12 {
        let note = (axis_low as u8).saturating_add(offset);
        let y = y_of(plot, note as f32, axis_low);
        let name = crate::roll::pitch_name(note);
        pen.text(
            &name,
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(r.x + 2.0, y - 6.0, 28.0, 12.0),
            Align::Right,
        );
    }
    let Some(history) = history else { return };
    let mut prior: Option<Point> = None;
    for k in 0..history.points.len() {
        let Some(p) = history.points[(history.next + k) % history.points.len()] else {
            continue;
        };
        if p.at > now || now.duration_since(p.at) > Duration::from_secs(4) {
            continue;
        }
        if p.input <= 0.0 {
            prior = None;
            continue;
        }
        let x = plot.right() - now.duration_since(p.at).as_secs_f32() / 4.0 * plot.w;
        if let Some(prev) = prior
            && p.at.duration_since(prev.at) < Duration::from_millis(150)
        {
            let old_x = plot.right() - now.duration_since(prev.at).as_secs_f32() / 4.0 * plot.w;
            segment(
                pen,
                (old_x, y_of(plot, midi(prev.input), axis_low)),
                (x, y_of(plot, midi(p.input), axis_low)),
                c.ink_blue,
                1.0,
            );
            segment(
                pen,
                (old_x, y_of(plot, midi(prev.output), axis_low)),
                (x, y_of(plot, midi(p.output), axis_low)),
                c.ink_blue,
                3.0,
            );
            if midi_on && prev.target >= 0 && p.target >= 0 {
                segment(
                    pen,
                    (old_x, y_of(plot, prev.target as f32, axis_low)),
                    (x, y_of(plot, p.target as f32, axis_low)),
                    c.ink_mark,
                    5.0,
                );
            }
        }
        prior = Some(p);
    }
    if let Some(p) = prior {
        let note = note_of(p.input);
        let name = crate::roll::pitch_name(note);
        pen.text(
            &name,
            FontSize::Body,
            Weight::Semibold,
            c.ink_strong,
            Rect::new(r.x, r.bottom() + 3.0, 42.0, 24.0),
            Align::Left,
        );
        let bar = Rect::new(r.x + 46.0, r.bottom() + 13.0, r.w - 140.0, 2.0);
        pen.fill(bar, c.line[0]);
        let cursor = ((p.cents + 50.0) / 100.0).clamp(0.0, 1.0);
        pen.round(
            Rect::new(bar.x + cursor * bar.w, r.bottom() + 8.0, 4.0, 12.0),
            c.ink_mark,
            Radius::S,
        );
        let cents = format!("{:+.0} cent", p.cents);
        pen.text(
            &cents,
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(r.right() - 90.0, r.bottom() + 3.0, 90.0, 24.0),
            Align::Right,
        );
    }
}

pub fn body(def: &BuiltinDef, s: &Shape<'_>) -> Body {
    let w = s.w.max(MIN_W);
    let narrow = w < 500.0;
    let graph_h = if narrow { 150.0 } else { 190.0 };
    let graph = Rect::new(PAD, PAD + 20.0, w - PAD * 2.0, graph_h);
    let picker_y = graph.bottom() + 38.0;
    let key_box_x = PAD + 25.0;
    let key_box_w = 70.0;
    let scale_label_x = key_box_x + key_box_w + 12.0;
    let scale_box_x = scale_label_x + 36.0;
    let scale_box_w = if narrow { 122.0 } else { 148.0 };
    let key_y = picker_y + 36.0;
    let keyboard_w = if narrow {
        w - PAD * 2.0
    } else {
        w * 0.58 - PAD
    };
    let white_w = keyboard_w / 7.0;
    let keys = (0..12)
        .map(|n| {
            let (column, black) = match n {
                0 => (0.0, false),
                1 => (0.5, true),
                2 => (1.0, false),
                3 => (1.5, true),
                4 => (2.0, false),
                5 => (3.0, false),
                6 => (3.5, true),
                7 => (4.0, false),
                8 => (4.5, true),
                9 => (5.0, false),
                10 => (5.5, true),
                _ => (6.0, false),
            };
            Key {
                pitch: n,
                r: Rect::new(
                    PAD + column * white_w,
                    key_y + if black { 0.0 } else { 28.0 },
                    white_w - 2.0,
                    26.0,
                ),
                black,
            }
        })
        .collect();
    let control_y = if narrow { key_y + 68.0 } else { picker_y };
    let knob_x = if narrow { PAD } else { w * 0.62 };
    let knob_w = (w - knob_x - PAD - 16.0) / 3.0;
    let bottom_y = control_y + 76.0;
    let slots = def
        .params
        .iter()
        .enumerate()
        .map(|(k, spec)| {
            let (cell, look) = match k {
                0 => (Rect::new(key_box_x, picker_y, key_box_w, 26.0), Look::Menu),
                1 => (
                    Rect::new(scale_box_x, picker_y, scale_box_w, 26.0),
                    Look::Menu,
                ),
                2..=4 => (
                    Rect::new(
                        knob_x + (k - 2) as f32 * (knob_w + 8.0),
                        control_y,
                        knob_w,
                        64.0,
                    ),
                    Look::Knob,
                ),
                5 => (Rect::new(knob_x, bottom_y, 74.0, 28.0), Look::Toggle),
                _ => (
                    Rect::new(
                        knob_x + 112.0,
                        bottom_y,
                        (w - PAD - knob_x - 112.0).max(120.0),
                        28.0,
                    ),
                    control::look(spec),
                ),
            };
            Slot {
                k,
                look,
                dial: if k <= 1 || k >= 5 { 16.0 } else { 30.0 },
                cell,
                label: None,
                dim: false,
            }
        })
        .collect();
    Body {
        w,
        h: bottom_y + 36.0,
        graphs: vec![graph],
        names: vec![
            Name {
                r: Rect::new(PAD, PAD, w - PAD * 2.0, 18.0),
                text: "Pitch",
                size: FontSize::Small,
                strong: true,
                dim: false,
            },
            Name {
                r: Rect::new(PAD, picker_y, 24.0, 26.0),
                text: "Key",
                size: FontSize::Small,
                strong: false,
                dim: false,
            },
            Name {
                r: Rect::new(scale_label_x, picker_y, 34.0, 26.0),
                text: "Scale",
                size: FontSize::Small,
                strong: false,
                dim: false,
            },
            Name {
                r: Rect::new(knob_x + 76.0, bottom_y, 36.0, 28.0),
                text: "Range",
                size: FontSize::Small,
                strong: false,
                dim: false,
            },
        ],
        slots,
        keys,
        ..Body::default()
    }
}

pub fn lit(scale: f64, key: u8, pitch: u8) -> bool {
    pitch_correction::scale_mask(scale) & (1 << ((pitch + 12 - key) % 12)) != 0
}

/// The scale written after a key is switched. A restored preset uses its
/// canonical value, so the existing choice button says its name again.
pub fn toggle_scale(scale: f64, relative: u8) -> f64 {
    let next = pitch_correction::toggle_relative(scale, relative);
    match pitch_correction::scale_mask(next) {
        pitch_correction::CHROMATIC => 0.0,
        pitch_correction::MAJOR => 1.0 / 3.0,
        pitch_correction::MINOR => 2.0 / 3.0,
        _ => next,
    }
}

pub fn paint_keys(pen: &mut Pen, keys: &[Key], scale: f64, root: u8, target: Option<u8>) {
    let labels = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    for key in keys {
        let on = lit(scale, root, key.pitch);
        let face = if on { pen.c.fill } else { pen.c.surface_2 };
        pen.round(key.r, face, Radius::S);
        pen.text(
            labels[usize::from(key.pitch)],
            FontSize::Caption,
            Weight::Medium,
            if on { pen.c.on_fill } else { pen.c.ink },
            key.r,
            Align::Center,
        );
        if target.is_some_and(|n| n % 12 == key.pitch) {
            pen.round(
                Rect::new(key.r.right() - 8.0, key.r.y + 3.0, 4.0, 4.0),
                if on { pen.c.on_fill } else { pen.c.ink },
                Radius::Pill,
            );
        }
    }
}
