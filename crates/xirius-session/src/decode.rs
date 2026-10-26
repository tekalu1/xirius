//! Reading audio files (LIC-014, ENG-022, ENG-031): WAV, AIFF, FLAC, MP3 and Ogg Vorbis with
//! symphonia, and the one conversion of a file's rate to the song's, with rubato.
//!
//! [`probe`] reads what an import needs without decoding the sound where the container says how
//! long it is: the rate, the length and the tempo the file carries (a WAV's `acid` chunk, or a
//! name such as `Drums 92bpm.wav`). [`read`] decodes a file for the engine; the song's own
//! rate is never changed to suit a file, and nothing else in the session resamples.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rubato::{FftFixedIn, Resampler};
use xirius_engine::AudioData;

/// The extensions [`read`] reads, lower case.
pub const EXTENSIONS: &[&str] = &[
    "wav", "wave", "aif", "aiff", "aifc", "flac", "mp3", "ogg", "oga",
];

/// Whether `path` names a file [`read`] reads, by its extension.
pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// What [`probe`] finds in a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub rate: u32,
    pub channels: u16,
    /// Length in frames at the file's own rate.
    pub frames: u64,
    /// The tempo the file carries, if any.
    pub bpm: Option<f64>,
    /// Where the tempo was read: `"acid"` (the WAV's chunk) or `"name"` (the file's name).
    pub bpm_from: Option<&'static str>,
}

impl Info {
    pub fn seconds(&self) -> f64 {
        if self.rate == 0 {
            0.0
        } else {
            self.frames as f64 / f64::from(self.rate)
        }
    }
}

/// A decoded file at its own rate. A mono file has the same samples on both sides.
#[derive(Clone, Debug, Default)]
pub struct Decoded {
    pub rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub mono: bool,
}

fn unsupported(path: &Path) -> String {
    format!(
        "{} は読み込めない形式です（WAV・AIFF・FLAC・MP3・Ogg Vorbis に対応しています）",
        path.display()
    )
}

/// A file opened by [`open`]: the reader, the audio track's id, its parameters and its length
/// in frames when the container says.
pub(crate) type Opened = (
    Box<dyn symphonia::core::formats::FormatReader>,
    u32,
    symphonia::core::codecs::audio::AudioCodecParameters,
    Option<u64>,
);

/// Opens `path` with symphonia.
pub(crate) fn open(path: &Path) -> Result<Opened, String> {
    if !is_audio(path) {
        return Err(unsupported(path));
    }
    let file =
        std::fs::File::open(path).map_err(|e| format!("{} を開けません: {e}", path.display()))?;
    open_source(path, Box::new(file))
}

/// Opens the bytes of `path` that `source` reads with symphonia; `path` gives the format's
/// hint and the name in a message.
fn open_source(
    path: &Path,
    source: Box<dyn symphonia::core::io::MediaSource>,
) -> Result<Opened, String> {
    use symphonia::core::formats::TrackType;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::io::MediaSourceStream;

    let mss = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(&ext.to_ascii_lowercase());
    }
    let format = symphonia::default::get_probe()
        .probe(&hint, mss, Default::default(), Default::default())
        .map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| format!("{} に音がありません", path.display()))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|c| c.audio())
        .ok_or_else(|| unsupported(path))?
        .clone();
    let (id, frames) = (track.id, track.num_frames);
    Ok((format, id, params, frames))
}

/// A take past 4 GiB, written as RF64 by the recorder (LIC-014): symphonia does not read RF64,
/// so the engine's own reader does, for these files only.
fn rf64(path: &Path) -> Result<Option<Decoded>, String> {
    let mut head = [0u8; 4];
    let is = std::fs::File::open(path)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
        .is_ok()
        && &head == b"RF64";
    if !is {
        return Ok(None);
    }
    let read = xirius_engine::wavfile::read_rf64(path);
    from_rf64(path, read)
}

/// An RF64 take as [`decode`] gives it, from what `source` reads of it (`len` bytes, opened
/// once by the caller). `None` when it is not one.
pub fn decode_rf64_source(
    path: &Path,
    source: impl std::io::Read + std::io::Seek,
    len: u64,
) -> Result<Option<Decoded>, String> {
    from_rf64(path, xirius_engine::wavfile::read_rf64_from(source, len))
}

fn from_rf64(
    path: &Path,
    read: std::io::Result<Option<xirius_engine::wavfile::Rf64>>,
) -> Result<Option<Decoded>, String> {
    let read = read.map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    Ok(read.map(|w| {
        let mono = w.channels.len() == 1;
        let mut c = w.channels.into_iter();
        let left = c.next().unwrap_or_default();
        let right = c.next().unwrap_or_else(|| left.clone());
        Decoded {
            rate: w.sample_rate,
            left,
            right,
            mono,
        }
    }))
}

/// Decodes `path` at its own rate.
pub fn decode(path: &Path) -> Result<Decoded, String> {
    if let Some(d) = rf64(path)? {
        return Ok(d);
    }
    decode_opened(path, open(path)?, &mut |_, _| true)
}

/// Decodes what `source` reads of `path` (opened once by the caller) at its own rate. `seen`
/// is called after each packet with what has been decoded so far and the length in frames the
/// container says, if it does; when it returns false the decoding stops there. An RF64 take is
/// not read this way: [`decode`] and [`decode_rf64_source`] read those.
pub fn decode_source(
    path: &Path,
    source: Box<dyn symphonia::core::io::MediaSource>,
    seen: &mut dyn FnMut(&Decoded, Option<u64>) -> bool,
) -> Result<Decoded, String> {
    if !is_audio(path) {
        return Err(unsupported(path));
    }
    decode_opened(path, open_source(path, source)?, seen)
}

fn decode_opened(
    path: &Path,
    opened: Opened,
    seen: &mut dyn FnMut(&Decoded, Option<u64>) -> bool,
) -> Result<Decoded, String> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::errors::Error;

    let (mut format, id, params, frames) = opened;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default().gapless(true))
        .map_err(|_| unsupported(path))?;
    let mut out = Decoded {
        rate: params.sample_rate.unwrap_or(0),
        mono: params.channels.as_ref().is_some_and(|c| c.count() == 1),
        ..Decoded::default()
    };
    let mut planes: Vec<Vec<f32>> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("{} を読めません: {e}", path.display())),
        };
        if packet.track_id != id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                if buf.spec().rate() != 0 {
                    out.rate = buf.spec().rate();
                }
                buf.copy_to_vecs_planar::<f32>(&mut planes);
                match planes.as_slice() {
                    [mono] => {
                        out.mono = true;
                        out.left.extend_from_slice(mono);
                        out.right.extend_from_slice(mono);
                    }
                    [l, r, ..] => {
                        out.mono = false;
                        out.left.extend_from_slice(l);
                        out.right.extend_from_slice(r);
                    }
                    [] => {}
                }
            }
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("{} をデコードできません: {e}", path.display())),
        }
        if !seen(&out, frames) {
            break;
        }
    }
    if out.rate == 0 {
        return Err(format!("{} のサンプルレートが分かりません", path.display()));
    }
    Ok(out)
}

/// The rate, length and tempo of `path`. The sound is decoded only when the container does not
/// say how long it is.
pub fn probe(path: &Path) -> Result<Info, String> {
    if let Some(d) = rf64(path)? {
        return Ok(Info {
            rate: d.rate,
            channels: if d.mono { 1 } else { 2 },
            frames: d.left.len() as u64,
            bpm: None,
            bpm_from: None,
        });
    }
    let (_, _, params, frames) = open(path)?;
    let (rate, channels, frames) = match (params.sample_rate, frames) {
        (Some(rate), Some(frames)) if rate > 0 => (
            rate,
            params.channels.as_ref().map_or(2, |c| c.count() as u16),
            frames,
        ),
        _ => {
            let d = decode(path)?;
            (d.rate, if d.mono { 1 } else { 2 }, d.left.len() as u64)
        }
    };
    let (bpm, bpm_from) = match acid_tempo(path) {
        Some(b) => (Some(b), Some("acid")),
        None => match name_tempo(path) {
            Some(b) => (Some(b), Some("name")),
            None => (None, None),
        },
    };
    Ok(Info {
        rate,
        channels: channels.max(1),
        frames,
        bpm,
        bpm_from,
    })
}

/// Decodes `path` and converts it to `rate` (the song's). The error is a sentence for the user.
pub fn read(path: &Path, rate: u32) -> Result<AudioData, String> {
    #[cfg(feature = "test-io")]
    xirius_core::io_test::before("prepare_decode", path).map_err(|e| e.to_string())?;
    let d = decode(path)?;
    #[cfg(feature = "test-io")]
    if d.rate != rate {
        xirius_core::io_test::before("prepare_convert", path).map_err(|e| e.to_string())?;
    }
    let (left, right) = convert(d.left, d.right, d.mono, d.rate, rate)?;
    Ok(AudioData { left, right })
}

/// Converts a stereo signal from `from` Hz to `to` Hz with rubato (ENG-022). The length scales
/// with the ratio and the resampler's delay is taken off, so the sound stays where it was.
pub fn convert(
    left: Vec<f32>,
    right: Vec<f32>,
    mono: bool,
    from: u32,
    to: u32,
) -> Result<(Vec<f32>, Vec<f32>), String> {
    if from == to || left.is_empty() {
        return Ok((left, right));
    }
    let len = left.len();
    let want = (len as f64 * f64::from(to) / f64::from(from)).round() as usize;
    let input: Vec<Vec<f32>> = if mono { vec![left] } else { vec![left, right] };
    let channels = input.len();
    let fail = |e: &dyn std::fmt::Display| format!("{from} Hz から {to} Hz に変換できません: {e}");
    let mut r = FftFixedIn::<f32>::new(from as usize, to as usize, 1024, 2, channels)
        .map_err(|e| fail(&e))?;
    let delay = r.output_delay();
    let mut out: Vec<Vec<f32>> = vec![Vec::with_capacity(want + delay + 2048); channels];
    let push = |o: Vec<Vec<f32>>, out: &mut Vec<Vec<f32>>| {
        for (dst, src) in out.iter_mut().zip(o) {
            dst.extend_from_slice(&src);
        }
    };
    let mut pos = 0;
    while pos + r.input_frames_next() <= len {
        let n = r.input_frames_next();
        let chunk: Vec<&[f32]> = input.iter().map(|c| &c[pos..pos + n]).collect();
        push(r.process(&chunk, None).map_err(|e| fail(&e))?, &mut out);
        pos += n;
    }
    if pos < len {
        let rest: Vec<&[f32]> = input.iter().map(|c| &c[pos..]).collect();
        push(
            r.process_partial(Some(&rest), None).map_err(|e| fail(&e))?,
            &mut out,
        );
    }
    while out[0].len() < want + delay {
        push(
            r.process_partial::<&[f32]>(None, None)
                .map_err(|e| fail(&e))?,
            &mut out,
        );
    }
    for c in &mut out {
        c.drain(..delay);
        c.truncate(want);
    }
    let mut it = out.into_iter();
    let left = it.next().unwrap_or_default();
    let right = it.next().unwrap_or_else(|| left.clone());
    Ok((left, right))
}

/// The tempo in a WAV's `acid` chunk (ACIDized loops), unless it is marked as a one-shot.
pub fn acid_tempo(path: &Path) -> Option<f64> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 12];
    f.read_exact(&mut head).ok()?;
    if !(&head[..4] == b"RIFF" || &head[..4] == b"RF64") || &head[8..12] != b"WAVE" {
        return None;
    }
    loop {
        let mut ch = [0u8; 8];
        f.read_exact(&mut ch).ok()?;
        let size = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]);
        if &ch[..4] == b"acid" {
            let mut body = vec![0u8; size.min(64) as usize];
            f.read_exact(&mut body).ok()?;
            if body.len() < 24 {
                return None;
            }
            let flags = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
            let bpm = f32::from_le_bytes([body[20], body[21], body[22], body[23]]);
            let one_shot = flags & 1 != 0;
            return (!one_shot && (20.0..=999.0).contains(&bpm)).then_some(round_bpm(bpm.into()));
        }
        // `data` of an RF64 file says 0xFFFFFFFF; nothing after it is read then.
        if size == u32::MAX {
            return None;
        }
        f.seek(SeekFrom::Current(i64::from(size) + i64::from(size & 1)))
            .ok()?;
    }
}

/// A tempo in the file's name: `92bpm`, `92 BPM`, `92_bpm`, `bpm92`.
pub fn name_tempo(path: &Path) -> Option<f64> {
    let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
    let bytes = stem.as_bytes();
    let number = |s: &str| -> Option<f64> {
        let v: f64 = s.parse().ok()?;
        (30.0..=300.0).contains(&v).then_some(round_bpm(v))
    };
    let mut from = 0;
    while let Some(i) = stem[from..].find("bpm").map(|i| i + from) {
        // Before: digits, maybe a separator.
        let mut end = i;
        while end > 0 && matches!(bytes[end - 1], b' ' | b'_' | b'-') {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && (bytes[start - 1].is_ascii_digit() || bytes[start - 1] == b'.') {
            start -= 1;
        }
        if start < end
            && let Some(v) = number(&stem[start..end])
        {
            return Some(v);
        }
        // After: maybe a separator, then digits.
        let mut s = i + 3;
        while s < bytes.len() && matches!(bytes[s], b' ' | b'_' | b'-') {
            s += 1;
        }
        let mut e = s;
        while e < bytes.len() && (bytes[e].is_ascii_digit() || bytes[e] == b'.') {
            e += 1;
        }
        if s < e
            && let Some(v) = number(&stem[s..e])
        {
            return Some(v);
        }
        from = i + 3;
    }
    None
}

fn round_bpm(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests;
