//! Synthetic MIDI notes for the piano-roll spike (UI-023).

pub const TICKS_PER_BEAT: u32 = 960;
pub const LOWEST_PITCH: u8 = 24;
pub const HIGHEST_PITCH: u8 = 108;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    pub start: u32,
    pub len: u32,
    pub pitch: u8,
    pub velocity: u8,
}

impl Note {
    pub fn end(&self) -> u32 {
        self.start + self.len
    }
}

/// Notes sorted by start so a viewport can binary-search its first note.
#[derive(Clone, Debug)]
pub struct NoteSet {
    notes: Vec<Note>,
    max_len: u32,
}

impl NoteSet {
    pub fn new(mut notes: Vec<Note>) -> NoteSet {
        notes.sort_by_key(|n| (n.start, n.pitch));
        let max_len = notes.iter().map(|n| n.len).max().unwrap_or(0);
        NoteSet { notes, max_len }
    }

    /// A busy part: `count` notes over `beats` beats, mostly short with some
    /// long pads, in a pitch range that drifts over time.
    pub fn synth(count: usize, beats: u32) -> NoteSet {
        let total = u64::from(beats) * u64::from(TICKS_PER_BEAT);
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let notes = (0..count)
            .map(|i| {
                let start = (i as u64 * total / count as u64) as u32;
                let r = next();
                let len = if r % 11 == 0 {
                    TICKS_PER_BEAT * (2 + (r >> 8) as u32 % 6)
                } else {
                    TICKS_PER_BEAT / 8 * (1 + (r >> 8) as u32 % 4)
                };
                let drift = 60.0 + 18.0 * (start as f64 / total as f64 * 9.0).sin();
                let pitch = (drift as i64 + (r >> 20) as i64 % 25 - 12)
                    .clamp(i64::from(LOWEST_PITCH), i64::from(HIGHEST_PITCH))
                    as u8;
                Note {
                    start,
                    len,
                    pitch,
                    velocity: 50 + ((r >> 32) % 77) as u8,
                }
            })
            .collect();
        NoteSet::new(notes)
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn all(&self) -> &[Note] {
        &self.notes
    }

    /// Notes overlapping `[from, to)` ticks, found without scanning from the
    /// beginning: everything before `from - max_len` cannot overlap.
    pub fn visible(&self, from: u32, to: u32) -> impl Iterator<Item = &Note> {
        let lo_start = from.saturating_sub(self.max_len);
        let first = self.notes.partition_point(|n| n.start < lo_start);
        self.notes[first..]
            .iter()
            .take_while(move |n| n.start < to)
            .filter(move |n| n.end() > from)
    }
}

pub fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synth_makes_the_requested_number_sorted() {
        let set = NoteSet::synth(10_000, 640);
        assert_eq!(set.len(), 10_000);
        assert!(set.all().windows(2).all(|w| w[0].start <= w[1].start));
        assert!(
            set.all()
                .iter()
                .all(|n| (LOWEST_PITCH..=HIGHEST_PITCH).contains(&n.pitch) && n.len > 0)
        );
    }

    #[test]
    fn visible_matches_a_full_scan() {
        let set = NoteSet::synth(10_000, 640);
        let total = 640 * TICKS_PER_BEAT;
        for (from, to) in [
            (0, 1),
            (0, total),
            (total / 2, total / 2 + 5000),
            (total - 100, total + 100),
            (123_456, 123_456),
        ] {
            let fast: Vec<_> = set.visible(from, to).copied().collect();
            let slow: Vec<_> = set
                .all()
                .iter()
                .copied()
                .filter(|n| n.start < to && n.end() > from)
                .collect();
            assert_eq!(fast, slow, "range {from}..{to}");
        }
    }

    #[test]
    fn long_notes_that_started_earlier_are_still_found() {
        let set = NoteSet::new(vec![
            Note {
                start: 0,
                len: 100_000,
                pitch: 60,
                velocity: 100,
            },
            Note {
                start: 90_000,
                len: 10,
                pitch: 61,
                velocity: 100,
            },
        ]);
        let v: Vec<_> = set.visible(50_000, 60_000).collect();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].pitch, 60);
    }

    #[test]
    fn black_keys() {
        let blacks = (60..72).filter(|p| is_black_key(*p)).count();
        assert_eq!(blacks, 5);
        assert!(!is_black_key(60));
        assert!(is_black_key(61));
    }
}
