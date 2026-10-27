//! The spike window: a timeline with 200 tracks, a piano roll with 10,000
//! notes and 64 meters, plus the loop that drives them with real input events
//! and records the frame times (UI-023 / UI-024 / UI-029).
//!
//! The frame loop is `Window::on_next_frame`, which runs right before the
//! window draws. In it the benchmark builds wheel and mouse events and sends
//! them through `Window::dispatch_event`, so a scripted scroll takes the same
//! path as a person's: platform event -> hit test -> element handler.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, Context, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PlatformInput, Point, Render, ScrollDelta, ScrollWheelEvent, SharedString, TitlebarOptions,
    TouchPhase, Window, WindowBounds, WindowControlArea, WindowDecorations, WindowOptions, canvas,
    div, point, prelude::*, px, size, svg,
};

use crate::bench::{self, Meta, Runner, Tick};
use crate::chrome::{self, Probe};
use crate::data::{ARRANGEMENT_S, PART_BEATS, Project};
use crate::drive::{apply_scroll, apply_zoom, plan_axis, plan_rows, scale_at};
use crate::frame::{FrameLog, Pane};
use crate::meters::{EngineSim, MeterBus, MeterState};
use crate::paint::{
    Colors, PITCH_ROWS, RollView, TimelineView, paint_headers, paint_meters, paint_roll,
    paint_timeline,
};
use crate::park::{PARK_AT, park};
use crate::script::{REST_PR_ZOOM, REST_TL_ZOOM, SCENARIOS, Scenario};
use crate::theme::{METRICS, THEME, family};
use crate::viewport::{Axis, Rows};
use xirius_design::{Font, Radius};

pub const WINDOW_TITLE: &str = "Xirius UI spike";
const GPUI_VERSION: &str = "gpui-pre 0.3.8";

/// Pixels per second on the timeline: 3 shows the whole arrangement, 375 is
/// one base peak (128 samples at 48 kHz) per pixel.
const TL_SCALE: (f64, f64) = (3.0, 375.0);
/// Pixels per beat on the piano roll.
const PR_SCALE: (f64, f64) = (1.5, 240.0);
const PLAYHEAD_START_S: f64 = 20.0;
const SECONDS_PER_BEAT: f64 = 0.5;
/// Wheel "lines" are this many pixels (only used for real mouse wheels).
const LINE_PX: f32 = 20.0;

pub struct Options {
    pub scenarios: Vec<&'static Scenario>,
    /// Overrides the length of every scenario.
    pub secs: Option<f64>,
    pub out_dir: PathBuf,
    /// No script: the window stays open and can be used by hand.
    pub interactive: bool,
    /// Seconds to keep the window open after the last scenario (0 = until closed).
    pub hold_secs: f64,
    /// Logical size of the window.
    pub window: (f32, f32),
    /// Where the window goes. It is never made the active window, whatever
    /// the place, so a run does not take the keyboard from the person using the PC.
    pub place: Place,
    /// Ask the window what its title bar and borders answer to the system
    /// (hit tests, style bits, maximise / restore) and write `chrome.md`.
    pub chrome: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    /// Centred on the primary display.
    Centered,
    /// Wholly outside every display, so nothing of the person's screen is covered.
    Parked,
    /// Logical position of the top-left corner.
    At(f32, f32),
}

impl Default for Options {
    fn default() -> Self {
        Options {
            scenarios: SCENARIOS.iter().collect(),
            secs: None,
            out_dir: PathBuf::from("target/xirius-ui-bench"),
            interactive: false,
            hold_secs: 3.0,
            window: (1440.0, 900.0),
            place: Place::Centered,
            chrome: false,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn from(b: Bounds<gpui::Pixels>) -> Rect {
        Rect {
            x: f32::from(b.origin.x),
            y: f32::from(b.origin.y),
            w: f32::from(b.size.width),
            h: f32::from(b.size.height),
        }
    }

    fn known(&self) -> bool {
        self.w > 0.0 && self.h > 0.0
    }
}

#[derive(Clone, Copy, Default)]
struct Rects {
    headers: Rect,
    timeline: Rect,
    roll: Rect,
    meters: Rect,
}

/// How many scripted events reached a handler, to show the input path works.
#[derive(Clone, Copy, Default)]
struct Counts {
    sent: u64,
    wheel: u64,
    down: u64,
    moved: u64,
}

struct Shared {
    project: Rc<Project>,
    colors: Rc<Colors>,
    font: SharedString,

    tl_axis: Axis,
    tl_rows: Rows,
    pr_axis: Axis,
    pr_rows: Rows,
    playhead_s: f64,
    dragging: bool,
    drag_down: bool,

    meters: MeterState,
    bus: Arc<MeterBus>,
    _engine: Option<EngineSim>,
    meter_clock: Option<Instant>,

    log: Rc<RefCell<FrameLog>>,
    runner: Option<Runner>,
    cur: Option<&'static Scenario>,
    rects: Rects,
    counts: Counts,

    hud: String,
    overlay: Vec<String>,
    done_at: Option<Instant>,
    opts: Options,
    parked: bool,
    probe: Option<Probe>,
    probe_started: bool,
    window_px: (f32, f32),
    scale_factor: f32,
}

fn tl_axis() -> Axis {
    let base = Axis::new(0.0, TL_SCALE.0, TL_SCALE.0, TL_SCALE.1);
    Axis::new(
        0.0,
        scale_at(&base, zoom_rest(REST_TL_ZOOM)),
        TL_SCALE.0,
        TL_SCALE.1,
    )
}

fn pr_axis() -> Axis {
    let base = Axis::new(0.0, PR_SCALE.0, PR_SCALE.0, PR_SCALE.1);
    Axis::new(
        0.0,
        scale_at(&base, zoom_rest(REST_PR_ZOOM)),
        PR_SCALE.0,
        PR_SCALE.1,
    )
}

fn zoom_rest(pos: f64) -> f64 {
    pos
}

fn tl_rows(track_h: f32) -> Rows {
    Rows {
        scroll: 0.0,
        row_h: f64::from(track_h + METRICS.track_gap),
        count: crate::data::TRACK_COUNT,
    }
}

fn pr_rows() -> Rows {
    Rows {
        scroll: 0.0,
        row_h: f64::from(METRICS.note_row_h),
        count: PITCH_ROWS,
    }
}

fn wheel(pos: Point<gpui::Pixels>, dx: f64, dy: f64, ctrl: bool) -> PlatformInput {
    PlatformInput::ScrollWheel(ScrollWheelEvent {
        position: pos,
        delta: ScrollDelta::Pixels(point(px(dx as f32), px(dy as f32))),
        modifiers: Modifiers {
            control: ctrl,
            ..Modifiers::default()
        },
        touch_phase: TouchPhase::Moved,
    })
}

fn pt(x: f32, y: f32) -> Point<gpui::Pixels> {
    point(px(x), px(y))
}

impl Shared {
    fn new(opts: Options) -> Shared {
        let bus = Arc::new(MeterBus::default());
        let engine = EngineSim::spawn(bus.clone(), Duration::from_micros(5333));
        let log = Rc::new(RefCell::new(FrameLog::new(0)));
        let runner = (!opts.interactive).then(|| {
            let bus = bus.clone();
            Runner::new(
                opts.scenarios.clone(),
                log.clone(),
                opts.secs,
                Box::new(move || bus.blocks()),
            )
        });
        let started = Instant::now();
        let project = Rc::new(Project::synth());
        // design-lint: allow(ui-blocking-io) PRJ-031: explicit screenshot/benchmark harness output, outside normal Studio callbacks.
        println!(
            "synthesised {} tracks / {} clips / {} notes in {:.0} ms",
            project.tracks.len(),
            project.clip_count(),
            project.notes.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        Shared {
            project,
            colors: Rc::new(Colors::new(THEME)),
            font: family(Font::Inter),
            tl_axis: tl_axis(),
            tl_rows: tl_rows(METRICS.track_h),
            pr_axis: pr_axis(),
            pr_rows: pr_rows(),
            playhead_s: PLAYHEAD_START_S,
            dragging: false,
            drag_down: false,
            meters: MeterState::default(),
            bus,
            _engine: Some(engine),
            meter_clock: None,
            log,
            runner,
            cur: None,
            rects: Rects::default(),
            counts: Counts::default(),
            hud: String::new(),
            overlay: Vec::new(),
            done_at: None,
            parked: false,
            probe: None,
            probe_started: false,
            opts,
            window_px: (0.0, 0.0),
            scale_factor: 1.0,
        }
    }

    fn ready(&self) -> bool {
        let r = &self.rects;
        r.headers.known() && r.timeline.known() && r.roll.known() && r.meters.known()
    }

    fn track_h(&self) -> f32 {
        self.cur.map_or(METRICS.track_h, |s| s.track_h)
    }

    fn timeline_view(&self) -> TimelineView {
        TimelineView {
            axis: self.tl_axis,
            rows: self.tl_rows,
            playhead_s: self.playhead_s,
            track_h: self.track_h(),
        }
    }

    fn roll_view(&self) -> RollView {
        RollView {
            axis: self.pr_axis,
            rows: self.pr_rows,
            playhead_beat: self.playhead_s / SECONDS_PER_BEAT,
        }
    }

    fn tl_view_px(&self) -> (f64, f64) {
        let r = self.rects.timeline;
        (f64::from(r.w), f64::from((r.h - METRICS.ruler_h).max(0.0)))
    }

    fn pr_view_px(&self) -> (f64, f64) {
        let r = self.rects.roll;
        (
            f64::from((r.w - METRICS.piano_keys_w).max(0.0)),
            f64::from(r.h),
        )
    }

    // ------------------------------------------------------------ handlers

    fn on_timeline_wheel(&mut self, x: f32, dx: f32, dy: f32, ctrl: bool) {
        self.counts.wheel += 1;
        let (view_w, view_h) = self.tl_view_px();
        if ctrl {
            let anchor = f64::from(x - self.rects.timeline.x);
            apply_zoom(
                &mut self.tl_axis,
                anchor,
                f64::from(dy),
                view_w,
                ARRANGEMENT_S,
            );
        } else {
            apply_scroll(&mut self.tl_axis, f64::from(dx), view_w, ARRANGEMENT_S);
            self.tl_rows.scroll_by(-f64::from(dy), view_h);
        }
    }

    fn on_headers_wheel(&mut self, dy: f32) {
        self.counts.wheel += 1;
        let (_, view_h) = self.tl_view_px();
        self.tl_rows.scroll_by(-f64::from(dy), view_h);
    }

    fn on_roll_wheel(&mut self, x: f32, dx: f32, dy: f32, ctrl: bool) {
        self.counts.wheel += 1;
        let (view_w, view_h) = self.pr_view_px();
        if ctrl {
            let anchor = f64::from(x - (self.rects.roll.x + METRICS.piano_keys_w));
            apply_zoom(
                &mut self.pr_axis,
                anchor,
                f64::from(dy),
                view_w,
                f64::from(PART_BEATS),
            );
        } else {
            apply_scroll(
                &mut self.pr_axis,
                f64::from(dx),
                view_w,
                f64::from(PART_BEATS),
            );
            self.pr_rows.scroll_by(-f64::from(dy), view_h);
        }
    }

    fn scrub_to(&mut self, x: f32) {
        let px_in_view = f64::from(x - self.rects.timeline.x);
        self.playhead_s = self.tl_axis.to_unit(px_in_view).clamp(0.0, ARRANGEMENT_S);
    }

    fn on_timeline_down(&mut self, x: f32, y: f32) {
        self.counts.down += 1;
        if y - self.rects.timeline.y < METRICS.ruler_h {
            self.dragging = true;
            self.scrub_to(x);
        }
    }

    fn on_timeline_move(&mut self, x: f32, pressed: bool) {
        if pressed && self.dragging {
            self.counts.moved += 1;
            self.scrub_to(x);
        }
    }

    fn on_timeline_up(&mut self) {
        self.dragging = false;
    }

    // ----------------------------------------------------------- frame loop

    /// Everything that happens at the start of a frame, before the window
    /// draws. Returns the input events to send and whether to quit.
    fn begin(&mut self, now: Instant) -> (Vec<PlatformInput>, bool) {
        let mut events = Vec::new();
        let mut quit = false;
        if self.ready()
            && let Some(mut runner) = self.runner.take()
        {
            match runner.tick(now) {
                Tick::Run { scenario, t, first } => {
                    if first {
                        self.enter(scenario);
                    }
                    self.cur = Some(scenario);
                    self.drive(scenario, t, &mut events);
                }
                Tick::Ended { .. } => {
                    if self.drag_down {
                        self.drag_down = false;
                        let r = self.rects.timeline;
                        events.push(PlatformInput::MouseUp(MouseUpEvent {
                            button: MouseButton::Left,
                            position: pt(r.x + r.w / 2.0, r.y + METRICS.ruler_h / 2.0),
                            modifiers: Modifiers::default(),
                            click_count: 1,
                        }));
                    }
                    if let Some(last) = runner.results().last() {
                        // design-lint: allow(ui-blocking-io) PRJ-031: explicit screenshot/benchmark harness output, outside normal Studio callbacks.
                        println!("{}", bench::summary_line(last));
                    }
                    self.cur = None;
                }
                Tick::Done => {
                    if self.done_at.is_none() {
                        self.finish(&runner, now);
                    }
                    let hold = self.opts.hold_secs;
                    if let Some(at) = self.done_at {
                        quit = hold > 0.0 && now.duration_since(at).as_secs_f64() >= hold;
                    }
                }
            }
            self.runner = Some(runner);
        }
        self.counts.sent += events.len() as u64;
        self.log.borrow_mut().begin_frame(now);
        self.update_meters(now);
        self.update_hud();
        (events, quit)
    }

    /// Puts everything that the script does not move at its resting place.
    fn enter(&mut self, sc: &'static Scenario) {
        self.cur = Some(sc);
        let tg = sc.targets(0.0);
        self.tl_axis = tl_axis();
        self.tl_rows = tl_rows(sc.track_h);
        self.pr_axis = pr_axis();
        self.pr_rows = pr_rows();
        let (_, pr_h) = self.pr_view_px();
        let max = (self.pr_rows.content_h() - pr_h).max(0.0);
        self.pr_rows.scroll = tg.pr_y * max;
        self.playhead_s = PLAYHEAD_START_S;
        self.dragging = false;
        self.drag_down = false;
        self.meters = MeterState::default();
        self.meter_clock = None;
    }

    fn drive(&mut self, sc: &'static Scenario, t: f64, out: &mut Vec<PlatformInput>) {
        let tg = sc.targets(t);

        let tl = self.rects.timeline;
        let (view_w, view_h) = self.tl_view_px();
        let anchor = view_w / 2.0;
        let center = pt(
            tl.x + tl.w / 2.0,
            tl.y + METRICS.ruler_h + (view_h as f32) / 2.0,
        );
        let plan = plan_axis(
            &self.tl_axis,
            view_w,
            ARRANGEMENT_S,
            anchor,
            sc.tl_zoom.then_some(tg.tl_zoom),
            sc.tl_scroll.then_some(tg.tl_x),
        );
        if let Some(dy) = plan.zoom_dy {
            out.push(wheel(center, 0.0, dy, true));
        }
        let rows_dy = if sc.tl_scroll {
            plan_rows(&self.tl_rows, view_h, tg.tl_y)
        } else {
            None
        };
        if plan.scroll_dx.is_some() || rows_dy.is_some() {
            out.push(wheel(
                center,
                plan.scroll_dx.unwrap_or(0.0),
                rows_dy.unwrap_or(0.0),
                false,
            ));
        }

        let roll = self.rects.roll;
        let (rview_w, rview_h) = self.pr_view_px();
        let rcenter = pt(
            roll.x + METRICS.piano_keys_w + (rview_w as f32) / 2.0,
            roll.y + roll.h / 2.0,
        );
        let plan = plan_axis(
            &self.pr_axis,
            rview_w,
            f64::from(PART_BEATS),
            rview_w / 2.0,
            sc.pr_zoom.then_some(tg.pr_zoom),
            sc.pr_scroll.then_some(tg.pr_x),
        );
        if let Some(dy) = plan.zoom_dy {
            out.push(wheel(rcenter, 0.0, dy, true));
        }
        let rows_dy = if sc.pr_scroll {
            plan_rows(&self.pr_rows, rview_h, tg.pr_y)
        } else {
            None
        };
        if plan.scroll_dx.is_some() || rows_dy.is_some() {
            out.push(wheel(
                rcenter,
                plan.scroll_dx.unwrap_or(0.0),
                rows_dy.unwrap_or(0.0),
                false,
            ));
        }

        if let Some(frac) = tg.drag_x {
            let pos = pt(tl.x + (frac * view_w) as f32, tl.y + METRICS.ruler_h / 2.0);
            if !self.drag_down {
                self.drag_down = true;
                out.push(PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: pos,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                }));
            }
            out.push(PlatformInput::MouseMove(MouseMoveEvent {
                position: pos,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }));
        }
    }

    fn update_meters(&mut self, now: Instant) {
        let on = match self.cur {
            Some(sc) => sc.meters,
            None => self.runner.is_none(),
        };
        if !on {
            self.meter_clock = None;
            return;
        }
        let dt = self
            .meter_clock
            .map_or(1.0 / 60.0, |prev| now.duration_since(prev).as_secs_f32())
            .min(0.1);
        self.meter_clock = Some(now);
        let bus = self.bus.clone();
        self.meters.update(|ch| bus.take(ch), dt);
    }

    fn update_hud(&mut self) {
        let recent = self.log.borrow().recent(60);
        let what = match (self.cur, self.done_at) {
            (_, Some(_)) => "done".to_string(),
            (Some(sc), _) => sc.name.to_string(),
            (None, None) if self.opts.interactive => "interactive".to_string(),
            (None, None) => "starting".to_string(),
        };
        let fps = if recent.mean > 0.0 {
            1000.0 / recent.mean
        } else {
            0.0
        };
        self.hud = format!(
            "Xirius UI spike    {what}    {fps:.0} fps    interval p95 {:.1} ms  p99 {:.1} ms    {} tracks  {} notes  64 meters",
            recent.p95,
            recent.p99,
            self.project.tracks.len(),
            self.project.notes.len(),
        );
    }

    fn meta(&self, runner: &Runner) -> Meta {
        let cadence = runner
            .results()
            .first()
            .map_or(0.0, |r| r.summary.interval.p50);
        Meta {
            window_px: self.window_px,
            scale_factor: self.scale_factor,
            refresh_hz: if cadence > 0.0 { 1000.0 / cadence } else { 0.0 },
            tracks: self.project.tracks.len(),
            clips: self.project.clip_count(),
            notes: self.project.notes.len(),
            peak_mib: self.project.peak_bytes() as f64 / (1024.0 * 1024.0),
            gpui: GPUI_VERSION,
            os: std::env::consts::OS,
        }
    }

    fn finish(&mut self, runner: &Runner, now: Instant) {
        self.done_at = Some(now);
        self.cur = None;
        let meta = self.meta(runner);
        let md = format!(
            "Window placement: {:?}\n\n{}",
            self.opts.place,
            bench::markdown(&meta, runner.results())
        );
        let js = bench::json(&meta, runner.results());
        // design-lint: allow(ui-blocking-io) PRJ-031: explicit screenshot/benchmark harness output, outside normal Studio callbacks.
        println!("\n{md}");
        // design-lint: allow(ui-blocking-io) PRJ-031: explicit screenshot/benchmark harness output, outside normal Studio callbacks.
        println!(
            "input events sent {}, reached handlers: wheel {}, mouse-down {}, drag-move {}",
            self.counts.sent, self.counts.wheel, self.counts.down, self.counts.moved
        );
        let dir = &self.opts.out_dir;
        // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
        let wrote = std::fs::create_dir_all(dir)
            // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
            .and_then(|()| std::fs::write(dir.join("bench.md"), &md))
            // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
            .and_then(|()| std::fs::write(dir.join("bench.json"), &js));
        match wrote {
            // design-lint: allow(ui-blocking-io) PRJ-031: explicit screenshot/benchmark harness output, outside normal Studio callbacks.
            Ok(()) => println!("wrote {}", dir.display()),
            // design-lint: allow(ui-blocking-io) PRJ-031: explicit screenshot/benchmark harness output, outside normal Studio callbacks.
            Err(e) => eprintln!("could not write {}: {e}", dir.display()),
        }
        self.overlay = runner
            .results()
            .iter()
            .map(|r| {
                let s = &r.summary;
                let verdict = match r.pass {
                    Some(true) => "PASS",
                    Some(false) => "FAIL",
                    None => "ref",
                };
                format!(
                    "{:<34} {:>5.1} fps   p95 {:>5.1}   p99 {:>5.1}   dropped {:>3}   {verdict}",
                    r.scenario.name, s.fps, s.interval.p95, s.interval.p99, s.dropped
                )
            })
            .collect();
    }
}

/// Runs at the start of every frame and schedules itself for the next.
fn step(shared: &Rc<RefCell<Shared>>, window: &mut Window, cx: &mut App) {
    let now = Instant::now();
    {
        let mut s = shared.borrow_mut();
        let size = window.viewport_size();
        s.window_px = (f32::from(size.width), f32::from(size.height));
        s.scale_factor = window.scale_factor();
        if !s.parked && s.opts.place == Place::Parked {
            s.parked = park(WINDOW_TITLE, PARK_AT, PARK_AT);
        }
        if s.opts.chrome && !s.probe_started && s.window_px.0 > 0.0 {
            s.probe_started = true;
            let (w, h) = s.window_px;
            let queries = chrome::queries(w, h, METRICS.caption_button_w, METRICS.top_bar_h);
            s.probe = Probe::start(
                WINDOW_TITLE,
                s.scale_factor,
                queries,
                s.opts.out_dir.clone(),
            );
        }
    }
    if let Some(p) = shared.borrow().probe.as_ref() {
        p.frame();
    }
    // The probe's thread asks the window procedure where the pointer is; GPUI
    // answers from its last mouse position, so put that position there first.
    let at = shared.borrow().probe.as_ref().and_then(|p| p.pending());
    if let Some((index, x, y)) = at {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: pt(x, y),
                pressed_button: None,
                modifiers: Modifiers::default(),
            }),
            cx,
        );
        if let Some(p) = shared.borrow().probe.as_ref() {
            p.positioned(index);
        }
    }
    // Handlers borrow `shared` too, so it must not be held while dispatching.
    let (events, quit) = shared.borrow_mut().begin(now);
    // The chrome check writes its own report; the run is not over before it is.
    let quit = quit && shared.borrow().probe.as_ref().is_none_or(|p| p.done());
    for event in events {
        window.dispatch_event(event, cx);
    }
    window.refresh();
    if quit {
        cx.quit();
        return;
    }
    let next = shared.clone();
    window.on_next_frame(move |window, cx| step(&next, window, cx));
}

fn ms_since(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1000.0
}

/// The top bar and the window's title bar are one element: the OS title bar
/// is hidden (`appears_transparent`), the empty part of the bar drags the
/// window, and on Windows the three caption buttons sit at its right end. The
/// window procedure learns about these areas through `window_control_area`,
/// so moving, double-click to maximise, snapping and the Windows 11 snap
/// layouts are the system's own.
fn title_bar(window: &Window, c: &Colors, hud: String) -> impl IntoElement {
    let left = if cfg!(target_os = "macos") {
        METRICS.traffic_light_room
    } else {
        12.0
    };
    let drag = div()
        .flex_1()
        .h_full()
        .flex()
        .items_center()
        .pl(px(left))
        .window_control_area(WindowControlArea::Drag)
        .child(SharedString::from(hud));
    let mut bar = div()
        .w_full()
        .h(px(METRICS.top_bar_h))
        .flex_none()
        .flex()
        .flex_row()
        .bg(c.surface_1)
        .child(drag);
    if cfg!(windows) {
        let side = crate::caption::icon_side();
        for (i, (area, path)) in crate::caption::buttons(window.is_maximized())
            .into_iter()
            .enumerate()
        {
            let is_close = i == 2;
            let hover = if is_close { c.danger } else { c.surface_3 };
            let hover_ink = if is_close { c.on_fill } else { c.ink_strong };
            let group = SharedString::from(format!("caption-{i}"));
            bar = bar.child(
                div()
                    .group(group.clone())
                    .w(px(METRICS.caption_button_w))
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .window_control_area(area)
                    .hover(move |s| s.bg(hover))
                    .child(
                        svg()
                            .w(px(side))
                            .h(px(side))
                            .path(path)
                            .text_color(c.ink)
                            .group_hover(group, move |s| s.text_color(hover_ink)),
                    ),
            );
        }
    }
    bar
}

struct Workspace {
    shared: Rc<RefCell<Shared>>,
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let sh = &self.shared;
        let (hud, overlay, c) = {
            let s = sh.borrow();
            (s.hud.clone(), s.overlay.clone(), s.colors.clone())
        };

        let headers = div()
            .w(px(METRICS.header_w))
            .h_full()
            .flex_none()
            .on_scroll_wheel({
                let sh = sh.clone();
                move |ev, _, _| {
                    let d = ev.delta.pixel_delta(px(LINE_PX));
                    sh.borrow_mut().on_headers_wheel(f32::from(d.y));
                }
            })
            .child(
                canvas(|_, _, _| (), {
                    let sh = sh.clone();
                    move |b, (), window, cx| {
                        let t0 = Instant::now();
                        let (view, project, colors, font, log) = {
                            let mut s = sh.borrow_mut();
                            s.rects.headers = Rect::from(b);
                            (
                                s.timeline_view(),
                                s.project.clone(),
                                s.colors.clone(),
                                s.font.clone(),
                                s.log.clone(),
                            )
                        };
                        paint_headers(window, cx, b, &view, &project, &colors, &font);
                        log.borrow_mut().add_pane(Pane::Headers, ms_since(t0));
                    }
                })
                .size_full(),
            );

        let timeline = div()
            .flex_1()
            .h_full()
            .on_scroll_wheel({
                let sh = sh.clone();
                move |ev, window, _| {
                    let d = ev.delta.pixel_delta(px(LINE_PX));
                    sh.borrow_mut().on_timeline_wheel(
                        f32::from(ev.position.x),
                        f32::from(d.x),
                        f32::from(d.y),
                        ev.modifiers.control,
                    );
                    window.refresh();
                }
            })
            .on_mouse_down(MouseButton::Left, {
                let sh = sh.clone();
                move |ev, window, _| {
                    sh.borrow_mut()
                        .on_timeline_down(f32::from(ev.position.x), f32::from(ev.position.y));
                    window.refresh();
                }
            })
            .on_mouse_move({
                let sh = sh.clone();
                move |ev, window, _| {
                    let pressed = ev.pressed_button == Some(MouseButton::Left);
                    sh.borrow_mut()
                        .on_timeline_move(f32::from(ev.position.x), pressed);
                    window.refresh();
                }
            })
            .on_mouse_up(MouseButton::Left, {
                let sh = sh.clone();
                move |_, _, _| sh.borrow_mut().on_timeline_up()
            })
            .child(
                canvas(|_, _, _| (), {
                    let sh = sh.clone();
                    move |b, (), window, cx| {
                        let t0 = Instant::now();
                        let (view, project, colors, font, log) = {
                            let mut s = sh.borrow_mut();
                            s.rects.timeline = Rect::from(b);
                            (
                                s.timeline_view(),
                                s.project.clone(),
                                s.colors.clone(),
                                s.font.clone(),
                                s.log.clone(),
                            )
                        };
                        paint_timeline(window, cx, b, &view, &project, &colors, &font);
                        log.borrow_mut().add_pane(Pane::Timeline, ms_since(t0));
                    }
                })
                .size_full(),
            );

        let roll = div()
            .w_full()
            .h(px(240.0))
            .flex_none()
            .on_scroll_wheel({
                let sh = sh.clone();
                move |ev, window, _| {
                    let d = ev.delta.pixel_delta(px(LINE_PX));
                    sh.borrow_mut().on_roll_wheel(
                        f32::from(ev.position.x),
                        f32::from(d.x),
                        f32::from(d.y),
                        ev.modifiers.control,
                    );
                    window.refresh();
                }
            })
            .child(
                canvas(|_, _, _| (), {
                    let sh = sh.clone();
                    move |b, (), window, cx| {
                        let t0 = Instant::now();
                        let (view, project, colors, font, log) = {
                            let mut s = sh.borrow_mut();
                            s.rects.roll = Rect::from(b);
                            (
                                s.roll_view(),
                                s.project.clone(),
                                s.colors.clone(),
                                s.font.clone(),
                                s.log.clone(),
                            )
                        };
                        paint_roll(window, cx, b, &view, &project, &colors, &font);
                        log.borrow_mut().add_pane(Pane::PianoRoll, ms_since(t0));
                    }
                })
                .size_full(),
            );

        // Painted last: its end marks the end of the frame for the frame log.
        let meters = div()
            .w_full()
            .h(px(METRICS.meter_strip_h))
            .flex_none()
            .child(
                canvas(|_, _, _| (), {
                    let sh = sh.clone();
                    move |b, (), window, _| {
                        let t0 = Instant::now();
                        let (state, colors, log) = {
                            let mut s = sh.borrow_mut();
                            s.rects.meters = Rect::from(b);
                            (s.meters.clone(), s.colors.clone(), s.log.clone())
                        };
                        paint_meters(window, b, &state, &colors);
                        let end = Instant::now();
                        let mut log = log.borrow_mut();
                        log.add_pane(Pane::Meters, end.duration_since(t0).as_secs_f64() * 1000.0);
                        log.end_frame(end);
                    }
                })
                .size_full(),
            );

        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(c.surface_0)
            .text_color(c.ink)
            .font_family(family(Font::Inter))
            .text_size(px(METRICS.text_size))
            .child(title_bar(window, &c, hud))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .min_h(px(0.0))
                    .flex()
                    .flex_row()
                    .child(headers)
                    .child(timeline),
            )
            .child(roll)
            .child(meters);

        if !overlay.is_empty() {
            root = root.child(
                div()
                    .absolute()
                    .top(px(METRICS.top_bar_h + 24.0))
                    .left(px(METRICS.header_w + 24.0))
                    .p(px(16.0))
                    .bg(c.surface_3)
                    .rounded(px(Radius::S.px()))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .text_color(c.ink_strong)
                    .children(
                        overlay
                            .into_iter()
                            .map(|line| div().child(SharedString::from(line))),
                    ),
            );
        }
        root
    }
}

/// Opens the window and runs until the script ends (or the window closes).
pub fn run(opts: Options) {
    gpui_platform::application().run(move |cx: &mut App| {
        if let Err(e) = crate::fonts::load(cx) {
            // design-lint: allow(ui-blocking-io) PRJ-031: explicit screenshot/benchmark harness output, outside normal Studio callbacks.
            eprintln!("could not load the bundled fonts: {e}");
        }
        let (w, h) = opts.window;
        let bounds = match opts.place {
            Place::Centered => Bounds::centered(None, size(px(w), px(h)), cx),
            Place::At(x, y) => Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
            // Opened centred, then moved away by `step` (see park.rs).
            Place::Parked => Bounds::centered(None, size(px(w), px(h)), cx),
        };
        let shared = Rc::new(RefCell::new(Shared::new(opts)));
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let handle = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // One bar for the app and the window: the OS title bar is
                // hidden and `title_bar` draws the bar and the buttons.
                titlebar: Some(TitlebarOptions {
                    title: Some(WINDOW_TITLE.into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(
                        px(METRICS.traffic_light_x),
                        px(METRICS.traffic_light_y),
                    )),
                }),
                window_decorations: Some(WindowDecorations::Client),
                // Without this an unfocused window is held to 30 fps, which
                // would make a run in the background look like a failure.
                inactive_frame_interval: None,
                // Shown without taking the focus (SW_SHOWNOACTIVATE).
                focus: false,
                ..WindowOptions::default()
            },
            {
                let shared = shared.clone();
                move |window, cx| {
                    let first = shared.clone();
                    window.on_next_frame(move |window, cx| step(&first, window, cx));
                    cx.new(|_| Workspace { shared })
                }
            },
        );
        if let Err(e) = handle {
            // design-lint: allow(ui-blocking-io) PRJ-031: explicit screenshot/benchmark harness output, outside normal Studio callbacks.
            eprintln!("could not open the window: {e:#}");
            cx.quit();
        }
    });
}
