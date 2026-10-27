//! Runs the scenarios one after another and writes the numbers (UI-023 /
//! UI-024 / UI-029).
//!
//! The runner does not know about GPUI: the app calls [`Runner::tick`] once at
//! the start of every frame and does what the returned [`Tick`] says.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;
use std::time::Instant;

use crate::frame::{Criteria, Distribution, FrameLog, FrameSummary, Pane};
use crate::script::{Scenario, Spike};

/// Frames dropped from the start of every scenario: glyph / path / atlas
/// warm-up and the zoom jump from the resting view.
pub const WARMUP_FRAMES: usize = 30;

pub struct ScenarioResult {
    pub scenario: &'static Scenario,
    pub summary: FrameSummary,
    /// `None` for reference scenarios, which have no pass line.
    pub pass: Option<bool>,
    /// Meter blocks the fake engine published during the scenario.
    pub engine_blocks: u32,
}

pub enum Tick {
    /// Drive `scenario` at `t` seconds since it started. `first` is true on
    /// the first frame of a scenario.
    Run {
        scenario: &'static Scenario,
        t: f64,
        first: bool,
    },
    /// `scenario` has just ended; nothing is driven this frame.
    Ended { scenario: &'static Scenario },
    /// Every scenario has ended.
    Done,
}

pub struct Runner {
    queue: Vec<&'static Scenario>,
    next: usize,
    started: Option<Instant>,
    current: Option<&'static Scenario>,
    log: Rc<RefCell<FrameLog>>,
    results: Vec<ScenarioResult>,
    secs_override: Option<f64>,
    engine_blocks: Box<dyn Fn() -> u32>,
    blocks_at_start: u32,
}

impl Runner {
    pub fn new(
        queue: Vec<&'static Scenario>,
        log: Rc<RefCell<FrameLog>>,
        secs_override: Option<f64>,
        engine_blocks: Box<dyn Fn() -> u32>,
    ) -> Runner {
        Runner {
            queue,
            next: 0,
            started: None,
            current: None,
            log,
            results: Vec::new(),
            secs_override,
            engine_blocks,
            blocks_at_start: 0,
        }
    }

    pub fn results(&self) -> &[ScenarioResult] {
        &self.results
    }

    pub fn current(&self) -> Option<&'static Scenario> {
        self.current
    }

    pub fn is_done(&self) -> bool {
        self.current.is_none() && self.next >= self.queue.len() && !self.results.is_empty()
    }

    pub fn tick(&mut self, now: Instant) -> Tick {
        if let (Some(scenario), Some(started)) = (self.current, self.started) {
            let t = now.duration_since(started).as_secs_f64();
            let secs = self.secs_override.unwrap_or(scenario.secs);
            if t < secs {
                return Tick::Run {
                    scenario,
                    t,
                    first: false,
                };
            }
            self.finish(scenario);
            return Tick::Ended { scenario };
        }
        match self.queue.get(self.next).copied() {
            Some(scenario) => {
                self.next += 1;
                self.current = Some(scenario);
                self.started = Some(now);
                self.blocks_at_start = (self.engine_blocks)();
                *self.log.borrow_mut() = FrameLog::new(WARMUP_FRAMES);
                Tick::Run {
                    scenario,
                    t: 0.0,
                    first: true,
                }
            }
            None => Tick::Done,
        }
    }

    fn finish(&mut self, scenario: &'static Scenario) {
        let summary = self.log.borrow().summarize(Criteria::FPS_60);
        let pass = match scenario.spike {
            Spike::Reference => None,
            Spike::One | Spike::Two => Some(summary.pass),
        };
        let engine_blocks = (self.engine_blocks)().saturating_sub(self.blocks_at_start);
        self.results.push(ScenarioResult {
            scenario,
            summary,
            pass,
            engine_blocks,
        });
        self.current = None;
        self.started = None;
    }
}

/// Facts about the run that are not frame times.
pub struct Meta {
    pub window_px: (f32, f32),
    pub scale_factor: f32,
    pub refresh_hz: f64,
    pub tracks: usize,
    pub clips: usize,
    pub notes: usize,
    pub peak_mib: f64,
    pub gpui: &'static str,
    pub os: &'static str,
}

fn ms(x: f64) -> String {
    format!("{x:.2}")
}

fn verdict(pass: Option<bool>) -> &'static str {
    match pass {
        Some(true) => "PASS",
        Some(false) => "FAIL",
        None => "ref",
    }
}

/// One-line form for stdout while the run is going.
pub fn summary_line(r: &ScenarioResult) -> String {
    let s = &r.summary;
    format!(
        "{:<34} {:>5} frames  {:>6.1} fps  interval mean {} p95 {} p99 {} max {} ms  dropped {}  cpu mean {} p99 {} ms  {}",
        r.scenario.name,
        s.frames,
        s.fps,
        ms(s.interval.mean),
        ms(s.interval.p95),
        ms(s.interval.p99),
        ms(s.interval.max),
        s.dropped,
        ms(s.cpu.mean),
        ms(s.cpu.p99),
        verdict(r.pass),
    )
}

pub fn markdown(meta: &Meta, results: &[ScenarioResult]) -> String {
    let mut out = String::new();
    // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
    let _ = writeln!(
        out,
        "window {}x{} px at scale {}, display {} Hz, {} tracks / {} clips / {} notes, peaks {:.1} MiB, {} on {}\n",
        meta.window_px.0,
        meta.window_px.1,
        meta.scale_factor,
        meta.refresh_hz,
        meta.tracks,
        meta.clips,
        meta.notes,
        meta.peak_mib,
        meta.gpui,
        meta.os,
    );
    // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
    let _ = writeln!(
        out,
        "| scenario | frames | fps | interval mean | p95 | p99 | max | dropped | cpu mean | cpu p95 | cpu p99 | result |"
    );
    // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
    let _ = writeln!(
        out,
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|"
    );
    for r in results {
        let s = &r.summary;
        // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
        let _ = writeln!(
            out,
            "| {} | {} | {:.1} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            r.scenario.name,
            s.frames,
            s.fps,
            ms(s.interval.mean),
            ms(s.interval.p95),
            ms(s.interval.p99),
            ms(s.interval.max),
            s.dropped,
            ms(s.cpu.mean),
            ms(s.cpu.p95),
            ms(s.cpu.p99),
            verdict(r.pass),
        );
    }
    // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
    let _ = writeln!(out, "\nPaint time per pane (ms, mean / p99):\n");
    // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
    let _ = write!(out, "| scenario |");
    for pane in Pane::ALL {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
        let _ = write!(out, " {} |", pane.name());
    }
    // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
    let _ = writeln!(out);
    // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
    let _ = writeln!(out, "|---|{}", "---:|".repeat(Pane::ALL.len()));
    for r in results {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
        let _ = write!(out, "| {} |", r.scenario.name);
        for d in &r.summary.panes {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
            let _ = write!(out, " {} / {} |", ms(d.mean), ms(d.p99));
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
        let _ = writeln!(out);
    }
    out
}

fn json_dist(d: &Distribution) -> String {
    format!(
        "{{\"count\":{},\"mean\":{:.4},\"min\":{:.4},\"p50\":{:.4},\"p95\":{:.4},\"p99\":{:.4},\"max\":{:.4}}}",
        d.count, d.mean, d.min, d.p50, d.p95, d.p99, d.max
    )
}

pub fn json(meta: &Meta, results: &[ScenarioResult]) -> String {
    let mut out = String::new();
    // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
    let _ = write!(
        out,
        "{{\"window\":[{},{}],\"scale_factor\":{},\"refresh_hz\":{},\"tracks\":{},\"clips\":{},\"notes\":{},\"peak_mib\":{:.2},\"gpui\":\"{}\",\"os\":\"{}\",\"scenarios\":[",
        meta.window_px.0,
        meta.window_px.1,
        meta.scale_factor,
        meta.refresh_hz,
        meta.tracks,
        meta.clips,
        meta.notes,
        meta.peak_mib,
        meta.gpui,
        meta.os,
    );
    for (i, r) in results.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let s = &r.summary;
        let panes: Vec<String> = Pane::ALL
            .iter()
            .zip(&s.panes)
            .map(|(p, d)| format!("\"{}\":{}", p.name(), json_dist(d)))
            .collect();
        let pass = match r.pass {
            Some(true) => "true",
            Some(false) => "false",
            None => "null",
        };
        // design-lint: allow(ui-blocking-io) PRJ-031 review: fmt::Write into a local String for a benchmark report, with no file/stream destination.
        let _ = write!(
            out,
            "{{\"name\":\"{}\",\"frames\":{},\"fps\":{:.3},\"dropped\":{},\"pass\":{},\"engine_blocks\":{},\"interval_ms\":{},\"cpu_ms\":{},\"panes_ms\":{{{}}}}}",
            r.scenario.name,
            s.frames,
            s.fps,
            s.dropped,
            pass,
            r.engine_blocks,
            json_dist(&s.interval),
            json_dist(&s.cpu),
            panes.join(","),
        );
    }
    out.push_str("]}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::SCENARIOS;
    use std::time::Duration;

    fn runner(names: &[&str], secs: f64) -> (Runner, Rc<RefCell<FrameLog>>) {
        let queue = names
            .iter()
            .map(|n| crate::script::find(n).expect("known scenario"))
            .collect();
        let log = Rc::new(RefCell::new(FrameLog::new(0)));
        (
            Runner::new(queue, log.clone(), Some(secs), Box::new(|| 0)),
            log,
        )
    }

    #[test]
    fn runs_scenarios_in_order_then_finishes() {
        let (mut r, _log) = runner(&["still", "meters-only"], 1.0);
        let t0 = Instant::now();
        let mut seen = Vec::new();
        for frame in 0..400u64 {
            let now = t0 + Duration::from_millis(frame * 16);
            match r.tick(now) {
                Tick::Run {
                    scenario, first, ..
                } => {
                    if first {
                        seen.push(format!("start {}", scenario.name));
                    }
                }
                Tick::Ended { scenario } => seen.push(format!("end {}", scenario.name)),
                Tick::Done => {
                    seen.push("done".into());
                    break;
                }
            }
        }
        assert_eq!(
            seen,
            [
                "start still",
                "end still",
                "start meters-only",
                "end meters-only",
                "done"
            ]
        );
        assert!(r.is_done());
        assert_eq!(r.results().len(), 2);
    }

    #[test]
    fn results_use_the_frames_logged_during_the_scenario() {
        let (mut r, log) = runner(&["spike1-all-moving"], 1.0);
        let t0 = Instant::now();
        for frame in 0..200u64 {
            let now = t0 + Duration::from_millis(frame * 20);
            log.borrow_mut().begin_frame(now);
            let tick = r.tick(now);
            log.borrow_mut().end_frame(now + Duration::from_millis(2));
            if matches!(tick, Tick::Done) {
                break;
            }
        }
        let res = &r.results()[0];
        assert!(res.summary.frames > 10);
        // 20 ms frames are 50 fps: below the 60 fps line.
        assert_eq!(res.pass, Some(false));
    }

    #[test]
    fn reference_scenarios_have_no_verdict() {
        let (mut r, log) = runner(&["still"], 0.5);
        let t0 = Instant::now();
        for frame in 0..100u64 {
            let now = t0 + Duration::from_millis(frame * 50);
            log.borrow_mut().begin_frame(now);
            let tick = r.tick(now);
            log.borrow_mut().end_frame(now);
            if matches!(tick, Tick::Done) {
                break;
            }
        }
        assert_eq!(r.results()[0].pass, None);
    }

    #[test]
    fn reports_name_every_scenario() {
        let (mut r, log) = runner(&["still", "meters-only"], 0.4);
        let t0 = Instant::now();
        for frame in 0..200u64 {
            let now = t0 + Duration::from_millis(frame * 16);
            log.borrow_mut().begin_frame(now);
            let tick = r.tick(now);
            log.borrow_mut().end_frame(now + Duration::from_millis(1));
            if matches!(tick, Tick::Done) {
                break;
            }
        }
        let meta = Meta {
            window_px: (1600.0, 900.0),
            scale_factor: 1.0,
            refresh_hz: 60.0,
            tracks: 200,
            clips: 1800,
            notes: 10_000,
            peak_mib: 12.3,
            gpui: "gpui-pre 0.3.8",
            os: "test",
        };
        let md = markdown(&meta, r.results());
        let js = json(&meta, r.results());
        for name in ["still", "meters-only"] {
            assert!(md.contains(name));
            assert!(js.contains(&format!("\"name\":\"{name}\"")));
        }
        assert!(js.starts_with('{') && js.ends_with("]}"));
        assert_eq!(js.matches('{').count(), js.matches('}').count());
        assert!(!summary_line(&r.results()[0]).is_empty());
    }

    #[test]
    fn every_scenario_fits_the_default_runner() {
        for s in SCENARIOS {
            assert!(s.secs > 0.0);
        }
    }
}
