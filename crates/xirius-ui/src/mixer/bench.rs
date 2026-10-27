//! UI-024 on the real mixer: 64 strips playing, measured in the real window
//! (`xirius-ui --bench --mixer`).
//!
//! The sample song's instruments are duplicated up to 64 strips and looped over
//! bars 5 to 8, where every one of them plays, to a silent output. Four runs of
//! frames are timed: the mixer still, then scrolled across all the strips and back,
//! then scrolled the same way with the song stopped, then once more with the loop
//! off from two bars before the song's end, so the song plays out and stops inside
//! the run. Frames of [`SLOW_MS`] or more are listed with where they fell.

use std::f32::consts::TAU;
use std::path::PathBuf;
use std::time::Instant;

use serde_json::{Value, json};
use xirius_core::model::TrackKind;

use super::strip_order;
use crate::frame::{Criteria, Distribution};
use crate::studio::{DrawerTab, Geometry, Hit, Studio};

/// Strips in the song, the master and the buses included.
pub const STRIPS: usize = 64;
/// Frames let go before each run is timed.
const WARM: usize = 60;
/// Frames timed in each run.
const FRAMES: usize = 600;
const RUNS: [&str; 4] = ["still", "scroll", "scroll_stopped", "scroll_to_end"];
/// A frame this long or longer is listed on its own, ms.
const SLOW_MS: f64 = 20.0;

#[derive(Default)]
struct Samples {
    /// From the start of one frame to the next, ms.
    frames: Vec<f64>,
    /// The mixer's paint, ms.
    paints: Vec<f64>,
    /// Strips on screen.
    shown: Vec<usize>,
    /// Strips whose meter showed a level.
    metered: Vec<usize>,
    /// The transport played in that frame.
    playing: Vec<bool>,
}

pub struct Bench {
    out: PathBuf,
    ready: bool,
    warm: usize,
    run: usize,
    last: Option<Instant>,
    samples: Samples,
    results: Vec<Value>,
}

/// The song's strips made up to [`STRIPS`] by duplicating, in turn, the instruments
/// that play all through the loop.
pub fn prepare(s: &mut Studio) -> Result<(), String> {
    let p = s.project();
    let tm = p.time_map();
    let (start, end) = (tm.bar_start(5), tm.bar_start(9));
    let sources: Vec<String> = strip_order(&p)
        .into_iter()
        .filter(|id| {
            let t = &p.tracks[id];
            t.kind == TrackKind::Instrument
                && t.clips.iter().any(|c| c.start <= start && c.end() >= end)
        })
        .map(|id| id.to_string())
        .collect();
    let mut i = 0;
    while strip_order(&s.project()).len() < STRIPS {
        let id = &sources[i % sources.len()];
        // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
        s.song.run("track.duplicate", json!({ "ids": [id] }))?;
        i += 1;
    }
    s.song
        // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
        .run("loop.set", json!({ "start": start, "end": end }))?;
    s.toggle_loop();
    // design-lint: allow(ui-blocking-io) PRJ-031: transport playhead seek updates memory and submits an engine command.
    s.seek(start);
    Ok(())
}

impl Bench {
    pub fn new(out: PathBuf) -> Self {
        Bench {
            out,
            ready: false,
            warm: WARM,
            run: 0,
            last: None,
            samples: Samples::default(),
            results: Vec::new(),
        }
    }

    /// Called at the start of every frame. True when the measurement is written.
    pub fn tick(&mut self, s: &mut Studio, g: &Geometry) -> bool {
        if !self.ready {
            // Through the tab, as a press would; then play.
            s.press(g, Hit::Tab(DrawerTab::Mixer));
            s.toggle_play();
            self.ready = true;
        }
        if self.warm > 0 {
            self.warm -= 1;
            self.last = None;
            return false;
        }
        let now = Instant::now();
        if let Some(last) = self.last {
            let m = &mut self.samples;
            m.frames
                .push(now.duration_since(last).as_secs_f64() * 1000.0);
            m.paints.push(s.mixer.paint_us.replace(0) as f64 / 1000.0);
            m.shown.push(s.mixer_strips(g).len());
            m.metered.push(s.mixer.meters.count());
            m.playing.push(s.transport.playing);
        }
        self.last = Some(now);
        if RUNS[self.run].starts_with("scroll") {
            // Once across every strip and back over the run.
            let t = self.samples.frames.len() as f32 / FRAMES as f32;
            s.mixer.scroll = s.mixer_max_scroll(g) * (0.5 - 0.5 * (t * TAU).cos());
        }
        if self.samples.frames.len() < FRAMES {
            return false;
        }
        let m = std::mem::take(&mut self.samples);
        self.results
            .push(summary(RUNS[self.run], &m, s.transport.playing));
        self.run += 1;
        if self.run < RUNS.len() {
            match RUNS[self.run] {
                "scroll_stopped" => {
                    s.toggle_play();
                }
                "scroll_to_end" => {
                    // Without the loop, two bars before the end: the song plays out and
                    // stops a few seconds into the run.
                    s.toggle_loop();
                    let p = s.project();
                    let end = xirius_session::graph::song_end_tick(&p);
                    // design-lint: allow(ui-blocking-io) PRJ-031: transport playhead seek updates memory and submits an engine command.
                    s.seek(end - i64::from(p.ppq) * 8);
                    s.toggle_play();
                }
                _ => {}
            }
            self.warm = 10;
            return false;
        }
        let data = json!({
            "strips": strip_order(&s.project()).len(),
            "window": [g.size.0, g.size.1 + xirius_design::Layout::TOP_BAR_H + xirius_design::Layout::BOTTOM_BAR_H],
            "error": s.error,
            "criteria": { "min_fps": Criteria::FPS_60.min_fps, "max_p99_ms": Criteria::FPS_60.max_p99_ms },
            "runs": self.results,
        });
        // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
        if let Err(e) = std::fs::create_dir_all(&self.out).and_then(|()| {
            // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
            std::fs::write(
                self.out.join("mixer-bench.json"),
                serde_json::to_vec_pretty(&data).unwrap_or_default(),
            )
        }) {
            // design-lint: allow(ui-blocking-io) PRJ-031: explicit screenshot/benchmark harness output, outside normal Studio callbacks.
            eprintln!("could not write {}: {e}", self.out.display());
        }
        // design-lint: allow(ui-blocking-io) PRJ-031: explicit screenshot/benchmark harness output, outside normal Studio callbacks.
        println!("{data}");
        true
    }
}

fn summary(name: &str, m: &Samples, playing: bool) -> Value {
    let f = Distribution::from_samples(&m.frames);
    let p = Distribution::from_samples(&m.paints);
    let c = Criteria::FPS_60;
    let fps = 1000.0 / f.mean;
    let span = |v: &[usize]| json!([v.iter().min(), v.iter().max()]);
    json!({
        "run": name,
        "playing": playing,
        "frames": f.count,
        "fps": fps,
        "frame_ms": { "mean": f.mean, "p50": f.p50, "p95": f.p95, "p99": f.p99, "max": f.max },
        "mixer_paint_ms": { "mean": p.mean, "p99": p.p99, "max": p.max },
        "strips_shown": span(&m.shown),
        "strips_metered": span(&m.metered),
        "slow_frames": slow(m),
        "ended_at": m.playing.windows(2).position(|w| w[0] && !w[1]).map(|i| i + 1),
        "pass_60fps": fps >= c.min_fps && f.p99 <= c.max_p99_ms,
    })
}

/// Every frame of [`SLOW_MS`] or more: where in the run, how long, the mixer's paint and
/// whether the song played.
fn slow(m: &Samples) -> Vec<Value> {
    m.frames
        .iter()
        .enumerate()
        .filter(|(_, ms)| **ms >= SLOW_MS)
        .map(|(i, ms)| json!({ "at": i, "ms": ms, "paint_ms": m.paints[i], "playing": m.playing[i] }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_measured_song_has_64_strips_and_loops_where_they_play() {
        let mut s = Studio::sample();
        prepare(&mut s).unwrap();
        let p = s.project();
        assert_eq!(strip_order(&p).len(), STRIPS);
        let tm = p.time_map();
        let (start, end) = (tm.bar_start(5), tm.bar_start(9));
        assert_eq!(p.loop_range.map(|r| (r.start, r.end)), Some((start, end)));
        assert!(s.transport.looping);
        assert_eq!(s.playhead, start);
        let playing = p
            .tracks
            .values()
            .filter(|t| t.clips.iter().any(|c| c.start <= start && c.end() >= end))
            .count();
        assert!(playing >= 56, "{playing}");
    }

    #[test]
    fn slow_frames_and_the_end_of_the_song_are_found_in_a_run() {
        let m = Samples {
            frames: vec![16.6, 22.4, 16.7, 20.0],
            paints: vec![0.5, 0.9, 0.4, 0.6],
            shown: vec![13; 4],
            metered: vec![61, 61, 0, 0],
            playing: vec![true, true, false, false],
        };
        let v = summary("scroll_to_end", &m, false);
        let slow = v["slow_frames"].as_array().unwrap();
        assert_eq!(slow.len(), 2);
        assert_eq!(slow[0]["at"], 1);
        assert_eq!(slow[0]["playing"], true);
        assert_eq!(slow[1]["at"], 3);
        assert_eq!(v["ended_at"], 2);
        assert!(RUNS.contains(&"scroll_to_end"));
    }
}
