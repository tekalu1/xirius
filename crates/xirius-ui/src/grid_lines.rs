//! The time grid both editing views draw, and the ruler's ticks under it
//! (UI-339).
//!
//! Four tiers ([`GridTier`]), strongest first: each bar's first tick, the
//! meter's beats (a dotted quarter in 6/8, 9/8 and 12/8), the beat's split (a
//! half, or a third in a compound meter or under a triplet grid) and the
//! snap's own value when it is finer than the split or off the beats and
//! splits drawn (1/4t, a 1/4 note in 6/8). Bars always show; beats show from
//! [`GridLine::MIN_BEAT_PX`] apart and are never thinned; the split and the
//! snap's value show from [`GridLine::MIN_FINE_PX`], the snap's value doubled
//! until it is that wide. Every tier counts from the start of its bar,
//! as the snap does ([`crate::tools::Snap::round`]), and fades in from its
//! threshold. Frames are found in seconds through the tempo map, so they stay
//! on the frame after a tempo change.
//!
//! Zoomed in until a 1/64 note is [`TIME_ON_PX`] wide, the arrangement also
//! draws the seconds ([`times`]): lines every 1, 2 or 5 of a power of ten
//! seconds at least [`TIME_LINE_PX`] apart in the finest tier's color, the
//! finest note lines then in the split's color, and the ruler gives the time
//! every [`TIME_LABEL_PX`].
//!
//! Width, place and color come from [`GridLine`] only.

use std::collections::BTreeMap;

use xirius_core::time::{Meter as Sig, Tick};
use xirius_design::{GridLine, GridTier};

use crate::draw::Pen;
use crate::studio::Rect;
use crate::tools::{FPS, Meter, Snap, SnapMode};

/// One line of the grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub tick: Tick,
    pub tier: GridTier,
    /// How much of the tier's color it takes (it fades in from its threshold).
    pub fade: f32,
}

/// The heights of the ruler's ticks for a bar, a beat and a split, in px.
pub const TICK_H: [f32; 3] = [10.0, 6.0, 3.0];

/// Whether a meter's beat is a dotted note: 6/8, 9/8, 12/8 (3/8 keeps eighths).
pub fn compound(sig: Sig) -> bool {
    sig.den >= 8 && sig.num.is_multiple_of(3) && sig.num > 3
}

/// A beat's length in ticks: the denominator's note, or three of them in a
/// compound meter.
pub fn beat_of(sig: Sig, ppq: u32) -> f64 {
    let unit = sig.beat_ticks(ppq) as f64;
    if compound(sig) { unit * 3.0 } else { unit }
}

/// The beat's split in ticks: a third in a compound meter or under a triplet
/// grid, else a half.
pub fn sub_of(sig: Sig, ppq: u32, triplets: bool) -> f64 {
    let parts = if compound(sig) || triplets { 3.0 } else { 2.0 };
    beat_of(sig, ppq) / parts
}

/// The grid's value as the lines see it in a bar of `sig`: frames have their
/// own lines and a bar's grid has no line finer than the beat.
fn step_in(snap: &Snap, m: &Meter, sig: Sig) -> f64 {
    match snap.mode {
        SnapMode::Frames => f64::INFINITY,
        SnapMode::Bar => sig.bar_ticks(m.ppq) as f64,
        SnapMode::Auto | SnapMode::Quantize => snap.step(m) as f64,
    }
}

/// The steps a bar of `sig` draws from its first tick, each with its tier and
/// fade: the beat; the split while the snap is that fine; and the snap's own
/// value, doubled until it is [`GridLine::MIN_FINE_PX`] apart, unless a step
/// already drawn lands on each of its points (a 1/4 note on the beats) — so a
/// value the beat does not halve (1/4t, a 1/4 note in 6/8) still shows. Below
/// the split it doubles only up to the split: never two tiers one spacing.
fn runs(snap: &Snap, m: &Meter, sig: Sig) -> Vec<(f64, GridTier, f32)> {
    let px = |ticks: f64| m.px(ticks);
    let bar = sig.bar_ticks(m.ppq) as f64;
    let beat = beat_of(sig, m.ppq);
    let sub = sub_of(sig, m.ppq, snap.triplets());
    let step = step_in(snap, m, sig);
    let mut runs = vec![(beat, GridTier::Beat)];
    if step <= sub + 0.5 {
        runs.push((sub, GridTier::Sub));
    }
    let mut runs: Vec<_> = runs
        .into_iter()
        .map(|(st, tier)| (st, tier, GridLine::fade(tier, px(st))))
        .filter(|r| r.2 > 0.0)
        .collect();
    // Whether every point of the snap is a line every `r`.
    let lands = |r: f64| {
        let n = (step / r).round();
        n >= 1.0 && (step - n * r).abs() < 0.5
    };
    if step < bar - 0.5 && !runs.iter().any(|r| lands(r.0)) {
        let limit = if step < sub - 0.5 { sub } else { bar };
        let mut sp = step;
        while px(sp) < GridLine::MIN_FINE_PX && sp * 2.0 < limit - 0.5 {
            sp *= 2.0;
        }
        let fade = GridLine::fade(GridTier::Fine, px(sp));
        if fade > 0.0 {
            runs.push((sp, GridTier::Fine, fade));
        }
    }
    runs
}

/// The lines from `first` to `last` (song ticks), in order. Where two tiers
/// meet the stronger one is kept.
pub fn lines(snap: &Snap, m: &Meter, first: Tick, last: Tick) -> Vec<Line> {
    let mut found: Vec<(f64, GridTier, f32)> = Vec::new();
    for (bar, s) in crate::ruler::bars_in(&m.time, first, last) {
        found.push((s as f64, GridTier::Bar, 1.0));
        let end = m.time.bar_start(bar + 1) as f64;
        // Each tier at `s + k·step` up to the bar's end, from the view's left.
        for (step, tier, fade) in runs(snap, m, m.time.meter_at(bar)) {
            if step < 1.0 {
                continue;
            }
            let mut k = (((first - s) as f64 / step).floor() as i64).max(1);
            loop {
                let t = s as f64 + k as f64 * step;
                if t >= end - 0.5 || t > last as f64 + 0.5 {
                    break;
                }
                found.push((t, tier, fade));
                k += 1;
            }
        }
    }
    if snap.mode == SnapMode::Frames {
        frames(m, first, last, &mut found);
    }
    let mut all: BTreeMap<Tick, (GridTier, f32)> = BTreeMap::new();
    for (t, tier, fade) in found {
        let t = t.round() as Tick;
        if (first..=last).contains(&t) && all.get(&t).is_none_or(|(have, _)| *have > tier) {
            all.insert(t, (tier, fade));
        }
    }
    thin(all, m)
}

/// Keeps the bars and beats, then each split and finer line only where it sits
/// at least both thresholds from the lines already kept: a step that does not
/// divide the split (1/8t in 6/8) or a bar shorter than its steps (7/8) puts a
/// line close to a stronger one, and the weaker one gives way.
fn thin(all: BTreeMap<Tick, (GridTier, f32)>, m: &Meter) -> Vec<Line> {
    let mut kept: BTreeMap<Tick, (GridTier, f32)> = BTreeMap::new();
    for tier in GridTier::ALL {
        for (&t, &(have, fade)) in &all {
            if have != tier {
                continue;
            }
            let far = |n: Option<(&Tick, &(GridTier, f32))>| {
                n.is_none_or(|(&o, &(other, _))| {
                    m.px((t - o).abs() as f64) + 0.01 >= tier.min_px().max(other.min_px())
                })
            };
            let strong = matches!(tier, GridTier::Bar | GridTier::Beat);
            if strong || (far(kept.range(..t).next_back()) && far(kept.range(t..).next())) {
                kept.insert(t, (tier, fade));
            }
        }
    }
    kept.into_iter()
        .map(|(tick, (tier, fade))| Line { tick, tier, fade })
        .collect()
}

/// The finest grid of the snap's points the lines show in the bar at `at` (the
/// view's left edge), in ticks, when some point of the snap has no line there
/// (UI-339): the snap keeps the value chosen and the grid button adds what the
/// lines show. It is read off [`lines`] themselves, after the thinning: every
/// `n`-th point for the smallest `n` whose points all have a line, else the
/// bar. `None` when every point has a line, or the grid is a bar's or frames.
pub fn visible(snap: &Snap, m: &Meter, at: Tick) -> Option<f64> {
    if matches!(snap.mode, SnapMode::Frames | SnapMode::Bar) {
        return None;
    }
    let bar = m.time.to_bbt(at.max(0)).0;
    let (s, e) = (m.time.bar_start(bar), m.time.bar_start(bar + 1));
    let shown: Vec<Tick> = lines(snap, m, s, e).iter().map(|l| l.tick).collect();
    let step = snap.step(m);
    let on = |every: Tick| {
        (1..)
            .map(|k| s + k * every)
            .take_while(|&t| t < e)
            .all(|t| shown.binary_search(&t).is_ok())
    };
    if on(step) {
        return None;
    }
    let every = (2..)
        .map(|n| n * step)
        .take_while(|&v| v < e - s)
        .find(|&v| on(v));
    Some(every.unwrap_or(e - s) as f64)
}

/// The width of a 1/64 note from which the arrangement draws the seconds.
pub const TIME_ON_PX: f32 = 256.0;
/// The narrowest a step of the seconds' lines may be, in px.
pub const TIME_LINE_PX: f64 = 12.0;
/// The narrowest a step of the ruler's times may be, in px.
pub const TIME_LABEL_PX: f64 = 96.0;
/// The ruler's tick under a numbered second, in px.
pub const TIME_TICK_H: [f32; 2] = [6.0, 3.0];

/// One second's line, between ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeLine {
    /// In song ticks, with the fraction.
    pub at: f64,
    pub secs: f64,
}

/// The seconds the arrangement draws: the lines, and those the ruler numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Times {
    pub lines: Vec<TimeLine>,
    pub labels: Vec<(TimeLine, String)>,
}

/// Whether the view is zoomed in far enough for the seconds.
pub fn time_on(m: &Meter) -> bool {
    m.px(f64::from(m.ppq) / 16.0) >= TIME_ON_PX
}

/// The shortest step of 1, 2 or 5 times a power of ten seconds (1 µs to 50 s)
/// at least `min_px` wide at `px_per_sec`, else 10 s.
pub fn time_step(px_per_sec: f64, min_px: f64) -> f64 {
    for k in -6..=1 {
        for mant in [1.0, 2.0, 5.0] {
            // Divided for the small steps, so 5e-5 is the literal's double.
            let v = if k < 0 {
                mant / 10f64.powi(-k)
            } else {
                mant * 10f64.powi(k)
            };
            if v * px_per_sec >= min_px {
                return v;
            }
        }
    }
    10.0
}

/// `secs` as the ruler writes it: `0:09.3765`, with as many decimals as the
/// step needs (three at least).
pub fn time_label(secs: f64, step: f64) -> String {
    let dec = ((-step.log10() - 1e-9).ceil().max(3.0)) as u32;
    let scale = 10u64.pow(dec);
    let t = (secs.max(0.0) * scale as f64).round() as u64;
    let (min, rem) = (t / (60 * scale), t % (60 * scale));
    format!(
        "{min}:{:02}.{:0w$}",
        rem / scale,
        rem % scale,
        w = dec as usize
    )
}

/// The seconds every `step` from `first` to `last`.
fn seconds(m: &Meter, first: Tick, last: Tick, step: f64) -> Vec<TimeLine> {
    let s0 = m.tempo.seconds_at(first.max(0));
    let s1 = m.tempo.seconds_at(last);
    let mut out = Vec::new();
    let mut k = (s0 / step).ceil() as i64;
    while k as f64 * step <= s1 {
        let secs = k as f64 * step;
        out.push(TimeLine {
            at: m.tempo.tick_at_f(secs),
            secs,
        });
        k += 1;
    }
    out
}

/// The seconds from `first` to `last`, or `None` until a 1/64 note is
/// [`TIME_ON_PX`] wide. The steps are found at the slowest tempo between them,
/// where a second is the fewest px.
pub fn times(m: &Meter, first: Tick, last: Tick) -> Option<Times> {
    if !time_on(m) {
        return None;
    }
    let pps = f64::from(m.beat_px) * m.tempo.min_bpm(first.max(0), last) / 60.0;
    let label = time_step(pps, TIME_LABEL_PX);
    Some(Times {
        lines: seconds(m, first, last, time_step(pps, TIME_LINE_PX)),
        labels: seconds(m, first, last, label)
            .into_iter()
            .map(|l| (l, time_label(l.secs, label)))
            .collect(),
    })
}

/// The note lines under the seconds: the finest in the split's color, which
/// the seconds' lines take over.
pub fn under_times(lines: &[Line]) -> Vec<Line> {
    lines
        .iter()
        .map(|l| Line {
            tier: if l.tier == GridTier::Fine {
                GridTier::Sub
            } else {
                l.tier
            },
            ..*l
        })
        .collect()
}

/// The seconds' lines from `top` down `h` px, but where a note line already is;
/// `x_at` puts a place between ticks on screen.
pub fn paint_times(
    pen: &mut Pen,
    times: &[TimeLine],
    lines: &[Line],
    top: f32,
    h: f32,
    x_at: impl Fn(f64) -> f32,
) {
    let tier = GridTier::Fine;
    for t in times {
        let near = lines.partition_point(|l| (l.tick as f64) < t.at - 0.5);
        if lines
            .get(near)
            .is_some_and(|l| (l.tick as f64 - t.at).abs() < 0.5)
        {
            continue;
        }
        pen.fill(
            Rect::new(
                GridLine::left(x_at(t.at), tier),
                top,
                GridLine::width(tier),
                h,
            ),
            pen.c.grid[tier.index()],
        );
    }
}

/// The ruler's ticks under the seconds: a short one under each line, a longer
/// one in the beat's color under each numbered second.
pub fn paint_time_ticks(pen: &mut Pen, times: &Times, ruler: Rect, x_at: impl Fn(f64) -> f32) {
    let ticks = times
        .lines
        .iter()
        .map(|l| (l, GridTier::Sub, TIME_TICK_H[1]))
        .chain(
            times
                .labels
                .iter()
                .map(|(l, _)| (l, GridTier::Beat, TIME_TICK_H[0])),
        );
    for (l, tier, h) in ticks {
        pen.fill(
            Rect::new(
                GridLine::left(x_at(l.at), tier),
                ruler.bottom() - h,
                GridLine::width(tier),
                h,
            ),
            pen.c.grid[tier.index()],
        );
    }
}

/// The frames from `first` to `last` on the finest tier, each found in seconds.
fn frames(m: &Meter, first: Tick, last: Tick, found: &mut Vec<(f64, GridTier, f32)>) {
    let frame_px = |bpm: f64| m.px(f64::from(m.ppq) * bpm / 60.0 / FPS);
    // Too narrow everywhere in the view: no frame shows, so none is looked up.
    if frame_px(m.tempo.max_bpm(first.max(0), last)) < GridLine::MIN_FINE_PX {
        return;
    }
    let s0 = m.tempo.seconds_at(first.max(0));
    let s1 = m.tempo.seconds_at(last);
    let mut k = (s0 * FPS).ceil() as i64;
    while k as f64 <= s1 * FPS {
        let t = m.tempo.tick_at(k as f64 / FPS);
        let fade = GridLine::fade(GridTier::Fine, frame_px(m.tempo.bpm_at(t)));
        if fade > 0.0 {
            found.push((t as f64, GridTier::Fine, fade));
        }
        k += 1;
    }
}

/// The lines from `top` down `h` px; `x_of` puts a tick on screen.
pub fn paint(pen: &mut Pen, lines: &[Line], top: f32, h: f32, x_of: impl Fn(Tick) -> f32) {
    for l in lines {
        pen.fill(
            Rect::new(
                GridLine::left(x_of(l.tick), l.tier),
                top,
                GridLine::width(l.tier),
                h,
            ),
            pen.c.grid[l.tier.index()].opacity(l.fade),
        );
    }
}

/// The ruler's ticks at its bottom edge under the bar, beat and split lines,
/// [`TICK_H`] tall, in the lines' own colors.
pub fn paint_ticks(pen: &mut Pen, lines: &[Line], ruler: Rect, x_of: impl Fn(Tick) -> f32) {
    for l in lines {
        let Some(&h) = TICK_H.get(l.tier.index()) else {
            continue;
        };
        pen.fill(
            Rect::new(
                GridLine::left(x_of(l.tick), l.tier),
                ruler.bottom() - h,
                GridLine::width(l.tier),
                h,
            ),
            pen.c.grid[l.tier.index()].opacity(l.fade),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::tempo::TempoMap;
    use xirius_core::time::TimeMap;

    const PPQ: u32 = 960;

    fn plain(beat_px: f32) -> Meter {
        Meter::plain(PPQ, beat_px)
    }

    fn sig(num: u32, den: u32) -> Meter {
        Meter {
            time: TimeMap::new(PPQ, &[(1, Sig { num, den })]).into(),
            ..plain(48.0)
        }
    }

    fn quantize(grid: Tick) -> Snap {
        Snap {
            mode: SnapMode::Quantize,
            grid,
            ..Snap::default()
        }
    }

    fn quantize_t(grid: Tick) -> Snap {
        Snap {
            triplet: true,
            ..quantize(grid)
        }
    }

    fn at(m: &Meter, beat_px: f32) -> Meter {
        Meter {
            beat_px,
            ..m.clone()
        }
    }

    /// The ticks of one tier from `first` to `last`.
    fn tier(lines: &[Line], tier: GridTier) -> Vec<Tick> {
        lines
            .iter()
            .filter(|l| l.tier == tier)
            .map(|l| l.tick)
            .collect()
    }

    #[test]
    fn four_four_at_sixteen_px_a_beat_shows_bars_beats_and_eighths() {
        // The arrangement's default: 16 px a beat, the auto step 1/8 beat (2 px) ... 1/2 (8 px).
        let m = plain(16.0);
        let l = lines(&Snap::default(), &m, 0, 3840 * 2);
        assert_eq!(tier(&l, GridTier::Bar), [0, 3840, 7680]);
        assert_eq!(
            tier(&l, GridTier::Beat),
            [960, 1920, 2880, 4800, 5760, 6720]
        );
        // The auto step is 1/2 beat (8 px): it is the split, so no finer line.
        assert_eq!(tier(&l, GridTier::Sub).len(), 8);
        assert!(tier(&l, GridTier::Fine).is_empty());
        let sub = l.iter().find(|l| l.tier == GridTier::Sub).unwrap();
        assert!((sub.fade - 0.4).abs() < 1e-6, "{}", sub.fade);
    }

    #[test]
    fn beats_show_from_six_px_and_are_never_thinned() {
        let m = plain(5.9);
        let l = lines(&quantize(240), &m, 0, 3840 * 4);
        assert!(tier(&l, GridTier::Beat).is_empty());
        assert_eq!(tier(&l, GridTier::Bar).len(), 5);
        let l = lines(&quantize(240), &plain(6.0), 0, 3840);
        assert_eq!(tier(&l, GridTier::Beat), [960, 1920, 2880]);
        // Even a bar's grid shows the beats, and nothing finer.
        let bar = Snap {
            mode: SnapMode::Bar,
            ..Snap::default()
        };
        let l = lines(&bar, &plain(200.0), 0, 3840);
        assert_eq!(tier(&l, GridTier::Beat), [960, 1920, 2880]);
        assert!(tier(&l, GridTier::Sub).is_empty() && tier(&l, GridTier::Fine).is_empty());
    }

    #[test]
    fn lines_finer_than_the_snap_never_show() {
        // 1/4 at 400 px a beat: the halves would be 200 px apart, but the snap is the beat.
        let l = lines(&quantize(960), &plain(400.0), 0, 3840);
        assert!(tier(&l, GridTier::Sub).is_empty() && tier(&l, GridTier::Fine).is_empty());
        let l = lines(&quantize(480), &plain(400.0), 0, 3840);
        assert_eq!(tier(&l, GridTier::Sub).len(), 4);
        assert!(tier(&l, GridTier::Fine).is_empty());
    }

    #[test]
    fn the_fine_step_doubles_until_it_is_eight_px_apart() {
        // 1/64 note (60 ticks) at 48 px a beat is 3 px: doubled to 120 (6 px), 240 (12 px).
        let l = lines(&quantize(60), &plain(48.0), 0, 3840);
        let fine = tier(&l, GridTier::Fine);
        assert_eq!(&fine[..3], [240, 720, 1200]);
        // At 64 px 1/16 note (240) is 16 px; 1/64 note doubles to 120 (8 px) and fades in.
        let l = lines(&quantize(60), &plain(64.0), 0, 960);
        assert_eq!(tier(&l, GridTier::Fine), [120, 240, 360, 600, 720, 840]);
        let f = l.iter().find(|l| l.tier == GridTier::Fine).unwrap().fade;
        assert!((f - 0.4).abs() < 1e-6);
        // Doubling stops below the split: never two tiers at the same spacing.
        let l = lines(&quantize(60), &plain(10.0), 0, 3840);
        assert!(tier(&l, GridTier::Fine).is_empty());
        for w in lines(&quantize(15), &plain(4096.0), 0, 960).windows(2) {
            let px = (w[1].tick - w[0].tick) as f32 / 960.0 * 4096.0;
            assert!(px >= GridLine::MIN_FINE_PX, "{px}");
        }
    }

    #[test]
    fn no_two_lines_sit_closer_than_their_threshold_at_any_zoom() {
        for m in [plain(1.0), sig(3, 4), sig(6, 8), sig(7, 8), sig(12, 8)] {
            for beat_px in [
                1.0_f32, 3.0, 6.0, 8.0, 16.0, 29.1, 48.0, 100.0, 400.0, 4096.0,
            ] {
                let m = at(&m, beat_px);
                let eighth_t = Snap {
                    triplet: true,
                    ..quantize(320)
                };
                for snap in [Snap::default(), quantize(60), quantize(240), eighth_t] {
                    let l = lines(&snap, &m, 0, 3840 * 6);
                    for w in l.windows(2) {
                        let px = m.px((w[1].tick - w[0].tick) as f64);
                        let min = w[1].tier.min_px().max(w[0].tier.min_px());
                        assert!(px + 0.01 >= min, "{beat_px} {snap:?} {:?}: {px}", w);
                    }
                }
            }
        }
    }

    #[test]
    fn three_four_has_quarter_beats_and_eighth_splits() {
        let m = at(&sig(3, 4), 48.0);
        let l = lines(&quantize(240), &m, 0, 2880);
        assert_eq!(tier(&l, GridTier::Bar), [0, 2880]);
        assert_eq!(tier(&l, GridTier::Beat), [960, 1920]);
        assert_eq!(tier(&l, GridTier::Sub), [480, 1440, 2400]);
        assert_eq!(tier(&l, GridTier::Fine).len(), 6);
    }

    #[test]
    fn six_eight_beats_on_the_dotted_quarter_and_splits_in_three_eighths() {
        let m = at(&sig(6, 8), 48.0);
        let l = lines(&quantize(240), &m, 0, 2880);
        assert_eq!(tier(&l, GridTier::Bar), [0, 2880]);
        assert_eq!(tier(&l, GridTier::Beat), [1440]);
        assert_eq!(tier(&l, GridTier::Sub), [480, 960, 1920, 2400]);
        // 1/16 is finer than the eighth: it is the fine tier.
        assert_eq!(tier(&l, GridTier::Fine).len(), 6);
        // 3/8 keeps an eighth beat.
        assert!(!compound(Sig { num: 3, den: 8 }));
        assert!(compound(Sig { num: 12, den: 8 }));
        assert!(!compound(Sig { num: 6, den: 4 }));
    }

    #[test]
    fn seven_eight_beats_on_eighths_and_a_triplet_grid_starts_each_bar_again() {
        // 7/8: a bar of 3360 ticks, eighth beats, 16th splits.
        let m = at(&sig(7, 8), 96.0);
        let l = lines(&quantize(240), &m, 0, 3360 * 2);
        assert_eq!(tier(&l, GridTier::Bar), [0, 3360, 6720]);
        assert_eq!(tier(&l, GridTier::Beat).len(), 12);
        assert_eq!(tier(&l, GridTier::Sub).len(), 14);
        // 1/8t (320): a third of the eighth beat is 160, the triplet grid's split;
        // 320 is coarser than that, so the split does not show, the lines count
        // from each bar's first tick and the second bar starts again at 3360.
        let t = Snap {
            triplet: true,
            ..quantize(320)
        };
        let l = lines(&t, &m, 0, 3360 * 2);
        assert!(tier(&l, GridTier::Sub).is_empty());
        let beats = tier(&l, GridTier::Beat);
        assert!(beats.contains(&(3360 + 480)));
        // The 1/16t grid (160) is the split under a triplet grid.
        let t = Snap {
            triplet: true,
            ..quantize(160)
        };
        let l = lines(&t, &m, 3360, 3360 * 2);
        let sub = tier(&l, GridTier::Sub);
        assert_eq!(&sub[..4], [3360 + 160, 3360 + 320, 3360 + 640, 3360 + 800]);
    }

    #[test]
    fn a_triplet_grid_splits_the_beat_in_three() {
        let t = Snap {
            triplet: true,
            ..quantize(160)
        };
        let l = lines(&t, &plain(96.0), 0, 960);
        assert_eq!(tier(&l, GridTier::Sub), [320, 640]);
        assert_eq!(tier(&l, GridTier::Fine), [160, 480, 800]);
        // The same value chosen in the auto mode is not a triplet.
        let auto = Snap {
            triplet: true,
            ..Snap::default()
        };
        assert_eq!(
            tier(&lines(&auto, &plain(96.0), 0, 960), GridTier::Sub),
            [480]
        );
    }

    #[test]
    fn frames_are_found_in_seconds_past_a_tempo_change() {
        use xirius_core::id::TempoId;
        use xirius_core::model::TempoPoint;
        let pt = |n, tick, bpm| TempoPoint {
            id: TempoId(n),
            tick,
            bpm,
            ramp: false,
        };
        // 120 bpm for one second (1920 ticks), then 90 bpm.
        let m = Meter {
            tempo: TempoMap::new(PPQ, &[pt(1, 0, 120.0), pt(2, 1920, 90.0)]).into(),
            ..plain(480.0)
        };
        let frames = Snap {
            mode: SnapMode::Frames,
            ..Snap::default()
        };
        let l = lines(&frames, &m, 0, 3840);
        let fine = tier(&l, GridTier::Fine);
        // 64 ticks a frame at 120 bpm (32 px), 48 at 90 bpm.
        assert_eq!(&fine[..2], [64, 128]);
        assert!(tier(&l, GridTier::Beat).contains(&1920));
        assert!(fine.contains(&(1920 + 48)) && fine.contains(&(1920 + 96)));
        // Frames carry no split or finer step of their own.
        assert!(tier(&l, GridTier::Sub).is_empty());
        // Zoomed out, frames are too narrow everywhere and none is drawn.
        assert!(tier(&lines(&frames, &at(&m, 48.0), 0, 3840), GridTier::Fine).is_empty());
    }

    #[test]
    fn a_meter_change_restarts_the_beats_at_its_bar() {
        let m = Meter {
            time: TimeMap::new(
                PPQ,
                &[(1, Sig { num: 4, den: 4 }), (2, Sig { num: 6, den: 8 })],
            )
            .into(),
            ..plain(48.0)
        };
        let l = lines(&quantize(480), &m, 3840, 3840 + 2880);
        assert_eq!(tier(&l, GridTier::Bar), [3840, 3840 + 2880]);
        assert_eq!(tier(&l, GridTier::Beat), [3840 + 1440]);
    }

    #[test]
    fn the_button_adds_what_the_lines_show_only_when_it_is_coarser_than_the_snap() {
        let t = |grid| Snap {
            triplet: true,
            ..quantize(grid)
        };
        // 1/64 note at 16 px a beat: 1 px apart, so the lines show the 1/16 note.
        assert_eq!(visible(&quantize(60), &plain(16.0), 0), Some(480.0));
        assert_eq!(visible(&quantize(60), &plain(64.0), 0), Some(120.0));
        // 1/64t at 48 px: the 1/16t shows (8 px).
        assert_eq!(visible(&t(40), &plain(48.0), 0), Some(160.0));
        // 1/16 at 16 px: 4 px, so the eighths show.
        assert_eq!(visible(&quantize(240), &plain(16.0), 0), Some(480.0));
        // The snap's own value shows: nothing is added.
        assert_eq!(visible(&quantize(240), &plain(48.0), 0), None);
        assert_eq!(visible(&quantize(60), &plain(128.0), 0), None);
        // A bar's grid and frames add nothing.
        assert_eq!(visible(&quantize(960), &plain(2.0), 0), Some(3840.0));
        let mode = |mode| Snap {
            mode,
            ..Snap::default()
        };
        assert_eq!(visible(&mode(SnapMode::Bar), &plain(2.0), 0), None);
        assert_eq!(visible(&mode(SnapMode::Frames), &plain(2.0), 0), None);
        // Even the beats too narrow (7/8 at the widest zoom): the 1/16 doubles
        // to the quarter, the last quarter gives way to the bar, so the half.
        assert_eq!(
            visible(&quantize(240), &at(&sig(7, 8), 8.0), 0),
            Some(1920.0)
        );
        // In 5/8 only the bar holds every point that shows.
        assert_eq!(
            visible(&quantize(240), &at(&sig(5, 8), 8.0), 0),
            Some(2400.0)
        );
        // The meter at the view's left edge decides: 6/8 from bar 2 splits in eighths.
        let m = Meter {
            time: TimeMap::new(
                PPQ,
                &[(1, Sig { num: 4, den: 4 }), (2, Sig { num: 6, den: 8 })],
            )
            .into(),
            ..plain(20.0)
        };
        assert_eq!(visible(&quantize(60), &m, 0), Some(480.0));
        assert_eq!(visible(&quantize(60), &m, 3840), Some(480.0));
        assert_eq!(visible(&quantize(240), &at(&m, 24.0), 3840), Some(480.0));
        // The auto step is the finest value 8 px apart: it always shows.
        for beat_px in [8.0, 16.0, 48.0, 128.0, 838.0, 4096.0] {
            assert_eq!(
                visible(&Snap::default(), &plain(beat_px), 0),
                None,
                "{beat_px}"
            );
        }
    }

    /// Each value of the grid list, with and without the triplet.
    fn menu() -> Vec<Snap> {
        let whole = Tick::from(PPQ) * 4;
        let mut out = Vec::new();
        for d in [1, 2, 4, 8, 16, 32, 64] {
            out.push(quantize(whole / d));
            out.push(Snap {
                triplet: true,
                ..quantize(whole * 2 / (3 * d))
            });
        }
        out
    }

    /// The points every `step` inside the bar from `s` to `e`.
    fn points(s: Tick, e: Tick, step: Tick) -> Vec<Tick> {
        (1..).map(|k| s + k * step).take_while(|&t| t < e).collect()
    }

    #[test]
    fn a_triplet_quarter_grid_draws_its_own_lines_from_each_bar() {
        // 1/4t (640): the beats (960) miss every other point, so the value shows itself.
        let m = plain(48.0);
        let l = lines(&quantize_t(640), &m, 0, 3840 * 2);
        let fine = tier(&l, GridTier::Fine);
        for t in [640, 1280, 2560, 3200, 3840 + 640] {
            assert!(fine.contains(&t), "{t}: {fine:?}");
        }
        assert_eq!(visible(&quantize_t(640), &m, 0), None);
        // At 16 px a beat the points by the beats give way: the half notes show.
        assert_eq!(visible(&quantize_t(640), &plain(16.0), 0), Some(1920.0));
        // 1/2t and 1/1t too.
        let l = lines(&quantize_t(1280), &m, 0, 3840);
        assert_eq!(tier(&l, GridTier::Fine), [1280, 2560]);
        assert_eq!(visible(&quantize_t(1280), &m, 0), None);
        assert_eq!(visible(&quantize_t(2560), &m, 0), None);
    }

    #[test]
    fn six_eight_quarter_and_half_grids_draw_their_own_lines() {
        let m = at(&sig(6, 8), 48.0);
        let l = lines(&quantize(960), &m, 0, 2880);
        assert_eq!(tier(&l, GridTier::Fine), [960, 1920]);
        assert_eq!(tier(&l, GridTier::Beat), [1440]);
        assert_eq!(visible(&quantize(960), &m, 0), None);
        assert_eq!(visible(&quantize(960), &at(&m, 24.0), 0), None);
        let l = lines(&quantize(1920), &m, 0, 2880);
        assert_eq!(tier(&l, GridTier::Fine), [1920]);
        assert_eq!(visible(&quantize(1920), &m, 0), None);
    }

    #[test]
    fn seven_eight_at_the_widest_zoom_draws_the_quarters_it_snaps_to() {
        // 8 px a quarter: the eighth beats are 4 px and do not show.
        let m = at(&sig(7, 8), 8.0);
        let l = lines(&Snap::default(), &m, 0, 3360);
        assert!(tier(&l, GridTier::Beat).is_empty());
        assert_eq!(tier(&l, GridTier::Fine), [960, 1920]);
        // The last quarter is 4 px before the bar and gives way: the button says so.
        assert_eq!(visible(&quantize(960), &m, 0), Some(1920.0));
        // 自動 finds no value 8 px apart, keeps 1/4 and says the same.
        assert_eq!(Snap::default().step(&m), 960);
        assert_eq!(visible(&Snap::default(), &m, 0), Some(1920.0));
        // 5/8 keeps only the first quarter; the bar is the finest that shows.
        assert_eq!(
            visible(&quantize(960), &at(&sig(5, 8), 8.0), 0),
            Some(2400.0)
        );
    }

    #[test]
    fn every_snap_point_has_a_line_or_the_button_says_what_shows() {
        for m in [plain(1.0), sig(3, 4), sig(6, 8), sig(7, 8), sig(12, 8)] {
            for beat_px in [8.0_f32, 16.0, 48.0, 128.0, 838.0, 4096.0] {
                let m = at(&m, beat_px);
                for snap in menu().into_iter().chain([Snap::default()]) {
                    let step = snap.step(&m);
                    for bar in 1..=2 {
                        let (s, e) = (m.time.bar_start(bar), m.time.bar_start(bar + 1));
                        let shown: Vec<Tick> =
                            lines(&snap, &m, s, e).iter().map(|l| l.tick).collect();
                        let every = match visible(&snap, &m, s) {
                            None => step,
                            Some(v) => {
                                assert!(v > step as f64, "{beat_px} {snap:?}: {v}");
                                v as Tick
                            }
                        };
                        for t in points(s, e, every) {
                            assert!(shown.contains(&t), "{beat_px} {snap:?} {t}: {shown:?}");
                            assert_eq!((t - s) % step, 0, "{beat_px} {snap:?} {t}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_seconds_show_from_a_1_64_note_256_px_wide() {
        // A 1/64 note is a 16th of a beat: from 4096 px a beat.
        assert!(!time_on(&plain(4095.0)));
        assert!(time_on(&plain(4096.0)));
        assert!(times(&plain(4095.0), 0, 960).is_none());
        // 1, 2 or 5 times a power of ten, the first at least as wide as asked.
        assert_eq!(time_step(8192.0, 12.0), 0.002);
        assert_eq!(time_step(8192.0, 96.0), 0.02);
        assert_eq!(time_step(288_000.0, 12.0), 0.00005);
        assert_eq!(time_step(1.0, 12.0), 20.0);
        assert_eq!(time_step(0.1, 12.0), 10.0);
        // 144000 px a beat at 120 bpm: 288000 px a second, so 50 µs are 14.4 px.
        let m = plain(144_000.0);
        let t = times(&m, 0, 4).unwrap();
        let gap = t.lines[1].at - t.lines[0].at;
        assert!((m.px(gap) - 14.4).abs() < 1e-3, "{}", m.px(gap));
        // The lines sit between ticks: 50 µs is 0.096 of a tick.
        assert!((t.lines[1].at - 0.096).abs() < 1e-9);
        // The ruler numbers every 0.5 ms (144 px): four decimals.
        assert_eq!(t.labels[1].1, "0:00.0005");
    }

    #[test]
    fn the_ruler_writes_the_time_with_the_step_s_decimals() {
        assert_eq!(time_label(9.3765, 0.0005), "0:09.3765");
        assert_eq!(time_label(9.376, 0.002), "0:09.376");
        assert_eq!(time_label(9.0, 0.1), "0:09.000");
        assert_eq!(time_label(75.5, 0.000_01), "1:15.50000");
        assert_eq!(time_label(59.99996, 0.0001), "1:00.0000");
    }

    #[test]
    fn the_seconds_follow_a_tempo_change() {
        use xirius_core::id::TempoId;
        use xirius_core::model::TempoPoint;
        let pt = |n, tick, bpm| TempoPoint {
            id: TempoId(n),
            tick,
            bpm,
            ramp: false,
        };
        // 120 bpm for one second (1920 ticks), then 60 bpm.
        let m = Meter {
            tempo: TempoMap::new(PPQ, &[pt(1, 0, 120.0), pt(2, 1920, 60.0)]).into(),
            ..plain(4096.0)
        };
        // 4096 px a second at 60 bpm, the slowest on screen: lines every 5 ms,
        // so past the change they stay 12 px apart and the numbers 96 px.
        let t = times(&m, 1900, 1940).unwrap();
        for (w, gap) in [
            (&t.lines, TIME_LINE_PX),
            (&t.labels.iter().map(|l| l.0).collect(), TIME_LABEL_PX),
        ] {
            for p in w.windows(2) {
                let px = f64::from(m.px(p[1].at - p[0].at));
                assert!(px >= gap - 1e-3, "{px} at {}", p[0].secs);
            }
        }
        let at: Vec<_> = t
            .lines
            .iter()
            .map(|l| (l.secs * 1000.0).round() as i64)
            .collect();
        assert_eq!(at.first(), Some(&990));
        // Past the change a millisecond is half as many ticks.
        let tick = |ms: f64| {
            t.lines
                .iter()
                .find(|l| (l.secs * 1000.0 - ms).abs() < 1e-6)
                .unwrap()
                .at
        };
        assert!((tick(1000.0) - 1920.0).abs() < 1e-6);
        assert!((tick(1005.0) - 1924.8).abs() < 1e-6);
        assert!((tick(995.0) - 1910.4).abs() < 1e-6);
        // The finest note lines take the split's color while the seconds show.
        let l = lines(&quantize(60), &m, 0, 960);
        let under = under_times(&l);
        assert!(under.iter().all(|l| l.tier != GridTier::Fine));
        assert_eq!(under.len(), l.len());
    }

    #[test]
    fn the_view_bounds_the_lines() {
        let l = lines(&quantize(240), &plain(48.0), 1000, 2000);
        assert!(l.iter().all(|l| (1000..=2000).contains(&l.tick)));
        assert_eq!(l.first().map(|l| l.tick), Some(1200));
    }
}
