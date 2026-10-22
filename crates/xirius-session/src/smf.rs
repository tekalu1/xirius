//! Standard MIDI files (UI-076, EXP-026): reading the notes of every track and channel in the
//! song's ticks, with the file's track names, tempo and time signatures. Format 0 and 1; a file
//! timed in SMPTE frames is refused. Notes keep their place in beats.

use std::collections::{HashMap, VecDeque};

/// A note of the file, in the song's ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmfNote {
    pub at: i64,
    pub length: i64,
    pub pitch: u8,
    pub velocity: u8,
}

/// One part of a file: a track of a format 1 file, or a channel of a format 0 file.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// The track's name in the file, when it has one.
    pub name: Option<String>,
    /// The notes, sorted by start then pitch.
    pub notes: Vec<SmfNote>,
}

/// What a file holds, in the song's ticks.
#[derive(Clone, Debug, PartialEq)]
pub struct Smf {
    /// The parts that have notes, in the file's order.
    pub parts: Vec<Part>,
    /// Tempo changes: tick and quarter notes a minute.
    pub tempo: Vec<(i64, f64)>,
    /// Time signatures: tick, beats a bar and the beat's note value.
    pub meter: Vec<(i64, u32, u32)>,
}

/// The notes of `bytes`, sorted by start then pitch, with ticks scaled from
/// the file's quarter note to `ppq`.
pub fn read(bytes: &[u8], ppq: u32) -> Result<Vec<SmfNote>, String> {
    let mut out: Vec<SmfNote> = read_song(bytes, ppq)?
        .parts
        .into_iter()
        .flat_map(|p| p.notes)
        .collect();
    out.sort_by_key(|n| (n.at, n.pitch));
    Ok(out)
}

/// The parts, tempo and time signatures of `bytes`, with ticks scaled from the file's quarter
/// note to `ppq`. A format 0 file is split by channel.
pub fn read_song(bytes: &[u8], ppq: u32) -> Result<Smf, String> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(4)? != b"MThd" {
        return Err("MIDI ファイルではありません".into());
    }
    let len = r.u32()? as usize;
    let header = r.take(len)?;
    if header.len() < 6 {
        return Err("MIDI ファイルの見出しが短すぎます".into());
    }
    let format = u16::from_be_bytes([header[0], header[1]]);
    if format > 1 {
        return Err(format!("MIDI ファイルの形式 {format} には対応していません"));
    }
    let division = u16::from_be_bytes([header[4], header[5]]);
    if division & 0x8000 != 0 || division == 0 {
        return Err("SMPTE で時間を数える MIDI ファイルには対応していません".into());
    }
    let mut tracks = Vec::new();
    while r.at < bytes.len() {
        let id = r.take(4)?;
        let len = r.u32()? as usize;
        let body = r.take(len)?;
        if id == b"MTrk" {
            tracks.push(track(body)?);
        }
    }
    let scale = |t: u64| -> i64 {
        ((t as u128 * u128::from(ppq) + u128::from(division) / 2) / u128::from(division)) as i64
    };
    let notes = |raw: Vec<Raw>| -> Vec<SmfNote> {
        let mut out: Vec<SmfNote> = raw
            .into_iter()
            .map(|n| {
                let at = scale(n.start);
                SmfNote {
                    at,
                    length: (scale(n.end) - at).max(1),
                    pitch: n.pitch,
                    velocity: n.velocity.max(1),
                }
            })
            .collect();
        out.sort_by_key(|n| (n.at, n.pitch));
        out
    };
    let mut tempo: Vec<(i64, f64)> = Vec::new();
    let mut meter: Vec<(i64, u32, u32)> = Vec::new();
    let mut parts = Vec::new();
    for t in tracks {
        tempo.extend(t.tempo.iter().map(|(at, bpm)| (scale(*at), *bpm)));
        meter.extend(t.meter.iter().map(|(at, n, d)| (scale(*at), *n, *d)));
        if format == 0 {
            let mut channels: Vec<u8> = t.notes.iter().map(|n| n.channel).collect();
            channels.sort_unstable();
            channels.dedup();
            let one = channels.len() == 1;
            for ch in channels {
                let raw = t
                    .notes
                    .iter()
                    .filter(|n| n.channel == ch)
                    .cloned()
                    .collect();
                parts.push(Part {
                    name: if one { t.name.clone() } else { None },
                    notes: notes(raw),
                });
            }
        } else if !t.notes.is_empty() {
            parts.push(Part {
                name: t.name,
                notes: notes(t.notes),
            });
        }
    }
    // The last word at a tick wins; one list in time.
    tempo.sort_by_key(|t| t.0);
    tempo.reverse();
    tempo.dedup_by_key(|t| t.0);
    tempo.reverse();
    meter.sort_by_key(|m| m.0);
    meter.reverse();
    meter.dedup_by_key(|m| m.0);
    meter.reverse();
    Ok(Smf {
        parts,
        tempo,
        meter,
    })
}

/// A note in the file's ticks.
#[derive(Clone)]
struct Raw {
    start: u64,
    end: u64,
    pitch: u8,
    velocity: u8,
    channel: u8,
}

/// What one track chunk holds, in the file's ticks.
struct Track {
    name: Option<String>,
    notes: Vec<Raw>,
    tempo: Vec<(u64, f64)>,
    meter: Vec<(u64, u32, u32)>,
}

/// One track chunk. A note still held at the end of the track ends there.
fn track(body: &[u8]) -> Result<Track, String> {
    let mut r = Reader { bytes: body, at: 0 };
    let mut out = Track {
        name: None,
        notes: Vec::new(),
        tempo: Vec::new(),
        meter: Vec::new(),
    };
    let mut now = 0u64;
    let mut status = 0u8;
    // Notes sounding, by channel and pitch; the first on ends first.
    let mut held: HashMap<(u8, u8), VecDeque<(u64, u8)>> = HashMap::new();
    while r.at < body.len() {
        now += r.vlq()?;
        let mut b = r.byte()?;
        if b < 0x80 {
            // Running status: the byte is the first data byte.
            if status == 0 {
                return Err("MIDI ファイルのイベントが読めません".into());
            }
            r.at -= 1;
            b = status;
        } else if b < 0xf0 {
            status = b;
        }
        match b & 0xf0 {
            0x80 | 0x90 => {
                let (pitch, vel) = (r.byte()? & 0x7f, r.byte()? & 0x7f);
                let channel = b & 0x0f;
                let key = (channel, pitch);
                if b & 0xf0 == 0x90 && vel > 0 {
                    held.entry(key).or_default().push_back((now, vel));
                } else if let Some((start, velocity)) =
                    held.get_mut(&key).and_then(VecDeque::pop_front)
                {
                    out.notes.push(Raw {
                        start,
                        end: now,
                        pitch,
                        velocity,
                        channel,
                    });
                }
            }
            0xa0 | 0xb0 | 0xe0 => {
                r.take(2)?;
            }
            0xc0 | 0xd0 => {
                r.take(1)?;
            }
            _ => match b {
                0xff => {
                    let kind = r.byte()?;
                    let len = r.vlq()? as usize;
                    let data = r.take(len)?;
                    match (kind, data) {
                        (0x2f, _) => break,
                        (0x03, name) if out.name.is_none() => {
                            // A name that is not UTF-8 (often Shift_JIS) is left out rather
                            // than shown broken.
                            out.name = std::str::from_utf8(name)
                                .ok()
                                .map(|n| n.trim_matches(char::from(0)).trim().to_string())
                                .filter(|n| !n.is_empty());
                        }
                        (0x51, [a, b, c]) => {
                            let us = u32::from_be_bytes([0, *a, *b, *c]);
                            if us > 0 {
                                out.tempo.push((now, 60_000_000.0 / f64::from(us)));
                            }
                        }
                        (0x58, [n, d, ..]) if *n > 0 && *d < 8 => {
                            out.meter.push((now, u32::from(*n), 1 << d));
                        }
                        _ => {}
                    }
                }
                0xf0 | 0xf7 => {
                    let len = r.vlq()? as usize;
                    r.take(len)?;
                }
                _ => return Err("MIDI ファイルのイベントが読めません".into()),
            },
        }
    }
    for ((channel, pitch), starts) in held {
        for (start, velocity) in starts {
            out.notes.push(Raw {
                start,
                end: now.max(start + 1),
                pitch,
                velocity,
                channel,
            });
        }
    }
    Ok(out)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.bytes.len());
        let Some(end) = end else {
            return Err("MIDI ファイルが途中で終わっています".into());
        };
        let s = &self.bytes[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A variable-length quantity: 7 bits a byte, at most 4 bytes.
    fn vlq(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        for _ in 0..4 {
            let b = self.byte()?;
            v = (v << 7) | u64::from(b & 0x7f);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("MIDI ファイルの長さの値が読めません".into())
    }
}

/// Building files byte by byte: the writer of MIDI exports and the tests' files.
pub mod build {
    /// A variable-length quantity.
    pub fn vlq(mut v: u32) -> Vec<u8> {
        let mut out = vec![(v & 0x7f) as u8];
        v >>= 7;
        while v > 0 {
            out.insert(0, (v & 0x7f) as u8 | 0x80);
            v >>= 7;
        }
        out
    }

    /// A track chunk of (delta, event bytes), closed by the end-of-track event.
    pub fn mtrk(events: &[(u32, &[u8])]) -> Vec<u8> {
        let mut body = Vec::new();
        for (delta, e) in events {
            body.extend(vlq(*delta));
            body.extend_from_slice(e);
        }
        body.extend([0x00, 0xff, 0x2f, 0x00]);
        let mut out = b"MTrk".to_vec();
        out.extend((body.len() as u32).to_be_bytes());
        out.extend(body);
        out
    }

    /// A file of `tracks` with `division` ticks a quarter note.
    pub fn file(format: u16, division: u16, tracks: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"MThd".to_vec();
        out.extend(6u32.to_be_bytes());
        out.extend(format.to_be_bytes());
        out.extend((tracks.len() as u16).to_be_bytes());
        out.extend(division.to_be_bytes());
        for t in tracks {
            out.extend(t);
        }
        out
    }

    /// A format 0 file of C, E, G, a quarter note each, at 480 a quarter and 120 BPM.
    pub fn three_notes() -> Vec<u8> {
        file(
            0,
            480,
            &[mtrk(&[
                (0, &[0xff, 0x51, 0x03, 0x07, 0xa1, 0x20]),
                (0, &[0x90, 60, 100]),
                (480, &[0x80, 60, 0]),
                (0, &[0x90, 64, 90]),
                (480, &[0x80, 64, 0]),
                (0, &[0x90, 67, 80]),
                (480, &[0x80, 67, 0]),
            ])],
        )
    }
}

#[cfg(test)]
mod tests;
