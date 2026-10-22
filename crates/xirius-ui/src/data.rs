//! The made-up project the spikes draw: 200 tracks of clips with pre-reduced
//! peaks, and one MIDI part with 10,000 notes.

use crate::notes::NoteSet;
use crate::peaks::{PeakPyramid, SAMPLE_RATE, synth_base};

pub const TRACK_COUNT: usize = 200;
pub const NOTE_COUNT: usize = 10_000;
/// Length of the arrangement in seconds.
pub const ARRANGEMENT_S: f64 = 240.0;
/// The MIDI part is 160 bars of 4/4.
pub const PART_BEATS: u32 = 640;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    pub start_s: f64,
    pub len_s: f64,
    /// Where in the track's source the clip starts.
    pub src_s: f64,
}

impl Clip {
    pub fn end_s(&self) -> f64 {
        self.start_s + self.len_s
    }
}

pub struct Track {
    pub name: String,
    pub color: usize,
    pub peaks: PeakPyramid,
    /// Sorted by start, not overlapping.
    pub clips: Vec<Clip>,
}

impl Track {
    /// Clips that overlap `[from_s, to_s)`.
    pub fn visible_clips(&self, from_s: f64, to_s: f64) -> impl Iterator<Item = &Clip> {
        let first = self.clips.partition_point(|c| c.end_s() <= from_s);
        self.clips[first..]
            .iter()
            .take_while(move |c| c.start_s < to_s)
    }
}

pub struct Project {
    pub tracks: Vec<Track>,
    pub notes: NoteSet,
}

impl Project {
    pub fn synth() -> Project {
        let tracks = (0..TRACK_COUNT)
            .map(|i| {
                let seed = 0x5EED_0000 + i as u64;
                let peaks = PeakPyramid::from_base(synth_base(seed, ARRANGEMENT_S));
                Track {
                    name: format!("Track {:03}", i + 1),
                    color: i % 10,
                    peaks,
                    clips: synth_clips(seed),
                }
            })
            .collect();
        Project {
            tracks,
            notes: NoteSet::synth(NOTE_COUNT, PART_BEATS),
        }
    }

    /// Total number of clips, for the report.
    pub fn clip_count(&self) -> usize {
        self.tracks.iter().map(|t| t.clips.len()).sum()
    }

    /// Bytes held by the peak pyramids, for the report.
    pub fn peak_bytes(&self) -> usize {
        self.tracks
            .iter()
            .map(|t| {
                (0..t.peaks.level_count())
                    .map(|l| t.peaks.level(l).len() * 2)
                    .sum::<usize>()
            })
            .sum()
    }
}

/// 6 to 13 clips per track, with gaps, inside the arrangement.
fn synth_clips(seed: u64) -> Vec<Clip> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 11) as f64 / (1u64 << 53) as f64
    };
    let count = 6 + (next() * 8.0) as usize;
    let slot = ARRANGEMENT_S / count as f64;
    (0..count)
        .map(|i| {
            let len = slot * (0.45 + 0.5 * next());
            let start = i as f64 * slot + (slot - len) * next();
            Clip {
                start_s: start,
                len_s: len,
                src_s: start,
            }
        })
        .collect()
}

/// Seconds in one peak-pyramid sample position.
pub fn secs_to_samples(s: f64) -> f64 {
    s * SAMPLE_RATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_are_ordered_and_inside_the_arrangement() {
        for seed in 0..50 {
            let clips = synth_clips(seed);
            assert!(clips.len() >= 6);
            for pair in clips.windows(2) {
                assert!(
                    pair[0].end_s() <= pair[1].start_s + 1e-9,
                    "overlap in seed {seed}"
                );
            }
            let last = clips.last().unwrap();
            assert!(last.end_s() <= ARRANGEMENT_S + 1e-9);
            assert!(clips[0].start_s >= 0.0);
        }
    }

    #[test]
    fn visible_clips_match_a_full_scan() {
        let track = Track {
            name: String::new(),
            color: 0,
            peaks: PeakPyramid::from_base(synth_base(1, 10.0)),
            clips: synth_clips(9),
        };
        for (from, to) in [
            (0.0, 5.0),
            (100.0, 130.0),
            (239.0, 300.0),
            (50.0, 50.0),
            (0.0, 1e9),
        ] {
            let fast: Vec<_> = track.visible_clips(from, to).copied().collect();
            let slow: Vec<_> = track
                .clips
                .iter()
                .copied()
                .filter(|c| c.start_s < to && c.end_s() > from)
                .collect();
            assert_eq!(fast, slow, "{from}..{to}");
        }
    }

    #[test]
    fn the_project_has_the_promised_size() {
        let p = Project::synth();
        assert_eq!(p.tracks.len(), 200);
        assert_eq!(p.notes.len(), 10_000);
        assert!(p.clip_count() > 1000);
        assert!(p.peak_bytes() > 10_000_000);
    }
}
