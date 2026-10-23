//! The audio files a built-in device plays (ENG-077): read off the audio thread by the
//! session and handed to the processor through a pair of single-producer single-consumer
//! rings, the same way plans move in [`crate::engine`].
//!
//! [`FileOutbox`] stays with the session: it pushes the newest [`FileSet`] and frees what the
//! audio thread let go. [`FileInbox`] moves to the audio thread with the processor: `poll`
//! takes the newest set at a block's start and hands a replaced one back, so no file is read
//! and no buffer freed on the audio thread itself.

use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::timeline::AudioData;

/// The sets waiting or handed back, at most. An update that finds the ring full is dropped
/// (the caller can send it again); a set the audio thread cannot hand back is leaked, the
/// lesser harm next to freeing it there.
const QUEUE: usize = 8;

/// A file read for a device: the name it was listed under (the root note comes from it for
/// now) and its sound, both channels at the mix's rate.
#[derive(Clone)]
pub struct FileSlot {
    pub name: String,
    pub data: Arc<AudioData>,
}

/// The files a device was given, in the order the song lists them (`Plugin::samples`). A
/// `None` slot is a file that could not be read; the song says so and the slot stays silent.
#[derive(Clone, Default)]
pub struct FileSet {
    pub slots: Vec<Option<FileSlot>>,
}

impl FileSet {
    /// Whether any slot holds a file.
    pub fn any(&self) -> bool {
        self.slots.iter().any(|s| s.is_some())
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

    fn set(names: &[&str]) -> FileSet {
        FileSet {
            slots: names
                .iter()
                .map(|n| {
                    Some(FileSlot {
                        name: (*n).to_string(),
                        data: Arc::new(AudioData {
                            left: vec![1.0, 2.0, 3.0],
                            right: vec![4.0, 5.0, 6.0],
                        }),
                    })
                })
                .collect(),
        }
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
        let s = FileSlot {
            name: "x.wav".into(),
            data: Arc::new(AudioData {
                left: vec![0.0, 1.0, 0.0],
                right: vec![1.0, 0.0, 1.0],
            }),
        };
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
