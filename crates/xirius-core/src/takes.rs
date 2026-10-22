//! Takes on a track (REC-005, REC-006, REC-014, REC-015, REC-016): clips that lie over one another
//! are the takes of one recording passage. Each take has a row of its own under the track, and
//! the clips that play, the ones not muted, are the comp.
//!
//! This only reads the song; [`crate::commands`] has `take.comp`, which picks the take that plays
//! over a range.

use crate::id::ClipId;
use crate::model::{Clip, ClipBody, Track};
use crate::time::Tick;

/// A place in a stack: which row a clip is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeSlot {
    pub clip: ClipId,
    pub row: usize,
    pub start: Tick,
    pub end: Tick,
    /// Whether the clip plays (it is not muted).
    pub plays: bool,
}

/// Clips of one track that cover one another, from the first start to the last end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeStack {
    pub start: Tick,
    pub end: Tick,
    pub rows: usize,
    pub slots: Vec<TakeSlot>,
}

impl TakeStack {
    /// The slots of one row, left to right.
    pub fn row(&self, row: usize) -> impl Iterator<Item = &TakeSlot> {
        self.slots.iter().filter(move |s| s.row == row)
    }

    /// The slot of `row` under `tick`.
    pub fn at(&self, row: usize, tick: Tick) -> Option<&TakeSlot> {
        self.row(row).find(|s| s.start <= tick && tick < s.end)
    }

    /// The slots that play at `tick`.
    pub fn playing_at(&self, tick: Tick) -> impl Iterator<Item = &TakeSlot> {
        self.slots
            .iter()
            .filter(move |s| s.plays && s.start <= tick && tick < s.end)
    }
}

/// Whether two clips are pieces of one take (one cut in two keeps its take, or for a clip not
/// recorded its name and its file).
pub fn same_take(a: &Clip, b: &Clip) -> bool {
    if a.take.is_some() || b.take.is_some() {
        return a.take == b.take && a.is_midi() == b.is_midi();
    }
    a.name == b.name
        && match (&a.body, &b.body) {
            (ClipBody::Audio { file: x, .. }, ClipBody::Audio { file: y, .. }) => x == y,
            (ClipBody::Midi { .. }, ClipBody::Midi { .. }) => true,
            _ => false,
        }
}

/// Whether `a` and `b` lie over one another as takes do: at least half of the shorter one is
/// covered. A short overlap, the crossfade of two clips in a row, is not.
fn lies_over(a: &Clip, b: &Clip) -> bool {
    let over = (a.start + a.length).min(b.start + b.length) - a.start.max(b.start);
    over > 0 && over * 2 >= a.length.min(b.length)
}

/// The stacks of `t`: groups of two or more clips that overlap, each laid out in rows. A clip
/// that lies over nothing is not in a stack. A take cut in pieces stays in its row, and the stack goes on across the cut.
pub fn stacks(t: &Track) -> Vec<TakeStack> {
    let mut order: Vec<&Clip> = t.clips.iter().filter(|c| c.length > 0).collect();
    order.sort_by_key(|c| (c.start, c.id.0));
    let mut groups: Vec<Vec<&Clip>> = Vec::new();
    for c in order {
        match groups.last_mut() {
            Some(g)
                if g.iter().any(|x| {
                    lies_over(x, c) || (x.start + x.length == c.start && same_take(x, c))
                }) =>
            {
                g.push(c);
            }
            _ => groups.push(vec![c]),
        }
    }
    groups
        .into_iter()
        .filter(|g| g.len() > 1)
        .map(|g| layout(&g))
        .filter(|s| s.rows > 1)
        .collect()
}

fn layout(g: &[&Clip]) -> TakeStack {
    // The end of the last clip in each row, and the clip itself.
    let mut rows: Vec<(Tick, &Clip)> = Vec::new();
    let mut slots = Vec::new();
    for c in g {
        let free = |r: &(Tick, &Clip)| r.0 <= c.start;
        // The row of the piece this one continues, when that row is free.
        let row = rows
            .iter()
            .position(|r| free(r) && r.0 == c.start && same_take(r.1, c))
            .or_else(|| rows.iter().position(free));
        let row = match row {
            Some(i) => {
                rows[i] = (c.start + c.length, c);
                i
            }
            None => {
                rows.push((c.start + c.length, c));
                rows.len() - 1
            }
        };
        slots.push(TakeSlot {
            clip: c.id,
            row,
            start: c.start,
            end: c.start + c.length,
            plays: !c.muted,
        });
    }
    TakeStack {
        start: g.iter().map(|c| c.start).min().unwrap_or(0),
        end: g.iter().map(|c| c.start + c.length).max().unwrap_or(0),
        rows: rows.len(),
        slots,
    }
}

/// Whether `take` (any piece of it) is already the only one that plays over `from..to`: a
/// comp there would cut every take at the ends and change nothing to hear.
pub fn already_plays(t: &Track, take: ClipId, from: Tick, to: Tick) -> bool {
    let Some(c) = t.clips.iter().find(|c| c.id == take) else {
        return false;
    };
    let mut over = t
        .clips
        .iter()
        .filter(|x| x.length > 0 && x.start < to && from < x.start + x.length)
        .peekable();
    over.peek().is_some() && over.all(|x| same_take(c, x) == !x.muted)
}

/// The stack that holds `clip`, if it is in one.
pub fn stack_of(t: &Track, clip: ClipId) -> Option<TakeStack> {
    stacks(t)
        .into_iter()
        .find(|s| s.slots.iter().any(|x| x.clip == clip))
}
