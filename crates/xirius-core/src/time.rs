//! Musical time: ticks, `bar.beat.tick` positions and quantize grids.

use serde::{Deserialize, Serialize};

use crate::error::{CmdError, CmdResult};

/// Ticks since the start of the song (or of a clip, for notes).
pub type Tick = i64;

pub const DEFAULT_PPQ: u32 = 960;

/// A time-signature change. Changes sit on bar lines, so they are kept by bar
/// number (1-based) and never need to be re-aligned when the tempo changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Meter {
    pub num: u32,
    pub den: u32,
}

impl Meter {
    pub fn beat_ticks(self, ppq: u32) -> Tick {
        ppq as Tick * 4 / self.den as Tick
    }
    pub fn bar_ticks(self, ppq: u32) -> Tick {
        self.beat_ticks(ppq) * self.num as Tick
    }
}

/// Which grid line [`TimeMap::on_grid`] takes: the nearest, or the one at or
/// before, or at or after.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Near,
    Before,
    After,
}

/// Converts between ticks and `bar.beat.tick` for a given meter map.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeMap {
    ppq: u32,
    /// (first bar of the segment, first tick of the segment, meter)
    segments: Vec<(u32, Tick, Meter)>,
}

impl TimeMap {
    /// `changes` is (bar, meter) sorted by bar; the first one must be bar 1.
    pub fn new(ppq: u32, changes: &[(u32, Meter)]) -> TimeMap {
        let mut segments: Vec<(u32, Tick, Meter)> = Vec::new();
        for &(bar, meter) in changes {
            let tick = match segments.last() {
                Some(&(b0, t0, m0)) => t0 + (bar - b0) as Tick * m0.bar_ticks(ppq),
                None => 0,
            };
            segments.push((bar, tick, meter));
        }
        if segments.is_empty() {
            segments.push((1, 0, Meter { num: 4, den: 4 }));
        }
        TimeMap { ppq, segments }
    }

    pub fn ppq(&self) -> u32 {
        self.ppq
    }

    /// The meter in force at the start of `bar`.
    pub fn meter_at(&self, bar: u32) -> Meter {
        self.segments
            .iter()
            .rev()
            .find(|s| s.0 <= bar)
            .unwrap_or(&self.segments[0])
            .2
    }

    pub fn bar_start(&self, bar: u32) -> Tick {
        let &(b0, t0, m) = self
            .segments
            .iter()
            .rev()
            .find(|s| s.0 <= bar)
            .unwrap_or(&self.segments[0]);
        t0 + (bar - b0) as Tick * m.bar_ticks(self.ppq)
    }

    /// (bar, beat, tick), bar and beat 1-based, tick 0-based.
    pub fn to_bbt(&self, tick: Tick) -> (u32, u32, Tick) {
        let tick = tick.max(0);
        let &(b0, t0, m) = self
            .segments
            .iter()
            .rev()
            .find(|s| s.1 <= tick)
            .unwrap_or(&self.segments[0]);
        let rel = tick - t0;
        let bar_len = m.bar_ticks(self.ppq);
        let beat_len = m.beat_ticks(self.ppq);
        let bar = b0 + (rel / bar_len) as u32;
        let in_bar = rel % bar_len;
        (bar, (in_bar / beat_len) as u32 + 1, in_bar % beat_len)
    }

    pub fn format(&self, tick: Tick) -> String {
        let (b, beat, t) = self.to_bbt(tick);
        format!("{b}.{beat}.{t}")
    }

    pub fn from_bbt(&self, bar: u32, beat: u32, tick: Tick) -> Tick {
        let &(b0, _, m) = self
            .segments
            .iter()
            .rev()
            .find(|s| s.0 <= bar)
            .unwrap_or(&self.segments[0]);
        let _ = b0;
        self.bar_start(bar) + (beat.max(1) - 1) as Tick * m.beat_ticks(self.ppq) + tick
    }

    /// `t` on a grid of `step` counted from the start of its bar (UI-339), as
    /// the lines, the snap, the quantize and the grid's splits count it. The
    /// bar's end is always a line too, so a step that does not divide the bar
    /// never pushes the next bar's grid off; a step of a bar or more (and
    /// `Tick::MAX`) is the bar. `Near` takes the later line on a tie. Before the
    /// song's start the grid counts from tick 0, `Tick::MAX` being the first bar.
    pub fn on_grid(&self, t: Tick, step: Tick, side: Side) -> Tick {
        if t < 0 {
            let step = if step == Tick::MAX {
                self.meter_at(1).bar_ticks(self.ppq)
            } else {
                step
            };
            if step <= 1 {
                return t;
            }
            return match side {
                Side::Near => (t as f64 / step as f64).round() as Tick * step,
                Side::Before => t.div_euclid(step) * step,
                Side::After => -(-t).div_euclid(step) * step,
            };
        }
        let (bar, _, _) = self.to_bbt(t);
        let (s, e) = (self.bar_start(bar), self.bar_start(bar + 1));
        let step = step.clamp(1, (e - s).max(1));
        let before = s + (t - s) / step * step;
        if before == t {
            return t;
        }
        let after = (before + step).min(e);
        match side {
            Side::Before => before,
            Side::After => after,
            Side::Near if after - t <= t - before => after,
            Side::Near => before,
        }
    }

    /// `t` pulled towards the nearest line of [`TimeMap::on_grid`] by
    /// `strength` percent; in a 4/4 song on a grid that divides the bar, the
    /// same as [`quantize_tick`].
    pub fn quantize(&self, t: Tick, step: Tick, strength: u32) -> Tick {
        let to = self.on_grid(t, step, Side::Near);
        t + ((to - t) as f64 * strength.min(100) as f64 / 100.0).round() as Tick
    }

    /// `1.2.0` (bar.beat.tick), `3.1` (bar.beat) or a bare tick count.
    pub fn parse(&self, text: &str) -> CmdResult<Tick> {
        let text = text.trim();
        if let Ok(t) = text.parse::<Tick>() {
            return Ok(t);
        }
        let parts: Vec<&str> = text.split('.').collect();
        let num = |s: &str| {
            s.parse::<i64>().map_err(|_| {
                CmdError::invalid(format!(
                    "位置 `{text}` を読み取れません（小節.拍.tick または tick 数で指定してください）"
                ))
            })
        };
        let (bar, beat, tick) = match parts.as_slice() {
            [b] => (num(b)?, 1, 0),
            [b, e] => (num(b)?, num(e)?, 0),
            [b, e, t] => (num(b)?, num(e)?, num(t)?),
            _ => {
                return Err(CmdError::invalid(format!(
                    "位置 `{text}` を読み取れません（小節.拍.tick または tick 数で指定してください）"
                )));
            }
        };
        if bar < 1 || beat < 1 {
            return Err(CmdError::invalid("小節と拍は 1 から数えます"));
        }
        Ok(self.from_bbt(bar as u32, beat as u32, tick))
    }
}

/// A quantize grid such as `1/16`, `1/8t` (triplet), `1/4d` (dotted) or a
/// bare tick count.
pub fn parse_grid(text: &str, ppq: u32) -> CmdResult<Tick> {
    let t = text.trim();
    if let Ok(n) = t.parse::<Tick>() {
        return if n > 0 {
            Ok(n)
        } else {
            Err(CmdError::invalid("グリッドは 1 tick 以上にしてください"))
        };
    }
    let (body, suffix) = match t.chars().last() {
        Some(c @ ('t' | 'd')) => (&t[..t.len() - 1], Some(c)),
        _ => (t, None),
    };
    let den = body
        .strip_prefix("1/")
        .and_then(|d| d.parse::<Tick>().ok())
        .filter(|d| *d > 0)
        .ok_or_else(|| {
            CmdError::invalid(format!(
                "グリッド `{text}` を読み取れません（1/16・1/8t・1/4d・tick 数で指定してください）"
            ))
        })?;
    let base = ppq as Tick * 4 / den;
    let ticks = match suffix {
        Some('t') => base * 2 / 3,
        Some('d') => base * 3 / 2,
        _ => base,
    };
    if ticks < 1 {
        return Err(CmdError::invalid("グリッドが細かすぎます"));
    }
    Ok(ticks)
}

/// Snaps `value` towards the nearest multiple of `grid` by `strength` percent
/// (lengths; a position counts from its bar with [`TimeMap::quantize`]).
pub fn quantize_tick(value: Tick, grid: Tick, strength: u32) -> Tick {
    let snapped = ((value as f64 / grid as f64).round() as Tick) * grid;
    value + ((snapped - value) as f64 * strength.min(100) as f64 / 100.0).round() as Tick
}
