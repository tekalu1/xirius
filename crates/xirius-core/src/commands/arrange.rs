//! Rearranging the song by its sections (ARR-002, ARR-004).

use super::*;
use crate::command::EditCtx;
use crate::id::{IdKind, SectionId, TempoId, TimeSigId};
use crate::model::{TempoPoint, TimeSigPoint};
use crate::time::Meter;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "section.reorder",
        "セクションを並べ替えます。全トラックのクリップとオートメーションのポイント、マーカー、セクション内のテンポと拍子の変化も一緒に移動します（各セクションは次のセクションまでの空白ごと移動します）。境にまたがるクリップは境で分けます。ループ範囲は動きません",
        vec![
            ArgDef::id("id", IdKind::Section, "セクション").req(),
            ArgDef::int("index", "新しい順番（0 が先頭）")
                .req()
                .range(0.0, 1e6),
        ],
        reorder,
    )]
}

/// A section and the gap after it, up to the next section.
struct Block {
    id: SectionId,
    from: Tick,
    to: Tick,
}

fn reorder(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    reorder_section(p, a.id("id")?, a.i64("index")? as usize)
}

/// Moves a section to place `to` (0 is first); its block takes the clips, lines, markers
/// and tempo of its time along.
pub(super) fn reorder_section(p: &mut Project, id: SectionId, to: usize) -> CmdResult<Output> {
    let mut secs = p.sections.clone();
    secs.sort_by_key(|s| (s.start, s.id));
    let from = secs
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| CmdError::not_found(format!("セクション {id} が見つかりません")))?;
    let to = to.min(secs.len() - 1);
    if from == to {
        return Err(CmdError::new(
            crate::error::ErrorCode::NothingToDo,
            "順番が変わりません",
        ));
    }
    let mut blocks = Vec::new();
    for (i, s) in secs.iter().enumerate() {
        let end = s.start + s.length;
        let next = secs.get(i + 1).map(|n| n.start);
        if next.is_some_and(|n| n < end) {
            return Err(CmdError::conflict(format!(
                "セクション {} と {} が重なっているため、並べ替えられません",
                s.id,
                secs[i + 1].id
            )));
        }
        blocks.push(Block {
            id: s.id,
            from: s.start,
            to: next.unwrap_or(end),
        });
    }
    // a clip across a block's edge is cut there, and a lane keeps its line on both
    // sides of it, so that each block carries what is in it
    let edges: Vec<Tick> = blocks.iter().flat_map(|b| [b.from, b.to]).collect();
    super::time_shift::cut_at(p, &edges)?;
    let moved = blocks.remove(from);
    blocks.insert(to, moved);
    let mut cursor = secs[0].start;
    let mut shifts = Vec::new();
    for b in &blocks {
        shifts.push((b.from, b.to, cursor - b.from));
        if let Some(s) = p.sections.iter_mut().find(|s| s.id == b.id) {
            s.start += cursor - b.from;
        }
        cursor += b.to - b.from;
    }
    p.sections.sort_by_key(|s| (s.start, s.id));
    let shift = |t: Tick| {
        shifts
            .iter()
            .find(|(f, e, _)| (*f..*e).contains(&t))
            .map_or(0, |s| s.2)
    };
    let ids: Vec<_> = p.tracks.keys().copied().collect();
    for tid in ids {
        let t = p.track_mut(tid)?;
        for c in &mut t.clips {
            c.start += shift(c.start);
        }
        for l in &mut t.automation {
            let pts = Arc::make_mut(&mut l.points);
            for x in pts.iter_mut() {
                x.tick += shift(x.tick);
            }
            pts.sort_by_key(|x| x.tick);
        }
    }
    for m in &mut p.markers {
        m.tick += shift(m.tick);
    }
    let end = cursor;
    move_tempo(p, &blocks, &shifts, secs[0].start, end);
    move_meters(p, &shifts, &blocks, secs[0].start, end);
    Ok(Output::msg(format!("{id} を {to} 番目に移動しました")))
}

/// The tempo in force at `t`, a ramp read as a straight line to its next point.
pub(super) fn bpm_at(tempo: &[TempoPoint], t: Tick) -> f64 {
    let i = tempo.iter().rposition(|x| x.tick <= t);
    let Some(i) = i else {
        return tempo.first().map_or(120.0, |x| x.bpm);
    };
    let a = &tempo[i];
    match tempo.get(i + 1) {
        Some(b) if a.ramp && b.tick > a.tick => {
            let f = (t - a.tick) as f64 / (b.tick - a.tick) as f64;
            a.bpm + (b.bpm - a.bpm) * f
        }
        _ => a.bpm,
    }
}

/// Tempo points ride with their block. Each block starts at its new place with
/// the tempo it had at its old start, and the song after the moved stretch
/// keeps the tempo it had there, so nothing outside the blocks plays faster
/// or slower than before.
fn move_tempo(
    p: &mut Project,
    blocks: &[Block],
    shifts: &[(Tick, Tick, Tick)],
    start: Tick,
    end: Tick,
) {
    let old = p.tempo.clone();
    let shift = |t: Tick| {
        shifts
            .iter()
            .find(|(f, e, _)| (*f..*e).contains(&t))
            .map(|s| s.2)
    };
    let mut out: Vec<TempoPoint> = Vec::new();
    for x in &old {
        let mut x = x.clone();
        if (start..end).contains(&x.tick) {
            match shift(x.tick) {
                Some(d) => x.tick += d,
                None => continue,
            }
        }
        out.push(x);
    }
    let mut cursor = start;
    let mut wanted = Vec::new();
    for b in blocks {
        wanted.push((cursor, bpm_at(&old, b.from)));
        cursor += b.to - b.from;
    }
    wanted.push((end, bpm_at(&old, end)));
    let mut added = Vec::new();
    for (at, bpm) in wanted {
        if out.iter().any(|x| x.tick == at) {
            continue;
        }
        let id = TempoId(p.alloc());
        added.push(id);
        out.push(TempoPoint {
            id,
            tick: at,
            bpm,
            ramp: false,
        });
    }
    out.sort_by_key(|x| x.tick);
    // an added point that says what is already in force says nothing
    let mut kept: Vec<TempoPoint> = Vec::new();
    for x in out {
        if let Some(prev) = kept.last()
            && added.contains(&x.id)
            && !prev.ramp
            && (prev.bpm - x.bpm).abs() < 1e-9
        {
            continue;
        }
        kept.push(x);
    }
    p.tempo = kept;
}

/// Meter changes ride with their block the same way. They are kept by bar, so
/// they go to ticks with the old map, move, and come back to bars in order.
fn move_meters(
    p: &mut Project,
    shifts: &[(Tick, Tick, Tick)],
    blocks: &[Block],
    start: Tick,
    end: Tick,
) {
    let map = p.time_map();
    let old = p.timesig.clone();
    let shift = |t: Tick| {
        shifts
            .iter()
            .find(|(f, e, _)| (*f..*e).contains(&t))
            .map(|s| s.2)
    };
    // In a song of one meter there is nothing to keep in step: the bars stay as they are.
    let keep = old.len() > 1;
    let mut at: Vec<MeterAt> = Vec::new();
    for s in &old {
        let mut t = map.bar_start(s.bar);
        if (start..end).contains(&t) {
            match shift(t) {
                Some(d) => t += d,
                None => continue,
            }
        }
        at.push((
            (
                t,
                Some(s.id),
                Meter {
                    num: s.num,
                    den: s.den,
                },
            ),
            keep,
        ));
    }
    // a block's head and the end keep their place in the bars when they were on a bar line
    let on_line = |t: Tick| keep && map.bar_start(map.to_bbt(t).0) == t;
    let mut cursor = start;
    let mut wanted = Vec::new();
    for b in blocks {
        wanted.push((cursor, b.from));
        cursor += b.to - b.from;
    }
    wanted.push((end, end));
    for (t, from) in wanted {
        if !at.iter().any(|x| x.0.0 == t) {
            let m = meter_at(&old, map.to_bbt(from).0);
            at.push(((t, None, m), on_line(from)));
        }
    }
    lay_meters(p, at);
}

/// The meter in force at `bar`.
pub(super) fn meter_at(sigs: &[TimeSigPoint], bar: u32) -> Meter {
    sigs.iter()
        .rev()
        .find(|s| s.bar <= bar)
        .map_or(Meter { num: 4, den: 4 }, |s| Meter {
            num: s.num,
            den: s.den,
        })
}

/// A meter change to lay down: `(tick, kept id, meter)`, and whether it keeps its tick.
pub(super) type MeterAt = ((Tick, Option<TimeSigId>, Meter), bool);

/// The meter changes as `(tick, kept id, meter)`, the old ones going by the song as it is.
pub(super) fn meter_ticks(p: &Project) -> Vec<(Tick, Option<TimeSigId>, Meter)> {
    let map = p.time_map();
    p.timesig
        .iter()
        .map(|s| {
            (
                map.bar_start(s.bar),
                Some(s.id),
                Meter {
                    num: s.num,
                    den: s.den,
                },
            )
        })
        .collect()
}

/// Lays the meter changes down again by bar, in order of their ticks. A change that
/// is not on a bar line goes to the next one; one that says what is in force says nothing.
pub(super) fn rebuild_meters(p: &mut Project, at: Vec<(Tick, Option<TimeSigId>, Meter)>) {
    lay_meters(p, at.into_iter().map(|x| (x, false)).collect());
}

/// [`rebuild_meters`], where a change marked `true` stays on its tick (ARR-004): it stood on
/// a bar line among the music it moved with, so when it lands inside a bar, that bar is cut
/// short to end there. It goes to the next bar line only when no meter fits the short bar.
pub(super) fn lay_meters(p: &mut Project, mut at: Vec<MeterAt>) {
    at.sort_by_key(|x| x.0.0);
    let ppq = p.ppq;
    let mut out: Vec<TimeSigPoint> = Vec::new();
    let (mut bar, mut tick, mut meter) = (1u32, 0 as Tick, None::<Meter>);
    for ((t, id, m), exact) in at {
        if let Some(m0) = meter {
            let len = m0.bar_ticks(ppq);
            let whole = (t - tick).div_euclid(len).max(0);
            let cut = short_bar(m0, t - tick - whole * len, ppq).filter(|_| exact);
            if id.is_none() && m0 == m && cut.is_none() {
                continue;
            }
            match cut {
                // the short bar is a bar of the new meter already
                Some(cut) if cut == m => {
                    bar += whole as u32;
                    tick = t - cut.bar_ticks(ppq);
                }
                Some(cut) => {
                    bar += whole as u32;
                    if out.last().is_some_and(|s| s.bar == bar) {
                        out.pop();
                    }
                    out.push(TimeSigPoint {
                        id: TimeSigId(p.alloc()),
                        bar,
                        num: cut.num,
                        den: cut.den,
                    });
                    bar += 1;
                    tick = t;
                }
                None => {
                    let bars = ((t - tick) + len - 1).div_euclid(len).max(0);
                    bar += bars as u32;
                    tick += bars * len;
                }
            }
        }
        if out.last().is_some_and(|s| s.bar == bar) {
            out.pop();
        }
        meter = Some(m);
        out.push(TimeSigPoint {
            id: id.unwrap_or_else(|| TimeSigId(p.alloc())),
            bar,
            num: m.num,
            den: m.den,
        });
    }
    if out.first().is_some_and(|s| s.bar == 1) {
        p.timesig = out;
    }
}

/// A bar of `ticks` (less than a bar of `m`), in `m`'s beat when it fits, else in the longest
/// beat that does (`timesig.set`'s 1 to 32 and up to 64 beats). None for no bar or none fitting.
fn short_bar(m: Meter, ticks: Tick, ppq: u32) -> Option<Meter> {
    if ticks <= 0 {
        return None;
    }
    std::iter::once(m.den)
        .chain([1, 2, 4, 8, 16, 32])
        .map(|den| Meter { num: 0, den })
        .find_map(|x| {
            let beat = x.beat_ticks(ppq);
            let num = ticks / beat;
            (ticks % beat == 0 && (1..=64).contains(&num)).then_some(Meter {
                num: num as u32,
                den: x.den,
            })
        })
}
