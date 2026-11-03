//! The audio files a built-in device plays (ENG-077): read off the audio thread by the
//! session and handed to the processor through a pair of single-producer single-consumer
//! rings, the same way plans move in [`crate::engine`].
//!
//! [`FileOutbox`] stays with the session: it pushes the newest [`FileSet`] and frees what the
//! audio thread let go. [`FileInbox`] moves to the audio thread with the processor: `poll`
//! takes the newest set at a block's start and hands a replaced one back, so no file is read
//! and no buffer freed on the audio thread itself.

use std::collections::BTreeMap;
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
use xirius_params::{BuiltinDef, PADS, ParamSpec, file_key, pad_key};

use crate::timeline::AudioData;

/// The sets waiting or handed back, at most. An update that finds the ring full is dropped
/// (the caller can send it again); a set the audio thread cannot hand back is leaked, the
/// lesser harm next to freeing it there.
const QUEUE: usize = 8;

/// A file read for a device: the name it was listed under (a Sampler zone's root note comes
/// from it), its sound, both channels at the mix's rate, and its loudest sample (what
/// Normalize lifts to full scale).
#[derive(Clone)]
pub struct FileSlot {
    pub name: String,
    pub data: Arc<AudioData>,
    pub peak: f32,
}

impl FileSlot {
    /// A slot for `data`, its peak measured here (off the audio thread).
    pub fn new(name: impl Into<String>, data: Arc<AudioData>) -> FileSlot {
        let peak = data
            .left
            .iter()
            .chain(&data.right)
            .fold(0f32, |m, x| m.max(x.abs()));
        FileSlot {
            name: name.into(),
            data,
            peak,
        }
    }
}

/// Where a sample plays from and how (PLG-067): the `start`..`loop`..`normalize` values of
/// `xirius_params`'s per-file or per-pad table, the lengths as shares (0..1) of the file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub start: f64,
    pub end: f64,
    pub looped: bool,
    pub loop_start: f64,
    pub loop_end: f64,
    /// The crossfade at the loop's seam, as a share of the loop.
    pub xfade: f64,
    pub reverse: bool,
    pub normalize: bool,
}

impl Default for Cut {
    fn default() -> Cut {
        Cut {
            start: 0.0,
            end: 1.0,
            looped: false,
            loop_start: 0.35,
            loop_end: 0.85,
            xfade: 0.1,
            reverse: false,
            normalize: false,
        }
    }
}

/// A voice's sound (PLG-067): the envelope, filter and amp values, in their units. The
/// Sampler has one for the device, the Drum Kit one for each pad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voicing {
    pub attack: f64,
    pub decay: f64,
    /// 0..1.
    pub sustain: f64,
    pub release: f64,
    /// 0 lowpass, 1 bandpass, 2 highpass (`xirius_params::FILTER_TYPES`).
    pub ftype: usize,
    pub cutoff: f64,
    /// 0..1.
    pub resonance: f64,
    /// 0..1.
    pub drive: f64,
    pub transpose: f64,
    pub fine: f64,
    pub volume: f64,
    /// -1 (left) ..1 (right).
    pub pan: f64,
    /// How far velocity sets the level, 0..1.
    pub velocity: f64,
}

/// What a Drum Kit pad plays with besides its voice (PLG-068).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LayerMode {
    /// The layer whose velocity range holds the hit's.
    #[default]
    Velocity,
    /// The layers in turn.
    RoundRobin,
    Random,
    /// Every layer at once.
    Stack,
}

/// How a pad answers its key (PLG-068).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Trigger {
    /// Plays from start to end whatever the key does after it (the loop is not used).
    #[default]
    OneShot,
    /// Loops while the key is down.
    Loop,
    /// Plays while the key is down, looping if the sample loops.
    Normal,
    /// One press starts it, the next lets it go.
    Toggle,
}

/// A Drum Kit pad (PLG-068): its voice, sample cut, layers and the zones they are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pad {
    pub voicing: Voicing,
    pub cut: Cut,
    pub layer_mode: LayerMode,
    pub trigger: Trigger,
    /// 0 none, 1 itself, 2.. the group `choke - 1`.
    pub choke: usize,
    pub mute: bool,
    pub solo: bool,
    /// Its layers: `zones[first..first + count]` of the set.
    pub first: usize,
    pub count: usize,
}

/// What a note plays (PLG-067, PLG-068): a slot read with a cut, over a range of keys
/// (Sampler) or of velocities (a Drum Kit pad's layer).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    pub slot: usize,
    pub root: u8,
    pub lo: u8,
    pub hi: u8,
    pub cut: Cut,
    /// What Normalize multiplies the file by (1 without it).
    pub gain: f64,
    /// The pad the zone is a layer of (0 on a Sampler).
    pub pad: usize,
}

/// The files a device was given, in the order the song lists them (`Plugin::samples`), and
/// how they play: the zones and, on a Drum Kit, the pads. A `None` slot is a file that could
/// not be read; the song says so and its zone stays silent. Built off the audio thread by
/// [`FileSet::layout`].
#[derive(Clone, Default)]
pub struct FileSet {
    pub slots: Vec<Option<FileSlot>>,
    /// The Sampler's zones in key order; the Drum Kit's layers, pad by pad.
    pub zones: Vec<Zone>,
    /// The Drum Kit's [`PADS`] pads; empty on a Sampler.
    pub pads: Vec<Pad>,
    /// Whether a pad is soloed (only soloed pads sound then).
    pub soloed: bool,
}

/// Normalize lifts a file at most this far (+24 dB), so near silence stays quiet.
const MOST_LIFT: f64 = 15.85;

impl FileSet {
    /// Whether any slot holds a file.
    pub fn any(&self) -> bool {
        self.slots.iter().any(|s| s.is_some())
    }

    /// The slots laid out for `def` (the Sampler or the Drum Kit) with a song's `values`
    /// (0..1, keyed as `xirius_params::BuiltinDef::resolve` names them; what a key does not
    /// name is its default):
    /// - Sampler: a zone for each slot, its root from its name, the keys between two roots
    ///   split at the middle, the lowest and highest zones reaching the keyboard's ends;
    /// - Drum Kit: each file is a layer of the pad `file<n>.pad` names (the `n`th pad when
    ///   it does not), its velocity range ending at `file<n>.top`. When a pad's tops do not
    ///   rise from one layer to the next, its layers split the velocities evenly.
    pub fn layout(
        def: &'static BuiltinDef,
        values: &BTreeMap<String, f64>,
        slots: Vec<Option<FileSlot>>,
    ) -> FileSet {
        let get = |key: String, spec: &ParamSpec| {
            spec.to_plain(values.get(&key).copied().unwrap_or(spec.default_norm()))
        };
        let file = |i: usize, id: &str| {
            let spec = def.per_file.iter().find(|p| p.id == id)?;
            Some(get(file_key(i, id), spec))
        };
        let cut = |of: &dyn Fn(&str) -> Option<f64>| {
            let d = Cut::default();
            let share = |id, d: f64| of(id).map_or(d, |v| v / 100.0);
            let on = |id| of(id).is_some_and(|v| v >= 0.5);
            Cut {
                start: share("start", d.start),
                end: share("end", d.end),
                looped: on("loop"),
                loop_start: share("loop_start", d.loop_start),
                loop_end: share("loop_end", d.loop_end),
                xfade: share("xfade", d.xfade),
                reverse: on("reverse"),
                normalize: on("normalize"),
            }
        };
        let lift = |slot: &Option<FileSlot>, normalize: bool| match slot {
            Some(s) if normalize && s.peak > 0.0 => (1.0 / f64::from(s.peak)).min(MOST_LIFT),
            _ => 1.0,
        };
        if def.per_pad.is_empty() {
            // The Sampler: zones by root.
            let mut order: Vec<(u8, usize)> = slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.as_ref().map(|s| (root_of(&s.name), i)))
                .collect();
            order.sort();
            let mut zones = Vec::with_capacity(order.len());
            for (k, &(root, slot)) in order.iter().enumerate() {
                let lo = match k {
                    0 => 0,
                    _ => ((u16::from(order[k - 1].0) + u16::from(root)) / 2 + 1).min(127) as u8,
                };
                let hi = match order.get(k + 1) {
                    None => 127,
                    Some(&(next, _)) => ((u16::from(root) + u16::from(next)) / 2) as u8,
                };
                let c = cut(&|id| file(slot, id));
                zones.push(Zone {
                    slot,
                    root,
                    lo: lo.min(hi),
                    hi,
                    cut: c,
                    gain: lift(&slots[slot], c.normalize),
                    pad: 0,
                });
            }
            return FileSet {
                slots,
                zones,
                pads: Vec::new(),
                soloed: false,
            };
        }
        // The Drum Kit: layers by pad, in the order the song lists them.
        let mut pads: Vec<Pad> = (0..PADS)
            .map(|p| {
                let spec = |id: &str| def.per_pad.iter().find(|s| s.id == id);
                let of = |id: &str| spec(id).map(|s| get(pad_key(p, id), s));
                let n = |id: &str| of(id).unwrap_or(0.0).round().max(0.0) as usize;
                Pad {
                    voicing: voicing(&of),
                    cut: cut(&of),
                    layer_mode: match n("layer_mode") {
                        1 => LayerMode::RoundRobin,
                        2 => LayerMode::Random,
                        3 => LayerMode::Stack,
                        _ => LayerMode::Velocity,
                    },
                    trigger: match n("trigger") {
                        1 => Trigger::Loop,
                        2 => Trigger::Normal,
                        3 => Trigger::Toggle,
                        _ => Trigger::OneShot,
                    },
                    choke: n("choke"),
                    mute: n("mute") > 0,
                    solo: n("solo") > 0,
                    first: 0,
                    count: 0,
                }
            })
            .collect();
        let mut layers: Vec<(usize, usize, f64)> = slots
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let pad = def.file_pad(values, i);
                (pad, i, file(i, "top").unwrap_or(127.0))
            })
            .collect();
        layers.sort_by_key(|&(pad, i, _)| (pad, i));
        let mut zones = Vec::with_capacity(layers.len());
        let mut k = 0;
        while k < layers.len() {
            let pad = layers[k].0;
            let end = layers[k..]
                .iter()
                .position(|l| l.0 != pad)
                .map_or(layers.len(), |n| k + n);
            let mine = &layers[k..end];
            let count = mine.len();
            let rising = mine.windows(2).all(|w| w[0].2 < w[1].2);
            let p = &mut pads[pad];
            p.first = zones.len();
            p.count = count;
            let mut lo = 0u8;
            for (j, &(_, slot, top)) in mine.iter().enumerate() {
                let hi = if j + 1 == count {
                    127
                } else if rising {
                    top.round().clamp(f64::from(lo), 126.0) as u8
                } else {
                    (((j + 1) * 128 + count / 2) / count)
                        .saturating_sub(1)
                        .min(126) as u8
                };
                zones.push(Zone {
                    slot,
                    root: 60,
                    lo,
                    hi,
                    cut: p.cut,
                    gain: lift(&slots[slot], p.cut.normalize),
                    pad,
                });
                lo = hi.saturating_add(1).min(127);
            }
            k = end;
        }
        let soloed = pads.iter().any(|p| p.solo);
        FileSet {
            slots,
            zones,
            pads,
            soloed,
        }
    }
}

/// A [`Voicing`] from the values `of` reads by id (the default where it has none).
pub fn voicing(of: &dyn Fn(&str) -> Option<f64>) -> Voicing {
    let v = |id: &str, d: f64| of(id).unwrap_or(d);
    Voicing {
        attack: v("attack", 2.0),
        decay: v("decay", 1000.0),
        sustain: v("sustain", 100.0) * 0.01,
        release: v("release", 300.0),
        ftype: v("ftype", 0.0).round().clamp(0.0, 2.0) as usize,
        cutoff: v("cutoff", 20_000.0),
        resonance: v("resonance", 0.0) * 0.01,
        drive: v("drive", 0.0) * 0.01,
        transpose: v("transpose", 0.0),
        fine: v("fine", 0.0),
        volume: v("volume", 0.0),
        pan: v("pan", 0.0) * 0.01,
        velocity: v("velocity", 80.0) * 0.01,
    }
}

/// The session's end of a device's file channel.
pub struct FileOutbox {
    tx: Producer<Arc<FileSet>>,
    retired: Consumer<Arc<FileSet>>,
}

impl FileOutbox {
    /// Hands `set` to the device at the start of its next block. `false` when the ring is
    /// full; the next send (an edit later) can try again.
    pub fn set(&mut self, set: FileSet) -> bool {
        self.reclaim();
        self.tx.push(Arc::new(set)).is_ok()
    }

    /// Frees the sets the audio thread handed back. Called by [`Self::set`]; on its own for
    /// tests that want the ring empty.
    pub fn reclaim(&mut self) {
        while self.retired.pop().is_ok() {}
    }
}

/// The device's end, moved to the audio thread with the processor.
pub struct FileInbox {
    rx: Consumer<Arc<FileSet>>,
    retired: Producer<Arc<FileSet>>,
}

/// The channel a file-taking device and its handle share.
pub fn channel() -> (FileOutbox, FileInbox) {
    let (tx, rx) = RingBuffer::new(QUEUE);
    let (back_tx, back_rx) = RingBuffer::new(QUEUE);
    (
        FileOutbox {
            tx,
            retired: back_rx,
        },
        FileInbox {
            rx,
            retired: back_tx,
        },
    )
}

impl FileInbox {
    /// Moves `cur` to the newest set; `true` when it changed. A replaced one goes back to be
    /// freed off this thread; if the way back is full it is leaked instead (as
    /// `Engine::hand_back` does).
    pub fn poll(&mut self, cur: &mut Option<Arc<FileSet>>) -> bool {
        let mut changed = false;
        while let Ok(set) = self.rx.pop() {
            changed = true;
            if let Some(old) = cur.replace(set)
                && let Err(rtrb::PushError::Full(old)) = self.retired.push(old)
            {
                std::mem::forget(old);
            }
        }
        changed
    }
}

/// The frame `i` of `data` as a voice reads it (silence past its end).
#[inline]
fn at(data: &[f32], i: usize) -> f64 {
    f64::from(data.get(i).copied().unwrap_or(0.0))
}

/// `data` at `pos` frames, between frames by a cubic (Hermite) curve as `Zone::read`.
#[inline]
fn cubic(data: &[f32], pos: f64) -> f64 {
    let i = pos as usize;
    let t = pos - i as f64;
    let xm1 = if i == 0 { 0.0 } else { at(data, i - 1) };
    let (x0, x1, x2) = (at(data, i), at(data, i + 1), at(data, i + 2));
    let c1 = 0.5 * (x1 - xm1);
    let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
    let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
    ((c3 * t + c2) * t + c1) * t + x0
}

impl FileSlot {
    /// Frames in the file.
    pub fn frames(&self) -> usize {
        self.data.frames()
    }

    /// (left, right) at `pos` frames, cubic between the frames.
    #[inline]
    pub fn read(&self, pos: f64) -> (f64, f64) {
        (cubic(&self.data.left, pos), cubic(&self.data.right, pos))
    }

    /// Whether a voice at `pos` has played the whole file (files do not loop yet).
    #[inline]
    pub fn done(&self, pos: f64) -> bool {
        pos >= self.frames() as f64
    }
}

/// The root note a file's name asks for: the last note-name token it has (`c3`, `F#4`, `Bb2`)
/// in the C3 = 60 convention, else C3 itself. Numbers alone do not count (`snare_01` keeps
/// the default).
pub fn root_of(name: &str) -> u8 {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let mut root = 60;
    let bytes = stem.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(note) = note_at(bytes, i) {
            root = note;
            i += 1;
        } else {
            i += 1;
        }
    }
    root
}

/// The note `bytes[i]` starts, if a letter C..B (any case) is followed by an accidental
/// (`#` or `b`) and an octave digit (optionally `-`). `None` mid-word (`cab2` reads nothing).
fn note_at(bytes: &[u8], i: usize) -> Option<u8> {
    let base = match bytes[i].to_ascii_lowercase() {
        b'c' => 0i32,
        b'd' => 2,
        b'e' => 4,
        b'f' => 5,
        b'g' => 7,
        b'a' => 9,
        b'b' => 11,
        _ => return None,
    };
    if i > 0 && bytes[i - 1].is_ascii_alphanumeric() {
        return None;
    }
    let mut j = i + 1;
    let mut semi = base;
    if j < bytes.len() {
        match bytes[j] {
            b'#' => {
                semi += 1;
                j += 1;
            }
            b'b' if j + 1 < bytes.len()
                && (bytes[j + 1].is_ascii_digit() || bytes[j + 1] == b'-') =>
            {
                semi -= 1;
                j += 1;
            }
            _ => {}
        }
    }
    let neg = j < bytes.len() && bytes[j] == b'-';
    if neg {
        j += 1;
    }
    let start = j;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        j += 1;
    }
    if j == start || j - start > 2 {
        return None;
    }
    if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
        // `c3x` is not a note name.
        return None;
    }
    let octave: i32 = stem_i32(&bytes[start..j])? * if neg { -1 } else { 1 };
    let note = (octave + 2) * 12 + semi;
    u8::try_from(note).ok()
}

fn stem_i32(digits: &[u8]) -> Option<i32> {
    std::str::from_utf8(digits).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots(names: &[&str]) -> Vec<Option<FileSlot>> {
        names
            .iter()
            .map(|n| {
                Some(FileSlot::new(
                    *n,
                    Arc::new(AudioData {
                        left: vec![1.0, 2.0, 3.0],
                        right: vec![4.0, -5.0, 6.0],
                    }),
                ))
            })
            .collect()
    }

    fn set(names: &[&str]) -> FileSet {
        FileSet {
            slots: slots(names),
            ..Default::default()
        }
    }

    fn values(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(k, v)| ((*k).to_string(), *v)).collect()
    }

    /// The 0..1 value of `key`'s plain `v` on `def`.
    fn norm(def: &BuiltinDef, key: &str, v: f64) -> (String, f64) {
        let r = def.resolve(key).unwrap();
        (r.key, r.spec.to_norm(v))
    }

    #[test]
    fn the_samplers_zones_split_between_roots_and_keep_their_cut() {
        let def = &xirius_params::SAMPLER;
        let mut v = values(&[]);
        v.extend([
            norm(def, "file2.start", 25.0),
            norm(def, "file2.normalize", 1.0),
        ]);
        let s = FileSet::layout(def, &v, slots(&["hi_C5.wav", "mid_C3.wav", "lo_C2.wav"]));
        let keys: Vec<(usize, u8, u8, u8)> = s
            .zones
            .iter()
            .map(|z| (z.slot, z.root, z.lo, z.hi))
            .collect();
        assert_eq!(keys, [(2, 48, 0, 54), (1, 60, 55, 72), (0, 84, 73, 127)]);
        let mid = s.zones[1];
        assert!((mid.cut.start - 0.25).abs() < 1e-9 && mid.cut.normalize);
        // Normalize lifts the file's peak (6) to full.
        assert!((mid.gain - 1.0 / 6.0).abs() < 1e-6, "{}", mid.gain);
        assert_eq!(s.zones[0].cut, Cut::default());
        assert!(s.pads.is_empty());
    }

    #[test]
    fn the_kits_files_are_layers_of_the_pads_they_name() {
        let def = &xirius_params::DRUM_KIT;
        let mut v = values(&[]);
        v.extend([
            norm(def, "file2.pad", 0.0),
            norm(def, "file1.top", 40.0),
            norm(def, "file4.pad", 9.0),
            norm(def, "file5.pad", 9.0),
            norm(def, "B2.trigger", 1.0),
            norm(def, "B2.solo", 1.0),
        ]);
        let s = FileSet::layout(def, &v, slots(&["a", "b", "c", "d", "e"]));
        assert_eq!(s.pads.len(), PADS);
        let layers = |p: usize| {
            let pad = &s.pads[p];
            s.zones[pad.first..pad.first + pad.count]
                .iter()
                .map(|z| (z.slot, z.lo, z.hi))
                .collect::<Vec<_>>()
        };
        // A1: files 1 and 2, split at file 1's top; A3: file 3 on its own pad.
        assert_eq!(layers(0), [(0, 0, 40), (1, 41, 127)]);
        assert_eq!(layers(1), []);
        assert_eq!(layers(2), [(2, 0, 127)]);
        // A10: tops that do not rise split the velocities evenly.
        assert_eq!(layers(9), [(3, 0, 63), (4, 64, 127)]);
        let b2 = s.pads[17];
        assert_eq!((b2.trigger, b2.solo, b2.count), (Trigger::Loop, true, 0));
        assert!(s.soloed);
        assert_eq!(s.pads[0].trigger, Trigger::OneShot);
    }

    #[test]
    fn the_newest_set_wins_and_the_replaced_one_goes_back() {
        let (mut out, mut inbox) = channel();
        let mut cur: Option<Arc<FileSet>> = None;
        assert!(out.set(set(&["a_C3.wav"])));
        assert!(out.set(set(&["b_E3.wav"])));
        inbox.poll(&mut cur);
        let got = cur.as_ref().unwrap();
        assert_eq!(got.slots[0].as_ref().unwrap().name, "b_E3.wav");
        out.reclaim();
        assert!(out.set(set(&["c_G3.wav"])));
    }

    #[test]
    fn a_file_reads_stereo_between_frames_and_ends() {
        let s = FileSlot::new(
            "x.wav",
            Arc::new(AudioData {
                left: vec![0.0, 1.0, 0.0],
                right: vec![1.0, 0.0, 1.0],
            }),
        );
        assert_eq!(s.frames(), 3);
        let (l, r) = s.read(1.0);
        assert!((l - 1.0).abs() < 1e-6 && r.abs() < 1e-6, "{l} {r}");
        assert!(!s.done(2.9));
        assert!(s.done(3.0));
        assert_eq!(s.read(5.0), (0.0, 0.0));
    }

    #[test]
    fn the_root_comes_from_the_last_note_name_in_the_name() {
        assert_eq!(root_of("piano_C3.wav"), 60);
        assert_eq!(root_of("lead_f#4.wav"), 78);
        assert_eq!(root_of("Bb2 hit.wav"), 58);
        assert_eq!(root_of("a_C3_b_D3.wav"), 62);
        assert_eq!(root_of("snare_01.wav"), 60);
        assert_eq!(root_of("cab2.wav"), 60);
        assert_eq!(root_of("E-1.wav"), 16);
    }
}
