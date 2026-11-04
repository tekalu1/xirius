//! The voice the Sampler and the Drum Kit share (PLG-067, PLG-068): a file read between its
//! start and end (cubic between frames), backwards when it is reversed, round its loop with a
//! crossfade at the seam; through a drive, a resonant state-variable filter (lowpass,
//! bandpass or highpass) and an ADSR (a straight attack, decay and release that fall
//! exponentially, 60 dB over their time); level by velocity, then pan.
//!
//! A [`Shape`] is a [`Voicing`] worked out at a rate: the Sampler makes one for the device
//! when its values move, the Drum Kit one for each pad when a set arrives. Nothing here
//! allocates.

use super::dsp::{Svf, SvfCoefs, db_to_gain};
use super::files::{Cut, FileSlot, Trigger, Voicing, Zone};
use super::probe::VoiceStage;

/// Where an envelope counts as silent (-80 dB).
pub const SILENT: f64 = 1e-4;
/// How long a voice that gives way takes to fade.
pub const FADE_MS: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
    /// Giving way to another note (or choked).
    Fade,
}

/// What a voicing does in a frame at a rate.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shape {
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
    fade: f64,
    filter: Option<SvfCoefs>,
    ftype: usize,
    /// The drive's gain into the curve (0 for none).
    drive: f64,
    /// What transpose and fine multiply the speed by.
    pub speed: f64,
    /// The pan on each side, and the volume when the shape carries it.
    gain_l: f64,
    gain_r: f64,
    spread: f64,
}

impl Shape {
    /// `v` at `rate`. With `volume`, the shape's level includes the voicing's volume (the
    /// Drum Kit's pads); without, the caller ramps it (the Sampler's, which automation moves).
    pub fn new(v: &Voicing, rate: f64, volume: bool) -> Shape {
        let frames = |ms: f64| (ms * 0.001 * rate).max(1.0);
        // 60 dB over the time.
        let fall = |ms: f64| 10f64.powf(-3.0 / frames(ms));
        let resonance = v.resonance.clamp(0.0, 1.0);
        let open = match v.ftype {
            0 => v.cutoff >= 19_999.0 && resonance <= 0.0,
            2 => v.cutoff <= 20.5 && resonance <= 0.0,
            _ => false,
        };
        let pan = v.pan.clamp(-1.0, 1.0);
        let level = if volume { db_to_gain(v.volume) } else { 1.0 };
        Shape {
            attack: 1.0 / frames(v.attack),
            decay: fall(v.decay),
            sustain: v.sustain.clamp(0.0, 1.0),
            release: fall(v.release),
            fade: 1.0 / frames(FADE_MS),
            filter: (!open).then(|| SvfCoefs::lowpass(v.cutoff, resonance, rate)),
            ftype: v.ftype.min(2),
            drive: if v.drive > 0.0 {
                1.0 + 9.0 * v.drive.min(1.0)
            } else {
                0.0
            },
            speed: 2f64.powf((v.transpose + v.fine * 0.01) / 12.0),
            gain_l: level * (1.0 - pan).min(1.0),
            gain_r: level * (1.0 + pan).min(1.0),
            spread: v.velocity.clamp(0.0, 1.0),
        }
    }

    /// The level of a note at `velocity` (0..1): all of it with no spread, its square at the
    /// most.
    pub fn level(&self, velocity: f32) -> f64 {
        f64::from(velocity.clamp(0.0, 1.0)).powf(2.0 * self.spread)
    }
}

/// Where a voice reads, in frames read forwards: a reversed file reads from its end, so its
/// start and loop are mirrored.
#[derive(Clone, Copy, Debug, Default)]
struct Path {
    frames: f64,
    from: f64,
    to: f64,
    loop_from: f64,
    loop_to: f64,
    /// Frames of crossfade before the loop's end.
    xfade: f64,
    reverse: bool,
    looping: bool,
}

impl Path {
    fn new(cut: &Cut, frames: usize, trigger: Trigger) -> Option<Path> {
        let n = frames as f64;
        if frames < 2 {
            return None;
        }
        let at = |share: f64| share.clamp(0.0, 1.0) * n;
        let (mut s, mut e) = (at(cut.start), at(cut.end));
        if e < s {
            std::mem::swap(&mut s, &mut e);
        }
        if e - s < 1.0 {
            return None;
        }
        let (mut ls, mut le) = (at(cut.loop_start).clamp(s, e), at(cut.loop_end).clamp(s, e));
        if le < ls {
            std::mem::swap(&mut ls, &mut le);
        }
        let mut looping = match trigger {
            Trigger::OneShot => false,
            Trigger::Loop => true,
            Trigger::Normal | Trigger::Toggle => cut.looped,
        };
        if looping && le - ls < 2.0 {
            // A Loop pad loops its whole cut when its loop is too short to hold.
            (ls, le) = (s, e);
            looping = trigger == Trigger::Loop && le - ls >= 2.0;
        }
        let (from, to, loop_from, loop_to) = if cut.reverse {
            (n - e, n - s, n - le, n - ls)
        } else {
            (s, e, ls, le)
        };
        Some(Path {
            frames: n,
            from,
            to,
            loop_from,
            loop_to,
            xfade: (cut.xfade.clamp(0.0, 1.0) * (loop_to - loop_from)).min(loop_from),
            reverse: cut.reverse,
            looping,
        })
    }

    /// The file's frame `u` frames along the path.
    #[inline]
    fn read(&self, slot: &FileSlot, u: f64) -> (f64, f64) {
        let p = if self.reverse {
            self.frames - 1.0 - u
        } else {
            u
        };
        if p < 0.0 {
            return (0.0, 0.0);
        }
        slot.read(p)
    }
}

#[derive(Clone, Copy)]
pub struct Voice {
    pub stage: Stage,
    pub pitch: u8,
    /// The slot it reads and the address of that slot's sound: a newer set with the same
    /// sound in the slot keeps the voice playing.
    pub slot: usize,
    pub data: usize,
    /// The Drum Kit pad it plays (0 on the Sampler).
    pub pad: usize,
    pub trigger: Trigger,
    /// When it started, to find the oldest.
    pub born: u64,
    /// Let go while the sustain pedal was down: it releases when the pedal comes up.
    pub held: bool,
    pub env: f64,
    /// How far along the path it reads, and how far it moves a frame before tuning.
    u: f64,
    step: f64,
    /// Velocity and Normalize.
    gain: f64,
    path: Path,
    left: Svf,
    right: Svf,
}

impl Voice {
    pub const IDLE: Voice = Voice {
        stage: Stage::Idle,
        pitch: 0,
        slot: 0,
        data: 0,
        pad: 0,
        trigger: Trigger::Normal,
        born: 0,
        held: false,
        env: 0.0,
        u: 0.0,
        step: 1.0,
        gain: 0.0,
        path: Path {
            frames: 0.0,
            from: 0.0,
            to: 0.0,
            loop_from: 0.0,
            loop_to: 0.0,
            xfade: 0.0,
            reverse: false,
            looping: false,
        },
        left: Svf::ZERO,
        right: Svf::ZERO,
    };

    /// A voice playing `zone` of `slot` for `pitch` at `step` frames a frame, at `level`
    /// (velocity). `None` when the zone's cut leaves nothing to play.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        zone: &Zone,
        slot: &FileSlot,
        pitch: u8,
        step: f64,
        level: f64,
        trigger: Trigger,
        born: u64,
    ) -> Option<Voice> {
        let path = Path::new(&zone.cut, slot.frames(), trigger)?;
        Some(Voice {
            stage: Stage::Attack,
            pitch,
            slot: zone.slot,
            data: data_of(slot),
            pad: zone.pad,
            trigger,
            born,
            held: false,
            env: 0.0,
            u: path.from,
            step,
            gain: level * zone.gain,
            path,
            left: Svf::ZERO,
            right: Svf::ZERO,
        })
    }

    /// Takes a newer set's cut for the same sound, where it reads now kept.
    pub fn recut(&mut self, zone: &Zone, slot: &FileSlot, level: f64) {
        match Path::new(&zone.cut, slot.frames(), self.trigger) {
            Some(path) => {
                if self.path.reverse != path.reverse {
                    self.u = path.frames - 1.0 - self.u;
                }
                self.u = self.u.max(path.from);
                self.path = path;
                self.gain = level * zone.gain;
            }
            None => self.fade(),
        }
    }

    pub fn active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// Lets go: the release starts.
    pub fn release(&mut self) {
        if matches!(self.stage, Stage::Attack | Stage::Decay) {
            self.stage = Stage::Release;
        }
    }

    /// Gives way in [`FADE_MS`].
    pub fn fade(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Fade;
        }
    }

    /// How it shows: its stage, and where it reads as a share of the file.
    pub fn view(&self) -> (VoiceStage, f64) {
        let stage = match self.stage {
            Stage::Attack => VoiceStage::Attack,
            Stage::Decay => VoiceStage::Decay,
            Stage::Release => VoiceStage::Release,
            Stage::Idle | Stage::Fade => VoiceStage::Fade,
        };
        let n = self.path.frames.max(1.0);
        let p = if self.path.reverse {
            n - 1.0 - self.u
        } else {
            self.u
        };
        (stage, (p / n).clamp(0.0, 1.0))
    }

    /// The next frame of the voice through `shape`, its speed times `speed` (bend,
    /// vibrato); `None` once it has ended (it is idle then).
    #[inline]
    pub fn tick(&mut self, slot: &FileSlot, shape: &Shape, speed: f64) -> Option<(f64, f64)> {
        match self.stage {
            Stage::Attack => {
                self.env += shape.attack;
                if self.env >= 1.0 {
                    self.env = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => self.env = shape.sustain + (self.env - shape.sustain) * shape.decay,
            Stage::Release => self.env *= shape.release,
            Stage::Fade => self.env -= shape.fade,
            Stage::Idle => return None,
        }
        if self.env < SILENT && self.stage != Stage::Attack {
            *self = Voice::IDLE;
            return None;
        }
        let p = self.path;
        if p.looping {
            let len = p.loop_to - p.loop_from;
            // One step, however many times a short loop played fast went round (L2-smp-4).
            if self.u >= p.loop_to && len > 0.0 {
                self.u = p.loop_from + (self.u - p.loop_from) % len;
            }
        } else if self.u >= p.to {
            *self = Voice::IDLE;
            return None;
        }
        let (mut l, mut r) = p.read(slot, self.u);
        if p.looping && p.xfade > 0.0 && self.u > p.loop_to - p.xfade {
            // Into the loop's start before the seam, so the jump does not click.
            let t = (self.u - (p.loop_to - p.xfade)) / p.xfade;
            let (l2, r2) = p.read(slot, self.u - (p.loop_to - p.loop_from));
            l += (l2 - l) * t;
            r += (r2 - r) * t;
        }
        self.u += self.step * speed;
        l *= self.gain;
        r *= self.gain;
        if shape.drive > 0.0 {
            let k = shape.drive.sqrt();
            l = (shape.drive * l).tanh() / k;
            r = (shape.drive * r).tanh() / k;
        }
        if let Some(c) = &shape.filter {
            let pick = |(lp, bp, hp): (f64, f64, f64)| match shape.ftype {
                1 => bp,
                2 => hp,
                _ => lp,
            };
            l = pick(self.left.tick(c, l));
            r = pick(self.right.tick(c, r));
        }
        Some((l * self.env * shape.gain_l, r * self.env * shape.gain_r))
    }
}

/// The address of a slot's sound, which a newer set keeps for the same file.
pub fn data_of(slot: &FileSlot) -> usize {
    std::sync::Arc::as_ptr(&slot.data) as usize
}
