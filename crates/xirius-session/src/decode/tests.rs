use std::path::PathBuf;

use super::*;
use crate::export::{write_flac, write_wav};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("xirius-decode-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sine(rate: u32, secs: f64, hz: f64, amp: f32) -> Vec<f32> {
    let n = (f64::from(rate) * secs).round() as usize;
    (0..n)
        .map(|i| amp * (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin() as f32)
        .collect()
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

/// A 16-bit mono AIFF, written by hand (big endian, the rate as an 80-bit float).
fn write_aiff(path: &Path, x: &[f32], rate: u32) {
    let mut ssnd = vec![0u8; 8];
    for s in x {
        ssnd.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_be_bytes());
    }
    let mut comm = Vec::new();
    comm.extend_from_slice(&1u16.to_be_bytes());
    comm.extend_from_slice(&(x.len() as u32).to_be_bytes());
    comm.extend_from_slice(&16u16.to_be_bytes());
    // 80-bit extended: exponent, then the mantissa with its leading 1.
    let e = 31 - rate.leading_zeros();
    comm.extend_from_slice(&((16383 + e) as u16).to_be_bytes());
    comm.extend_from_slice(&(u64::from(rate) << (63 - e)).to_be_bytes());
    let mut body = b"AIFF".to_vec();
    for (id, c) in [(b"COMM", &comm), (b"SSND", &ssnd)] {
        body.extend_from_slice(id);
        body.extend_from_slice(&(c.len() as u32).to_be_bytes());
        body.extend_from_slice(c);
    }
    let mut f = b"FORM".to_vec();
    f.extend_from_slice(&(body.len() as u32).to_be_bytes());
    f.extend_from_slice(&body);
    std::fs::write(path, f).unwrap();
}

/// Adds an `acid` chunk saying `bpm` (and `flags`) to the end of a WAV.
fn add_acid(path: &Path, bpm: f32, flags: u32) {
    let mut b = std::fs::read(path).unwrap();
    let mut acid = Vec::new();
    acid.extend_from_slice(&flags.to_le_bytes());
    acid.extend_from_slice(&60u16.to_le_bytes());
    acid.extend_from_slice(&0x8000u16.to_le_bytes());
    acid.extend_from_slice(&0f32.to_le_bytes());
    acid.extend_from_slice(&8u32.to_le_bytes());
    acid.extend_from_slice(&4u16.to_le_bytes());
    acid.extend_from_slice(&4u16.to_le_bytes());
    acid.extend_from_slice(&bpm.to_le_bytes());
    b.extend_from_slice(b"acid");
    b.extend_from_slice(&(acid.len() as u32).to_le_bytes());
    b.extend_from_slice(&acid);
    let riff = (b.len() - 8) as u32;
    b[4..8].copy_from_slice(&riff.to_le_bytes());
    std::fs::write(path, b).unwrap();
}

#[test]
fn wav_flac_and_aiff_are_read_with_their_rate_and_length() {
    let d = Dir::new("formats");
    let x = sine(44_100, 0.5, 440.0, 0.5);
    let wav = d.0.join("a.wav");
    let flac = d.0.join("a.flac");
    let aiff = d.0.join("a.aiff");
    write_wav(&wav, &x, &x, 44_100, 24).unwrap();
    write_flac(&flac, &x, &x, 44_100, 16).unwrap();
    write_aiff(&aiff, &x, 44_100);
    for p in [&wav, &flac, &aiff] {
        let info = probe(p).unwrap();
        assert_eq!(info.rate, 44_100, "{}", p.display());
        assert_eq!(info.frames, x.len() as u64, "{}", p.display());
        assert!((info.seconds() - 0.5).abs() < 1e-6);
        let dec = decode(p).unwrap();
        assert_eq!(dec.left.len(), x.len(), "{}", p.display());
        assert!(
            (rms(&dec.left) - 0.5 / 2f64.sqrt()).abs() < 0.01,
            "{}",
            p.display()
        );
    }
    assert_eq!(probe(&aiff).unwrap().channels, 1);
    assert!(decode(&aiff).unwrap().mono);
}

#[test]
fn a_file_at_44_1_khz_is_converted_to_48_khz_and_keeps_its_time() {
    let d = Dir::new("rate");
    let x = sine(44_100, 1.0, 1000.0, 0.5);
    let p = d.0.join("tone.wav");
    write_wav(&p, &x, &x, 44_100, 32).unwrap();
    let a = read(&p, 48_000).unwrap();
    assert_eq!(a.left.len(), 48_000);
    assert_eq!(a.right.len(), 48_000);
    // Same loudness, and still 1 kHz: the zero crossings of 1 s count 2,000.
    let mid = &a.left[2_000..46_000];
    assert!((rms(mid) - 0.5 / 2f64.sqrt()).abs() < 0.005, "{}", rms(mid));
    let crossings = a
        .left
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    assert!((crossings as i64 - 1000).abs() <= 2, "{crossings}");
    // No shift: the first peak of the sine is still at 0.25 ms.
    let peak = a.left[..48]
        .iter()
        .enumerate()
        .fold((0, 0.0f32), |m, (i, v)| if *v > m.1 { (i, *v) } else { m });
    assert!((peak.0 as i64 - 12).abs() <= 1, "{peak:?}");
    // The same rate is left as it is.
    let same = read(&p, 44_100).unwrap();
    assert_eq!(same.left, x);
}

#[test]
fn the_tempo_comes_from_the_acid_chunk_then_the_name() {
    let d = Dir::new("tempo");
    let x = sine(48_000, 0.1, 440.0, 0.2);
    let acid = d.0.join("Loop 120bpm.wav");
    write_wav(&acid, &x, &x, 48_000, 16).unwrap();
    add_acid(&acid, 92.0, 0);
    let info = probe(&acid).unwrap();
    assert_eq!(info.bpm, Some(92.0));
    assert_eq!(info.bpm_from, Some("acid"));
    // The chunk does not change the length (ENG-031).
    assert_eq!(info.frames, x.len() as u64);
    assert_eq!(decode(&acid).unwrap().left.len(), x.len());

    let one_shot = d.0.join("Hit 140 BPM.wav");
    write_wav(&one_shot, &x, &x, 48_000, 16).unwrap();
    add_acid(&one_shot, 92.0, 1);
    let info = probe(&one_shot).unwrap();
    assert_eq!((info.bpm, info.bpm_from), (Some(140.0), Some("name")));

    let plain = d.0.join("Vox.wav");
    write_wav(&plain, &x, &x, 48_000, 16).unwrap();
    assert_eq!(probe(&plain).unwrap().bpm, None);
}

#[test]
fn tempos_in_names() {
    let t = |s: &str| name_tempo(Path::new(s));
    assert_eq!(t("Drums 92bpm.wav"), Some(92.0));
    assert_eq!(t("drums_92_BPM.wav"), Some(92.0));
    assert_eq!(t("BPM128 Kick.wav"), Some(128.0));
    assert_eq!(t("loop-87.5bpm.flac"), Some(87.5));
    assert_eq!(t("bpm.wav"), None);
    assert_eq!(t("Take 2.wav"), None);
    assert_eq!(t("5000bpm.wav"), None);
}

#[test]
fn what_is_not_audio_is_refused_with_the_formats_named() {
    let d = Dir::new("bad");
    let txt = d.0.join("notes.txt");
    std::fs::write(&txt, "hello").unwrap();
    let e = probe(&txt).unwrap_err();
    assert!(e.contains("WAV・AIFF・FLAC・MP3"), "{e}");
    let fake = d.0.join("fake.wav");
    std::fs::write(&fake, "not a wav").unwrap();
    assert!(probe(&fake).is_err());
    assert!(is_audio(Path::new("x.FLAC")));
    assert!(!is_audio(Path::new("x.mid")));
}
