//! Audio clips at another speed or pitch, or backwards (EDT-007, EDT-016, LIC-013): the export
//! renders them on the spot, and the live mix leaves them silent until their render is ready.

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_core::id::{ClipId, Node, TrackId};
use xirius_core::model::{AudioProcess, Clip, ClipBody, Project, Track, TrackKind};
use xirius_engine::Command;
use xirius_session::build::Builder;
use xirius_session::export::{self, Options, RangeSpec};
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::processed::State;
use xirius_session::tempo::TempoMap;

const RATE: u32 = 48_000;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xirius-processed-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A tone of `secs` seconds whose level rises from `from` to `to`.
fn write_tone(path: &Path, freq: f64, secs: f64, from: f64, to: f64) {
    let mut w = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate: RATE,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    let n = (secs * f64::from(RATE)) as usize;
    for i in 0..n {
        let level = from + (to - from) * i as f64 / n as f64;
        let v = (level * (TAU * freq * i as f64 / f64::from(RATE)).sin()) as f32;
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}

/// A song with one audio clip at the start, `beats` long, playing `file` with `process`.
fn song(file: &Path, beats: i64, process: AudioProcess) -> Project {
    let mut p = Project::new("processed");
    let ppq = p.ppq as i64;
    let id = TrackId(1);
    let mut t = Track::new(id, TrackKind::Audio, "audio");
    t.clips.push(Clip {
        id: ClipId(2),
        name: "tone".into(),
        color: None,
        take: None,
        controllers: Vec::new(),
        start: 0,
        length: beats * ppq,
        muted: false,
        body: ClipBody::Audio {
            file: file.to_string_lossy().into_owned(),
            offset: 0,
            gain_db: 0.0,
            process,
        },
    });
    p.tracks.insert(id, Arc::new(t));
    p.root.push(Node::Track(id));
    p
}

/// The left channel of an export of the first `beats` beats.
fn export_left(p: &Project, dir: &Path, beats: i64) -> Vec<f32> {
    let out = dir.join("mix.wav");
    export::mixdown(
        p,
        &out,
        &Options {
            range: RangeSpec::Ticks(0, beats * p.ppq as i64),
            bit_depth: Some(32),
            ..Options::default()
        },
    )
    .unwrap();
    let mut r = hound::WavReader::open(&out).unwrap();
    let all: Vec<f32> = r.samples::<f32>().map(|s| s.unwrap()).collect();
    all.chunks(2).map(|c| c[0]).collect()
}

/// The frequency from the rising zero crossings of `x[from..to]`.
fn frequency(x: &[f32], from: usize, to: usize) -> f64 {
    let (mut first, mut last, mut n) = (None, 0.0, 0);
    for i in from.max(1)..to {
        let (a, b) = (f64::from(x[i - 1]), f64::from(x[i]));
        if a < 0.0 && b >= 0.0 {
            let t = (i - 1) as f64 + a / (a - b);
            if first.is_none() {
                first = Some(t);
            } else {
                n += 1;
            }
            last = t;
        }
    }
    f64::from(n) * f64::from(RATE) / (last - first.unwrap())
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

#[test]
fn half_speed_lasts_twice_as_long_at_the_same_pitch() {
    let dir = temp("speed");
    let file = dir.join("tone.wav");
    // One second: two beats at 120.
    write_tone(&file, 440.0, 1.0, 0.5, 0.5);
    let process = AudioProcess {
        speed: 50.0,
        ..Default::default()
    };
    let left = export_left(&song(&file, 4, process), &dir, 4);
    assert_eq!(left.len(), 2 * RATE as usize);
    let f = frequency(&left, 12_000, 84_000);
    assert!((f - 440.0).abs() < 1.0, "{f} Hz");
    // Sounding nearly to the end of the two seconds.
    let tail = &left[88_000..94_000];
    assert!(rms(tail) > 0.2, "{}", rms(tail));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_octave_up_doubles_the_frequency_and_keeps_the_length() {
    let dir = temp("octave");
    let file = dir.join("tone.wav");
    write_tone(&file, 300.0, 1.0, 0.5, 0.5);
    let process = AudioProcess {
        semis: 12,
        ..Default::default()
    };
    let left = export_left(&song(&file, 2, process), &dir, 2);
    let f = frequency(&left, 8_000, 40_000);
    assert!((f - 600.0).abs() < 1.0, "{f} Hz");
    let process = AudioProcess {
        cents: -100,
        ..Default::default()
    };
    let left = export_left(&song(&file, 2, process), &dir, 2);
    let f = frequency(&left, 8_000, 40_000);
    let want = 300.0 * 2f64.powf(-1.0 / 12.0);
    assert!((f - want).abs() < 1.0, "{f} Hz, want {want}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_reversed_clip_plays_its_end_first() {
    let dir = temp("reverse");
    let file = dir.join("rise.wav");
    write_tone(&file, 440.0, 1.0, 0.0, 0.8);
    let process = AudioProcess {
        reverse: true,
        ..Default::default()
    };
    let left = export_left(&song(&file, 2, process), &dir, 2);
    let quarter = left.len() / 4;
    let (start, end) = (rms(&left[..quarter]), rms(&left[3 * quarter..]));
    assert!(start > end * 3.0, "{start} then {end}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_live_mix_leaves_a_clip_silent_until_its_render_is_ready() {
    let dir = temp("live");
    let file = dir.join("tone.wav");
    // Long enough that the render is still under way when the timeline is first built.
    write_tone(&file, 440.0, 20.0, 0.5, 0.5);
    let process = AudioProcess {
        speed: 80.0,
        ..Default::default()
    };
    let p = song(&file, 50, process);
    let tmap = TempoMap::from_project(&p);

    let mut b = Builder::new(RATE, None);
    b.set_background(true);
    let tl = b.timeline(&p, &p.tracks[&TrackId(1)], &tmap, &mut Vec::new());
    assert!(tl.regions().is_empty(), "silent while rendering");

    let (mut host, mut audio, warnings) = SessionHost::start(
        &p,
        HostConfig {
            rate: Some(RATE),
            ..HostConfig::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let clip = &p.tracks[&TrackId(1)].clips[0];
    assert!(
        matches!(host.clip_state(clip), Some(State::Working(_))),
        "{:?}",
        host.clip_state(clip)
    );
    host.controller().send(Command::Play).ok().unwrap();
    let mut out = vec![0.0; 256];
    audio.render(&mut out, 2);
    assert!(out.iter().all(|v| *v == 0.0), "silent while rendering");

    let t = Instant::now();
    while !matches!(host.clip_state(clip), Some(State::Ready(_))) {
        assert!(t.elapsed() < Duration::from_secs(60), "never ready");
        std::thread::sleep(Duration::from_millis(10));
    }
    // The poll sends the timeline again, and the clip is heard.
    host.poll();
    let mut heard = false;
    for _ in 0..20 {
        audio.render(&mut out, 2);
        host.poll();
        heard |= out.iter().any(|v| v.abs() > 0.05);
    }
    assert!(heard, "heard once ready");
    drop(host);
    let _ = std::fs::remove_dir_all(dir);
}
