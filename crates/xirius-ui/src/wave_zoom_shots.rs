//! `--script wave-zoom` (UI-310): an audio clip zoomed in past its peaks. A real WAV (a 70 Hz
//! tone in hits, at 48 kHz) on the Vocal track of a song at 120 BPM, drawn from its peaks, from
//! its samples in columns, joined by a line, with a dot on each sample at the most a beat is
//! drawn (a sample 6 px wide), with the clip's gain and fades, at the vertical zoom ×4, and the
//! pointer on the vertical zoom's button in the head. Silent; nothing takes the PC's focus.
//!
//! Each stage reads the chunks of samples in view before the shot, so the shot does not see the
//! peaks drawn while they are read.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::id::{ClipId, TrackId};
use xirius_session::{peaks, samples};

use crate::arrange::probe::track;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, Studio};
use crate::wave::{self, Look};

const fn laid(n: u8) -> Act {
    Act::Stage(Play::Laid(stage), n)
}

pub const SHOTS: &[Shot] = &[
    // 1 px holds 750 frames: columns of peaks, as before.
    Shot {
        name: "wz01-peaks",
        acts: &[Act::Reset, laid(0), laid(1)],
    },
    // 1 px holds 20 frames: columns of the samples themselves (level 0 of the peaks is 128).
    Shot {
        name: "wz02-samples",
        acts: &[Act::Reset, laid(0), laid(2)],
    },
    // A sample is 2 px: the samples joined by a line.
    Shot {
        name: "wz03-line",
        acts: &[Act::Reset, laid(0), laid(3)],
    },
    // Zoomed in all the way: a sample is 6 px, with a dot on each.
    Shot {
        name: "wz04-dots",
        acts: &[Act::Reset, laid(0), laid(4)],
    },
    // The clip at -12 dB with a fade in and out: the waveform stands as it plays.
    Shot {
        name: "wz05-gain-fades",
        acts: &[Act::Reset, laid(0), laid(5)],
    },
    // The same at the vertical zoom ×4, and the pointer on its button.
    Shot {
        name: "wz06-wave-zoom-4",
        acts: &[Act::Reset, laid(0), laid(5), laid(6)],
    },
];

const VOCAL: &str = "Vocal";
const RATE: u32 = 48_000;
const SECS: f64 = 8.0;
/// Where the clip starts, in bars.
const AT_BAR: i64 = 40;

fn shots_dir() -> PathBuf {
    std::env::temp_dir().join(format!("xirius-wave-zoom-shots-{}", std::process::id()))
}

fn wav_path() -> PathBuf {
    shots_dir().join("tone_hits.wav")
}

/// A 70 Hz tone struck on each beat at 120 BPM, louder on each bar.
fn write_wav() -> PathBuf {
    let path = wav_path();
    if path.exists() {
        return path;
    }
    let n = (f64::from(RATE) * SECS) as usize;
    let x: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(RATE);
            let level = if t % 2.0 < 0.5 { 0.95 } else { 0.6 };
            let tone = (t * 70.0 * std::f64::consts::TAU).sin();
            (tone * level * (-(t % 0.5) / 0.2).exp()) as f32
        })
        .collect();
    let _ = std::fs::create_dir_all(shots_dir());
    let _ = xirius_session::export::write_wav(&path, &x, &x, RATE, 16);
    path
}

/// The clip: the last clip of Vocal, by start.
fn the_clip(s: &Studio) -> (TrackId, ClipId) {
    let t = track(s, VOCAL);
    let p = s.project();
    let mut v: Vec<_> = p.tracks[&t].clips.iter().collect();
    v.sort_by_key(|c| c.start);
    (t, v.last().expect("the clip").id)
}

fn ids(s: &Studio) -> serde_json::Value {
    json!([the_clip(s).1.to_string()])
}

/// A beat `beat_w` px wide, with the clip's start and `secs` into it at the lanes' left.
fn view(s: &mut Studio, g: &Geometry, beat_w: f32, secs: f64) {
    s.beat_w = beat_w.min(s.beat_w_max());
    let bar = i64::from(s.project().ppq) * 4;
    let tick = AT_BAR * bar + (secs * 2.0 * f64::from(s.project().ppq)) as i64;
    s.scroll.0 = (s.ticks_to_px(tick) - 40.0).max(0.0);
    s.resized(g);
    read_samples(s, secs);
}

/// Reads the chunks around `secs` of the file and hands them to the screen.
fn read_samples(s: &mut Studio, secs: f64) {
    let frame = (secs * f64::from(RATE)) as u64;
    let first = frame / samples::CHUNK;
    let path = wav_path();
    for i in first.saturating_sub(1)..=first + 1 {
        samples::request(&path, i);
    }
    samples::wait();
    wave::poll();
    s.idle();
}

/// Turns the screen's poll until the peaks of the file are there to draw, or ten seconds.
fn wait_peaks(s: &mut Studio) {
    let store = peaks::store_dir(s.song().dir().as_deref());
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end && !matches!(wave::look(&wav_path(), &store), Look::Ready(_)) {
        s.idle();
        std::thread::sleep(Duration::from_millis(16));
    }
}

/// Plays stage `n` of [`SHOTS`] on the screen's state.
fn stage(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => {
            let bar = i64::from(s.project().ppq) * 4;
            s.arrange_run("tempo.set", json!({ "at": 0, "bpm": 120 }));
            let path = write_wav();
            let vocal = track(s, VOCAL);
            s.arrange_run(
                "clip.add.audio",
                json!({ "track": vocal.to_string(), "at": AT_BAR * bar, "length": 4 * bar,
                        "file": path.to_string_lossy() }),
            );
            let g = &s.geometry(g.size.0, g.size.1);
            s.rebuild(g);
            wait_peaks(s);
        }
        // 24,000 frames a beat over 32 px: 750 a pixel.
        1 => view(s, g, 32.0, 0.0),
        2 => view(s, g, 1_200.0, 0.48),
        3 => view(s, g, 48_000.0, 0.5),
        4 => view(s, g, f32::MAX, 0.5),
        5 => {
            let ppq = i64::from(s.project().ppq);
            s.arrange_run("clip.gain.set", json!({ "ids": ids(s), "db": -12.0 }));
            s.arrange_run(
                "clip.fade.set",
                json!({ "ids": ids(s), "in": 2 * ppq, "out": 4 * ppq }),
            );
            view(s, g, 96.0, 0.0);
            s.scroll.0 = (s.ticks_to_px(AT_BAR * 4 * ppq) - 40.0).max(0.0);
        }
        6 => {
            s.set_wave_zoom(4.0);
            s.hover = Hit::WaveZoom;
        }
        _ => unreachable!("wave-zoom stage {n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::{Detail, detail_for};

    #[test]
    fn the_script_is_listed() {
        let listed = crate::SCRIPTS.iter().find(|(name, _)| *name == "wave-zoom");
        let names = |shots: &[Shot]| shots.iter().map(|s| s.name).collect::<Vec<_>>();
        assert_eq!(listed.map(|(_, s)| names(s)), Some(names(SHOTS)));
    }

    /// What each zoom of the script draws, worked out without a window or a file.
    #[test]
    fn each_zoom_draws_what_its_shot_is_named_after() {
        let s = Studio::sample();
        let frames_a_beat = f64::from(RATE) * 60.0 / 120.0;
        let at = |beat_w: f32| detail_for(frames_a_beat / f64::from(beat_w.min(s.beat_w_max())));
        assert_eq!(at(32.0), Detail::Peaks);
        assert_eq!(at(1_200.0), Detail::Samples);
        assert_eq!(at(48_000.0), Detail::Line { dots: false });
        assert_eq!(at(f32::MAX), Detail::Line { dots: true });
        assert_eq!(s.beat_w_max(), 144_000.0);
    }

    /// The last stage's pointer on the button shows the step it is at.
    #[test]
    fn the_last_shot_names_the_step_on_the_button() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 900.0);
        stage(&mut s, &g, 6);
        assert_eq!(s.wave_zoom, 4.0);
        assert_eq!(
            crate::hover_tip::text(&s, s.hover).as_deref(),
            Some("波形の縦の拡大 ×4")
        );
    }
}
