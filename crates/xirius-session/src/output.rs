//! Where a playing engine's sound goes: the sound device, or nowhere (tests and machines
//! without one). Both pull the engine on a thread of their own and can be held by a session
//! that moves between threads. Stopping one gives the engine back to the thread that stops it,
//! where it is dropped: a plugin made in the process is released on the thread that made it,
//! not on the output's (ARC-044).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use xirius_engine::AudioEngine;
use xirius_engine::device::{CpalOutput, Lost};
use xirius_engine::record::{AudioInput, CpalInput};

#[cfg(test)]
mod background_tests;

/// Frames per pull of the silent output.
const CHUNK: usize = 480;

/// Pulls the engine on a thread of its own and throws the sound away.
pub struct SilentOutput {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<AudioEngine>>,
    /// Never set by the output itself: a test sets it to stand for a device that stopped.
    lost: Lost,
}

impl SilentOutput {
    /// `paced` follows the clock like a device; otherwise it runs as fast as it can while the
    /// transport plays (and rests while it does not).
    pub fn start(mut audio: AudioEngine, paced: bool) -> SilentOutput {
        audio.set_silent(true);
        let period = Duration::from_secs_f64(CHUNK as f64 / audio.setup().sample_rate);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = thread::spawn(move || {
            let mut buf = vec![0.0f32; CHUNK * 2];
            let mut due = Instant::now();
            while !flag.load(Ordering::Relaxed) {
                audio.render(&mut buf, 2);
                if paced {
                    due += period;
                    let now = Instant::now();
                    if due > now {
                        thread::sleep(due - now);
                    } else if now - due > period * 20 {
                        due = now;
                    }
                } else if !audio.is_playing() {
                    thread::sleep(Duration::from_millis(1));
                }
            }
            audio
        });
        SilentOutput {
            stop,
            thread: Some(thread),
            lost: Lost::default(),
        }
    }

    /// The handle [`Output::lost`] reads; setting it stands for a device that stopped.
    pub fn lost(&self) -> Lost {
        self.lost.clone()
    }
}

impl Drop for SilentOutput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// What the shared-mode output's stream is to [`DeviceOutput`]: a name for the screen, the
/// `lost` flag, and `stop` giving the engine back. `CpalOutput` is the real one; a test puts
/// in a stand-in that opens no device (ARC-044).
pub trait SharedStream: Send {
    fn name(&self) -> String;
    fn lost(&self) -> Lost;
    /// Stops the sound and gives the engine back (`None` when it was lost with the stream).
    fn stop(self: Box<Self>) -> Option<AudioEngine>;
}

impl SharedStream for CpalOutput {
    fn name(&self) -> String {
        format!(
            "{}（{} Hz、{} ch）",
            self.info.name, self.info.sample_rate, self.info.channels
        )
    }
    fn lost(&self) -> Lost {
        CpalOutput::lost(self)
    }
    fn stop(self: Box<Self>) -> Option<AudioEngine> {
        CpalOutput::stop(*self)
    }
}

/// The sound device. The device stream may not leave the thread that opened it, so a thread
/// of its own opens it, keeps it and closes it when this is dropped.
pub struct DeviceOutput {
    /// What the device said it is, e.g. `Speakers（48000 Hz、2 ch）`.
    pub name: String,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<Option<AudioEngine>>>,
    lost: Lost,
}

impl DeviceOutput {
    pub fn start(audio: AudioEngine) -> Result<DeviceOutput, String> {
        Self::start_named(audio, String::new(), None)
    }
    pub fn start_named(
        audio: AudioEngine,
        device: String,
        buffer: Option<u32>,
    ) -> Result<DeviceOutput, String> {
        Self::open_with(audio, move |audio| {
            CpalOutput::start_keeping(audio, &device, buffer)
                .map(|o| Box::new(o) as Box<dyn SharedStream>)
                .map_err(|(e, audio)| Box::new((e.to_string(), audio.map(|a| *a))))
        })
    }

    /// The same thread arrangement as [`DeviceOutput::start_named`], with `open` in place of
    /// `CpalOutput::start_keeping`: a stand-in opens no device (ARC-044's device path tests).
    #[doc(hidden)]
    pub fn open_with(
        audio: AudioEngine,
        open: impl FnOnce(
            AudioEngine,
        ) -> Result<Box<dyn SharedStream>, Box<(String, Option<AudioEngine>)>>
        + Send
        + 'static,
    ) -> Result<DeviceOutput, String> {
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(String, Lost), String>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let thread = thread::spawn(move || match open(audio) {
            Ok(o) => {
                let _ = ready_tx.send(Ok((o.name(), o.lost())));
                // Dropping the sender (or sending) ends the wait.
                let _ = stop_rx.recv();
                o.stop()
            }
            Err(e) => {
                let (e, audio) = *e;
                let _ = ready_tx.send(Err(e));
                audio
            }
        });
        match ready_rx.recv() {
            Ok(Ok((name, lost))) => Ok(DeviceOutput {
                name,
                stop: Some(stop_tx),
                thread: Some(thread),
                lost,
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err("オーディオ出力のスレッドが停止しました".into())
            }
        }
    }
}

impl Drop for DeviceOutput {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Where a session's player sends its sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputMode {
    /// No player: the transport commands are refused.
    #[default]
    None,
    /// A silent output (tests, machines without a device).
    Silent { paced: bool },
    /// The default sound device.
    Device,
}

/// A started output of either kind.
pub enum Output {
    Silent(SilentOutput),
    Device(DeviceOutput),
    #[cfg(windows)]
    Exclusive(xirius_engine::exclusive::ExclusiveOutput),
}

/// An exclusive-mode output that could not be opened, with the engine it was to play, if it
/// came back.
pub struct Refused {
    pub error: String,
    pub audio: Option<Box<AudioEngine>>,
}

/// An output opened on `name`, with a buffer of about `buffer` frames.
type OpenOutput<E> =
    Arc<dyn Fn(AudioEngine, String, Option<u32>) -> Result<Output, E> + Send + Sync>;
type OpenInput = Arc<dyn Fn(u32, String, Option<u32>) -> Result<DeviceInput, String> + Send + Sync>;
/// The devices there are now, to choose from when the one chosen is gone (ENG-061).
type ListDevices = Arc<dyn Fn() -> Vec<xirius_engine::devices::Device> + Send + Sync>;

/// How the sound device is opened, per driver (ENG-027). The system's ways are the default;
/// a test puts in stand-ins, so no device is opened.
#[derive(Clone)]
pub struct Drivers {
    /// `None` where there is no exclusive mode (anywhere but Windows).
    pub exclusive_out: Option<OpenOutput<Refused>>,
    pub shared_out: OpenOutput<String>,
    pub exclusive_in: Option<OpenInput>,
    pub shared_in: OpenInput,
    pub devices: ListDevices,
}

impl Default for Drivers {
    fn default() -> Drivers {
        Drivers::system()
    }
}

impl Drivers {
    pub fn system() -> Drivers {
        Drivers {
            #[cfg(windows)]
            exclusive_out: Some(Arc::new(|audio, device, buffer| {
                match xirius_engine::exclusive::ExclusiveOutput::start_keeping(
                    audio, &device, buffer,
                ) {
                    Ok(o) => Ok(Output::Exclusive(o)),
                    Err(n) => Err(Refused {
                        error: n.error.to_string(),
                        audio: n.audio,
                    }),
                }
            })),
            #[cfg(not(windows))]
            exclusive_out: None,
            shared_out: Arc::new(|audio, device, buffer| {
                DeviceOutput::start_named(audio, device, buffer).map(Output::Device)
            }),
            #[cfg(windows)]
            exclusive_in: Some(Arc::new(|rate, device, buffer| {
                xirius_engine::exclusive::ExclusiveInput::open_named(rate, &device, buffer)
                    .map(DeviceInput::exclusive)
            })),
            #[cfg(not(windows))]
            exclusive_in: None,
            shared_in: Arc::new(|rate, device, buffer| {
                CpalInput::open_named(rate, &device, buffer).map(DeviceInput::shared)
            }),
            devices: Arc::new(|| xirius_engine::devices::Unopened::enumerate().devices),
        }
    }
}

/// An output that was started, and what to say about how.
pub struct Opened {
    pub output: Option<Output>,
    /// One line, when the driver asked for could not be had and another was used.
    pub notice: Option<String>,
}

/// The first line of `e`, for a notice that is one line.
fn first_line(e: &str) -> &str {
    e.lines().next().unwrap_or("").trim()
}

/// Why a device could not be had, in one line: said plainly when another program holds it.
pub fn reason(error: &str) -> String {
    match xirius_engine::device::in_use(error) {
        true => "ほかのアプリが使用中です".into(),
        false => first_line(error).to_string(),
    }
}

/// The notice for an exclusive-mode output or input that fell back to shared mode.
pub fn fallback_notice(what: &str, error: &str) -> String {
    format!(
        "WASAPI 排他で{what}を開けなかったため共有モードで使います（{}）",
        reason(error)
    )
}

/// What the screen says while the sound device cannot be had: it would not open, or it stopped
/// by itself.
pub fn unavailable(error: &str) -> String {
    format!("オーディオデバイスを使用できません（{}）", reason(error))
}

/// [`unavailable`] for the input device, which records and monitors: the output may still play.
pub fn input_unavailable(error: &str) -> String {
    format!("{INPUT_UNAVAILABLE}（{}）", reason(error))
}

const INPUT_UNAVAILABLE: &str = "オーディオ入力を使用できません";

/// Whether `said` is what [`input_unavailable`] says: the input device would not open or
/// stopped, rather than something else stopping a recording.
pub fn is_input_unavailable(said: &str) -> bool {
    said.starts_with(INPUT_UNAVAILABLE)
}

impl Output {
    /// Opens the output without waiting for a driver on the calling thread.
    /// An abandoned result is stopped and destroyed by the worker, even when
    /// cancellation races the successful open (PRJ-030).
    pub fn open_background(
        mode: OutputMode,
        audio: AudioEngine,
        prefs: xirius_prefs::Prefs,
        drivers: Drivers,
    ) -> Result<crate::background::Job<Opened>, String> {
        crate::background::Job::spawn("output-open", move || {
            Self::open(mode, audio, &prefs, &drivers)
        })
    }

    /// The caller keeps the ticket until cleanup acknowledges completion. Dropping
    /// that ticket abandons the notification, never the cleanup itself.
    pub fn retire(self) -> Result<crate::background::Job<()>, String> {
        crate::background::Job::spawn("output-retire", move || {
            drop(self);
            Ok(())
        })
    }

    /// [`Output::open`] with the system's drivers; a fallback to shared mode is said on stderr.
    pub fn configured(
        mode: OutputMode,
        audio: AudioEngine,
        prefs: &xirius_prefs::Prefs,
    ) -> Result<Option<Output>, String> {
        let opened = Self::open(mode, audio, prefs, &Drivers::system())?;
        if let Some(n) = &opened.notice {
            eprintln!("注意: {n}");
        }
        Ok(opened.output)
    }

    /// Starts `mode` for `audio`. A device is opened by the driver of the `audio.driver`
    /// setting: exclusive mode where the system has it, and when the device will not open that
    /// way, shared mode with a notice that says so. Exclusive mode asks for the `audio.buffer`
    /// setting; in shared mode the Windows audio engine sets the buffer, so none is asked for.
    pub fn open(
        mode: OutputMode,
        audio: AudioEngine,
        prefs: &xirius_prefs::Prefs,
        drivers: &Drivers,
    ) -> Result<Opened, String> {
        if mode != OutputMode::Device {
            return Ok(Opened {
                output: Self::start(mode, audio)?,
                notice: None,
            });
        }
        // The same interface the inputs are taken from, chosen again once unplugged (ENG-061).
        let device = crate::audio_settings::configured_device(prefs, &(drivers.devices)());
        let buffer = Some(prefs.number("audio.buffer") as u32);
        let (audio, notice) = match (&drivers.exclusive_out, prefs.text("audio.driver").as_str()) {
            (Some(open), "wasapi-exclusive") => match open(audio, device.clone(), buffer) {
                Ok(o) => {
                    return Ok(Opened {
                        output: Some(o),
                        notice: None,
                    });
                }
                Err(Refused {
                    error,
                    audio: Some(audio),
                }) => (*audio, Some(fallback_notice("出力", &error))),
                Err(Refused { error, audio: None }) => return Err(error),
            },
            (_, _) => (audio, None),
        };
        let output = (drivers.shared_out)(audio, device, None)?;
        Ok(Opened {
            output: Some(output),
            notice,
        })
    }

    /// Starts `mode` for `audio`. `None` for [`OutputMode::None`].
    pub fn start(mode: OutputMode, audio: AudioEngine) -> Result<Option<Output>, String> {
        Ok(match mode {
            OutputMode::None => None,
            OutputMode::Silent { paced } => Some(Output::Silent(SilentOutput::start(audio, paced))),
            OutputMode::Device => Some(Output::Device(DeviceOutput::start(audio)?)),
        })
    }

    /// Why the device stopped by itself, once it has; `None` while it plays.
    pub fn lost(&self) -> Option<String> {
        match self {
            Output::Silent(s) => s.lost.get(),
            Output::Device(d) => d.lost.get(),
            #[cfg(windows)]
            Output::Exclusive(d) => d.lost().get(),
        }
    }

    /// For messages: `無音` or the device.
    pub fn describe(&self) -> String {
        match self {
            Output::Silent(_) => "無音".into(),
            Output::Device(d) => d.name.clone(),
            #[cfg(windows)]
            Output::Exclusive(d) => format!("{}（排他）", d.info.device),
        }
    }
}

// ---- the input ---------------------------------------------------------------------------------

/// The sound device's input, opened by whichever driver; the engine reads it like any input.
pub struct DeviceInput {
    pub name: String,
    pub exclusive: bool,
    probe: Arc<std::sync::atomic::AtomicU64>,
    input: Box<dyn AudioInput>,
    lost: Lost,
}

impl DeviceInput {
    pub fn new(
        name: String,
        exclusive: bool,
        probe: Arc<std::sync::atomic::AtomicU64>,
        input: Box<dyn AudioInput>,
    ) -> DeviceInput {
        DeviceInput {
            name,
            exclusive,
            probe,
            input,
            lost: Lost::default(),
        }
    }

    pub fn shared(c: CpalInput) -> DeviceInput {
        let (name, probe, lost) = (c.name.clone(), c.latency_probe(), c.lost());
        DeviceInput {
            lost,
            ..DeviceInput::new(name, false, probe, Box::new(c))
        }
    }

    #[cfg(windows)]
    pub fn exclusive(c: xirius_engine::exclusive::ExclusiveInput) -> DeviceInput {
        let (name, probe, lost) = (c.info.device.clone(), c.latency_probe(), c.lost());
        DeviceInput {
            lost,
            ..DeviceInput::new(name, true, probe, Box::new(c))
        }
    }

    /// Says why the device stopped by itself, once it has: what it no longer delivers reads as
    /// silence. A stand-in's is set by its test.
    pub fn lost(&self) -> Lost {
        self.lost.clone()
    }

    /// A handle that reads the device's input latency, once it reports it.
    pub fn latency_probe(&self) -> Arc<std::sync::atomic::AtomicU64> {
        self.probe.clone()
    }
}

impl AudioInput for DeviceInput {
    fn read(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.input.read(left, right);
    }
    fn channels(&self) -> usize {
        self.input.channels()
    }
    fn read_channels(&mut self, buf: &mut [f32], frames: usize) {
        self.input.read_channels(buf, frames);
    }
    fn delivered(&self, frames: usize) -> usize {
        self.input.delivered(frames)
    }
}

/// Opens the input device by the `audio.driver` setting, as [`Output::open`] does the output:
/// exclusive where the system has it, and shared with a notice when the device will not. The
/// buffer is asked for in exclusive mode only, as for the output.
pub fn open_input(
    rate: u32,
    prefs: &xirius_prefs::Prefs,
    device: String,
    drivers: &Drivers,
) -> Result<(DeviceInput, Option<String>), String> {
    let buffer = Some(prefs.number("audio.buffer") as u32);
    let mut notice = None;
    if let (Some(open), "wasapi-exclusive") =
        (&drivers.exclusive_in, prefs.text("audio.driver").as_str())
    {
        match open(rate, device.clone(), buffer) {
            Ok(i) => return Ok((i, None)),
            Err(e) => notice = Some(fallback_notice("入力", &e)),
        }
    }
    (drivers.shared_in)(rate, device, None).map(|i| (i, notice))
}
