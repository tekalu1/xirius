//! WASAPI exclusive-mode output through the `wasapi` crate (ENG-003, ENG-026, ENG-027).
//!
//! cpal only opens WASAPI in shared mode, where the period is fixed by the Windows audio engine
//! (typically 10 ms or more). Exclusive mode talks to the driver directly and lets the period
//! go down to the device minimum. The loop is event driven: each event asks for one period,
//! which is rendered, converted to the device format and written.
//!
//! The input side ([`ExclusiveInput`]) is the same loop the other way: each event delivers one
//! period of captured frames, which are decoded to f32 and pushed into the ring the audio thread
//! reads, as [`CpalInput`] does for the shared input.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use rtrb::{Producer, RingBuffer};
use wasapi::{
    AudioClient, Device, DeviceEnumerator, Direction, Handle, SampleType, StreamMode, WasapiError,
    WaveFormat, calculate_period_100ns,
};

use crate::device::{DeviceError, IN_USE, Lost};
use crate::engine::AudioEngine;
use crate::record::{AudioInput, CpalInput, NO_INPUT};
use crate::rt::ProAudioThread;
use crate::stats::Stats;

/// `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED`.
const BUFFER_SIZE_NOT_ALIGNED: i32 = 0x8889_0019_u32 as i32;
/// `AUDCLNT_E_DEVICE_IN_USE`: another program holds the device.
const DEVICE_IN_USE: i32 = 0x8889_000A_u32 as i32;
/// How long the device has to ask for the next period before it is taken as stopped.
const EVENT_WAIT_MS: u32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    F32,
    /// 32-bit container; `valid` bits are meaningful.
    I32,
    I24,
    I16,
}

#[derive(Clone, Debug)]
pub struct ExclusiveInfo {
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub format: String,
    /// The period asked of the driver after alignment, in frames.
    pub requested_period_frames: u32,
    /// Frames per event (and per write), as the driver set it.
    pub period_frames: u32,
    pub min_period_frames: u32,
    pub default_period_frames: u32,
}

/// How the output's thread ended, and the engine it played, to be dropped by whoever stops the
/// output: a plugin made in the process is released on the thread that made it, not on this one
/// (ARC-044). No engine when a failed setup sent it back.
#[doc(hidden)]
pub type Ended = (Result<(), String>, Option<AudioEngine>);

pub struct ExclusiveOutput {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Ended>>,
    stats: Arc<Stats>,
    lost: Lost,
    pub info: ExclusiveInfo,
}

/// Why a device could not be opened exclusively, and the engine that was to play on it (`None`
/// when it was lost with the thread).
pub struct NotOpened {
    pub error: DeviceError,
    pub audio: Option<Box<AudioEngine>>,
}

fn werr(context: &str, e: WasapiError) -> String {
    match &e {
        WasapiError::Windows(w) if w.code().0 == DEVICE_IN_USE => format!("{context}: {IN_USE}"),
        _ => format!("{context}: {e}"),
    }
}

/// The WASAPI endpoint id of the device called `name` (empty: the default one).
fn endpoint_id(name: &str, input: bool) -> Result<String, DeviceError> {
    use cpal::traits::DeviceTrait;
    Ok(crate::devices::endpoint(name, input)
        .map_err(DeviceError)?
        .id()
        .map_err(|e| DeviceError(e.to_string()))?
        .id()
        .to_string())
}

fn frames_of(hns: i64, rate: u32) -> u32 {
    ((hns as f64) * rate as f64 / 10_000_000.0).round() as u32
}

impl ExclusiveOutput {
    /// Opens the default render device exclusively at the engine's sample rate and starts the
    /// audio thread. `period_frames` of `None` picks the device minimum.
    pub fn start(
        audio: AudioEngine,
        period_frames: Option<u32>,
    ) -> Result<ExclusiveOutput, DeviceError> {
        Self::start_named(audio, "", period_frames)
    }
    pub fn start_named(
        audio: AudioEngine,
        name: &str,
        period_frames: Option<u32>,
    ) -> Result<ExclusiveOutput, DeviceError> {
        Self::start_keeping(audio, name, period_frames).map_err(|n| n.error)
    }

    /// Like [`ExclusiveOutput::start_named`], but a device that cannot be opened exclusively
    /// gives the engine back, so it can be opened in shared mode instead.
    pub fn start_keeping(
        audio: AudioEngine,
        name: &str,
        period_frames: Option<u32>,
    ) -> Result<ExclusiveOutput, NotOpened> {
        let name = match endpoint_id(name, false) {
            Ok(id) => id,
            Err(error) => {
                return Err(NotOpened {
                    error,
                    audio: Some(Box::new(audio)),
                });
            }
        };
        Self::start_with(audio, move |audio, stop, ready| {
            run(audio, period_frames, &name, &stop, ready)
        })
    }

    /// The same thread arrangement as [`ExclusiveOutput::start_keeping`], with `run` in place
    /// of the WASAPI loop: a stand-in opens no device (ARC-044's device path tests).
    #[doc(hidden)]
    pub fn start_with(
        audio: AudioEngine,
        run: impl FnOnce(AudioEngine, Arc<AtomicBool>, mpsc::Sender<Ready>) -> Ended + Send + 'static,
    ) -> Result<ExclusiveOutput, NotOpened> {
        let stop = Arc::new(AtomicBool::new(false));
        let stats = audio.stats();
        let lost = Lost::default();
        let (tx, rx) = mpsc::channel();
        let (stop2, lost2) = (stop.clone(), lost.clone());
        // The engine goes to the thread; a failed setup sends it back through `tx`.
        let thread = std::thread::Builder::new()
            .name("xirius-wasapi-exclusive".into())
            .spawn(move || {
                let ended = run(audio, stop2, tx);
                if let Err(e) = &ended.0 {
                    lost2.set(e.clone());
                }
                ended
            })
            .map_err(|e| NotOpened {
                error: DeviceError(format!("spawn: {e}")),
                audio: None,
            })?;
        match rx.recv() {
            Ok(Ok(info)) => Ok(ExclusiveOutput {
                stop,
                thread: Some(thread),
                stats,
                lost,
                info,
            }),
            Ok(Err((e, audio))) => {
                let _ = thread.join();
                Err(NotOpened {
                    error: DeviceError(e),
                    audio: Some(audio),
                })
            }
            Err(_) => {
                let e = match thread.join() {
                    Ok((Err(e), _)) => e,
                    _ => "the exclusive-mode thread ended early".into(),
                };
                Err(NotOpened {
                    error: DeviceError(e),
                    audio: None,
                })
            }
        }
    }

    pub fn stats(&self) -> &Arc<Stats> {
        &self.stats
    }

    /// Says why the stream ended by itself (the device stopped asking for periods, a write
    /// failed), once it has.
    pub fn lost(&self) -> Lost {
        self.lost.clone()
    }

    /// Stops the stream and returns the error that ended it early, if any.
    pub fn stop(mut self) -> Result<(), DeviceError> {
        self.stop.store(true, Ordering::Release);
        match self.thread.take().map(JoinHandle::join) {
            Some(Ok((Err(e), _))) => Err(DeviceError(e)),
            Some(Err(_)) => Err(DeviceError("the exclusive-mode thread panicked".into())),
            _ => Ok(()),
        }
    }
}

impl Drop for ExclusiveOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Formats to try, best first. The engine renders f32, so float is preferred.
fn candidates(rate: usize, channels: usize) -> [(WaveFormat, Encoding, &'static str); 5] {
    [
        (
            WaveFormat::new(32, 32, &SampleType::Float, rate, channels, None),
            Encoding::F32,
            "f32",
        ),
        (
            WaveFormat::new(32, 32, &SampleType::Int, rate, channels, None),
            Encoding::I32,
            "i32",
        ),
        (
            WaveFormat::new(32, 24, &SampleType::Int, rate, channels, None),
            Encoding::I32,
            "i24 in 32",
        ),
        (
            WaveFormat::new(24, 24, &SampleType::Int, rate, channels, None),
            Encoding::I24,
            "i24 packed",
        ),
        (
            WaveFormat::new(16, 16, &SampleType::Int, rate, channels, None),
            Encoding::I16,
            "i16",
        ),
    ]
}

fn encode(src: &[f32], dst: &mut [u8], enc: Encoding) {
    match enc {
        Encoding::F32 => {
            for (s, d) in src.iter().zip(dst.as_chunks_mut::<4>().0) {
                d.copy_from_slice(&s.to_le_bytes());
            }
        }
        Encoding::I32 => {
            for (s, d) in src.iter().zip(dst.as_chunks_mut::<4>().0) {
                let v = (s.clamp(-1.0, 1.0) as f64 * 2_147_483_647.0) as i32;
                d.copy_from_slice(&v.to_le_bytes());
            }
        }
        Encoding::I24 => {
            for (s, d) in src.iter().zip(dst.as_chunks_mut::<3>().0) {
                let v = (s.clamp(-1.0, 1.0) * 8_388_607.0) as i32;
                d.copy_from_slice(&v.to_le_bytes()[..3]);
            }
        }
        Encoding::I16 => {
            for (s, d) in src.iter().zip(dst.as_chunks_mut::<2>().0) {
                let v = (s.clamp(-1.0, 1.0) * 32_767.0) as i16;
                d.copy_from_slice(&v.to_le_bytes());
            }
        }
    }
}

/// Samples of `enc` in `src` as f32 in `dst`: the inverse of [`encode`].
fn decode(src: &[u8], dst: &mut [f32], enc: Encoding) {
    match enc {
        Encoding::F32 => {
            for (s, d) in src.as_chunks::<4>().0.iter().zip(dst) {
                *d = f32::from_le_bytes(*s);
            }
        }
        Encoding::I32 => {
            for (s, d) in src.as_chunks::<4>().0.iter().zip(dst) {
                *d = (i32::from_le_bytes(*s) as f64 / 2_147_483_648.0) as f32;
            }
        }
        Encoding::I24 => {
            for (s, d) in src.as_chunks::<3>().0.iter().zip(dst) {
                // Sign-extend the 24 bits through the top of a 32-bit integer.
                let v = i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8;
                *d = v as f32 / 8_388_608.0;
            }
        }
        Encoding::I16 => {
            for (s, d) in src.as_chunks::<2>().0.iter().zip(dst) {
                *d = i16::from_le_bytes(*s) as f32 / 32_768.0;
            }
        }
    }
}

/// A device opened exclusively in one direction, ready to `start_stream`.
struct Opened {
    client: AudioClient,
    event: Handle,
    format: WaveFormat,
    enc: Encoding,
    label: &'static str,
    channels: usize,
    period_hns: i64,
    min_hns: i64,
    def_hns: i64,
}

/// Opens `device` exclusively for `direction` at `rate`, in the best format it takes, with
/// events and a period of about `period_frames` (the device minimum when `None`).
fn open(
    device: &Device,
    direction: &Direction,
    rate: u32,
    period_frames: Option<u32>,
) -> Result<Opened, String> {
    let mut client = device
        .get_iaudioclient()
        .map_err(|e| werr("audio client", e))?;
    let mix = client.get_mixformat().map_err(|e| werr("mix format", e))?;
    let channels = mix.get_nchannels() as usize;
    let (format, enc, label) = candidates(rate as usize, channels)
        .into_iter()
        .find_map(|(f, enc, label)| {
            client
                .is_supported_exclusive_with_quirks(&f)
                .ok()
                .map(|f| (f, enc, label))
        })
        .ok_or_else(|| format!("no exclusive format at {rate} Hz, {channels} ch"))?;
    let (def_hns, min_hns) = client
        .get_device_period()
        .map_err(|e| werr("device period", e))?;
    let want_hns = match period_frames {
        Some(n) => calculate_period_100ns(n as i64, rate as i64).max(min_hns),
        None => min_hns,
    };
    let period_hns = client
        .calculate_aligned_period_near(want_hns, Some(128), &format)
        .map_err(|e| werr("aligned period", e))?;
    let mode = StreamMode::EventsExclusive { period_hns };
    if let Err(e) = client.initialize_client(&format, direction, &mode) {
        let aligned =
            matches!(&e, WasapiError::Windows(w) if w.code().0 == BUFFER_SIZE_NOT_ALIGNED);
        if !aligned {
            return Err(werr("initialize exclusive", e));
        }
        // Retry with the size the driver rounded to, as the WASAPI documentation describes.
        let frames = client
            .get_buffer_size()
            .map_err(|e| werr("buffer size", e))?;
        let hns = calculate_period_100ns(frames as i64, rate as i64);
        client = device
            .get_iaudioclient()
            .map_err(|e| werr("audio client", e))?;
        client
            .initialize_client(
                &format,
                direction,
                &StreamMode::EventsExclusive { period_hns: hns },
            )
            .map_err(|e| werr("initialize exclusive (aligned)", e))?;
    }
    let event = client
        .set_get_eventhandle()
        .map_err(|e| werr("event handle", e))?;
    Ok(Opened {
        client,
        event,
        format,
        enc,
        label,
        channels,
        period_hns,
        min_hns,
        def_hns,
    })
}

fn format_text(o: &Opened) -> String {
    format!(
        "{} ({} bits in {})",
        o.label,
        o.format.get_validbitspersample(),
        o.format.get_bitspersample()
    )
}

/// The answer the output's thread sends once the device is asking for periods (or failed to
/// open, with the engine back). `doc(hidden)`: a test's stand-in loop answers it.
#[doc(hidden)]
pub type Ready = Result<ExclusiveInfo, (String, Box<AudioEngine>)>;

/// What a device that stopped asking for periods is said to have done.
fn no_event() -> &'static str {
    "no event from the device for 1 s"
}

fn run(
    mut audio: AudioEngine,
    period_frames: Option<u32>,
    name: &str,
    stop: &AtomicBool,
    ready: mpsc::Sender<Ready>,
) -> Ended {
    let setup = (|| -> Result<_, String> {
        let _ = wasapi::initialize_mta();
        let enumerator = DeviceEnumerator::new().map_err(|e| werr("enumerator", e))?;
        let device = enumerator
            .get_device(name)
            .map_err(|e| werr("output device", e))?;
        let name = device.get_friendlyname().unwrap_or_default();
        let rate = audio.setup().sample_rate as u32;
        let o = open(&device, &Direction::Render, rate, period_frames)?;
        let render = o
            .client
            .get_audiorenderclient()
            .map_err(|e| werr("render client", e))?;
        let period = o
            .client
            .get_buffer_size()
            .map_err(|e| werr("buffer size", e))?;
        let info = ExclusiveInfo {
            device: name,
            sample_rate: rate,
            channels: o.channels as u16,
            format: format_text(&o),
            requested_period_frames: frames_of(o.period_hns, rate),
            period_frames: period,
            min_period_frames: frames_of(o.min_hns, rate),
            default_period_frames: frames_of(o.def_hns, rate),
        };
        Ok((o, render, info))
    })();
    let (o, render, info) = match setup {
        Ok(s) => s,
        Err(e) => {
            let _ = ready.send(Err((e.clone(), Box::new(audio))));
            return (Err(e), None);
        }
    };
    let Opened {
        client,
        event,
        enc,
        format,
        ..
    } = o;
    let block_align = format.get_blockalign() as usize;
    let channels = info.channels as usize;
    let period = info.period_frames as usize;

    let mut samples = vec![0.0f32; period * channels];
    let mut bytes = vec![0u8; period * block_align];
    let mut write = |audio: &mut AudioEngine| -> Result<(), String> {
        audio.render_timed(&mut samples, channels);
        encode(&samples, &mut bytes, enc);
        render
            .write_to_device(period, &bytes, None)
            .map_err(|e| werr("write", e))
    };
    // The period alone does not report the driver's safety offset; leave latency unknown.
    // The device is open once it has asked for a period: one that initializes but never asks
    // (another program has the hardware) is not opened, and the engine goes back.
    let started = write(&mut audio)
        .and_then(|()| client.start_stream().map_err(|e| werr("start", e)))
        .and_then(|()| {
            event
                .wait_for_event(EVENT_WAIT_MS)
                .map_err(|_| no_event().to_string())
        })
        .and_then(|()| write(&mut audio));
    if let Err(e) = started {
        let _ = client.stop_stream();
        let _ = ready.send(Err((e.clone(), Box::new(audio))));
        return (Err(e), None);
    }
    let _ = ready.send(Ok(info));
    let _mmcss = ProAudioThread::join();
    let result = loop {
        if stop.load(Ordering::Acquire) {
            break Ok(());
        }
        if event.wait_for_event(EVENT_WAIT_MS).is_err() {
            break Err(no_event().to_string());
        }
        if let Err(e) = write(&mut audio) {
            break Err(e);
        }
    };
    let _ = client.stop_stream();
    (result, Some(audio))
}

// ---- the input ---------------------------------------------------------------------------------

/// A capture device opened exclusively, on a thread of its own. It reads like [`CpalInput`]: the
/// audio thread takes what the ring holds, and frames that did not arrive are silence.
pub struct ExclusiveInput {
    input: CpalInput,
    thread: Option<JoinHandle<Result<(), String>>>,
    pub info: ExclusiveInfo,
}

impl ExclusiveInput {
    /// Opens the default capture device at `sample_rate` (the engine's).
    pub fn open(sample_rate: u32, period_frames: Option<u32>) -> Result<ExclusiveInput, String> {
        Self::open_named(sample_rate, "", period_frames)
    }

    pub fn open_named(
        sample_rate: u32,
        name: &str,
        period_frames: Option<u32>,
    ) -> Result<ExclusiveInput, String> {
        let id = endpoint_id(name, true).map_err(|e| e.0)?;
        let stop = Arc::new(AtomicBool::new(false));
        let latency_ns = Arc::new(AtomicU64::new(0));
        let lost = Lost::default();
        type Started = Result<(ExclusiveInfo, rtrb::Consumer<f32>), String>;
        let (tx, rx) = mpsc::channel::<Started>();
        let (stop2, latency2, lost2) = (stop.clone(), latency_ns.clone(), lost.clone());
        let thread = std::thread::Builder::new()
            .name("xirius-wasapi-exclusive-in".into())
            .spawn(move || {
                let ran = capture(sample_rate, period_frames, &id, &stop2, &latency2, tx);
                if let Err(e) = &ran {
                    lost2.set(e.clone());
                }
                ran
            })
            .map_err(|e| format!("入力のスレッドを作成できません: {e}"))?;
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok((info, ring))) => Ok(ExclusiveInput {
                input: CpalInput::from_parts(
                    ring,
                    stop,
                    info.device.clone(),
                    info.channels,
                    latency_ns,
                    lost,
                ),
                thread: Some(thread),
                info,
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                // Opening is worker-owned. Do not start a shared fallback or
                // report retirement until this owner actually releases WASAPI.
                let _ = thread.join();
                Err("入力デバイスが応答しません".into())
            }
        }
    }

    /// A handle that reads the capture latency (one period, in ns) once the stream runs.
    pub fn latency_probe(&self) -> Arc<AtomicU64> {
        self.input.latency_probe()
    }

    /// Says why the device stopped by itself, once it has.
    pub fn lost(&self) -> Lost {
        self.input.lost()
    }
}

impl AudioInput for ExclusiveInput {
    fn read(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.input.read(left, right);
    }
    fn channels(&self) -> usize {
        AudioInput::channels(&self.input)
    }
    fn read_channels(&mut self, buf: &mut [f32], frames: usize) {
        self.input.read_channels(buf, frames);
    }
    fn delivered(&self, frames: usize) -> usize {
        self.input.delivered(frames)
    }
}

impl Drop for ExclusiveInput {
    fn drop(&mut self) {
        // The stop flag is the input's, which sets it as it drops after this.
        self.input.stop_now();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Pushes the frames of `bytes` (`channels` per frame, as `enc`) into `ring`; a frame that does
/// not fit whole is dropped. Gives the number of frames pushed.
fn push_frames(
    bytes: &[u8],
    enc: Encoding,
    channels: usize,
    silent: bool,
    scratch: &mut [f32],
    ring: &mut Producer<f32>,
) -> usize {
    let frames = scratch.len() / channels;
    let width = match enc {
        Encoding::F32 | Encoding::I32 => 4,
        Encoding::I24 => 3,
        Encoding::I16 => 2,
    };
    let frames = frames.min(bytes.len() / (width * channels));
    let used = &mut scratch[..frames * channels];
    if silent {
        used.fill(0.0);
    } else {
        decode(bytes, used, enc);
    }
    let mut pushed = 0;
    for frame in used.chunks_exact(channels) {
        if ring.slots() < channels {
            break;
        }
        for s in frame {
            let _ = ring.push(*s);
        }
        pushed += 1;
    }
    pushed
}

fn capture(
    rate: u32,
    period_frames: Option<u32>,
    id: &str,
    stop: &AtomicBool,
    latency_ns: &AtomicU64,
    ready: mpsc::Sender<Result<(ExclusiveInfo, rtrb::Consumer<f32>), String>>,
) -> Result<(), String> {
    let setup = (|| -> Result<_, String> {
        let _ = wasapi::initialize_mta();
        let enumerator = DeviceEnumerator::new().map_err(|e| werr("enumerator", e))?;
        let device = enumerator
            .get_device(id)
            .map_err(|e| werr("input device", e))?;
        let name = device.get_friendlyname().unwrap_or_default();
        let o = open(&device, &Direction::Capture, rate, period_frames)?;
        let capture = o
            .client
            .get_audiocaptureclient()
            .map_err(|e| werr("capture client", e))?;
        let period = o
            .client
            .get_buffer_size()
            .map_err(|e| werr("buffer size", e))?;
        let info = ExclusiveInfo {
            device: name,
            sample_rate: rate,
            channels: o.channels as u16,
            format: format_text(&o),
            requested_period_frames: frames_of(o.period_hns, rate),
            period_frames: period,
            min_period_frames: frames_of(o.min_hns, rate),
            default_period_frames: frames_of(o.def_hns, rate),
        };
        Ok((o, capture, info))
    })();
    let (o, capture, info) = match setup {
        Ok(s) => s,
        Err(e) => {
            let _ = ready.send(Err(e.clone()));
            return Err(e);
        }
    };
    let Opened {
        client,
        event,
        enc,
        format,
        ..
    } = o;
    let block_align = format.get_blockalign() as usize;
    let channels = info.channels as usize;
    let period = info.period_frames as usize;
    latency_ns.store(
        (period as u64 * 1_000_000_000) / u64::from(rate.max(1)),
        Ordering::Relaxed,
    );
    let (mut tx, rx) = RingBuffer::<f32>::new(rate as usize * channels.max(1));
    // The device is open once it has delivered a period: one that initializes but never does
    // (another program has the hardware) is not.
    let started = client
        .start_stream()
        .map_err(|e| werr("start", e))
        .and_then(|()| {
            event
                .wait_for_event(EVENT_WAIT_MS)
                .map_err(|_| NO_INPUT.to_string())
        });
    if let Err(e) = started {
        let _ = client.stop_stream();
        let _ = ready.send(Err(e.clone()));
        return Err(e);
    }
    let _ = ready.send(Ok((info, rx)));

    let mut bytes = vec![0u8; period * block_align];
    let mut scratch = vec![0.0f32; period * channels];
    let _mmcss = ProAudioThread::join();
    // The event that said the device started is the first period's.
    let mut due = true;
    let result = loop {
        if stop.load(Ordering::Acquire) {
            break Ok(());
        }
        if !std::mem::take(&mut due) && event.wait_for_event(EVENT_WAIT_MS).is_err() {
            break Err(NO_INPUT.to_string());
        }
        match capture.read_from_device(&mut bytes) {
            Ok((0, _)) => {}
            Ok((n, buffer)) => {
                let used = n as usize * block_align;
                push_frames(
                    &bytes[..used],
                    enc,
                    channels,
                    buffer.flags.silent,
                    &mut scratch,
                    &mut tx,
                );
            }
            Err(e) => break Err(werr("read", e)),
        }
    };
    let _ = client.stop_stream();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(enc: Encoding, tolerance: f32) {
        let src: Vec<f32> = (0..64).map(|i| ((i as f32) / 31.5 - 1.0) * 0.99).collect();
        let width = match enc {
            Encoding::F32 | Encoding::I32 => 4,
            Encoding::I24 => 3,
            Encoding::I16 => 2,
        };
        let mut bytes = vec![0u8; src.len() * width];
        encode(&src, &mut bytes, enc);
        let mut back = vec![0.0f32; src.len()];
        decode(&bytes, &mut back, enc);
        for (a, b) in src.iter().zip(&back) {
            assert!((a - b).abs() <= tolerance, "{enc:?}: {a} came back {b}");
        }
    }

    #[test]
    fn what_is_written_to_a_device_reads_back_from_one_in_every_format() {
        roundtrip(Encoding::F32, 0.0);
        roundtrip(Encoding::I32, 1e-6);
        roundtrip(Encoding::I24, 2e-7);
        roundtrip(Encoding::I16, 1e-4);
    }

    #[test]
    fn a_negative_24_bit_sample_keeps_its_sign() {
        let mut out = [0.0f32; 2];
        // -1 and the most negative value, in three little-endian bytes each.
        decode(
            &[0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x80],
            &mut out,
            Encoding::I24,
        );
        assert!((out[0] + 1.0 / 8_388_608.0).abs() < 1e-12);
        assert_eq!(out[1], -1.0);
    }

    #[test]
    fn captured_frames_reach_the_ring_whole_and_silence_is_silence() {
        let (mut tx, mut rx) = RingBuffer::<f32>::new(8);
        let mut scratch = vec![0.0f32; 6];
        let samples = [0.5f32, -0.5, 0.25, -0.25, 0.125, -0.125];
        let mut bytes = vec![0u8; samples.len() * 2];
        encode(&samples, &mut bytes, Encoding::I16);
        let n = push_frames(&bytes, Encoding::I16, 2, false, &mut scratch, &mut tx);
        assert_eq!(n, 3);
        let got: Vec<f32> = std::iter::from_fn(|| rx.pop().ok()).collect();
        assert_eq!(got.len(), 6);
        assert!((got[0] - 0.5).abs() < 1e-4 && (got[5] + 0.125).abs() < 1e-4);

        // The ring holds 8 samples: of three frames of 3 channels only two fit.
        let mut scratch = vec![0.0f32; 9];
        let mut bytes = vec![0u8; 9 * 4];
        encode(&[0.1f32; 9], &mut bytes, Encoding::F32);
        let n = push_frames(&bytes, Encoding::F32, 3, false, &mut scratch, &mut tx);
        assert_eq!(n, 2, "a frame that does not fit whole is dropped");

        let (mut tx, mut rx) = RingBuffer::<f32>::new(8);
        let mut scratch = vec![1.0f32; 4];
        let n = push_frames(&[9u8; 16], Encoding::F32, 2, true, &mut scratch, &mut tx);
        assert_eq!(n, 2);
        assert!(std::iter::from_fn(|| rx.pop().ok()).all(|s| s == 0.0));
    }
}
