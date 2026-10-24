//! The ruler both editing views share: the bar numbers, the loop band with its
//! grips, the seek on a click, and the playhead's head.
//!
//! The arrangement and the piano roll put song time on different pixels (the
//! arrangement by its lanes, the roll by its grid and the clip's start), so
//! each hands these functions a [`Scale`]. What a press on the ruler means,
//! what a drag leaves as the loop and how the ruler is painted are decided
//! here once, so the two rulers cannot drift apart.

use gpui::Hsla;
use xirius_core::time::{Tick, TimeMap};
use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::studio::{Geometry, Rect, Studio};

/// The loop band along the top of the ruler (ARR-001).
pub const LOOP_H: f32 = 14.0;
/// How wide a loop grip is for a press, in px.
pub const GRIP: f32 = 8.0;
/// The narrowest a bar number's gap may be before fewer are shown, in px.
const BAR_GAP: f32 = 40.0;

/// Which part of the loop a press on the band takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopGrip {
    Start,
    End,
    Body,
    /// Outside the band: a drag makes a new range.
    New,
}

/// Where song ticks sit on screen: `x = origin + tick / ppq * beat_px`. The
/// arrangement's origin is its lanes' left edge less the scroll; the roll's is
/// its grid's left edge less the scroll, less the clip's start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    pub origin: f32,
    pub beat_px: f32,
    pub ppq: f32,
}

impl Scale {
    pub fn x(&self, tick: Tick) -> f32 {
        self.origin + tick as f32 / self.ppq * self.beat_px
    }

    /// The tick under window x, not clamped below zero.
    pub fn tick(&self, x: f32) -> Tick {
        self.ticks((x - self.origin) / self.beat_px * self.ppq)
    }

    /// The tick a pixel distance spans (rounded).
    pub fn delta(&self, px: f32) -> Tick {
        self.ticks(px / self.beat_px * self.ppq)
    }

    fn ticks(&self, t: f32) -> Tick {
        t.round() as Tick
    }
}

impl Studio {
    /// The arrangement's scale: the lanes' left edge less the scroll.
    pub fn ruler_scale(&self, g: &Geometry) -> Scale {
        Scale {
            origin: g.lanes.x - self.scroll.0,
            beat_px: self.beat_w,
            ppq: self.project().ppq as f32,
        }
    }
}

/// Which grip a press at `x` takes, given where the loop's ends sit on screen
/// (`None` when there is no loop: the press makes one).
pub fn grip_at(span: Option<(f32, f32)>, x: f32) -> LoopGrip {
    let Some((a, b)) = span else {
        return LoopGrip::New;
    };
    let half = GRIP / 2.0;
    if (x - a).abs() <= half {
        LoopGrip::Start
    } else if (x - b).abs() <= half {
        LoopGrip::End
    } else if x > a && x < b {
        LoopGrip::Body
    } else {
        LoopGrip::New
    }
}

/// Whether a press at `y` lands on the loop band (the top of the ruler). Below
/// it a press only seeks.
pub fn on_band(ruler: Rect, y: f32) -> bool {
    y < ruler.y + LOOP_H
}

/// The loop a drag leaves, if it changes anything. `now` is the loop before
/// the drag, `press` and `at` are where the drag began and where it is now,
/// `dt` is how far a grip moved, and `snap` puts a tick on the grid.
pub fn loop_drag(
    now: Option<(Tick, Tick)>,
    grip: LoopGrip,
    press: Tick,
    at: Tick,
    dt: Tick,
    snap: impl Fn(Tick) -> Tick,
) -> Option<(Tick, Tick)> {
    let (a, b) = match (grip, now) {
        (LoopGrip::Start, Some((s, e))) => (snap(s + dt), e),
        (LoopGrip::End, Some((s, e))) => (s, snap(e + dt)),
        (LoopGrip::Body, Some((s0, e0))) => {
            let s = snap(s0 + dt).max(0);
            (s, s + (e0 - s0))
        }
        _ => (snap(press), snap(at)),
    };
    let (start, end) = (a.min(b).max(0), a.max(b));
    if end <= start || now == Some((start, end)) {
        return None;
    }
    Some((start, end))
}

/// The bars that show in a view from `first` to `last` (song ticks), each
/// with its number and first tick.
pub fn bars_in(tm: &TimeMap, first: Tick, last: Tick) -> Vec<(u32, Tick)> {
    let (mut bar, _, _) = tm.to_bbt(first.max(0));
    let mut out = Vec::new();
    loop {
        let tick = tm.bar_start(bar);
        if tick > last {
            break;
        }
        out.push((bar, tick));
        bar += 1;
    }
    out
}

/// Every how many bars a number is shown, so that the numbers keep a gap.
pub fn bar_every(bar_px: f32) -> u32 {
    [1u32, 2, 4, 8]
        .into_iter()
        .find(|n| *n as f32 * bar_px >= BAR_GAP)
        .unwrap_or(16)
}

/// The narrowest a numbered step may be, in px.
pub const STEP_MIN_PX: f32 = 52.0;

/// One number on the ruler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub tick: Tick,
    pub label: String,
    /// A bar's number (the others are `bar.beat` or `bar.beat.16th`).
    pub bar: bool,
}

/// The numbers along the ruler from `first` to `last` (song ticks), a quarter
/// note `beat_px` wide (UI-339). Bars are numbered, thinned out by
/// [`bar_every`] until a beat is [`STEP_MIN_PX`] wide, when every bar is.
/// Then each beat is numbered (`5.2`; in 6/8 the beats are dotted quarters but
/// the numbers count eighths, `5.4`), and from a 16th [`STEP_MIN_PX`] wide each
/// 16th (`5.2.3`), counted from the start of its bar. Under a triplet grid
/// (`triplets`) a 16th falls between the grid's thirds, so only the beats are
/// numbered; the ticks at the thirds come from the lines' split.
pub fn labels(
    tm: &TimeMap,
    ppq: u32,
    beat_px: f32,
    triplets: bool,
    first: Tick,
    last: Tick,
) -> Vec<Mark> {
    let px = |ticks: f64| (ticks / f64::from(ppq.max(1))) as f32 * beat_px;
    let bars = bars_in(tm, first, last);
    let bar_px = bars
        .iter()
        .map(|(bar, tick)| px((tm.bar_start(bar + 1) - tick) as f64))
        .reduce(f32::min)
        .unwrap_or_else(|| px(tm.bar_start(2) as f64));
    let every = bar_every(bar_px);
    let six = Tick::from(ppq / 4).max(1);
    let mut out = Vec::new();
    for (bar, s) in bars {
        let sig = tm.meter_at(bar);
        let end = tm.bar_start(bar + 1);
        let unit = sig.beat_ticks(ppq).max(1);
        let beat = crate::grid_lines::beat_of(sig, ppq);
        let beats = px(beat) >= STEP_MIN_PX;
        if s >= first && ((bar - 1) % every == 0 || beats) {
            out.push(Mark {
                tick: s,
                label: bar.to_string(),
                bar: true,
            });
        }
        let mut put = |t: Tick, label: String| {
            if t >= first && t <= last {
                out.push(Mark {
                    tick: t,
                    label,
                    bar: false,
                });
            }
        };
        if !triplets && px(six as f64) >= STEP_MIN_PX && unit % six == 0 {
            for t in (s + six..end).step_by(six as usize) {
                let n = (t - s) / unit + 1;
                put(t, format!("{bar}.{n}.{}", (t - s) % unit / six + 1));
            }
        } else if beats {
            let mut k = 1.0;
            loop {
                let t = (s as f64 + k * beat).round() as Tick;
                if t >= end || t > last {
                    break;
                }
                put(t, format!("{bar}.{}", (t - s) / unit + 1));
                k += 1.0;
            }
        }
    }
    out
}

/// A translucent stretch of the loop, behind whatever it covers.
pub fn shade(pen: &mut Pen, x0: f32, x1: f32, rect: Rect) {
    pen.fill(
        Rect::new(x0, rect.y, x1 - x0, rect.h),
        pen.c.fill.opacity(0.07),
    );
}

/// What the loop band shows: where the loop is, whether it is lit (it repeats
/// or is being dragged), whether it is held, and which grip the pointer is on.
pub struct Band {
    pub x0: f32,
    pub x1: f32,
    pub lit: bool,
    pub held: bool,
    pub hover: Option<LoopGrip>,
    /// The face while auto-punch records inside it (red), instead of the loop's.
    pub punch: bool,
}

/// The grey stretches the pre-roll and the post-roll add around the punch range, level
/// with the band, as window x pairs.
pub fn paint_rolls(pen: &mut Pen, ruler: Rect, rolls: [Option<(f32, f32)>; 2]) {
    for (x0, x1) in rolls.into_iter().flatten() {
        let rect = Rect::new(x0, ruler.y + 2.0, x1 - x0, 11.0);
        pen.round(rect, pen.c.surface_3, Radius::Xs);
    }
}

/// The loop band along the top of the ruler: filled while the range repeats,
/// grey while it does not. `shade_to`, when given, is the rectangle the lit
/// stretch covers below the band.
pub fn paint_band(pen: &mut Pen, band: &Band, ruler: Rect, shade_to: Option<Rect>) {
    if let (true, Some(rect)) = (band.lit, shade_to) {
        shade(pen, band.x0, band.x1, rect);
    }
    let hot = band.held || band.hover.is_some_and(|g| g != LoopGrip::New);
    let rect = Rect::new(band.x0, ruler.y + 2.0, band.x1 - band.x0, 11.0);
    let face = if band.punch {
        if hot {
            pen.c.alert.opacity(0.85)
        } else {
            pen.c.alert
        }
    } else if hot {
        pen.c.fill_hover
    } else if band.lit {
        pen.c.fill.opacity(0.7)
    } else {
        pen.c.surface_3
    };
    pen.round(rect, face, Radius::Xs);
    for (x, grip) in [(band.x0, LoopGrip::Start), (band.x1, LoopGrip::End)] {
        if band.hover == Some(grip) {
            pen.round(
                Rect::new(x - 1.5, rect.y + 2.0, 3.0, rect.h - 4.0),
                pen.c.on_fill,
                Radius::Pill,
            );
        }
    }
}

/// The ruler's numbers, each just right of its tick: a bar's in blue, the
/// others in the ink. `x_of` puts a tick on screen.
pub fn paint_labels(pen: &mut Pen, marks: &[Mark], ruler: Rect, x_of: impl Fn(Tick) -> f32) {
    for m in marks {
        let x = x_of(m.tick).round();
        let ink = if m.bar { pen.c.ink_blue } else { pen.c.ink };
        pen.digits(
            &m.label,
            FontSize::Caption,
            Weight::Medium,
            ink,
            Rect::new(x + Space::X1.px(), ruler.y, STEP_MIN_PX, ruler.h),
            Align::Left,
        );
    }
}

/// The room a time takes on the ruler (`0:09.3765`), in px.
pub const TIME_W: f32 = 72.0;

/// The ruler's times (UI-339), each just right of its tick at window x, in
/// the ink.
pub fn paint_times(pen: &mut Pen, times: &[(f32, &str)], ruler: Rect) {
    for (x, text) in times {
        pen.digits(
            text,
            FontSize::Caption,
            Weight::Medium,
            pen.c.ink,
            Rect::new(x.round() + Space::X1.px(), ruler.y, TIME_W, ruler.h),
            Align::Left,
        );
    }
}

/// The playhead at window x: a line from the top of `view` down, with its
/// head on the ruler, red while recording (as in the mock).
pub fn paint_playhead(pen: &mut Pen, studio: &Studio, x: f32, view: Rect) {
    let c = pen.c;
    // The count-in is not recording: the playhead stays white until the take begins.
    let color = if studio.transport.recording && studio.transport.pre_count.is_none() {
        c.alert
    } else {
        c.ink_strong
    };
    paint_head(pen, x, view, color);
}

/// The playhead's line and head in `color`, drawn inside `view` only, with no light around the
/// line (2026-10-21).
fn paint_head(pen: &mut Pen, x: f32, view: Rect, color: Hsla) {
    pen.mask(view, |pen| {
        pen.fill(
            Rect::new(
                x - Layout::PLAYHEAD_W / 2.0,
                view.y,
                Layout::PLAYHEAD_W,
                view.h,
            ),
            color,
        );
        pen.playhead_head(x, view.y, color);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_maps_ticks_both_ways_and_rounds_like_the_arrangement() {
        let s = Scale {
            origin: 100.0,
            beat_px: 48.0,
            ppq: 960.0,
        };
        assert_eq!(s.x(960), 148.0);
        assert_eq!(s.tick(148.0), 960);
        assert_eq!(s.delta(48.0), 960);
        // Left of the origin the tick is negative (the caller clamps).
        assert_eq!(s.tick(90.0), -200);
    }

    fn names(tm: &TimeMap, beat_px: f32, first: Tick, last: Tick) -> Vec<String> {
        labels(tm, 960, beat_px, false, first, last)
            .into_iter()
            .map(|m| m.label)
            .collect()
    }

    #[test]
    fn the_ruler_numbers_the_beats_and_then_the_16ths_as_it_zooms_in() {
        let tm = TimeMap::new(960, &[]);
        let bar5 = tm.bar_start(5);
        let bar = |b: u32| b.to_string();
        // 48 px a beat: a bar is 192 px, every bar is numbered and nothing else.
        assert_eq!(names(&tm, 48.0, 0, tm.bar_start(4)), [1, 2, 3, 4].map(bar));
        // 2 px a beat: 8 px a bar, every 8th bar.
        assert_eq!(names(&tm, 2.0, 0, tm.bar_start(17)), [1, 9, 17].map(bar));
        // Bar 5 starts at 16 beats: its first step is "5", then "5.2" ...
        assert_eq!(
            names(&tm, 52.0, bar5, bar5 + 960 * 4 - 1),
            ["5", "5.2", "5.3", "5.4"]
        );
        assert_eq!(names(&tm, 207.0, bar5, bar5 + 960 * 2 - 1), ["5", "5.2"]);
        assert_eq!(
            names(&tm, 208.0, bar5 + 960, bar5 + 960 + 959),
            ["5.2.1", "5.2.2", "5.2.3", "5.2.4"]
        );
        let m = labels(&tm, 960, 52.0, false, bar5, bar5);
        assert!(m[0].bar);
        assert!(!labels(&tm, 960, 52.0, false, bar5 + 1, bar5 + 960)[0].bar);
    }

    #[test]
    fn a_triplet_grid_numbers_only_the_beats() {
        let tm = TimeMap::new(960, &[]);
        let trip = |beat_px| -> Vec<String> {
            labels(&tm, 960, beat_px, true, 0, 1919)
                .into_iter()
                .map(|m| m.label)
                .collect()
        };
        // Wide enough for the 16ths, but a triplet grid keeps to the beats.
        assert_eq!(names(&tm, 400.0, 0, 1919).len(), 8);
        assert_eq!(trip(400.0), ["1", "1.2"]);
        assert_eq!(trip(4096.0), ["1", "1.2"]);
        // The split's ticks sit at the beat's thirds (UI-339).
        let t = crate::tools::Snap {
            mode: crate::tools::SnapMode::Quantize,
            grid: 160,
            triplet: true,
            ..crate::tools::Snap::default()
        };
        let lines = crate::grid_lines::lines(&t, &crate::tools::Meter::plain(960, 400.0), 0, 959);
        let ticked: Vec<_> = lines
            .iter()
            .filter(|l| l.tier.index() < crate::grid_lines::TICK_H.len())
            .map(|l| l.tick)
            .collect();
        assert_eq!(ticked, [0, 320, 640]);
    }

    #[test]
    fn the_numbers_follow_the_meter_of_each_bar() {
        use xirius_core::time::Meter;
        let tm = TimeMap::new(
            960,
            &[(1, Meter { num: 4, den: 4 }), (2, Meter { num: 6, den: 8 })],
        );
        let (s, e) = (tm.bar_start(2), tm.bar_start(3) - 1);
        // 6/8 beats on the dotted quarter, but the numbers count eighths.
        assert_eq!(names(&tm, 30.0, s, e), ["2"]);
        assert_eq!(names(&tm, 104.0, s, e), ["2", "2.4"]);
        let sixteenths = names(&tm, 208.0, s, e);
        // An eighth holds two 16ths.
        assert_eq!(sixteenths[1], "2.1.2");
        assert_eq!(sixteenths[2], "2.2.1");
        assert_eq!(sixteenths.len(), 12);
        // 7/8: eighth beats, numbered from 52 px an eighth.
        let tm = TimeMap::new(960, &[(1, Meter { num: 7, den: 8 })]);
        assert_eq!(names(&tm, 104.0, 0, 959), ["1", "1.2"]);
        // A 32nd beat holds no whole 16th: the beats are numbered instead.
        let tm = TimeMap::new(960, &[(1, Meter { num: 4, den: 32 })]);
        assert_eq!(names(&tm, 1664.0, 0, 479), ["1", "1.2", "1.3", "1.4"]);
    }

    #[test]
    fn a_press_takes_the_grip_it_is_on() {
        let span = Some((100.0, 200.0));
        assert_eq!(grip_at(span, 102.0), LoopGrip::Start);
        assert_eq!(grip_at(span, 198.0), LoopGrip::End);
        assert_eq!(grip_at(span, 150.0), LoopGrip::Body);
        assert_eq!(grip_at(span, 50.0), LoopGrip::New);
        assert_eq!(grip_at(None, 150.0), LoopGrip::New);
    }

    #[test]
    fn the_band_is_the_top_of_the_ruler_only() {
        let ruler = Rect::new(0.0, 10.0, 500.0, 28.0);
        assert!(on_band(ruler, 10.0));
        assert!(on_band(ruler, 23.0));
        assert!(!on_band(ruler, 24.5));
    }

    #[test]
    fn a_drag_moves_the_loop_by_its_grip_and_empty_ranges_are_dropped() {
        let snap = |t: Tick| t / 240 * 240;
        let now = Some((960, 3840));
        assert_eq!(
            loop_drag(now, LoopGrip::Start, 0, 0, 480, snap),
            Some((1440, 3840))
        );
        assert_eq!(
            loop_drag(now, LoopGrip::End, 0, 0, -960, snap),
            Some((960, 2880))
        );
        assert_eq!(
            loop_drag(now, LoopGrip::Body, 0, 0, 240, snap),
            Some((1200, 4080))
        );
        // Making a new range from 480 to 1440, backwards too.
        assert_eq!(
            loop_drag(None, LoopGrip::New, 1440, 480, 0, snap),
            Some((480, 1440))
        );
        // A grip put back where it was, or a zero-length range, changes nothing.
        assert_eq!(loop_drag(now, LoopGrip::Start, 0, 0, 0, snap), None);
        assert_eq!(loop_drag(None, LoopGrip::New, 500, 510, 0, snap), None);
    }

    #[test]
    fn bar_numbers_thin_out_as_bars_narrow() {
        assert_eq!(bar_every(100.0), 1);
        assert_eq!(bar_every(30.0), 2);
        assert_eq!(bar_every(12.0), 4);
        assert_eq!(bar_every(5.0), 8);
        assert_eq!(bar_every(1.0), 16);
    }
}
