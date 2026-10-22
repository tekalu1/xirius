//! The metronome (REC-003, ENG-035, ENG-036): a short tone on every beat while the transport
//! plays. It is added to what goes to the device only, after the master was metered and tapped,
//! so it is in neither the master nor an export. A silent output stays silent.
//!
//! The control side works out where the beats fall from the song's tempo and sends a [`Click`]
//! with [`crate::Command::SetClick`]; the audio thread only looks them up and plays them.

/// Where the beats fall, and how loud the click is.
pub struct Click {
    /// Frames from the start of the song of the beats, in order, and each one's place in its bar
    /// (0 is the downbeat).
    beats: Vec<(u64, u32)>,
    /// After the last beat in the list, a beat every this many frames.
    every: f64,
    per_bar: u32,
    gain: f32,
    /// Phase steps per frame of the two tones (downbeat, other beats).
    step: [f32; 2],
    /// The decay per frame and the length of one click, in frames.
    decay: f32,
    len: u32,
    voice: Voice,
}

#[derive(Clone, Copy, Default)]
struct Voice {
    /// Frames left of the click that sounds; 0 when none does.
    left: u32,
    t: u32,
    env: f32,
    step: f32,
}

impl Click {
    /// `beats` as above (sorted here); past the last one, `every` frames apart with `per_bar`
    /// beats in a bar. `gain` is linear.
    pub fn new(
        sample_rate: f64,
        mut beats: Vec<(u64, u32)>,
        every: f64,
        per_bar: u32,
        gain: f32,
    ) -> Click {
        beats.sort_by_key(|b| b.0);
        let len = (sample_rate * 0.035).round().max(1.0) as u32;
        Click {
            beats,
            every: every.max(1.0),
            per_bar: per_bar.max(1),
            gain,
            step: [(1760.0 / sample_rate) as f32, (1320.0 / sample_rate) as f32],
            // Down to about -60 dB over the click.
            decay: (-6.9 / f64::from(len)).exp() as f32,
            len,
            voice: Voice::default(),
        }
    }

    /// The last beat in the list (index and frame); beat 0 at frame 0 when there is none.
    fn last(&self) -> (usize, u64, u32) {
        match self.beats.last() {
            Some(&(frame, place)) => (self.beats.len() - 1, frame, place),
            None => (0, 0, 0),
        }
    }

    /// Beat `i`: its frame and its place in the bar.
    fn beat(&self, i: usize) -> (u64, u32) {
        if let Some(b) = self.beats.get(i) {
            return *b;
        }
        let (from, last, place) = self.last();
        let k = (i - from) as u64;
        let frame = last + (k as f64 * self.every).round() as u64;
        (
            frame,
            ((u64::from(place) + k) % u64::from(self.per_bar)) as u32,
        )
    }

    /// The first beat at or after `frame`.
    fn first_from(&self, frame: u64) -> usize {
        let (from, last, _) = self.last();
        if frame <= last {
            return self.beats.partition_point(|b| b.0 < frame);
        }
        let mut i = from + ((frame - last) as f64 / self.every).ceil() as usize;
        // The rounding in `beat` may put a beat a frame either side.
        while i > from && self.beat(i - 1).0 >= frame {
            i -= 1;
        }
        while self.beat(i).0 < frame {
            i += 1;
        }
        i
    }

    /// Adds the click of the song frames `at..at + out.len()` to `out`. Allocates nothing.
    pub fn render(&mut self, out: &mut [f32], at: u64) {
        let n = out.len();
        let mut next = self.first_from(at);
        let mut i = 0;
        while i < n {
            let (frame, place) = self.beat(next);
            let until = ((frame - at) as usize).clamp(i, n);
            self.sound(&mut out[i..until]);
            if until >= n {
                break;
            }
            self.voice = Voice {
                left: self.len,
                t: 0,
                env: self.gain,
                step: self.step[usize::from(place != 0)],
            };
            i = until;
            next += 1;
        }
    }

    /// The click that sounds, into `out`, for as long as it lasts.
    fn sound(&mut self, out: &mut [f32]) {
        let v = &mut self.voice;
        for s in out.iter_mut() {
            if v.left == 0 {
                return;
            }
            let phase = (v.t as f32 * v.step).fract();
            *s += (phase * std::f32::consts::TAU).sin() * v.env;
            v.env *= self.decay;
            v.t += 1;
            v.left -= 1;
        }
    }
}

/// The bars counted in before a recording starts (REC-003, ENG-036): the song's clock waits
/// while this clicks, then the song plays from where it was. The beats are `beat` frames apart,
/// at the tempo of the place the song starts from.
pub struct CountIn {
    click: Click,
    /// Frames in all, and frames counted so far.
    frames: u64,
    done: u64,
    /// Beats in all, and frames in one.
    beats: u32,
    beat: f64,
}

impl CountIn {
    /// `bars` bars of `per_bar` beats, `beat` frames each; `gain` is linear. At least one beat.
    pub fn new(sample_rate: f64, beat: f64, per_bar: u32, bars: u32, gain: f32) -> CountIn {
        let per_bar = per_bar.max(1);
        let beats = u64::from(per_bar) * u64::from(bars.max(1));
        let beat = beat.max(1.0);
        let list = (0..beats)
            .map(|k| {
                (
                    (k as f64 * beat).round() as u64,
                    (k % u64::from(per_bar)) as u32,
                )
            })
            .collect();
        CountIn {
            click: Click::new(sample_rate, list, beat, per_bar, gain),
            frames: (beats as f64 * beat).round() as u64,
            done: 0,
            beats: beats as u32,
            beat,
        }
    }

    /// The length of the count, in frames.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// The beats counted in all, and the frames of one.
    pub fn beats(&self) -> (u32, f64) {
        (self.beats, self.beat)
    }

    /// Frames still to click.
    pub fn left(&self) -> u64 {
        self.frames.saturating_sub(self.done)
    }

    pub(crate) fn is_over(&self) -> bool {
        self.done >= self.frames
    }

    /// Clicks into `out` as many frames as are left, at most `out.len()`, and returns how many.
    /// Allocates nothing.
    pub(crate) fn render(&mut self, out: &mut [f32]) -> usize {
        let n = out.len().min((self.frames - self.done) as usize);
        self.click.render(&mut out[..n], self.done);
        self.done += n as u64;
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click() -> Click {
        // 120 BPM at 48 kHz: a beat every 24 000 frames, 4 to a bar.
        Click::new(48_000.0, vec![(0, 0), (24_000, 1)], 24_000.0, 4, 0.5)
    }

    fn onsets(c: &mut Click, from: u64, frames: usize, block: usize) -> Vec<u64> {
        let mut out = vec![0.0; frames];
        for (k, chunk) in out.chunks_mut(block).enumerate() {
            c.render(chunk, from + (k * block) as u64);
        }
        // The tone starts at phase 0: its first sample is 0 and the next is not, after a stretch
        // of silence longer than a click.
        (1..frames)
            .filter(|&i| out[i] != 0.0 && out[i.saturating_sub(2000)..i].iter().all(|s| *s == 0.0))
            .map(|i| from + i as u64 - 1)
            .collect()
    }

    #[test]
    fn beats_from_the_list_and_after_it_sound_where_they_fall_whatever_the_block() {
        for block in [64, 256, 1000] {
            let mut c = click();
            assert_eq!(
                onsets(&mut c, 0, 100_000, block),
                vec![0, 24_000, 48_000, 72_000, 96_000],
                "block {block}"
            );
        }
    }

    #[test]
    fn the_downbeat_is_higher_and_a_seek_finds_the_next_beat() {
        let c = click();
        assert_eq!(c.beat(1), (24_000, 1));
        assert_eq!(c.beat(3), (72_000, 3));
        assert_eq!(c.beat(4), (96_000, 0));
        assert_eq!(c.first_from(24_001), 2);
        assert_eq!(c.first_from(96_000), 4);
        assert_eq!(c.first_from(1), 1);
        let mut c = click();
        assert_eq!(onsets(&mut c, 50_000, 50_000, 256), vec![72_000, 96_000]);
    }

    #[test]
    fn a_count_in_clicks_every_beat_of_its_bars_and_then_is_over() {
        let mut c = CountIn::new(48_000.0, 24_000.0, 4, 2, 0.5);
        assert_eq!(c.frames(), 8 * 24_000);
        let mut out = vec![0.0; 300_000];
        let mut done = 0;
        while !c.is_over() {
            let end = (done + 1000).min(out.len());
            done += c.render(&mut out[done..end]);
        }
        assert_eq!(done, 192_000);
        let starts: Vec<usize> = (1..done)
            .filter(|&i| out[i] != 0.0 && out[i.saturating_sub(2000)..i].iter().all(|s| *s == 0.0))
            .map(|i| i - 1)
            .collect();
        assert_eq!(starts, (0..8).map(|k| k * 24_000).collect::<Vec<_>>());
        assert_eq!(c.render(&mut out[..10]), 0);
    }
}
