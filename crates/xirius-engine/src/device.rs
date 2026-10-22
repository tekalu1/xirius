//! Shared-mode output through cpal (WASAPI on Windows).
//!
//! cpal's WASAPI backend calls back with whatever the device buffer has room for, so the size
//! varies between callbacks; the engine chops it into blocks of at most `max_block` (ENG-023).

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::engine::AudioEngine;
use crate::rt::ProAudioThread;
use crate::stats::{Stats, Steady};

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
}

#[derive(Debug)]
pub struct DeviceError(pub String);

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DeviceError {}

fn err(context: &str, e: impl std::fmt::Display) -> DeviceError {
    DeviceError(format!("{context}: {e}"))
}

/// What an error says when another program holds the device (`AUDCLNT_E_DEVICE_IN_USE`).
pub const IN_USE: &str = "the device is in use by another program";

/// Whether `error` says another program holds the device.
pub fn in_use(error: &str) -> bool {
    error.contains(IN_USE)
}

/// A cpal error as an error of this crate, saying [`IN_USE`] when another program holds the
/// device.
fn cpal_err(context: &str, e: cpal::Error) -> DeviceError {
    match e.kind() {
        cpal::ErrorKind::DeviceBusy => err(context, IN_USE),
        _ => err(context, e),
    }
}

/// Why a running device stream ended by itself, once it has: the device went away, another
/// program took it, or the stream stopped. Shared between the stream and whoever plays on it.
#[derive(Clone, Debug, Default)]
pub struct Lost(Arc<Mutex<Option<String>>>);

impl Lost {
    /// The stream ended because of `why`. The first reason stays.
    pub fn set(&self, why: String) {
        let mut at = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if at.is_none() {
            *at = Some(why);
        }
    }

    /// Why the stream ended, once it has.
    pub fn get(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// Whether a cpal stream error ends the sound. Underruns, a refused real-time priority and a
/// route the stream followed do not.
pub(crate) fn ends_stream(kind: cpal::ErrorKind) -> bool {
    !matches!(
        kind,
        cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied | cpal::ErrorKind::DeviceChanged
    )
}

/// What a stream error that ended the stream says, [`IN_USE`] when another program took the
/// device.
pub(crate) fn why_ended(e: &cpal::Error) -> String {
    match e.kind() {
        cpal::ErrorKind::DeviceBusy => IN_USE.to_string(),
        _ => e.to_string(),
    }
}

fn default_device() -> Result<cpal::Device, DeviceError> {
    cpal::default_host()
        .default_output_device()
        .ok_or_else(|| DeviceError("no default output device".into()))
}

/// The default output device and its shared-mode mix format.
pub fn default_output() -> Result<DeviceInfo, DeviceError> {
    let device = default_device()?;
    output_info(&device)
}

fn output_info(device: &cpal::Device) -> Result<DeviceInfo, DeviceError> {
    let config = device
        .default_output_config()
        .map_err(|e| err("default output config", e))?;
    Ok(DeviceInfo {
        name: device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "(unknown)".into()),
        sample_rate: config.sample_rate(),
        channels: config.channels(),
        sample_format: format!("{:?}", config.sample_format()),
    })
}

/// The engine inside a stream's callback. The callback is dropped on the device's thread, so
/// this sends the engine back to be dropped by whoever stops the stream: a plugin made in the
/// process is released on the thread that made it, not on the device's (ARC-044).
struct Homeward {
    audio: Option<AudioEngine>,
    home: mpsc::Sender<AudioEngine>,
}

impl Drop for Homeward {
    fn drop(&mut self) {
        if let Some(audio) = self.audio.take() {
            let _ = self.home.send(audio);
        }
    }
}

/// How long a stopped stream is waited for to give its engine back. The stream has gone by
/// then, so it is there at once; the wait is for a backend that drops its callback later.
const HOME_WAIT: Duration = Duration::from_secs(2);

/// A running shared-mode output stream. Dropping it stops the sound, and the engine is dropped
/// on the thread that drops it.
pub struct CpalOutput {
    stream: Option<cpal::Stream>,
    back: mpsc::Receiver<AudioEngine>,
    stats: Arc<Stats>,
    pub info: DeviceInfo,
    /// The buffer size asked for, in frames; `None` means the device default.
    pub requested_frames: Option<u32>,
    lost: Lost,
}

impl CpalOutput {
    /// Starts the default device in f32 at the engine's sample rate. The callback joins MMCSS
    /// "Pro Audio" (ENG-007), times every call and counts drained-buffer callbacks.
    pub fn start(
        audio: AudioEngine,
        buffer_frames: Option<u32>,
    ) -> Result<CpalOutput, DeviceError> {
        Self::start_named(audio, "", buffer_frames)
    }

    pub fn start_named(
        audio: AudioEngine,
        name: &str,
        buffer_frames: Option<u32>,
    ) -> Result<CpalOutput, DeviceError> {
        Self::start_keeping(audio, name, buffer_frames).map_err(|(e, _)| e)
    }

    /// Like [`CpalOutput::start_named`], but a stream that does not start gives the engine back
    /// (`None` when it was lost with the callback).
    pub fn start_keeping(
        audio: AudioEngine,
        name: &str,
        buffer_frames: Option<u32>,
    ) -> Result<CpalOutput, (DeviceError, Option<Box<AudioEngine>>)> {
        let device = match crate::devices::endpoint(name, false) {
            Ok(d) => d,
            Err(e) => return Err((DeviceError(e), Some(Box::new(audio)))),
        };
        let mut info = match output_info(&device) {
            Ok(i) => i,
            Err(e) => return Err((e, Some(Box::new(audio)))),
        };
        if info.sample_format != "F32" {
            let e = DeviceError(format!(
                "the device mix format is {}, only F32 is handled",
                info.sample_format
            ));
            return Err((e, Some(Box::new(audio))));
        }
        let sample_rate = audio.setup().sample_rate as u32;
        info.sample_rate = sample_rate;
        let channels = info.channels as usize;
        let config = cpal::StreamConfig {
            channels: info.channels,
            sample_rate,
            buffer_size: match buffer_frames {
                Some(n) => cpal::BufferSize::Fixed(n),
                None => cpal::BufferSize::Default,
            },
        };
        let stats = audio.stats();
        let stats_cb = stats.clone();
        let stats_err = stats.clone();
        let lost = Lost::default();
        let lost_err = lost.clone();
        let mut steady = Steady::new();
        let mut mmcss: Option<Option<ProAudioThread>> = None;
        // The first callback is asked to fill the whole device buffer. Later ones only see the
        // part played since the last call, so one that sees the whole buffer again came too late
        // and the device ran dry.
        let mut whole_buffer = 0usize;
        let (home, back) = mpsc::channel();
        let mut homeward = Homeward {
            audio: Some(audio),
            home,
        };
        let built = device
            .build_output_stream::<f32, _, _>(
                config,
                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    if mmcss.is_none() {
                        mmcss = Some(ProAudioThread::join());
                    }
                    let frames = data.len() / channels;
                    if whole_buffer == 0 {
                        whole_buffer = frames;
                    } else if frames >= whole_buffer {
                        stats_cb.record_starved();
                    }
                    // The first frame written is heard after what is still queued; the last after
                    // this block too. Their sum is about the whole device buffer, whatever share
                    // of it this callback was asked for.
                    let ts = info.timestamp();
                    if let Some(ahead) = ts.playback.checked_duration_since(ts.callback) {
                        let block = frames as u64 * 1_000_000_000 / u64::from(sample_rate);
                        let ns = ahead.as_nanos().min(u128::from(u64::MAX)) as u64;
                        let steady = steady.push(ns.saturating_add(block));
                        stats_cb.record_output_latency(Duration::from_nanos(steady));
                    }
                    if let Some(audio) = homeward.audio.as_mut() {
                        audio.render_timed(data, channels);
                    }
                },
                move |e: cpal::Error| {
                    if e.kind() == cpal::ErrorKind::Xrun {
                        stats_err.record_xrun();
                    } else {
                        stats_err.record_device_error();
                    }
                    if ends_stream(e.kind()) {
                        lost_err.set(why_ended(&e));
                    }
                },
                None,
            )
            .map_err(|e| cpal_err("build output stream", e))
            .and_then(|stream| {
                stream
                    .play()
                    .map_err(|e| cpal_err("start output stream", e))
                    .map(|()| stream)
            });
        // A stream that did not start has dropped its callback, and the engine came back.
        let stream = match built {
            Ok(s) => s,
            Err(e) => return Err((e, back.recv_timeout(HOME_WAIT).ok().map(Box::new))),
        };
        Ok(CpalOutput {
            stream: Some(stream),
            back,
            stats,
            info,
            requested_frames: buffer_frames,
            lost,
        })
    }

    /// Says why the stream ended by itself, once it has.
    pub fn lost(&self) -> Lost {
        self.lost.clone()
    }

    pub fn stats(&self) -> &Arc<Stats> {
        &self.stats
    }

    /// Stops the sound and gives the engine back (`None` when it was lost with the callback).
    pub fn stop(mut self) -> Option<AudioEngine> {
        if let Some(s) = self.stream.take() {
            let _ = s.pause();
        }
        self.back.recv_timeout(HOME_WAIT).ok()
    }
}

impl Drop for CpalOutput {
    fn drop(&mut self) {
        drop(self.stream.take());
        drop(self.back.recv_timeout(HOME_WAIT));
    }
}
