//! What a dynamics device's window shows of its sound (UI-327, UI-328): the
//! input's and the output's meters, the gain reduction's, the most each
//! reached since it was last cleared, and the last few seconds of the three
//! as a history. The probe (ENG-072) hands the peaks and the reduction over
//! each frame; a sample of the history is taken every [`STEP`].

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use xirius_design::{FontSize, Radius, Weight};
use xirius_engine::mixer::MeterReading;

use super::graph::{self, Axis};
use crate::draw::{Align, Pen};
use crate::mixer::meters::Meter;
use crate::studio::Rect;

/// How often the history takes a sample.
pub const STEP: Duration = Duration::from_millis(25);
/// The bottom of the history's and the transfer curve's scale, in dB.
pub const FLOOR: f64 = -60.0;
/// The reduction the meter and the history's line span, in dB.
pub const GR_SPAN: f32 = 24.0;

/// The dB of a linear level, the floor for silence.
pub fn db(linear: f32) -> f32 {
    if linear > 0.0 {
        (20.0 * linear.log10()).max(FLOOR as f32)
    } else {
        FLOOR as f32
    }
}

/// One sample of the history: the input's and the output's peaks (dB) and
/// the most reduction (dB, 0 or more) over its step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub input: f32,
    pub output: f32,
    pub gr: f32,
}

/// The levels a dynamics device's window shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dynamics {
    pub input: Meter,
    pub output: Meter,
    /// The input's peak (dB) and the reduction (dB) of the last frame: the
    /// dot on the transfer curve and the reduction's meter.
    pub now: (f32, f32),
    /// The most since each was cleared: the input's and the output's peaks
    /// (linear) and the reduction (dB).
    pub most: [f32; 3],
    /// The last samples, the oldest first.
    pub history: VecDeque<Sample>,
    /// The step being gathered: the most of each since its start.
    pending: Option<Sample>,
    last: Option<Instant>,
}

impl Dynamics {
    /// Takes a frame of the probe: the peaks since the last frame
    /// (`[left, right]`, linear) and the reduction (dB). The history keeps
    /// `span` of samples.
    pub fn feed(&mut self, peaks: ([f32; 2], [f32; 2]), gr: f32, now: Instant, span: Duration) {
        let (input, output) = peaks;
        let reading = |p: [f32; 2]| MeterReading {
            peak: p,
            rms: p.map(|v| v * std::f32::consts::FRAC_1_SQRT_2),
        };
        self.input.step(reading(input));
        self.output.step(reading(output));
        let (li, lo) = (input[0].max(input[1]), output[0].max(output[1]));
        let gr = gr.max(0.0);
        self.now = (db(li), gr);
        self.most[0] = self.most[0].max(li);
        self.most[1] = self.most[1].max(gr);
        self.most[2] = self.most[2].max(lo);
        let s = Sample {
            input: db(li),
            output: db(lo),
            gr,
        };
        let pending = self.pending.get_or_insert(s);
        pending.input = pending.input.max(s.input);
        pending.output = pending.output.max(s.output);
        pending.gr = pending.gr.max(s.gr);
        let Some(last) = self.last else {
            self.last = Some(now);
            return;
        };
        let steps = (now.saturating_duration_since(last).as_micros() / STEP.as_micros()) as u32;
        if steps == 0 {
            return;
        }
        let cap = (span.as_micros() / STEP.as_micros()) as usize;
        if let Some(s) = self.pending.take() {
            for _ in 0..(steps as usize).min(cap) {
                self.history.push_back(s);
            }
        }
        while self.history.len() > cap {
            self.history.pop_front();
        }
        self.last = Some(last + STEP * steps);
    }

    /// Clears the `i`th most (the input's, the reduction's, the output's).
    pub fn clear(&mut self, i: usize) {
        if let Some(m) = self.most.get_mut(i) {
            *m = 0.0;
        }
    }

    /// Something still moves: a meter falls or a peak shows.
    pub fn moving(&self) -> bool {
        [&self.input, &self.output]
            .iter()
            .any(|m| m.level.iter().chain(&m.hold).any(|v| *v > 0.0))
            || self.now.1 > 0.0
    }

    /// What the `i`th readout says: the most in dB to a tenth, the
    /// reduction as a cut.
    pub fn readout(&self, i: usize) -> String {
        match i {
            1 if self.most[1] >= 0.05 => format!("-{:.1}", self.most[1]),
            1 => "0.0".to_string(),
            _ if self.most[i.min(2)] <= 0.0 => "-\u{221e}".to_string(),
            _ => format!("{:.1}", 20.0 * self.most[i.min(2)].log10()),
        }
    }

    /// The output went over full scale since it was cleared.
    pub fn over(&self) -> bool {
        self.most[2] > 1.0
    }
}

// ------------------------------------------------------------------ layout

/// What a meter's column shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    In,
    Gr,
    Out,
}

/// A meter's column at the side of a dynamics device: its name, its bars and
/// the readout of its most under them (a press clears it).
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub kind: Kind,
    pub label: &'static str,
    pub name: Rect,
    pub bars: Vec<Rect>,
    pub readout: Rect,
}

impl Level {
    pub fn at(self, dx: f32, dy: f32) -> Level {
        let m = |r: Rect| Rect::new(r.x + dx, r.y + dy, r.w, r.h);
        Level {
            name: m(self.name),
            bars: self.bars.into_iter().map(m).collect(),
            readout: m(self.readout),
            ..self
        }
    }
}

/// The columns' names.
pub const LABELS: [(&str, Kind); 3] = [("In", Kind::In), ("GR", Kind::Gr), ("Out", Kind::Out)];

const COL_MIN: f32 = 34.0;
const COL_GAP: f32 = 6.0;
const SIDE_PAD: f32 = 8.0;
const SIDE_FOOT: f32 = 6.0;
const NAME_H: f32 = 14.0;
const READ_H: f32 = 16.0;
const BAR_W: f32 = 5.0;
const GR_W: f32 = 8.0;

fn col_w(label: &str) -> f32 {
    crate::fonts::measure(label, FontSize::Micro, Weight::Regular)
        .ceil()
        .max(COL_MIN)
}

/// How wide the side with the columns `labels` is.
pub fn side_w(labels: &[(&'static str, Kind)]) -> f32 {
    let cols: f32 = labels.iter().map(|(l, _)| col_w(l)).sum();
    cols + COL_GAP * labels.len().saturating_sub(1) as f32 + SIDE_PAD * 2.0
}

/// The columns in the side `r`.
pub fn side(r: Rect, labels: &[(&'static str, Kind)]) -> Vec<Level> {
    let mut x = r.x + (r.w - side_w(labels)).max(0.0) / 2.0 + SIDE_PAD;
    let top = r.y + SIDE_PAD;
    let bar_y = top + NAME_H + 4.0;
    let bar_h = (r.bottom() - SIDE_FOOT - READ_H - 4.0 - bar_y).max(0.0);
    labels
        .iter()
        .map(|&(label, kind)| {
            let w = col_w(label);
            let bars = if kind == Kind::Gr {
                vec![Rect::new(x + (w - GR_W) / 2.0, bar_y, GR_W, bar_h)]
            } else {
                let pair = BAR_W * 2.0 + 2.0;
                let bx = x + (w - pair) / 2.0;
                (0..2)
                    .map(|ch| Rect::new(bx + ch as f32 * (BAR_W + 2.0), bar_y, BAR_W, bar_h))
                    .collect()
            };
            let level = Level {
                kind,
                label,
                name: Rect::new(x, top, w, NAME_H),
                bars,
                readout: Rect::new(x - 2.0, bar_y + bar_h + 4.0, w + 4.0, READ_H),
            };
            x += w + COL_GAP;
            level
        })
        .collect()
}

/// The index of a column's most (the input's, the reduction's, the output's).
pub fn most_of(kind: Kind) -> usize {
    match kind {
        Kind::In => 0,
        Kind::Gr => 1,
        Kind::Out => 2,
    }
}

/// How long a device's history is, `None` for one without levels.
pub fn span(def: &xirius_params::BuiltinDef) -> Option<Duration> {
    match def.uid {
        "compressor" => Some(super::compressor::SPAN),
        _ => None,
    }
}

// ------------------------------------------------------------------ painting

/// Paints the columns: names, the meters, and each readout (lighter under the
/// pointer, `hot`; the output's alert once over full scale).
pub fn paint_levels(pen: &mut Pen, levels: &[Level], d: &Dynamics, hot: Option<usize>) {
    let c = pen.c;
    for (i, l) in levels.iter().enumerate() {
        pen.text(
            l.label,
            FontSize::Micro,
            Weight::Regular,
            c.ink,
            l.name,
            Align::Center,
        );
        match l.kind {
            Kind::Gr => {
                if let Some(&r) = l.bars.first() {
                    pen.round(r, c.well, Radius::Clip);
                    let h = (d.now.1 / GR_SPAN).clamp(0.0, 1.0) * r.h;
                    if h > 0.0 {
                        pen.mask(r, |pen| pen.fill(Rect::new(r.x, r.y, r.w, h), c.fill));
                    }
                }
            }
            kind => {
                let m = if kind == Kind::In {
                    &d.input
                } else {
                    &d.output
                };
                for (ch, &r) in l.bars.iter().enumerate() {
                    crate::mixer::paint::meter(pen, r, m, ch.min(1));
                }
            }
        }
        if hot == Some(i) {
            pen.round(l.readout, c.surface_3, Radius::Clip);
        }
        let ink = if l.kind == Kind::Out && d.over() {
            c.ink_alert
        } else {
            c.ink_strong
        };
        pen.text(
            &d.readout(most_of(l.kind)),
            FontSize::Micro,
            Weight::Regular,
            ink,
            l.readout,
            Align::Center,
        );
    }
}

/// The history's level axis on the plane `r`: 0 dB at the top, the floor at
/// the foot.
pub fn level_axis(r: Rect) -> Axis {
    Axis::lin(r.y, r.bottom(), 0.0, FLOOR)
}

/// The scale's marks on a level plane.
pub const LEVEL_MARKS: [(f64, &str); 4] = [
    (-12.0, "-12"),
    (-24.0, "-24"),
    (-36.0, "-36"),
    (-48.0, "-48"),
];

/// Paints a history on the plane `r`, the newest at the right edge, `span`
/// of it across: the face over `over` dB a step darker, the input as an
/// area, the output as a line, and the reduction as a line hanging from the
/// top (dim while the device is bypassed).
pub fn paint_history(
    pen: &mut Pen,
    r: Rect,
    history: &VecDeque<Sample>,
    span: Duration,
    over: Option<f64>,
    bypassed: bool,
) {
    let c = pen.c;
    pen.round(r, c.surface_2, Radius::S);
    pen.mask(r, |pen| {
        let ay = level_axis(r);
        if let Some(t) = over {
            let y = ay.px(t).clamp(r.y, r.bottom());
            pen.fill(Rect::new(r.x, r.y, r.w, y - r.y), c.surface_0);
        }
        let cap = (span.as_micros() / STEP.as_micros()).max(2) as f32;
        let n = history.len();
        let x = |i: usize| r.w - (n - 1 - i) as f32 / (cap - 1.0) * r.w;
        let local = level_axis(Rect::new(0.0, 0.0, r.w, r.h));
        let line = |f: &dyn Fn(&Sample) -> f32| -> Vec<(f32, f32)> {
            history
                .iter()
                .enumerate()
                .map(|(i, s)| (x(i), f(s)))
                .collect()
        };
        let input = line(&|s| local.px(f64::from(s.input)));
        let output = line(&|s| local.px(f64::from(s.output)));
        let gr = line(&|s| (s.gr / GR_SPAN).clamp(0.0, 1.0) * r.h);
        let (ink, blue) = if bypassed {
            (c.scroll_thumb[0], c.scroll_thumb[0])
        } else {
            (c.ink_strong, c.ink_blue)
        };
        if n > 1 {
            graph::area(pen, r, &input, r.h, c.surface_3);
        }
        // The scale over the input's face, under the lines.
        for (v, name) in LEVEL_MARKS {
            let cell = Rect::new(r.x + 4.0, ay.px(v) - 7.0, 40.0, 14.0);
            pen.text(
                name,
                FontSize::Micro,
                Weight::Regular,
                c.ink,
                cell,
                Align::Left,
            );
        }
        if n > 1 {
            graph::stroke(pen, r, &output, 2.0, ink);
            graph::stroke(pen, r, &gr, 2.0, blue);
        }
    });
}
