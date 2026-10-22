//! The spatial index (UI-036): the notes of a clip that touch a span of ticks, found without
//! looking at the others.
//!
//! The intervals are sorted by start and laid out as an implicit balanced tree (each range's
//! middle is its node); every node keeps the latest end under it. A query skips a whole side
//! when nothing under it ends after the span begins, or when the node already starts after the
//! span ends, so it costs `O(log n + k)` for `k` hits. The roll builds one per clip and keeps
//! it until the song's snapshot changes.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use xirius_core::id::NoteId;
use xirius_core::model::Note;
use xirius_core::time::Tick;

/// Intervals `[start, end)` over positions `0..n` of a list, searched by span.
#[derive(Clone, Debug, Default)]
pub struct IntervalIndex {
    /// `(start, end, position in the list)`, sorted by start.
    items: Vec<(Tick, Tick, u32)>,
    /// For the node at `m`: the latest end among the items of its range.
    latest_end: Vec<Tick>,
}

impl IntervalIndex {
    pub fn new(spans: impl Iterator<Item = (Tick, Tick)>) -> IntervalIndex {
        let mut items: Vec<(Tick, Tick, u32)> = spans
            .enumerate()
            .map(|(i, (start, end))| (start, end.max(start + 1), i as u32))
            .collect();
        items.sort_by_key(|&(start, _, i)| (start, i));
        let mut latest_end = vec![Tick::MIN; items.len()];
        fill(&items, &mut latest_end, 0, items.len());
        IntervalIndex { items, latest_end }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The positions of what overlaps `[from, to)` (or touches the point `from` when the span is
    /// empty), in list order, so that what is painted over what stays the same.
    pub fn query(&self, from: Tick, to: Tick) -> Vec<u32> {
        let to = to.max(from + 1);
        let mut hits = Vec::new();
        self.visit(0, self.items.len(), from, to, &mut hits);
        hits.sort_unstable();
        hits
    }

    fn visit(&self, lo: usize, hi: usize, from: Tick, to: Tick, hits: &mut Vec<u32>) {
        if lo >= hi {
            return;
        }
        let m = lo + (hi - lo) / 2;
        // Nothing under this node reaches the span.
        if self.latest_end[m] <= from {
            return;
        }
        self.visit(lo, m, from, to, hits);
        let (start, end, i) = self.items[m];
        // Everything to the right starts later still.
        if start >= to {
            return;
        }
        if end > from {
            hits.push(i);
        }
        self.visit(m + 1, hi, from, to, hits);
    }
}

fn fill(items: &[(Tick, Tick, u32)], latest: &mut [Tick], lo: usize, hi: usize) -> Tick {
    if lo >= hi {
        return Tick::MIN;
    }
    let m = lo + (hi - lo) / 2;
    let left = fill(items, latest, lo, m);
    let right = fill(items, latest, m + 1, hi);
    let top = items[m].1.max(left).max(right);
    latest[m] = top;
    top
}

/// Whether the interval `[start, end)` shows in the span `[from, to)`, an interval or a span of
/// no length counting as one tick wide; the one rule that the index and the full scan answer to.
pub fn overlaps(start: Tick, end: Tick, from: Tick, to: Tick) -> bool {
    start < to.max(from + 1) && end.max(start + 1) > from
}

/// What the roll keeps for one clip: its notes by span, and where each note is in the list.
#[derive(Clone, Debug, Default)]
pub struct ClipIndex {
    spans: IntervalIndex,
    at: HashMap<NoteId, u32>,
}

impl ClipIndex {
    pub fn new(notes: &[Note]) -> ClipIndex {
        ClipIndex {
            spans: IntervalIndex::new(notes.iter().map(|n| (n.start, n.start + n.length))),
            at: notes
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id, i as u32))
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The notes of `notes` (the list this was built from) that show in `[from, to)`, and
    /// `also`: notes a drag shows somewhere else than the song has them. In list order.
    pub fn visible<'a, 'b>(
        &self,
        notes: &'a [Note],
        from: Tick,
        to: Tick,
        also: impl Iterator<Item = &'b NoteId>,
    ) -> Vec<&'a Note> {
        let mut at = self.spans.query(from, to);
        let moved = at.len();
        at.extend(also.filter_map(|id| self.at.get(id).copied()));
        if at.len() > moved {
            at.sort_unstable();
            at.dedup();
        }
        at.into_iter()
            .filter_map(|i| notes.get(i as usize))
            .collect()
    }
}

/// The index of one list of notes, good while the list is the one it was built from.
#[derive(Clone, Debug)]
struct Built {
    /// Held so the list's address cannot be reused while the index is kept.
    notes: Arc<Vec<Note>>,
    index: Arc<ClipIndex>,
}

/// The roll's cache of the index of the clip on screen. A clip's notes are shared between the
/// song's snapshots until the clip is edited, so an edit elsewhere keeps the index.
#[derive(Clone, Debug, Default)]
pub struct NoteCache {
    built: RefCell<Option<Built>>,
    /// How many times an index was built: a frame that changes nothing builds none.
    builds: Cell<u32>,
}

impl NoteCache {
    /// The index of `notes`, built only when they are not the list kept.
    pub fn of(&self, notes: &Arc<Vec<Note>>) -> Arc<ClipIndex> {
        let mut built = self.built.borrow_mut();
        if let Some(b) = built.as_ref()
            && Arc::ptr_eq(&b.notes, notes)
        {
            return b.index.clone();
        }
        let index = Arc::new(ClipIndex::new(notes));
        self.builds.set(self.builds.get() + 1);
        *built = Some(Built {
            notes: notes.clone(),
            index: index.clone(),
        });
        index
    }

    pub fn builds(&self) -> u32 {
        self.builds.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(n: usize) -> Vec<(Tick, Tick)> {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        (0..n)
            .map(|_| {
                let start = (next() % 2_000_000) as Tick;
                let len = if next() % 13 == 0 {
                    (next() % 40_000) as Tick
                } else {
                    (next() % 960) as Tick
                };
                (start, start + len)
            })
            .collect()
    }

    fn scan(spans: &[(Tick, Tick)], from: Tick, to: Tick) -> Vec<u32> {
        spans
            .iter()
            .enumerate()
            .filter(|(_, (s, e))| overlaps(*s, *e, from, to))
            .map(|(i, _)| i as u32)
            .collect()
    }

    #[test]
    fn a_query_finds_what_a_full_scan_finds() {
        let list = spans(20_000);
        let index = IntervalIndex::new(list.iter().copied());
        for (from, to) in [
            (0, 1),
            (0, 2_000_000),
            (1_000_000, 1_003_840),
            (1_999_000, 2_100_000),
            (500, 500),
            (777_777, 777_778),
            (-5_000, 10),
            (3_000_000, 3_001_000),
        ] {
            assert_eq!(
                index.query(from, to),
                scan(&list, from, to),
                "span {from}..{to}"
            );
        }
    }

    #[test]
    fn a_long_note_that_began_far_before_is_found() {
        let list = [(0, 1_000_000), (900_000, 900_010), (5, 6)];
        let index = IntervalIndex::new(list.iter().copied());
        assert_eq!(index.query(500_000, 500_100), [0]);
        assert_eq!(index.query(900_000, 900_005), [0, 1]);
    }

    #[test]
    fn an_empty_list_and_a_zero_length_note() {
        assert!(IntervalIndex::default().query(0, 100).is_empty());
        let index = IntervalIndex::new([(10, 10)].into_iter());
        assert_eq!(index.query(0, 100), [0]);
        assert!(index.query(11, 100).is_empty());
    }

    #[test]
    fn the_hits_keep_the_order_of_the_list() {
        let list = [(300, 400), (100, 200), (200, 300)];
        let index = IntervalIndex::new(list.iter().copied());
        assert_eq!(index.query(0, 1000), [0, 1, 2]);
    }
}
