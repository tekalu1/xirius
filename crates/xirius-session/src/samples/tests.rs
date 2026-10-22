//! Chunks of a file's own samples (UI-310): the same frames as the whole file decoded, from where
//! the chunk starts, and read off the caller's thread.

use super::*;
use crate::export::{write_flac, write_wav};

fn dir(tag: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-samples-{tag}-"))
        .tempdir()
        .unwrap()
}

/// 3.5 chunks of a sweep, the right side upside down and quieter.
fn sweep() -> (Vec<f32>, Vec<f32>) {
    let n = (CHUNK * 7 / 2) as usize;
    let l: Vec<f32> = (0..n)
        .map(|i| (i as f32 * 0.002 * (1.0 + i as f32 / n as f32)).sin() * 0.9)
        .collect();
    let r = l.iter().map(|v| -v * 0.5).collect();
    (l, r)
}

/// What the whole file decodes to, as spans, for frames `[a, b)`.
fn whole(path: &Path, a: usize, b: usize) -> Vec<Span> {
    let d = crate::decode::decode(path).unwrap();
    let b = b.min(d.left.len());
    Samples::from_channels(d.rate, a as u64, &d.left[a..b], &d.right[a..b]).frames
}

#[test]
fn a_chunk_is_the_frames_the_whole_file_decodes_to_from_where_it_starts() {
    let tmp = dir("read");
    let (l, r) = sweep();
    let wav = tmp.path().join("a.wav");
    let flac = tmp.path().join("a.flac");
    write_wav(&wav, &l, &r, 48_000, 24).unwrap();
    write_flac(&flac, &l, &r, 44_100, 16).unwrap();
    for path in [&wav, &flac] {
        for start in [0, CHUNK, 2 * CHUNK + 1000] {
            let got = read(path, start, CHUNK).unwrap();
            assert_eq!(got.start, start);
            assert_eq!(got.frames.len(), CHUNK as usize, "{}", path.display());
            assert_eq!(
                got.frames,
                whole(path, start as usize, (start + CHUNK) as usize),
                "{} from {start}",
                path.display()
            );
            // Decoded from the file's start (MP3 and Ogg are read so): the same frames.
            assert_eq!(read_from(path, start, CHUNK, false).unwrap(), got);
        }
        // The last chunk stops at the file's end.
        let last = read(path, 3 * CHUNK, CHUNK).unwrap();
        assert_eq!(last.end(), l.len() as u64);
        assert_eq!(read(path, 9 * CHUNK, CHUNK).unwrap().frames.len(), 0);
    }
    assert_eq!(read(&wav, 0, 4).unwrap().rate, 48_000);
    assert_eq!(read(&flac, 0, 4).unwrap().rate, 44_100);
    // Both channels together: the right side reaches below where the left one does.
    let f = read(&wav, 0, CHUNK).unwrap().frames[800];
    assert!(l[800] > 0.5);
    assert!((f32::from(f.max) / 32767.0 - l[800]).abs() < 1e-3);
    assert!((f32::from(f.min) / 32767.0 - r[800]).abs() < 1e-3);
}

#[test]
fn a_span_of_many_frames_is_their_min_and_max() {
    let (l, r) = sweep();
    let s = Samples::from_channels(48_000, 1_000, &l[..5_000], &r[..5_000]);
    let brute = |a: u64, b: u64| {
        s.frames[(a - 1_000) as usize..(b - 1_000) as usize]
            .iter()
            .copied()
            .reduce(Span::merge)
    };
    for (a, b) in [
        (1_000, 1_001),
        (1_003, 1_040),
        (1_016, 1_032),
        (1_017, 5_999),
        (1_000, 6_000),
    ] {
        assert_eq!(s.reduce(a as f64, b as f64), brute(a, b), "{a}..{b}");
    }
    // A part of a frame counts; past the chunk's ends is clipped, outside it is None.
    assert_eq!(s.reduce(1_002.4, 1_002.6), brute(1_002, 1_003));
    assert_eq!(s.reduce(0.0, 1_010.0), brute(1_000, 1_010));
    assert_eq!(s.reduce(5_990.0, 9_000.0), brute(5_990, 6_000));
    assert_eq!(s.reduce(0.0, 999.0), None);
    assert_eq!(s.reduce(6_000.0, 6_100.0), None);
    assert_eq!(s.at(1_000), Some(s.frames[0]));
    assert_eq!(s.at(999), None);
    assert_eq!(s.bytes(), (5_000 + 5_000usize.div_ceil(BLOCK)) * 4);
}

#[test]
fn chunks_are_read_in_the_background_and_handed_over_once() {
    let tmp = dir("bg");
    let (l, r) = sweep();
    let wav = tmp.path().join("b.wav");
    write_wav(&wav, &l, &r, 48_000, 16).unwrap();
    let before = finished();
    request(&wav, 1);
    request(&wav, 1);
    request(&wav, 2);
    wait();
    let mut got: Vec<_> = take().into_iter().filter(|((p, _), _)| *p == wav).collect();
    got.sort_by_key(|((_, i), _)| *i);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].0.1, 1);
    assert_eq!(got[0].1.as_ref().unwrap().start, CHUNK);
    assert_eq!(got[1].1.as_ref().unwrap().start, 2 * CHUNK);
    assert!(finished() >= before + 2);
    assert!(take().iter().all(|((p, _), _)| *p != wav));
    // A file that cannot be read says why.
    let txt = tmp.path().join("x.txt");
    std::fs::write(&txt, "x").unwrap();
    request(&txt, 0);
    wait();
    let bad = take().into_iter().find(|((p, _), _)| *p == txt).unwrap();
    assert!(bad.1.unwrap_err().contains("WAV"));
}
