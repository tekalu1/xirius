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
}

impl Default for History {
    fn default() -> Self {
        Self {
            points: [None; 256],
            next: 0,
        }
    }
}

impl History {
    pub fn feed(&mut self, view: (f32, f32, i32, f32), now: Instant, latency: Duration) {
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

fn y_of(r: Rect, note: f32) -> f32 {
    r.bottom() - (note - 52.0) / 12.0 * r.h
}

fn segment(pen: &mut Pen, a: (f32, f32), b: (f32, f32), color: gpui::Hsla) {
    let mut path = PathBuilder::stroke(px(1.5));
    path.move_to(point(px(pen.origin.0 + a.0), px(pen.origin.1 + a.1)));
    path.line_to(point(px(pen.origin.0 + b.0), px(pen.origin.1 + b.1)));
    if let Ok(path) = path.build() {
        pen.window.paint_path(path, color);
    }
}

pub fn paint_graph(pen: &mut Pen, r: Rect, history: Option<&History>, now: Instant, midi_on: bool) {
    let c = pen.c;
    let legend = Rect::new(r.right() - 106.0, r.y - 19.0, 32.0, 16.0);
    pen.text(
        "In",
        FontSize::Caption,
        Weight::Regular,
        c.ink_blue,
        legend,
        Align::Right,
    );
    pen.text(
        "Out",
        FontSize::Caption,
        Weight::Regular,
        c.ink_mark,
        Rect::new(legend.x + 36.0, legend.y, 32.0, 16.0),
        Align::Right,
    );
    if midi_on {
        pen.text(
            "MIDI",
            FontSize::Caption,
            Weight::Regular,
            c.ink_ai,
            Rect::new(legend.x + 66.0, legend.y, 40.0, 16.0),
            Align::Right,
        );
    }
    pen.round(r, c.surface_0, Radius::S);
    let plot = Rect::new(r.x + 32.0, r.y + 7.0, r.w - 40.0, r.h - 14.0);
    for note in 52..=64 {
        let y = y_of(plot, note as f32);
        pen.fill(Rect::new(plot.x, y, plot.w, 1.0), c.line[0]);
        let name = crate::roll::pitch_name(note as u8);
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
                (old_x, y_of(plot, midi(prev.input))),
                (x, y_of(plot, midi(p.input))),
                c.ink_blue,
            );
            segment(
                pen,
                (old_x, y_of(plot, midi(prev.output))),
                (x, y_of(plot, midi(p.output))),
                c.ink_mark,
            );
            if midi_on && prev.target >= 0 && p.target >= 0 {
                segment(
                    pen,
                    (old_x, y_of(plot, prev.target as f32)),
                    (x, y_of(plot, p.target as f32)),
                    c.ink_ai,
                );
            }
        }
        prior = Some(p);
    }
    if let Some(p) = prior {
        let note = midi(p.input).round().clamp(0.0, 127.0) as u8;
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
    let picker_w = if narrow { 100.0 } else { 112.0 };
    let key_y = picker_y + 46.0;
    let white_w = (w - PAD * 2.0) / 7.0;
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
    let bottom_y = if narrow {
        control_y + 76.0
    } else {
        key_y + 64.0
    };
    let slots = def
        .params
        .iter()
        .enumerate()
        .map(|(k, spec)| {
            let (cell, look) = match k {
                0 | 1 => (
                    Rect::new(PAD + k as f32 * (picker_w + 8.0), picker_y, picker_w, 40.0),
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
                5 => (
                    Rect::new(PAD, bottom_y, (w - PAD * 2.0) / 3.0, 40.0),
                    Look::Toggle,
                ),
                _ => (
                    Rect::new(
                        PAD + (w - PAD * 2.0) / 3.0,
                        bottom_y,
                        (w - PAD * 2.0) * 2.0 / 3.0,
                        40.0,
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
        h: bottom_y + 48.0,
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
                r: Rect::new(PAD, picker_y - 14.0, picker_w, 14.0),
                text: "Key",
                size: FontSize::Small,
                strong: false,
                dim: false,
            },
            Name {
                r: Rect::new(PAD + picker_w + 8.0, picker_y - 14.0, picker_w, 14.0),
                text: "Scale",
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

pub fn paint_keys(pen: &mut Pen, keys: &[Key], scale: f64, root: u8) {
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
    }
}
