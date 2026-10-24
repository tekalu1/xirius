//! The WAV file a recording is written to (LIC-014, REC-001): 32-bit float, and one that is
//! still readable after the program dies in the middle of a take.
//!
//! A plain WAV says how long it is in its header, and writers fill that in when the file is
//! closed, so a file left open by a crash claims to hold nothing. This one writes the header
//! again once a second of audio (and flushes to the disk), so a crash loses at most the last
//! second; [`repair`] settles what is left. The header has room for an RF64 `ds64` chunk
//! (EBU TECH 3306, as a `JUNK` chunk until it is needed): a take past 4 GiB turns the file into
//! RF64 on the spot, with no copy.
//!
//! ```text
//!  0 RIFF size WAVE            RF64 0xFFFFFFFF WAVE
//! 12 JUNK 28 [28 bytes]        ds64 28 riff:u64 data:u64 frames:u64 table:u32
//! 48 fmt  16 [IEEE float]
//! 72 fact  4 frames
//! 84 data size [samples ...]   0xFFFFFFFF in RF64
//! ```

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

const RIFF_SIZE_AT: u64 = 4;
const DS64_AT: u64 = 12;
const FACT_AT: u64 = 80;
const DATA_SIZE_AT: u64 = 88;
const DATA_AT: u64 = 92;
/// The most a RIFF size or a data size field can say.
const U32_MAX: u64 = 0xFFFF_FFFF;
/// Where a plain WAV stops being able to say how long it is: the data and the headers before it
/// have to fit in 4 GiB less the chunk header of the data and a little room.
const RIFF_LIMIT: u64 = U32_MAX - DATA_AT - 8;

/// A WAV file being written, with its header kept up to date.
pub struct SafeWav {
    reservation: crate::storage::Reservation,
    out: BufWriter<File>,
    channels: u16,
    data_bytes: u64,
    rf64: bool,
    /// Samples written since the header was last written.
    unsynced: u64,
    /// How many samples to write between two header writes.
    every: u64,
    /// Data bytes past which the file turns RF64.
    limit: u64,
}

impl SafeWav {
    /// Creates the file (over any file there) and writes the header of an empty take.
    pub fn create(path: &Path, channels: u16, sample_rate: u32) -> io::Result<SafeWav> {
        let file = File::create(path)?;
        let mut reservation = crate::storage::Reservation::new(
            path,
            u64::from(sample_rate) * u64::from(channels) * 4,
        );
        reservation.ensure(&file, DATA_AT)?;
        let mut out = BufWriter::with_capacity(1 << 16, file);
        let block = channels * 4;
        let mut head = Vec::with_capacity(DATA_AT as usize);
        head.extend_from_slice(b"RIFF");
        head.extend_from_slice(&((DATA_AT as u32 - 8).to_le_bytes()));
        head.extend_from_slice(b"WAVE");
        head.extend_from_slice(b"JUNK");
        head.extend_from_slice(&28u32.to_le_bytes());
        head.extend_from_slice(&[0; 28]);
        head.extend_from_slice(b"fmt ");
        head.extend_from_slice(&16u32.to_le_bytes());
        head.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
        head.extend_from_slice(&channels.to_le_bytes());
        head.extend_from_slice(&sample_rate.to_le_bytes());
        head.extend_from_slice(&(sample_rate * u32::from(block)).to_le_bytes());
        head.extend_from_slice(&block.to_le_bytes());
        head.extend_from_slice(&32u16.to_le_bytes());
        head.extend_from_slice(b"fact");
        head.extend_from_slice(&4u32.to_le_bytes());
        head.extend_from_slice(&0u32.to_le_bytes());
        head.extend_from_slice(b"data");
        head.extend_from_slice(&0u32.to_le_bytes());
        debug_assert_eq!(head.len() as u64, DATA_AT);
        out.write_all(&head)?;
        out.flush()?;
        Ok(SafeWav {
            reservation,
            out,
            channels,
            data_bytes: 0,
            rf64: false,
            unsynced: 0,
            every: u64::from(sample_rate) * u64::from(channels),
            limit: RIFF_LIMIT,
        })
    }

    /// Turns the file RF64 after `bytes` of audio instead of after 4 GiB. For tests, which
    /// cannot write 4 GiB.
    pub fn turning_rf64_after(mut self, bytes: u64) -> SafeWav {
        self.limit = bytes;
        self
    }

    /// Writes the header again after this many samples instead of a second's worth.
    pub fn syncing_every(mut self, samples: u64) -> SafeWav {
        self.every = samples.max(1);
        self
    }

    pub fn write_sample(&mut self, v: f32) -> io::Result<()> {
        self.reservation
            .ensure(self.out.get_ref(), DATA_AT + self.data_bytes + 4)?;
        if !self.rf64 && self.data_bytes + 4 > self.limit {
            self.sync()?;
            self.rf64 = true;
            self.sync()?;
        }
        self.out.write_all(&v.to_le_bytes())?;
        self.data_bytes += 4;
        self.unsynced += 1;
        if self.unsynced >= self.every {
            self.sync()?;
        }
        Ok(())
    }

    /// Writes many samples at once, with the header written again at the same places as it is
    /// for [`SafeWav::write_sample`] and the file turning RF64 at the same sample.
    pub fn write_samples(&mut self, samples: &[f32]) -> io::Result<()> {
        let mut rest = samples;
        while !rest.is_empty() {
            let before_sync = (self.every - self.unsynced).max(1);
            let before_limit = if self.rf64 {
                u64::MAX
            } else {
                self.limit.saturating_sub(self.data_bytes) / 4
            };
            let n = (rest.len() as u64).min(before_sync).min(before_limit) as usize;
            if n == 0 {
                // The next sample is the one that turns the file RF64.
                self.write_sample(rest[0])?;
                rest = &rest[1..];
                continue;
            }
            let (now, later) = rest.split_at(n);
            self.reservation
                .ensure(self.out.get_ref(), DATA_AT + self.data_bytes + 4 * n as u64)?;
            let mut bytes = Vec::with_capacity(now.len() * 4);
            for v in now {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            self.out.write_all(&bytes)?;
            self.data_bytes += bytes.len() as u64;
            self.unsynced += n as u64;
            if self.unsynced >= self.every {
                self.sync()?;
            }
            rest = later;
        }
        Ok(())
    }

    /// Whole frames written.
    pub fn frames(&self) -> u64 {
        self.data_bytes / u64::from(self.channels * 4)
    }

    pub fn is_rf64(&self) -> bool {
        self.rf64
    }
    pub fn disk_space_low(&self) -> bool {
        self.reservation.low()
    }
    pub fn disk_space_unknown(&self) -> bool {
        self.reservation.unknown()
    }
    pub fn reserved_bytes(&self) -> u64 {
        self.reservation.allocated()
    }

    /// Makes what is written readable: pushes it to the disk and writes the sizes in the header.
    pub fn sync(&mut self) -> io::Result<()> {
        self.reservation.check_space();
        self.unsynced = 0;
        self.out.flush()?;
        let block = u64::from(self.channels) * 4;
        let data = self.data_bytes - self.data_bytes % block;
        let frames = data / block;
        let file = self.out.get_mut();
        write_sizes(file, self.rf64, data, frames)?;
        file.seek(SeekFrom::End(0))?;
        file.sync_data()
    }

    /// Closes the file with the full length in its header.
    pub fn finalize(mut self) -> io::Result<()> {
        self.sync()?;
        self.out.get_ref().set_len(DATA_AT + self.data_bytes)
    }
}

/// Cuts a closed file to its first `frames` frames (one it holds fewer of is left as it is).
pub fn shorten(path: &Path, frames: u64) -> io::Result<()> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let mut head = [0u8; DATA_AT as usize];
    file.read_exact(&mut head)?;
    if &head[48..52] != b"fmt " || &head[84..88] != b"data" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "録音のファイルの形ではありません",
        ));
    }
    let rf64 = &head[0..4] == b"RF64";
    let channels = u16::from_le_bytes([head[58], head[59]]);
    let data = frames * u64::from(channels) * 4;
    if DATA_AT + data >= file.metadata()?.len() {
        return Ok(());
    }
    write_sizes(&mut file, rf64, data, frames)?;
    file.set_len(DATA_AT + data)?;
    file.sync_data()
}

/// The header fields that say how long the file is, for `data` bytes of audio.
fn write_sizes(file: &mut File, rf64: bool, data: u64, frames: u64) -> io::Result<()> {
    let riff = DATA_AT - 8 + data;
    if rf64 {
        file.seek(SeekFrom::Start(0))?;
        file.write_all(b"RF64")?;
        file.write_all(&(U32_MAX as u32).to_le_bytes())?;
        file.seek(SeekFrom::Start(DS64_AT))?;
        file.write_all(b"ds64")?;
        file.write_all(&28u32.to_le_bytes())?;
        file.write_all(&riff.to_le_bytes())?;
        file.write_all(&data.to_le_bytes())?;
        file.write_all(&frames.to_le_bytes())?;
        file.write_all(&0u32.to_le_bytes())?;
        file.seek(SeekFrom::Start(FACT_AT))?;
        file.write_all(&(U32_MAX as u32).to_le_bytes())?;
        file.seek(SeekFrom::Start(DATA_SIZE_AT))?;
        file.write_all(&(U32_MAX as u32).to_le_bytes())?;
    } else {
        file.seek(SeekFrom::Start(RIFF_SIZE_AT))?;
        file.write_all(&(riff as u32).to_le_bytes())?;
        file.seek(SeekFrom::Start(FACT_AT))?;
        file.write_all(&(frames as u32).to_le_bytes())?;
        file.seek(SeekFrom::Start(DATA_SIZE_AT))?;
        file.write_all(&(data as u32).to_le_bytes())?;
    }
    Ok(())
}

/// What [`repair`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Repaired {
    pub channels: u16,
    pub sample_rate: u32,
    /// Whole frames of audio in the file.
    pub frames: u64,
    /// The header said something else, and was written again.
    pub fixed: bool,
    /// Bytes at the end that are not a whole frame (the write the crash cut off), cut away.
    pub cut: u64,
}

/// Where the parts of a 32-bit float WAV or RF64 file are.
#[derive(Clone, Copy, Debug)]
struct Layout {
    rf64: bool,
    channels: u16,
    sample_rate: u32,
    data_at: u64,
    fact_at: Option<u64>,
    ds64_at: Option<u64>,
}

/// Walks the chunks up to the audio. `None` for anything but a float WAV.
fn layout(file: &mut (impl Read + Seek)) -> io::Result<Option<Layout>> {
    file.seek(SeekFrom::Start(0))?;
    let mut head = [0u8; 12];
    if file.read_exact(&mut head).is_err() {
        return Ok(None);
    }
    let rf64 = &head[..4] == b"RF64";
    if !(rf64 || &head[..4] == b"RIFF") || &head[8..12] != b"WAVE" {
        return Ok(None);
    }
    let (mut at, mut fmt, mut fact_at, mut ds64_at) = (12u64, None, None, None);
    let data_at = loop {
        let mut c = [0u8; 8];
        file.seek(SeekFrom::Start(at))?;
        if file.read_exact(&mut c).is_err() {
            return Ok(None);
        }
        let size = u64::from(u32::from_le_bytes([c[4], c[5], c[6], c[7]]));
        match &c[..4] {
            b"fmt " => {
                let mut f = [0u8; 16];
                if file.read_exact(&mut f).is_err() {
                    return Ok(None);
                }
                fmt = Some(f);
            }
            b"fact" => fact_at = Some(at + 8),
            b"ds64" => ds64_at = Some(at + 8),
            b"data" => break at + 8,
            _ => {}
        }
        at += 8 + size + (size & 1);
    };
    let Some(f) = fmt else { return Ok(None) };
    let tag = u16::from_le_bytes([f[0], f[1]]);
    let channels = u16::from_le_bytes([f[2], f[3]]);
    let sample_rate = u32::from_le_bytes([f[4], f[5], f[6], f[7]]);
    let bits = u16::from_le_bytes([f[14], f[15]]);
    if tag != 3 || bits != 32 || channels == 0 {
        return Ok(None);
    }
    Ok(Some(Layout {
        rf64,
        channels,
        sample_rate,
        data_at,
        fact_at,
        ds64_at,
    }))
}

/// A 32-bit float RF64 file, which the decoder in use cannot open: its audio read to the end.
#[derive(Clone, Debug, Default)]
pub struct Rf64 {
    pub sample_rate: u32,
    /// Each channel's samples.
    pub channels: Vec<Vec<f32>>,
}

/// The format of a file of ours past 4 GiB. `None` unless it is RF64 holding float samples.
pub fn read_rf64(path: &Path) -> io::Result<Option<Rf64>> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    read_rf64_from(file, len)
}

/// [`read_rf64`] of what `file` reads: a file `len` bytes long, opened already.
pub fn read_rf64_from(mut file: impl Read + Seek, len: u64) -> io::Result<Option<Rf64>> {
    let Some(l) = layout(&mut file)? else {
        return Ok(None);
    };
    if !l.rf64 {
        return Ok(None);
    }
    let block = u64::from(l.channels) * 4;
    let frames = len.saturating_sub(l.data_at) / block;
    file.seek(SeekFrom::Start(l.data_at))?;
    let mut input = io::BufReader::with_capacity(1 << 20, file);
    let mut channels = vec![Vec::with_capacity(frames as usize); l.channels as usize];
    let mut frame = vec![0u8; block as usize];
    for _ in 0..frames {
        input.read_exact(&mut frame)?;
        for (c, v) in channels.iter_mut().zip(frame.as_chunks::<4>().0) {
            c.push(f32::from_le_bytes(*v));
        }
    }
    Ok(Some(Rf64 {
        sample_rate: l.sample_rate,
        channels,
    }))
}

/// Makes a recording that was cut off readable: the header is made to say what the file holds
/// (every whole frame up to its end). Only for files [`SafeWav`] wrote, and for any other WAV
/// whose data chunk runs to the end of the file and fits 4 GiB. Gives `None` for a file that is
/// not a WAV of 32-bit float.
pub fn repair(path: &Path) -> io::Result<Option<Repaired>> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let len = file.metadata()?.len();
    let Some(Layout {
        rf64,
        channels,
        sample_rate,
        data_at,
        fact_at,
        ds64_at,
    }) = layout(&mut file)?
    else {
        return Ok(None);
    };
    let block = u64::from(channels) * 4;
    let held = len.saturating_sub(data_at);
    let data = held - held % block;
    let frames = data / block;

    // What the header says now.
    file.seek(SeekFrom::Start(data_at - 4))?;
    let mut said = [0u8; 4];
    file.read_exact(&mut said)?;
    let said_data = u64::from(u32::from_le_bytes(said));
    let mut says = said_data;
    if rf64 {
        let Some(d) = ds64_at else { return Ok(None) };
        file.seek(SeekFrom::Start(d + 8))?;
        let mut v = [0u8; 8];
        file.read_exact(&mut v)?;
        says = u64::from_le_bytes(v);
    }
    let cut = held - data;
    let want_rf64 = rf64 || data > RIFF_LIMIT;
    let fixed = says != data || cut > 0 || want_rf64 != rf64;
    if fixed {
        if data > RIFF_LIMIT && (data_at != DATA_AT || ds64_at != Some(DS64_AT + 8)) {
            // No room for a ds64 chunk: a file of another writer past 4 GiB.
            return Ok(None);
        }
        file.set_len(data_at + data)?;
        if want_rf64 || (data_at == DATA_AT && fact_at == Some(FACT_AT)) {
            write_sizes(&mut file, want_rf64, data, frames)?;
        } else {
            let riff = (data_at - 8 + data) as u32;
            file.seek(SeekFrom::Start(RIFF_SIZE_AT))?;
            file.write_all(&riff.to_le_bytes())?;
            file.seek(SeekFrom::Start(data_at - 4))?;
            file.write_all(&(data as u32).to_le_bytes())?;
            if let Some(a) = fact_at {
                file.seek(SeekFrom::Start(a))?;
                file.write_all(&(frames as u32).to_le_bytes())?;
            }
        }
        file.sync_data()?;
    }
    Ok(Some(Repaired {
        channels,
        sample_rate,
        frames,
        fixed,
        cut,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of the test's own, removed when dropped (also when the test fails).
    fn dir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("xirius-wavfile-")
            .tempdir()
            .unwrap()
    }

    /// The sample of frame `i`, channel `c`.
    fn v(i: usize, c: usize) -> f32 {
        i as f32 * 0.001 + c as f32
    }

    fn ramp(w: &mut SafeWav, from: usize, frames: usize, channels: usize) {
        for i in from..from + frames {
            for c in 0..channels {
                w.write_sample(v(i, c)).unwrap();
            }
        }
    }

    fn read(path: &Path) -> (hound::WavSpec, Vec<f32>) {
        let mut r = hound::WavReader::open(path).unwrap();
        let spec = r.spec();
        (spec, r.samples::<f32>().map(Result::unwrap).collect())
    }

    #[test]
    fn a_finished_file_reads_back_whole() {
        let d = dir();
        let p = d.path().join("a.wav");
        let mut w = SafeWav::create(&p, 2, 48_000).unwrap();
        ramp(&mut w, 0, 1000, 2);
        assert_eq!(w.frames(), 1000);
        w.finalize().unwrap();
        let (spec, s) = read(&p);
        assert_eq!((spec.channels, spec.sample_rate), (2, 48_000));
        assert_eq!(spec.bits_per_sample, 32);
        assert_eq!(s.len(), 2000);
        assert_eq!(s[2 * 999], v(999, 0));
        assert_eq!(s[2 * 999 + 1], v(999, 1));
        let r = repair(&p).unwrap().unwrap();
        assert_eq!((r.frames, r.fixed, r.cut), (1000, false, 0));
    }

    #[test]
    fn a_file_the_program_died_in_reads_back_up_to_the_last_header_write() {
        let d = dir();
        let p = d.path().join("crash.wav");
        let mut w = SafeWav::create(&p, 1, 1000).unwrap();
        // The header is written every 1000 samples.
        ramp(&mut w, 0, 2500, 1);
        let on_disk_before = std::fs::metadata(&p).unwrap().len();
        // The process dies here: nothing is flushed or finalized (the buffered rest is lost).
        std::mem::forget(w);
        assert!(on_disk_before >= DATA_AT + 2000 * 4);
        let (_, s) = read(&p);
        assert_eq!(
            s.len(),
            2000,
            "the two header writes made 2000 frames readable"
        );
        assert_eq!(s[1999], v(1999, 0));
        // The file may hold more than the header says; repair makes it say so.
        let r = repair(&p).unwrap().unwrap();
        assert!(r.frames >= 2000);
        let (_, s) = read(&p);
        assert_eq!(s.len() as u64, r.frames);
    }

    #[test]
    fn repair_reads_a_header_that_says_nothing_and_cuts_a_half_written_frame() {
        let d = dir();
        let p = d.path().join("zero.wav");
        let mut w = SafeWav::create(&p, 2, 8000).unwrap();
        ramp(&mut w, 0, 300, 2);
        w.finalize().unwrap();
        // As a writer that only fills in the sizes when it closes would leave it: sizes of
        // nothing, and a frame cut in half at the end.
        {
            let mut f = OpenOptions::new().read(true).write(true).open(&p).unwrap();
            f.seek(SeekFrom::Start(RIFF_SIZE_AT)).unwrap();
            f.write_all(&0u32.to_le_bytes()).unwrap();
            f.seek(SeekFrom::Start(DATA_SIZE_AT)).unwrap();
            f.write_all(&0u32.to_le_bytes()).unwrap();
            f.seek(SeekFrom::End(0)).unwrap();
            f.write_all(&[1, 2, 3, 4]).unwrap();
        }
        assert!(hound::WavReader::open(&p).unwrap().len() == 0);
        let r = repair(&p).unwrap().unwrap();
        assert_eq!((r.frames, r.fixed, r.cut), (300, true, 4));
        let (_, s) = read(&p);
        assert_eq!(s.len(), 600);
        assert_eq!(s[598], v(299, 0));
        // Again: nothing left to fix.
        assert!(!repair(&p).unwrap().unwrap().fixed);
    }

    #[test]
    fn something_that_is_not_a_float_wav_is_left_alone() {
        let tmp = dir();
        let d = tmp.path();
        let t = d.join("t.txt");
        std::fs::write(&t, b"not a wav at all, just text").unwrap();
        assert_eq!(repair(&t).unwrap(), None);
        let pcm = d.join("pcm.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&pcm, spec).unwrap();
        w.write_sample(1i16).unwrap();
        w.finalize().unwrap();
        assert_eq!(repair(&pcm).unwrap(), None);
    }

    /// The header of an RF64 file: (riff size, data size, frames, data size field of `data`).
    fn rf64_head(p: &Path) -> (u64, u64, u64, u32) {
        let b = std::fs::read(p).unwrap();
        assert_eq!(&b[..4], b"RF64");
        assert_eq!(&b[4..8], &[0xFF; 4]);
        assert_eq!(&b[12..16], b"ds64");
        let u64_at = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        (
            u64_at(20),
            u64_at(28),
            u64_at(36),
            u32::from_le_bytes(b[88..92].try_into().unwrap()),
        )
    }

    #[test]
    fn writing_a_block_leaves_the_same_file_as_writing_one_sample_at_a_time() {
        let tmp = dir();
        let d = tmp.path();
        let all: Vec<f32> = (0..2500).map(|i| i as f32 * 0.5).collect();
        for (name, limit) in [("plain", 100_000), ("turning", 4000)] {
            let one = d.join(format!("{name}-one.wav"));
            let mut w = SafeWav::create(&one, 2, 1000)
                .unwrap()
                .turning_rf64_after(limit)
                .syncing_every(700);
            for &x in &all {
                w.write_sample(x).unwrap();
            }
            w.finalize().unwrap();
            let blocks = d.join(format!("{name}-blocks.wav"));
            let mut w = SafeWav::create(&blocks, 2, 1000)
                .unwrap()
                .turning_rf64_after(limit)
                .syncing_every(700);
            // Block sizes that cross the sync and the turn at different places.
            for chunk in all.chunks(333) {
                w.write_samples(chunk).unwrap();
            }
            assert_eq!(w.is_rf64(), limit < 10_000);
            w.finalize().unwrap();
            assert_eq!(
                std::fs::read(&one).unwrap(),
                std::fs::read(&blocks).unwrap(),
                "{name}"
            );
        }
    }

    #[test]
    fn a_take_past_the_limit_turns_into_rf64_and_says_its_true_length() {
        let d = dir();
        let p = d.path().join("big.wav");
        let mut w = SafeWav::create(&p, 2, 48_000)
            .unwrap()
            .turning_rf64_after(4000);
        ramp(&mut w, 0, 400, 2); // 3200 bytes: still RIFF
        assert!(!w.is_rf64());
        ramp(&mut w, 400, 600, 2); // 8000 bytes
        assert!(w.is_rf64());
        assert_eq!(w.frames(), 1000);
        w.finalize().unwrap();
        let (riff, data, frames, field) = rf64_head(&p);
        assert_eq!((data, frames, field), (8000, 1000, 0xFFFF_FFFF));
        assert_eq!(riff, DATA_AT - 8 + 8000);
        assert_eq!(std::fs::metadata(&p).unwrap().len(), DATA_AT + 8000);
        // Everything is where the data chunk says.
        let b = std::fs::read(&p).unwrap();
        let sample = |i: usize| {
            let at = DATA_AT as usize + i * 4;
            f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
        };
        assert_eq!(sample(0), 0.0);
        assert_eq!(sample(2 * 399), v(399, 0));
        assert_eq!(sample(2 * 999 + 1), v(999, 1));
        let r = repair(&p).unwrap().unwrap();
        assert_eq!((r.frames, r.fixed), (1000, false));
    }

    /// A take whose input stopped is cut to the frames that came (ENG-040): the header says the
    /// new length, the frames kept are untouched, and a file asked to keep more is left alone.
    #[test]
    fn a_closed_file_is_shortened_to_the_frames_kept() {
        let d = dir();
        let p = d.path().join("cut.wav");
        let mut w = SafeWav::create(&p, 2, 48_000).unwrap();
        ramp(&mut w, 0, 1000, 2);
        w.finalize().unwrap();
        shorten(&p, 2000).unwrap();
        assert_eq!(read(&p).1.len(), 2000);
        shorten(&p, 600).unwrap();
        let (spec, s) = read(&p);
        assert_eq!(spec.channels, 2);
        assert_eq!(s.len(), 1200);
        assert_eq!((s[2 * 599], s[2 * 599 + 1]), (v(599, 0), v(599, 1)));
        assert_eq!(std::fs::metadata(&p).unwrap().len(), DATA_AT + 1200 * 4);
        let r = repair(&p).unwrap().unwrap();
        assert_eq!((r.frames, r.fixed), (600, false));

        let big = d.path().join("cut64.wav");
        let mut w = SafeWav::create(&big, 2, 48_000)
            .unwrap()
            .turning_rf64_after(4000);
        ramp(&mut w, 0, 1000, 2);
        assert!(w.is_rf64());
        w.finalize().unwrap();
        shorten(&big, 700).unwrap();
        let (riff, data, frames, _) = rf64_head(&big);
        assert_eq!((data, frames), (5600, 700));
        assert_eq!(riff, DATA_AT - 8 + 5600);
        let r = read_rf64(&big).unwrap().unwrap();
        assert_eq!(r.channels[0].len(), 700);
        assert_eq!(r.channels[1][699], v(699, 1));

        let text = d.path().join("t.txt");
        std::fs::write(&text, vec![b'x'; 200]).unwrap();
        assert!(shorten(&text, 1).is_err());
        assert_eq!(std::fs::metadata(&text).unwrap().len(), 200);
    }

    #[test]
    fn an_rf64_take_cut_off_by_a_crash_is_repaired_as_rf64() {
        let d = dir();
        let p = d.path().join("bigcrash.wav");
        let mut w = SafeWav::create(&p, 1, 8000)
            .unwrap()
            .turning_rf64_after(2000)
            .syncing_every(100);
        ramp(&mut w, 0, 900, 1);
        std::mem::forget(w);
        let r = repair(&p).unwrap().unwrap();
        assert!(r.frames >= 800 && r.frames <= 900, "{r:?}");
        let (_, data, frames, _) = rf64_head(&p);
        assert_eq!((data, frames), (r.frames * 4, r.frames));
    }
}
