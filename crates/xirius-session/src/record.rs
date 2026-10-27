//! Recording into the song (REC-001, REC-004, REC-005, REC-014, REC-016). The UI's record
//! button and the `record.*` commands both come here.
//!
//! - [`SessionHost::record_start`] captures, while the transport plays, every armed track at
//!   once (OPEN-07): each audio track the channels of the input its `input` names (`In 1`,
//!   `In 1+2`; several tracks may take the same ones), each instrument track the MIDI of the port
//!   its `input` names (every port without one), which it also plays as it comes in. The audio thread only pushes into lock-free rings; a thread of
//!   the engine's [`Recorder`] writes the files.
//! - [`SessionHost::record_stop`] stops the transport and the capture, and
//!   [`SessionHost::record_takes`] waits for the files and turns the takes into the arguments
//!   of core's `record.take.add`. [`finish_recording`] does both and runs that command, so one
//!   recording is one step of the history, and brings the engine up to date with the new clips.
//! - The files are 32-bit float WAV (mono or stereo, as the track's input) in `Recordings/` under the song's folder, named after
//!   the track (`Vocal-1.wav`, `Vocal-2.wav`, …, never over an existing file). The clips name
//!   them relative to the folder. Undo takes the clips out and leaves the files, so redo can put
//!   them back.
//! - Each lap of a loop is a take of its own. All of them stay in the song, at the same place;
//!   on each track only the last one plays and the earlier ones are muted (REC-005, REC-014).
//! - With `count_in_bars` a recording started from a stopped transport counts that many bars in
//!   first (REC-003): the song waits at `at` while the engine clicks, then plays from there. The
//!   count is not recorded.
//! - The input is shifted back by the round trip (output to input) so a take lines up with what
//!   was heard: the `latency` given, else the device's input latency plus one block, else 0.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use xirius_core::diff::ChangeSet;
use xirius_core::id::{Node, TrackId};
use xirius_core::model::{Media, Project, TrackKind};
use xirius_core::session::{RunOptions, RunResult};
use xirius_core::time::Tick;
use xirius_core::{Args, CmdError, CmdResult, Session};
use xirius_engine::click::CountIn;
use xirius_engine::record::{
    AudioInput, AudioLane, MAX_INPUT_CHANNELS, MidiInput, RecordSpec, Recorder, Recording, Thru,
    note_of,
};
use xirius_engine::{Command, NoteKind};

use crate::export::sanitize;
use crate::host::SessionHost;
use crate::tempo::TempoMap;

/// The folder the takes are written to, under the song's media folder (BRW-004); a song made
/// before that has its older takes in this folder right under the song's.
pub const FOLDER: &str = Media::RECORDINGS;

/// A take the program died in the middle of, made readable again by [`repair_folder`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovered {
    /// Relative to the song's folder, with `/`.
    pub file: String,
    pub frames: u64,
    pub seconds: u64,
}

/// The folders a song's takes are in: the media folder's `Recordings/`, then the older
/// `Recordings/` right under the song's folder.
pub fn take_folders(media: &Media, dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![media.place(dir, FOLDER)];
    let old = dir.join(FOLDER);
    if !out.contains(&old) {
        out.push(old);
    }
    out
}

/// Looks through the take folders ([`take_folders`] of the default media folder) of the song's
/// folder for takes that were cut off (their header says less than the file holds, or nothing)
/// and writes the header again so that they play. Gives the files it had to fix, in name
/// order. A folder that is not there holds none.
pub fn repair_folder(dir: &Path) -> std::io::Result<Vec<Recovered>> {
    repair_takes(&Media::default(), dir)
}

/// [`repair_folder`] for a song whose media folder is `media`.
pub fn repair_takes(media: &Media, dir: &Path) -> std::io::Result<Vec<Recovered>> {
    let mut files: Vec<PathBuf> = Vec::new();
    for root in take_folders(media, dir) {
        if !root.is_dir() {
            continue;
        }
        files.extend(
            std::fs::read_dir(&root)?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))),
        );
    }
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let Some(r) = xirius_engine::wavfile::repair(&path)? else {
            continue;
        };
        if r.fixed {
            out.push(Recovered {
                file: relative(Some(dir), &path),
                frames: r.frames,
                seconds: r.frames / u64::from(r.sample_rate.max(1)),
            });
        }
    }
    Ok(out)
}

/// How long [`finish_recording`] waits for an audio side that renders nothing to let go of the
/// capture (one that renders, however slowly, is waited for).
pub const STOP_WAIT: Duration = Duration::from_secs(5);

/// The audio input of one recording.
#[derive(Default)]
pub enum Input {
    /// No input: an armed audio track records silence of the right length.
    #[default]
    None,
    /// The device's default input (the microphone).
    Device,
    /// This input (a stand-in such as [`xirius_engine::record::MockInput`]).
    Given(Box<dyn AudioInput>),
}

/// Where `record.start`, typed as a command, takes its inputs from
/// ([`SessionHost::set_record_sources`]).
#[derive(Default)]
pub struct Sources {
    pub audio: Source,
    /// A new MIDI input for each recording; `None`: armed instrument tracks are not recorded.
    pub midi: Option<Box<dyn FnMut() -> Box<dyn MidiInput> + Send>>,
}

#[derive(Default)]
pub enum Source {
    /// A player without a device: silence.
    #[default]
    None,
    Device,
    /// A new input for each recording.
    Make(Box<dyn FnMut() -> Box<dyn AudioInput> + Send>),
}

/// What [`SessionHost::record_start`] is given.
#[derive(Default)]
pub struct RecordOptions {
    /// Play from here (`None`: from where the transport is; a playing transport goes on).
    pub at: Option<Tick>,
    /// The round trip in frames (`None`: the input's own, see the module's notes).
    pub latency: Option<u64>,
    /// How late a note played along reaches the engine, in frames.
    pub midi_latency: u64,
    /// Manual placement offsets, milliseconds (audio, MIDI).
    pub offsets_ms: [f64; 2],
    pub input: Input,
    /// The MIDI input; without one an armed instrument track is not recorded.
    pub midi: Option<Box<dyn MidiInput>>,
    /// Bars clicked before the song starts (0: none). Only a stopped transport counts in, so it
    /// takes `at`.
    pub count_in_bars: u32,
    /// The stretch of the capture to keep, from and to (`None`: all of it; REC-007).
    pub keep: Option<(Tick, Option<Tick>)>,
    /// Stop by itself when the song reaches this tick (the post-roll's end).
    pub stop_at: Option<Tick>,
    /// Snap the heads of the notes played to this grid, in ticks (REC-009).
    pub quantize: Option<Tick>,
}

/// How loud the count-in clicks (linear).
const COUNT_IN_GAIN: f32 = 0.3;

/// What a started recording records.
#[derive(Clone, Debug, PartialEq)]
pub struct Started {
    /// The first of `audio_tracks`.
    pub audio: Option<TrackId>,
    /// The first of `midi_tracks`.
    pub midi: Option<TrackId>,
    /// The audio tracks recorded, top to bottom.
    pub audio_tracks: Vec<TrackId>,
    /// The instrument tracks recorded, top to bottom.
    pub midi_tracks: Vec<TrackId>,
    /// Frames the audio is shifted back by.
    pub latency: u64,
    /// The input's name, for messages.
    pub input: String,
    /// Frames the song waits for the count-in (0: none).
    pub count_in: u64,
    pub warnings: Vec<String>,
}

/// A finished recording: the takes as `record.take.add` takes them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Finished {
    pub dropped_blocks: u64,
    pub dropouts: Vec<xirius_engine::record::Dropout>,
    pub takes: Vec<Value>,
    /// The files written, whether a clip names them or not.
    pub files: Vec<PathBuf>,
    /// Frames captured while recording.
    pub frames: u64,
    /// Frames lost because the writer fell behind (silence in the files).
    pub dropped: u64,
    pub warnings: Vec<String>,
}

impl Finished {
    /// The arguments of `record.take.add`.
    pub fn args(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("takes".into(), Value::Array(self.takes.clone()));
        m
    }

    pub fn summary(&self) -> String {
        let mut s = format!(
            "テイク {} 本・ファイル {} 個",
            self.takes.len(),
            self.files.len()
        );
        if self.dropped > 0 {
            s.push_str(&format!(
                "・書き込みの遅れで {} フレーム（{} 回）が欠落しました",
                self.dropped, self.dropped_blocks
            ));
        }
        s
    }

    pub fn to_json(&self) -> Value {
        json!({
            "takes": self.takes,
            "files": self.files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>(),
            "frames": self.frames,
            "dropped": self.dropped,
            "dropped_blocks": self.dropped_blocks,
            "dropouts": dropout_rows(&self.dropouts),
            "warnings": self.warnings,
        })
    }
}

/// A recording under way, kept by the host.
pub(crate) struct Active {
    recorder: Recorder,
    offsets_ms: [f64; 2],
    /// The track of each lane of the recorder.
    audio: Vec<TrackId>,
    /// The instrument tracks and the MIDI port each takes (`None`: every port).
    pub(crate) midi: Vec<(TrackId, Option<usize>)>,
    /// What the engine's thru plays, as last sent: the track values are followed while the
    /// take records (UI-312).
    pub(crate) thru: Vec<xirius_engine::record::Thru>,
    dir: Option<PathBuf>,
    latency: u64,
    input: String,
    started: Instant,
    stopped: bool,
    warnings: Vec<String>,
    /// The watched input this recording went on with; given back to the host when it ends.
    watched: Option<crate::watch::Watch>,
    /// Says why the input device recorded from stopped by itself (`None`: no device).
    lost: Option<xirius_engine::device::Lost>,
    /// The beats counted in, and the frames of one (`None`: no count-in).
    count_in: Option<(u32, f64)>,
    keep: Option<(Tick, Option<Tick>)>,
    stop_at: Option<Tick>,
    quantize: Option<Tick>,
}

impl Active {
    /// Why the input device recorded from stopped by itself, once it has.
    pub(crate) fn input_lost(&self) -> Option<String> {
        self.lost.as_ref()?.get()
    }
}

/// A note played while recording, before the take is a clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveNote {
    pub start: Tick,
    pub length: Tick,
    pub pitch: u8,
}

/// Where a count-in stands: the beats still to click and the beats in all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreCount {
    pub left: u32,
    pub beats: u32,
}

fn conflict(msg: impl Into<String>) -> CmdError {
    CmdError::conflict(msg.into())
}

fn queue_full() -> CmdError {
    conflict("オーディオエンジンが処理中です。しばらく待ってからやり直してください")
}

/// The tracks top to bottom.
pub(crate) fn column(p: &Project) -> Vec<TrackId> {
    let mut out = Vec::new();
    for n in &p.root {
        match n {
            Node::Track(t) => out.push(*t),
            Node::Folder(f) => out.extend(p.tracks_in(*f)),
        }
    }
    out
}

/// The count-in of `bars` bars at `at`: the beat of the meter in force there, as long as the
/// tempo makes it at that place.
fn count_in_of(p: &Project, tmap: &TempoMap, rate: f64, at: Tick, bars: u32) -> CountIn {
    let (bar, _, _) = p.time_map().to_bbt(at);
    let (num, den) = p
        .timesig
        .iter()
        .rfind(|t| t.bar <= bar)
        .or(p.timesig.first())
        .map_or((4, 4), |t| (t.num, t.den.max(1)));
    let beat_ticks = (i64::from(p.ppq) * 4 / i64::from(den)).max(1);
    let beat = tmap.frame_at(at + beat_ticks, rate) - tmap.frame_at(at, rate);
    CountIn::new(rate, beat as f64, num, bars, COUNT_IN_GAIN)
}

/// The first tick at or after `frame`.
fn ceil_tick(tmap: &TempoMap, frame: u64, rate: f64) -> Tick {
    let mut t = tmap.tick_of_frame(frame, rate);
    while tmap.frame_at(t, rate) < frame {
        t += 1;
    }
    while t > 0 && tmap.frame_at(t - 1, rate) >= frame {
        t -= 1;
    }
    t
}

/// The last tick at or before `frame`.
fn floor_tick(tmap: &TempoMap, frame: u64, rate: f64) -> Tick {
    let mut t = tmap.tick_of_frame(frame, rate).max(0);
    while t > 0 && tmap.frame_at(t, rate) > frame {
        t -= 1;
    }
    while tmap.frame_at(t + 1, rate) <= frame {
        t += 1;
    }
    t
}

/// `<dir>/<base>-<n>.wav` for the first `n` from 1 that names no file, nor one a cleanup has
/// aside there ([`crate::cleanup`]): that goes back by its name.
pub(crate) fn unused(dir: &Path, base: &str) -> PathBuf {
    let aside = crate::cleanup::aside_names(dir);
    (1u32..)
        .map(|n| format!("{base}-{n}.wav"))
        .find(|name| !aside.contains(&name.to_lowercase()) && !dir.join(name).exists())
        .map(|name| dir.join(name))
        .expect("some number is free")
}

/// `path` relative to the song's folder, with `/`, as a clip names it.
fn relative(dir: Option<&Path>, path: &Path) -> String {
    match dir.and_then(|d| path.strip_prefix(d).ok()) {
        Some(rel) => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        None => path.display().to_string(),
    }
}

/// The input channels (from 0) a track's input (`track.input`) names: `In 1` is mono, `In 1+2`
/// or `In 3/4` stereo. Without an input a track records `In 1+2`, or `In 1` from a mono input.
/// `channels` is how many the input has.
pub fn input_channels(input: Option<&str>, channels: usize) -> Result<Vec<usize>, String> {
    let channels = channels.max(1);
    let Some(text) = input.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(if channels >= 2 { vec![0, 1] } else { vec![0] });
    };
    let lower = text.to_ascii_lowercase();
    let rest = ["input", "in", "入力"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower)
        .trim();
    let nums: Option<Vec<usize>> = rest
        .split(['+', '/', '-', ','])
        .map(|n| n.trim().parse::<usize>().ok().filter(|&n| n >= 1))
        .collect();
    let Some(nums) = nums.filter(|n| (1..=2).contains(&n.len())) else {
        return Err(format!(
            "入力「{text}」を読み取れません（In 1・In 1+2 のように指定します）"
        ));
    };
    if nums.iter().any(|&n| n > channels) {
        return Err(format!(
            "入力「{text}」が見つかりません（入力デバイスのチャンネルは {channels} 個です）"
        ));
    }
    Ok(nums.into_iter().map(|n| n - 1).collect())
}

/// The MIDI port (an index into `ports`) an instrument track's input (`track.input`) names:
/// the exact name, else the same name in other case, else the one port whose name contains it.
/// `None`: every port (no input, `all`, or a MIDI input whose ports have no names).
pub fn midi_port(input: Option<&str>, ports: &[String]) -> Result<Option<usize>, String> {
    xirius_engine::midi_in::find_port(input, ports)
}

impl SessionHost {
    /// Where `record.start`, typed as a command, takes its inputs from. A new host has none:
    /// it records silence on an armed audio track and leaves instrument tracks out.
    pub fn set_record_sources(&mut self, sources: Sources) {
        self.record_sources = sources;
    }

    /// Whether a recording is under way (started and not taken with [`Self::record_takes`]).
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Starts recording the armed tracks of `p` and plays (from `opts.at` if given). `dir` is
    /// the song's folder, where the files go; a song with an armed audio track must be saved.
    pub fn record_start(
        &mut self,
        p: &Project,
        dir: Option<&Path>,
        opts: RecordOptions,
    ) -> CmdResult<Started> {
        if self.recording.is_some() {
            return Err(conflict(
                "すでに録音中です（record.stop で停止してください）",
            ));
        }
        let mut warnings = Vec::new();
        let (mut audio_t, mut inst_t) = (Vec::new(), Vec::new());
        for t in column(p)
            .iter()
            .filter_map(|id| p.tracks.get(id))
            .map(Arc::as_ref)
            .filter(|t| t.armed)
        {
            // A frozen track cannot take a clip (freeze::check turns the take's edit away), so
            // it is left out with a word instead of dropping every track's take with it.
            if t.frozen.is_some() {
                warnings.push(format!("{} はフリーズ中のため録音しません", t.name));
                continue;
            }
            match t.kind {
                TrackKind::Audio => audio_t.push(t),
                TrackKind::Instrument => inst_t.push(t),
                _ => warnings.push(format!(
                    "{} は録音できない種類のトラックのため、録音しません",
                    t.name
                )),
            }
        }
        if audio_t.is_empty() && inst_t.is_empty() {
            return Err(conflict(match warnings.is_empty() {
                true => "録音準備のトラックがありません（track.arm で録音準備にしてください）"
                    .to_string(),
                false => warnings.join("\n"),
            }));
        }
        if self.windowed
            && !audio_t.is_empty()
            && matches!(opts.input, Input::Device)
            && !self.record_input_ready().map_err(conflict)?
        {
            return Err(conflict("オーディオ入力を準備しています"));
        }
        let rate = self.rate();
        let r = f64::from(rate);
        let song_dir = match audio_t.is_empty() {
            true => None,
            false => Some(dir.ok_or_else(|| {
                conflict("録音するには曲の保存が必要です（録音のファイルは曲のフォルダーに書き込みます）")
            })?),
        };

        // The audio input is opened first, so its channels are known. The device's input that
        // is already watched is not opened twice.
        let reuse = self
            .watch
            .as_ref()
            .filter(|w| w.device && !audio_t.is_empty() && matches!(opts.input, Input::Device))
            .map(|w| (w.name.clone(), w.probe.clone(), w.channels, w.lost.clone()));
        let (input, input_name, probe, channels, lost) = match (audio_t.is_empty(), opts.input) {
            (true, _) => (None, "MIDI のみ".to_string(), None, 0, None),
            (false, Input::Device) if reuse.is_some() => {
                let (name, probe, ch, lost) = reuse.clone().expect("checked in the guard");
                (None, name, probe, ch, lost)
            }
            (false, Input::None) => (
                None,
                "なし（無音を録音します）".to_string(),
                None,
                MAX_INPUT_CHANNELS,
                None,
            ),
            (false, Input::Given(i)) => {
                let ch = i.channels();
                (Some(i), "指定された入力".to_string(), None, ch, None)
            }
            (false, Input::Device) => {
                let c = self.open_configured_input().map_err(conflict)?;
                let (name, probe, ch) = (c.name.clone(), c.latency_probe(), c.channels());
                let lost = c.lost();
                (
                    Some(Box::new(c) as Box<dyn AudioInput>),
                    name,
                    Some(probe),
                    ch,
                    Some(lost),
                )
            }
        };
        // OPEN-07: every armed audio track records its own channels; several may record the
        // same one.
        let mut lanes = Vec::new();
        for t in &audio_t {
            match input_channels(t.input.as_deref(), channels) {
                Ok(ch) => lanes.push((t.id, sanitize(&t.name), ch)),
                Err(e) => warnings.push(format!("{} は録音しません: {e}", t.name)),
            }
        }
        let ports = opts.midi.as_ref().map(|m| m.ports()).unwrap_or_default();
        let mut midi_t = Vec::new();
        for t in &inst_t {
            if opts.midi.is_none() {
                warnings.push(format!("MIDI 入力がないため、{} は録音しません", t.name));
                continue;
            }
            match midi_port(t.input.as_deref(), &ports) {
                Ok(port) => midi_t.push((t.id, port)),
                Err(e) => warnings.push(format!("{} は録音しません: {e}", t.name)),
            }
        }
        if lanes.is_empty() && midi_t.is_empty() {
            return Err(conflict(match (audio_t.is_empty(), opts.midi.is_none()) {
                (true, true) => format!(
                    "{} は録音準備ですが、MIDI 入力がないため録音できません",
                    inst_t.first().map_or("", |t| t.name.as_str())
                ),
                _ => warnings.join("\n"),
            }));
        }
        let rec_dir = match (song_dir, lanes.is_empty()) {
            (Some(d), false) => Some(p.media.place(d, FOLDER)),
            _ => None,
        };

        // The watched input is the recording's now (or, if another one comes, is let go).
        let watched = match audio_t.is_empty() {
            true => None,
            false => self.watch.take().filter(|_| reuse.is_some()),
        };
        if !audio_t.is_empty() {
            self.apply_input_plan(p, channels);
        }
        // The input goes to the engine first, so a device has started when it is measured.
        if let Some(i) = input.filter(|_| !lanes.is_empty())
            && !self.send(Command::SetInput(Some(i)))
        {
            return Err(queue_full());
        }
        // An input no lane records from is not heard, stopped or not.
        let lost = lost.filter(|_| !lanes.is_empty());
        let latency = match (opts.latency, probe) {
            (Some(f), _) => f,
            (None, Some(probe)) => {
                let until = Instant::now() + Duration::from_millis(300);
                while !self.windowed && probe.load(Ordering::Relaxed) == 0 && Instant::now() < until
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                let input_ns = probe.load(Ordering::Relaxed) as f64;
                let block = self.controller().shape().setup.max_block as u64;
                (input_ns * r / 1e9).round() as u64 + block
            }
            (None, None) => 0,
        };

        let (latency, audio_offset) =
            crate::midi_settings::compensate(latency, opts.offsets_ms[0], r);
        let (midi_latency, midi_offset) =
            crate::midi_settings::compensate(opts.midi_latency, opts.offsets_ms[1], r);
        let tmap = self.tempo_map().clone();
        let folder = rec_dir.unwrap_or_default();
        let thru: Vec<Thru> = midi_t
            .iter()
            .filter_map(|&(t, port)| {
                let track = self.track_index(t)?;
                let values = p.tracks.get(&t);
                Some(Thru {
                    track,
                    port,
                    transpose: values.map_or(0, |x| x.transpose),
                    velocity: values.map_or(0, |x| x.velocity),
                })
            })
            .collect();
        let audio: Vec<TrackId> = lanes.iter().map(|l| l.0).collect();
        let spec = RecordSpec {
            sample_rate: rate,
            lanes: lanes
                .into_iter()
                .map(|(_, base, channels)| {
                    let folder = folder.clone();
                    AudioLane {
                        channels,
                        path: Box::new(move |_| {
                            #[cfg(feature = "test-io")]
                            let _ = xirius_core::io_test::before("record_writer", &folder);
                            // Directory and filename I/O belongs to the writer too.
                            #[cfg(feature = "test-io")]
                            let _ = xirius_core::io_test::before("record_directory", &folder);
                            let _ = std::fs::create_dir_all(&folder);
                            unused(&folder, &base)
                        }),
                    }
                })
                .collect(),
            midi: !midi_t.is_empty(),
            latency,
            midi_latency,
            align: Box::new(move |f| tmap.frame_at(ceil_tick(&tmap, f, r), r)),
            ring_seconds: 4.0,
            thru: thru.clone(),
        };
        let (recorder, capture) = Recorder::start(spec)
            .map_err(|e| conflict(format!("録音の書き込みを開始できません: {e}")))?;
        if let Some(m) = opts.midi.filter(|_| !midi_t.is_empty())
            && !self.send(Command::SetMidiInput(Some(m)))
        {
            return Err(queue_full());
        }
        // A recording from a position stops there before the capture begins: the engine may
        // render between two commands, and a capture that saw the transport still playing at
        // the old position would give a take there.
        if let Some(t) = opts.at
            && !(self.stop() && self.seek_tick(t))
        {
            return Err(queue_full());
        }
        if !self.send(Command::Record(Some(capture))) {
            return Err(queue_full());
        }
        let count_in = match (opts.at, opts.count_in_bars) {
            (Some(at), bars) if bars > 0 => Some(Box::new(count_in_of(
                p,
                &self.tempo_map().clone(),
                r,
                at,
                bars,
            ))),
            _ => None,
        };
        let started = Started {
            audio: audio.first().copied(),
            midi: midi_t.first().map(|m| m.0),
            audio_tracks: audio.clone(),
            midi_tracks: midi_t.iter().map(|m| m.0).collect(),
            latency,
            input: input_name,
            count_in: count_in.as_ref().map_or(0, |c| c.frames()),
            warnings,
        };
        self.recording = Some(Active {
            offsets_ms: [audio_offset, midi_offset],
            recorder,
            audio,
            midi: midi_t,
            thru,
            dir: dir.map(Path::to_path_buf),
            latency,
            input: started.input.clone(),
            started: Instant::now(),
            stopped: false,
            warnings: started.warnings.clone(),
            watched,
            lost,
            count_in: count_in.as_ref().map(|c| c.beats()),
            keep: opts.keep,
            stop_at: opts.stop_at,
            quantize: opts.quantize,
        });
        // The count is told before it starts, so a screen never sees the recording without it.
        if let Some(c) = &count_in {
            self.engine_stats().set_count_in_left(c.frames());
        }
        // Played on past the song's end while recording.
        let ok = self.send_end()
            && count_in.is_none_or(|c| self.send(Command::SetCountIn(Some(c))))
            && self.play();
        if !ok {
            self.record_stop();
            return Err(queue_full());
        }
        Ok(started)
    }

    /// Stops the transport and the capture; the capture lets go once the input has caught up
    /// with what was played (the latency later). `false` when not recording. An input device
    /// that stopped by itself ends the takes at the last frame it delivered: the silence after
    /// it, until the stop was noticed, is not kept.
    pub fn record_stop(&mut self) -> bool {
        let Some(a) = &self.recording else {
            return false;
        };
        if !a.stopped {
            if a.input_lost().is_some() {
                a.recorder.end_at_last_input();
            }
            self.engine_stats().set_count_in_left(0);
            let ok = self.stop() & self.send(Command::Record(None));
            if let Some(a) = &mut self.recording {
                a.stopped = ok;
            }
        }
        true
    }

    /// Requests stop and polls the real writer; no callback/disk wait on the caller.
    /// The active recording remains owned by this host until its files are closed.
    pub fn record_takes_ready(&mut self, wait: Duration) -> CmdResult<Option<Finished>> {
        if !self.record_stop() {
            return Err(conflict("録音していないため、停止できません"));
        }
        let active = self.recording.as_mut().expect("checked above");
        if !active.stopped || !active.recorder.finish_ready(wait) {
            return Ok(None);
        }
        self.record_takes(Duration::ZERO).map(Some)
    }

    /// Waits for the capture to end (stopping it if [`Self::record_stop`] was not called),
    /// closes the files and gives the takes. The engine must go on rendering meanwhile (a device
    /// or a silent output does); it is given up on only once it has rendered nothing for `wait`.
    pub fn record_takes(&mut self, wait: Duration) -> CmdResult<Finished> {
        if !self.record_stop() {
            return Err(conflict("録音していないため、停止できません"));
        }
        let a = self.recording.take().expect("checked above");
        let rec = a.recorder.finish(wait);
        self.send_end();
        // The input is closed (a microphone is let go) until the next recording, unless it is
        // watched.
        match a.watched {
            Some(w) => self.watch = Some(w),
            None => {
                self.close_input();
                self.input_state().set_monitor(0, 0);
            }
        }
        let _ = self.send(Command::SetMidiInput(None));
        self.poll();
        let rate = f64::from(self.rate());
        let mut f = takes_of(
            &rec,
            self.tempo_map(),
            rate,
            &a.audio,
            &a.midi,
            a.dir.as_deref(),
        );
        if let Some((_, loop_end)) = self.loop_ticks {
            select_lap(&mut f.takes, loop_end);
        }
        for take in &mut f.takes {
            let ms = a.offsets_ms[usize::from(take.get("notes").is_some())];
            if ms != 0.0 {
                let tick = take["start"].as_i64().unwrap_or(0);
                let frame = self.tempo_map().frame_at(tick, rate);
                let shifted = crate::midi_settings::shift_frame(frame, ms, rate);
                take["start"] = json!(self.tempo_map().tick_of_frame(shifted, rate));
            }
        }
        if a.keep.is_some() || a.quantize.is_some() {
            let (takes, gone) =
                crate::punch::shape(std::mem::take(&mut f.takes), a.keep, a.quantize);
            f.takes = takes;
            if gone > 0 {
                f.warnings.push(format!(
                    "録音の範囲の外だったテイク {gone} 本はクリップにしません（ファイルは残っています）"
                ));
            }
        }
        f.warnings.splice(0..0, a.warnings);
        if !rec.detached {
            f.warnings.push(format!(
                "オーディオエンジンが {:.1} 秒止まったため、録音の終了を待たずに閉じました（最後の部分が欠けている可能性があります）",
                wait.as_secs_f64()
            ));
        }
        f.warnings.extend(rec.errors);
        if rec.lost_midi > 0 {
            f.warnings.push(format!(
                "MIDI のメッセージ {} 個を記録できませんでした（バッファーがいっぱいでした）",
                rec.lost_midi
            ));
        }
        Ok(f)
    }

    /// Whether the song has played to where the recording stops by itself (the end of the
    /// punch range or of its post-roll).
    pub fn record_due(&self) -> bool {
        let Some(stop) = self
            .recording
            .as_ref()
            .filter(|a| !a.stopped)
            .and_then(|a| a.stop_at)
        else {
            return false;
        };
        self.pre_count().is_none() && self.tick_now().is_some_and(|t| t >= stop)
    }

    /// Where the recording begins to count: the start of the stretch kept (`None`: it counts
    /// from the first frame). The playhead is before it during a pre-roll.
    pub fn record_keep(&self) -> Option<(Tick, Option<Tick>)> {
        self.recording.as_ref().and_then(|a| a.keep)
    }

    /// The count-in before the recording, while it clicks (the song waits at its start meanwhile).
    pub fn pre_count(&self) -> Option<PreCount> {
        let (beats, beat) = self.recording.as_ref()?.count_in?;
        let left = self.engine_stats().count_in_left();
        (left > 0).then(|| PreCount {
            left: ((left as f64 / beat).ceil() as u32).clamp(1, beats),
            beats,
        })
    }

    /// The notes played into the MIDI tracks so far, in song ticks, a note still held lasting to
    /// the playhead. Empty when no MIDI track records.
    pub fn live_notes(&self) -> Vec<LiveNote> {
        let Some(a) = self.recording.as_ref().filter(|a| !a.midi.is_empty()) else {
            return Vec::new();
        };
        let Some(now) = self.tick_now() else {
            return Vec::new();
        };
        notes_of(
            &a.recorder.live_midi(),
            self.tempo_map(),
            f64::from(self.rate()),
            0,
            now,
        )
        .iter()
        .filter_map(|n| {
            Some(LiveNote {
                start: n["start"].as_i64()?,
                length: n["length"].as_i64()?,
                pitch: n["pitch"].as_u64()? as u8,
            })
        })
        .collect()
    }

    /// What `record.status` says.
    pub fn record_status(&self) -> (String, Value) {
        let Some(a) = &self.recording else {
            return ("録音していません".into(), json!({ "recording": false }));
        };
        let secs = a.started.elapsed().as_secs_f64();
        let audio: Vec<String> = a.audio.iter().map(|t| t.to_string()).collect();
        let midi: Vec<String> = a.midi.iter().map(|t| t.0.to_string()).collect();
        let tracks: Vec<String> = audio.iter().chain(&midi).cloned().collect();
        let dropped = a.recorder.dropped();
        let dropped_blocks = a.recorder.dropped_blocks();
        let disk_space_low = a.recorder.disk_space_low();
        let mut warnings: Vec<String> = if disk_space_low {
            vec!["録音先のディスクの空き容量が少なくなっています".into()]
        } else {
            Vec::new()
        };
        if let Some(error) = a.recorder.writer_error() {
            warnings.push(error);
        }
        if a.recorder.disk_space_unknown() {
            warnings.push("録音先のディスクの空き容量を取得できません".into());
        }
        (
            format!(
                "録音中（{secs:.1} 秒・入力 {}・レイテンシー補正 {} フレーム{}）{}",
                a.input,
                a.latency,
                if dropped > 0 {
                    format!("・欠落 {dropped} フレーム")
                } else {
                    String::new()
                },
                warnings.first().map_or(String::new(), |w| format!("・{w}"))
            ),
            json!({
                "recording": true,
                "stopping": a.stopped,
                "audio_track": audio.first(),
                "midi_track": midi.first(),
                "audio_tracks": audio,
                "midi_tracks": midi,
                "tracks": tracks,
                "seconds": secs,
                "input": a.input,
                "latency": a.latency,
                "dropped": dropped,
                "dropped_blocks": dropped_blocks,
                "dropouts": dropout_rows(&a.recorder.dropouts()),
                "disk_space_low": disk_space_low,
                "disk_space_unknown": a.recorder.disk_space_unknown(),
                "warnings": warnings,
            }),
        )
    }
}

/// The takes of a recording as `record.take.add` takes them: an audio take (one per lane, on
/// the lane's track) from its aligned first frame for as many whole ticks as the file holds; a
/// MIDI take (one per instrument track, of the messages of its port) from the tick at or before
/// its run's start to the tick at or after its end, its notes paired by pitch.
fn takes_of(
    rec: &Recording,
    tmap: &TempoMap,
    rate: f64,
    audio: &[TrackId],
    midi: &[(TrackId, Option<usize>)],
    dir: Option<&Path>,
) -> Finished {
    let mut f = Finished {
        dropped_blocks: rec.dropped_blocks,
        dropouts: rec.dropouts.clone(),
        frames: rec.frames,
        dropped: rec.dropped,
        ..Finished::default()
    };
    if rec.disk_space_low {
        f.warnings
            .push("録音先のディスクの空き容量が少なくなっています".into());
    }
    if rec.disk_space_unknown {
        f.warnings
            .push("録音先のディスクの空き容量を取得できません".into());
    }
    for take in &rec.takes {
        for a in &take.audio {
            f.files.push(a.path.clone());
            let Some(track) = audio.get(a.lane) else {
                continue;
            };
            let start = ceil_tick(tmap, a.start, rate);
            let end = floor_tick(tmap, a.start + a.frames, rate);
            if end > start {
                f.takes.push(json!({
                    "track": track.to_string(),
                    "start": start,
                    "length": end - start,
                    "file": relative(dir, &a.path),
                }));
            } else {
                f.warnings.push(format!(
                    "{} は 1 tick より短いため、クリップにしません",
                    a.path.display()
                ));
            }
        }
        for &(track, port) in midi {
            let start = floor_tick(tmap, take.song_start, rate);
            let end = ceil_tick(tmap, take.song_end, rate).max(start + 1);
            let msgs: Vec<(u64, [u8; 3])> = take
                .midi
                .iter()
                .filter(|m| port.is_none_or(|p| p == m.port))
                .map(|m| (m.at, m.msg))
                .collect();
            let notes = notes_of(&msgs, tmap, rate, start, end);
            if !notes.is_empty() {
                f.takes.push(json!({
                    "track": track.to_string(),
                    "start": start,
                    "length": end - start,
                    "notes": notes,
                }));
            }
        }
    }
    if !midi.is_empty() && !f.takes.iter().any(|t| t.get("notes").is_some()) {
        f.warnings
            .push("MIDI のメッセージを受信しませんでした".into());
    }
    f
}

/// How far short of the loop's end a lap may stop and still count as a whole one.
const LAP_SLACK: Tick = 2;

/// Of the laps of a loop recording, on each track only the last one that was recorded to the end
/// of the loop plays (the selected take). A lap stopped midway stays as an unselected take;
/// with no whole lap at all, the last one plays. A track with one take is left alone.
fn select_lap(takes: &mut [Value], loop_end: Tick) {
    let mut tracks: Vec<String> = Vec::new();
    for t in takes.iter() {
        if let Some(id) = t["track"].as_str()
            && !tracks.iter().any(|x| x == id)
        {
            tracks.push(id.to_string());
        }
    }
    for id in tracks {
        let laps: Vec<usize> = (0..takes.len())
            .filter(|&i| takes[i]["track"].as_str() == Some(id.as_str()))
            .collect();
        if laps.len() < 2 {
            continue;
        }
        let whole = |i: usize| {
            let end =
                takes[i]["start"].as_i64().unwrap_or(0) + takes[i]["length"].as_i64().unwrap_or(0);
            end + LAP_SLACK >= loop_end
        };
        let chosen = laps
            .iter()
            .rev()
            .copied()
            .find(|&i| whole(i))
            .unwrap_or(laps[laps.len() - 1]);
        for i in laps {
            takes[i]["muted"] = json!(i != chosen);
        }
    }
}

fn dropout_rows(rows: &[xirius_engine::record::Dropout]) -> Vec<Value> {
    rows.iter().map(|r| json!({ "capture_frame": r.capture_frame, "song_frame": r.song_frame, "frames": r.frames, "blocks": r.blocks })).collect()
}

/// Note ons and offs paired by pitch, in ticks from `start`. A note still held at the end ends
/// there; a second note on of a held pitch ends the first.
fn notes_of(
    msgs: &[(u64, [u8; 3])],
    tmap: &TempoMap,
    rate: f64,
    start: Tick,
    end: Tick,
) -> Vec<Value> {
    let mut held: [Option<(Tick, u8)>; 128] = [None; 128];
    let mut notes = Vec::new();
    let close = |notes: &mut Vec<Value>, pitch: u8, on: (Tick, u8), at: Tick| {
        notes.push(json!({
            "start": on.0,
            "length": (at - on.0).max(1),
            "pitch": pitch,
            "velocity": on.1,
        }));
    };
    for &(frame, msg) in msgs {
        let at = tmap.tick_of_frame(frame, rate).clamp(start, end) - start;
        match note_of(msg) {
            Some(NoteKind::On { pitch, .. }) => {
                if let Some(on) = held[usize::from(pitch)].take() {
                    close(&mut notes, pitch, on, at);
                }
                held[usize::from(pitch)] = Some((at, msg[2] & 0x7f));
            }
            Some(NoteKind::Off { pitch }) => {
                if let Some(on) = held[usize::from(pitch)].take() {
                    close(&mut notes, pitch, on, at);
                }
            }
            _ => {}
        }
    }
    for (pitch, on) in held.iter().enumerate() {
        if let Some(on) = on {
            close(&mut notes, pitch as u8, *on, end - start);
        }
    }
    notes.sort_by_key(|n| (n["start"].as_i64(), n["pitch"].as_i64()));
    notes
}

/// Stops the recording and puts its takes into the song as one step of the history
/// (`record.take.add`), then brings the engine up to date. The UI's record button calls this
/// on its second press; `wait` is how long the audio side may take to let go (a device or a
/// silent output must be rendering the engine).
pub fn finish_recording(
    session: &mut Session,
    host: &mut SessionHost,
    opts: &RunOptions,
    wait: Duration,
) -> CmdResult<RunResult> {
    let f = host.record_takes(wait)?;
    place_recording(session, host, opts, &f)
}

/// Places closed takes. On failure the caller can keep `f` and retry placement.
pub fn place_recording(
    session: &mut Session,
    host: &mut SessionHost,
    opts: &RunOptions,
    f: &Finished,
) -> CmdResult<RunResult> {
    let summary = f.summary();
    let mut data = f.to_json();
    if f.takes.is_empty() {
        let mut message =
            format!("録音を停止しました（{summary}）。クリップにできるテイクはありません");
        for w in &f.warnings {
            message.push_str(&format!("\n{w}"));
        }
        return Ok(RunResult {
            command: "record.stop".into(),
            message,
            data,
            changes: ChangeSet::default(),
            dry_run: false,
            forced_dry_run: false,
            step: None,
            decision: None,
            events: Vec::new(),
        });
    }
    let mut r = session
        .run("record.take.add", &f.args(), opts)
        .map_err(|mut e| {
            e.message = format!(
                "{}（録音したファイルは残っています: {}）",
                e.message,
                f.files
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            );
            e
        })?;
    host.sync(session.project());
    data["clips"] = r.data["clips"].clone();
    r.command = "record.stop".into();
    r.message = format!("録音を停止しました（{summary}）。{}", r.message);
    for w in &f.warnings {
        r.message.push_str(&format!("\n{w}"));
    }
    r.data = data;
    Ok(r)
}

/// `record.start` or `record.status` on a host, its inputs from
/// [`SessionHost::set_record_sources`].
pub(crate) fn record_on(
    host: &mut SessionHost,
    p: &Project,
    dir: Option<&Path>,
    name: &str,
    args: &Args,
) -> CmdResult<(String, Value)> {
    if name != "record.start" {
        let (mut message, mut data) = host.record_status();
        data["phase"] = json!(match (host.is_recording(), host.pre_count()) {
            (false, _) => "idle",
            (true, Some(_)) => "count-in",
            (true, None) => "recording",
        });
        if let Some(settings) = &host.io_settings {
            let mut settings = settings.lock().unwrap_or_else(|e| e.into_inner());
            settings.reload_if_changed();
            let prefs = &settings.prefs;
            let bars = |on: &str, n: &str| json!({"on": prefs.bool(on), "bars": prefs.number(n)});
            let count_in = prefs.number("audio.count-in");
            data["settings"] = json!({
                "punch": prefs.bool("record.punch"),
                "preroll": bars("record.preroll", "record.preroll-bars"),
                "postroll": bars("record.postroll", "record.postroll-bars"),
                "count_in": {"on": count_in > 0.0, "bars": count_in},
                "input_quantize": {
                    "on": prefs.bool("edit.record.input-quantize"),
                    "value": prefs.text("edit.record.input-quantize-value"),
                },
            });
            let on = |b: bool| if b { "オン" } else { "オフ" };
            message.push_str(&format!(
                "
オートパンチ {}・プリロール {}・ポストロール {}・プリカウント {}・入力クオンタイズ {}",
                on(prefs.bool("record.punch")),
                on(prefs.bool("record.preroll")),
                on(prefs.bool("record.postroll")),
                on(count_in > 0.0),
                on(prefs.bool("edit.record.input-quantize")),
            ));
        }
        return Ok((message, data));
    }
    let at = match args.0.get("at") {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => Some(p.time_map().parse(s)?),
        _ => None,
    };
    if at.is_some_and(|t| t < 0) {
        return Err(CmdError::range("位置は 0 以上で指定してください"));
    }
    let latency = match args.opt_f64("latency") {
        Some(ms) if !(0.0..=2000.0).contains(&ms) => {
            return Err(CmdError::range(
                "--latency は 0〜2000 ms で指定してください",
            ));
        }
        Some(ms) => Some((ms * f64::from(host.rate()) / 1000.0).round() as u64),
        None => None,
    };
    let sources = &mut host.record_sources;
    let input = match &mut sources.audio {
        Source::None => Input::None,
        Source::Device => Input::Device,
        Source::Make(make) => Input::Given(make()),
    };
    let mut midi = sources.midi.as_mut().map(|make| make());
    let mut offsets_ms = [0.0; 2];
    let mut count_in_bars = 0;
    let mut panel = None;
    if let Some(settings) = &host.io_settings {
        let mut settings = settings.lock().unwrap_or_else(|e| e.into_inner());
        settings.reload_if_changed();
        offsets_ms = [
            settings.prefs.number("audio.record-offset"),
            settings.prefs.number("midi.input-offset"),
        ];
        count_in_bars = settings.prefs.number("audio.count-in") as u32;
        panel = Some(crate::punch::Panel::from_prefs(&settings.prefs, p.ppq));
        midi = midi.map(|m| {
            Box::new(
                crate::midi_settings::EnabledInput::new(m, &settings.prefs)
                    .with_assignments(&settings.midimap),
            ) as Box<dyn MidiInput>
        });
    }
    // The record panel's punch, pre-roll, post-roll and input quantize (REC-007, REC-009).
    let (mut at, mut keep, mut stop_at, mut quantize) = (at, None, None, None);
    if let Some(panel) = panel {
        let status = host.status();
        let rate = f64::from(host.rate());
        let here = host.tempo_map().tick_of_frame(status.frame, rate);
        let playing = at.is_none() && status.playing;
        let plan = crate::punch::plan(p, &panel, at.unwrap_or(here), playing, host.looping());
        let moves = plan.keep.is_some();
        if moves {
            (at, keep, stop_at) = (plan.at.or(at), plan.keep, plan.stop_at);
        }
        quantize = plan.quantize;
    }
    let s = host.record_start(
        p,
        dir,
        RecordOptions {
            at,
            latency,
            input,
            midi,
            offsets_ms,
            count_in_bars,
            keep,
            stop_at,
            quantize,
            ..RecordOptions::default()
        },
    )?;
    let name = |id: &TrackId| p.tracks.get(id).map(|t| t.name.clone());
    let what: Vec<String> = (s.audio_tracks.iter().filter_map(name))
        .map(|n| format!("{n}（オーディオ）"))
        .chain(
            s.midi_tracks
                .iter()
                .filter_map(name)
                .map(|n| format!("{n}（MIDI）")),
        )
        .collect();
    let mut message = format!(
        "録音を開始しました: {}（入力 {}・レイテンシー補正 {} フレーム）",
        what.join("・"),
        s.input,
        s.latency
    );
    for w in &s.warnings {
        message.push_str(&format!("\n{w}"));
    }
    Ok((
        message,
        json!({
            "recording": true,
            "audio_track": s.audio.map(|t| t.to_string()),
            "midi_track": s.midi.map(|t| t.to_string()),
            "audio_tracks": s.audio_tracks.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
            "midi_tracks": s.midi_tracks.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
            "latency": s.latency,
            "input": s.input,
            "warnings": s.warnings,
        }),
    ))
}

#[cfg(test)]
mod lap_tests {
    use super::*;

    fn take(track: &str, start: i64, length: i64) -> Value {
        json!({ "track": track, "start": start, "length": length })
    }

    fn muted(takes: &[Value]) -> Vec<Option<bool>> {
        takes.iter().map(|t| t["muted"].as_bool()).collect()
    }

    #[test]
    fn the_last_whole_lap_is_selected_and_a_half_lap_after_it_is_not() {
        let mut t = vec![take("a", 0, 100), take("a", 0, 100), take("a", 0, 40)];
        select_lap(&mut t, 100);
        assert_eq!(muted(&t), [Some(true), Some(false), Some(true)]);
    }

    #[test]
    fn the_last_lap_is_selected_when_it_is_whole() {
        let mut t = vec![take("a", 0, 100), take("a", 0, 99)];
        select_lap(&mut t, 100);
        assert_eq!(muted(&t), [Some(true), Some(false)]);
    }

    #[test]
    fn with_no_whole_lap_the_last_one_is_selected() {
        let mut t = vec![take("a", 0, 30), take("a", 0, 50)];
        select_lap(&mut t, 100);
        assert_eq!(muted(&t), [Some(true), Some(false)]);
    }

    #[test]
    fn each_track_is_judged_on_its_own_and_a_single_take_is_left_alone() {
        let mut t = vec![
            take("a", 0, 100),
            take("b", 0, 100),
            take("a", 0, 100),
            take("a", 0, 10),
        ];
        select_lap(&mut t, 100);
        assert_eq!(muted(&t), [Some(true), None, Some(false), Some(true)]);
    }
}
