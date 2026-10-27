//! Watching the input of the armed audio tracks without recording (REC-002, REC-011, REC-012):
//! the level of every input channel, the trim of each, and the input monitor, the armed track's
//! input heard on the output.
//!
//! [`SessionHost::watch_start`] opens the input (or takes a stand-in) and hands it to the
//! engine; [`SessionHost::watch_sync`] tells the engine, from the song, which channels are
//! trimmed by how much and which are heard. A recording started while the device input is
//! watched uses that same input, so the microphone is opened once and the levels go on through
//! the take. The levels, the trims and the monitor live in the engine's [`InputState`], which the
//! audio thread reads and writes through atomics only.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use xirius_core::model::{Project, Track, TrackKind};
use xirius_core::{CmdError, CmdResult};
use xirius_engine::Command;
use xirius_engine::device::Lost;
use xirius_engine::monitor::InputState;
use xirius_engine::record::{AudioInput, MAX_INPUT_CHANNELS};

use crate::host::SessionHost;
use crate::output::{DeviceInput, input_unavailable, open_input};
use crate::record::{Input, column, input_channels};

pub(crate) struct Opening {
    prefs: xirius_prefs::Prefs,
    work: crate::background::Job<(DeviceInput, Option<String>)>,
}

struct TrackedInput {
    input: Option<Box<dyn AudioInput>>,
    done: Arc<AtomicBool>,
}
impl AudioInput for TrackedInput {
    fn played(&mut self, left: &[f32], right: &[f32]) {
        self.input.as_mut().unwrap().played(left, right);
    }
    fn read(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.input.as_mut().unwrap().read(l, r);
    }
    fn channels(&self) -> usize {
        self.input.as_ref().unwrap().channels()
    }
    fn read_channels(&mut self, out: &mut [f32], frames: usize) {
        self.input.as_mut().unwrap().read_channels(out, frames);
    }
    fn delivered(&self, frames: usize) -> usize {
        self.input.as_ref().unwrap().delivered(frames)
    }
}
impl Drop for TrackedInput {
    fn drop(&mut self) {
        if let Some(input) = self.input.take() {
            input.close();
        }
        self.done.store(true, Ordering::Release);
    }
}

/// The input the engine holds for watching.
pub(crate) struct Watch {
    pub(crate) opened: std::time::Instant,
    pub channels: usize,
    pub name: String,
    /// The device's input latency, once it reports it.
    pub probe: Option<Arc<AtomicU64>>,
    /// Whether it is the device (a stand-in given by a test is not reused by a recording).
    pub device: bool,
    /// Says why the device stopped by itself (`None`: a stand-in given by a test).
    pub lost: Option<Lost>,
}

/// The armed audio tracks of `p`, top to bottom.
fn armed_audio(p: &Project) -> Vec<&Track> {
    column(p)
        .iter()
        .filter_map(|id| p.tracks.get(id))
        .map(Arc::as_ref)
        .filter(|t| t.armed && t.kind == TrackKind::Audio)
        .collect()
}

/// What the armed audio tracks of `p` ask of an input of `channels` channels: the trim of each
/// channel (linear; the first track that names a channel decides) and the channels heard on the
/// left and on the right (bit `c` for channel `c`).
///
/// A track that monitors a single channel is heard in the middle, a pair on its two sides.
pub fn input_plan(p: &Project, channels: usize) -> ([f32; MAX_INPUT_CHANNELS], (u32, u32)) {
    let mut gain = [1.0f32; MAX_INPUT_CHANNELS];
    let mut named = [false; MAX_INPUT_CHANNELS];
    let (mut left, mut right) = (0u32, 0u32);
    for t in armed_audio(p) {
        let Ok(ch) = input_channels(t.input.as_deref(), channels) else {
            continue;
        };
        let g = 10f64.powf(t.input_gain_db / 20.0) as f32;
        for &c in ch.iter().filter(|&&c| c < MAX_INPUT_CHANNELS) {
            if !named[c] {
                named[c] = true;
                gain[c] = g;
            }
        }
        if t.monitor {
            match ch.as_slice() {
                [c] if *c < MAX_INPUT_CHANNELS => {
                    left |= 1 << c;
                    right |= 1 << c;
                }
                [a, b] if *a < MAX_INPUT_CHANNELS && *b < MAX_INPUT_CHANNELS => {
                    left |= 1 << a;
                    right |= 1 << b;
                }
                _ => {}
            }
        }
    }
    (gain, (left, right))
}

impl SessionHost {
    fn cancel_input_open(&mut self) {
        if let Some(opening) = self.input_opening.take() {
            opening.work.cancel();
            self.input_retiring.push(opening.work);
        }
    }
    pub(crate) fn input_retirement_ready(&mut self) -> bool {
        if self.input_close_requested {
            if !self.send(Command::SetInput(None)) {
                return false;
            }
            self.input_close_requested = false;
        }
        self.input_retiring.retain(|job| !job.finished());
        self.input_retiring.is_empty()
            && self
                .input_closed
                .as_ref()
                .is_none_or(|done| done.load(Ordering::Acquire))
    }
    pub(crate) fn close_input(&mut self) {
        self.input_close_requested = true;
        self.input_retirement_ready();
    }

    /// Opens the input device by the `audio.driver` setting: in exclusive mode where the
    /// system has it, and in shared mode, said by [`SessionHost::take_driver_notice`], when the
    /// device will not open that way. A device that will not open is said by
    /// [`input_unavailable`].
    pub(crate) fn open_configured_input(&mut self) -> Result<DeviceInput, String> {
        if self.windowed {
            return Err("入力装置は裏で準備する必要があります".into());
        }
        let (prefs, device) = match &self.io_settings {
            Some(settings) => {
                let settings = settings.lock().unwrap_or_else(|e| e.into_inner());
                // Listed even with one chosen: one unplugged since is chosen again (ENG-061).
                let devices = (self.drivers.devices)();
                let device = crate::audio_settings::configured_device(&settings.prefs, &devices);
                (settings.prefs.clone(), device)
            }
            None => (xirius_prefs::Prefs::default(), String::new()),
        };
        let (input, notice) = open_input(self.rate(), &prefs, device, &self.drivers)
            .map_err(|e| input_unavailable(&e))?;
        self.driver_notice = notice;
        Ok(input)
    }
    fn poll_configured_input(&mut self) -> Result<Option<DeviceInput>, String> {
        let prefs = self
            .io_settings
            .as_ref()
            .map(|settings| {
                settings
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .prefs
                    .clone()
            })
            .unwrap_or_default();
        if self
            .input_opening
            .as_ref()
            .is_some_and(|old| old.prefs != prefs)
        {
            self.cancel_input_open();
            return Ok(None);
        }
        if self.input_opening.is_none() {
            let snapshot = prefs.clone();
            let drivers = self.drivers.clone();
            let rate = self.rate();
            let work = crate::background::Job::spawn("input-open", move || {
                let devices = (drivers.devices)();
                let device = crate::audio_settings::configured_device(&snapshot, &devices);
                open_input(rate, &snapshot, device, &drivers).map_err(|e| input_unavailable(&e))
            })?;
            self.input_opening = Some(Opening { prefs, work });
        }
        let Some(result) = self.input_opening.as_ref().unwrap().work.poll() else {
            return Ok(None);
        };
        self.input_opening = None;
        let (input, notice) = result?;
        self.driver_notice = notice;
        Ok(Some(input))
    }

    /// Nonblocking preflight for REC. The shared input's device readiness is
    /// checked on its worker; latency convergence is polled after attachment.
    pub fn record_input_ready(&mut self) -> Result<bool, String> {
        if self.watch.as_ref().is_some_and(|watch| !watch.device) {
            self.watch_stop();
        }
        self.watch_start(Input::Device).map_err(|e| e.message)?;
        let Some(watch) = &self.watch else {
            return Ok(false);
        };
        Ok(watch.probe.as_ref().is_none_or(|probe| {
            probe.load(std::sync::atomic::Ordering::Relaxed) > 0
                || watch.opened.elapsed() >= std::time::Duration::from_millis(300)
        }))
    }

    pub fn input_latency_ns(&self) -> Option<u64> {
        self.watch
            .as_ref()?
            .probe
            .as_ref()
            .map(|p| p.load(std::sync::atomic::Ordering::Relaxed))
            .filter(|&n| n > 0)
    }
    /// Why the input device the engine holds (watched, or recorded from) stopped by itself, once
    /// it has: what it no longer delivers is silence, so the screen says so and lets it go.
    pub fn input_lost(&self) -> Option<String> {
        match &self.recording {
            Some(a) => a.input_lost(),
            None => self.watch.as_ref()?.lost.as_ref()?.get(),
        }
    }

    /// Whether the input is open for watching (a recording that took it over does not count).
    pub fn is_watching(&self) -> bool {
        self.watch.is_some()
    }

    /// How many channels the watched input has (0 when none is).
    pub fn watch_channels(&self) -> usize {
        self.watch.as_ref().map_or(0, |w| w.channels)
    }

    /// Opens the input and gives it to the engine; its levels are measured from now on. A
    /// recording under way, or an input already watched, is left as it is. Gives the number of
    /// channels.
    pub fn watch_start(&mut self, input: Input) -> CmdResult<usize> {
        if self.recording.is_some() || self.watch.is_some() {
            return Ok(self.watch_channels());
        }
        if self.windowed && !self.input_retirement_ready() {
            return Ok(0);
        }
        let (input, name, probe, lost): (Box<dyn AudioInput>, _, _, _) = match input {
            Input::None => {
                return Err(CmdError::conflict("入力がありません".to_string()));
            }
            Input::Given(i) => (i, "指定された入力".to_string(), None, None),
            Input::Device => {
                let c = if self.windowed {
                    let Some(c) = self.poll_configured_input().map_err(CmdError::conflict)? else {
                        return Ok(0);
                    };
                    c
                } else {
                    self.open_configured_input().map_err(CmdError::conflict)?
                };
                let (name, probe, lost) = (c.name.clone(), c.latency_probe(), c.lost());
                (Box::new(c), name, Some(probe), Some(lost))
            }
        };
        let channels = input.channels().clamp(1, MAX_INPUT_CHANNELS);
        let input = if self.windowed {
            let done = Arc::new(AtomicBool::new(false));
            self.input_closed = Some(done.clone());
            Box::new(TrackedInput {
                input: Some(input),
                done,
            }) as Box<dyn AudioInput>
        } else {
            input
        };
        if !self.send(Command::SetInput(Some(input))) {
            return Err(CmdError::conflict(
                "オーディオエンジンが処理中です。しばらく待ってからやり直してください".to_string(),
            ));
        }
        self.watch = Some(Watch {
            opened: std::time::Instant::now(),
            channels,
            name,
            probe,
            device: lost.is_some(),
            lost,
        });
        Ok(channels)
    }

    /// Lets the input go (a microphone is released). A recording under way keeps its input.
    pub fn watch_stop(&mut self) {
        self.cancel_input_open();
        if self.recording.is_some() {
            return;
        }
        if self.watch.take().is_some() {
            self.close_input();
            self.input_state().set_monitor(0, 0);
        }
    }

    /// The levels, trims and monitor of the engine's input.
    pub fn input_state(&mut self) -> Arc<InputState> {
        self.controller().input_state()
    }

    /// Tells the engine the trims and the monitor the armed tracks of `p` ask for. Cheap: a few
    /// atomic stores, so the screen may call it after every change of the song.
    pub fn watch_sync(&mut self, p: &Project) {
        let channels = self
            .watch
            .as_ref()
            .map_or(MAX_INPUT_CHANNELS, |w| w.channels);
        self.apply_input_plan(p, channels);
    }

    pub(crate) fn apply_input_plan(&mut self, p: &Project, channels: usize) {
        let (gain, (left, right)) = input_plan(p, channels);
        let state = self.input_state();
        for (c, g) in gain.iter().enumerate() {
            state.set_gain(c, *g);
        }
        state.set_monitor(left, right);
    }

    /// The loudest level of every input channel since the last call (linear, 0 when silent).
    pub fn input_levels(&mut self) -> [f32; MAX_INPUT_CHANNELS] {
        let state = self.input_state();
        std::array::from_fn(|c| state.take_peak(c))
    }
}
