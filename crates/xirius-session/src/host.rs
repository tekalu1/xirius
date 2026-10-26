//! Keeps a running engine in step with the song (ENG-005, ENG-006, ARC-013).
//!
//! After a command changes the song, [`SessionHost::sync`] compares it with what the engine was
//! last given and sends only what differs, as small commands through the engine's lock-free queue:
//! faders, pans, mutes, solos (as mutes), send levels, and a track's notes and audio. When the
//! routing or an insert row changes, a whole new mix is built here and swapped in. The audio
//! thread never builds, allocates or frees anything for either.
//!
//! VST3 plugins in the plugin host process (OPEN-02) take a new state or parameter from the
//! song while they run, without a new mix. What the user changes in their editors comes back
//! through [`collect_plugin_states`], which writes it into the song as one undoable step and
//! hands the parameter moves on for automation (AUT-002). [`SessionHost::plugin_statuses`] says
//! whether the host went down, and [`SessionHost::restart_plugins`] brings it back.
//!
//! The sample rate is fixed for the life of the engine.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use xirius_core::clipfx::ClipFx;
use xirius_core::id::{ClipId, PluginId, TrackId};
use xirius_core::model::{Plugin, Project, TempoPoint, Track};
use xirius_core::session::{RunOptions, RunResult};
use xirius_core::time::Tick;
use xirius_core::{CmdResult, Session};
use xirius_engine::engine::create;
use xirius_engine::mixer::{MeterReading, NodeId, PlanError};
use xirius_engine::server::{EditPhase, PluginStatus};
use xirius_engine::{
    AudioEngine, Command, Controller, EngineConfig, EngineEvent, Plan, RenderMode,
};

use crate::automation;
use crate::build::{Builder, setup};
use crate::clap_plugins::is_hosted;
use crate::graph::{Graph, Mix, Scope, Select, StripMix, Structure};
use crate::plugins::{Isolation, PluginLoader};
use crate::tempo::TempoMap;

thread_local! {
    static WINDOW_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
/// Registers the event-loop thread so synchronous compatibility APIs cannot
/// instantiate an SDK there, even if a caller bypasses the window host facade.
pub fn mark_window_thread() {
    WINDOW_THREAD.with(|v| v.set(true));
}
pub(crate) fn on_window_thread() -> bool {
    WINDOW_THREAD.with(std::cell::Cell::get)
}

#[derive(Default)]
pub struct HostConfig {
    /// Uses a dedicated controller owner for each local VST3 instance (PRJ-030).
    pub threaded_local: bool,
    /// The engine's sample rate. `None` takes the song's setting.
    pub rate: Option<u32>,
    /// The folder audio paths in the song are relative to.
    pub dir: Option<PathBuf>,
    pub engine: EngineConfig,
    /// Where VST3 plugins are looked up (the standard cache when `None`).
    pub catalog: Option<Arc<xirius_plugins::Catalog>>,
    /// Where VST3 plugins run (`None`: [`Isolation::default`]).
    pub isolation: Option<Isolation>,
}

/// What one [`SessionHost::sync`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// A whole new mix was swapped in.
    pub swapped: bool,
    /// Small commands queued.
    pub commands: usize,
    /// The recording's monitored tracks were sent (`Command::Thru`), so the values changed
    /// while a take records reach the monitor (UI-312).
    pub thru: bool,
    /// Differences that could not be queued because the queue was full; the next sync retries.
    pub deferred: usize,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Status {
    pub playing: bool,
    /// The song position in frames, as of the last [`SessionHost::poll`].
    pub frame: u64,
}

/// One move of a plugin parameter in its editor, placed on the song (AUT-002). Touch writes
/// from `Begin` to `End`; Latch writes from `Begin` until the transport stops; Write writes
/// every `Change` while playing.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamEvent {
    pub plugin: PluginId,
    /// The parameter's numeric id, and its title as the plugin names it.
    pub param: u32,
    pub title: String,
    pub phase: EditPhase,
    /// Normalised 0..1, on `Change`.
    pub value: Option<f64>,
    /// When the edit reached this process.
    pub at: Instant,
    /// The song frame it falls on, when the transport was playing.
    pub frame: Option<u64>,
}

/// What [`collect_plugin_states`] did.
#[derive(Debug, Default)]
pub struct Collected {
    /// The command that wrote the plugins' states into the song (one undo step), if any.
    pub result: Option<RunResult>,
    /// The parameter moves, oldest first.
    pub params: Vec<ParamEvent>,
}

/// The longest a meter waits for the engine to say it took a new mix.
const SWAP_WAIT: std::time::Duration = std::time::Duration::from_millis(250);

/// A strip whose level the engine reports (MIX-010).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Strip {
    /// A track or a bus.
    Track(TrackId),
    Master,
}

/// What a new mix that waits for its plugins is for (ENG-065).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SwapKind {
    /// The song's routing or an insert row changed.
    Sync,
    /// A plugin's latency changed: the mix is built again to compensate it.
    Latency,
}

/// A new mix whose plugins are being opened while the old one plays on (ENG-065).
struct PendingSwap {
    /// What the new mix is built from.
    target: Project,
    kind: SwapKind,
    /// The plugins it opens, and what each is on.
    hosted: Vec<(String, Plugin)>,
    /// The plugins that run in a host process were asked for.
    asked: bool,
    warnings: Vec<String>,
    /// A plugin's latency changed since the mix that plays was built: when what the new mix is
    /// for is given up, a mix is still built for the latency (ENG-068).
    latency: bool,
}

/// The transport values the engine was last given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Sent {
    loop_frames: Option<(u64, u64)>,
    end: Option<u64>,
}

/// The clip data last queued for one track. Fades, envelopes and loops are stored outside
/// `Track`, so sharing the track alone does not mean its timeline is unchanged.
struct TimelineState {
    track: Arc<Track>,
    fx: BTreeMap<ClipId, ClipFx>,
}

impl TimelineState {
    fn new(p: &Project, track: &Arc<Track>) -> Self {
        Self {
            track: track.clone(),
            fx: track
                .clips
                .iter()
                .filter_map(|c| p.fx(c.id).map(|fx| (c.id, fx.clone())))
                .collect(),
        }
    }

    fn all(p: &Project) -> BTreeMap<TrackId, Self> {
        p.tracks
            .iter()
            .map(|(id, t)| (*id, Self::new(p, t)))
            .collect()
    }

    fn matches(&self, p: &Project, t: &Arc<Track>) -> bool {
        (Arc::ptr_eq(&self.track, t)
            || (self.track.clips == t.clips
                && self.track.frozen == t.frozen
                && self.track.transpose == t.transpose
                && self.track.velocity == t.velocity))
            && t.clips.iter().all(|c| self.fx.get(&c.id) == p.fx(c.id))
    }
}

mod material;

pub struct SessionHost {
    material: Option<material::Update>,
    material_dirty: bool,
    live: BTreeMap<TrackId, bool>,
    song: Project,
    latency_dirty: bool,
    /// A new mix whose plugins are opening ([`SessionHost::step_swap`]).
    pending: Option<PendingSwap>,
    /// A change of latency builds its mix without waiting for the plugins too.
    nonblocking: bool,
    windowed: bool,
    automation_start: u64,
    seek_serial: u64,
    pending_seek: Option<u64>,
    controller: Controller,
    builder: Builder,
    graph: Graph,
    pub(crate) structure: Structure,
    mix: Mix,
    tracks: BTreeMap<TrackId, TimelineState>,
    tempo: (u32, Vec<TempoPoint>),
    tmap: TempoMap,
    pub(crate) loop_ticks: Option<(i64, i64)>,
    end_frame: u64,
    looping: bool,
    sent: Sent,
    status: Status,
    /// The song while it has automation to follow as it plays.
    automated: Option<Project>,
    /// Plugin parameter lanes sent to the running plugins.
    params: automation::ParamFollow,
    /// When `status` was taken.
    polled: Option<Instant>,
    /// Levels reported since [`SessionHost::take_levels`] last ran.
    levels: BTreeMap<Strip, MeterReading>,
    /// When a new mix was sent that the engine has not said it took: until then its meter
    /// indices are the old mix's. The word can be lost when the event queue is full, so it is
    /// not waited for longer than [`SWAP_WAIT`].
    swapping: Option<std::time::Instant>,
    /// The [`crate::processed::generation`] the timelines were last built at.
    processed_seen: u64,
    /// The recording under way ([`crate::record`]).
    pub(crate) recording: Option<crate::record::Active>,
    /// Where `record.start` takes its inputs from.
    pub(crate) record_sources: crate::record::Sources,
    pub io_settings: Option<xirius_prefs::Shared>,
    /// The input held for watching its level and for the monitor ([`crate::watch`]).
    pub(crate) watch: Option<crate::watch::Watch>,
    /// The MIDI output ports tracks send to ([`crate::midi_out`]).
    pub(crate) midi_out: crate::midi_out::MidiOutState,
    /// How the input device is opened (the system's, unless a test puts in stand-ins).
    pub drivers: crate::output::Drivers,
    /// What was said about the last input device opened, until it is taken.
    pub(crate) driver_notice: Option<String>,
    /// The engine's busy time as of the last [`SessionHost::cpu`] reading.
    cpu: CpuWindow,
}

/// How busy the whole callback is, and how many blocks missed their time (ENG-008).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cpu {
    /// Processing time over the audio time since the last reading: 0.4 is 40%.
    pub load: f64,
    /// Callbacks that took longer than the buffer they filled, and device underruns.
    pub xruns: u64,
    /// Callbacks so far.
    pub callbacks: u64,
}

#[derive(Default)]
struct CpuWindow {
    at: Option<Instant>,
    callbacks: u64,
    frames: u64,
    busy_ns: u128,
    last: Cpu,
}

/// A reading is not renewed more often than this, so the load is over a stretch long enough
/// not to jump.
const CPU_WINDOW: Duration = Duration::from_millis(500);

impl SessionHost {
    pub fn begin_pull_states(&mut self) -> crate::plugins::StatePull {
        self.builder.plugins_mut().begin_pull_states()
    }

    /// Save As moved this same song. Future loads and recordings use its new folder;
    /// current plugin instances, the audio engine and the transport stay alive (ARC-043).
    pub fn relocate(&mut self, dir: Option<PathBuf>) {
        if self.builder.dir() != dir.as_deref() {
            self.builder.set_dir(dir);
            // Relative paths may be unchanged while naming different files. The next sync
            // sends fresh timelines, including frozen tracks, without replacing the mix.
            self.tracks.clear();
        }
    }

    /// The catalogue VST3 plugins are looked up in from the next rebuild on.
    pub fn set_catalog(&mut self, catalog: Option<Arc<xirius_plugins::Catalog>>) {
        self.builder.set_catalog(catalog);
    }

    /// Builds the engine for a song. Hand the [`AudioEngine`] to a device or render it yourself.
    /// The plugins that run in host processes are asked for together; to open them a little at
    /// a time, use [`crate::starting::Starting`] (ENG-064).
    pub fn start(
        p: &Project,
        config: HostConfig,
    ) -> Result<(SessionHost, AudioEngine, Vec<String>), PlanError> {
        crate::starting::Starting::new(p, config).finish(p)
    }

    /// As [`SessionHost::start`], for a window (ENG-065): a plugin in a host process that has
    /// not opened yet plays dry until it has, and does not hold the window.
    pub fn start_late(
        p: &Project,
        config: HostConfig,
    ) -> Result<(SessionHost, AudioEngine, Vec<String>), PlanError> {
        crate::starting::Starting::new(p, config).finish_late(p)
    }

    /// The host of a mix `builder` built for `p`.
    pub(crate) fn assemble(
        p: &Project,
        builder: Builder,
        mut built: crate::build::Built,
        setup: xirius_engine::ProcessSetup,
        engine: EngineConfig,
        processed_seen: u64,
    ) -> Result<(SessionHost, AudioEngine, Vec<String>), PlanError> {
        let plan = Plan::build(built.spec, setup, None)?;
        built.warnings.extend(builder.plugins().opening_warnings());
        builder.note_open_times();
        let (audio, controller) = create(plan, engine);
        let tmap = TempoMap::from_project(p);
        let mut host = SessionHost {
            material: None,
            material_dirty: false,
            live: p
                .tracks
                .iter()
                .map(|(id, t)| (*id, automation::realtime(t)))
                .collect(),
            song: p.clone(),
            latency_dirty: false,
            pending: None,
            nonblocking: false,
            windowed: false,
            automation_start: 0,
            seek_serial: 0,
            pending_seek: None,
            controller,
            builder,
            graph: built.graph,
            structure: built.structure,
            mix: built.mix,
            tracks: TimelineState::all(p),
            tempo: (p.ppq, p.tempo.clone()),
            tmap,
            loop_ticks: None,
            end_frame: built.end_frame,
            looping: false,
            sent: Sent::default(),
            status: Status::default(),
            automated: None,
            params: automation::ParamFollow::default(),
            polled: None,
            levels: BTreeMap::new(),
            swapping: None,
            processed_seen,
            recording: None,
            record_sources: crate::record::Sources::default(),
            io_settings: None,
            watch: None,
            midi_out: Default::default(),
            drivers: Default::default(),
            driver_notice: None,
            cpu: CpuWindow::default(),
        };
        host.send_transport(p);
        host.follow_song(p);
        host.sweep_processed();
        Ok((host, audio, built.warnings))
    }

    pub fn rate(&self) -> u32 {
        self.builder.rate()
    }

    /// Preparation failures kept silent for this player, without changing the song.
    pub fn unavailable_tracks(&self) -> &BTreeMap<TrackId, String> {
        self.builder.unavailable()
    }

    /// Retry an unpublished preparation candidate on its worker. No device has
    /// taken `audio` yet: applying the swap with an empty buffer moves retained
    /// nodes without processing sound, then reclaims the old plan on this worker.
    pub fn retry_prepared(
        &mut self,
        audio: &mut AudioEngine,
        p: &Project,
        mut track: impl FnMut(TrackId, bool),
        cancelled: impl Fn() -> bool,
    ) -> Result<Vec<crate::build::PreparationFailure>, String> {
        let keep = self.builder.retry_materials(p);
        let hosted: Vec<_> = crate::starting::hosted(p)
            .into_iter()
            .filter(|(_, x)| !keep.contains(&x.id))
            .collect();
        let setup = setup(self.rate(), RenderMode::Realtime);
        for (owner, x) in &hosted {
            if cancelled() {
                return Err("中止しました".into());
            }
            self.builder.plugins_mut().preload(owner, x, &setup);
        }
        loop {
            if cancelled() {
                return Err("中止しました".into());
            }
            self.builder.plugins_mut().poll_preloads();
            let loader = self.builder.plugins();
            if hosted
                .iter()
                .all(|(_, x)| loader.preloaded(x.id) && !loader.preparing(x.id))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        self.builder.set_background(false);
        let built = self.builder.build_observed(p, Select::All, &mut track);
        self.builder.set_background(true);
        if let Some(failure) = built.failures.iter().find(|f| f.track.is_none()) {
            return Err(format!("{}: {}", failure.name, failure.reason));
        }
        #[cfg(feature = "test-io")]
        xirius_core::io_test::before(
            "prepare_graph",
            self.builder
                .dir()
                .unwrap_or_else(|| std::path::Path::new("")),
        )
        .map_err(|e| e.to_string())?;
        if cancelled() {
            return Err("中止しました".into());
        }
        self.controller
            .swap(built.spec)
            .map_err(|e| e.to_string())?;
        audio.render(&mut [], 0);
        self.poll();
        self.builder.plugins_mut().promote_opened();
        self.builder.note_open_times();
        self.graph = built.graph;
        self.structure = built.structure;
        self.mix = built.mix;
        self.end_frame = built.end_frame;
        Ok(built.failures)
    }

    /// The line said when the input device was opened in another mode than asked for (the
    /// exclusive mode that would not open), once.
    pub fn take_driver_notice(&mut self) -> Option<String> {
        self.driver_notice.take()
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn tempo_map(&self) -> &TempoMap {
        &self.tmap
    }

    /// The frame where the last clip ends.
    pub fn end_frame(&self) -> u64 {
        self.end_frame
    }

    pub fn controller(&mut self) -> &mut Controller {
        &mut self.controller
    }

    /// The engine's counters, readable without the controller.
    pub(crate) fn engine_stats(&self) -> std::sync::Arc<xirius_engine::stats::Stats> {
        self.controller.stats()
    }

    /// Takes in what the engine reported: the position, whether it plays and the levels.
    pub fn poll(&mut self) -> Status {
        let status = &mut self.status;
        let levels = &mut self.levels;
        if self.swapping.is_some_and(|t| t.elapsed() > SWAP_WAIT) {
            self.swapping = None;
        }
        let swapping = &mut self.swapping;
        let structure = &self.structure;
        let pending_seek = &mut self.pending_seek;
        let automation_start = &mut self.automation_start;
        let loop_start = self
            .loop_ticks
            .map(|(start, _)| self.tmap.frame_at(start, f64::from(self.builder.rate())));
        self.controller.poll(|ev| match ev {
            EngineEvent::Transport {
                playing,
                position,
                serial,
            } => {
                if pending_seek.is_some_and(|wanted| wanted != serial) {
                    return;
                }
                *pending_seek = None;
                if position < status.frame {
                    *automation_start = loop_start.unwrap_or(position);
                }
                status.playing = playing;
                status.frame = position;
            }
            EngineEvent::Meter { node, reading } if swapping.is_none() => {
                let strip = match node {
                    NodeId::Track(i) => structure.tracks.get(i).map(|s| Strip::Track(s.id)),
                    NodeId::Bus(i) => structure
                        .buses
                        .get(i)
                        .filter(|s| s.id != crate::graph::DISCARD)
                        .map(|s| Strip::Track(s.id)),
                    NodeId::Master => Some(Strip::Master),
                };
                if let Some(strip) = strip {
                    let held = levels.entry(strip).or_default();
                    for ch in 0..2 {
                        held.peak[ch] = held.peak[ch].max(reading.peak[ch]);
                        held.rms[ch] = held.rms[ch].max(reading.rms[ch]);
                    }
                }
            }
            EngineEvent::PlanSwapped => *swapping = None,
            EngineEvent::Position(_) | EngineEvent::Meter { .. } => {}
        });
        self.polled = Some(Instant::now());
        // The windows that moved with the last mix, as the hosts that had them let go.
        self.builder.plugins_mut().settle_following();
        self.follow();
        self.refresh_latency();
        self.refresh_processed();
        self.status
    }

    /// Where the sound of an audio clip with a tempo, speed or pitch of its own is (`None` for a
    /// clip that plays its file as it is, or one not built yet).
    pub fn clip_state(&self, clip: &xirius_core::model::Clip) -> Option<crate::processed::State> {
        let key =
            crate::processed::clip_key(clip, &self.tmap, self.builder.rate(), self.builder.dir())?;
        crate::processed::state(&key)
    }

    /// Sends again the timelines of the tracks with a clip whose render finished, so it is
    /// heard (it was left silent while rendering).
    fn refresh_processed(&mut self) {
        let now = crate::processed::generation();
        // Window plans are published only after strict material preparation.
        // Their timeline updates use that same worker path. Notifications from
        // another render must not re-enter Builder::timeline on the event loop.
        if self.windowed {
            if self.processed_seen != now {
                self.processed_seen = now;
                self.material_dirty = true;
                let song = self.song.clone();
                self.begin_material(&song, &mut SyncReport::default());
            }
            return;
        }
        if now == self.processed_seen {
            return;
        }
        self.processed_seen = now;
        let song = self.song.clone();
        let (rate, mut warnings) = (self.builder.rate(), Vec::new());
        for (i, id) in self.graph.tracks.clone().iter().enumerate() {
            let Some(t) = song.tracks.get(id) else {
                continue;
            };
            let dir = self.builder.dir();
            if !t
                .clips
                .iter()
                .any(|c| crate::processed::clip_key(c, &self.tmap, rate, dir).is_some())
            {
                continue;
            }
            let timeline = self.builder.timeline(&song, t, &self.tmap, &mut warnings);
            if !self.send(Command::SetTimeline {
                track: i,
                timeline: Box::new(timeline),
            }) {
                // The queue is full: try again at the next poll.
                self.processed_seen = now.wrapping_sub(1);
            }
        }
    }

    /// Stops the renders the song no longer plays.
    fn sweep_processed(&self) {
        let keys = crate::processed::keys_of(
            &self.song,
            &self.tmap,
            self.builder.rate(),
            self.builder.dir(),
        );
        crate::processed::sweep(self.builder.owner(), &keys);
    }

    fn refresh_latency(&mut self) {
        self.latency_dirty |= self.builder.plugins_mut().take_latency_changes();
        if self.latency_dirty && self.nonblocking {
            // A mix waiting for its plugins builds with the latency they report.
            match &mut self.pending {
                Some(pd) => pd.latency = true,
                None => {
                    let heard = self.heard_now();
                    self.begin_swap(heard, SwapKind::Latency, Vec::new());
                }
            }
            self.latency_dirty = false;
        } else if self.latency_dirty {
            let song = self.song.clone();
            let timing = crate::automation_timing::Timing::new(
                &song,
                &self.graph,
                &self.controller.shape(),
                self.builder.plugins(),
            )
            .with_start(self.automation_start);
            let heard =
                timing.song_at(&song, self.status.frame, &self.tmap, f64::from(self.rate()));
            let mut report = SyncReport::default();
            self.swap_all(&heard, &mut report);
            if report.swapped {
                self.latency_dirty = false;
                self.params.clear();
                // Comparison snapshots remain the song's values; the new mix starts at
                // the current automation values, without a block at the base fader/mute.
                self.structure = Structure::of(&song, &self.graph, &Scope::new(&song, Select::All));
                self.tracks = TimelineState::all(&song);
                self.follow();
            }
        }
    }

    /// Remembers the song when it has automation, and sends its values at the position.
    fn follow_song(&mut self, p: &Project) {
        self.automated = automation::has_lanes(p).then(|| p.clone());
        self.follow();
    }

    /// Sends what the automation sets at the reported position (AUT-001).
    fn follow(&mut self) {
        let Some(song) = &self.automated else {
            return;
        };
        let timing = crate::automation_timing::Timing::new(
            song,
            &self.graph,
            &self.controller.shape(),
            self.builder.plugins(),
        );
        let timing = timing.with_start(self.automation_start);
        let at = timing.song_at(song, self.status.frame, &self.tmap, f64::from(self.rate()));
        let mix = Mix::of(&at, &self.graph, &Scope::new(&at, Select::All));
        self.send_mix(&mix, &mut SyncReport::default());
        let song = self.automated.as_ref().expect("checked above");
        self.params.send_each(
            song,
            |id, target| {
                timing.tick(
                    id,
                    target,
                    self.status.frame,
                    &self.tmap,
                    f64::from(self.builder.rate()),
                )
            },
            self.builder.plugins(),
        );
    }

    /// The song position now, between polls as well: the last reported frame moved on by the
    /// time since. `None` while stopped.
    pub fn tick_now(&self) -> Option<Tick> {
        let frame = self.frame_at(Instant::now())?;
        Some(self.tmap.tick_of_frame(frame, f64::from(self.rate())))
    }

    /// The meters of each plugin's processing time, by the number of its id.
    pub fn plugin_timings(&self) -> Arc<xirius_engine::timing::Timings> {
        self.builder.timings()
    }

    /// The load of the whole callback over the last stretch, and the blocks that missed.
    pub fn cpu(&mut self) -> Cpu {
        let now = Instant::now();
        let snap = self.controller.stats().snapshot();
        let rate = f64::from(self.rate()).max(1.0);
        let w = &mut self.cpu;
        let xruns = snap.deadline_misses + snap.device_xruns;
        let Some(at) = w.at else {
            *w = CpuWindow {
                at: Some(now),
                callbacks: snap.callbacks,
                frames: snap.frames,
                busy_ns: snap.busy_total.as_nanos(),
                last: Cpu {
                    load: 0.0,
                    xruns,
                    callbacks: snap.callbacks,
                },
            };
            return w.last;
        };
        if now.duration_since(at) >= CPU_WINDOW {
            let frames = snap.frames.saturating_sub(w.frames);
            let busy = snap.busy_total.as_nanos().saturating_sub(w.busy_ns);
            if frames > 0 {
                w.last.load = busy as f64 / (frames as f64 / rate * 1e9);
            } else if snap.callbacks == w.callbacks {
                // Nothing is playing: the engine is idle.
                w.last.load = 0.0;
            }
            w.at = Some(now);
            w.callbacks = snap.callbacks;
            w.frames = snap.frames;
            w.busy_ns = snap.busy_total.as_nanos();
        }
        w.last.xruns = xruns;
        w.last.callbacks = snap.callbacks;
        w.last
    }

    pub fn plugins(&self) -> &PluginLoader {
        self.builder.plugins()
    }

    /// The song as the engine was last given it.
    pub(crate) fn song(&self) -> &Project {
        &self.song
    }

    /// How each plugin in the plugin host process is: for the UI to show one that crashed.
    pub fn plugin_statuses(&self) -> Vec<(PluginId, String, PluginStatus)> {
        self.builder.plugins().statuses()
    }

    /// Starts the plugin host again after a crash; each plugin comes back with the state it
    /// had and takes over from the dry signal.
    pub fn restart_plugins(&self) -> Result<(), String> {
        self.builder.plugins().restart()
    }

    /// Starts the host process one plugin runs in again, as [`SessionHost::restart_plugins`]
    /// does for the processes that went down.
    pub fn restart_plugin(&self, id: PluginId) -> Result<(), String> {
        self.builder.plugins().restart_of(id)
    }

    /// As [`SessionHost::restart_plugins`], without waiting for the plugins to open again
    /// (ENG-065): each plays dry until it is back.
    pub fn restart_plugins_later(&self) -> Result<(), String> {
        self.builder.plugins().restart_later()
    }

    /// As [`SessionHost::restart_plugin`], without waiting (ENG-065).
    pub fn restart_plugin_later(&self, id: PluginId) -> Result<(), String> {
        self.builder.plugins().restart_of_later(id)
    }

    /// What the host processes said about the plugins the player took before they answered
    /// (a plugin that cannot be opened, parameters it does not have), once they have.
    pub fn take_late_warnings(&mut self) -> Vec<String> {
        self.builder.plugins_mut().take_late_warnings()
    }

    /// The track as the engine plays it now: a new mix that waits for its plugins has not
    /// replaced it yet.
    pub(crate) fn played_track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.get(&id).map(|t| t.track.as_ref())
    }

    /// Opens or closes a plugin's editor (in the plugin host process).
    pub fn plugin_editor(
        &self,
        id: PluginId,
        open: bool,
        caption: Option<xirius_vst3::Caption>,
    ) -> Result<(), String> {
        self.builder.plugins().editor(id, open, caption)
    }

    /// Gives the editor of `id` that is open in its host process new caption colours.
    pub fn plugin_caption(&self, id: PluginId, caption: xirius_vst3::Caption) {
        self.builder.plugins().set_caption(id, caption);
    }

    /// The song frame `at` falls on, from the position of the last [`SessionHost::poll`].
    fn frame_at(&self, at: Instant) -> Option<u64> {
        let polled = self.polled?;
        if !self.status.playing {
            return None;
        }
        let rate = f64::from(self.rate());
        let delta = if at >= polled {
            (at - polled).as_secs_f64() * rate
        } else {
            -((polled - at).as_secs_f64() * rate)
        };
        Some((self.status.frame as f64 + delta).max(0.0).round() as u64)
    }

    /// The loudest level of each strip since the last call (peak and RMS, linear, per
    /// channel), as reported between [`SessionHost::poll`]s. A strip that reported nothing is
    /// missing. The readings come through the engine's lock-free event queue.
    pub fn take_levels(&mut self) -> BTreeMap<Strip, MeterReading> {
        std::mem::take(&mut self.levels)
    }

    /// Where a track is among the engine's tracks.
    pub(crate) fn track_index(&self, id: TrackId) -> Option<usize> {
        self.graph.tracks.iter().position(|t| *t == id)
    }

    pub(crate) fn send(&mut self, cmd: Command) -> bool {
        self.controller.send(cmd).is_ok()
    }

    pub fn play(&mut self) -> bool {
        self.send(Command::Play)
    }

    pub fn stop(&mut self) -> bool {
        self.send(Command::Stop)
    }

    pub fn seek_frame(&mut self, frame: u64) -> bool {
        let serial = self.seek_serial.wrapping_add(1).max(1);
        if !self.send(Command::SeekTracked { frame, serial }) {
            return false;
        }
        self.seek_serial = serial;
        self.pending_seek = Some(serial);
        self.status.frame = frame;
        self.automation_start = frame;
        self.polled = Some(Instant::now());
        self.follow();
        true
    }

    pub fn seek_tick(&mut self, tick: i64) -> bool {
        let frame = self.tmap.frame_at(tick, f64::from(self.rate()));
        self.seek_frame(frame)
    }

    /// Turns repeating of the song's loop range on or off.
    pub fn set_looping(&mut self, p: &Project, on: bool) {
        self.looping = on;
        self.send_transport(p);
    }

    pub fn looping(&self) -> bool {
        self.looping
    }

    fn send_transport(&mut self, p: &Project) -> bool {
        let rate = f64::from(self.rate());
        self.loop_ticks = p.loop_range.map(|l| (l.start, l.end));
        let want = Sent {
            loop_frames: match (self.looping, self.loop_ticks) {
                (true, Some((s, e))) => {
                    Some((self.tmap.frame_at(s, rate), self.tmap.frame_at(e, rate)))
                }
                _ => None,
            },
            end: self.end_wanted(),
        };
        let mut ok = true;
        if want.loop_frames != self.sent.loop_frames {
            if self.send(Command::SetLoop(want.loop_frames)) {
                self.sent.loop_frames = want.loop_frames;
            } else {
                ok = false;
            }
        }
        self.send_end() && ok
    }

    /// Where the transport stops by itself: the song's end, or nowhere while recording (a take
    /// may go on past the last clip).
    fn end_wanted(&self) -> Option<u64> {
        match self.recording {
            Some(_) => None,
            None => Some(self.end_frame),
        }
    }

    pub(crate) fn send_end(&mut self) -> bool {
        let end = self.end_wanted();
        if end == self.sent.end {
            return true;
        }
        let ok = self.send(Command::SetEnd(end));
        if ok {
            self.sent.end = end;
        }
        ok
    }

    /// Brings the engine up to date with the song.
    /// A mix on its way for a new latency is kept unless this builds one (ENG-068): the
    /// screen syncs so when it takes a window's edit into the song, often in the frame that
    /// heard of the latency the edit changed.
    pub fn sync(&mut self, p: &Project) -> SyncReport {
        // Window hosts must never fall back to synchronous construction, including
        // when editor/state collection calls this shared entry point.
        if self.nonblocking {
            return self.sync_later(p);
        }
        if !self.pending.as_ref().is_some_and(|pd| pd.latency) {
            self.pending = None;
            self.builder.plugins_mut().drop_ready();
        }
        let report = self.sync_with(p, false);
        if report.swapped {
            self.pending = None;
            self.builder.plugins_mut().drop_ready();
        } else {
            self.settle_pending();
        }
        report
    }

    /// As [`SessionHost::sync`], but a new mix is built without holding the caller (ENG-065):
    /// the plugins it needs open while the old mix plays on, and [`SessionHost::step_swap`],
    /// called once a frame, swaps the new mix in when they are open. Small changes go through
    /// at once.
    pub fn sync_later(&mut self, p: &Project) -> SyncReport {
        self.sync_with(p, true)
    }

    /// Sends the recording's thru the track values it should play with now (UI-312).
    fn sync_thru(&mut self, p: &Project, report: &mut SyncReport) {
        let Some(rec) = &self.recording else {
            return;
        };
        let midi = rec.midi.clone();
        let thru: Vec<xirius_engine::record::Thru> = midi
            .iter()
            .filter_map(|&(t, port)| {
                let track = self.track_index(t)?;
                let values = p.tracks.get(&t);
                Some(xirius_engine::record::Thru {
                    track,
                    port,
                    transpose: values.map_or(0, |x| x.transpose),
                    velocity: values.map_or(0, |x| x.velocity),
                })
            })
            .collect();
        if self.recording.as_ref().is_some_and(|r| r.thru == thru) {
            return;
        }
        if self.send(Command::Thru(thru.clone())) {
            if let Some(rec) = self.recording.as_mut() {
                rec.thru = thru;
            }
            report.commands += 1;
            report.thru = true;
        } else {
            report.deferred += 1;
        }
    }

    fn sync_with(&mut self, p: &Project, later: bool) -> SyncReport {
        self.song = p.clone();
        let mut report = SyncReport::default();
        // The song's values may have been given to the plugins again: the lanes are sent anew.
        self.params.clear();
        let graph = Graph::of(p);
        let scope = Scope::new(p, Select::All);
        let structure = Structure::of(p, &graph, &scope);
        if structure != self.structure
            && graph == self.graph
            && self.builder.plugins().live()
            && without_plugin_values(&structure) == without_plugin_values(&self.structure)
            && self.apply_plugin_values(&structure, &mut report)
        {
            self.structure = structure.clone();
        }
        // Ordinary edits retain the player, output and plugin controllers. Only
        // changed material is prepared on a worker; cheap mix commands still run.
        if self.windowed && (graph != self.graph || structure != self.structure) {
            self.send_pending_mix(p, &mut report);
            self.sync_thru(p, &mut report);
            self.retarget_swap(p);
            return report;
        }
        if self.windowed {
            self.begin_material(p, &mut report);
        }
        if graph != self.graph || structure != self.structure {
            if later {
                self.retarget_swap(p);
                return report;
            }
            self.swap_all(p, &mut report);
            // The engine's track numbers changed with the new mix: the monitor's tracks are
            // made again from the new graph (UI-312).
            self.sync_thru(p, &mut report);
            self.sync_midi_out(&mut report);
            self.send_transport(p);
            self.follow_song(p);
            self.sweep_processed();
            return report;
        }
        self.settle_pending();

        let tempo_changed = self.tempo.0 != p.ppq || self.tempo.1 != p.tempo;
        if tempo_changed && !self.windowed {
            self.tempo = (p.ppq, p.tempo.clone());
            self.tmap = TempoMap::from_project(p);
            let bpm = self.tmap.bpm_at(0);
            self.builder.plugins_mut().set_tempo(bpm);
        }
        self.automated = automation::has_lanes(p).then(|| p.clone());
        let timing = crate::automation_timing::Timing::new(
            p,
            &graph,
            &self.controller.shape(),
            self.builder.plugins(),
        )
        .with_start(self.automation_start);
        let heard = timing.song_at(p, self.status.frame, &self.tmap, f64::from(self.rate()));
        let mix = Mix::of(&heard, &graph, &scope);
        self.send_mix(&mix, &mut report);
        self.params.send_each(
            p,
            |id, target| {
                timing.tick(
                    id,
                    target,
                    self.status.frame,
                    &self.tmap,
                    f64::from(self.builder.rate()),
                )
            },
            self.builder.plugins(),
        );

        for (i, id) in graph.tracks.iter().enumerate() {
            let t = &p.tracks[id];
            let live = automation::realtime(t);
            if self.live.get(id) != Some(&live) {
                if self.send(Command::TrackLive { track: i, live }) {
                    self.live.insert(*id, live);
                    report.commands += 1;
                } else {
                    report.deferred += 1;
                }
            }
            let same = self.tracks.get(id).is_some_and(|old| old.matches(p, t));
            if same && !tempo_changed {
                continue;
            }
            if self.windowed {
                continue;
            }
            let tl = self
                .builder
                .timeline(p, t, &self.tmap, &mut report.warnings);
            if self.send(Command::SetTimeline {
                track: i,
                timeline: Box::new(tl),
            }) {
                self.tracks.insert(*id, TimelineState::new(p, t));
                report.commands += 1;
            } else {
                report.deferred += 1;
            }
        }
        // While a take records, its monitored tracks follow the values as they are changed
        // (UI-312).
        self.sync_thru(p, &mut report);
        // Tracks that changed in ways that never reach the engine (a color, a name) are
        // remembered too, so the next comparison is quick.
        for (id, t) in &p.tracks {
            if let Some(old) = self.tracks.get_mut(id)
                && old.matches(p, t)
            {
                old.track = t.clone();
            }
        }

        let new_end = TempoMap::from_project(p)
            .frame_at(crate::graph::song_end_tick(p), f64::from(self.rate()));
        self.end_frame = new_end;
        if !self.send_transport(p) {
            report.deferred += 1;
        }
        self.sync_midi_out(&mut report);
        self.sweep_processed();
        report
    }

    /// Gives running plugins the state and parameters the song now has for them. `false` when
    /// one needs a new mix.
    fn apply_plugin_values(&mut self, new: &Structure, report: &mut SyncReport) -> bool {
        let rows = |s: &Structure| -> Vec<Plugin> {
            s.tracks
                .iter()
                .chain(&s.buses)
                .flat_map(|n| n.inserts.iter().cloned())
                .chain(s.master_inserts.iter().cloned())
                .collect()
        };
        let (old, new) = (rows(&self.structure), rows(new));
        let loader = self.builder.plugins_mut();
        for (a, b) in old.iter().zip(&new) {
            if !same_values(a, b) {
                if self.windowed && a.samples != b.samples {
                    return false;
                }
                let running = is_hosted(b) || loader.is_stock(b.id);
                if !b.runs() || a.deactivated != b.deactivated || !running || !loader.apply(b) {
                    return false;
                }
                report.warnings.extend(loader.take_file_warnings());
                report.commands += 1;
            }
        }
        true
    }

    fn swap_all(&mut self, p: &Project, report: &mut SyncReport) {
        let built = self.builder.build(p, Select::All);
        report.warnings.extend(built.warnings);
        let swapped = self.controller.swap(built.spec);
        report
            .warnings
            .extend(self.builder.plugins().opening_warnings());
        self.builder.note_open_times();
        match swapped {
            Ok(()) => {
                self.builder.plugins_mut().promote_opened();
                self.live = p
                    .tracks
                    .iter()
                    .map(|(id, t)| (*id, automation::realtime(t)))
                    .collect();
                self.swapping = Some(std::time::Instant::now());
                self.levels.clear();
                self.graph = built.graph;
                self.structure = built.structure;
                self.mix = built.mix;
                self.tracks = TimelineState::all(p);
                self.tempo = (p.ppq, p.tempo.clone());
                self.tmap = TempoMap::from_project(p);
                self.end_frame = built.end_frame;
                report.swapped = true;
            }
            Err(e) => {
                report.deferred += 1;
                report
                    .warnings
                    .push(format!("the new mix was not sent: {e}"));
            }
        }
    }

    /// The song as it is heard at the position: the values the automation gives there.
    fn heard_now(&self) -> Project {
        let song = &self.song;
        let timing = crate::automation_timing::Timing::new(
            song,
            &self.graph,
            &self.controller.shape(),
            self.builder.plugins(),
        )
        .with_start(self.automation_start);
        timing.song_at(song, self.status.frame, &self.tmap, f64::from(self.rate()))
    }

    /// A new mix for `p` is wanted: begins opening what it needs, or, when one is on its way
    /// already, points it at `p` (and opens again only if its plugins are not the same).
    fn retarget_swap(&mut self, p: &Project) {
        let hosted = crate::starting::hosted(p);
        match &mut self.pending {
            Some(pd) if pd.hosted == hosted => {
                pd.target = p.clone();
                pd.kind = SwapKind::Sync;
            }
            _ => {
                let (warnings, latency) = self
                    .pending
                    .take()
                    .map(|pd| (pd.warnings, pd.latency))
                    .unwrap_or_default();
                self.begin_swap(p.clone(), SwapKind::Sync, warnings);
                if let Some(pd) = &mut self.pending {
                    pd.latency |= latency;
                }
            }
        }
    }

    /// What plays is what the song asks for again: a mix on its way for the song is not
    /// needed. One a new latency waits for is built still, from the song as it is now
    /// (ENG-068); its plugins open again if their values changed meanwhile.
    fn settle_pending(&mut self) {
        let Some(latency) = self.pending.as_ref().map(|pd| pd.latency) else {
            return;
        };
        if !latency {
            self.pending = None;
            self.builder.plugins_mut().drop_ready();
            return;
        }
        let hosted = crate::starting::hosted(&self.heard_now());
        let pd = self.pending.as_mut().expect("checked above");
        pd.kind = SwapKind::Latency;
        if pd.hosted != hosted {
            pd.hosted = hosted;
            pd.asked = false;
            self.builder.plugins_mut().drop_ready();
        }
    }

    fn begin_swap(&mut self, target: Project, kind: SwapKind, warnings: Vec<String>) {
        self.builder.plugins_mut().drop_ready();
        self.pending = Some(PendingSwap {
            hosted: crate::starting::hosted(&target),
            target,
            kind,
            asked: false,
            warnings,
            latency: kind == SwapKind::Latency,
        });
    }

    /// A new mix is waiting for its plugins to open.
    pub fn swap_pending(&self) -> bool {
        self.pending.is_some() || self.material.is_some()
    }

    /// Whether a change of latency builds its mix as [`SessionHost::sync_later`] does, instead
    /// of waiting for the plugins to open in [`SessionHost::poll`].
    pub fn set_nonblocking(&mut self, on: bool) {
        self.nonblocking = on || self.windowed;
    }

    /// A UI-owned host never constructs a plan or reads material. The caller
    /// consumes its requested project through the background preparation path.
    pub fn set_windowed(&mut self) {
        self.windowed = true;
        self.nonblocking = true;
    }

    pub fn preparation_target(&self) -> Option<Project> {
        self.windowed
            .then(|| self.pending.as_ref().map(|p| p.target.clone()))
            .flatten()
    }

    /// After the output has retired on a worker, carry the loaders into the next
    /// preparation. Their owner snapshots preserve patches not reported by an
    /// editor, as a normal graph replacement does.
    pub fn prepare_again(
        self,
        p: &Project,
        engine: EngineConfig,
    ) -> Result<crate::starting::Starting, String> {
        if on_window_thread() {
            return Err("音の準備は裏で行う必要があります".into());
        }
        Ok(crate::starting::Starting::reuse(p, self.builder, engine))
    }

    /// Does a little more of the new mix [`SessionHost::sync_later`] began: the first call asks
    /// every plugin that runs in a host process to open, each later one opens one plugin in this
    /// process (a VST3 plugin is made on the thread that uses it). Swaps the mix in once
    /// everything is open and gives what that did.
    pub fn step_swap(&mut self) -> Option<SyncReport> {
        if self.windowed {
            return self.step_material();
        }
        self.builder.plugins_mut().poll_preloads();
        let setup = setup(self.builder.rate(), RenderMode::Realtime);
        let pd = self.pending.as_mut()?;
        let loader = self.builder.plugins_mut();
        if !pd.asked {
            pd.asked = true;
            for (owner, x) in &pd.hosted {
                if loader.runs_apart(x) {
                    loader.preload(owner, x, &setup);
                }
            }
        } else if let Some((owner, x)) = pd.hosted.iter().find(|(_, x)| !loader.preloaded(x.id)) {
            loader.preload(owner, x, &setup);
        }
        if pd
            .hosted
            .iter()
            .any(|(_, x)| !loader.preloaded(x.id) || loader.preparing(x.id))
        {
            return None;
        }
        let pd = self.pending.take().expect("checked above");
        let mut report = SyncReport {
            warnings: pd.warnings,
            ..SyncReport::default()
        };
        match pd.kind {
            SwapKind::Sync => {
                let p = pd.target;
                self.swap_all(&p, &mut report);
                self.sync_thru(&p, &mut report);
                self.sync_midi_out(&mut report);
                self.send_transport(&p);
                self.follow_song(&p);
                self.sweep_processed();
            }
            SwapKind::Latency => {
                let song = self.song.clone();
                let heard = self.heard_now();
                self.swap_all(&heard, &mut report);
                if report.swapped {
                    self.params.clear();
                    self.structure =
                        Structure::of(&song, &self.graph, &Scope::new(&song, Select::All));
                    self.tracks = TimelineState::all(&song);
                    self.follow();
                }
                self.sync_thru(&song, &mut report);
            }
        }
        Some(report)
    }

    /// Queues what differs between the strips the engine has and `mix`. A strip's snapshot
    /// changes only for the values that were queued.
    fn send_mix(&mut self, mix: &Mix, report: &mut SyncReport) {
        // A branch is matched by its node id: a list that does not line up belongs to a plan
        // that is being replaced and gets its values when it is built.
        for (k, new) in mix.branches.iter().enumerate() {
            match self.mix.branches.get(k) {
                Some(old) if old.node == new.node && old != new => {}
                _ => continue,
            }
            if self.send(Command::BranchGain {
                node: new.node,
                gain: new.gain,
                pan: new.pan,
                mute: new.mute,
            }) {
                self.mix.branches[k] = *new;
                report.commands += 1;
            } else {
                report.deferred += 1;
            }
        }
        for i in 0..mix.tracks.len() {
            let (new, old) = (&mix.tracks[i], self.mix.tracks[i].clone());
            let mut cur = old.clone();
            self.diff_strip(
                new,
                &mut cur,
                report,
                |gain| Command::TrackGain { track: i, gain },
                |pan| Command::TrackPan { track: i, pan },
                |mute| Command::TrackMute { track: i, mute },
                |send, level| Command::SendLevel {
                    track: i,
                    send,
                    level,
                },
            );
            self.mix.tracks[i] = cur;
        }
        for i in 0..mix.buses.len() {
            let (new, old) = (&mix.buses[i], self.mix.buses[i].clone());
            let mut cur = old;
            self.diff_strip(
                new,
                &mut cur,
                report,
                |gain| Command::BusGain { bus: i, gain },
                |pan| Command::BusPan { bus: i, pan },
                |mute| Command::BusMute { bus: i, mute },
                |send, level| Command::BusSendLevel {
                    bus: i,
                    send,
                    level,
                },
            );
            self.mix.buses[i] = cur;
        }
        if mix.master_gain != self.mix.master_gain {
            if self.send(Command::MasterGain(mix.master_gain)) {
                self.mix.master_gain = mix.master_gain;
                report.commands += 1;
            } else {
                report.deferred += 1;
            }
        }
        let mut cur = std::mem::take(&mut self.mix.master_keys);
        self.diff_keys(&mix.master_keys, &mut cur, report);
        self.mix.master_keys = cur;
    }

    #[allow(clippy::too_many_arguments)]
    fn diff_strip(
        &mut self,
        new: &StripMix,
        cur: &mut StripMix,
        report: &mut SyncReport,
        gain: impl Fn(f32) -> Command,
        pan: impl Fn(f32) -> Command,
        mute: impl Fn(bool) -> Command,
        send: impl Fn(usize, f32) -> Command,
    ) {
        if new.gain != cur.gain {
            let ok = self.send(gain(new.gain));
            if tally(report, ok) {
                cur.gain = new.gain;
            }
        }
        if new.pan != cur.pan {
            let ok = self.send(pan(new.pan));
            if tally(report, ok) {
                cur.pan = new.pan;
            }
        }
        if new.mute != cur.mute {
            let ok = self.send(mute(new.mute));
            if tally(report, ok) {
                cur.mute = new.mute;
            }
        }
        for (k, level) in new.sends.iter().enumerate() {
            if cur.sends.get(k) != Some(level) {
                let ok = self.send(send(k, *level));
                if tally(report, ok) {
                    cur.sends[k] = *level;
                }
            }
        }
        self.diff_keys(&new.keys, &mut cur.keys, report);
    }

    /// The keyed inserts' on flags and source levels (ENG-078). Entries that
    /// appear or disappear are structure (a new plan), so only values that
    /// stay are diffed; a snapshot's values change only when queued.
    fn diff_keys(
        &mut self,
        new: &[crate::graph::KeyMix],
        cur: &mut [crate::graph::KeyMix],
        report: &mut SyncReport,
    ) {
        for k in new {
            let Some(pos) = cur.iter().position(|o| o.plugin == k.plugin) else {
                continue;
            };
            let old = cur[pos].clone();
            let mut merged = k.clone();
            if old.on != k.on {
                let ok = self.send(Command::KeyOn {
                    plugin: k.plugin.0,
                    on: k.on,
                });
                if !tally(report, ok) {
                    merged.on = old.on;
                }
            }
            for (j, &(id, level)) in k.sends.iter().enumerate() {
                let was = old.sends.iter().find(|(s, _)| *s == id).map(|(_, l)| *l);
                if Some(level) != was {
                    let ok = self.send(Command::KeyLevel { send: id.0, level });
                    if !tally(report, ok)
                        && let Some(l) = was
                    {
                        merged.sends[j].1 = l;
                    }
                }
            }
            cur[pos] = merged;
        }
    }
}

/// A sidechain's live values: `on` and the send levels (ENG-078). What
/// remains — the sources and their modes — is structure.
fn strip_sidechain(x: &mut Plugin) {
    let Some(sc) = &mut x.sidechain else {
        return;
    };
    sc.on = false;
    for s in &mut sc.sources {
        if let xirius_core::model::KeyMode::Send {
            level_db,
            muted_level,
            ..
        } = &mut s.mode
        {
            *level_db = 0.0;
            *muted_level = None;
        }
    }
}

/// The structure without what a running plugin in the host process or a built-in takes in
/// place.
fn without_plugin_values(s: &Structure) -> Structure {
    let mut s = s.clone();
    let strip = |row: &mut Vec<Plugin>| {
        for x in row.iter_mut() {
            if is_hosted(x) || x.builtin().is_some() {
                x.params.clear();
                x.state = None;
            }
            strip_sidechain(x);
        }
    };
    for n in s.tracks.iter_mut().chain(s.buses.iter_mut()) {
        strip(&mut n.inserts);
    }
    strip(&mut s.master_inserts);
    s
}

/// Plugins equal in what a running plugin takes in place: the sidechain's on
/// flag and levels are the engine's, sent by `send_mix` (ENG-078).
fn same_values(a: &Plugin, b: &Plugin) -> bool {
    if a == b {
        return true;
    }
    let (mut a, mut b) = (a.clone(), b.clone());
    strip_sidechain(&mut a);
    strip_sidechain(&mut b);
    a == b
}

/// Takes what the user changed in the plugins' editors into the song, as one step that one
/// undo takes back: each plugin's state, and the song's values for the parameters it names.
/// The parameter moves come back for automation. A plugin whose latency changed gets a new
/// mix afterwards, so delay compensation follows it (ENG-011).
pub fn collect_plugin_states(
    session: &mut Session,
    host: &mut SessionHost,
    opts: &RunOptions,
) -> CmdResult<Collected> {
    let edits = host.builder.plugins_mut().take_edits();
    collect_edits(session, host, opts, edits)
}

pub fn collect_pulled_states(
    session: &mut Session,
    host: &mut SessionHost,
    pull: &crate::plugins::StatePull,
    opts: &RunOptions,
) -> CmdResult<Collected> {
    let edits = host
        .builder
        .plugins_mut()
        .take_pulled_edits(pull)
        .map_err(xirius_core::CmdError::io)?;
    collect_edits(session, host, opts, edits)
}

fn collect_edits(
    session: &mut Session,
    host: &mut SessionHost,
    opts: &RunOptions,
    edits: Vec<crate::plugins::RemoteEdit>,
) -> CmdResult<Collected> {
    let mut out = Collected::default();
    let mut commands = Vec::new();
    let mut taken = Vec::new();
    for e in edits {
        out.params.extend(e.params.iter().map(|p| ParamEvent {
            plugin: e.plugin,
            param: p.id,
            title: p.title.clone(),
            phase: p.phase,
            value: p.value,
            at: p.at,
            frame: host.frame_at(p.at),
        }));
        let Some(moves) = host.builder.plugins_mut().moves_until(&e) else {
            continue;
        };
        let Some(state) = e.state else {
            continue;
        };
        let Some(x) = find_plugin(session.project(), e.plugin) else {
            continue;
        };
        if x.state.as_deref() == Some(state.as_str()) {
            continue;
        }
        commands.push(json!({
            "command": "plugin.state.set",
            "args": { "id": e.plugin.to_string(), "state": state },
        }));
        // The song's values for the parameters it names win over the state when the plugin
        // is opened again, so they follow the edit too.
        let titles = host.builder.plugins().param_titles(e.plugin);
        let mut params = Vec::new();
        for (key, old) in &x.params {
            let last = moves.iter().find(|p| {
                p.value.is_some()
                    && (key.parse::<u32>().ok() == Some(p.id)
                        || titles
                            .iter()
                            .any(|(id, t)| *id == p.id && t.eq_ignore_ascii_case(key)))
            });
            if let Some(v) = last.and_then(|p| p.value)
                && v != *old
            {
                commands.push(json!({
                    "command": "plugin.param.set",
                    "args": { "id": e.plugin.to_string(), "param": key, "value": v },
                }));
                params.push((key.clone(), v));
            }
        }
        taken.push((e.plugin, state, params));
    }
    out.params.sort_by_key(|p| p.at);
    if !commands.is_empty() {
        for (id, state, params) in &taken {
            host.builder
                .plugins_mut()
                .taken(*id, Some(state.clone()), params);
        }
        let mut args = Map::new();
        args.insert("commands".into(), Value::Array(commands));
        args.insert("title".into(), json!("プラグインの編集を反映"));
        let r = session.run("ai.batch", &args, opts)?;
        host.sync(session.project());
        out.result = Some(r);
    }
    // Every link is asked, so none keeps a change for the next time.
    host.refresh_latency();
    Ok(out)
}

/// How long [`pull_plugin_states`] waits for the plugins in host processes to answer.
pub const PULL_WAIT: Duration = Duration::from_millis(1500);

/// [`collect_plugin_states`] after reading every plugin's state, not only the ones that said
/// they had changed (PLG-057). For the moments the song is written or left, and when a
/// plugin's window closes: a plugin that changes its patch in its own window sends no edit,
/// and the song would keep the state it was opened with. The song changes only for a plugin
/// whose state differs from the song's, as one step that one undo takes back.
pub fn pull_plugin_states(
    session: &mut Session,
    host: &mut SessionHost,
    opts: &RunOptions,
    wait: Duration,
) -> CmdResult<Collected> {
    let pull = host
        .builder
        .plugins_mut()
        .pull_states(wait)
        .map_err(xirius_core::error::CmdError::io)?;
    collect_pulled_states(session, host, &pull, opts)
}

fn find_plugin(p: &Project, id: PluginId) -> Option<&Plugin> {
    p.tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|x| x.id == id)
}

/// Counts a queued command or a deferred one, and passes the outcome on.
fn tally(report: &mut SyncReport, ok: bool) -> bool {
    if ok {
        report.commands += 1;
    } else {
        report.deferred += 1;
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::id::SendId;
    use xirius_core::model::{KeyMode, KeySource, PluginIsolation, PluginKind, Sidechain};

    fn keyed(id: u64, sources: Vec<KeySource>) -> Plugin {
        Plugin {
            id: PluginId(id),
            name: "g".into(),
            kind: PluginKind::Effect,
            format: "builtin".into(),
            uid: "gain".into(),
            bypass: false,
            deactivated: false,
            params: BTreeMap::new(),
            state: None,
            samples: Vec::new(),
            isolation: PluginIsolation::Dedicated,
            sidechain: Some(Sidechain { on: true, sources }),
            preset: None,
        }
    }

    fn source(id: u64, pre: bool) -> KeySource {
        KeySource {
            id: SendId(id),
            track: TrackId(1),
            mode: KeyMode::Send {
                pre,
                level_db: -6.0,
                muted_level: None,
            },
        }
    }

    /// The on flag and the source levels are live values (ENG-078): they move
    /// through `KeyOn`/`KeyLevel` without a new plan, so `same_values` must
    /// not see them.
    #[test]
    fn the_sidechains_live_values_are_not_structure() {
        let a = keyed(7, vec![source(1, true)]);
        let mut b = a.clone();
        let sc = b.sidechain.as_mut().unwrap();
        sc.on = false;
        let KeyMode::Send {
            level_db,
            muted_level,
            ..
        } = &mut sc.sources[0].mode
        else {
            unreachable!()
        };
        *level_db = 3.0;
        *muted_level = Some(-12.0);
        assert!(same_values(&a, &b), "on and levels are live");

        // The wiring — which sources, from where, pre or post — is structure.
        let mut c = a.clone();
        c.sidechain.as_mut().unwrap().sources[0].track = TrackId(9);
        assert!(!same_values(&a, &c), "a different source is structure");
        let mut c = a.clone();
        c.sidechain.as_mut().unwrap().sources[0].id = SendId(99);
        assert!(!same_values(&a, &c), "a different send row is structure");
        let c = keyed(7, vec![source(1, false)]);
        assert!(!same_values(&a, &c), "pre/post is structure");
        let mut c = a.clone();
        c.sidechain.as_mut().unwrap().sources.push(source(2, true));
        assert!(!same_values(&a, &c), "a source more is structure");
    }
}
