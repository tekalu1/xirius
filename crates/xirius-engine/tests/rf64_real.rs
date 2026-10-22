//! LIC-014: a recording past 4 GiB, written for real (4.3 GiB of disk and a minute or so), is
//! RF64 and says its true length. Not part of the usual run: it needs the disk space, and
//! small files already hold the same logic (`wavfile`'s own tests, with the limit lowered).
//! Run it with `cargo test -p xirius-engine --test rf64_real --release -- --ignored`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use xirius_engine::wavfile::{SafeWav, repair};

const CHANNELS: u16 = 2;
/// 1.1 billion samples: 4.4 GB of audio, past what a RIFF size can say (4,294,967,295).
const SAMPLES: u64 = 1_100_000_000;

/// The sample at index `i`: a ramp that repeats, so a wrong offset shows.
fn at(i: u64) -> f32 {
    (i % 65_536) as f32 / 65_536.0
}

#[test]
#[ignore = "writes 4.4 GB; run alone with --release"]
fn a_take_of_4_4_gb_is_rf64_with_its_true_length_and_its_last_samples() {
    // Removed when dropped, so a failed run does not leave 4.4 GB behind.
    let dir = tempfile::Builder::new()
        .prefix("rf64-real-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let path = dir.path().join("big.wav");

    let mut w = SafeWav::create(&path, CHANNELS, 48_000).unwrap();
    let mut done = 0u64;
    let mut block = vec![0f32; 1 << 20];
    while done < SAMPLES {
        let n = block.len().min((SAMPLES - done) as usize);
        for (k, v) in block[..n].iter_mut().enumerate() {
            *v = at(done + k as u64);
        }
        w.write_samples(&block[..n]).unwrap();
        done += n as u64;
    }
    assert!(w.is_rf64(), "past 4 GiB the file turns RF64 by itself");
    assert_eq!(w.frames(), SAMPLES / u64::from(CHANNELS));
    w.finalize().unwrap();

    let bytes = SAMPLES * 4;
    let mut f = File::open(&path).unwrap();
    assert_eq!(f.metadata().unwrap().len(), 92 + bytes);
    let mut head = [0u8; 92];
    f.read_exact(&mut head).unwrap();
    assert_eq!(&head[..4], b"RF64");
    assert_eq!(&head[4..8], &[0xFF; 4]);
    assert_eq!(&head[12..16], b"ds64");
    let u64_at = |i: usize| u64::from_le_bytes(head[i..i + 8].try_into().unwrap());
    assert_eq!(u64_at(20), 92 - 8 + bytes, "riff size in ds64");
    assert_eq!(u64_at(28), bytes, "data size in ds64");
    assert_eq!(u64_at(36), SAMPLES / 2, "frames in ds64");
    assert_eq!(&head[88..92], &[0xFF; 4], "data size field says: see ds64");

    // The audio at the very end, past the 4 GiB mark, is where it was written.
    let tail = 1000u64;
    f.seek(SeekFrom::Start(92 + (SAMPLES - tail) * 4)).unwrap();
    let mut raw = vec![0u8; tail as usize * 4];
    f.read_exact(&mut raw).unwrap();
    for (k, c) in raw.as_chunks::<4>().0.iter().enumerate() {
        let got = f32::from_le_bytes(*c);
        assert_eq!(
            got,
            at(SAMPLES - tail + k as u64),
            "sample {k} from the end"
        );
    }
    drop(f);

    // A header that says the truth is left alone.
    let r = repair(&path).unwrap().unwrap();
    assert_eq!(
        (r.frames, r.fixed, r.cut),
        (SAMPLES / u64::from(CHANNELS), false, 0)
    );
}
