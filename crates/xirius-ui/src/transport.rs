//! Playback from the screen (UI-009, UI-010, ARR-006, REC-010, KEY-003).
//!
//! The top bar's buttons and the transport keys drive a session player
//! (`xirius-session`): the song's engine with its sound going to the device,
//! or nowhere when the screen runs for a test. The playhead follows the
//! position the engine reports, and every edit of the song reaches the player
//! through [`Transport::sync`], so what plays is what the screen shows.
//!
//! The loop range is set in the band along the top of the ruler, which is
//! the arrangement's (`crate::arrange`): here it only repeats or not.
//!
//! Recording (REC-001, REC-016) runs on the same player: the record button
//! starts the session's recording into every armed track at once (OPEN-07;
//! instrument tracks from the MIDI input, ENG-028), and stopping puts the
//! takes into the song as one step of the history. The metronome
//! (REC-003) is the engine's click, heard on the device only.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::id::TrackId;
use xirius_core::model::Project;
use xirius_core::time::Tick;
use xirius_engine::click::Click;
use xirius_engine::mixer::MeterReading;
use xirius_engine::record::MAX_INPUT_CHANNELS;
use xirius_engine::{Command, NoteKind};
use xirius_plugins::Catalog;
use xirius_session::host::{HostConfig, SessionHost, Strip};
use xirius_session::output::{
    Drivers, Output, OutputMode, input_unavailable, is_input_unavailable, unavailable,
};
use xirius_session::record::{Input, LiveNote, PreCount, RecordOptions};
use xirius_session::starting::{Progress, Starting};

use crate::input;
use crate::studio::Studio;
use crate::topbar::BarButton;
use crate::tree::RowKind;

/// A running player: the song's engine and where its sound goes.
struct Player {
    host: SessionHost,
    out: Output,
    /// The folder the song's audio paths are relative to when it started.
    dir: Option<PathBuf>,
}

/// Where a player started a little at a time is (ENG-064).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StartStep {
    /// Plugins are still opening.
    Going(Progress),
    /// Everything is open and the player started, or why it did not.
    Done(Result<(), String>),
}

/// What the screen waits to hear from the engine after a command, before it
/// believes the reported position again: until then, reports still describe
/// the state before the command.
#[derive(Clone, Copy, Debug)]
struct Expect {
    playing: bool,
    /// Where a seek went to.
    frame: Option<u64>,
    since: Instant,
}

/// How long a command's answer is waited for.
const EXPECT_WAIT: Duration = Duration::from_millis(300);

/// What the screen says once a sound device that could not be had opened again.
pub const REOPENED: &str = "オーディオデバイスを開き直しました";

/// What the screen says once an input device that could not be had opened again.
pub const INPUT_REOPENED: &str = "オーディオ入力を開き直しました";

/// How often a sound device that cannot be had is tried again.
pub const REOPEN: Duration = Duration::from_secs(2);

/// The sound device would not open (another program holds it), or stopped by itself: the
/// player went with it. It is opened again every [`REOPEN`] until it opens.
#[derive(Clone, Debug)]
pub struct Down {
    /// What the screen says, from [`unavailable`] (or [`input_unavailable`]).
    pub said: String,
    /// When it is tried again.
    due: Instant,
}

/// The recording under way: the tracks it records into, as the session
/// started it, and where it began.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Take {
    /// The audio tracks, top to bottom.
    pub audio: Vec<TrackId>,
    /// The instrument tracks, top to bottom.
    pub midi: Vec<TrackId>,
    /// Where the take begins to count: the start of the punch range, or the playhead the record
    /// button was pressed at (the song plays on from before it during a pre-roll).
    pub from: Tick,
    /// The stretch kept, from and to (`None`: until the stop).
    pub keep: Option<(Tick, Option<Tick>)>,
}

pub struct Transport {
    pub(crate) auditioning_export: bool,
    pub loss: crate::record_loss::Loss,
    /// Where the sound goes. [`OutputMode::None`] refuses to play.
    pub mode: OutputMode,
    player: Option<Player>,
    pub playing: bool,
    /// A recording runs (REC-010); [`Transport::take`] says into what.
    pub recording: bool,
    pub take: Option<Take>,
    /// The count-in before the recording clicks (REC-007): the song waits, nothing is recorded.
    pub pre_count: Option<PreCount>,
    /// The notes played into the MIDI tracks of the take so far, which the take shows as they
    /// come in.
    pub live_notes: Vec<LiveNote>,
    /// The metronome sounds while playing (REC-003).
    pub metronome: bool,
    /// The song's loop range repeats.
    pub looping: bool,
    /// Where play last started: stop goes back here.
    pub from: Tick,
    /// Where the playhead stands while there is no player: a player started for a key or a
    /// note starts here, so it does not take the playhead back to the start (UI-285).
    parked: Tick,
    expect: Option<Expect>,
    /// The loudest level of each strip since the screen last took them.
    levels: BTreeMap<Strip, MeterReading>,
    /// Where the player looks its VST3 plugins up: the list the screen shows.
    catalog: Option<Arc<Catalog>>,
    /// The loudest level of each input channel since the screen last took them (REC-012).
    input: [f32; MAX_INPUT_CHANNELS],
    /// The armed tracks the input was last tried to open for, so a failure is not retried
    /// every frame; arming another track tries again.
    watch_tried: Option<Vec<TrackId>>,
    pub io_prefs: xirius_prefs::Prefs,
    pub midi_record_input: Option<Box<dyn xirius_engine::record::MidiInput>>,
    pub midi_control: Option<xirius_session::midi_settings::Control>,
    /// The knobs and pads assigned to a part or a command (OPS-007): a recording leaves them out.
    pub midimap: xirius_prefs::Midimap,
    output_pair: [usize; 2],
    pub observe_inputs: bool,
    io_levels: Vec<f32>,
    /// How the sound device is opened: the system's, unless a test puts in stand-ins.
    drivers: Drivers,
    /// What is to be said about how the device was opened (the exclusive mode that would not
    /// open), until the screen takes it.
    driver_notice: Option<String>,
    /// The sound device cannot be had now.
    pub down: Option<Down>,
    /// The player being started a little at a time for the song in the folder (ENG-064):
    /// [`Transport::step_start`] opens its plugins one by one, and the next player takes what
    /// is open so far.
    starting: Option<(Starting, Option<PathBuf>)>,
    /// The tracks whose instrument does not sound yet, whatever the player says, for tests
    /// and screenshots.
    pub not_ready: Vec<TrackId>,
    /// The input device cannot be had now (it would not open, or stopped by itself): the
    /// output plays on, and the input is opened again every [`REOPEN`] while a track is armed.
    pub input_down: Option<Down>,
}

impl Transport {
    pub fn new(mode: OutputMode) -> Transport {
        Transport {
            auditioning_export: false,
            loss: Default::default(),
            mode,
            player: None,
            playing: false,
            recording: false,
            take: None,
            pre_count: None,
            live_notes: Vec::new(),
            metronome: false,
            looping: false,
            from: 0,
            parked: 0,
            expect: None,
            levels: BTreeMap::new(),
            catalog: None,
            input: [0.0; MAX_INPUT_CHANNELS],
            watch_tried: None,
            io_prefs: Default::default(),
            midi_record_input: None,
            midi_control: None,
            midimap: xirius_prefs::Midimap::default(),
            output_pair: [0, 1],
            observe_inputs: false,
            io_levels: Vec::new(),
            drivers: Drivers::system(),
            driver_notice: None,
            down: None,
            starting: None,
            not_ready: Vec::new(),
            input_down: None,
        }
    }

    fn host_config(&self, dir: Option<PathBuf>) -> HostConfig {
        HostConfig {
            dir,
            catalog: self.catalog.clone(),
            ..HostConfig::default()
        }
    }

    /// Begins starting the player for `p` a little at a time (ENG-064), when it would play and
    /// none runs. False when nothing began.
    pub(crate) fn begin_start(&mut self, p: &Project, dir: Option<PathBuf>) -> bool {
        if self.mode == OutputMode::None
            || self.auditioning_export
            || self.player.is_some()
            || self.down.is_some()
        {
            return false;
        }
        self.starting = Some((Starting::new(p, self.host_config(dir.clone())), dir));
        true
    }

    /// How far the player being started is.
    pub(crate) fn start_progress(&self) -> Option<Progress> {
        self.starting.as_ref().map(|(s, _)| s.progress())
    }

    /// A player is being started a little at a time; the next one takes what is open of it.
    pub fn preparing(&self) -> bool {
        self.starting.is_some()
    }

    /// Opens a little more of the player being started for `p`, and starts it (and opens the
    /// sound device) once everything is open. `None` when none is being started, or it was for
    /// another folder.
    pub(crate) fn step_start(&mut self, p: &Project, dir: Option<PathBuf>) -> Option<StartStep> {
        let (s, d) = self.starting.as_mut()?;
        if *d != dir {
            self.starting = None;
            return None;
        }
        if !s.step() {
            return Some(StartStep::Going(s.progress()));
        }
        let began = Instant::now() - s.elapsed();
        let units = s.progress().units.len();
        let started = self.ensure(p, dir).map(|_| ());
        xirius_engine::crash::note(&format!(
            "song_prepared ms={:.1} units={units} ok={}",
            began.elapsed().as_secs_f64() * 1000.0,
            started.is_ok()
        ));
        Some(StartStep::Done(started))
    }

    /// Whether a note played now on `track`'s instrument sounds (ENG-064): it may still be
    /// opening in its host, or warming up after it opened again. Starts the player when there
    /// is none, as [`Transport::note`] does.
    pub(crate) fn instrument_ready(
        &mut self,
        p: &Project,
        dir: Option<PathBuf>,
        track: TrackId,
    ) -> Result<bool, String> {
        if self.player.is_none()
            && let Some(d) = &self.down
        {
            return Err(d.said.clone());
        }
        let held = self.not_ready.contains(&track);
        let player = self.ensure(p, dir)?;
        Ok(!held && player.host.instrument_ready(track))
    }

    /// Opens the sound device the way `drivers` say, from the next player on.
    pub fn set_drivers(&mut self, drivers: Drivers) {
        self.drivers = drivers;
    }

    /// The line said about how the sound device was opened, once.
    pub fn take_driver_notice(&mut self) -> Option<String> {
        self.driver_notice.take()
    }

    /// The player looks its VST3 plugins up in `catalog`, from its next mix on.
    pub fn set_catalog(&mut self, catalog: Arc<Catalog>) {
        if let Some(pl) = &mut self.player {
            pl.host.set_catalog(Some(catalog.clone()));
        }
        self.catalog = Some(catalog);
    }

    /// The player's session, started (it plays nothing until asked) when none runs: a
    /// plugin's window shows the instance the player plays.
    pub fn start_host(
        &mut self,
        p: &Project,
        dir: Option<PathBuf>,
    ) -> Result<&mut SessionHost, String> {
        self.ensure(p, dir).map(|pl| &mut pl.host)
    }

    /// A player is running (it reports positions and levels).
    pub fn route_output(&mut self, pair: [usize; 2]) {
        if self.output_pair == pair {
            return;
        }
        if let Some(pl) = &mut self.player
            && pl
                .host
                .controller()
                .send(Command::OutputChannels(pair))
                .is_err()
        {
            return;
        }
        self.output_pair = pair;
    }

    pub fn is_live(&self) -> bool {
        self.player.is_some()
    }
    pub fn configure_io(
        &mut self,
        p: &Project,
        dir: Option<PathBuf>,
        prefs: xirius_prefs::Prefs,
    ) -> Result<(), String> {
        // In shared mode the Windows audio engine sets the buffer: asking for another one
        // changes nothing.
        let shared = prefs.text("audio.driver") == "wasapi-shared";
        let changed = ["audio.driver", "audio.device", "audio.buffer"]
            .iter()
            .filter(|k| !(shared && **k == "audio.buffer"))
            .any(|k| prefs.get(k) != self.io_prefs.get(k));
        if changed && self.recording {
            return Err("録音中はオーディオデバイスを変更できません".into());
        }
        self.io_prefs = prefs;
        if changed {
            self.watch_tried = None;
            self.io_levels.clear();
            for d in [&mut self.down, &mut self.input_down].into_iter().flatten() {
                d.due = Instant::now();
            }
        }
        if !changed || self.mode != OutputMode::Device || self.player.is_none() {
            return Ok(());
        }
        let at = self.tick_now().unwrap_or(self.from);
        let playing = self.playing;
        self.player = None;
        self.playing = false;
        self.watch_tried = None;
        self.io_levels.clear();
        self.ensure(p, dir.clone())?;
        if playing {
            self.play(p, dir, at)?;
        }
        Ok(())
    }
    pub fn io_observation(&mut self) -> (Vec<f32>, Option<xirius_engine::devices::Latency>) {
        let latency = self.player.as_mut().and_then(|p| {
            let input = p.host.input_latency_ns()?;
            let output = p.host.controller().stats().snapshot().output_latency;
            if output.is_zero() {
                return None;
            }
            let rate = p.host.rate();
            Some(xirius_engine::devices::Latency {
                rate,
                input: (input as f64 * rate as f64 / 1e9).round() as u32,
                output: (output.as_secs_f64() * rate as f64).round() as u32,
                conversion: 0, // These streams run at the engine rate without a resampler.
            })
        });
        (self.io_levels.clone(), latency)
    }

    /// Starts the player for `p` if there is none (or the song moved).
    fn ensure(&mut self, p: &Project, dir: Option<PathBuf>) -> Result<&mut Player, String> {
        if self.auditioning_export {
            return Err("書き出した音を試聴中です".into());
        }
        if self.player.as_ref().is_some_and(|pl| pl.dir != dir) {
            self.player = None;
        }
        if self.player.is_none() {
            let at = self.parked;
            if self.mode == OutputMode::None {
                return Err("オーディオ出力がありません".into());
            }
            // What was opened a little at a time is taken, and the rest opened now; what runs
            // in a host process is not waited for: it plays dry until it is in (ENG-065).
            let started = match self.starting.take() {
                Some((s, d)) if d == dir => s.finish_late(p),
                _ => SessionHost::start_late(p, self.host_config(dir.clone())),
            };
            let (mut host, audio, _warnings) =
                started.map_err(|e| format!("再生を開始できませんでした: {e}"))?;
            host.set_nonblocking(true);
            host.drivers = self.drivers.clone();
            host.set_looping(p, self.looping);
            host.controller()
                .send(Command::OutputChannels(self.output_pair))
                .map_err(|_| "出力を設定できません")?;
            let mut settings = xirius_prefs::Settings::memory();
            settings.prefs = self.io_prefs.clone();
            host.io_settings = Some(settings.shared());
            if self.mode == OutputMode::Device {
                host.set_midi_out(Some(xirius_session::midi_out::system_maker()));
            }
            let opened = match Output::open(self.mode, audio, &self.io_prefs, &self.drivers) {
                Ok(o) => o,
                Err(e) => {
                    let said = unavailable(&e);
                    self.went_down(said.clone());
                    return Err(said);
                }
            };
            self.down = None;
            if opened.notice.is_some() {
                self.driver_notice = opened.notice;
            }
            let out = opened.output.ok_or("オーディオ出力がありません")?;
            if self.metronome {
                send_click(&mut host, p, true);
            }
            let frame = host.tempo_map().frame_at(at, f64::from(host.rate()));
            if host.seek_frame(frame) {
                self.expect(false, Some(frame));
            }
            self.player = Some(Player { host, out, dir });
        }
        Ok(self.player.as_mut().expect("started above"))
    }

    /// The sound device cannot be had: it is tried again after [`REOPEN`].
    fn went_down(&mut self, said: String) {
        self.down = Some(Down {
            said,
            due: Instant::now() + REOPEN,
        });
    }

    /// Whether the player's device stopped by itself; then the player goes (the position
    /// stays where it was) and what the screen says is given. A recording keeps its player
    /// until its takes are in: the caller finishes it first.
    pub fn take_lost(&mut self) -> Option<String> {
        if self.take.is_some() {
            return None;
        }
        let why = self.player.as_ref()?.out.lost()?;
        let said = unavailable(&why);
        self.player = None;
        self.playing = false;
        self.recording = false;
        self.expect = None;
        self.watch_tried = None;
        self.io_levels.clear();
        self.went_down(said.clone());
        Some(said)
    }

    /// Why the input device the player holds (watched, or recorded from) stopped by itself,
    /// once it has.
    pub fn input_lost(&self) -> Option<String> {
        self.player.as_ref()?.host.input_lost()
    }

    /// The input device cannot be had (`said`, from [`input_unavailable`]): it is let go, and
    /// tried again after [`REOPEN`]. A recording is finished before.
    pub fn input_went_down(&mut self, said: String) {
        if let Some(pl) = &mut self.player {
            pl.host.watch_stop();
        }
        self.input = [0.0; MAX_INPUT_CHANNELS];
        self.watch_tried = None;
        self.input_down = Some(Down {
            said,
            due: Instant::now() + REOPEN,
        });
    }

    /// While the input device cannot be had, opens it again for the `armed` tracks once
    /// [`REOPEN`] has passed. `None` when it was not time (or there is no player, or a
    /// recording runs); else whether it opened, or what the screen says. With nothing armed the
    /// input is not needed, and is not tried.
    pub fn reopen_input(
        &mut self,
        p: &Project,
        dir: Option<PathBuf>,
        armed: &[TrackId],
    ) -> Option<Result<(), String>> {
        let due = self.input_down.as_ref()?.due;
        if Instant::now() < due || self.player.is_none() || self.recording || self.take.is_some() {
            return None;
        }
        if armed.is_empty() && !self.observe_inputs {
            self.input_down = None;
            return None;
        }
        self.input_down = None;
        self.watch_tried = None;
        match self.watch(p, dir, armed) {
            Some(said) => Some(Err(said)),
            None if self.input_down.is_none()
                && self.player.as_ref().is_some_and(|pl| pl.host.is_watching()) =>
            {
                Some(Ok(()))
            }
            None => None,
        }
    }

    /// Whether the player's device stopped by itself, without letting it go.
    pub fn is_lost(&self) -> bool {
        self.player
            .as_ref()
            .is_some_and(|pl| pl.out.lost().is_some())
    }

    /// While the device cannot be had, opens it again once [`REOPEN`] has passed. `None`
    /// when it was not time; else whether it opened, or what the screen says.
    pub fn reopen(&mut self, p: &Project, dir: Option<PathBuf>) -> Option<Result<(), String>> {
        let due = self.down.as_ref()?.due;
        if Instant::now() < due || self.mode != OutputMode::Device || self.auditioning_export {
            return None;
        }
        let opened = self.ensure(p, dir).map(|_| ());
        if opened.is_ok() {
            self.watch_tried = None;
        }
        Some(opened)
    }

    fn expect(&mut self, playing: bool, frame: Option<u64>) {
        self.expect = Some(Expect {
            playing,
            frame,
            since: Instant::now(),
        });
    }

    /// Plays from `at`.
    pub fn play(&mut self, p: &Project, dir: Option<PathBuf>, at: Tick) -> Result<(), String> {
        let player = self.ensure(p, dir)?;
        let host = &mut player.host;
        let frame = host.tempo_map().frame_at(at, f64::from(host.rate()));
        if !(host.seek_frame(frame) && host.play()) {
            return Err("再生の指示を送れませんでした".into());
        }
        self.playing = true;
        self.from = at;
        self.expect(true, Some(frame));
        Ok(())
    }

    /// Plays a note by hand on the instrument of `track`, playing or stopped
    /// (UI-074). Starts the player when there is none.
    pub(crate) fn note(
        &mut self,
        p: &Project,
        dir: Option<PathBuf>,
        track: TrackId,
        kind: NoteKind,
    ) -> Result<(), String> {
        // While the device cannot be had, a note does not try to open it (each key and each
        // pitch of a drag would wait for it); it opens again on its own (UI-285).
        if self.player.is_none()
            && let Some(d) = &self.down
        {
            return Err(d.said.clone());
        }
        let player = self.ensure(p, dir)?;
        if player.host.note(track, kind) {
            Ok(())
        } else {
            Err("インストゥルメントにノートを送れませんでした".into())
        }
    }

    pub(crate) fn suspend_for_export(&mut self) {
        self.pause();
        self.player = None;
        self.watch_tried = None;
        self.auditioning_export = true;
    }

    /// Stops where it is.
    pub fn pause(&mut self) {
        self.playing = false;
        self.recording = false;
        self.pre_count = None;
        self.live_notes.clear();
        if let Some(pl) = &mut self.player {
            pl.host.stop();
            self.expect(false, None);
        }
    }

    /// Where a player that starts starts: the playhead, said once a frame and at each seek.
    pub(crate) fn park(&mut self, at: Tick) {
        self.parked = at;
    }

    /// Moves the position, playing or not.
    pub fn seek(&mut self, at: Tick) {
        self.parked = at;
        let Some(pl) = &mut self.player else { return };
        let host = &mut pl.host;
        let frame = host.tempo_map().frame_at(at, f64::from(host.rate()));
        host.seek_frame(frame);
        let playing = self.playing;
        self.expect(playing, Some(frame));
    }

    /// Turns the metronome on and off; a running player hears it at once.
    pub fn set_metronome(&mut self, p: &Project, on: bool) {
        self.metronome = on;
        if let Some(pl) = &mut self.player {
            send_click(&mut pl.host, p, on);
        }
    }

    /// Starts recording into the armed tracks as the record panel plans it: from `plan.at` when
    /// stopped (`playhead` when it names none) or from where it plays. With `midi` (an instrument track is armed) the
    /// MIDI input is opened too. A screen without a device records a test
    /// tone and test notes, so a test never opens a microphone or a MIDI
    /// device. Returns what the session says it leaves out.
    pub fn record(
        &mut self,
        p: &Project,
        dir: Option<PathBuf>,
        plan: xirius_session::punch::Plan,
        playhead: Tick,
        midi: bool,
        count_in_bars: u32,
    ) -> Result<Vec<String>, String> {
        let mode = self.mode;
        let prefs = self.io_prefs.clone();
        let midimap = self.midimap.clone();
        let midi_input = self.midi_record_input.take();
        let playing = self.playing;
        let pl = self.ensure(p, dir.clone())?;
        let host = &mut pl.host;
        let rate = host.rate();
        let mut opts = RecordOptions {
            at: (!playing).then(|| plan.at.unwrap_or(playhead)),
            count_in_bars,
            keep: plan.keep,
            stop_at: plan.stop_at,
            quantize: plan.quantize,
            offsets_ms: [
                prefs.number("audio.record-offset"),
                prefs.number("midi.input-offset"),
            ],
            ..RecordOptions::default()
        };
        let device = mode == OutputMode::Device;
        if device {
            opts.input = Input::Device;
        } else {
            opts.input = Input::Given(input::test_input(rate));
            opts.latency = Some(0);
        }
        let mut warnings = Vec::new();
        let mut midi_control = None;
        if midi {
            match midi_input.map_or_else(|| input::midi_input(device, rate), Ok) {
                Ok(m) => {
                    let input = xirius_session::midi_settings::EnabledInput::new(m, &prefs)
                        .with_assignments(&midimap);
                    midi_control = Some(input.control());
                    opts.midi = Some(Box::new(input));
                }
                // The session says which tracks that leaves out.
                Err(e) => warnings.push(e),
            }
        }
        let started = match host.record_start(p, dir.as_deref(), opts) {
            Ok(s) => s,
            Err(e) => {
                if is_input_unavailable(&e.message) {
                    self.input_went_down(e.message.clone());
                }
                return Err(e.message);
            }
        };
        // The input opened for the recording: it can be had again.
        let input_back = device && !started.audio_tracks.is_empty();
        warnings.extend(started.warnings);
        let from = if playing {
            host.tick_now().unwrap_or(playhead)
        } else {
            plan.at.unwrap_or(playhead)
        };
        let frame = host.tempo_map().frame_at(from, f64::from(rate));
        let pre_count = host.pre_count();
        self.loss = crate::record_loss::Loss {
            tracks: started.audio_tracks.clone(),
            ..Default::default()
        };
        self.pre_count = pre_count;
        self.playing = true;
        self.recording = true;
        self.midi_control = midi_control;
        self.take = Some(Take {
            audio: started.audio_tracks,
            midi: started.midi_tracks,
            from: plan.keep.map_or(from, |k| k.0),
            keep: plan.keep,
        });
        if !playing {
            self.from = from;
            self.expect(true, Some(frame));
        }
        if input_back {
            self.input_down = None;
        }
        Ok(warnings)
    }

    /// The player's session, while one runs.
    pub fn host(&self) -> Option<&SessionHost> {
        self.player.as_ref().map(|pl| &pl.host)
    }

    /// The player's session, while one runs (a recording finishes on it).
    pub fn host_mut(&mut self) -> Option<&mut SessionHost> {
        self.player.as_mut().map(|pl| &mut pl.host)
    }

    /// Plays the test tone once, the left and then the right (UI-260), starting the output if
    /// it is not running. Gives how long it sounds, in seconds.
    pub fn test_tone(&mut self, p: &Project, dir: Option<PathBuf>) -> Result<f64, String> {
        let host = &mut self.ensure(p, dir)?.host;
        let rate = f64::from(host.rate());
        let tone = xirius_engine::tone::Tone::new(rate);
        let seconds = f64::from(tone.frames()) / rate;
        host.controller()
            .send(Command::SetTone(Some(Box::new(tone))))
            .map_err(|_| "テスト音を再生できません".to_string())?;
        Ok(seconds)
    }

    /// Does the test tone sound now?
    pub fn tone_playing(&mut self) -> bool {
        self.host_mut()
            .is_some_and(|h| h.controller().stats().tone_left() > 0)
    }

    /// The loudest the output sent to the left and the right since the last call.
    pub fn take_output_peak(&mut self) -> [f32; 2] {
        self.host_mut()
            .map_or([0.0; 2], |h| h.controller().stats().take_output_peak())
    }

    /// The recording is over: its takes went into the song (or there were
    /// none). The session stopped the transport.
    pub fn end_take(&mut self) {
        self.recording = false;
        self.take = None;
        self.pre_count = None;
        self.live_notes.clear();
        self.playing = false;
        if self.player.is_some() {
            self.expect(false, None);
        }
    }

    pub fn set_looping(&mut self, p: &Project, on: bool) {
        self.looping = on;
        if let Some(pl) = &mut self.player {
            pl.host.set_looping(p, on);
        }
    }

    /// The song changed: the player follows (small commands for a fader,
    /// a new mix when the routing changed). A song from another folder
    /// stops the player; the next play starts a new one.
    pub fn sync(&mut self, p: &Project, dir: Option<PathBuf>) {
        if self.player.as_ref().is_some_and(|pl| pl.dir != dir) {
            self.reset();
            return;
        }
        if let Some(pl) = &mut self.player {
            // The new mix is built a little at a time (see `Transport::step_swap`): the one
            // playing goes on meanwhile.
            pl.host.sync_later(p);
            pl.host.watch_sync(p);
            // The tempo or the metre may have changed.
            if self.metronome {
                send_click(&mut pl.host, p, true);
            }
        }
    }

    /// Does a little more of the mix a change of the song began (ENG-065), and gives what the
    /// host processes said about plugins the player took before they answered. True while there
    /// is more to do, so the frames keep coming.
    pub fn step_swap(&mut self) -> (bool, Vec<String>) {
        let Some(pl) = &mut self.player else {
            return (false, Vec::new());
        };
        let said = pl.host.take_late_warnings();
        // The report's own warnings (the song's files that are missing, and so on) are not said
        // again: a change of the song has never said them, and the open of the song did.
        let _ = pl.host.step_swap();
        let busy = pl.host.swap_pending() || pl.host.plugins().late_pending();
        (busy, said)
    }

    /// Save As keeps the song and its running plugins; only their file base changes.
    pub(crate) fn relocate(&mut self, dir: Option<PathBuf>) {
        if let Some(pl) = &mut self.player {
            pl.host.relocate(dir.clone());
            pl.dir = dir;
        }
    }

    /// The position now, between polls too (AUT-002 writes at it).
    pub fn tick_now(&self) -> Option<Tick> {
        self.player.as_ref()?.host.tick_now()
    }

    /// Lets the player go (another song was opened).
    pub fn reset(&mut self) {
        self.auditioning_export = false;
        self.player = None;
        self.starting = None;
        self.loss = Default::default();
        self.playing = false;
        self.recording = false;
        self.take = None;
        self.pre_count = None;
        self.live_notes.clear();
        self.expect = None;
        self.levels.clear();
        self.input = [0.0; MAX_INPUT_CHANNELS];
        self.watch_tried = None;
    }

    /// Watches the input of the armed audio tracks while nothing records, so their level moves
    /// before the record button is pressed (REC-002, REC-012); lets it go when none is armed.
    /// A screen without a device watches a test tone. Gives what stopped it opening.
    pub fn watch(
        &mut self,
        p: &Project,
        dir: Option<PathBuf>,
        armed: &[TrackId],
    ) -> Option<String> {
        if self.recording || self.take.is_some() {
            return None;
        }
        if (armed.is_empty() && !self.observe_inputs) || self.mode == OutputMode::None {
            self.watch_tried = None;
            self.input_down = None;
            if let Some(pl) = &mut self.player {
                pl.host.watch_stop();
            }
            return None;
        }
        // An input that cannot be had is opened again by [`Transport::reopen_input`] only.
        if self.player.as_ref().is_some_and(|pl| pl.host.is_watching())
            || self.watch_tried.as_deref() == Some(armed)
            || self.input_down.is_some()
        {
            return None;
        }
        let device = self.mode == OutputMode::Device;
        let started = self.ensure(p, dir).and_then(|pl| {
            let rate = pl.host.rate();
            let input = if device {
                Input::Device
            } else {
                Input::Given(input::test_input(rate))
            };
            pl.host.watch_start(input).map_err(|e| e.message)?;
            pl.host.watch_sync(p);
            Ok(pl.host.take_driver_notice())
        });
        if let Ok(Some(n)) = &started {
            self.driver_notice = Some(n.clone());
        }
        // An input device that would not open is tried again after a while; another failure
        // not until the armed tracks change.
        let failed = started.err();
        match &failed {
            Some(e) if is_input_unavailable(e) => self.input_went_down(e.clone()),
            Some(_) => self.watch_tried = Some(armed.to_vec()),
            None => {}
        }
        failed
    }

    /// How many channels the input being watched has (a stereo pair when none is).
    pub fn input_channels(&self) -> usize {
        self.player
            .as_ref()
            .map(|pl| pl.host.watch_channels())
            .filter(|n| *n > 0)
            .unwrap_or(2)
    }

    /// The loudest the input monitor put on the output, left and right, since the last call.
    pub fn take_monitor_peak(&mut self) -> [f32; 2] {
        self.player
            .as_mut()
            .map_or([0.0; 2], |pl| pl.host.input_state().take_monitor_peak())
    }

    /// The loudest level of each input channel since the last call.
    pub fn take_input_levels(&mut self) -> [f32; MAX_INPUT_CHANNELS] {
        std::mem::replace(&mut self.input, [0.0; MAX_INPUT_CHANNELS])
    }

    /// Takes in what the engine reported. Returns the position to show, or
    /// `None` while the engine has not answered the last command yet.
    pub fn poll(&mut self) -> Option<Tick> {
        let pl = self.player.as_mut()?;
        let st = pl.host.poll();
        for (strip, r) in pl.host.take_levels() {
            let held = self.levels.entry(strip).or_default();
            for ch in 0..2 {
                held.peak[ch] = held.peak[ch].max(r.peak[ch]);
                held.rms[ch] = held.rms[ch].max(r.rms[ch]);
            }
        }
        let readings = pl.host.input_levels();
        self.io_levels = readings.to_vec();
        for (held, now) in self.input.iter_mut().zip(readings) {
            *held = held.max(now);
        }
        let rate = pl.host.rate();
        if let Some(e) = self.expect {
            let answered = st.playing == e.playing
                && e.frame
                    .is_none_or(|f| st.frame >= f && st.frame <= f + u64::from(rate) / 2);
            if !answered && e.since.elapsed() < EXPECT_WAIT {
                return None;
            }
            self.expect = None;
        }
        if self.take.is_some() {
            // A recording plays on past the end of the song until stopped.
            self.playing = true;
            self.pre_count = pl.host.pre_count();
            self.live_notes = pl.host.live_notes();
        } else {
            if self.playing && !st.playing {
                // The song ended.
                self.recording = false;
            }
            self.playing = st.playing;
        }
        Some(pl.host.tempo_map().tick_of_frame(st.frame, f64::from(rate)))
    }

    /// The loudest level of each strip since the last call.
    pub fn take_levels(&mut self) -> BTreeMap<Strip, MeterReading> {
        std::mem::take(&mut self.levels)
    }
}

/// Gives the song's metronome to the engine (`on: false` takes it away).
fn send_click(host: &mut SessionHost, p: &Project, on: bool) {
    let click = on.then(|| Box::new(click_of(host, p)));
    let _ = host.controller().send(Command::SetClick(click));
}

/// A click on every beat from the start to a few bars past the end of the
/// song, then on at the last tempo.
fn click_of(host: &SessionHost, p: &Project) -> Click {
    let tm = host.tempo_map();
    let rate = f64::from(host.rate());
    crate::metronome_map::click(p, tm, rate, host.end_frame())
}

/// The time of a tick as the top bar shows it: `m:ss.sss`.
pub fn clock(seconds: f64) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

impl Studio {
    // ---------------------------------------------------------- the buttons

    /// A press on a transport button of the top bar.
    pub fn transport_press(&mut self, b: BarButton) -> bool {
        match b {
            BarButton::ToStart => self.to_start(),
            BarButton::Play => self.toggle_play(),
            BarButton::Stop => self.stop_transport(),
            BarButton::Record => self.toggle_record(),
            BarButton::Loop => self.toggle_loop(),
            BarButton::Metronome => self.toggle_metronome(),
            BarButton::PreRoll => {
                self.rec_set("record.preroll", serde_json::json!({}));
                true
            }
            _ => false,
        }
    }

    /// The transport keys: Space plays and pauses, Enter stops, Home goes to
    /// the start, R records and C turns the loop on and off.
    pub fn transport_key(&mut self, key: &str) -> bool {
        match key {
            "space" => self.toggle_play(),
            "enter" => self.stop_transport(),
            "home" => self.to_start(),
            "r" => self.toggle_record(),
            "/" => self.toggle_loop(),
            _ => false,
        }
    }

    pub fn toggle_play(&mut self) -> bool {
        self.stop_export_preview();
        if self.transport.take.is_some() {
            self.finish_take();
            return true;
        }
        if self.transport.playing {
            self.transport.pause();
            return true;
        }
        let p = self.project();
        let at = self.playhead;
        if let Err(e) = self.transport.play(&p, self.song.dir(), at) {
            self.error = Some(e);
        }
        true
    }

    /// Playing: stops and goes back to where play started. Stopped: goes to
    /// the start of the song.
    pub fn stop_transport(&mut self) -> bool {
        if self.transport.take.is_some() {
            self.finish_take();
            let from = self.transport.from;
            self.seek(from);
            return true;
        }
        let to = if self.transport.playing {
            self.transport.pause();
            self.transport.from
        } else {
            self.transport.recording = false;
            0
        };
        self.seek(to);
        true
    }

    pub fn to_start(&mut self) -> bool {
        self.seek(0);
        true
    }

    /// Moves the playhead, and the player's position with it.
    pub fn seek(&mut self, at: Tick) {
        self.auto_seek(at.max(0));
        self.playhead = at.max(0);
        self.transport.seek(self.playhead);
        if self.transport.playing {
            self.transport.from = self.playhead;
        }
    }

    /// Records into every armed track, from the playhead or where it plays.
    /// Pressed again, the takes go into the song as one step of the history.
    /// A track the session leaves out (an input the device does not have, no
    /// MIDI input) is said in the message line; the others record.
    pub fn toggle_record(&mut self) -> bool {
        self.stop_export_preview();
        if self.transport.take.is_some() {
            self.finish_take();
            return true;
        }
        let p = self.project();
        let (audio, midi) = input::armed(&p);
        if audio.is_empty() && midi.is_empty() {
            // No words: the button shakes and the arm button lights up.
            self.record_shake = Some(Instant::now());
            self.arm_cue = self.arm_candidate();
            return true;
        }
        let dir = self.song.dir();
        if !audio.is_empty() && dir.is_none() {
            self.error = Some("オーディオを録音するには曲の保存が必要です".into());
            return true;
        }
        let at = self.playhead;
        let midi = !midi.is_empty();
        let plan = self.record_plan(&p, at);
        if midi {
            self.release_midi();
            self.transport.midi_record_input = self.live_midi.input.take();
            self.live_midi.resume |= self.transport.midi_record_input.is_some();
        }
        self.poll_io();
        let count_in = self
            .song
            .settings()
            .lock()
            .map_or(0, |s| s.prefs.number("audio.count-in").max(0.0) as u32);
        let from = (!self.transport.playing).then(|| plan.at.unwrap_or(at));
        match self.transport.record(&p, dir, plan, at, midi, count_in) {
            Ok(warnings) => {
                // The playhead goes to where the song plays from (the pre-roll or the range).
                if let Some(from) = from {
                    self.playhead = from;
                }
                self.error = (!warnings.is_empty()).then(|| warnings.join("。"));
            }
            Err(e) => self.error = Some(e),
        }
        true
    }

    /// The song plays up to where the punch range begins: the record button blinks (REC-007).
    pub fn pre_rolling(&self) -> bool {
        self.transport.recording
            && self.transport.pre_count.is_none()
            && self
                .transport
                .take
                .as_ref()
                .is_some_and(|t| t.keep.is_some_and(|(from, _)| self.playhead < from))
    }

    /// What the record panel makes of a recording pressed at `at`: where to play from, which
    /// stretch to keep, where to stop and how to snap the notes (REC-007, REC-009).
    fn record_plan(&self, p: &Project, at: Tick) -> xirius_session::punch::Plan {
        let panel = self.song.settings().lock().map_or(None, |s| {
            Some(xirius_session::punch::Panel::from_prefs(&s.prefs, p.ppq))
        });
        match panel {
            Some(panel) => xirius_session::punch::plan(
                p,
                &panel,
                at,
                self.transport.playing,
                self.transport.looping,
            ),
            None => xirius_session::punch::Plan {
                at: (!self.transport.playing).then_some(at),
                ..Default::default()
            },
        }
    }

    /// The track whose arm button is next to press: the selected track when it
    /// records, else the first one that does (REC-010).
    fn arm_candidate(&self) -> Option<TrackId> {
        use xirius_core::model::TrackKind;
        let p = self.project();
        let records = |t: &TrackId| {
            p.tracks
                .get(t)
                .is_some_and(|t| matches!(t.kind, TrackKind::Audio | TrackKind::Instrument))
        };
        if let Some(RowKind::Track(t)) = self.selected_row
            && records(&t)
        {
            return Some(t);
        }
        self.tree()
            .rows
            .iter()
            .filter_map(|r| match r.kind {
                RowKind::Track(t) => Some(t),
                _ => None,
            })
            .find(records)
    }

    /// Stops the recording and puts its takes into the song (one undo takes
    /// them out again).
    pub(crate) fn finish_take(&mut self) {
        let Some(host) = self.transport.host_mut() else {
            self.transport.end_take();
            return;
        };
        let rate = host.rate();
        let done = self.song.finish_recording(host);
        if let Ok(data) = &done {
            self.transport.loss.receive(data, &self.project(), rate);
        }
        self.transport.end_take();
        match done {
            Ok(_) => {
                self.error = None;
                self.after_edit();
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Turns the metronome on and off (REC-003).
    pub fn toggle_metronome(&mut self) -> bool {
        let on = !self.transport.metronome;
        let p = self.project();
        self.transport.set_metronome(&p, on);
        true
    }

    /// Turns repeating of the loop range on and off. With no range yet, the
    /// selected clip (or four bars from the playhead) becomes the range.
    pub fn toggle_loop(&mut self) -> bool {
        let p = self.project();
        if p.loop_range.is_none() {
            let clip = self.selected_clip.and_then(|(t, c)| {
                let clip = p.tracks.get(&t)?.clips.iter().find(|k| k.id == c)?;
                Some((clip.start, clip.end()))
            });
            let (s, e) = clip.unwrap_or_else(|| {
                let tm = p.time_map();
                let (bar, _, _) = tm.to_bbt(self.playhead);
                (tm.bar_start(bar), tm.bar_start(bar + 4))
            });
            if !self.run_command("loop.set", json!({"start": s, "end": e})) {
                return true;
            }
        }
        let on = !self.transport.looping;
        let p = self.project();
        self.transport.set_looping(&p, on);
        true
    }

    /// Runs one core command for the screen; false (and the error shown)
    /// when it failed.
    pub(crate) fn run_command(&mut self, command: &str, args: serde_json::Value) -> bool {
        if self.saving.is_some() {
            return false;
        }
        // A wheel's undo step ends before any other command (P3-7 of review 15).
        if !self.wheel_running {
            self.close_wheel_group();
        }
        if crate::audio_job::NAMES.contains(&command) {
            return self.job_start(command, args);
        }
        match self.song.run(command, args) {
            Ok(data) => {
                self.error = None;
                self.after_edit();
                self.after_command(command, &data);
                true
            }
            Err(e) => {
                self.fail(e);
                false
            }
        }
    }

    /// Keeps the input of the armed audio tracks watched and their meters moving (REC-002).
    /// Gives true while a meter shows something.
    fn watch_inputs(&mut self) -> bool {
        if self.transport.auditioning_export {
            return false;
        }
        let p = self.project();
        let armed: Vec<_> = crate::input::armed(&p).0.iter().map(|t| t.id).collect();
        if let Some(e) = self.transport.watch(&p, self.song.dir(), &armed) {
            self.error = Some(e);
        }
        let levels = self.transport.take_input_levels();
        let moving = self.inputs.feed(&levels);
        self.note_recorded_peaks(&p);
        moving
    }

    /// While recording, keeps the loudest peak of each armed audio track's input (REC-013).
    fn note_recorded_peaks(&mut self, p: &Project) {
        if !self.transport.recording {
            self.rec_peaks.clear();
            return;
        }
        let device = self.transport.input_channels();
        for t in input::armed(p).0 {
            let m = self.inputs.of(t, device);
            let top = m.hold[0].max(m.hold[1]);
            let at = self.rec_peaks.entry(t.id).or_insert(0.0);
            *at = at.max(top);
        }
    }

    /// Whether the take being recorded on `track` has an input outside the frame: its loudest
    /// peak so far is below the zone (too quiet) or above it (too loud). Nothing heard yet is
    /// not outside.
    pub fn input_outside_frame(&self, track: TrackId) -> bool {
        use crate::inmeter::{Fit, fit_of};
        self.transport.recording
            && self
                .rec_peaks
                .get(&track)
                .is_some_and(|top| matches!(fit_of(*top), Fit::Low | Fit::High))
    }

    /// Once a frame: takes in the engine's position and levels. Returns true
    /// while something moves (the playhead or a meter), so the screen asks
    /// for the next frame.
    pub fn tick(&mut self) -> bool {
        let saved = self.poll_saving();
        if self.saving.is_some() {
            if let Some(at) = self.transport.poll() {
                self.playhead = at;
            }
            return true;
        }
        self.transport.park(self.playhead);
        let midi = self.poll_live_midi() || self.live_midi.input.is_some();
        if let Some(at) = self.transport.poll() {
            self.playhead = at;
        }
        // The song played to where the punch range or its post-roll ends: the take is over.
        if self.transport.recording && self.transport.host().is_some_and(|h| h.record_due()) {
            self.finish_take();
        }
        if self.transport.recording
            && let Some(host) = self.transport.host()
        {
            let rate = host.rate();
            let (_, data) = host.record_status();
            self.transport.loss.receive(&data, &self.project(), rate);
        }
        self.auto_settle();
        let levels = self.transport.take_levels();
        let meters = self.mixer.meters.feed(&levels);
        let inputs = self.watch_inputs();
        let scan = self.poll_plugin_scan();
        let loading = self.poll_loading();
        let export = self.poll_export();
        let job = self.poll_job() | self.poll_processed() | self.poll_dawproject();
        let plugins = self.poll_preparing()
            | self.poll_swap()
            | self.poll_waiting_key()
            | self.poll_plugins()
            | self.poll_perf()
            | self.poll_roll_host();
        let editors = self.poll_editors();
        let ai = self.ai_moving();
        let said = self.say_driver_notice();
        let device = self.poll_device();
        self.expire_notice();
        let shaking = self
            .record_shake
            .is_some_and(|t| t.elapsed() < crate::topbar::SHAKE);
        if !shaking {
            self.record_shake = None;
        }
        let toast = self.expire_toast() | self.expire_gain_tip() | self.gain_tip.is_some();
        let settings = self.settings_tick();
        let rig = self.rig_tick();
        let update = self.poll_update();
        saved
            || self.save_mark() == crate::keep::SaveMark::Saving
            || self.transport.playing
            || midi
            || meters
            || inputs
            || scan
            || loading
            || export
            || job
            || toast
            || plugins
            || editors
            || ai
            || settings
            || said
            || shaking
            || device
            || rig
            || update
    }

    /// Keeps the sound device in hand: when it stopped by itself (another program took it, it
    /// was unplugged), a recording is finished and the screen says so; while it cannot be had,
    /// it is opened again every [`REOPEN`], and the screen says when it opened. Playing does
    /// not start again by itself. Gives true when something on screen changed.
    pub fn poll_device(&mut self) -> bool {
        self.poll_output_device() | self.poll_input_device()
    }

    fn poll_output_device(&mut self) -> bool {
        let mut changed = false;
        if self.transport.take.is_some() && self.transport.is_lost() {
            self.finish_take();
            changed = true;
        }
        if let Some(said) = self.transport.take_lost() {
            // What finishing the recording said stays under it.
            self.error = Some(match self.error.take() {
                Some(e) if changed => format!("{said}\n{e}"),
                _ => said,
            });
            changed = true;
        }
        let before = self.transport.down.as_ref().map(|d| d.said.clone());
        let p = self.project();
        let Some(tried) = self.transport.reopen(&p, self.song.dir()) else {
            return changed;
        };
        self.say_reopened(tried, before, REOPENED) | changed
    }

    /// Says what opening a device again (`tried`) came to: that it opened, or why not when that
    /// is not what the screen says already (`before`, said when it went down, unless something
    /// else is said over it). True when the screen changed: a try that failed as the one
    /// before is not drawn again.
    fn say_reopened(
        &mut self,
        tried: Result<(), String>,
        before: Option<String>,
        reopened: &str,
    ) -> bool {
        let shown = self.error.is_none() || self.error == before;
        match tried {
            Ok(()) => {
                if self.error.is_some() && shown {
                    self.error = None;
                }
                self.tell(reopened.into());
                true
            }
            Err(e) if shown && self.error.as_ref() != Some(&e) => {
                self.error = Some(e);
                true
            }
            Err(_) => false,
        }
    }

    /// Keeps the input device in hand as [`Studio::poll_device`] does the output: when it
    /// stopped by itself, a recording is finished (what it recorded so far goes into the song),
    /// the input is let go and the screen says so; while it cannot be had, it is opened again
    /// every [`REOPEN`], and the screen says when it opened. The output plays on. Gives true
    /// when something on screen changed.
    fn poll_input_device(&mut self) -> bool {
        let mut changed = false;
        if let Some(why) = self.transport.input_lost() {
            let finished = self.transport.take.is_some();
            if finished {
                self.finish_take();
            }
            let said = input_unavailable(&why);
            self.transport.input_went_down(said.clone());
            // What finishing the recording said stays under it.
            self.error = Some(match self.error.take() {
                Some(e) if finished => format!("{said}\n{e}"),
                _ => said,
            });
            changed = true;
        }
        let before = self.transport.input_down.as_ref().map(|d| d.said.clone());
        let p = self.project();
        let armed: Vec<_> = crate::input::armed(&p).0.iter().map(|t| t.id).collect();
        let Some(tried) = self.transport.reopen_input(&p, self.song.dir(), &armed) else {
            return changed;
        };
        self.say_reopened(tried, before, INPUT_REOPENED) | changed
    }

    /// Says how the sound device was opened when that was not the way asked for.
    fn say_driver_notice(&mut self) -> bool {
        match self.transport.take_driver_notice() {
            Some(text) => {
                self.tell(text);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Hit;

    fn studio(mode: OutputMode) -> Studio {
        let mut s = Studio::sample();
        s.transport.mode = mode;
        s
    }

    fn paced() -> OutputMode {
        OutputMode::Silent { paced: true }
    }

    /// Polls once a frame until `done`, or gives up after a minute: far over what a run needs,
    /// for a machine busy with a parallel build.
    fn until(s: &mut Studio, done: impl Fn(&Studio) -> bool) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(60) {
            s.tick();
            if done(s) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        false
    }

    #[test]
    fn the_clock_reads_minutes_seconds_and_milliseconds() {
        assert_eq!(clock(0.0), "0:00.000");
        assert_eq!(clock(83.2505), "1:23.251");
    }

    #[test]
    fn without_an_output_play_says_so_and_nothing_starts() {
        let mut s = studio(OutputMode::None);
        assert!(s.transport_key("space"));
        assert!(!s.transport.playing);
        assert!(!s.transport.is_live());
        assert_eq!(s.error.as_deref(), Some("オーディオ出力がありません"));
    }

    #[test]
    fn play_moves_the_playhead_pause_keeps_it_and_stop_goes_back_then_to_the_start() {
        let mut s = studio(paced());
        let start = s.playhead;
        assert!(s.transport_key("space"));
        assert!(s.transport.playing && s.error.is_none(), "{:?}", s.error);
        assert!(
            until(&mut s, |s| s.playhead > start + 200),
            "{}",
            s.playhead
        );

        // Space again pauses where it is.
        s.transport_key("space");
        assert!(!s.transport.playing);
        std::thread::sleep(Duration::from_millis(60));
        s.tick();
        let paused = s.playhead;
        assert!(paused > start);
        std::thread::sleep(Duration::from_millis(100));
        assert!(until(&mut s, |s| !s.transport.playing));
        assert!((s.playhead - paused).abs() < 120, "{} {paused}", s.playhead);

        // Play from there, then stop: back to where play started.
        s.transport_key("space");
        assert!(until(&mut s, |s| s.playhead > paused + 200));
        s.transport_key("enter");
        assert_eq!(s.playhead, paused);
        assert!(until(&mut s, |s| !s.transport.playing && s.playhead == paused));

        // Stop while stopped: the start of the song.
        s.transport_key("enter");
        assert_eq!(s.playhead, 0);
        assert!(until(&mut s, |s| s.playhead == 0));
    }

    #[test]
    fn the_top_bar_buttons_drive_the_same_transport() {
        let mut s = studio(paced());
        let g = s.geometry(1440.0, 860.0);
        let at = s.center_of(&g, Hit::Bar(BarButton::Play)).unwrap();
        assert_eq!(s.hit(&g, at.0, at.1), Hit::Bar(BarButton::Play));
        s.press(&g, Hit::Bar(BarButton::Play));
        assert!(s.transport.playing);
        assert!(s.bar_view().playing);
        s.press(&g, Hit::Bar(BarButton::ToStart));
        assert_eq!(s.playhead, 0);
        assert!(until(&mut s, |s| s.playhead > 100 && s.playhead < 3840 * 2));
        s.press(&g, Hit::Bar(BarButton::Stop));
        assert!(!s.transport.playing);
        assert_eq!(s.playhead, 0);
    }

    #[test]
    fn the_readout_shows_the_bar_and_the_time_of_the_playhead() {
        let mut s = studio(OutputMode::None);
        s.seek(s.project().time_map().bar_start(5));
        let v = s.bar_view();
        assert_eq!(v.time, "5.1.000");
        // Bar 5 at 4/4 is 16 beats in; the sample's tempo decides the seconds.
        let bpm = s.project().tempo[0].bpm;
        assert_eq!(v.seconds, clock(16.0 * 60.0 / bpm));
        assert_eq!(v.bpm, format!("{bpm:.1}"));
    }

    #[test]
    fn record_needs_an_armed_track_then_plays() {
        let mut s = studio(paced());
        let keys = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        assert!(!s.project().tracks.values().any(|t| t.armed));
        s.transport_key("r");
        assert!(!s.transport.playing && !s.transport.recording);
        // No words: the record button shakes and an arm button lights up.
        assert!(s.error.is_none());
        assert!(s.record_shake.is_some());
        assert_eq!(s.arm_cue, s.arm_candidate());
        assert!(s.arm_cue.is_some());
        assert!(s.bar_view().record_shake.abs() <= 3.0);
        s.song.toggle_arm(keys).unwrap();
        s.transport_key("r");
        assert!(
            s.transport.playing && s.transport.recording,
            "{:?}",
            s.error
        );
        assert!(s.bar_view().recording);
        s.transport_key("enter");
        assert!(!s.transport.recording && !s.transport.playing);
        assert!(s.transport.take.is_none());
    }

    fn set_pref(s: &Studio, key: &str, v: serde_json::Value) {
        s.song
            .settings()
            .lock()
            .unwrap()
            .prefs
            .set(key, &v)
            .unwrap();
    }

    #[test]
    fn auto_punch_records_only_inside_the_range_and_stops_by_itself() {
        let mut s = studio(paced());
        let keys = track(&s, "Keys");
        s.song.toggle_arm(keys).unwrap();
        let bar = s.project().time_map().bar_start(2);
        let end = s.project().time_map().bar_start(3);
        s.song
            .run("loop.set", json!({"start": bar, "end": end}))
            .unwrap();
        set_pref(&s, "record.punch", json!(true));
        let before: Vec<_> = s.project().tracks[&keys]
            .clips
            .iter()
            .map(|c| c.id)
            .collect();
        // Pressed from the song's start: the recording waits for the range.
        s.seek(0);
        s.transport_key("r");
        assert!(s.transport.recording, "{:?}", s.error);
        let take = s.transport.take.clone().unwrap();
        assert_eq!((take.from, take.keep), (bar, Some((bar, Some(end)))));
        assert!(
            !s.pre_rolling(),
            "no pre-roll: the song starts at the range"
        );
        // The range is over: the take went into the song by itself.
        assert!(until(&mut s, |s| !s.transport.recording));
        assert!(!s.transport.playing && s.transport.take.is_none());
        let p = s.project();
        let new: Vec<_> = p.tracks[&keys]
            .clips
            .iter()
            .filter(|c| !before.contains(&c.id))
            .collect();
        assert_eq!(new.len(), 1);
        assert!(
            new[0].start >= bar && new[0].start + new[0].length <= end,
            "{:?}",
            new[0].start
        );
        s.transport.reset();
    }

    #[test]
    fn the_pre_roll_plays_up_to_the_range_and_the_record_button_blinks_until_then() {
        let mut s = studio(paced());
        let keys = track(&s, "Keys");
        s.song.toggle_arm(keys).unwrap();
        let bar = s.project().time_map().bar_start(3);
        let end = s.project().time_map().bar_start(4);
        s.song
            .run("loop.set", json!({"start": bar, "end": end}))
            .unwrap();
        set_pref(&s, "record.punch", json!(true));
        set_pref(&s, "record.preroll", json!(true));
        s.seek(0);
        s.transport_key("r");
        assert!(s.transport.recording, "{:?}", s.error);
        // The take counts from the range; the song plays from one bar before it.
        assert_eq!(s.transport.take.as_ref().unwrap().from, bar);
        assert!(s.playhead < bar && s.playhead >= s.project().time_map().bar_start(2));
        assert!(s.pre_rolling() && s.bar_view().pre_roll);
        assert!(until(&mut s, |s| s.playhead >= bar));
        assert!(!s.pre_rolling());
        s.transport_key("enter");
        assert!(s.transport.take.is_none());
        s.transport.reset();
    }

    #[test]
    fn the_record_button_shakes_three_times_and_settles() {
        use crate::topbar::{SHAKE, shake_offset};
        assert_eq!(shake_offset(None, false), 0.0);
        assert_eq!(shake_offset(Some(SHAKE), false), 0.0);
        assert_eq!(shake_offset(Some(Duration::from_millis(40)), true), 0.0);
        let peak = (0..32)
            .map(|i| shake_offset(Some(SHAKE * i / 32), false).abs())
            .fold(0.0, f32::max);
        assert!(peak > 1.0 && peak <= 3.0, "{peak}");
    }

    /// Plays `events` (frame, message) as the MIDI input of the next recording.
    fn play_notes(s: &mut Studio, events: Vec<(u64, [u8; 3])>) {
        s.live_midi_input(Box::new(xirius_engine::record::MockMidi::new(events)));
    }

    /// The notes of the clip the recording on Keys made, as (pitch, tick in the song).
    fn recorded_notes(
        s: &Studio,
        keys: TrackId,
        before: &[xirius_core::id::ClipId],
    ) -> Vec<(u8, i64)> {
        let p = s.project();
        let clip = p.tracks[&keys]
            .clips
            .iter()
            .find(|c| !before.contains(&c.id))
            .expect("the take became a clip");
        let xirius_core::model::ClipBody::Midi { notes, .. } = &clip.body else {
            panic!("a MIDI clip")
        };
        let mut v: Vec<_> = notes
            .iter()
            .map(|n| (n.pitch, clip.start + n.start))
            .collect();
        v.sort_by_key(|n| n.1);
        v
    }

    #[test]
    fn the_note_on_the_very_first_frame_is_shown_and_is_in_the_take() {
        let mut s = studio(paced());
        let keys = track(&s, "Keys");
        s.song.toggle_arm(keys).unwrap();
        let before: Vec<_> = s.project().tracks[&keys]
            .clips
            .iter()
            .map(|c| c.id)
            .collect();
        let rate = 48_000u64;
        play_notes(
            &mut s,
            vec![
                (0, [0x90, 60, 100]),
                (rate / 8, [0x80, 60, 0]),
                (rate / 2, [0x90, 64, 100]),
                (rate / 2 + rate / 8, [0x80, 64, 0]),
            ],
        );
        s.transport_key("r");
        assert!(s.transport.recording, "{:?}", s.error);
        let from = s.transport.take.as_ref().unwrap().from;
        assert!(until(&mut s, |s| s.transport.live_notes.len() >= 2));
        // The display has it, on the frame the take began.
        let shown: Vec<_> = s
            .transport
            .live_notes
            .iter()
            .map(|n| (n.pitch, n.start))
            .collect();
        assert_eq!(shown[0], (60, from), "{shown:?}");
        s.transport_key("enter");
        // And the take that went into the song has the same notes.
        let notes = recorded_notes(&s, keys, &before);
        assert_eq!(notes[0], (60, from), "{notes:?}");
        assert_eq!(notes.iter().map(|n| n.0).collect::<Vec<_>>(), [60, 64]);
        assert_eq!(notes[1].1, shown[1].1, "shown and recorded agree");
        s.transport.reset();
    }

    #[test]
    fn the_note_on_the_frame_the_count_in_ends_is_shown_and_is_in_the_take() {
        let mut s = studio(paced());
        let keys = track(&s, "Keys");
        s.song.toggle_arm(keys).unwrap();
        set_pref(&s, "audio.count-in", json!(1));
        let before: Vec<_> = s.project().tracks[&keys]
            .clips
            .iter()
            .map(|c| c.id)
            .collect();
        let rate = 48_000u64;
        // The sample plays at 120 bpm in 4/4: one bar of count-in is two seconds.
        assert_eq!(s.project().tempo[0].bpm, 120.0);
        let end = rate * 2;
        play_notes(
            &mut s,
            vec![
                // The last frame of the count-in: heard by nobody.
                (end - 1, [0x90, 72, 100]),
                (end, [0x80, 72, 0]),
                // The first frame of the take.
                (end, [0x90, 60, 100]),
                (end + rate / 8, [0x80, 60, 0]),
                (end + rate / 2, [0x90, 64, 100]),
                (end + rate / 2 + rate / 8, [0x80, 64, 0]),
            ],
        );
        s.transport_key("r");
        assert!(s.transport.recording, "{:?}", s.error);
        assert!(s.bar_view().pre_count.is_some());
        let from = s.transport.take.as_ref().unwrap().from;
        assert!(until(&mut s, |s| s.transport.live_notes.len() >= 2));
        let shown: Vec<_> = s
            .transport
            .live_notes
            .iter()
            .map(|n| (n.pitch, n.start))
            .collect();
        assert_eq!(shown[0], (60, from), "{shown:?}");
        assert!(shown.iter().all(|n| n.0 != 72), "{shown:?}");
        s.transport_key("enter");
        let notes = recorded_notes(&s, keys, &before);
        assert_eq!(notes[0], (60, from), "{notes:?}");
        assert_eq!(notes.iter().map(|n| n.0).collect::<Vec<_>>(), [60, 64]);
        s.transport.reset();
    }

    #[test]
    fn the_count_in_is_not_recording_until_the_take_begins() {
        let mut s = studio(paced());
        let keys = track(&s, "Keys");
        s.song.toggle_arm(keys).unwrap();
        s.song
            .settings()
            .lock()
            .unwrap()
            .prefs
            .set("audio.count-in", &json!(1))
            .unwrap();
        s.transport_key("r");
        assert!(s.transport.recording, "{:?}", s.error);
        // While the bar is counted in, the record button counts the beats and the position
        // stays white: red means a take is being recorded.
        let v = s.bar_view();
        assert!(v.recording, "the record button is lit");
        assert!(v.pre_count.is_some_and(|n| n >= 1));
        assert!(!v.recording_now());
        // The count ends and the take begins: the beats are gone and the position is red.
        assert!(until(&mut s, |s| s.bar_view().recording_now()));
        assert!(s.bar_view().pre_count.is_none());
        s.transport_key("enter");
        assert!(s.bar_view().pre_count.is_none() && !s.bar_view().recording);
    }

    #[test]
    fn the_notes_played_show_while_a_midi_take_records() {
        let mut s = studio(paced());
        let keys = track(&s, "Keys");
        s.song.toggle_arm(keys).unwrap();
        s.transport_key("r");
        assert!(s.transport.recording, "{:?}", s.error);
        assert!(s.transport.live_notes.is_empty());
        // The test input plays C, E, G, C on the beats: the first notes are in at once.
        assert!(until(&mut s, |s| s.transport.live_notes.len() >= 3));
        let n = s.transport.live_notes[0];
        assert!(n.pitch == 60 && n.length >= 1, "{n:?}");
        // They sit in the take, from where it began.
        let from = s.transport.take.as_ref().unwrap().from;
        assert!(s.transport.live_notes.iter().all(|n| n.start >= from));
        s.transport_key("enter");
        assert!(s.transport.live_notes.is_empty());
    }

    #[test]
    fn without_a_count_in_the_take_is_recording_at_once() {
        let mut s = studio(paced());
        let keys = track(&s, "Keys");
        s.song.toggle_arm(keys).unwrap();
        s.transport_key("r");
        assert!(s.bar_view().recording_now(), "{:?}", s.error);
        s.transport_key("enter");
    }

    /// A folder of its own under the temp folder, gone when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "xirius-ui-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn track(s: &Studio, name: &str) -> TrackId {
        s.project()
            .tracks
            .values()
            .find(|t| t.name == name)
            .unwrap()
            .id
    }

    #[test]
    fn an_audio_take_needs_a_saved_song() {
        let mut s = studio(paced());
        let vocal = track(&s, "Vocal");
        s.song.toggle_arm(vocal).unwrap();
        s.transport_key("r");
        assert!(!s.transport.recording);
        assert_eq!(
            s.error.as_deref(),
            Some("オーディオを録音するには曲の保存が必要です")
        );
    }

    #[test]
    fn recording_from_the_button_grows_a_take_and_stopping_puts_it_in_as_one_step() {
        let mut s = studio(paced());
        let dir = Scratch::new("record");
        s.song.save_as(&dir.0).unwrap();
        let (vocal, keys) = (track(&s, "Vocal"), track(&s, "Keys"));
        s.song.toggle_arm(vocal).unwrap();
        s.song.toggle_arm(keys).unwrap();
        let clips = |s: &Studio, t: TrackId| s.project().tracks[&t].clips.len();
        let (v0, k0) = (clips(&s, vocal), clips(&s, keys));
        let before: Vec<_> = s.project().tracks[&vocal]
            .clips
            .iter()
            .map(|c| c.id)
            .collect();
        let at = s.project().time_map().bar_start(3);
        s.seek(at);

        let g = s.geometry(1440.0, 860.0);
        s.press(&g, Hit::Bar(BarButton::Record));
        assert!(s.error.is_none(), "{:?}", s.error);
        let take = s.transport.take.clone().expect("recording");
        assert_eq!(
            (take.audio, take.midi, take.from),
            (vec![vocal], vec![keys], at)
        );
        assert!(s.bar_view().recording && s.bar_view().playing);
        // The test input plays notes on the quarter seconds: a second is enough.
        let ppq = i64::from(s.project().ppq);
        assert!(
            until(&mut s, |s| s.playhead > at + ppq * 3),
            "{}",
            s.playhead
        );

        s.press(&g, Hit::Bar(BarButton::Record));
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(!s.transport.recording && s.transport.take.is_none());
        let p = s.project();
        assert_eq!(clips(&s, vocal), v0 + 1);
        assert_eq!(clips(&s, keys), k0 + 1);
        let new = p.tracks[&vocal]
            .clips
            .iter()
            .find(|c| !before.contains(&c.id))
            .unwrap();
        assert!((new.start - at).abs() < ppq / 4, "{} {at}", new.start);
        assert!(new.length > ppq, "{}", new.length);
        let files: Vec<_> = std::fs::read_dir(dir.0.join("Media/Recordings"))
            .unwrap()
            .collect();
        assert!(!files.is_empty());

        // One undo takes both out.
        assert!(s.undo(&g));
        assert_eq!((clips(&s, vocal), clips(&s, keys)), (v0, k0));
        s.transport.reset();
    }

    #[test]
    fn every_armed_track_records_at_once_and_one_the_input_lacks_is_said() {
        let mut s = studio(paced());
        let dir = Scratch::new("record-many");
        s.song.save_as(&dir.0).unwrap();
        let vocal = track(&s, "Vocal");
        let old: Vec<TrackId> = s.project().tracks.keys().copied().collect();
        s.song.add_track("audio").unwrap();
        s.song.add_track("audio").unwrap();
        let p = s.project();
        let added: Vec<TrackId> = crate::draw::ordered_tracks(&p)
            .into_iter()
            .map(|t| t.id)
            .filter(|t| !old.contains(t))
            .collect();
        let [second, third] = added[..] else {
            panic!("two new tracks: {added:?}")
        };
        // The test input has two channels: a track on the third is left out.
        for (t, input) in [(vocal, "In 1"), (second, "In 2"), (third, "In 3")] {
            s.run_command(
                "track.input.set",
                json!({ "ids": [t.to_string()], "input": input }),
            );
            s.song.toggle_arm(t).unwrap();
        }
        let clips = |s: &Studio, t: TrackId| s.project().tracks[&t].clips.len();
        let before = [vocal, second, third].map(|t| clips(&s, t));

        let g = s.geometry(1440.0, 860.0);
        s.press(&g, Hit::Bar(BarButton::Record));
        assert!(s.transport.recording);
        let take = s.transport.take.clone().unwrap();
        assert_eq!(take.audio, [vocal, second]);
        assert!(take.midi.is_empty());
        let said = s.error.clone().unwrap_or_default();
        assert!(said.contains("In 3"), "{said}");
        let ppq = i64::from(s.project().ppq);
        let at = take.from;
        assert!(until(&mut s, |s| s.playhead > at + ppq * 2));

        s.press(&g, Hit::Bar(BarButton::Record));
        assert!(s.error.is_none(), "{:?}", s.error);
        let after = [vocal, second, third].map(|t| clips(&s, t));
        assert_eq!(after, [before[0] + 1, before[1] + 1, before[2]]);
        let files = std::fs::read_dir(dir.0.join("Media/Recordings"))
            .unwrap()
            .count();
        assert_eq!(files, 2);
        s.transport.reset();
    }

    /// A studio whose `project.cleanup` moves files into `bin`, not the
    /// recycle bin.
    fn studio_with_bin(bin: &std::path::Path) -> Studio {
        std::fs::create_dir_all(bin).unwrap();
        let to = bin.to_path_buf();
        let trash: xirius_session::cleanup::Trash = std::sync::Arc::new(move |p| {
            std::fs::rename(p, to.join(p.file_name().unwrap())).map_err(|e| e.to_string())
        });
        let mut s = Studio::new(crate::song::Song::sample_with_trash(trash));
        s.transport.mode = paced();
        s
    }

    /// Records for a little while into the armed tracks.
    fn record_a_take(s: &mut Studio) {
        let g = s.geometry(1440.0, 860.0);
        s.press(&g, Hit::Bar(BarButton::Record));
        assert!(s.transport.recording, "{:?}", s.error);
        let from = s.transport.take.as_ref().unwrap().from;
        let ppq = i64::from(s.project().ppq);
        assert!(until(s, |s| s.playhead > from + ppq * 2));
        s.press(&g, Hit::Bar(BarButton::Record));
        assert!(s.error.is_none(), "{:?}", s.error);
    }

    #[test]
    fn closing_a_song_moves_only_the_recordings_nothing_names_any_more() {
        let song = Scratch::new("cleanup");
        let bin = Scratch::new("cleanup-bin");
        let mut s = studio_with_bin(&bin.0);
        s.song.save_as(&song.0).unwrap();
        let vocal = track(&s, "Vocal");
        s.song.toggle_arm(vocal).unwrap();
        let in_bin = || std::fs::read_dir(&bin.0).unwrap().count();
        let recordings = song.0.join("Media/Recordings");

        // A take in the song stays.
        record_a_take(&mut s);
        assert_eq!(s.close_song(), Ok(0));
        // Taken out by undo, it can still come back with redo: it stays.
        let g = s.geometry(1440.0, 860.0);
        assert!(s.undo(&g));
        let steps = s.song().steps();
        assert_eq!(s.close_song(), Ok(0));
        assert_eq!(s.song().steps(), steps, "tidying is no step");
        assert_eq!(in_bin(), 0);
        // Another edit drops the redo: now nothing names it.
        s.song.toggle_arm(vocal).unwrap();
        // While a take is being made, nothing is moved.
        s.song.toggle_arm(vocal).unwrap();
        s.press(&g, Hit::Bar(BarButton::Record));
        assert!(s.transport.recording);
        assert_eq!(s.close_song(), Ok(0));
        let from = s.transport.take.as_ref().unwrap().from;
        let ppq = i64::from(s.project().ppq);
        assert!(until(&mut s, |s| s.playhead > from + ppq * 2));
        s.press(&g, Hit::Bar(BarButton::Record));
        assert!(s.error.is_none(), "{:?}", s.error);
        let named = std::fs::read_dir(&recordings).unwrap().count();
        assert_eq!(named, 2);

        // Opening another song closes this one: the dropped take goes.
        let other = Scratch::new("cleanup-other");
        let mut next = crate::song::Song::sample();
        next.save_as(&other.0).unwrap();
        s.finish_file_request(crate::studio::FileRequest::Open, &other.0);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert_eq!(s.song().dir().as_deref(), Some(other.0.as_path()));
        assert_eq!(in_bin(), 1);
        assert_eq!(std::fs::read_dir(&recordings).unwrap().count(), 1);
        s.transport.reset();
    }

    #[test]
    fn an_unsaved_song_has_nothing_to_tidy_and_a_test_never_fills_the_recycle_bin() {
        let mut s = studio(paced());
        assert_eq!(s.close_song(), Ok(0));
        // The sample's own refusal: a test that forgot its bin hears of it.
        let dir = Scratch::new("cleanup-refused");
        s.song.save_as(&dir.0).unwrap();
        let stray = dir.0.join("Recordings").join("stray.wav");
        std::fs::create_dir_all(stray.parent().unwrap()).unwrap();
        std::fs::write(&stray, b"RIFF").unwrap();
        let said = s.close_song().unwrap_err();
        assert!(said.contains("試験ではごみ箱に移動しません"), "{said}");
        assert!(stray.exists());
    }

    #[test]
    fn the_metronome_button_turns_it_on_and_off_and_the_bar_shows_it() {
        let mut s = studio(paced());
        let g = s.geometry(1440.0, 860.0);
        let at = s.center_of(&g, Hit::Bar(BarButton::Metronome)).unwrap();
        assert_eq!(s.hit(&g, at.0, at.1), Hit::Bar(BarButton::Metronome));
        assert!(!s.bar_view().metronome);
        s.press(&g, Hit::Bar(BarButton::Metronome));
        assert!(s.transport.metronome && s.bar_view().metronome);
        // Playing with it on starts and runs as before (the click is silent here).
        s.transport_key("space");
        assert!(until(&mut s, |s| s.playhead > 200));
        s.press(&g, Hit::Bar(BarButton::Metronome));
        assert!(!s.transport.metronome && !s.bar_view().metronome);
        s.transport_key("space");
    }

    #[test]
    fn the_click_falls_on_every_beat_at_the_tempo_of_the_song() {
        let mut s = studio(paced());
        let p = s.project();
        s.transport.play(&p, None, 0).unwrap();
        let host = s.transport.host_mut().unwrap();
        let click = click_of(host, &p);
        let rate = f64::from(host.rate());
        let bpm = p.tempo[0].bpm;
        let beat = 60.0 / bpm * rate;
        let mut out = vec![0.0f32; (beat * 2.5) as usize];
        let mut c = click;
        c.render(&mut out, 0);
        let onsets: Vec<usize> = (1..out.len())
            .filter(|&i| out[i] != 0.0 && out[i.saturating_sub(2000)..i].iter().all(|s| *s == 0.0))
            .map(|i| i - 1)
            .collect();
        assert_eq!(onsets.len(), 3, "{onsets:?}");
        assert!((onsets[1] as f64 - beat).abs() < 2.0, "{onsets:?} {beat}");
        s.transport.reset();
    }

    #[test]
    fn loop_without_a_range_takes_the_selected_clip_and_repeats_it() {
        let mut s = studio(paced());
        let (t, c) = s.selected_clip.unwrap();
        let clip = s.project().tracks[&t]
            .clips
            .iter()
            .find(|k| k.id == c)
            .unwrap()
            .clone();
        s.transport_key("/");
        assert!(s.transport.looping);
        let l = s.project().loop_range.unwrap();
        assert_eq!((l.start, l.end), (clip.start, clip.end()));

        // Playing from just before the end comes round to the start.
        s.seek(clip.end() - 240);
        s.transport_key("space");
        assert!(until(&mut s, |s| s.playhead >= clip.start
            && s.playhead < clip.start + 960));
        s.transport_key("/");
        assert!(!s.transport.looping);
        s.transport_key("space");
    }

    #[test]
    fn an_edit_while_playing_reaches_the_player() {
        let mut s = studio(paced());
        s.transport_key("space");
        assert!(until(&mut s, |s| s.playhead > 0));
        let keys = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        assert!(s.run_command(
            "track.volume.set",
            json!({"ids": [keys.to_string()], "db": -6.0})
        ));
        // Still playing after the change was sent.
        assert!(until(&mut s, |s| s.transport.playing));
        s.transport_key("space");
    }

    /// Stand-in drivers: the exclusive one refuses (and gives the engine back) or opens; both
    /// give a silent output, so no sound device is opened.
    fn drivers(exclusive_opens: bool, asked: &Arc<std::sync::Mutex<Vec<&'static str>>>) -> Drivers {
        use xirius_session::output::{Refused, SilentOutput};
        let (a, b) = (asked.clone(), asked.clone());
        Drivers {
            exclusive_out: Some(Arc::new(move |audio, _, _| {
                a.lock().unwrap().push("exclusive");
                if exclusive_opens {
                    Ok(Output::Silent(SilentOutput::start(audio, true)))
                } else {
                    Err(Refused {
                        error: "initialize exclusive: 0x88890008".into(),
                        audio: Some(Box::new(audio)),
                    })
                }
            })),
            shared_out: Arc::new(move |audio, _, _| {
                b.lock().unwrap().push("shared");
                Ok(Output::Silent(SilentOutput::start(audio, true)))
            }),
            exclusive_in: None,
            shared_in: Arc::new(|_, _, _| Err("no input".into())),
            devices: Arc::new(Vec::new),
        }
    }

    /// Stand-in drivers for a device another program holds (exclusive and shared both refuse)
    /// until `free` is set. What opens hands its [`Lost`] to `lost`, so a test can stop it as
    /// the system would.
    fn held(
        free: &Arc<std::sync::atomic::AtomicBool>,
        lost: &Arc<std::sync::Mutex<Option<xirius_engine::device::Lost>>>,
        asked: &Arc<std::sync::Mutex<Vec<&'static str>>>,
    ) -> Drivers {
        use std::sync::atomic::Ordering::SeqCst;
        use xirius_engine::device::IN_USE;
        use xirius_session::output::{Refused, SilentOutput};
        let (f, l, a, b) = (free.clone(), lost.clone(), asked.clone(), asked.clone());
        Drivers {
            exclusive_out: Some(Arc::new(move |audio, _, _| {
                a.lock().unwrap().push("exclusive");
                if !f.load(SeqCst) {
                    return Err(Refused {
                        error: format!("initialize exclusive: {IN_USE}"),
                        audio: Some(Box::new(audio)),
                    });
                }
                let o = SilentOutput::start(audio, true);
                *l.lock().unwrap() = Some(o.lost());
                Ok(Output::Silent(o))
            })),
            shared_out: Arc::new(move |_, _, _| {
                b.lock().unwrap().push("shared");
                Err(format!("build output stream: {IN_USE}"))
            }),
            exclusive_in: None,
            shared_in: Arc::new(|_, _, _| Err("no input".into())),
            devices: Arc::new(Vec::new),
        }
    }

    #[test]
    fn a_device_another_program_holds_is_said_and_opened_again_once_it_is_free() {
        use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
        let free = Arc::new(AtomicBool::new(false));
        let lost = Arc::new(std::sync::Mutex::new(None));
        let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut s = studio(OutputMode::Device);
        s.transport.set_drivers(held(&free, &lost, &asked));
        let g = s.geometry(1440.0, 860.0);
        s.press(&g, Hit::Bar(BarButton::Play));
        // Not a silent player: nothing plays, and the screen says why.
        assert!(!s.transport.playing && !s.transport.is_live());
        let said = s.error.clone().expect("the screen says it");
        assert_eq!(
            said,
            "オーディオデバイスを使用できません（ほかのアプリが使用中です）"
        );
        assert!(s.transport.down.is_some());
        assert_eq!(asked.lock().unwrap().as_slice(), ["exclusive", "shared"]);

        // Tried again while held: still said, not opened, and the screen is not drawn again
        // for a try that changes nothing on it.
        let t = Instant::now();
        let mut drawn = false;
        while t.elapsed() < REOPEN + REOPEN / 2 {
            drawn |= s.poll_device();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            asked.lock().unwrap().len() >= 4,
            "{:?}",
            asked.lock().unwrap()
        );
        assert!(!drawn, "a try that failed again was drawn");
        assert_eq!(s.error.as_deref(), Some(said.as_str()));
        assert!(!s.transport.is_live());

        // The other program lets it go: it opens without a press, and the screen says so.
        free.store(true, SeqCst);
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(60) && s.notice_text() != Some(REOPENED) {
            s.idle();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(s.notice_text(), Some(REOPENED));
        assert!(s.error.is_none() && s.transport.down.is_none());
        assert!(s.transport.is_live() && !s.transport.playing);
        s.press(&g, Hit::Bar(BarButton::Play));
        assert!(s.transport.playing && s.error.is_none(), "{:?}", s.error);
        assert!(until(&mut s, |s| s.playhead > 200), "plays once free");
        s.transport_key("space");
    }

    #[test]
    fn a_device_that_stops_while_playing_is_said_and_opened_again() {
        use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
        let free = Arc::new(AtomicBool::new(true));
        let lost = Arc::new(std::sync::Mutex::new(None));
        let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut s = studio(OutputMode::Device);
        s.transport.set_drivers(held(&free, &lost, &asked));
        let g = s.geometry(1440.0, 860.0);
        s.press(&g, Hit::Bar(BarButton::Play));
        assert!(until(&mut s, |s| s.playhead > 200), "plays");

        // Another program takes the device: the stream ends, as WASAPI ends it.
        free.store(false, SeqCst);
        let gone = lost.lock().unwrap().take().expect("opened");
        gone.set(xirius_engine::device::IN_USE.into());
        s.tick();
        let at = s.playhead;
        assert!(!s.transport.playing && !s.transport.is_live());
        assert_eq!(
            s.error.as_deref(),
            Some("オーディオデバイスを使用できません（ほかのアプリが使用中です）")
        );

        // Free again: opened again on the timer, at the same place, without playing.
        free.store(true, SeqCst);
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(60) && s.notice_text() != Some(REOPENED) {
            s.idle();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(s.notice_text(), Some(REOPENED));
        assert!(s.error.is_none() && s.transport.is_live() && !s.transport.playing);
        assert_eq!(s.playhead, at);
    }

    /// Stand-in drivers whose output opens, and whose input is refused with what `refuse`
    /// holds (another program holds it, it never delivers) until that is taken out. The input
    /// that opens hands its [`Lost`] to `lost`, so a test can stop it as the system would.
    fn input_held(
        refuse: &Arc<std::sync::Mutex<Option<String>>>,
        lost: &Arc<std::sync::Mutex<Option<xirius_engine::device::Lost>>>,
        asked: &Arc<std::sync::atomic::AtomicUsize>,
    ) -> Drivers {
        use xirius_engine::record::MockInput;
        use xirius_session::output::{DeviceInput, SilentOutput};
        let (r, l, a) = (refuse.clone(), lost.clone(), asked.clone());
        Drivers {
            exclusive_out: None,
            shared_out: Arc::new(|audio, _, _| {
                Ok(Output::Silent(SilentOutput::start(audio, true)))
            }),
            exclusive_in: None,
            shared_in: Arc::new(move |rate, _, _| {
                a.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(e) = r.lock().unwrap().clone() {
                    return Err(e);
                }
                let input = DeviceInput::new(
                    "Interface".into(),
                    false,
                    Arc::new(std::sync::atomic::AtomicU64::new(0)),
                    Box::new(MockInput::sine(f64::from(rate), 440.0, 0.5)),
                );
                *l.lock().unwrap() = Some(input.lost());
                Ok(input)
            }),
            devices: Arc::new(Vec::new),
        }
    }

    /// Ticks until the screen says `notice`, for a minute at most.
    fn until_told(s: &mut Studio, notice: &str) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(60) && s.notice_text() != Some(notice) {
            s.tick();
            std::thread::sleep(Duration::from_millis(50));
        }
        s.notice_text() == Some(notice)
    }

    fn watching(s: &Studio) -> bool {
        s.transport.host().is_some_and(|h| h.is_watching())
    }

    #[test]
    fn an_input_another_program_holds_is_said_and_opened_again_once_it_is_free() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        use xirius_engine::device::IN_USE;
        let refuse = Arc::new(std::sync::Mutex::new(Some(format!(
            "入力デバイスを開けません: {IN_USE}"
        ))));
        let lost = Arc::new(std::sync::Mutex::new(None));
        let asked = Arc::new(AtomicUsize::new(0));
        let mut s = studio(OutputMode::Device);
        s.transport.set_drivers(input_held(&refuse, &lost, &asked));
        let vocal = track(&s, "Vocal");
        s.song.toggle_arm(vocal).unwrap();
        s.tick();
        // Not a silent meter: the screen says why, and the output is not let go.
        let said = s.error.clone().expect("the screen says it");
        assert_eq!(
            said,
            "オーディオ入力を使用できません（ほかのアプリが使用中です）"
        );
        assert!(s.transport.input_down.is_some() && !watching(&s));
        assert!(s.transport.is_live() && s.transport.down.is_none());
        assert_eq!(asked.load(SeqCst), 1);

        // Tried again while held: still said, not opened, and not at every frame. The screen
        // is not drawn again for a try that changes nothing on it.
        let t = Instant::now();
        let mut drawn = false;
        while t.elapsed() < Duration::from_secs(60) && asked.load(SeqCst) < 2 {
            drawn |= s.poll_device();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(asked.load(SeqCst), 2);
        assert!(!drawn, "a try that failed again was drawn");
        assert!(t.elapsed() >= REOPEN - Duration::from_millis(100));
        assert_eq!(s.error.as_deref(), Some(said.as_str()));
        assert!(!watching(&s));

        // The other program lets it go: it opens without a press, and the screen says so.
        *refuse.lock().unwrap() = None;
        assert!(until_told(&mut s, INPUT_REOPENED), "{:?}", s.error);
        assert!(s.error.is_none() && s.transport.input_down.is_none());
        assert!(watching(&s));
        s.transport.reset();
    }

    /// `--silent`: an armed track watches a test tone; the input device is not opened, and
    /// nothing is tried again or drawn for it.
    #[test]
    fn a_screen_without_a_device_does_not_open_the_input() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        use xirius_engine::device::IN_USE;
        let refuse = Arc::new(std::sync::Mutex::new(Some(IN_USE.to_string())));
        let lost = Arc::new(std::sync::Mutex::new(None));
        let asked = Arc::new(AtomicUsize::new(0));
        let mut s = studio(paced());
        s.transport.set_drivers(input_held(&refuse, &lost, &asked));
        let vocal = track(&s, "Vocal");
        s.song.toggle_arm(vocal).unwrap();
        s.tick();
        assert!(watching(&s), "the test tone is watched");
        let t = Instant::now();
        let mut drawn = false;
        while t.elapsed() < REOPEN + REOPEN / 2 {
            drawn |= s.poll_device();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!drawn);
        assert_eq!(asked.load(SeqCst), 0, "the device was asked for");
        assert!(s.error.is_none() && s.transport.input_down.is_none());
        s.transport.reset();
    }

    #[test]
    fn an_input_whose_first_period_never_comes_is_said_and_tried_again() {
        use std::sync::atomic::AtomicUsize;
        use xirius_engine::record::NO_INPUT;
        let refuse = Arc::new(std::sync::Mutex::new(Some(NO_INPUT.to_string())));
        let lost = Arc::new(std::sync::Mutex::new(None));
        let asked = Arc::new(AtomicUsize::new(0));
        let mut s = studio(OutputMode::Device);
        s.transport.set_drivers(input_held(&refuse, &lost, &asked));
        let dir = Scratch::new("input-silent");
        s.song.save_as(&dir.0).unwrap();
        let vocal = track(&s, "Vocal");
        s.song.toggle_arm(vocal).unwrap();
        s.tick();
        let said = "オーディオ入力を使用できません（入力デバイスから 1 秒間データが届きません）";
        assert_eq!(s.error.as_deref(), Some(said));
        assert!(s.transport.input_down.is_some());

        // Recording does not start on silence either: it says the same.
        let g = s.geometry(1440.0, 860.0);
        s.press(&g, Hit::Bar(BarButton::Record));
        assert!(!s.transport.recording && s.transport.take.is_none());
        assert_eq!(s.error.as_deref(), Some(said));

        *refuse.lock().unwrap() = None;
        assert!(until_told(&mut s, INPUT_REOPENED), "{:?}", s.error);
        assert!(s.error.is_none() && watching(&s));
        s.transport.reset();
    }

    #[test]
    fn an_input_that_stops_while_watched_is_said_while_the_song_plays_on() {
        use std::sync::atomic::AtomicUsize;
        let refuse = Arc::new(std::sync::Mutex::new(None));
        let lost = Arc::new(std::sync::Mutex::new(None));
        let asked = Arc::new(AtomicUsize::new(0));
        let mut s = studio(OutputMode::Device);
        s.transport.set_drivers(input_held(&refuse, &lost, &asked));
        let vocal = track(&s, "Vocal");
        s.song.toggle_arm(vocal).unwrap();
        s.tick();
        assert!(watching(&s), "{:?}", s.error);
        let g = s.geometry(1440.0, 860.0);
        s.press(&g, Hit::Bar(BarButton::Play));
        assert!(until(&mut s, |s| s.playhead > 200), "plays");

        // Unplugged: the input stops, as WASAPI stops it.
        *refuse.lock().unwrap() = Some("デバイスが見つかりません".into());
        let gone: xirius_engine::device::Lost = lost.lock().unwrap().take().expect("opened");
        gone.set("the device was unplugged".into());
        s.tick();
        assert_eq!(
            s.error.as_deref(),
            Some("オーディオ入力を使用できません（the device was unplugged）")
        );
        assert!(!watching(&s) && s.transport.input_down.is_some());
        let at = s.playhead;
        assert!(s.transport.playing, "the song plays on");
        assert!(until(&mut s, |s| s.playhead > at + 200), "and moves");

        // Plugged in again: opened again on the timer.
        *refuse.lock().unwrap() = None;
        assert!(until_told(&mut s, INPUT_REOPENED), "{:?}", s.error);
        assert!(s.error.is_none() && watching(&s) && s.transport.playing);
        s.transport_key("space");
        s.transport.reset();
    }

    #[test]
    fn an_input_that_stops_while_recording_finishes_the_take_and_keeps_it() {
        use std::sync::atomic::AtomicUsize;
        let refuse = Arc::new(std::sync::Mutex::new(None));
        let lost = Arc::new(std::sync::Mutex::new(None));
        let asked = Arc::new(AtomicUsize::new(0));
        let mut s = studio(OutputMode::Device);
        s.transport.set_drivers(input_held(&refuse, &lost, &asked));
        let dir = Scratch::new("input-lost");
        s.song.save_as(&dir.0).unwrap();
        let vocal = track(&s, "Vocal");
        s.song.toggle_arm(vocal).unwrap();
        let clips = |s: &Studio| s.project().tracks[&vocal].clips.len();
        let before = clips(&s);
        let old: Vec<_> = s.project().tracks[&vocal]
            .clips
            .iter()
            .map(|c| c.id)
            .collect();
        let g = s.geometry(1440.0, 860.0);
        s.press(&g, Hit::Bar(BarButton::Record));
        assert!(s.transport.recording, "{:?}", s.error);
        let from = s.transport.take.as_ref().unwrap().from;
        let ppq = i64::from(s.project().ppq);
        assert!(until(&mut s, |s| s.playhead > from + ppq * 2));

        // Unplugged in the middle of the take.
        *refuse.lock().unwrap() = Some("デバイスが見つかりません".into());
        let gone: xirius_engine::device::Lost = lost.lock().unwrap().take().expect("opened");
        gone.set("the device was unplugged".into());
        s.tick();
        // The take is finished, not left recording silence, and what it got is in the song.
        assert!(!s.transport.recording && s.transport.take.is_none());
        assert_eq!(clips(&s), before + 1);
        let p = s.project();
        let take = p.tracks[&vocal]
            .clips
            .iter()
            .find(|c| !old.contains(&c.id))
            .expect("the take");
        assert!(take.length > ppq, "{}", take.length);
        let said = s.error.clone().expect("said");
        assert!(
            said.starts_with("オーディオ入力を使用できません（the device was unplugged）"),
            "{said}"
        );
        assert!(s.transport.input_down.is_some());
        // One undo takes it out, as any take.
        assert!(s.undo(&g));
        assert_eq!(clips(&s), before);
        assert!(s.redo(&g));
        assert_eq!(clips(&s), before + 1);

        *refuse.lock().unwrap() = None;
        assert!(until_told(&mut s, INPUT_REOPENED), "{:?}", s.error);
        assert!(watching(&s));
        s.transport.reset();
    }

    #[test]
    fn play_opens_the_device_exclusively_by_default_and_says_when_it_had_to_share() {
        // The settings' default is exclusive: it opens, and nothing is said.
        let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut s = studio(OutputMode::Device);
        s.transport.set_drivers(drivers(true, &asked));
        let g = s.geometry(1440.0, 860.0);
        s.press(&g, Hit::Bar(BarButton::Play));
        assert!(s.transport.playing && s.error.is_none(), "{:?}", s.error);
        s.tick();
        assert_eq!(asked.lock().unwrap().as_slice(), ["exclusive"]);
        assert_eq!(s.notice_text(), None);
        s.transport_key("space");

        // The device will not open exclusively: shared, and one line says so.
        let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut s = studio(OutputMode::Device);
        s.transport.set_drivers(drivers(false, &asked));
        let g = s.geometry(1440.0, 860.0);
        s.press(&g, Hit::Bar(BarButton::Play));
        assert!(s.transport.playing && s.error.is_none(), "{:?}", s.error);
        s.tick();
        assert_eq!(asked.lock().unwrap().as_slice(), ["exclusive", "shared"]);
        let said = s.notice_text().expect("the fall back is said").to_string();
        assert!(said.contains("排他") && said.contains("共有"), "{said}");
        assert!(!said.contains('\n'), "{said:?}");
        assert!(
            until(&mut s, |s| s.playhead > 200),
            "plays on the shared output"
        );
        s.transport_key("space");
        // It is said once, not at every frame.
        s.tick();
        assert!(s.transport.take_driver_notice().is_none());
    }

    #[test]
    fn ui_285_the_roll_on_a_part_starts_the_player_before_the_first_note() {
        let mut s = studio(paced());
        assert!(s.roll_target().is_some(), "the roll shows the Keys part");
        assert!(s.transport.host().is_none());
        s.seek(15360);
        s.tick();
        assert!(s.transport.host().is_some(), "started once the roll shows");
        assert!(!s.transport.playing, "it plays nothing");
        // It starts where the playhead stands, and leaves the playhead there.
        for _ in 0..20 {
            s.tick();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(s.playhead, 15360);
        // Without an output it is tried once for the track, not at every frame.
        let mut s = studio(OutputMode::None);
        s.tick();
        assert!(s.transport.host().is_none());
        let t = s.roll_target().unwrap().track.id;
        assert_eq!(s.roll.host_for, Some(t));
    }

    #[test]
    fn ui_285_a_note_does_not_wait_for_a_device_that_cannot_be_had() {
        let mut s = studio(paced());
        let t = s.roll_target().unwrap().track.id;
        s.transport.went_down("デバイスが見つかりません".into());
        let p = s.project();
        let on = NoteKind::On {
            pitch: 60,
            velocity: 0.8,
        };
        assert_eq!(
            s.transport.note(&p, s.song.dir(), t, on),
            Err("デバイスが見つかりません".into())
        );
        assert!(s.transport.host().is_none(), "it did not try to open");
        // The roll asks once for the pitch it wants, not at every move.
        s.roll.audition = Some(60);
        s.roll.audition_by_edit = false;
        s.sound_roll_audition();
        assert_eq!(s.pc_keys.roll, None);
        assert_eq!(s.pc_keys.missed, Some((t, 60)));
        s.roll.audition = None;
        s.sound_roll_audition();
        assert_eq!(s.pc_keys.missed, None);
        // Once it can be had again, the note opens it.
        s.transport.down = None;
        assert_eq!(s.transport.note(&p, s.song.dir(), t, on), Ok(()));
        assert!(s.transport.host().is_some());
    }
}
