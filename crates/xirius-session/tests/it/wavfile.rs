//! The recording file as the rest of the program reads it (LIC-014): a take is read back by the
//! decoder that plays and imports, also when the program died while it was written, and also
//! when it grew past what a plain WAV can say (RF64).

use xirius_engine::wavfile::{SafeWav, repair};
use xirius_session::decode::{decode, probe};

/// A folder of the test's own, removed when dropped (also when the test fails).
fn dir(tag: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-wavfile-{tag}-"))
        .tempdir()
        .unwrap()
}

fn sample(i: usize) -> f32 {
    ((i as f32) * 0.05).sin() * 0.5
}

fn write(w: &mut SafeWav, frames: usize) {
    for i in 0..frames {
        w.write_sample(sample(i)).unwrap();
        w.write_sample(-sample(i)).unwrap();
    }
}

#[test]
fn the_decoder_reads_a_finished_take() {
    let tmp = dir("done");
    let p = tmp.path().join("a.wav");
    let mut w = SafeWav::create(&p, 2, 44_100).unwrap();
    write(&mut w, 5000);
    w.finalize().unwrap();
    let info = probe(&p).unwrap();
    assert_eq!((info.rate, info.channels, info.frames), (44_100, 2, 5000));
    let d = decode(&p).unwrap();
    assert_eq!(d.left.len(), 5000);
    assert_eq!(d.left[1234], sample(1234));
    assert_eq!(d.right[1234], -sample(1234));
}

#[test]
fn the_decoder_reads_a_take_the_program_died_in_up_to_the_last_header_write() {
    let tmp = dir("crash");
    let p = tmp.path().join("a.wav");
    let mut w = SafeWav::create(&p, 2, 1000).unwrap();
    // A header write every second of audio: two of them, then 400 frames more in the buffer.
    write(&mut w, 2400);
    std::mem::forget(w);
    let d = decode(&p).unwrap();
    assert_eq!(d.left.len(), 2000);
    assert_eq!(d.left[1999], sample(1999));
    // Repaired, it holds what reached the disk, which the header now says.
    let r = repair(&p).unwrap().unwrap();
    assert!(r.frames >= 2000);
    assert_eq!(decode(&p).unwrap().left.len() as u64, r.frames);
}

#[test]
fn the_decoder_reads_a_take_that_is_rf64() {
    let tmp = dir("rf64");
    let p = tmp.path().join("a.wav");
    // The limit is brought down so that a few thousand frames cross it, as 4 GiB would.
    let mut w = SafeWav::create(&p, 2, 44_100)
        .unwrap()
        .turning_rf64_after(4000);
    write(&mut w, 5000);
    assert!(w.is_rf64());
    w.finalize().unwrap();
    assert_eq!(&std::fs::read(&p).unwrap()[..4], b"RF64");
    let info = probe(&p).unwrap();
    assert_eq!((info.rate, info.channels, info.frames), (44_100, 2, 5000));
    let d = decode(&p).unwrap();
    assert_eq!(d.left.len(), 5000);
    assert_eq!(d.left[4321], sample(4321));
    assert_eq!(d.right[4321], -sample(4321));
}

#[test]
fn a_song_folder_is_searched_for_takes_that_were_cut_off() {
    use std::io::{Seek, SeekFrom, Write};
    let tmp = dir("folder");
    let d = tmp.path();
    let rec = d.join("Recordings");
    std::fs::create_dir_all(&rec).unwrap();
    // One finished, one cut off the way a writer that fills in sizes at the end leaves it.
    for name in ["Vocal-1.wav", "Vocal-2.wav"] {
        let mut w = SafeWav::create(&rec.join(name), 2, 1000).unwrap();
        write(&mut w, 3000);
        w.finalize().unwrap();
    }
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(rec.join("Vocal-2.wav"))
            .unwrap();
        f.seek(SeekFrom::Start(88)).unwrap();
        f.write_all(&0u32.to_le_bytes()).unwrap();
    }
    std::fs::write(rec.join("notes.txt"), b"x").unwrap();
    let got = xirius_session::record::repair_folder(d).unwrap();
    assert_eq!(
        got,
        vec![xirius_session::record::Recovered {
            file: "Recordings/Vocal-2.wav".into(),
            frames: 3000,
            seconds: 3,
        }]
    );
    assert_eq!(decode(&rec.join("Vocal-2.wav")).unwrap().left.len(), 3000);
    assert!(xirius_session::record::repair_folder(d).unwrap().is_empty());
    assert!(
        xirius_session::record::repair_folder(&d.join("none"))
            .unwrap()
            .is_empty()
    );
}
