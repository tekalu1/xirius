//! The ZIP container a `.dawproject` is: entries stored or deflated, no ZIP64 (an entry and
//! the whole file stay under 4 GiB).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
/// The name is UTF-8.
const UTF8: u16 = 1 << 11;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;
/// 1980-01-01 00:00, the earliest DOS date.
const DOS_DATE: u16 = 0x0021;
const LIMIT: u64 = u32::MAX as u64;

struct Entry {
    name: String,
    method: u16,
    crc: u32,
    packed: u32,
    size: u32,
    offset: u32,
}

/// Writes entries one after another, then the central directory on [`ZipWriter::finish`].
pub struct ZipWriter<W: Write> {
    out: W,
    at: u64,
    entries: Vec<Entry>,
}

fn too_big() -> String {
    "4 GB を超えるファイルは書き出せません".to_string()
}

fn crc(data: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(data);
    c.sum()
}

impl<W: Write> ZipWriter<W> {
    pub fn new(out: W) -> ZipWriter<W> {
        ZipWriter {
            out,
            at: 0,
            entries: Vec::new(),
        }
    }

    /// Adds one entry; `deflate` packs it, otherwise it is stored as it is.
    pub fn add(&mut self, name: &str, data: &[u8], deflate: bool) -> Result<(), String> {
        let packed;
        let (method, body) = if deflate {
            let mut e = DeflateEncoder::new(Vec::new(), Compression::default());
            e.write_all(data).map_err(|e| e.to_string())?;
            packed = e.finish().map_err(|e| e.to_string())?;
            (DEFLATED, packed.as_slice())
        } else {
            (STORED, data)
        };
        if data.len() as u64 > LIMIT || body.len() as u64 > LIMIT || self.at > LIMIT {
            return Err(too_big());
        }
        let e = Entry {
            name: name.to_string(),
            method,
            crc: crc(data),
            packed: body.len() as u32,
            size: data.len() as u32,
            offset: self.at as u32,
        };
        let mut h = Vec::with_capacity(30 + name.len());
        h.extend_from_slice(&LOCAL.to_le_bytes());
        h.extend_from_slice(&20u16.to_le_bytes());
        h.extend_from_slice(&flags(name).to_le_bytes());
        h.extend_from_slice(&method.to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(&DOS_DATE.to_le_bytes());
        h.extend_from_slice(&e.crc.to_le_bytes());
        h.extend_from_slice(&e.packed.to_le_bytes());
        h.extend_from_slice(&e.size.to_le_bytes());
        h.extend_from_slice(&(name.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(name.as_bytes());
        self.put(&h)?;
        self.put(body)?;
        self.entries.push(e);
        Ok(())
    }

    fn put(&mut self, b: &[u8]) -> Result<(), String> {
        self.out.write_all(b).map_err(|e| e.to_string())?;
        self.at += b.len() as u64;
        Ok(())
    }

    /// Writes the central directory and hands the writer back.
    pub fn finish(mut self) -> Result<W, String> {
        let start = self.at;
        let mut dir = Vec::new();
        for e in &self.entries {
            dir.extend_from_slice(&CENTRAL.to_le_bytes());
            dir.extend_from_slice(&20u16.to_le_bytes());
            dir.extend_from_slice(&20u16.to_le_bytes());
            dir.extend_from_slice(&flags(&e.name).to_le_bytes());
            dir.extend_from_slice(&e.method.to_le_bytes());
            dir.extend_from_slice(&0u16.to_le_bytes());
            dir.extend_from_slice(&DOS_DATE.to_le_bytes());
            dir.extend_from_slice(&e.crc.to_le_bytes());
            dir.extend_from_slice(&e.packed.to_le_bytes());
            dir.extend_from_slice(&e.size.to_le_bytes());
            dir.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            // Extra, comment, disk, internal and external attributes.
            dir.extend_from_slice(&[0; 2 + 2 + 2 + 2 + 4]);
            dir.extend_from_slice(&e.offset.to_le_bytes());
            dir.extend_from_slice(e.name.as_bytes());
        }
        if start + dir.len() as u64 > LIMIT || self.entries.len() > usize::from(u16::MAX) {
            return Err(too_big());
        }
        let n = self.entries.len() as u16;
        let mut end = Vec::with_capacity(22);
        end.extend_from_slice(&END.to_le_bytes());
        end.extend_from_slice(&[0; 4]);
        end.extend_from_slice(&n.to_le_bytes());
        end.extend_from_slice(&n.to_le_bytes());
        end.extend_from_slice(&(dir.len() as u32).to_le_bytes());
        end.extend_from_slice(&(start as u32).to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        self.put(&dir)?;
        self.put(&end)?;
        self.out.flush().map_err(|e| e.to_string())?;
        Ok(self.out)
    }
}

fn flags(name: &str) -> u16 {
    if name.is_ascii() { 0 } else { UTF8 }
}

struct Found {
    method: u16,
    crc: u32,
    packed: u64,
    size: u64,
    offset: u64,
}

/// Reads the entries of an archive by name, through its central directory.
pub struct ZipReader<R: Read + Seek> {
    src: R,
    entries: HashMap<String, Found>,
    names: Vec<String>,
}

fn bad() -> String {
    "ZIP の形式ではないか、壊れています".to_string()
}

fn u16_at(b: &[u8], i: usize) -> Result<u16, String> {
    b.get(i..i + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(bad)
}

fn u32_at(b: &[u8], i: usize) -> Result<u32, String> {
    b.get(i..i + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(bad)
}

impl<R: Read + Seek> ZipReader<R> {
    pub fn new(mut src: R) -> Result<ZipReader<R>, String> {
        let len = src.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        // The end record is 22 bytes plus a comment of at most 64 KiB.
        let tail = len.min(22 + 0xFFFF);
        src.seek(SeekFrom::Start(len - tail))
            .map_err(|e| e.to_string())?;
        let mut t = vec![0; tail as usize];
        src.read_exact(&mut t).map_err(|e| e.to_string())?;
        let at = (0..t.len().saturating_sub(21))
            .rev()
            .find(|&i| u32_at(&t, i) == Ok(END))
            .ok_or_else(bad)?;
        let count = u16_at(&t, at + 10)?;
        let size = u32_at(&t, at + 12)?;
        let start = u32_at(&t, at + 16)?;
        if count == u16::MAX || size == u32::MAX || start == u32::MAX {
            return Err("ZIP64 の形式には対応していません".to_string());
        }
        src.seek(SeekFrom::Start(u64::from(start)))
            .map_err(|e| e.to_string())?;
        let mut dir = vec![0; size as usize];
        src.read_exact(&mut dir).map_err(|_| bad())?;
        let mut entries = HashMap::new();
        let mut names = Vec::new();
        let mut i = 0;
        for _ in 0..count {
            if u32_at(&dir, i)? != CENTRAL {
                return Err(bad());
            }
            let method = u16_at(&dir, i + 10)?;
            let crc = u32_at(&dir, i + 16)?;
            let packed = u32_at(&dir, i + 20)?;
            let size = u32_at(&dir, i + 24)?;
            let name_len = usize::from(u16_at(&dir, i + 28)?);
            let extra = usize::from(u16_at(&dir, i + 30)?);
            let comment = usize::from(u16_at(&dir, i + 32)?);
            let offset = u32_at(&dir, i + 42)?;
            if packed == u32::MAX || size == u32::MAX || offset == u32::MAX {
                return Err("ZIP64 の形式には対応していません".to_string());
            }
            let raw = dir.get(i + 46..i + 46 + name_len).ok_or_else(bad)?;
            let name = String::from_utf8_lossy(raw).replace('\\', "/");
            names.push(name.clone());
            entries.insert(
                name,
                Found {
                    method,
                    crc,
                    packed: u64::from(packed),
                    size: u64::from(size),
                    offset: u64::from(offset),
                },
            );
            i += 46 + name_len + extra + comment;
        }
        Ok(ZipReader {
            src,
            entries,
            names,
        })
    }

    /// The entries' names, in the archive's order.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// The bytes of an entry; `None` when there is no such entry.
    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, String> {
        crate::session::opening::before("read", std::path::Path::new(name))
            .map_err(|e| e.to_string())?;
        let got = self.read_entry(name)?;
        if got.is_some() {
            crate::session::opening::read_done();
        }
        crate::session::opening::check().map_err(|e| e.to_string())?;
        Ok(got)
    }

    fn read_entry(&mut self, name: &str) -> Result<Option<Vec<u8>>, String> {
        let Some(e) = self.entries.get(name) else {
            return Ok(None);
        };
        let (method, want, packed, size) = (e.method, e.crc, e.packed, e.size);
        self.src
            .seek(SeekFrom::Start(e.offset))
            .map_err(|e| e.to_string())?;
        let mut h = [0u8; 30];
        self.src.read_exact(&mut h).map_err(|_| bad())?;
        if u32_at(&h, 0)? != LOCAL {
            return Err(bad());
        }
        let skip = i64::from(u16_at(&h, 26)?) + i64::from(u16_at(&h, 28)?);
        self.src
            .seek(SeekFrom::Current(skip))
            .map_err(|e| e.to_string())?;
        let body = (&mut self.src).take(packed);
        let mut out = Vec::with_capacity(size.min(1 << 30) as usize);
        match method {
            STORED => {
                let mut b = body;
                b.read_to_end(&mut out).map_err(|e| e.to_string())?;
            }
            DEFLATED => {
                DeflateDecoder::new(body)
                    .read_to_end(&mut out)
                    .map_err(|_| bad())?;
            }
            m => return Err(format!("{name} の圧縮方式（{m}）には対応していません")),
        }
        if out.len() as u64 != size || crc(&out) != want {
            return Err(format!("{name} が壊れています"));
        }
        Ok(Some(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn entries_come_back_stored_and_deflated() {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        let text = "<Project/>".repeat(100);
        w.add("project.xml", text.as_bytes(), true).unwrap();
        w.add("audio/キック.wav", &[1, 2, 3, 4], false).unwrap();
        w.add("empty", &[], true).unwrap();
        let bytes = w.finish().unwrap().into_inner();
        let mut r = ZipReader::new(Cursor::new(bytes.clone())).unwrap();
        assert_eq!(r.names(), ["project.xml", "audio/キック.wav", "empty"]);
        assert_eq!(r.read("project.xml").unwrap().unwrap(), text.as_bytes());
        assert_eq!(r.read("audio/キック.wav").unwrap().unwrap(), [1, 2, 3, 4]);
        assert_eq!(r.read("empty").unwrap().unwrap(), Vec::<u8>::new());
        assert!(r.read("nothing").unwrap().is_none());
        // A flipped byte in the stored data is caught by the checksum.
        let mut broken = bytes;
        let at = broken.windows(4).position(|w| w == [1, 2, 3, 4]).unwrap();
        broken[at] = 9;
        let mut r = ZipReader::new(Cursor::new(broken)).unwrap();
        assert!(r.read("audio/キック.wav").is_err());
    }

    #[test]
    fn something_that_is_not_a_zip_is_refused() {
        assert!(ZipReader::new(Cursor::new(b"not a zip at all".to_vec())).is_err());
        assert!(ZipReader::new(Cursor::new(Vec::new())).is_err());
    }
}
