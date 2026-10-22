//! What a track plays while the transport runs: note events and audio regions placed on a
//! frame timeline counted from the start of the song.
//!
//! A [`Timeline`] is built on the control side (tempo map applied, audio decoded and converted to
//! the engine's sample rate) and handed to the audio thread whole. The audio thread only reads it:
//! a binary search for the notes of the block and a copy loop for the audio. Replacing one gives
//! the old one back through the garbage queue, so nothing is freed on the audio thread.

use std::sync::Arc;

use crate::disk_stream::DiskAudio;
use crate::processor::NoteKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimedNote {
    /// Frames since the start of the song.
    pub frame: u64,
    pub kind: NoteKind,
}

/// Decoded audio, already at the engine's sample rate.
#[derive(Debug, Default)]
pub struct AudioData {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl AudioData {
    pub fn frames(&self) -> usize {
        self.left.len().min(self.right.len())
    }
}

/// A stretch of audio on the timeline.
#[derive(Clone, Debug)]
pub struct Region {
    /// First frame on the timeline.
    pub start: u64,
    pub len: u64,
    /// Where in `data` the region starts.
    pub offset: u64,
    pub gain: f32,
    pub data: Arc<AudioData>,
    /// Fades and the gain line over `gain`.
    pub shape: Shape,
}

/// A file region backed by the disk reader's fixed page ring.
#[derive(Debug)]
pub struct StreamRegion {
    pub start: u64,
    pub len: u64,
    pub offset: u64,
    pub gain: f32,
    pub data: DiskAudio,
    pub shape: Shape,
}

#[derive(Debug)]
struct Streams {
    regions: Vec<StreamRegion>,
    order: Vec<RegionRef>,
    offline: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegionRef {
    Memory(usize),
    Disk(usize),
}

/// A point of a region's gain line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvPoint {
    /// Frames from the start of the region.
    pub frame: u64,
    /// Linear gain.
    pub gain: f32,
}

/// What shapes a region's level besides its gain: a fade at each end and a gain line.
///
/// A fade's gain is `x^exp` where `x` runs from 0 (quiet end) to 1; `exp` 1 is a straight line and
/// 1/2 keeps the power of two crossing fades constant. The line moves in a straight line in dB
/// between points and holds before the first and after the last. With none of it the region is
/// mixed as plainly as before.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub fade_in: u64,
    pub fade_in_exp: f32,
    pub fade_out: u64,
    pub fade_out_exp: f32,
    /// Sorted by `frame`.
    pub envelope: Vec<EnvPoint>,
}

impl Default for Shape {
    fn default() -> Shape {
        Shape {
            fade_in: 0,
            fade_in_exp: 1.0,
            fade_out: 0,
            fade_out_exp: 1.0,
            envelope: Vec::new(),
        }
    }
}

impl Shape {
    pub fn is_plain(&self) -> bool {
        self.fade_in == 0 && self.fade_out == 0 && self.envelope.is_empty()
    }
}

fn fade(x: f32, exp: f32) -> f32 {
    if exp == 1.0 { x } else { x.powf(exp) }
}

/// Walks a gain line frame by frame: the gain steps by a constant ratio inside a segment, which is
/// a straight line in dB, and is worked out afresh at every segment start (and at every block).
struct EnvCursor<'a> {
    env: &'a [EnvPoint],
    until: u64,
    gain: f64,
    ratio: f64,
}

impl<'a> EnvCursor<'a> {
    fn new(env: &'a [EnvPoint]) -> EnvCursor<'a> {
        EnvCursor {
            env,
            until: 0,
            gain: 1.0,
            ratio: 1.0,
        }
    }

    fn seek(&mut self, f: u64) {
        let i = self.env.partition_point(|p| p.frame <= f);
        if i == 0 {
            self.gain = f64::from(self.env[0].gain);
            self.ratio = 1.0;
            self.until = self.env[0].frame;
        } else if i == self.env.len() {
            self.gain = f64::from(self.env[i - 1].gain);
            self.ratio = 1.0;
            self.until = u64::MAX;
        } else {
            let (a, b) = (self.env[i - 1], self.env[i]);
            let span = (b.frame - a.frame) as f64;
            let k = f64::from(b.gain) / f64::from(a.gain);
            self.gain = f64::from(a.gain) * k.powf((f - a.frame) as f64 / span);
            self.ratio = k.powf(1.0 / span);
            self.until = b.frame;
        }
    }

    fn at(&mut self, f: u64) -> f32 {
        if self.env.is_empty() {
            return 1.0;
        }
        if f >= self.until {
            self.seek(f);
        }
        let g = self.gain;
        self.gain *= self.ratio;
        g as f32
    }
}

#[derive(Debug, Default)]
pub struct Timeline {
    /// Sorted by frame; at one frame, note-offs come before note-ons so a repeated pitch retriggers.
    notes: Vec<TimedNote>,
    /// The controller changes, sorted by frame (EDT-004). Kept apart from the notes so a jump can
    /// look back over them cheaply; at one frame they play after the note-offs and before the
    /// note-ons, so a bend is in place when the note starts.
    controls: Vec<TimedNote>,
    regions: Vec<Region>,
    streams: Option<Box<Streams>>,
}

impl Timeline {
    /// `notes` may hold [`NoteKind::Control`] events too; they are put in [`Self::controls`].
    pub fn new(notes: Vec<TimedNote>, regions: Vec<Region>) -> Timeline {
        let (mut controls, mut notes): (Vec<_>, Vec<_>) = notes
            .into_iter()
            .partition(|n| matches!(n.kind, NoteKind::Control { .. }));
        notes.sort_by_key(|n| (n.frame, matches!(n.kind, NoteKind::On { .. })));
        controls.sort_by_key(|n| n.frame);
        Timeline {
            notes,
            controls,
            regions,
            streams: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
            && self.controls.is_empty()
            && self.regions.is_empty()
            && self.streams().is_empty()
    }

    pub fn push_stream(&mut self, region: StreamRegion) {
        self.push_stream_at(self.regions.len(), region);
    }

    /// Inserts before resident region `before`, preserving the song's summation order.
    pub fn push_stream_at(&mut self, before: usize, region: StreamRegion) {
        let resident = self.regions.len();
        let streams = self.streams.get_or_insert_with(|| {
            Box::new(Streams {
                regions: Vec::new(),
                order: (0..resident).map(RegionRef::Memory).collect(),
                offline: false,
            })
        });
        let at = streams
            .order
            .iter()
            .position(|r| *r == RegionRef::Memory(before))
            .unwrap_or(streams.order.len());
        streams
            .order
            .insert(at, RegionRef::Disk(streams.regions.len()));
        streams.regions.push(region);
    }
    pub fn streams(&self) -> &[StreamRegion] {
        self.streams.as_ref().map_or(&[], |s| s.regions.as_slice())
    }
    pub fn stream_health(&self) -> (u64, bool) {
        self.streams()
            .iter()
            .fold((0, false), |(missed, failed), r| {
                (missed + r.data.missed_frames(), failed || r.data.failed())
            })
    }
    pub fn set_offline(&mut self, offline: bool) {
        if let Some(streams) = &mut self.streams {
            streams.offline = offline;
        }
    }

    pub fn controls(&self) -> &[TimedNote] {
        &self.controls
    }

    /// The controller changes with `from <= frame < to`.
    pub fn controls_in(&self, from: u64, to: u64) -> &[TimedNote] {
        let a = self.controls.partition_point(|n| n.frame < from);
        let b = self.controls[a..].partition_point(|n| n.frame < to) + a;
        &self.controls[a..b]
    }

    /// The last change of each controller number before `frame`, by calling `f` once for each.
    /// Does not allocate.
    pub fn controls_before(&self, frame: u64, mut f: impl FnMut(TimedNote)) {
        let end = self.controls.partition_point(|n| n.frame < frame);
        let mut seen = [false; crate::control::CONTROL_NUMBERS];
        for n in self.controls[..end].iter().rev() {
            if let NoteKind::Control { number, .. } = n.kind {
                let i = crate::control::index(number);
                if !seen[i] {
                    seen[i] = true;
                    f(*n);
                }
            }
        }
    }

    pub fn notes(&self) -> &[TimedNote] {
        &self.notes
    }

    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    /// The notes with `from <= frame < to`.
    pub fn notes_in(&self, from: u64, to: u64) -> &[TimedNote] {
        let a = self.notes.partition_point(|n| n.frame < from);
        let b = self.notes[a..].partition_point(|n| n.frame < to) + a;
        &self.notes[a..b]
    }

    /// Adds the audio of every region that overlaps `[pos, pos + left.len())` into the buffers.
    pub fn mix_audio(&self, pos: u64, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len()) as u64;
        let end = pos + n;
        let count = self
            .streams
            .as_ref()
            .map_or(self.regions.len(), |s| s.order.len());
        for index in 0..count {
            let entry = self
                .streams
                .as_ref()
                .map_or(RegionRef::Memory(index), |s| s.order[index]);
            match entry {
                RegionRef::Memory(index) => {
                    let r = &self.regions[index];
                    let r_end = r.start + r.len;
                    if r.start >= end || r_end <= pos {
                        continue;
                    }
                    let from = pos.max(r.start);
                    let to = end.min(r_end);
                    let avail = r.data.frames() as u64;
                    let sh = &r.shape;
                    let plain = sh.is_plain();
                    let mut env = EnvCursor::new(&sh.envelope);
                    for frame in from..to {
                        let src = r.offset + (frame - r.start);
                        if src >= avail {
                            break;
                        }
                        let i = (frame - pos) as usize;
                        let mut g = r.gain;
                        if !plain {
                            let f = frame - r.start;
                            if f < sh.fade_in {
                                g *= fade(f as f32 / sh.fade_in as f32, sh.fade_in_exp);
                            }
                            let to_end = r.len - f;
                            if to_end <= sh.fade_out {
                                g *= fade(to_end as f32 / sh.fade_out as f32, sh.fade_out_exp);
                            }
                            g *= env.at(f);
                        }
                        left[i] += r.data.left[src as usize] * g;
                        right[i] += r.data.right[src as usize] * g;
                    }
                }
                RegionRef::Disk(index) => {
                    let r = &self.streams.as_ref().expect("stream order exists").regions[index];
                    let from = pos.max(r.start);
                    let to = end.min(r.start + r.len);
                    if from >= to {
                        continue;
                    }
                    let mut env = EnvCursor::new(&r.shape.envelope);
                    r.data.mix(
                        r.offset + from - r.start,
                        (to - from) as usize,
                        self.streams.as_ref().is_some_and(|s| s.offline),
                        |i, l, rr| {
                            let f = from + i as u64 - r.start;
                            let mut g = r.gain;
                            if f < r.shape.fade_in {
                                g *= fade(f as f32 / r.shape.fade_in as f32, r.shape.fade_in_exp);
                            }
                            if r.len - f <= r.shape.fade_out {
                                g *= fade(
                                    (r.len - f) as f32 / r.shape.fade_out as f32,
                                    r.shape.fade_out_exp,
                                );
                            }
                            g *= env.at(f);
                            let dst = (from - pos) as usize + i;
                            left[dst] += l * g;
                            right[dst] += rr * g;
                        },
                    );
                }
            }
        }
    }

    /// The frame where the last note ends or the last region ends.
    pub fn end_frame(&self) -> u64 {
        let notes = self.notes.last().map_or(0, |n| n.frame);
        let regions = self
            .regions
            .iter()
            .map(|r| r.start + r.len)
            .max()
            .unwrap_or(0);
        let streams = self
            .streams()
            .iter()
            .map(|r| r.start + r.len)
            .max()
            .unwrap_or(0);
        notes.max(regions).max(streams)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(frame: u64, pitch: u8) -> TimedNote {
        TimedNote {
            frame,
            kind: NoteKind::On {
                pitch,
                velocity: 1.0,
            },
        }
    }

    fn off(frame: u64, pitch: u8) -> TimedNote {
        TimedNote {
            frame,
            kind: NoteKind::Off { pitch },
        }
    }

    #[test]
    fn offs_sort_before_ons_at_one_frame() {
        let t = Timeline::new(vec![on(10, 60), off(10, 60), on(5, 62)], vec![]);
        assert_eq!(t.notes()[0], on(5, 62));
        assert_eq!(t.notes()[1], off(10, 60));
        assert_eq!(t.notes()[2], on(10, 60));
    }

    #[test]
    fn notes_in_is_half_open() {
        let t = Timeline::new(vec![on(0, 60), off(10, 60), on(10, 62), on(20, 64)], vec![]);
        assert_eq!(t.notes_in(0, 10).len(), 1);
        assert_eq!(t.notes_in(10, 20).len(), 2);
        assert_eq!(t.notes_in(20, 21).len(), 1);
        assert_eq!(t.notes_in(21, 100).len(), 0);
    }

    fn ctl(frame: u64, number: u8, value: i32) -> TimedNote {
        TimedNote {
            frame,
            kind: NoteKind::control(number, value),
        }
    }

    #[test]
    fn controls_are_kept_apart_from_the_notes_and_sorted() {
        let t = Timeline::new(vec![ctl(20, 64, 0), on(5, 60), ctl(10, 64, 127)], vec![]);
        assert_eq!(t.notes().len(), 1);
        assert_eq!(t.controls(), &[ctl(10, 64, 127), ctl(20, 64, 0)]);
        assert!(!Timeline::new(vec![ctl(1, 1, 1)], vec![]).is_empty());
        assert_eq!(t.controls_in(10, 20), &[ctl(10, 64, 127)]);
        assert_eq!(t.controls_in(11, 20).len(), 0);
    }

    #[test]
    fn controls_before_gives_the_last_value_of_each_number() {
        let t = Timeline::new(
            vec![
                ctl(10, 64, 127),
                ctl(20, 1, 50),
                ctl(30, 64, 0),
                ctl(40, 1, 99),
            ],
            vec![],
        );
        let mut got = vec![];
        t.controls_before(35, |c| got.push(c));
        got.sort_by_key(|c| c.frame);
        assert_eq!(got, vec![ctl(20, 1, 50), ctl(30, 64, 0)]);
        let mut none = vec![];
        t.controls_before(10, |c| none.push(c));
        assert!(none.is_empty());
    }

    #[test]
    fn audio_is_mixed_at_its_place_and_clipped_to_the_block() {
        let data = Arc::new(AudioData {
            left: (0..100).map(|i| i as f32).collect(),
            right: vec![1.0; 100],
        });
        let t = Timeline::new(
            vec![],
            vec![Region {
                start: 10,
                len: 50,
                offset: 5,
                gain: 2.0,
                data,
                shape: Shape::default(),
            }],
        );
        let (mut l, mut r) = (vec![0.0; 8], vec![0.0; 8]);
        t.mix_audio(6, &mut l, &mut r);
        // frames 6..14: silent until 10, then source frame 5, 6, 7, 8
        assert_eq!(l, vec![0.0, 0.0, 0.0, 0.0, 10.0, 12.0, 14.0, 16.0]);
        assert_eq!(r, vec![0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 2.0, 2.0]);
        let (mut l, mut r) = (vec![0.0; 4], vec![0.0; 4]);
        t.mix_audio(58, &mut l, &mut r);
        // the region ends at frame 60 (source frame 55)
        assert_eq!(l, vec![2.0 * 53.0, 2.0 * 54.0, 0.0, 0.0]);
        assert_eq!(r, vec![2.0, 2.0, 0.0, 0.0]);
    }

    fn constant(v: f32, frames: usize) -> Arc<AudioData> {
        Arc::new(AudioData {
            left: vec![v; frames],
            right: vec![v; frames],
        })
    }

    fn region(start: u64, len: u64, data: Arc<AudioData>, shape: Shape) -> Region {
        Region {
            start,
            len,
            offset: 0,
            gain: 1.0,
            data,
            shape,
        }
    }

    fn render(t: &Timeline, frames: usize, block: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        let mut pos = 0;
        while pos < frames {
            let n = block.min(frames - pos);
            t.mix_audio(pos as u64, &mut l[pos..pos + n], &mut r[pos..pos + n]);
            pos += n;
        }
        // the left channel first, then the right one
        l.extend(r);
        l
    }

    #[test]
    fn a_plain_region_is_mixed_exactly_as_before() {
        let data = constant(0.5, 100);
        let t = Timeline::new(vec![], vec![region(0, 100, data, Shape::default())]);
        assert!(render(&t, 100, 100).iter().all(|x| *x == 0.5));
    }

    #[test]
    fn fades_ramp_the_ends_and_leave_the_middle() {
        let shape = Shape {
            fade_in: 10,
            fade_out: 20,
            ..Shape::default()
        };
        let t = Timeline::new(vec![], vec![region(0, 100, constant(1.0, 100), shape)]);
        let out = render(&t, 100, 100);
        assert_eq!(out[0], 0.0);
        assert!((out[5] - 0.5).abs() < 1e-6);
        assert!((out[10] - 1.0).abs() < 1e-6);
        assert_eq!(out[79], 1.0);
        // the fade-out starts at 1 on its first frame and ends near 0
        assert!((out[80] - 1.0).abs() < 1e-6);
        assert!((out[90] - 0.5).abs() < 1e-6);
        assert!((out[99] - 1.0 / 20.0).abs() < 1e-6);
    }

    #[test]
    fn a_fade_curve_bends_the_ramp() {
        let quiet = Shape {
            fade_in: 100,
            fade_in_exp: 4.0,
            ..Shape::default()
        };
        let loud = Shape {
            fade_in: 100,
            fade_in_exp: 0.25,
            ..Shape::default()
        };
        let q = render(
            &Timeline::new(vec![], vec![region(0, 100, constant(1.0, 100), quiet)]),
            100,
            100,
        );
        let l = render(
            &Timeline::new(vec![], vec![region(0, 100, constant(1.0, 100), loud)]),
            100,
            100,
        );
        assert!((q[50] - 0.5f32.powi(4)).abs() < 1e-6);
        assert!((l[50] - 0.5f32.powf(0.25)).abs() < 1e-6);
    }

    #[test]
    fn two_crossing_fades_keep_the_power_with_the_equal_power_curve() {
        let n = 200;
        let a = region(
            0,
            400,
            constant(1.0, 400),
            Shape {
                fade_out: n,
                fade_out_exp: 0.5,
                ..Shape::default()
            },
        );
        let b = region(
            400 - n,
            400,
            constant(1.0, 400),
            Shape {
                fade_in: n,
                fade_in_exp: 0.5,
                ..Shape::default()
            },
        );
        let out = render(&Timeline::new(vec![], vec![a, b]), 600, 64);
        // the crossing: power is the square of the sum of two signals in phase, so look at each part
        let (mut l1, mut r1) = (vec![0.0f32; 600], vec![0.0f32; 600]);
        let only_a = Timeline::new(
            vec![],
            vec![region(
                0,
                400,
                constant(1.0, 400),
                Shape {
                    fade_out: n,
                    fade_out_exp: 0.5,
                    ..Shape::default()
                },
            )],
        );
        only_a.mix_audio(0, &mut l1, &mut r1);
        for f in (400 - n) as usize..400 {
            let from_b = out[f] - l1[f];
            let p = l1[f] * l1[f] + from_b * from_b;
            assert!((p - 1.0).abs() < 1e-4, "frame {f}: {p}");
        }
    }

    #[test]
    fn the_gain_line_moves_in_decibels_and_holds_at_the_ends() {
        let db = |d: f32| 10f32.powf(d / 20.0);
        let shape = Shape {
            envelope: vec![
                EnvPoint {
                    frame: 100,
                    gain: db(-20.0),
                },
                EnvPoint {
                    frame: 300,
                    gain: db(0.0),
                },
            ],
            ..Shape::default()
        };
        let t = Timeline::new(vec![], vec![region(0, 500, constant(1.0, 500), shape)]);
        let out = render(&t, 500, 500);
        assert!((out[0] - db(-20.0)).abs() < 1e-6, "held before the first");
        assert!((out[100] - db(-20.0)).abs() < 1e-6);
        assert!((out[200] - db(-10.0)).abs() < 1e-5, "{}", out[200]);
        assert!((out[300] - 1.0).abs() < 1e-5);
        assert!((out[499] - 1.0).abs() < 1e-6, "held after the last");
    }

    #[test]
    fn the_result_does_not_depend_on_how_the_blocks_cut_it() {
        let shape = Shape {
            fade_in: 90,
            fade_in_exp: 0.7,
            fade_out: 130,
            fade_out_exp: 1.6,
            envelope: vec![
                EnvPoint {
                    frame: 50,
                    gain: 0.3,
                },
                EnvPoint {
                    frame: 250,
                    gain: 1.4,
                },
                EnvPoint {
                    frame: 400,
                    gain: 0.5,
                },
            ],
        };
        let data = Arc::new(AudioData {
            left: (0..600).map(|i| (i as f32 * 0.37).sin()).collect(),
            right: (0..600).map(|i| (i as f32 * 0.11).cos()).collect(),
        });
        let t = Timeline::new(vec![], vec![region(7, 500, data, shape)]);
        let whole = render(&t, 600, 600);
        for block in [1, 7, 64, 129] {
            let cut = render(&t, 600, block);
            for (i, (a, b)) in whole.iter().zip(&cut).enumerate() {
                assert!((a - b).abs() < 1e-5, "block {block}, frame {i}: {a} vs {b}");
            }
        }
    }
}
